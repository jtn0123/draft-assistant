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

#[cfg(test)]
mod tests {
    use super::{display_name, unique_name};

    /// A phone that sends no name at all — an empty field, or one holding
    /// only spaces — used to appear in the device list as a blank row with a
    /// Forget button and nothing to identify it by.
    #[test]
    fn a_device_that_sends_no_name_is_still_called_something() {
        assert_eq!(display_name(""), "A device");
        assert_eq!(display_name("   "), "A device");
        assert_eq!(display_name("\t\n "), "A device");
        assert_eq!(display_name("  Rob's iPhone  "), "Rob's iPhone");
    }

    /// The name comes off the network, so its length is the sender's choice.
    /// The clamp counts characters rather than bytes: cutting a name mid
    /// character would leave the device list holding invalid text.
    #[test]
    fn a_long_name_is_clamped_to_sixty_characters_not_sixty_bytes() {
        assert_eq!(display_name(&"x".repeat(200)).chars().count(), 60);
        assert_eq!(display_name(&"x".repeat(60)), "x".repeat(60));
        let emoji = display_name(&"📱".repeat(100));
        assert_eq!(emoji.chars().count(), 60);
        assert_eq!(emoji, "📱".repeat(60));
    }

    /// Every iPhone guesses the same name for itself, so the second phone in
    /// a house needs one of its own.
    #[test]
    fn a_name_already_in_the_list_gets_the_next_number() {
        assert_eq!(unique_name("iPhone", &[]), "iPhone");
        assert_eq!(unique_name("iPhone", &["iPhone"]), "iPhone 2");
        assert_eq!(unique_name("iPhone", &["iPhone", "iPhone 2"]), "iPhone 3");
        // A gap is filled rather than skipped past.
        assert_eq!(unique_name("iPhone", &["iPhone", "iPhone 3"]), "iPhone 2");
        assert_eq!(unique_name("iPad", &["iPhone"]), "iPad");
    }
}
