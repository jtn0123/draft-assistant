//! A remote snapshot must not stall shared state while waiting for CPU work.
use super::*;

#[test]
fn draft_snapshot_releases_both_locks_before_waiting_for_the_blocking_pool() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (state, dir) = AppState::scratch("snapshot-locks");
        *state.loaded.lock().await = Some(crate::keepers::bare_league("snapshot-draft"));
        let state = Arc::new(state);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        started_rx.await.unwrap();
        // Hold config until the snapshot demonstrably holds loaded and waits
        // for it. This makes the ordering deterministic without timing sleeps.
        let config = state.config.lock().await;
        let work_state = state.clone();
        let snapshot =
            tokio::spawn(
                async move { draft_view_snapshot(&work_state, Some("snapshot-draft")).await },
            );
        let reached = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while state.loaded.try_lock().is_ok() {
                tokio::task::yield_now().await;
            }
        })
        .await;
        drop(config);
        let unlocked = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let (Ok(_loaded), Ok(_config)) =
                    (state.loaded.try_lock(), state.config.try_lock())
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        let waiting_for_cpu = !snapshot.is_finished();
        // Always release the worker before asserting, so a regression cannot
        // hang runtime shutdown and hide the failure.
        release_tx.send(()).unwrap();
        blocker.await.unwrap();
        let view = snapshot.await.unwrap().unwrap();
        assert!(reached.is_ok(), "snapshot reached shared state");
        assert!(unlocked.is_ok(), "CPU queue retained a shared guard");
        assert!(waiting_for_cpu, "the blocking worker is still occupied");
        assert_eq!(view.draft.draft_id, "snapshot-draft");
        assert!(draft_view_snapshot(&state, Some("other-draft"))
            .await
            .is_err());
        std::fs::remove_dir_all(dir).unwrap();
    });
}
