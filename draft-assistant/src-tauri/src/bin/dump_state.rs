//! Headless state dump: loads a league end-to-end and prints the same
//! DraftView JSON the app serves. Doubles as an engine integration test.
//!
//! Usage: dump_state <league_id> [username] [out.json] [--simulate N]
//!                   [--ask "question"]... [--chat-out session.json]
//!                   [--price "<slot>|<give ids>|<get ids>|<give rounds>|<get rounds>"]
//!
//! --simulate N fakes the first N picks (market drafts by ADP; my own slots
//! take the engine's balanced recommendation) to exercise mid-draft state.
//!
//! DRAFT_ASSISTANT_DATA_DIR overrides the cache directory (default: a
//! `draft-assistant-cli` folder under the system temp dir).
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
    price: Option<String>,
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
    let mut price = None;
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
            "--price" => price = Some(args.next().unwrap_or_else(|| usage_exit())),
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
        price,
    }
}

/// "2|9509,4034|4046|3|1" — partner slot, ids out, ids in, rounds out,
/// rounds in. Empty fields are allowed and mean nothing on that side.
fn parse_offer(spec: &str) -> (u32, Vec<String>, Vec<String>, Vec<u32>, Vec<u32>) {
    let mut parts = spec.split('|');
    let mut next = || parts.next().unwrap_or("").trim().to_string();
    let slot: u32 = next().parse().unwrap_or_else(|_| {
        eprintln!("--price starts with the partner's slot");
        std::process::exit(2);
    });
    let ids = |field: String| -> Vec<String> {
        field
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    };
    let rounds = |field: String| -> Vec<u32> {
        field
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect()
    };
    (
        slot,
        ids(next()),
        ids(next()),
        rounds(next()),
        rounds(next()),
    )
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
    let data_dir = std::env::var_os("DRAFT_ASSISTANT_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("draft-assistant-cli"));
    // The CLI already talks on stderr; this also leaves a file behind next
    // to the caches, so a scripted run can be read after the fact.
    draft_assistant_lib::log::init(&data_dir);
    let engine = match Engine::new(data_dir) {
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
        // With --ask or --price the interesting output is below, not a
        // 300 KB dump on top of it.
        None if args.questions.is_empty() && args.price.is_none() => println!("{json}"),
        None => {}
    }

    if let Some(spec) = &args.price {
        let (slot, give, get, give_picks, get_picks) = parse_offer(spec);
        let offer = draft_assistant_lib::trade::Offer {
            my_slot: view
                .draft
                .my_slot
                .expect("set a username to price an offer"),
            partner_slot: slot,
            give: &give,
            get: &get,
            give_picks: &give_picks,
            get_picks: &get_picks,
            week: view.this_week.as_ref().map_or(1, |w| w.week),
        };
        match draft_assistant_lib::trade::evaluate(
            &loaded,
            &view.rosters,
            &offer,
            &loaded.roster_rules,
        ) {
            Ok(v) => {
                let picks = |ps: &[draft_assistant_lib::pick_value::PickPrice]| {
                    ps.iter()
                        .map(|p| format!("R{} {:.0}", p.round, p.points))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let mine = v.my_season_after - v.my_season_before
                    + draft_assistant_lib::pick_value::total(&v.get_picks)
                    - draft_assistant_lib::pick_value::total(&v.give_picks);
                let theirs = v.their_season_after - v.their_season_before
                    + draft_assistant_lib::pick_value::total(&v.give_picks)
                    - draft_assistant_lib::pick_value::total(&v.get_picks);
                println!(
                    "with {} — me {mine:+.1}, them {theirs:+.1}\n  out: {} [{}]\n  in:  {} [{}]\n  week {}: me {:.1} -> {:.1}",
                    v.partner_name.as_deref().unwrap_or("?"),
                    v.give.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", "),
                    picks(&v.give_picks),
                    v.get.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", "),
                    picks(&v.get_picks),
                    v.week,
                    v.my_week_before,
                    v.my_week_after
                );
            }
            Err(error) => {
                eprintln!("price failed: {error}");
                std::process::exit(1);
            }
        }
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
