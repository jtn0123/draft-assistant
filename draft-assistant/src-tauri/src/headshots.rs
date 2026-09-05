//! Pictures from Sleeper's CDN — player headshots and team avatars — fetched
//! once and kept on disk.
//!
//! Every board row wants a photo, and the same few hundred faces show up all
//! season, so each is downloaded a single time into `headshots/` in the app
//! data dir and served back as a data URL. One with no picture is remembered
//! as such (`<key>.none`) so the miss is never retried on every render.

use crate::engine::{now_secs, Engine};
use std::path::{Path, PathBuf};
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
fn is_player_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 12 && id.bytes().all(|b| b.is_ascii_digit())
}

/// Where an avatar reference points, and what to file it under. Sleeper gives
/// either a bare hash (the account picture) or a full uploads URL (a custom
/// team picture); anything else is refused rather than fetched.
fn avatar_target(reference: &str, full: bool) -> Option<(String, String)> {
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
fn mime_of(bytes: &[u8]) -> Option<&'static str> {
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
fn base64(bytes: &[u8]) -> String {
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

fn data_url(bytes: &[u8]) -> Option<String> {
    let mime = mime_of(bytes)?;
    Some(format!("data:{mime};base64,{}", base64(bytes)))
}

fn age_secs(path: &Path) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let at = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let now = now_secs();
    // A file stamped in the future never ages: `saturating_sub` floors it at
    // zero, so it would read as freshly fetched for as long as it sat there
    // and no refresh would ever happen. The cache envelope treats such a
    // timestamp as a miss (`cache::fresh_enough`) and so does this.
    (at <= now).then(|| now - at)
}

/// What the cache on disk had to say about one image.
enum Cached {
    /// A picture, fresh enough to serve as it stands.
    Image(Vec<u8>),
    /// A remembered "Sleeper has no picture for this", still fresh.
    KnownMissing,
    /// Nothing usable — go and fetch it.
    Nothing,
}

/// Make sure the cache directory exists and read whatever is already in it.
///
/// Every call in here blocks, which is why it runs on the blocking pool: a
/// roster render asks for dozens of images at once, and doing this on a
/// runtime thread stalls every other task for the length of all of them.
fn look_on_disk(dir: &Path, image: &Path, miss: &Path) -> Result<Cached, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("headshots dir: {e}"))?;
    crate::cache::owner_only_dir(dir);
    if age_secs(image).is_some_and(|age| age < FRESH_SECS) {
        if let Ok(bytes) = std::fs::read(image) {
            if mime_of(&bytes).is_some() {
                return Ok(Cached::Image(bytes));
            }
        }
    }
    if age_secs(miss).is_some_and(|age| age < MISS_SECS) {
        return Ok(Cached::KnownMissing);
    }
    Ok(Cached::Nothing)
}

/// Write down what the CDN gave us: the picture, or the fact that there is
/// none. Blocking, and run on the blocking pool for the same reason.
fn store_on_disk(image: &Path, miss: &Path, bytes: &[u8], usable: bool) {
    if !usable {
        std::fs::write(miss, b"").ok();
        return;
    }
    // One temp name per writer, the same way the JSON cache does it. With a
    // name derived only from the image, two fetches of the same headshot --
    // the board and the roster panel asking at once, or two windows of the app
    // -- wrote into the very same file and each renamed the interleaved result
    // over the picture, leaving a half-PNG that decodes to nothing.
    let tmp = crate::cache::temp_sibling(image);
    if crate::cache::write_synced(&tmp, bytes).is_ok() {
        std::fs::rename(&tmp, image).ok();
    } else {
        std::fs::remove_file(&tmp).ok();
    }
    std::fs::remove_file(miss).ok();
}

/// Sleeper's images, cached on disk.
///
/// A separate trait rather than more inherent methods on `Engine`: fetching
/// player photos has nothing to do with loading a league, and stating the seam
/// here means `Engine`'s real surface can be read off its trait list.
pub(crate) trait ImageCache {
    /// A player's photo as a data URL, or `None` if Sleeper has none.
    #[allow(async_fn_in_trait)]
    async fn headshot(&self, player_id: &str) -> Result<Option<String>, String>;
    /// A manager's team picture as a data URL. `full` asks for the large copy.
    #[allow(async_fn_in_trait)]
    async fn avatar(&self, reference: &str, full: bool) -> Result<Option<String>, String>;
    /// How many images are currently cached on disk. Only the cache's own
    /// tests count them; the Settings note reads the number off the view.
    #[cfg(test)]
    fn headshot_count(&self) -> usize;
}

impl Engine {
    fn headshot_dir(&self) -> PathBuf {
        self.data_dir.join("headshots")
    }

    async fn cached_image(&self, key: &str, url: &str) -> Result<Option<String>, String> {
        let dir = self.headshot_dir();
        let image = dir.join(format!("{key}.img"));
        let miss = dir.join(format!("{key}.none"));

        let looking = (dir, image.clone(), miss.clone());
        let found =
            tokio::task::spawn_blocking(move || look_on_disk(&looking.0, &looking.1, &looking.2))
                .await
                .map_err(|e| format!("headshot cache: {e}"))??;
        match found {
            Cached::Image(bytes) => return Ok(data_url(&bytes)),
            Cached::KnownMissing => return Ok(None),
            Cached::Nothing => {}
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

        let served = data_url(&bytes);
        let usable = served.is_some();
        tokio::task::spawn_blocking(move || store_on_disk(&image, &miss, &bytes, usable))
            .await
            .ok();
        Ok(served)
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

    /// How many photos are on disk.
    #[cfg(test)]
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
#[path = "headshots_tests.rs"]
mod tests;
