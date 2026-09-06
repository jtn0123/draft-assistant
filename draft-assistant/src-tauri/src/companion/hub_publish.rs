//! The fan-out half of the hub: what a `{type, payload}` frame costs to send,
//! and when one is not sent at all.
//!
//! A child module of `hub` rather than a sibling so it can reach the hub's
//! private channels; split out only to keep `hub.rs` under the size cap.

use super::CompanionHub;
use serde::Serialize;

impl CompanionHub {
    /// Whether any socket is on the far end of the event fan-out.
    ///
    /// The publish path asks this before it builds anything. A draft or season
    /// view is tens of kilobytes of JSON every three second tick, and
    /// `broadcast::send` drops the frame when no receiver exists, so with the
    /// companion server off or no phone connected the whole serialisation was
    /// paid for and then thrown away. Nothing is lost by skipping it: a socket
    /// that connects later gets its own opening snapshot from `ws.rs` before
    /// it starts reading this stream.
    pub fn has_listeners(&self) -> bool {
        self.events.receiver_count() > 0
    }

    /// Whether a frame published now would reach a phone. Off means off: the
    /// sockets a stopped server is still closing must not be fed one more
    /// board, so this is the running flag as well as the listener count.
    fn reaches_anyone(&self) -> bool {
        self.is_running() && self.has_listeners()
    }

    /// Fan one `{type, payload}` frame out to every open socket. Nothing is
    /// sent when nobody is listening, and a full channel is not an error —
    /// the events are a live feed, not a queue anyone replays.
    pub fn publish_json(&self, kind: &str, payload: serde_json::Value) {
        if !self.reaches_anyone() {
            return;
        }
        let frame = serde_json::json!({ "type": kind, "payload": payload });
        let _ = self.events.send(frame.to_string());
    }

    /// The same for anything serialisable. A value that will not serialise is
    /// dropped with a note rather than taking a poll tick down.
    pub fn publish<T: Serialize>(&self, kind: &str, payload: &T) {
        // Before `to_value`, not after: the serialisation is the expensive
        // half, and with nobody listening it has no reader to reach.
        if !self.reaches_anyone() {
            return;
        }
        match serde_json::to_value(payload) {
            Ok(value) => self.publish_json(kind, value),
            Err(e) => crate::applog::warn(format!(
                "companion: could not send a '{kind}' update to the paired devices: {e}"
            )),
        }
    }

    /// The device list, to the paired devices and to the host's own settings
    /// screen. The webview event is named separately in the contract because
    /// the desktop already has a `devices` of its own meaning nothing like it.
    pub fn publish_devices(&self) {
        let devices = self.devices();
        // Only the broadcast half is skipped when no socket is listening. The
        // webview emit below has to run either way, or the host's settings
        // screen would stop hearing about the first device to pair, which is
        // exactly the moment there is still no subscriber.
        self.publish("devices", &devices);
        match serde_json::to_value(&devices) {
            Ok(value) => self.to_webview("companion-devices", value),
            Err(e) => crate::applog::warn(format!("companion: could not list the devices: {e}")),
        }
    }
}
