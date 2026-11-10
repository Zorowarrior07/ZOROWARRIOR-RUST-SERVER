//! Levelled logging.
//!
//! The engine writes to stdout/stderr; the Java launcher captures those streams
//! and mirrors them into its own logger, so no logging framework is needed.
//! `RUST_LOG` (or the `log-level` config key) controls verbosity.

use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};

/// Log severity, ordered from most to least severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Unrecoverable problem; the engine stops.
    Error = 1,
    /// Something failed but the engine keeps running.
    Warn = 2,
    /// High-level lifecycle events (default).
    Info = 3,
    /// Per-connection and per-tick detail.
    Debug = 4,
    /// Packet-level tracing.
    Trace = 5,
}

impl Level {
    /// Parses a level name (case-insensitive).
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Self::Error),
            "warn" | "warning" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }

    /// Lower-case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        }
    }
}

static LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

/// Sets the global log level.
pub fn set_level(level: Level) {
    LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Current global log level.
#[must_use]
pub fn level() -> Level {
    match LEVEL.load(Ordering::Relaxed) {
        1 => Level::Error,
        2 => Level::Warn,
        4 => Level::Debug,
        5 => Level::Trace,
        _ => Level::Info,
    }
}

/// Whether a message at `level` would be emitted.
#[must_use]
pub fn enabled(level: Level) -> bool {
    level <= self::level()
}

/// Emits a log line when `level` is enabled.
///
/// Format: `[HH:MM:SS] [LEVEL] target: message`
pub fn log(level: Level, target: &str, message: &str) {
    if !enabled(level) {
        return;
    }
    let now = crate::time::Timestamp::now();
    let stamp = now.to_string();
    // `2026-11-10T10:00:00Z` -> `10:00:00`
    let time = stamp.get(11..19).unwrap_or("--:--:--");
    let line = format!("[{time}] [{}] {target}: {message}\n", level.as_str());
    // Logging must never panic or abort the server.
    let _ = if level <= Level::Warn {
        std::io::stderr().write_all(line.as_bytes())
    } else {
        std::io::stdout().write_all(line.as_bytes())
    };
}

/// Logs at [`Level::Error`].
#[macro_export]
macro_rules! log_error {
    ($target:expr, $($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Error, $target, &format!($($arg)*))
    };
}

/// Logs at [`Level::Warn`].
#[macro_export]
macro_rules! log_warn {
    ($target:expr, $($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Warn, $target, &format!($($arg)*))
    };
}

/// Logs at [`Level::Info`].
#[macro_export]
macro_rules! log_info {
    ($target:expr, $($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Info, $target, &format!($($arg)*))
    };
}

/// Logs at [`Level::Debug`].
#[macro_export]
macro_rules! log_debug {
    ($target:expr, $($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Debug, $target, &format!($($arg)*))
    };
}

/// Logs at [`Level::Trace`].
#[macro_export]
macro_rules! log_trace {
    ($target:expr, $($arg:tt)*) => {
        $crate::log::log($crate::log::Level::Trace, $target, &format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_ordering_and_parsing() {
        assert!(Level::Error < Level::Warn);
        assert!(Level::Info < Level::Debug);
        assert_eq!(Level::parse("WARN"), Some(Level::Warn));
        assert_eq!(Level::parse(" info "), Some(Level::Info));
        assert_eq!(Level::parse("verbose"), None);
        assert_eq!(Level::Error.as_str(), "ERROR");
    }

    #[test]
    fn enabled_respects_threshold() {
        set_level(Level::Warn);
        assert!(enabled(Level::Error));
        assert!(enabled(Level::Warn));
        assert!(!enabled(Level::Info));
        set_level(Level::Info);
    }
}
