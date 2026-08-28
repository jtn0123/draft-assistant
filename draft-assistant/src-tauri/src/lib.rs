/// The application behind the desktop commands, Tauri-free and tested.
pub mod app;
pub mod board;
pub mod chat;
/// The Tauri desktop shell. Optional so the domain library above can be built
/// and fuzzed without pulling in Tauri and its macro-generated command surface.
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod draft;
pub mod engine;
pub mod manual;
mod mock_league;
pub mod recommend;
pub mod roster;
pub mod scoring;
pub mod simulation;
pub mod sleeper;
mod store;
pub mod valuation;
pub mod view;

#[cfg(feature = "desktop")]
pub use desktop::run;
