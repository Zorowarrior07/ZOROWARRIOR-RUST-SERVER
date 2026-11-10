//! ZWRS core primitives.
//!
//! This crate owns everything that is *not* the network: configuration,
//! logging, player identity, persistence, sessions and administration queries.
//! It has **no third-party dependencies** so it can be audited quickly and
//! built on constrained platforms (Android/Termux, CI sandboxes).
//!
//! # Modules
//!
//! * [`error`] — the shared error enum
//! * [`identity`] — UUID derivation (online and offline modes)
//! * [`json`] — the small JSON reader/writer used for storage
//! * [`time`] — UTC timestamp handling and the injectable [`time::Clock`]
//! * [`config`] — validated server configuration
//! * [`profile`] — persistent player profiles
//! * [`session`] — connection sessions and playtime accounting
//! * [`storage`] — atomic JSON persistence with corruption recovery
//! * [`admin`] — `/players`, `/playerinfo`, `/status` query surface

#![deny(missing_docs)]

pub mod admin;
pub mod config;
pub mod error;
pub mod identity;
pub mod json;
pub mod log;
pub mod md5;
pub mod profile;
pub mod sha256;
pub mod session;
pub mod storage;
pub mod time;

pub use error::{Error, Result};
pub use identity::{offline_uuid, IdentityMode, PlayerIdentity, Username, Uuid};
pub use json::Json;
pub use time::{Clock, SystemClock, Timestamp};

/// Crate version, taken from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Product name used in logs, MOTD and responses.
pub const PRODUCT: &str = "ZOROWARRIOR Rust Server";
