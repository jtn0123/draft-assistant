//! What a paired device ends up being called in the host's device list.

/// A device name that is worth showing: trimmed, bounded, never empty.
pub fn display_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "A device".to_string();
    }
    trimmed.chars().take(60).collect()
}

/// A name nobody else is using: "iPhone", then "iPhone 2", "iPhone 3".
///
/// Every iPhone guesses the same name for itself, so without this the second
/// phone in a house is indistinguishable from the first in the device list —
/// and the dedupe by name that used to follow threw the first one off.
pub fn unique_name(wanted: &str, taken: &[&str]) -> String {
    if !taken.contains(&wanted) {
        return wanted.to_string();
    }
    for n in 2..1000 {
        let candidate = format!("{wanted} {n}");
        if !taken.iter().any(|t| *t == candidate) {
            return candidate;
        }
    }
    wanted.to_string()
}
