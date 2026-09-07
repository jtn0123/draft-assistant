// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// The app is the desktop shell, so without that feature there is nothing for
// this binary to start. `--no-default-features` is how the fuzz targets take
// the library alone; they never build this.
#[cfg(feature = "desktop")]
fn main() {
    draft_assistant_lib::run()
}

#[cfg(not(feature = "desktop"))]
fn main() {
    eprintln!("built without the `desktop` feature: there is no app to run");
    std::process::exit(1);
}
