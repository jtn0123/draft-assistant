/// The application behind the desktop commands, Tauri-free and tested.
pub mod app;
pub mod app_season;
pub mod board;
pub mod chat;
/// The Tauri desktop shell. Optional so the domain library above can be built
/// and fuzzed without pulling in Tauri and its macro-generated command surface.
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod draft;
pub mod engine;
pub mod engine_season;
pub mod history;
pub mod lineup;
pub mod loaded;
pub mod log;
pub mod manual;
pub mod matchup;
mod mock_league;
pub mod picks;
pub mod playoffs;
pub mod recommend;
pub mod results;
pub mod roster;
pub mod scoring;
pub mod season;
pub mod simulation;
pub mod sleeper;
pub mod sleeper_id;
mod store;
pub mod trade;
pub mod trades;
pub mod transactions;
pub mod valuation;
pub mod view;
pub mod view_types;
pub mod waivers;

#[cfg(feature = "desktop")]
pub use desktop::run;
