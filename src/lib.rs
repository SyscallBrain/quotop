//! `quotop` — the library behind the quotop terminal UI, which shows the
//! remaining balance, credits and quotas of API services, read-only.
//!
//! The binary (`src/main.rs`) is a thin shell over these modules, so that the
//! integration tests can exercise the contract without going through the TUI.

pub mod cache;
pub mod config;
pub mod credentials;
pub mod engine;
pub mod guard;
pub mod http;
pub mod i18n;
pub mod keyfile;
pub mod model;
pub mod panic_hook;
pub mod providers;
pub mod secret;
pub mod tui;
