fn main() {
    // Only the desktop shell needs Tauri's codegen; the domain library (what
    // the fuzz targets link) builds with --no-default-features.
    if std::env::var("CARGO_FEATURE_DESKTOP").is_ok() {
        tauri_build::build()
    }
}
