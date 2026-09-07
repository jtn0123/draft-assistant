//! The rules of money and length for the Ask Claude commands: which screens
//! may ask, what a thread may carry, what the cap is, and where the spend is
//! filed. Pure functions, split out of `commands_chat.rs` for the line cap.

use crate::chat::{ChatMessage, ChatModel};
use crate::engine::AppConfig;

/// The most turns and the most text one question may carry.
pub(super) const MAX_TURNS: usize = 60;
pub(super) const MAX_THREAD_BYTES: usize = 200_000;

/// The tail of a conversation that is short enough to send.
///
/// A thread over the limit used to be refused outright, which left the shared
/// thread with no way out: it keeps two hundred entries, every device adds to
/// the same one, and a phone has no "new chat" button, so a league that asked
/// sixty questions on draft night could never ask a sixty-first. What goes to
/// the model is a window over the end of the thread instead — the last
/// [`MAX_TURNS`] turns that fit inside [`MAX_THREAD_BYTES`], trimmed from the
/// front and started on a user turn, because a conversation that opens on an
/// assistant turn is a 400.
pub(super) fn window(messages: &[ChatMessage]) -> Result<&[ChatMessage], String> {
    if messages.is_empty() {
        return Err("nothing to ask".to_string());
    }
    let bytes_from = |from: usize| {
        messages[from..]
            .iter()
            .map(|m| m.content.len())
            .sum::<usize>()
    };
    let mut start = messages.len().saturating_sub(MAX_TURNS);
    while start < messages.len() && bytes_from(start) > MAX_THREAD_BYTES {
        start += 1;
    }
    while start < messages.len() && messages[start].role != "user" {
        start += 1;
    }
    let windowed = &messages[start..];
    if windowed.is_empty() {
        // Nothing survived the trim: one turn on its own is over the byte
        // limit, and no window of the thread can carry it.
        return Err(format!(
            "that question is too long to send ({} KB), ask a shorter one",
            bytes_from(messages.len() - 1) / 1024
        ));
    }
    Ok(windowed)
}

/// The cap a screen's chat runs under until the user sets one of their own.
pub const DEFAULT_BUDGET_USD: f64 = 5.0;

/// The cap in force, in dollars. Zero means the user turned it off.
pub fn budget_of(config: &AppConfig) -> f64 {
    config
        .chat_budget_usd
        .unwrap_or(DEFAULT_BUDGET_USD)
        .max(0.0)
}

/// Refuse a turn whose screen has already spent its cap.
///
/// The check is necessarily *before* the turn, because what a turn costs is
/// only known once it has been answered. So a turn that starts under the cap
/// always finishes, however far over it lands — and then counts in full
/// against the cap, which the next turn is refused by. The cap is a stop, not
/// a ceiling; the overshoot is one turn wide.
pub(super) fn check_budget(spent: f64, cap: f64, screen: &str) -> Result<(), String> {
    if cap > 0.0 && spent >= cap {
        return Err(format!(
            "Ask Claude has spent ${spent:.2} of its ${cap:.2} cap on the {screen} screen. Raise the budget above to keep asking."
        ));
    }
    Ok(())
}

/// The cap a caller asked for, or a refusal.
///
/// A negative cap used to be quietly rounded up to zero — and zero is the one
/// value that means *no cap at all*, so "-1" turned the budget off instead of
/// being rejected. Nothing below zero is a cap, so nothing below zero is
/// accepted; the panel keeps whatever cap it had and says why.
pub(super) fn checked_budget(dollars: f64) -> Result<f64, String> {
    if !dollars.is_finite() {
        return Err("that is not a number of dollars".to_string());
    }
    if dollars < 0.0 {
        return Err(format!(
            "a budget cannot be negative (${dollars:.2}), 0 is the way to turn the cap off"
        ));
    }
    Ok(dollars)
}

/// The two screens that can ask a question.
///
/// `screen` picks the context, and further down it keys the running spend the
/// budget is checked against. Anything else would open a fresh, uncapped tally
/// under whatever name arrived over the IPC — and the config would grow a new
/// entry for every one of them.
pub(super) fn check_screen(screen: &str) -> Result<(), String> {
    match screen {
        "draft" | "season" => Ok(()),
        other => Err(format!("'{other}' is not a screen Ask Claude answers for")),
    }
}

/// Where one screen's running spend is filed: `screen.league_id`.
///
/// The same shape the panel files its saved conversations under (`chatScope`
/// in `chatSessions.ts`), and for the same reason — a question about one
/// league's board is not a question about another's. Spend used to be keyed by
/// screen alone, so every league on the machine drew down one shared cap and
/// the panel's "spent on this screen" figure belonged to no league in
/// particular.
///
/// Keys written under the old scheme are bare screen names, which no scope can
/// collide with. They are left in the config and never read: they are a
/// mixture of every league's spending, so there is no league to migrate them
/// to, and the alternative — charging them all to whichever league happens to
/// be open — would refuse turns over money that league never spent.
pub fn spend_key(screen: &str, league_id: Option<&str>) -> String {
    format!("{screen}.{}", league_id.unwrap_or("none"))
}

/// The league a turn is billed to: the one whose board the question is about.
///
/// The panel reads its "spent on this screen" figure under the league it is
/// showing — the loaded one, which is also the league the context below is
/// built from. The backend used to file the spend under
/// `config.active_league_id`, which is a record of what was last loaded rather
/// than what is loaded now; while a league is being switched the two disagree,
/// and the cap was then drawn down under a key the panel was not reading. Both
/// sides go through this one function so they cannot drift again.
pub(super) fn charged_league<'a>(
    loaded: Option<&'a str>,
    active: Option<&'a str>,
) -> Option<&'a str> {
    loaded.or(active)
}

/// What a turn is billed at.
///
/// The requested model is what the panel picked; the reported one is what
/// answered. Those differ whenever a server-side fallback rescues a refusal,
/// and pricing the answer as the request charged the wrong rate — under, if
/// Opus was asked for and Fable answered, so the cap let the next turn
/// through on money that had already been spent.
pub(super) fn billed_model(requested: ChatModel, reported: &str) -> ChatModel {
    ChatModel::from_reported(reported).unwrap_or(requested)
}
