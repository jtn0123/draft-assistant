//! Where the Yahoo credentials and tokens live.
//!
//! Same deal as [`crate::secrets`], which keeps the Anthropic key in the macOS
//! login Keychain through `/usr/bin/security` and never puts the value in
//! `argv`. Two differences:
//!
//! - There are two items, not one: the app credentials Yahoo issues
//!   (client id **and** secret) and the token pair from the OAuth flow. They
//!   get their own Keychain accounts under the app's existing service, so
//!   revoking one does not disturb the Anthropic key.
//! - The non-Keychain fallback is a file of this module's own rather than the
//!   app config, because a Yahoo token has no business in a settings file that
//!   the config screen rewrites. It is written 0600 on unix.
//!
//! Both live behind [`SecretStore`], which is what lets the tests here run
//! against a directory in `/tmp` and never touch a real login Keychain.

use crate::yahoo_oauth::{TokenSet, YahooCredentials};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SERVICE: &str = "draft-assistant";

/// The two things worth keeping. Kept as an enum so an account name cannot be
/// mistyped into existence at a call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// `{"client_id": .., "client_secret": ..}` from developer.yahoo.com.
    Credentials,
    /// `{"access_token": .., "refresh_token": .., "expires_at": ..}`.
    Token,
}

impl Item {
    pub fn account(self) -> &'static str {
        match self {
            Item::Credentials => "yahoo-app-credentials",
            Item::Token => "yahoo-oauth-token",
        }
    }
}

/// The three things we ask a store to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Store,
    Load,
    Clear,
}

/// The `security` invocation for one operation on one item.
///
/// The value is never an argument: `add-generic-password -w` with nothing
/// after it reads from stdin, which keeps a refresh token out of the `ps`
/// output every process on the machine can read.
pub fn args_for(op: Op, item: Item) -> Vec<String> {
    let mut args: Vec<String> = match op {
        Op::Store => vec!["add-generic-password".into(), "-U".into()],
        Op::Load => vec!["find-generic-password".into()],
        Op::Clear => vec!["delete-generic-password".into()],
    };
    args.extend([
        "-s".into(),
        SERVICE.into(),
        "-a".into(),
        item.account().into(),
    ]);
    if matches!(op, Op::Store | Op::Load) {
        args.push("-w".into());
    }
    args
}

/// Somewhere a secret can be kept.
pub trait SecretStore: Send + Sync {
    fn read(&self, item: Item) -> Option<String>;
    fn write(&self, item: Item, value: &str) -> Result<(), String>;
    fn clear(&self, item: Item) -> Result<(), String>;
}

/// The macOS login Keychain, via the `security` tool.
pub struct Keychain;

/// Whether the Keychain is the right place: macOS with the tool present.
pub fn available() -> bool {
    cfg!(target_os = "macos") && Path::new("/usr/bin/security").is_file()
}

impl Keychain {
    fn run(op: Op, item: Item, value: Option<&str>) -> Result<String, String> {
        let mut child = Command::new("/usr/bin/security")
            .args(args_for(op, item))
            .stdin(if value.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("keychain: {e}"))?;
        if let Some(value) = value {
            // `security` prompts for the password and then for a confirmation,
            // so it wants the value twice.
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| "keychain: no stdin".to_string())?;
            stdin
                .write_all(format!("{value}\n{value}\n").as_bytes())
                .map_err(|e| format!("keychain: {e}"))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|e| format!("keychain: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "keychain {} failed: {}",
                item.account(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl SecretStore for Keychain {
    fn read(&self, item: Item) -> Option<String> {
        Self::run(Op::Load, item, None)
            .ok()
            .filter(|value| !value.is_empty())
    }

    fn write(&self, item: Item, value: &str) -> Result<(), String> {
        Self::run(Op::Store, item, Some(value)).map(|_| ())
    }

    fn clear(&self, item: Item) -> Result<(), String> {
        match Self::run(Op::Clear, item, None) {
            Ok(_) => Ok(()),
            // Nothing stored is the state that was wanted.
            Err(e) if e.contains("could not be found") => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// One JSON file, for machines with no Keychain — and for tests, which must
/// never write to the developer's real login Keychain.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    /// `<data dir>/yahoo-secrets.json`.
    pub fn in_dir(dir: impl AsRef<Path>) -> Self {
        Self {
            path: dir.as_ref().join("yahoo-secrets.json"),
        }
    }

    fn all(&self) -> serde_json::Map<String, serde_json::Value> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self, map: serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("yahoo secrets: {e}"))?;
        }
        let text = serde_json::to_string_pretty(&map).map_err(|e| format!("yahoo secrets: {e}"))?;
        std::fs::write(&self.path, text).map_err(|e| format!("yahoo secrets: {e}"))?;
        restrict(&self.path)
    }
}

/// Owner-only permissions. A token in a world-readable file would undo the
/// point of keeping it out of the config.
#[cfg(unix)]
fn restrict(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("yahoo secrets: {e}"))
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> Result<(), String> {
    Ok(())
}

impl SecretStore for FileStore {
    fn read(&self, item: Item) -> Option<String> {
        match self.all().get(item.account())? {
            serde_json::Value::String(value) if !value.is_empty() => Some(value.clone()),
            _ => None,
        }
    }

    fn write(&self, item: Item, value: &str) -> Result<(), String> {
        let mut map = self.all();
        map.insert(item.account().to_string(), value.into());
        self.save(map)
    }

    fn clear(&self, item: Item) -> Result<(), String> {
        let mut map = self.all();
        map.remove(item.account());
        self.save(map)
    }
}

/// The store this machine should use: the Keychain where there is one, the
/// file in the app's data directory otherwise.
pub fn store_for(data_dir: impl AsRef<Path>) -> Box<dyn SecretStore> {
    if available() {
        Box::new(Keychain)
    } else {
        Box::new(FileStore::in_dir(data_dir))
    }
}

/// The stored token pair, if the flow has ever been completed.
pub fn load_tokens(store: &dyn SecretStore) -> Option<TokenSet> {
    serde_json::from_str(&store.read(Item::Token)?).ok()
}

pub fn save_tokens(store: &dyn SecretStore, tokens: &TokenSet) -> Result<(), String> {
    let text = serde_json::to_string(tokens).map_err(|e| format!("yahoo secrets: {e}"))?;
    store.write(Item::Token, &text)
}

/// The registered app's id and secret.
pub fn load_credentials(store: &dyn SecretStore) -> Option<YahooCredentials> {
    let parsed: YahooCredentials = serde_json::from_str(&store.read(Item::Credentials)?).ok()?;
    (!parsed.client_id.is_empty() && !parsed.client_secret.is_empty()).then_some(parsed)
}

pub fn save_credentials(
    store: &dyn SecretStore,
    credentials: &YahooCredentials,
) -> Result<(), String> {
    let text = serde_json::to_string(credentials).map_err(|e| format!("yahoo secrets: {e}"))?;
    store.write(Item::Credentials, &text)
}

/// Disconnect: forget both items. Used by a "Disconnect Yahoo" button, and by
/// the recovery path when a refresh token has been revoked.
pub fn clear_all(store: &dyn SecretStore) -> Result<(), String> {
    store.clear(Item::Token)?;
    store.clear(Item::Credentials)
}

#[cfg(test)]
#[path = "yahoo_secrets_tests.rs"]
mod tests;
