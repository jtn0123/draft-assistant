//! Nothing kept `applog::logged!` coverage true. Every `#[tauri::command]`
//! that can fail is supposed to wrap its body so the failure outlives the
//! toast, but that was a convention: a command added without the wrapper
//! compiled, shipped, and failed into a toast that was dismissed, leaving no
//! record it had been called. This reads the source and fails the build
//! instead.

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Commands allowed to skip `applog::logged!`, each with the reason it cannot
/// fail. Anything not on this list must wrap its body, and anything on it
/// must stay infallible: the test below checks both.
///
/// The failure this prevents: `logged!` coverage was a convention, and a
/// command added without it failed into a toast that was dismissed, leaving
/// no record it had been called. Now the omission fails `cargo test`.
const INFALLIBLE: [(&str, &str); 5] = [
    (
        "diagnostics",
        "reads state that is already in memory; the Result is Tauri's async signature, never Err",
    ),
    (
        "log_frontend_error",
        "the page's own reporter; a reporter that can fail reports its own failure in a loop",
    ),
    ("chat_suggestions", "returns a Vec, not a Result"),
    (
        "start_polling",
        "flips a flag and spawns the loop; every failure inside the loop is logged there",
    ),
    ("stop_polling", "flips a flag"),
];

/// Every `#[tauri::command]` with the text of its body, by name.
fn command_bodies() -> Vec<(String, String)> {
    fn walk(dir: &Path, found: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).expect("read source dir") {
            let path = entry.expect("read dir entry").path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                collect(
                    &std::fs::read_to_string(&path).expect("read source file"),
                    found,
                );
            }
        }
    }

    fn collect(source: &str, found: &mut Vec<(String, String)>) {
        let lines: Vec<&str> = source.lines().collect();
        let mut at = 0;
        while at < lines.len() {
            if lines[at].trim() != "#[tauri::command]" {
                at += 1;
                continue;
            }
            let signature = (at + 1..lines.len())
                .find(|&i| lines[i].contains("fn "))
                .expect("a #[tauri::command] is followed by a function");
            let name = lines[signature]
                .split("fn ")
                .nth(1)
                .and_then(|rest| rest.split(['(', '<']).next())
                .expect("signature has a name")
                .trim()
                .to_string();
            // The body runs to the brace that closes the one opening it.
            let mut depth = 0i32;
            let mut opened = false;
            let mut end = signature;
            for (i, line) in lines.iter().enumerate().skip(signature) {
                let code = line.split("//").next().unwrap_or("");
                depth += code.matches('{').count() as i32 - code.matches('}').count() as i32;
                opened |= code.contains('{');
                end = i;
                if opened && depth == 0 {
                    break;
                }
            }
            found.push((name, lines[signature..=end].join("\n")));
            at = end + 1;
        }
    }

    let mut found = Vec::new();
    walk(&src_dir(), &mut found);
    found
}

/// Whether a body can hand back an `Err`: a `?` or an `Err(...)` that is a
/// value rather than a pattern (a `match` arm's `Err(e) =>`, an `if let
/// Err(e) = ...`). Comments are stripped first.
fn can_fail(body: &str) -> bool {
    let code: String = body
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    if code.contains('?') {
        return true;
    }
    code.match_indices("Err(").any(|(start, _)| {
        let after = &code[start + "Err(".len()..];
        let close = after.find(')').map_or(after.len(), |i| i + 1);
        let next = after[close..].trim_start();
        !(next.starts_with("=>") || next.starts_with("= "))
    })
}

/// A command that can fail and does not go through `logged!` fails into a
/// toast and nowhere else.
#[test]
fn every_command_that_can_fail_is_wrapped_in_logged() {
    let bodies = command_bodies();
    assert!(!bodies.is_empty(), "no commands found under src/");
    let mut missing = Vec::new();
    let mut not_infallible = Vec::new();
    for (name, body) in &bodies {
        let allowed = INFALLIBLE.iter().find(|(allowed, _)| allowed == name);
        match allowed {
            Some((_, reason)) => {
                assert!(
                    !body.contains("logged!"),
                    "{name} is on the infallible list but uses logged!; drop it from the list"
                );
                if can_fail(body) {
                    not_infallible.push(format!("{name} ({reason})"));
                }
            }
            None if !body.contains("logged!") => missing.push(name.clone()),
            None => {}
        }
    }
    assert!(
        missing.is_empty(),
        "commands that can fail without a log line: {missing:?}; wrap the body in applog::logged! \
         or add the command to INFALLIBLE with the reason it cannot fail"
    );
    assert!(
        not_infallible.is_empty(),
        "commands on the INFALLIBLE list that can now return Err: {not_infallible:?}"
    );
    for (name, _) in INFALLIBLE {
        assert!(
            bodies.iter().any(|(found, _)| found == name),
            "{name} is on the INFALLIBLE list but is no longer a command"
        );
    }
}
