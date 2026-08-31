fn main() {
    // Which capability files get compiled into the app depends on the `wdio`
    // feature. A capability is a static grant: whatever is in
    // `capabilities/` at build time is baked into the binary's ACL and cannot
    // be revoked at runtime. `capabilities/wdio.json` grants the in-app
    // WebDriver server permission to drive this window, so a default build
    // must not see that file at all -- otherwise the grant would ship even
    // though the plugin behind it did not, and re-adding the plugin later
    // would silently re-arm it.
    //
    // Cargo sets CARGO_FEATURE_WDIO for this build script exactly when the
    // feature is on, so the two cannot drift: the permission and the plugin
    // are switched by the same flag.
    let capabilities = if std::env::var_os("CARGO_FEATURE_WDIO").is_some() {
        "./capabilities/**/*"
    } else {
        "./capabilities/default.json"
    };

    // `capabilities_path_pattern` disables tauri-build's own rerun-if-changed
    // for the directory, so it has to be re-emitted here or an edited
    // capability would not trigger a rebuild.
    println!("cargo:rerun-if-changed=capabilities");

    tauri_build::try_build(tauri_build::Attributes::new().capabilities_path_pattern(capabilities))
        .expect("failed to run tauri-build");
}
