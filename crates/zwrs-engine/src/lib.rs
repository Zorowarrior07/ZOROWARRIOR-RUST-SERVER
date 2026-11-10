//! ZWRS Rust engine.
//!
//! The engine is the part of ZWRS that actually runs the Minecraft server:
//! it owns the listening socket, speaks the Java Edition protocol, authenticates
//! or derives identities, persists player profiles and sessions, and reports its
//! real capabilities.
//!
//! # Embedding
//!
//! The engine is a normal Rust library so the JNI layer
//! (`crates/zwrs-jni`) and the standalone binary (`zwrs-engine`) can both host
//! it:
//!
//! ```no_run
//! use std::sync::Arc;
//! use zwrs_core::config::Config;
//! use zwrs_engine::{prepare, start};
//!
//! let state = prepare(Config::default()).expect("state");
//! let mut handle = start(Arc::clone(&state)).expect("listen");
//! assert!(handle.is_running());
//! handle.stop().expect("clean stop");
//! ```
//!
//! # Honest scope
//!
//! This crate implements connection establishment (handshake, status, login) and
//! the full player-data pipeline. It does **not** implement play-state packets,
//! and it does not execute plugins. [`plugins::capabilities`] returns the exact
//! list so tools never have to guess.

#![deny(missing_docs)]

pub mod admin;
pub mod auth;
pub mod events;
pub mod packet;
pub mod plugins;
pub mod server;
pub mod status;

pub use events::{CommandReply, HostListener, PlayerEvent};
pub use server::{prepare, shutdown_all, start, ServerHandle, SharedState};

/// Engine version (kept in step with the workspace version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Minecraft version advertised to clients and shown in logs.
///
/// This is the newest protocol ZWRS accepts; see
/// `zwrs_core::config::MAX_SUPPORTED_PROTOCOL` and `docs/COMPATIBILITY.md`.
pub const VERSION_NAME: &str = "1.21.8";

/// Build metadata reported by `/version` and the launcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    /// Crate version.
    pub version: &'static str,
    /// `target_os`/`target_arch` triple-ish description.
    pub platform: String,
    /// Whether this is a release or debug build.
    pub profile: &'static str,
    /// Oldest accepted protocol version.
    pub min_protocol: i32,
    /// Newest accepted protocol version.
    pub max_protocol: i32,
}

impl BuildInfo {
    /// Builds the info for the running binary.
    #[must_use]
    pub fn current() -> Self {
        Self {
            version: VERSION,
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            profile: if cfg!(debug_assertions) { "debug" } else { "release" },
            min_protocol: zwrs_core::config::MIN_SUPPORTED_PROTOCOL,
            max_protocol: zwrs_core::config::MAX_SUPPORTED_PROTOCOL,
        }
    }

    /// Renders an informational JSON document for bridge consumers.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut map = zwrs_core::Json::object();
        map.insert("version".into(), zwrs_core::Json::String(self.version.into()));
        map.insert("product".into(), zwrs_core::Json::String(zwrs_core::PRODUCT.into()));
        map.insert("platform".into(), zwrs_core::Json::String(self.platform.clone()));
        map.insert("profile".into(), zwrs_core::Json::String(self.profile.into()));
        map.insert(
            "min_protocol".into(),
            zwrs_core::Json::Number(f64::from(self.min_protocol)),
        );
        map.insert(
            "max_protocol".into(),
            zwrs_core::Json::Number(f64::from(self.max_protocol)),
        );
        zwrs_core::Json::Object(map).to_pretty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_info_is_consistent() {
        let info = BuildInfo::current();
        assert!(info.min_protocol <= info.max_protocol);
        let parsed = zwrs_core::Json::parse(&info.to_json()).expect("valid json");
        assert_eq!(
            parsed.get("version").and_then(zwrs_core::Json::as_str),
            Some(VERSION)
        );
        assert!(info.platform.contains('-'));
    }
}
