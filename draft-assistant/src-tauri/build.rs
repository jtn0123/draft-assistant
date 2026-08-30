use std::path::Path;

/// The WebdriverIO plugin's permission has to sit in a capability file, and a
/// capability naming a plugin that is not compiled in fails the build. So the
/// file exists exactly when `--features wdio` does, written here rather than
/// checked in — a shipped build cannot accidentally carry it.
fn sync_wdio_capability(enabled: bool) {
    let path = Path::new("capabilities/wdio.json");
    if enabled {
        let json = r#"{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "wdio",
  "description": "WebdriverIO's control channel. Written by build.rs only when --features wdio is on; never in a shipped build.",
  "windows": ["main"],
  "permissions": ["wdio:default"]
}
"#;
        let current = std::fs::read_to_string(path).unwrap_or_default();
        if current != json {
            std::fs::write(path, json).expect("write wdio capability");
        }
    } else if path.exists() {
        std::fs::remove_file(path).expect("remove wdio capability");
    }
}

fn main() {
    // Only the desktop shell needs Tauri's codegen; the domain library (what
    // the fuzz targets link) builds with --no-default-features.
    if std::env::var("CARGO_FEATURE_DESKTOP").is_ok() {
        sync_wdio_capability(std::env::var("CARGO_FEATURE_WDIO").is_ok());
        tauri_build::build()
    }
}
