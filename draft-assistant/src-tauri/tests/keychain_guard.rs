//! The one guard that is about the suite itself: no test may reach the
//! machine's Keychain.
//!
//! `Engine::for_app` and the default `YahooState` are the two constructors
//! that can, and a test that used one would read, and could rewrite, the
//! signed-in user's real secrets. Its own binary because it is a scan over
//! the source tree rather than an exercise of the engine, and because
//! `tests/engine_config.rs` is at the line cap.

/// `Engine::for_app` and the default `YahooState` are the two constructors
/// that can reach the machine's Keychain. Neither belongs in a test: this
/// reads every test file in the crate so one cannot quietly come back. The
/// needles are assembled at runtime so this test's own text does not trip it.
///
/// "Test code" is every file under `tests/`, every `*_tests.rs` module, and
/// the inline `#[cfg(test)] mod tests` at the foot of an ordinary source
/// file. That last one used to be skipped, which left most of the crate's
/// test code outside a guard written to keep the Keychain out of the suite.
#[test]
fn no_test_builds_an_engine_or_yahoo_state_over_the_real_keychain() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_rust_files(&root.join("tests"), &mut files);
    collect_rust_files(&root.join("src"), &mut files);
    let needles = [
        format!("Engine::{}(", "for_app"),
        format!("YahooState::{}()", "default"),
        format!("YahooState::{}(", "new"),
    ];
    let mut offenders = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("read a source file");
        let Some(tests) = test_code(&file, &text, root) else {
            continue;
        };
        for needle in &needles {
            if tests.contains(needle.as_str()) {
                offenders.push(format!("{}: {needle}", file.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these tests can reach the real Keychain; use Engine::new / YahooState::sandboxed:\n{}",
        offenders.join("\n")
    );
}

/// The part of `text` that is test code, or `None` when none of it is.
///
/// A file under `tests/` or named `*_tests.rs` is test code throughout.
/// Anywhere else it is everything from the first inline `#[cfg(test)] mod`
/// on: such a module is the last thing in a source file here, so the tail of
/// the file is the module and nothing but. A `#[cfg(test)]` on a plain `mod
/// name;` declaration is not one, and `src/lib.rs` has exactly that.
fn test_code<'a>(file: &std::path::Path, text: &'a str, root: &std::path::Path) -> Option<&'a str> {
    let all_of_it = file.starts_with(root.join("tests"))
        || file
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("_tests.rs"));
    if all_of_it {
        return Some(text);
    }
    let marker = format!("#[{}(test)]", "cfg");
    let mut from = 0;
    while let Some(at) = text[from..].find(&marker) {
        let at = from + at;
        let rest = &text[at + marker.len()..];
        let opens_a_module = rest
            .lines()
            .find(|line| !line.trim().is_empty())
            .is_some_and(|line| line.trim_end().ends_with('{'));
        if opens_a_module {
            return Some(&text[at..]);
        }
        from = at + marker.len();
    }
    None
}

fn collect_rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}
