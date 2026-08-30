//! Where the Anthropic API key lives.
//!
//! On macOS it goes in the login Keychain through the system `security` tool,
//! so it never sits in a plaintext JSON file next to the caches. Anywhere the
//! Keychain is unavailable the key stays in the config file as before.

use std::io::Write;
use std::process::{Command, Stdio};

const SERVICE: &str = "draft-assistant";
const ACCOUNT: &str = "anthropic-api-key";

/// The three things we ask the Keychain to do. An enum rather than a string so
/// an unknown operation cannot be constructed at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Store,
    Load,
    Clear,
}

impl Op {
    fn name(self) -> &'static str {
        match self {
            Op::Store => "store",
            Op::Load => "load",
            Op::Clear => "clear",
        }
    }
}

/// The `security` subcommand and arguments for one operation. Pure, so the
/// exact invocation is testable without touching a real Keychain.
///
/// The key is never an argument: `add-generic-password -w` with no value reads
/// it from stdin instead. Passing it here would put an `sk-ant-...` string in
/// `ps` output for every process on the machine to read.
pub fn args_for(op: Op) -> Vec<String> {
    let mut args: Vec<String> = match op {
        Op::Store => vec!["add-generic-password".into(), "-U".into()],
        Op::Load => vec!["find-generic-password".into()],
        Op::Clear => vec!["delete-generic-password".into()],
    };
    args.extend(["-s".into(), SERVICE.into(), "-a".into(), ACCOUNT.into()]);
    if matches!(op, Op::Store | Op::Load) {
        args.push("-w".into());
    }
    args
}

/// Keychain storage is a macOS thing; everywhere else falls back to the file.
pub fn available() -> bool {
    cfg!(target_os = "macos") && std::path::Path::new("/usr/bin/security").is_file()
}

fn run(op: Op, key: Option<&str>) -> Result<String, String> {
    let mut child = Command::new("/usr/bin/security")
        .args(args_for(op))
        .stdin(if key.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("keychain: {e}"))?;

    if let Some(key) = key {
        // `security` asks for the password and then asks again to confirm, so
        // it wants the value twice.
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "keychain: no stdin".to_string())?;
        stdin
            .write_all(format!("{key}\n{key}\n").as_bytes())
            .map_err(|e| format!("keychain: {e}"))?;
        // Dropping closes the pipe, which is what ends the prompt.
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("keychain: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "keychain {} failed: {}",
            op.name(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn store(key: &str) -> Result<(), String> {
    run(Op::Store, Some(key)).map(|_| ())
}

pub fn load() -> Option<String> {
    run(Op::Load, None).ok().filter(|k| !k.is_empty())
}

pub fn clear() -> Result<(), String> {
    match run(Op::Clear, None) {
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
    fn the_key_never_appears_in_the_arguments() {
        // The whole point: anything in argv is visible in `ps` to every other
        // process on the machine.
        for op in [Op::Store, Op::Load, Op::Clear] {
            let args = args_for(op);
            assert!(
                !args.iter().any(|a| a.starts_with("sk-")),
                "{op:?} leaked a key into argv: {args:?}"
            );
        }
    }

    #[test]
    fn store_updates_in_place_and_asks_for_the_password_on_stdin() {
        let args = args_for(Op::Store);
        assert_eq!(args[0], "add-generic-password");
        assert!(
            args.contains(&"-U".to_string()),
            "must overwrite, not duplicate"
        );
        // A bare trailing -w means "read it from stdin".
        assert_eq!(args.last().map(String::as_str), Some("-w"));
    }

    #[test]
    fn load_asks_for_the_password_only() {
        let args = args_for(Op::Load);
        assert_eq!(args[0], "find-generic-password");
        assert_eq!(args.last().map(String::as_str), Some("-w"));
        assert!(args.contains(&SERVICE.to_string()));
    }

    #[test]
    fn clear_names_the_item_and_asks_for_nothing() {
        let args = args_for(Op::Clear);
        assert_eq!(args[0], "delete-generic-password");
        assert!(args.contains(&ACCOUNT.to_string()));
        assert!(!args.contains(&"-w".to_string()));
    }
}
