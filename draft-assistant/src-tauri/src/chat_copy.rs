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

/// Redact everything but the tail of a key, for display.
pub fn mask_key(key: &str) -> String {
    let visible: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if key.len() <= 4 {
        return "····".to_string();
    }
    format!("····{visible}")
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
    fn keys_are_masked_to_their_last_four() {
        assert_eq!(mask_key("sk-ant-api03-abcd1234"), "····1234");
        assert_eq!(mask_key("abc"), "····");
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
    }
}
