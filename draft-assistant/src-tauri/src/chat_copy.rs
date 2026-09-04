//! The words: the instruction Claude runs under, and the copy the panel puts
//! on its own controls.
//!
//! Kept apart from `chat.rs`, which is the wire — these change for editorial
//! reasons, not protocol ones, and a change here is a change to what the model
//! was told rather than to how it was asked.

use crate::chat::ChatModel;

/// The instruction the panel operates under. Kept separate from the volatile
/// board state so the stable half caches.
pub const GUIDANCE: &str = "\
You are a fantasy football draft and season assistant embedded in a read-only \
Sleeper second-screen app. You can see the user's live board, roster, and \
clock in the context below.

Answer in two or three short paragraphs at most. Lead with the recommendation, \
then the reasoning, then the risk. Cite the numbers you were given (points, \
VORP, tier, survival odds) rather than inventing any. If the context does not \
contain what you would need, say so plainly instead of guessing.

Take the league's house rules from the context as binding. Keepers have \
already spent the picks named; picks the user traded away are not theirs to \
plan around, and ones they acquired are. Where the order reverses instead of \
snaking, count the gap to their next pick from the order given, not from a \
plain snake. Round prices, where present, are what this draft has actually \
paid — use them for value, not a generic chart.

Kickers and defences are last-two-rounds picks, always. Their points over \
replacement look competitive from about the seventh round on, but that value \
is available to anyone who waits, so spending a pick on one before the last \
two rounds costs a startable body for nothing. Say so if the user is thinking \
about it, and do not put one forward as a best-available option earlier than \
that.

In season, the lineup rows are the user's BEST lineup, not the one they have \
set: if the two differ, say what to change. A player carrying an injury tag \
(Q, D, O) may not play at all — weigh that against the projection rather \
than reading the projection as settled.

This app cannot draft, set a lineup, or write anything to Sleeper. Never tell \
the user you have done something for them; tell them what to do.

Do not include internal or system XML tags in your response.";

/// Model / effort pairs the UI is allowed to offer.
pub fn effort_levels(model: ChatModel) -> Vec<&'static str> {
    if model.can_disable_thinking() {
        vec!["Off", "Low", "Medium", "High", "xhigh", "Max"]
    } else {
        // Fable 5 thinks on every turn; there is no off.
        vec!["Low", "Medium", "High", "xhigh", "Max"]
    }
}

/// Per-level copy for the tooltips and the footer note.
pub fn effort_note(effort: crate::chat::Effort) -> (&'static str, &'static str) {
    use crate::chat::Effort;
    match effort {
        Effort::Off => (
            "Adaptive thinking disabled — Claude answers without a reasoning pass",
            "no extended thinking",
        ),
        Effort::Low => (
            "Most efficient — significant token savings, some capability reduction",
            "low effort · fastest",
        ),
        Effort::Medium => (
            "Balanced — moderate token savings",
            "medium effort · balanced",
        ),
        Effort::High => (
            "Default — spends as many tokens as needed for excellent results",
            "high effort · the default",
        ),
        Effort::XHigh => (
            "For the hardest problems and long-horizon work",
            "xhigh effort · sustained reasoning",
        ),
        Effort::Max => (
            "No constraints on token spend — deepest analysis",
            "max effort · deepest, slowest",
        ),
    }
}

/// Redact a key down to what a user needs to recognise it by.
///
/// A real Anthropic key is a hundred-odd characters, and showing the `sk-ant-`
/// prefix with the last four is how the Console names them. Anything short
/// enough that those two windows would overlap into most of the string is
/// masked further instead: the four visible characters of a five-character
/// value are not a hint, they are the value. A short string here is a typo or
/// a test fixture rather than a real key, but it is still a secret, and the
/// masking cannot be a function of how good the secret is.
pub fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    // Eight or fewer: nothing is shown at all.
    if chars.len() <= 8 {
        return "····".to_string();
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    // The head is only worth showing once there is plenty of key hidden
    // between it and the tail.
    if chars.len() >= 20 {
        let head: String = chars[..7].iter().collect();
        return format!("{head}····{tail}");
    }
    format!("····{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_opus_offers_thinking_off() {
        assert!(effort_levels(ChatModel::Opus5).contains(&"Off"));
        assert!(!effort_levels(ChatModel::Fable5).contains(&"Off"));
    }

    #[test]
    fn a_real_key_shows_its_prefix_and_its_last_four() {
        // 21 characters: long enough that the two windows leave most of it
        // hidden between them.
        assert_eq!(mask_key("sk-ant-api03-abcd1234"), "sk-ant-····1234");
        assert_eq!(mask_key(&format!("sk-ant-api03-{}", "x".repeat(90))), {
            let tail = "x".repeat(4);
            format!("sk-ant-····{tail}")
        });
    }

    #[test]
    fn a_short_string_is_masked_rather_than_mostly_shown() {
        // These used to print four of the five characters, which is the
        // secret with a decoration on it.
        assert_eq!(mask_key("abcde"), "····");
        assert_eq!(mask_key("sk-ant12"), "····");
        // Between the two: the tail only, never the head.
        assert_eq!(mask_key("sk-ant123"), "····t123");
        // Nineteen characters: one short of the length that opens the head.
        assert_eq!(mask_key("sk-ant-api03-abc123"), "····c123");
        assert_eq!(mask_key(""), "····");
    }

    /// The two windows never meet in the middle, at any length.
    #[test]
    fn no_length_lets_the_windows_meet() {
        for len in 0..40usize {
            let key: String = ('a'..='z').cycle().take(len).collect();
            let masked = mask_key(&key);
            let shown = masked.chars().filter(|c| *c != '·').count();
            // Eleven characters at most — the seven-character head window
            // plus the four-character tail — and the head only opens once
            // there is a long key behind it to hide.
            assert!(shown <= 11, "{len}: {masked}");
            assert!(shown <= 4 || len >= 20, "{len}: {masked}");
            assert!(shown == 0 || len > 8, "{len}: {masked}");
            assert_ne!(masked, key, "{len}: the key was printed whole");
        }
    }

    #[test]
    fn the_guidance_carries_the_house_rules_and_the_set_versus_best_distinction() {
        // The context builds lines Claude can only use if it was told to. If
        // one half moves without the other, this is where it shows.
        assert!(
            GUIDANCE.contains("Keepers have already spent"),
            "{GUIDANCE}"
        );
        assert!(GUIDANCE.contains("traded away"), "{GUIDANCE}");
        assert!(GUIDANCE.contains("Round prices"), "{GUIDANCE}");
        assert!(GUIDANCE.contains("BEST lineup"), "{GUIDANCE}");
        assert!(GUIDANCE.contains("injury tag"), "{GUIDANCE}");
        // The board ranks kickers and defences down; the guidance has to say
        // why, or Claude argues them back up off their raw VORP.
        assert!(
            GUIDANCE.contains("Kickers and defences are last-two-rounds picks"),
            "{GUIDANCE}"
        );
    }
}
