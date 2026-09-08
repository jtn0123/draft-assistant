//! Opt-in browser journey over the real companion listener, with no model calls.
use crate::harness::host;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "runs Chromium and a local Vite server; invoke explicitly"]
async fn phone_and_desktop_follow_a_real_host_and_recover_together() {
    let host = host("browser-rehearsal").await;
    host.state
        .loaded
        .lock()
        .await
        .as_mut()
        .unwrap()
        .user_avatars
        .clear();
    let artifacts = host.data_dir.join("browser-evidence");
    std::fs::create_dir_all(&artifacts).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let mut child = tokio::process::Command::new("node")
        .arg("scripts/companion-browser-rehearsal.mjs")
        .current_dir(root)
        .env("COMPANION_TEST_URL", &host.base)
        .env("COMPANION_TEST_CODE", host.companion.hub.code())
        .env("COMPANION_TEST_ARTIFACTS", &artifacts)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("start browser rehearsal");
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    let run = async {
        while let Some(step) = output.next_line().await.unwrap() {
            let answer = match step.as_str() {
                "NEXT_CODE" => host.companion.hub.code(),
                "PAIRED" => {
                    assert_eq!(host.companion.hub.devices().len(), 2);
                    "ok".into()
                }
                "DROP" => {
                    host.companion.hub.close_everyone();
                    "ok".into()
                }
                "UPDATE" => {
                    let mut loaded = host.state.loaded.lock().await;
                    let loaded = loaded.as_mut().unwrap();
                    loaded.league.name = "Recovered Fixture League".into();
                    for label in loaded.user_names.values_mut() {
                        *label = "Recovered Manager".into();
                    }
                    "ok".into()
                }
                "REVOKE" => {
                    host.companion.hub.revoke().expect("revoke fixture devices");
                    "ok".into()
                }
                "PASSED" => break,
                _ => panic!("unexpected browser rehearsal step"),
            };
            input
                .write_all(format!("{answer}\n").as_bytes())
                .await
                .unwrap();
        }
        drop(input);
        child.wait_with_output().await.unwrap()
    };
    let result = tokio::time::timeout(Duration::from_secs(180), run)
        .await
        .expect("browser rehearsal completed within three minutes");
    // Browser script sanitizes diagnostics and never prints pairing credentials.
    if !result.status.success() {
        std::fs::write(artifacts.join("browser-error.log"), result.stderr).unwrap();
    }
    assert!(
        result.status.success(),
        "browser rehearsal failed; evidence {}",
        artifacts.display()
    );
    println!(
        "phone + desktop browser rehearsal passed; evidence {}",
        artifacts.display()
    );
}
