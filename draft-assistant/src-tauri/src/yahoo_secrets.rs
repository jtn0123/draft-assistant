//! Where the Yahoo credentials and tokens live.
//!
//! Same deal as [`crate::secrets`], which keeps the Anthropic key in the macOS
//! login Keychain through `/usr/bin/security`. Three differences:
//!
//! - The value goes to `security` as an argument, hex-encoded, not on stdin.
//!   The stdin route answers the tool's password prompt, and that prompt
//!   keeps 128 bytes and silently drops the rest: an Anthropic key fits, a
//!   Yahoo token set or the companion's device list does not, and what came
//!   back was an unparseable stub that read as "nothing stored". The
//!   argument is visible in `ps` for the milliseconds the tool runs, to
//!   processes of the same user, and those same processes can already read
//!   the item back with one `find-generic-password -w`; the tool that wrote
//!   the item is trusted to read it without a prompt. Hex rather than the
//!   text itself because `find-generic-password -w` prints an item as hex
//!   the moment it holds one non-ASCII byte and as text otherwise, and a
//!   host called "Justin’s MacBook Air" is one such byte.
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
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const SERVICE: &str = "draft-assistant";

/// The things worth keeping. Kept as an enum so an account name cannot be
/// mistyped into existence at a call site.
///
/// Not all of them are Yahoo's: the companion's device tokens live here too,
/// because this module is the one path to the Keychain that takes a value of
/// any length and can be swapped for a file in a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// `{"client_id": .., "client_secret": ..}` from developer.yahoo.com.
    Credentials,
    /// `{"access_token": .., "refresh_token": .., "expires_at": ..}`.
    Token,
    /// The companion's pairing code and the bearer token of every paired
    /// phone, as one blob of JSON. See [`crate::companion::store`].
    CompanionDevices,
}

impl Item {
    pub fn account(self) -> &'static str {
        match self {
            Item::Credentials => "yahoo-app-credentials",
            Item::Token => "yahoo-oauth-token",
            Item::CompanionDevices => "companion-devices",
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

/// The `security` invocation for one operation on one item. `value` is what
/// a store writes; the other two operations ignore it.
///
/// The stored value is always the hex of the UTF-8 text (see the module
/// doc), so what `-w` prints on the way back is always plain ASCII that
/// [`decode_stored`] turns into the text again.
pub fn args_for(op: Op, item: Item, value: Option<&str>) -> Vec<String> {
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
    match op {
        Op::Store => args.extend(["-w".into(), hex_of(value.unwrap_or_default())]),
        Op::Load => args.push("-w".into()),
        Op::Clear => {}
    }
    args
}

/// Lower-case hex of the text's UTF-8 bytes.
pub fn hex_of(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

/// What `find-generic-password -w` printed, back to text.
///
/// Hex written by [`hex_of`] decodes to the text it came from. Anything else
/// is an item written before values were hex-encoded, or an item the tool
/// printed as text, and is handed back as it is; a legacy value that happens
/// to be entirely hex digits is not a case that arises, because every value
/// this module has ever stored is JSON and starts with a brace.
pub fn decode_stored(printed: &str) -> String {
    let printed = printed.trim();
    let is_hex = !printed.is_empty()
        && printed.len().is_multiple_of(2)
        && printed.bytes().all(|b| b.is_ascii_hexdigit());
    if !is_hex {
        return printed.to_string();
    }
    let bytes: Option<Vec<u8>> = (0..printed.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&printed[i..i + 2], 16).ok())
        .collect();
    match bytes.and_then(|b| String::from_utf8(b).ok()) {
        Some(text) => text,
        None => printed.to_string(),
    }
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
        let output = Command::new("/usr/bin/security")
            .args(args_for(op, item, value))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
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
            .map(|printed| decode_stored(&printed))
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

/// Sign out: forget the token pair and nothing else.
///
/// This is what "Disconnect" does. The registered app's id and secret are not
/// the account — they identify this install to Yahoo, they cost a trip to
/// developer.yahoo.com to replace, and throwing them away to sign out of an
/// account would be a surprise.
pub fn clear_tokens(store: &dyn SecretStore) -> Result<(), String> {
    store.clear(Item::Token)
}

/// Forget both items: the token pair and the registered app with it. The
/// deliberate second step behind "Forget app credentials".
pub fn clear_all(store: &dyn SecretStore) -> Result<(), String> {
    store.clear(Item::Token)?;
    store.clear(Item::Credentials)
}

#[cfg(test)]
#[path = "yahoo_secrets_tests.rs"]
mod tests;
