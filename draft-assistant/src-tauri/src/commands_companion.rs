//! The desktop side of the phone / second-screen companion.

use crate::companion::routes_chat::{ask, AskError};
use crate::companion::{net, CompanionServer, Device};
use crate::shared_chat::{EntryDevice, SharedChatThread};
use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

/// What the Settings panel draws itself from.
#[derive(serde::Serialize)]
pub struct CompanionStatus {
    pub enabled: bool,
    /// The address to show and put in the QR code, while the server is up.
    pub url: Option<String>,
    /// The six digits typed into the phone. Shown on the host's own screen
    /// only — it is what stands between the LAN and this app's data.
    pub code: String,
    pub port: Option<u16>,
    pub host_name: String,
    pub devices: Vec<Device>,
}

fn status_of(companion: &CompanionServer) -> CompanionStatus {
    CompanionStatus {
        enabled: companion.is_enabled(),
        url: companion.url(),
        code: companion.hub.code(),
        port: companion.port(),
        host_name: companion.hub.host_name(),
        devices: companion.hub.devices(),
    }
}

#[tauri::command]
pub async fn companion_status(
    companion: State<'_, Arc<CompanionServer>>,
) -> Result<CompanionStatus, String> {
    Ok(status_of(&companion))
}

/// Turn the server on, on the last port used or the first free one after the
/// default. The port that was actually taken is written back to the config,
/// so a bookmarked address keeps working.
#[tauri::command]
pub async fn companion_enable(
    state: State<'_, AppState>,
    companion: State<'_, Arc<CompanionServer>>,
) -> Result<CompanionStatus, String> {
    let wanted = state
        .config
        .lock()
        .await
        .companion_port
        .unwrap_or(net::DEFAULT_PORT);
    let port = companion.start(wanted).await?;
    if wanted != port {
        let mut config = state.config.lock().await;
        config.companion_port = Some(port);
        // A port that could not be written down is not a reason to refuse the
        // connection the user just asked for; it only means the next launch
        // starts from the default again.
        if let Err(e) = state.engine.save_config(&config) {
            crate::applog::warn(format!(
                "could not remember the phone connection's port: {e}"
            ));
        }
    }
    Ok(status_of(&companion))
}

#[tauri::command]
pub async fn companion_disable(
    companion: State<'_, Arc<CompanionServer>>,
) -> Result<CompanionStatus, String> {
    companion.stop();
    Ok(status_of(&companion))
}

/// A new pairing code, and every paired device thrown off.
#[tauri::command]
pub async fn companion_revoke(
    companion: State<'_, Arc<CompanionServer>>,
) -> Result<CompanionStatus, String> {
    companion.hub.revoke()?;
    Ok(status_of(&companion))
}

/// What this Mac is called in the shared chat and on a follower's pill.
#[tauri::command]
pub async fn set_device_name(
    state: State<'_, AppState>,
    companion: State<'_, Arc<CompanionServer>>,
    name: String,
) -> Result<String, String> {
    let name = name.trim();
    let name = if name.is_empty() {
        default_host_name()
    } else {
        name.chars().take(60).collect()
    };
    let mut config = state.config.lock().await;
    config.device_name = Some(name.clone());
    state.engine.save_config(&config)?;
    companion.hub.set_host_name(name.clone());
    Ok(name)
}

#[tauri::command]
pub async fn shared_chat_get(
    companion: State<'_, Arc<CompanionServer>>,
    screen: String,
) -> Result<SharedChatThread, String> {
    let srv = companion.srv()?;
    let league_id = crate::companion::routes_chat::active_league(&srv).await?;
    Ok(srv.chat.thread(&league_id, screen_name(&screen)?).await)
}

/// Ask a question in the shared thread as the host machine.
#[tauri::command]
pub async fn shared_chat_send(
    companion: State<'_, Arc<CompanionServer>>,
    screen: String,
    text: String,
) -> Result<(), String> {
    let srv = companion.srv()?;
    let device = EntryDevice {
        name: srv.hub.host_name(),
        kind: "host".to_string(),
    };
    ask(srv, screen_name(&screen)?, device, text)
        .await
        .map_err(AskError::message)?;
    Ok(())
}

fn screen_name(screen: &str) -> Result<&'static str, String> {
    match screen {
        "draft" => Ok("draft"),
        "season" => Ok("season"),
        other => Err(format!("'{other}' is not a screen Ask Claude answers for")),
    }
}

/// What this machine calls itself, before the user has renamed it.
///
/// The name in System Settings first — it is the one the user recognises and
/// the one AirDrop shows. Then the network hostname, which is the same thing
/// with the rough edges on. Then a constant, because a host with no name is
/// still a host worth pairing with.
pub fn default_host_name() -> String {
    from_command("scutil", &["--get", "ComputerName"])
        .or_else(|| from_command("hostname", &["-s"]))
        .unwrap_or_else(|| "This Mac".to_string())
}

fn from_command(program: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name.chars().take(60).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{default_host_name, from_command, screen_name};

    #[test]
    fn a_host_always_has_a_name() {
        let name = default_host_name();
        assert!(!name.is_empty());
        assert!(name.chars().count() <= 60);
    }

    #[test]
    fn a_command_that_is_not_there_is_not_a_name() {
        assert_eq!(from_command("no-such-program-here", &[]), None);
        // A command that runs but says nothing is no name either.
        assert_eq!(from_command("true", &[]), None);
        assert_eq!(from_command("echo", &["hello"]).as_deref(), Some("hello"));
    }

    #[test]
    fn only_the_two_screens_have_a_shared_thread() {
        assert_eq!(screen_name("season").expect("a screen"), "season");
        assert!(screen_name("Draft").is_err());
    }
}
