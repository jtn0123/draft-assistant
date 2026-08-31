//! Pictures from Sleeper's CDN — player headshots and team avatars — fetched
//! once and kept on disk.
//!
//! Every board row wants a photo, and the same few hundred faces show up all
//! season, so each is downloaded a single time into `headshots/` in the app
//! data dir and served back as a data URL. One with no picture is remembered
//! as such (`<key>.none`) so the miss is never retried on every render.

use crate::engine::{now_secs, Engine};
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

const CDN: &str = "https://sleepercdn.com/content/nfl/players/thumb";
const AVATARS: &str = "https://sleepercdn.com/avatars/thumbs";
/// The same picture at 280px instead of 80px, for the zoomed view.
const AVATARS_FULL: &str = "https://sleepercdn.com/avatars";
/// Custom team pictures are uploaded here; nothing else is fetched.
const UPLOADS: &str = "https://sleepercdn.com/uploads/";
/// Photos change rarely (a new team, a new season); refresh monthly.
const FRESH_SECS: u64 = 30 * 24 * 3600;
/// Misses are re-checked sooner: a rookie's photo usually lands by week 1.
const MISS_SECS: u64 = 3 * 24 * 3600;

/// Sleeper ids are numeric; a team code ("DET") is a defence with no photo.
pub fn is_player_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 12 && id.bytes().all(|b| b.is_ascii_digit())
}

/// Where an avatar reference points, and what to file it under. Sleeper gives
/// either a bare hash (the account picture) or a full uploads URL (a custom
/// team picture); anything else is refused rather than fetched.
pub fn avatar_target(reference: &str, full: bool) -> Option<(String, String)> {
    let reference = reference.trim();
    let hex = |s: &str| !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
    let prefix = if full { "avf" } else { "av" };
    if hex(reference) {
        let base = if full { AVATARS_FULL } else { AVATARS };
        return Some((
            format!("{base}/{reference}"),
            format!("{prefix}-{reference}"),
        ));
    }
    // A custom upload is served at one size only, so both views share it.
    let name = reference.strip_prefix(UPLOADS)?;
    let stem = name.split('.').next().unwrap_or_default();
    hex(stem).then(|| (reference.to_string(), format!("av-{stem}")))
}

/// What the bytes are, from their magic number — the CDN says `.jpg` but
/// serves PNG, so the extension cannot be trusted.
pub fn mime_of(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

/// Standard base64, no dependencies.
pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub fn data_url(bytes: &[u8]) -> Option<String> {
    let mime = mime_of(bytes)?;
    Some(format!("data:{mime};base64,{}", base64(bytes)))
}

fn age_secs(path: &PathBuf) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let at = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(now_secs().saturating_sub(at))
}

/// Sleeper's images, cached on disk.
///
/// A separate trait rather than more inherent methods on `Engine`: fetching
/// player photos has nothing to do with loading a league, and stating the seam
/// here means `Engine`'s real surface can be read off its trait list.
pub trait ImageCache {
    /// A player's photo as a data URL, or `None` if Sleeper has none.
    #[allow(async_fn_in_trait)]
    async fn headshot(&self, player_id: &str) -> Result<Option<String>, String>;
    /// A manager's team picture as a data URL. `full` asks for the large copy.
    #[allow(async_fn_in_trait)]
    async fn avatar(&self, reference: &str, full: bool) -> Result<Option<String>, String>;
    /// How many images are currently cached on disk.
    fn headshot_count(&self) -> usize;
}

impl Engine {
    fn headshot_dir(&self) -> PathBuf {
        self.data_dir.join("headshots")
    }

    async fn cached_image(&self, key: &str, url: &str) -> Result<Option<String>, String> {
        let dir = self.headshot_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("headshots dir: {e}"))?;
        crate::cache::owner_only_dir(&dir);
        let image = dir.join(format!("{key}.img"));
        let miss = dir.join(format!("{key}.none"));

        if age_secs(&image).is_some_and(|age| age < FRESH_SECS) {
            if let Ok(bytes) = std::fs::read(&image) {
                if let Some(url) = data_url(&bytes) {
                    return Ok(Some(url));
                }
            }
        }
        if age_secs(&miss).is_some_and(|age| age < MISS_SECS) {
            return Ok(None);
        }

        let response = self
            .client
            .http_client()
            .get(url)
            .send()
            .await
            .map_err(|e| format!("headshot fetch: {e}"))?;
        let bytes = if response.status().is_success() {
            response
                .bytes()
                .await
                .map_err(|e| format!("headshot body: {e}"))?
                .to_vec()
        } else {
            Vec::new()
        };

        match data_url(&bytes) {
            Some(url) => {
                let tmp = dir.join(format!("{key}.img.tmp"));
                if std::fs::write(&tmp, &bytes).is_ok() {
                    std::fs::rename(&tmp, &image).ok();
                }
                std::fs::remove_file(&miss).ok();
                Ok(Some(url))
            }
            None => {
                std::fs::write(&miss, b"").ok();
                Ok(None)
            }
        }
    }
}

impl ImageCache for Engine {
    /// The player's photo as a data URL, or `None` when Sleeper has none.
    /// Hits the network only when nothing usable is on disk.
    async fn headshot(&self, player_id: &str) -> Result<Option<String>, String> {
        if !is_player_id(player_id) {
            return Ok(None);
        }
        self.cached_image(player_id, &format!("{CDN}/{player_id}.jpg"))
            .await
    }

    /// A manager's team picture, cached the same way. `reference` is whatever
    /// Sleeper handed us for the user; unrecognised shapes fetch nothing.
    async fn avatar(&self, reference: &str, full: bool) -> Result<Option<String>, String> {
        let Some((url, key)) = avatar_target(reference, full) else {
            return Ok(None);
        };
        self.cached_image(&key, &url).await
    }

    /// How many photos are on disk, for the Settings note.
    fn headshot_count(&self) -> usize {
        std::fs::read_dir(self.headshot_dir())
            .map(|d| {
                d.filter_map(Result::ok)
                    .filter(|e| e.path().extension().is_some_and(|x| x == "img"))
                    .count()
            })
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_numeric_ids_are_players() {
        assert!(is_player_id("11560"));
        assert!(!is_player_id("DET"));
        assert!(!is_player_id(""));
        assert!(!is_player_id("../etc/passwd"));
    }

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn the_mime_comes_from_the_bytes_not_the_extension() {
        let png = [0x89, b'P', b'N', b'G', 0, 0];
        assert_eq!(mime_of(&png), Some("image/png"));
        assert_eq!(mime_of(b"<html>not found"), None);
        assert!(data_url(&png)
            .unwrap()
            .starts_with("data:image/png;base64,iVBORw"));
    }

    #[test]
    fn an_avatar_reference_is_either_a_hash_or_an_uploads_url() {
        let (url, key) = avatar_target("93bf4ccf4ee12f405f5617b95b001ab4", false).unwrap();
        assert_eq!(
            url,
            "https://sleepercdn.com/avatars/thumbs/93bf4ccf4ee12f405f5617b95b001ab4"
        );
        assert_eq!(key, "av-93bf4ccf4ee12f405f5617b95b001ab4");

        let custom = "https://sleepercdn.com/uploads/c980da0b015929edf41c3b1182ab6b32.jpg";
        let (url, key) = avatar_target(custom, false).unwrap();
        assert_eq!(url, custom);
        assert_eq!(key, "av-c980da0b015929edf41c3b1182ab6b32");
    }

    #[test]
    fn the_zoomed_avatar_is_a_bigger_file_under_its_own_key() {
        let (url, key) = avatar_target("93bf4ccf4ee12f405f5617b95b001ab4", true).unwrap();
        assert_eq!(
            url,
            "https://sleepercdn.com/avatars/93bf4ccf4ee12f405f5617b95b001ab4"
        );
        assert_eq!(key, "avf-93bf4ccf4ee12f405f5617b95b001ab4");
    }

    #[test]
    fn anything_but_sleepers_own_cdn_is_refused() {
        assert!(avatar_target("https://evil.example/x.jpg", false).is_none());
        assert!(avatar_target("https://sleepercdn.com/uploads/../../etc/passwd", false).is_none());
        assert!(avatar_target("", false).is_none());
    }

    #[tokio::test]
    async fn a_defence_never_touches_the_network_or_disk() {
        let dir = std::env::temp_dir().join(format!("da-heads-{}", std::process::id()));
        let engine = Engine::new(dir.clone());
        assert_eq!(engine.headshot("DET").await.unwrap(), None);
        assert_eq!(engine.headshot_count(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
