//! Where the Anthropic API key lives.
//!
//! On macOS it goes in the login Keychain through the system `security` tool,
//! so it never sits in a plaintext JSON file next to the caches. Anywhere the
//! Keychain is unavailable the key stays in the config file as before.
//!
//! This is a thin layer over [`crate::yahoo_secrets`], the app's one secret
//! store: the key is [`Item::AnthropicKey`] there, under the account name
//! this module has always used, so a key an older build stored still reads.
//! It used to be a store of its own that answered the `security` tool's
//! password prompt on stdin and read the answer back literally. That prompt
//! keeps 128 bytes and drops the rest without a word, and
//! `find-generic-password -w` prints an item as hex the moment it holds one
//! non-ASCII byte, so a long key came back cut short and an odd one came back
//! as a hex string the API rejected. The shared store hex-encodes the value
//! into argv and decodes it on the way back, whatever its length.

use crate::engine::AppConfig;
use crate::yahoo_secrets::{Item, SecretStore};

pub use crate::yahoo_secrets::Op;

/// The `security` invocation for one operation on the key. `key` is what a
/// store writes, hex-encoded; the other two operations ignore it. Pure, so the
/// exact invocation is testable without touching a real Keychain.
pub fn args_for(op: Op, key: Option<&str>) -> Vec<String> {
    crate::yahoo_secrets::args_for(op, Item::AnthropicKey, key)
}

/// Keychain storage is a macOS thing; everywhere else falls back to the file.
pub fn available() -> bool {
    crate::yahoo_secrets::available()
}

/// The key as `store` holds it, or `None` when it holds nothing.
pub fn load_from(store: &dyn SecretStore) -> Option<String> {
    store.read(Item::AnthropicKey)
}

pub fn store_in(store: &dyn SecretStore, key: &str) -> Result<(), String> {
    store.write(Item::AnthropicKey, key)
}

pub fn clear_in(store: &dyn SecretStore) -> Result<(), String> {
    store.clear(Item::AnthropicKey)
}

/// How the rest of the app gets at the key.
///
/// These sit here rather than on `Engine` itself because everything they
/// decide — which copy of the key wins, when the store is worth asking, how
/// long the answer is good for — is this module's business. The store itself
/// is whatever the engine was built with ([`Engine::secret_store`]): the
/// machine's Keychain in the shipped app, a file in the data directory for
/// everything built over a scratch directory, so nothing here can reach a
/// Keychain the engine was not handed.
///
/// [`Engine::secret_store`]: crate::engine::Engine::secret_store
impl crate::engine::Engine {
    /// The Anthropic key, wherever it is kept.
    ///
    /// The Keychain lookup is a subprocess, so it runs on the blocking pool
    /// rather than on a runtime thread, and its answer is cached: a chat
    /// question used to spawn `security` every time it was asked.
    pub async fn api_key(&self, config: &AppConfig) -> Option<String> {
        let Some(store) = self.secrets.clone() else {
            return config.anthropic_api_key.clone();
        };
        let mut cache = self.key_cache.lock().await;
        let stored = match cache.as_ref() {
            Some(known) => known.clone(),
            None => {
                let loaded = tokio::task::spawn_blocking(move || load_from(store.as_ref()))
                    .await
                    .ok()
                    .flatten();
                *cache = Some(loaded.clone());
                loaded
            }
        };
        chosen_key(stored, config)
    }

    /// Store (or, with `None`, clear) the key: the engine's store when it has
    /// one, the config file otherwise.
    ///
    /// Nothing is written to `config.json` here, and no `AppConfig` is taken.
    /// This used to be handed a clone of the live config and save the whole
    /// thing once the Keychain came back — and the Keychain can take seconds
    /// and put a prompt in front of the user, so anything the pollers wrote
    /// meanwhile (a refreshed board, a new league, what Ask Claude had spent)
    /// was rolled back to whatever the clone remembered. What the caller gets
    /// instead is the one field that changed: it re-reads the live config,
    /// sets that field, and saves under the lock.
    pub async fn store_api_key(&self, key: Option<String>) -> Result<Option<String>, String> {
        let Some(store) = self.secrets.clone() else {
            // No store: the key itself is what belongs in the config file.
            return Ok(key);
        };
        let writing = key.clone();
        tokio::task::spawn_blocking(move || match &writing {
            Some(k) => store_in(store.as_ref(), k),
            None => clear_in(store.as_ref()),
        })
        .await
        .map_err(|e| format!("could not reach the key store: {e}"))??;
        // The remembered answer is now the one we just wrote, so the next
        // question does not have to go and ask again.
        *self.key_cache.lock().await = Some(key);
        // The store holds it; the config file must not also hold a copy.
        Ok(None)
    }
}

/// Which key wins: the Keychain's copy when it has one, otherwise whatever is
/// still in the config file. Older installs and machines with no Keychain
/// keep it in the file, and those must keep working.
fn chosen_key(stored: Option<String>, config: &AppConfig) -> Option<String> {
    stored.or_else(|| config.anthropic_api_key.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yahoo_secrets::{decode_stored, hex_of, FileStore};
    use std::path::PathBuf;

    /// A directory of this test's own, removed when it is done. Nothing here
    /// runs `/usr/bin/security`; the round trips go through a file store so
    /// the developer's real login Keychain is never written.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "draft-assistant-key-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).expect("scratch dir");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const KEY: &str = "sk-ant-api03-not-a-real-key";

    fn config_with(key: Option<&str>) -> AppConfig {
        AppConfig {
            anthropic_api_key: key.map(str::to_string),
            ..AppConfig::default()
        }
    }

    #[test]
    fn the_keychain_copy_wins_over_the_one_left_in_the_config_file() {
        let config = config_with(Some("from-file"));
        assert_eq!(
            chosen_key(Some("from-keychain".into()), &config).as_deref(),
            Some("from-keychain")
        );
    }

    #[test]
    fn a_key_still_in_the_config_file_is_used_when_the_keychain_has_none() {
        let config = config_with(Some("from-file"));
        assert_eq!(chosen_key(None, &config).as_deref(), Some("from-file"));
    }

    #[test]
    fn no_key_anywhere_is_no_key() {
        assert!(chosen_key(None, &config_with(None)).is_none());
    }

    #[test]
    fn the_key_never_appears_in_the_arguments_as_plain_text() {
        // Anything in argv is visible in `ps` to every other process of this
        // user for as long as the tool runs; the hex form at least keeps a
        // casual `ps | grep sk-` from finding it, and a load or clear has no
        // business carrying it at all.
        for op in [Op::Store, Op::Load, Op::Clear] {
            let args = args_for(op, Some(KEY));
            assert!(
                !args.iter().any(|a| a.contains("sk-")),
                "{op:?} put the key in argv as text: {args:?}"
            );
        }
        for op in [Op::Load, Op::Clear] {
            let args = args_for(op, Some(KEY));
            assert!(
                !args.contains(&hex_of(KEY)),
                "{op:?} carries a key it has no use for: {args:?}"
            );
        }
    }

    /// The stdin route answered the tool's password prompt, which keeps 128
    /// bytes and drops the rest without a word. The key has to travel as a
    /// hex argument, the way every other item does, so all of it arrives.
    #[test]
    fn store_passes_the_key_as_hex_in_argv_rather_than_on_the_password_prompt() {
        let args = args_for(Op::Store, Some(KEY));
        assert_eq!(args[0], "add-generic-password");
        assert!(
            args.contains(&"-U".to_string()),
            "must overwrite, not duplicate"
        );
        let n = args.len();
        assert_eq!(args[n - 2], "-w");
        assert_eq!(args[n - 1], hex_of(KEY));
        assert_eq!(decode_stored(&args[n - 1]), KEY);
    }

    #[test]
    fn a_key_longer_than_the_password_prompt_keeps_round_trips_whole() {
        let long = format!("sk-ant-api03-{}", "k".repeat(300));
        assert!(long.len() > 128);
        // Through argv: the whole thing is there to be decoded.
        let args = args_for(Op::Store, Some(&long));
        assert_eq!(decode_stored(args.last().expect("a value")), long);
        // Through the file store: what went in comes out.
        let scratch = Scratch::new("long");
        let store = FileStore::in_dir(&scratch.0);
        assert!(load_from(&store).is_none());
        store_in(&store, &long).expect("store");
        assert_eq!(load_from(&store).as_deref(), Some(long.as_str()));
        clear_in(&store).expect("clear");
        assert!(load_from(&store).is_none());
    }

    /// A test-built engine has a file store in its own directory and no
    /// other, so the round trip is provably a file write: nothing here can
    /// spawn `/usr/bin/security`, whatever machine runs it.
    #[tokio::test]
    async fn an_engine_stores_the_key_in_the_store_it_was_built_with() {
        let scratch = Scratch::new("engine-store");
        let engine = crate::engine::Engine::new(scratch.0.clone());
        let config = config_with(None);
        assert!(engine.api_key(&config).await.is_none());

        let left_for_config = engine
            .store_api_key(Some(KEY.to_string()))
            .await
            .expect("stored");
        assert!(
            left_for_config.is_none(),
            "the store holds it, so the config file must not"
        );
        let on_disk =
            std::fs::read_to_string(scratch.0.join("yahoo-secrets.json")).expect("the file store");
        assert!(on_disk.contains(KEY), "the key is not in the scratch store");
        assert_eq!(
            load_from(&FileStore::in_dir(&scratch.0)).as_deref(),
            Some(KEY)
        );
        assert_eq!(engine.api_key(&config).await.as_deref(), Some(KEY));

        // Clearing goes to the same place and empties the cache with it.
        assert!(engine.store_api_key(None).await.expect("cleared").is_none());
        assert!(load_from(&FileStore::in_dir(&scratch.0)).is_none());
        assert!(engine.api_key(&config).await.is_none());
    }

    /// No store at all (a machine with no Keychain): the key is handed back
    /// for the config file to keep, and read from there.
    #[tokio::test]
    async fn with_no_store_the_key_is_left_to_the_config_file() {
        let scratch = Scratch::new("engine-no-store");
        let engine = crate::engine::Engine::with_secrets(
            scratch.0.clone(),
            crate::sleeper::SleeperClient::new(),
            None,
        );
        assert_eq!(
            engine
                .store_api_key(Some(KEY.to_string()))
                .await
                .expect("nothing to fail")
                .as_deref(),
            Some(KEY)
        );
        assert!(!scratch.0.join("yahoo-secrets.json").exists());
        assert_eq!(
            engine.api_key(&config_with(Some(KEY))).await.as_deref(),
            Some(KEY)
        );
    }

    /// A key an older build wrote went into the Keychain as its own text, and
    /// `find-generic-password -w` prints it back as that text. The decoder
    /// must hand it on unchanged rather than try to read it as hex.
    #[test]
    fn a_key_stored_before_values_were_hex_encoded_still_reads() {
        assert_eq!(decode_stored(KEY), KEY);
        assert_eq!(decode_stored(&format!("{KEY}\n")), KEY);
    }

    /// The account name is what an existing user's key is filed under. Change
    /// it and every Mac that already has a key stored asks for it again.
    #[test]
    fn load_and_clear_name_the_account_older_builds_wrote_to() {
        for op in [Op::Load, Op::Clear] {
            let args = args_for(op, None);
            assert_eq!(args[3], "-a");
            assert_eq!(args[4], "anthropic-api-key");
            assert!(args.contains(&"draft-assistant".to_string()));
        }
        assert!(!args_for(Op::Clear, None).contains(&"-w".to_string()));
    }
}
