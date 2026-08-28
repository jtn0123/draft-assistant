//! Headless state dump: loads a league end-to-end and prints the same
//! DraftView JSON the app serves. Doubles as an engine integration test.
//!
//! Usage: dump_state <league_id> [username] [out.json] [--simulate N]
//!                   [--ask "question"]... [--chat-out session.json]
//!
//! --simulate N fakes the first N picks (market drafts by ADP; my own slots
//! take the engine's balanced recommendation) to exercise mid-draft state.
//!
//! --ask sends a question through the same Ask Claude path the app uses,
//! streaming the answer to stderr as it is written. Repeat it for a
//! conversation: each question sees the answers before it. --chat-out
//! records the exchange (question, answer, usage, as-of pick) so the browser
//! preview can replay a real session (`?chat=<url>`).

use draft_assistant_lib::chat::{ask, ChatOptions, ChatTurn};
use draft_assistant_lib::engine::{AppConfig, Engine};
use draft_assistant_lib::simulation::apply_simulated_pick;
use draft_assistant_lib::view::{build_view, DraftView};
use std::io::Write;

struct Args {
    league_id: String,
    username: Option<String>,
    out_path: Option<String>,
    simulate: u32,
    questions: Vec<String>,
    chat_out: Option<String>,
}

fn usage_exit() -> ! {
    eprintln!(
        "usage: dump_state <league_id> [username] [out.json] [--simulate N] \
         [--ask \"question\"]... [--chat-out session.json]"
    );
    std::process::exit(2);
}

fn parse_args() -> Args {
    let mut positional: Vec<String> = Vec::new();
    let mut simulate = 0u32;
    let mut questions = Vec::new();
    let mut chat_out = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--simulate" => {
                simulate = args.next().and_then(|n| n.parse().ok()).unwrap_or_else(|| {
                    eprintln!("--simulate needs a number");
                    std::process::exit(2);
                });
            }
            "--ask" => questions.push(args.next().unwrap_or_else(|| usage_exit())),
            "--chat-out" => chat_out = Some(args.next().unwrap_or_else(|| usage_exit())),
            _ => positional.push(arg),
        }
    }
    let Some(league_id) = positional.first().cloned() else {
        usage_exit();
    };
    Args {
        league_id,
        username: positional.get(1).cloned(),
        out_path: positional.get(2).cloned(),
        simulate,
        questions,
        chat_out,
    }
}

/// Run each question in turn, printing the answer as it streams, and return
/// the recorded exchanges.
async fn converse(view: &DraftView, questions: &[String]) -> Vec<serde_json::Value> {
    let options = ChatOptions::default();
    let mut history: Vec<ChatTurn> = Vec::new();
    let mut recorded = Vec::new();
    for question in questions {
        eprintln!("\n> {question}\n");
        let mut stderr = std::io::stderr();
        let mut on_text = |text: &str| {
            let _ = stderr.write_all(text.as_bytes());
            let _ = stderr.flush();
        };
        match ask(view, question, &history, &options, &mut on_text).await {
            Ok(reply) => {
                let u = &reply.usage;
                eprintln!(
                    "\n\n[{} · {} context tokens · {:.1} s · ${:.2}]",
                    u.model,
                    u.context_tokens,
                    u.duration_ms as f64 / 1000.0,
                    u.cost_usd.unwrap_or(0.0)
                );
                history.push(ChatTurn {
                    role: "you".into(),
                    text: question.clone(),
                });
                history.push(ChatTurn {
                    role: "claude".into(),
                    text: reply.answer.clone(),
                });
                recorded.push(serde_json::json!({
                    "question": question,
                    "answer": reply.answer,
                    "usage": reply.usage,
                    "as_of": reply.as_of,
                }));
            }
            Err(error) => {
                eprintln!("\nask failed: {error}");
                std::process::exit(1);
            }
        }
    }
    recorded
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let engine = match Engine::new(std::env::temp_dir().join("draft-assistant-cli")) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let mut config = AppConfig::default();
    if let Some(username) = &args.username {
        match engine.client.user_id(username).await {
            Ok(Some(user_id)) => config.my_user_id = Some(user_id),
            Ok(None) => eprintln!("warning: Sleeper user '{username}' not found"),
            Err(error) => eprintln!("warning: could not look up '{username}': {error}"),
        }
    }

    let mut loaded = match engine.load_any(&args.league_id, false).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("load failed: {e}");
            std::process::exit(1);
        }
    };

    for _ in 0..args.simulate {
        if apply_simulated_pick(&mut loaded, &config).is_none() {
            break;
        }
    }

    let view = build_view(&loaded, &config);
    let json = serde_json::to_string_pretty(&view).expect("serialize");
    match &args.out_path {
        Some(path) => {
            std::fs::write(path, &json).expect("write failed");
            eprintln!("wrote {path} ({} bytes)", json.len());
        }
        None if args.questions.is_empty() => println!("{json}"),
        None => {}
    }

    if !args.questions.is_empty() {
        let recorded = converse(&view, &args.questions).await;
        if let Some(path) = &args.chat_out {
            let json = serde_json::to_string_pretty(&recorded).expect("serialize");
            std::fs::write(path, &json).expect("write failed");
            eprintln!("wrote {path} ({} exchanges)", recorded.len());
        }
    }
}
