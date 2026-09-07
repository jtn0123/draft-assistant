//! Where one turn's money is written down, and the task that writes it
//! whatever becomes of the caller. Split out of `commands_chat.rs` for the
//! line cap.

use super::PROVIDER_CLI;
use crate::chat::{self, ChatModel, ChatReply};
use crate::chat_client;
use crate::engine::AppConfig;
use tokio::sync::Mutex;

/// Where one turn's money is written down.
pub(crate) struct Books {
    pub(crate) config: std::sync::Arc<Mutex<AppConfig>>,
    pub(crate) engine: std::sync::Arc<crate::engine::Engine>,
    pub(crate) key: String,
    pub(crate) model: ChatModel,
    pub(crate) provider: &'static str,
}

impl Books {
    /// What a reply — or the billed part of a failed one — cost.
    ///
    /// The CLI route is paid for by a subscription, not by the token:
    /// charging it list rates would stop the panel over money nobody spent.
    fn cost_of(&self, reply: &ChatReply) -> f64 {
        if self.provider == PROVIDER_CLI {
            0.0
        } else {
            chat::turn_cost_of(super::billed_model(self.model, &reply.model), reply)
        }
    }

    /// Add `cost` to the running spend and return the new total.
    async fn record(&self, cost: f64) -> f64 {
        let mut config = self.config.lock().await;
        let running = config.chat_spend_usd.entry(self.key.clone()).or_insert(0.0);
        *running += cost;
        let running = *running;
        // A failure to write it down is not a reason to withhold the answer
        // the user already paid for; the next turn re-reads whatever did land.
        if let Err(e) = self.engine.save_config(&config) {
            crate::applog::warn(format!("could not record what Ask Claude spent: {e}"));
        }
        running
    }
}

/// Run the model call to its end and write down what it cost, whatever
/// becomes of the caller.
///
/// The call runs on a task of its own, so a caller that stops waiting — the
/// shared thread's answer limit, or a webview that went away — does not
/// cancel it. That matters because cancelling the future does not cancel the
/// bill: the API charges from the moment it accepts the request, and a turn
/// that was aborted at the await used to be billed, discarded, and never
/// counted against the cap. Here the spend is recorded by the same task that
/// made the call, before anything is handed back, and a call that fails after
/// the API started answering records the usage that did arrive.
pub(crate) async fn settle<F>(
    books: Books,
    in_flight: chat_client::InFlight,
    call: F,
) -> Result<ChatReply, String>
where
    F: std::future::Future<Output = Result<ChatReply, chat::ChatError>> + Send + 'static,
{
    let task = tokio::spawn(async move {
        // Released when the call ends, not when the caller stops waiting.
        let _in_flight = in_flight;
        match call.await {
            Ok(mut reply) => {
                reply.cost_usd = books.cost_of(&reply);
                reply.provider = books.provider.to_string();
                reply.screen_spend_usd = books.record(reply.cost_usd).await;
                Ok(reply)
            }
            Err(error) => {
                if let Some(partial) = &error.partial {
                    books.record(books.cost_of(partial)).await;
                }
                Err(error.message)
            }
        }
    });
    task.await
        .map_err(|_| "The answer stopped unexpectedly".to_string())?
}
