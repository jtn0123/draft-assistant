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
    Some(now_secs().saturating_sub(at))
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
    let tmp = image.with_extension("img.tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        std::fs::rename(&tmp, image).ok();
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

    fn image_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "da-heads-{label}-{}-{}",
            std::process::id(),
            now_secs()
        ))
    }

    /// The cache lookup now happens on the blocking pool, so this pins down
    /// that it still finds what is already there and never reaches the CDN.
    #[tokio::test]
    async fn a_picture_already_on_disk_is_served_without_the_network() {
        let dir = image_dir("hit");
        let engine = Engine::new(dir.clone());
        let heads = dir.join("headshots");
        std::fs::create_dir_all(&heads).unwrap();
        std::fs::write(
            heads.join("11560.img"),
            [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        )
        .unwrap();

        let served = engine
            .headshot("11560")
            .await
            .unwrap()
            .expect("the copy on disk");
        assert!(served.starts_with("data:image/png;base64,"), "{served}");
        assert_eq!(engine.headshot_count(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A player Sleeper has no picture for is remembered as such, so the miss
    /// is not re-fetched on every render.
    #[tokio::test]
    async fn a_remembered_miss_answers_without_the_network() {
        let dir = image_dir("miss");
        let engine = Engine::new(dir.clone());
        let heads = dir.join("headshots");
        std::fs::create_dir_all(&heads).unwrap();
        std::fs::write(heads.join("11560.none"), b"").unwrap();

        assert_eq!(engine.headshot("11560").await.unwrap(), None);
        assert_eq!(engine.headshot_count(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn a_defence_never_touches_the_network_or_disk() {
        let dir = std::env::temp_dir().join(format!("da-heads-{}", std::process::id()));
        let engine = Engine::new(dir.clone());
        assert_eq!(engine.headshot("DET").await.unwrap(), None);
        assert_eq!(engine.headshot_count(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Age one file backwards so the freshness checks can be exercised
    /// without a test that sleeps for a month.
    fn age_file(path: &Path, seconds: u64) {
        let file = std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open to re-stamp");
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds);
        file.set_modified(when).expect("re-stamp");
    }

    #[test]
    fn a_month_old_photo_is_re_fetched_rather_than_served_forever() {
        let dir = image_dir("stale");
        let heads = dir.join("headshots");
        let image = heads.join("11560.img");
        let miss = heads.join("11560.none");
        std::fs::create_dir_all(&heads).unwrap();
        std::fs::write(&image, [0x89, b'P', b'N', b'G', 0, 0, 0, 0]).unwrap();

        // Inside the window it is served as it stands.
        assert!(matches!(
            look_on_disk(&heads, &image, &miss).unwrap(),
            Cached::Image(_)
        ));
        // Past it, the photo may be out of date -- a trade, a new season -- so
        // the cache stops answering and the CDN is asked again.
        age_file(&image, FRESH_SECS + 60);
        assert!(matches!(
            look_on_disk(&heads, &image, &miss).unwrap(),
            Cached::Nothing
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_remembered_miss_expires_sooner_than_a_photo_does() {
        let dir = image_dir("miss-expiry");
        let heads = dir.join("headshots");
        let image = heads.join("11560.img");
        let miss = heads.join("11560.none");
        std::fs::create_dir_all(&heads).unwrap();
        std::fs::write(&miss, b"").unwrap();

        assert!(matches!(
            look_on_disk(&heads, &image, &miss).unwrap(),
            Cached::KnownMissing
        ));
        // A rookie's photo usually lands by week 1, so the miss is re-checked
        // long before a real photo would be.
        age_file(&miss, MISS_SECS + 60);
        assert!(matches!(
            look_on_disk(&heads, &image, &miss).unwrap(),
            Cached::Nothing
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_that_is_not_an_image_is_ignored_rather_than_served() {
        // The CDN answers a missing photo with an HTML error page. Serving
        // that back as a data URL puts a broken image in every roster row.
        let dir = image_dir("corrupt");
        let heads = dir.join("headshots");
        let image = heads.join("11560.img");
        let miss = heads.join("11560.none");
        std::fs::create_dir_all(&heads).unwrap();
        std::fs::write(&image, b"<html>404</html>").unwrap();

        assert!(matches!(
            look_on_disk(&heads, &image, &miss).unwrap(),
            Cached::Nothing
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_photo_that_arrives_replaces_the_miss_that_was_remembered() {
        let dir = image_dir("store");
        let heads = dir.join("headshots");
        let image = heads.join("11560.img");
        let miss = heads.join("11560.none");
        std::fs::create_dir_all(&heads).unwrap();

        // No photo: the miss is written down so the next render does not
        // fetch it all over again.
        store_on_disk(&image, &miss, b"<html>404</html>", false);
        assert!(miss.exists());
        assert!(!image.exists());

        // The rookie's photo lands. The miss has to go with it, or he stays
        // faceless until it expires.
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        store_on_disk(&image, &miss, &png, true);
        assert_eq!(std::fs::read(&image).unwrap(), png);
        assert!(!miss.exists());
        // Written through a temp file, so a crash mid-write cannot leave half
        // an image behind for the next month.
        assert!(!image.with_extension("img.tmp").exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
