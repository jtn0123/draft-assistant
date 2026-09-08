//! Phone model choices. Provider credentials and configuration stay on the host.
use super::{routes::Auth, server::Srv};
use crate::{chat::ChatModel, chat_copy};
use axum::{extract::State, Json};
use std::sync::Arc;

const MODELS: [&str; 4] = ["Opus 5", "Fable 5.1", "GPT-6 Astra", "GPT-5.6 Sol"];

/// The word under a model's name on the phone: what picking it trades.
pub fn note(model: &str) -> &'static str {
    match model {
        "Fable 5.1" => "slower, smarter",
        "GPT-6 Astra" => "smarter",
        _ => "",
    }
}

#[derive(Clone, Debug)]
pub struct Choice {
    pub model: &'static str,
    pub effort: &'static str,
}

impl Choice {
    pub fn checked(model: &str, effort: &str) -> Result<Self, String> {
        let requested = if model.is_empty() { "Opus 5" } else { model };
        let parsed = ChatModel::parse(requested);
        let label = MODELS
            .into_iter()
            .find(|label| *label == requested || ChatModel::parse(label).id() == requested)
            .ok_or("unknown chat model")?;
        let requested = if effort.is_empty() { "High" } else { effort };
        let effort = chat_copy::effort_levels(parsed)
            .into_iter()
            .find(|level| level.eq_ignore_ascii_case(requested))
            .ok_or("that effort is not supported by this model")?;
        Ok(Self {
            model: label,
            effort,
        })
    }
}

pub async fn get_models(State(srv): State<Arc<Srv>>, _auth: Auth) -> Json<serde_json::Value> {
    let config = srv.state.config.lock().await.clone();
    let has_key = srv.state.engine.api_key(&config).await.is_some();
    let claude_cli = srv.state.engine.chat_cli(ChatModel::Opus5).is_some();
    let codex_cli = srv.state.engine.chat_cli(ChatModel::Sol).is_some();
    let provider = crate::commands_chat::resolve_provider(claude_cli);
    let models: Vec<_> = MODELS
        .into_iter()
        .map(|model| {
            let parsed = ChatModel::parse(model);
            let available = if parsed.is_openai() {
                codex_cli
            } else if provider == "claude_code" {
                claude_cli
            } else {
                has_key
            };
            serde_json::json!({"model": model, "available": available,
            "efforts": chat_copy::effort_levels(parsed), "note": note(model)})
        })
        .collect();
    Json(serde_json::json!({"models": models, "default_model": "Opus 5", "default_effort": "High"}))
}

#[cfg(test)]
mod tests {
    use super::Choice;
    #[test]
    fn selections_preserve_defaults_and_reject_silent_fallbacks() {
        let default = Choice::checked("", "").unwrap();
        assert_eq!((default.model, default.effort), ("Opus 5", "High"));
        for (input, label) in [
            ("gpt-6-astra", "GPT-6 Astra"),
            ("GPT-5.6 Sol", "GPT-5.6 Sol"),
            ("claude-fable-5-1", "Fable 5.1"),
        ] {
            let selected = Choice::checked(input, "low").unwrap();
            assert_eq!((selected.model, selected.effort), (label, "Low"));
        }
        assert!(Choice::checked("typo", "High").is_err());
        // The old label is not offered any more; a phone that saved it falls back.
        assert!(Choice::checked("Fable 5", "High").is_err());
        assert!(Choice::checked("GPT-6 Astra", "Off").is_err());
        assert!(Choice::checked("Fable 5.1", "Off").is_err());
        assert_eq!(super::note("Fable 5.1"), "slower, smarter");
        assert_eq!(super::note("GPT-6 Astra"), "smarter");
        assert_eq!(super::note("Opus 5"), "");
        assert!(Choice::checked("GPT-5.6 Sol", "Off").is_ok());
        assert!(Choice::checked("Opus 5", "unbounded").is_err());
    }
}
