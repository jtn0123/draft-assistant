/// The phone page, compiled in. The files are written by the page lane;
/// serving them from the binary rather than from disk is what keeps the app a
/// single bundle with nothing to install beside it.
pub const INDEX_HTML: &str = include_str!("../../companion-static/index.html");
pub const MODELS_JS: &str = include_str!("../../companion-static/models.js");
pub const MOBILE_MODELS_CSS: &str = include_str!("../../companion-static/mobile-models.css");
pub const DRAFT_MOBILE_CSS: &str = include_str!("../../companion-static/draft-mobile.css");
pub const BOOT_JS: &str = include_str!("../../companion-static/boot.js");
pub const HELPERS_JS: &str = include_str!("../../companion-static/helpers.js");
pub const CLOCK_JS: &str = include_str!("../../companion-static/clock.js");
pub const APP_JS: &str = include_str!("../../companion-static/app.js");
pub const APP_CSS: &str = include_str!("../../companion-static/app.css");
/// The Week tab, the best-available list, the your-turn nudge and their styles.
pub const WEEK_JS: &str = include_str!("../../companion-static/week.js");
pub const AVAILABLE_JS: &str = include_str!("../../companion-static/available.js");
pub const ALERTS_JS: &str = include_str!("../../companion-static/alerts.js");
pub const COMPACT_JS: &str = include_str!("../../companion-static/compact.js");
pub const PICTURES_JS: &str = include_str!("../../companion-static/pictures.js");
pub const RESTORE_JS: &str = include_str!("../../companion-static/restore.js");
pub const ROSTER_JS: &str = include_str!("../../companion-static/roster.js");
pub const SIGNALS_JS: &str = include_str!("../../companion-static/signals.js");
pub const EXTRAS_CSS: &str = include_str!("../../companion-static/extras.css");
/// The installed-app half: the manifest and icon that make the page
/// installable, the service worker the browser requires for it (it caches
/// nothing), and the script that asks for the screen wake lock.
pub const PWA_JS: &str = include_str!("../../companion-static/pwa.js");
pub const SW_JS: &str = include_str!("../../companion-static/sw.js");
pub const MANIFEST: &str = include_str!("../../companion-static/manifest.webmanifest");
pub const ICON_SVG: &str = include_str!("../../companion-static/icon.svg");
/// The 180x180 PNG iOS wants for a home-screen icon; it ignores the SVG.
pub const TOUCH_ICON_PNG: &[u8] = include_bytes!("../../companion-static/apple-touch-icon.png");

/// The static file behind a `/static/{file}` path, with its content type.
///
/// An allow-list of names rather than a directory read: there is no path
/// to traverse, so no request can ask this for anything the page is not.
pub fn static_file(name: &str) -> Option<(&'static str, &'static [u8])> {
    match name {
        "index.html" => Some(("text/html; charset=utf-8", INDEX_HTML.as_bytes())),
        "models.js" => Some(("text/javascript; charset=utf-8", MODELS_JS.as_bytes())),
        "mobile-models.css" => Some(("text/css; charset=utf-8", MOBILE_MODELS_CSS.as_bytes())),
        "draft-mobile.css" => Some(("text/css; charset=utf-8", DRAFT_MOBILE_CSS.as_bytes())),
        "boot.js" => Some(("text/javascript; charset=utf-8", BOOT_JS.as_bytes())),
        "helpers.js" => Some(("text/javascript; charset=utf-8", HELPERS_JS.as_bytes())),
        "clock.js" => Some(("text/javascript; charset=utf-8", CLOCK_JS.as_bytes())),
        "app.js" => Some(("text/javascript; charset=utf-8", APP_JS.as_bytes())),
        "app.css" => Some(("text/css; charset=utf-8", APP_CSS.as_bytes())),
        "week.js" => Some(("text/javascript; charset=utf-8", WEEK_JS.as_bytes())),
        "available.js" => Some(("text/javascript; charset=utf-8", AVAILABLE_JS.as_bytes())),
        "alerts.js" => Some(("text/javascript; charset=utf-8", ALERTS_JS.as_bytes())),
        "compact.js" => Some(("text/javascript; charset=utf-8", COMPACT_JS.as_bytes())),
        "pictures.js" => Some(("text/javascript; charset=utf-8", PICTURES_JS.as_bytes())),
        "restore.js" => Some(("text/javascript; charset=utf-8", RESTORE_JS.as_bytes())),
        "roster.js" => Some(("text/javascript; charset=utf-8", ROSTER_JS.as_bytes())),
        "signals.js" => Some(("text/javascript; charset=utf-8", SIGNALS_JS.as_bytes())),
        "extras.css" => Some(("text/css; charset=utf-8", EXTRAS_CSS.as_bytes())),
        "pwa.js" => Some(("text/javascript; charset=utf-8", PWA_JS.as_bytes())),
        "sw.js" => Some(("text/javascript; charset=utf-8", SW_JS.as_bytes())),
        "manifest.webmanifest" => Some(("application/manifest+json", MANIFEST.as_bytes())),
        "icon.svg" => Some(("image/svg+xml", ICON_SVG.as_bytes())),
        "apple-touch-icon.png" => Some(("image/png", TOUCH_ICON_PNG)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::static_file;

    #[test]
    fn only_the_page_files_are_served() {
        for name in [
            "index.html",
            "models.js",
            "mobile-models.css",
            "draft-mobile.css",
            "boot.js",
            "helpers.js",
            "clock.js",
            "pwa.js",
            "app.js",
            "app.css",
            "week.js",
            "available.js",
            "alerts.js",
            "compact.js",
            "pictures.js",
            "restore.js",
            "roster.js",
            "signals.js",
            "extras.css",
            "sw.js",
            "manifest.webmanifest",
            "icon.svg",
            "apple-touch-icon.png",
        ] {
            let (mime, body) = static_file(name).expect("{name} is served");
            assert!(!mime.is_empty());
            assert!(!body.is_empty(), "{name} is empty");
        }
        // No directory read behind this, so nothing to traverse out of.
        assert!(static_file("../../src/engine.rs").is_none());
        assert!(static_file("config.json").is_none());
        assert!(static_file("").is_none());
    }
}
