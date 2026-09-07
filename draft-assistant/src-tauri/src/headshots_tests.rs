//! The disk cache's own tests, in a file of their own.
//!
//! Split out of `headshots.rs` only because that file is at the repo's
//! 500-line cap; they are the same tests, and `use super::*` still reads the
//! module they belong to.

use super::*;

#[test]
fn only_numeric_ids_are_players() {
    assert!(is_player_id("11560"));
    assert!(!is_player_id("DET"));
    assert!(!is_player_id(""));
    assert!(!is_player_id("../etc/passwd"));
}

/// A file whose mtime is in the future — a clock that was wrong when it
/// was written, or has since been set back — used to age at zero
/// forever, so a broken image stayed "fresh" and was never refetched.
#[test]
fn a_file_stamped_in_the_future_has_no_age_rather_than_an_age_of_zero() {
    let dir = std::env::temp_dir().join(format!(
        "draft-assistant-headshot-age-{}-{}",
        std::process::id(),
        now_secs()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("11560.png");
    std::fs::write(&path, b"bytes").unwrap();
    assert!(age_secs(&path).is_some_and(|age| age < 5));

    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(86_400))
        .unwrap();
    assert_eq!(age_secs(&path), None, "a future mtime is a miss");
    assert_eq!(age_secs(&dir.join("absent.png")), None);
    std::fs::remove_dir_all(dir).unwrap();
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
    store_on_disk(&image, &miss, &Fetched::NoPicture);
    assert!(miss.exists());
    assert!(!image.exists());

    // The rookie's photo lands. The miss has to go with it, or he stays
    // faceless until it expires.
    let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    store_on_disk(&image, &miss, &Fetched::Picture(png.to_vec()));
    assert_eq!(std::fs::read(&image).unwrap(), png);
    assert!(!miss.exists());
    // Written through a temp file, so a crash mid-write cannot leave half
    // an image behind for the next month.
    assert!(temp_files_in(&heads).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

fn temp_files_in(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .expect("the headshot directory is readable")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "tmp"))
        .collect()
}

/// Two fetches of the same headshot used to share one `<id>.img.tmp`:
/// they interleaved into it and each renamed the mixture over the
/// picture, so the board showed a broken image until the entry expired.
#[test]
fn two_writers_of_one_headshot_leave_a_whole_image() {
    let dir = image_dir("store-concurrent");
    let heads = dir.join("headshots");
    let image = heads.join("11560.img");
    let miss = heads.join("11560.none");
    std::fs::create_dir_all(&heads).unwrap();

    let mut png_a = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    png_a.extend(std::iter::repeat_n(1u8, 200_000));
    let mut png_b = png_a[..8].to_vec();
    png_b.extend(std::iter::repeat_n(2u8, 200_000));

    let handles: Vec<_> = [png_a.clone(), png_b.clone()]
        .into_iter()
        .map(|bytes| {
            let (image, miss) = (image.clone(), miss.clone());
            let fetched = Fetched::Picture(bytes);
            std::thread::spawn(move || {
                for _ in 0..10 {
                    store_on_disk(&image, &miss, &fetched);
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("the writer finished");
    }

    let written = std::fs::read(&image).expect("an image is in place");
    assert!(written == png_a || written == png_b, "torn image");
    assert!(temp_files_in(&heads).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

/// Only the CDN's own "no such picture" is a miss. A 500, a 429 or a 503
/// says nothing about the player, and used to be filed as "no photo" for
/// three days.
#[test]
fn only_a_404_or_a_410_means_there_is_no_picture() {
    let png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    assert!(matches!(classify(200, png.clone()), Fetched::Picture(_)));
    // A 200 whose body is the CDN's HTML "not found" page is still no picture.
    assert!(matches!(
        classify(200, b"<html>not found</html>".to_vec()),
        Fetched::NoPicture
    ));
    assert!(matches!(classify(404, Vec::new()), Fetched::NoPicture));
    assert!(matches!(classify(410, Vec::new()), Fetched::NoPicture));
    for transient in [403, 429, 500, 502, 503, 504] {
        assert!(
            matches!(classify(transient, Vec::new()), Fetched::Unavailable(s) if s == transient),
            "{transient} was not treated as transient"
        );
    }
}

#[test]
fn a_transient_failure_writes_nothing_to_disk() {
    let dir = image_dir("transient-store");
    let heads = dir.join("headshots");
    let image = heads.join("11560.img");
    let miss = heads.join("11560.none");
    std::fs::create_dir_all(&heads).unwrap();

    store_on_disk(&image, &miss, &Fetched::Unavailable(503));
    assert!(!miss.exists(), "a 503 was remembered as a missing photo");
    assert!(!image.exists());
    assert!(temp_files_in(&heads).is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

/// A one-shot HTTP server on localhost that answers every request with
/// `status` and `body`, for driving the fetch path end to end. Returns the
/// URL to ask.
fn serve_status(status: u16, body: &'static [u8]) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        for mut socket in listener.incoming().flatten() {
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match socket.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&chunk[..n]),
                }
            }
            let head = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(head.as_bytes());
            let _ = socket.write_all(body);
            let _ = socket.flush();
        }
    });
    format!("http://{addr}/11560.jpg")
}

/// The whole path: a CDN that is having a bad minute must not leave a
/// `.none` behind, so the next render asks again and gets the photo.
#[tokio::test]
async fn a_cdn_error_is_retried_next_time_rather_than_remembered_as_no_photo() {
    let dir = image_dir("transient-fetch");
    let engine = Engine::with_client(
        dir.clone(),
        crate::sleeper::SleeperClient::with_host("http://127.0.0.1:9"),
    );
    let heads = dir.join("headshots");

    let broken = serve_status(503, b"");
    assert_eq!(engine.cached_image("11560", &broken).await.unwrap(), None);
    assert!(
        !heads.join("11560.none").exists(),
        "a 503 from the CDN was written down as a three-day miss"
    );

    // The CDN is back: the same key fetches the photo straight away.
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let working = serve_status(200, PNG);
    let served = engine
        .cached_image("11560", &working)
        .await
        .unwrap()
        .expect("the photo, now that the CDN answers");
    assert!(served.starts_with("data:image/png;base64,"), "{served}");
    assert_eq!(engine.headshot_count(), 1);

    // Whereas a 404 is the CDN's word, and is remembered.
    let missing = serve_status(404, b"");
    assert_eq!(engine.cached_image("99999", &missing).await.unwrap(), None);
    assert!(
        heads.join("99999.none").exists(),
        "a 404 should be remembered"
    );
    std::fs::remove_dir_all(&dir).ok();
}
