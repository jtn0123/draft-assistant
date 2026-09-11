//! Revocation failures use only an in-memory secret store, never the Keychain.
use super::*;
use crate::yahoo_secrets::Item;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Store(Arc<Mutex<(Option<String>, bool)>>);
impl SecretStore for Store {
    fn read(&self, _: Item) -> Option<String> {
        self.0.lock().unwrap().0.clone()
    }
    fn write(&self, _: Item, value: &str) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        if state.1 {
            return Err("secret store unavailable".into());
        }
        state.0 = Some(value.into());
        Ok(())
    }
    fn clear(&self, _: Item) -> Result<(), String> {
        self.0.lock().unwrap().0 = None;
        Ok(())
    }
}

#[test]
fn failed_revocation_is_reported_and_successful_retry_survives_restart() {
    let store = Store::default();
    let dir = std::env::temp_dir().join(format!("revocation-test-{}", std::process::id()));
    let build =
        || CompanionHub::with_secrets("Test".into(), dir.clone(), Box::new(store.clone())).unwrap();
    let hub = build();
    let code = hub.code();
    let PairOutcome::Ok { token, .. } = hub
        .pair(PairAttempt {
            code: &code,
            name: "Phone",
            kind: "phone",
            peer: "127.0.0.1".parse().unwrap(),
            previous_device_id: None,
        })
        .unwrap()
    else {
        panic!("pairing failed")
    };
    store.0.lock().unwrap().1 = true;
    let error = hub
        .revoke()
        .expect_err("failed storage must not report successful revocation");
    assert!(error.contains("restart"), "{error}");
    assert!(
        hub.device_for(&token).is_none(),
        "live access must still end"
    );
    assert!(
        build().device_for(&token).is_some(),
        "failed persistence retains the old store"
    );
    store.0.lock().unwrap().1 = false;
    hub.revoke().unwrap();
    assert!(
        build().device_for(&token).is_none(),
        "successful revocation must survive restart"
    );
}
