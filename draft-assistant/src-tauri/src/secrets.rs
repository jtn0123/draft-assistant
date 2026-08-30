//! Where the Anthropic API key lives.
//!
//! On macOS it goes in the login Keychain through the system `security` tool,
//! so it never sits in a plaintext JSON file next to the caches. Anywhere the
//! Keychain is unavailable the key stays in the config file as before.

use std::process::Command;

const SERVICE: &str = "draft-assistant";
const ACCOUNT: &str = "anthropic-api-key";

/// The `security` subcommand and arguments for one operation. Pure, so the
/// exact invocation is testable without touching a real Keychain.
pub fn args_for(op: &str, key: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = match op {
        "store" => vec!["add-generic-password".into(), "-U".into()],
        "load" => vec!["find-generic-password".into()],
        "clear" => vec!["delete-generic-password".into()],
        other => panic!("unknown keychain op {other}"),
    };
    args.extend(["-s".into(), SERVICE.into(), "-a".into(), ACCOUNT.into()]);
    match (op, key) {
        ("store", Some(k)) => args.extend(["-w".into(), k.into()]),
        ("load", _) => args.push("-w".into()),
        _ => {}
    }
    args
}

/// Keychain storage is a macOS thing; everywhere else falls back to the file.
pub fn available() -> bool {
    cfg!(target_os = "macos") && std::path::Path::new("/usr/bin/security").is_file()
}

fn run(op: &str, key: Option<&str>) -> Result<String, String> {
    let output = Command::new("/usr/bin/security")
        .args(args_for(op, key))
        .output()
        .map_err(|e| format!("keychain: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "keychain {op} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn store(key: &str) -> Result<(), String> {
    run("store", Some(key)).map(|_| ())
}

pub fn load() -> Option<String> {
    run("load", None).ok().filter(|k| !k.is_empty())
}

pub fn clear() -> Result<(), String> {
    match run("clear", None) {
        Ok(_) => Ok(()),
        // Nothing stored is the state we wanted.
        Err(e) if e.contains("could not be found") => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_updates_in_place_and_sends_the_key_last() {
        let args = args_for("store", Some("sk-ant-x"));
        assert_eq!(args[0], "add-generic-password");
        assert!(
            args.contains(&"-U".to_string()),
            "must overwrite, not duplicate"
        );
        assert_eq!(&args[args.len() - 2..], ["-w", "sk-ant-x"]);
    }

    #[test]
    fn load_asks_for_the_password_only() {
        let args = args_for("load", None);
        assert_eq!(args[0], "find-generic-password");
        assert_eq!(args.last().map(String::as_str), Some("-w"));
        assert!(args.contains(&SERVICE.to_string()));
    }
}
