//! Error type shared by all ZWRS crates.
//!
//! ZWRS deliberately avoids third-party error crates; the enum below covers the
//! failure classes the engine actually produces and is small enough to match on.

use std::fmt;

/// Result alias used across ZWRS.
pub type Result<T> = std::result::Result<T, Error>;

/// Failure classes produced by the ZWRS engine.
#[derive(Debug)]
pub enum Error {
    /// An I/O operation failed.
    Io(std::io::Error),
    /// A configuration file or argument was invalid.
    Config(String),
    /// A JSON document was malformed.
    Json(String),
    /// A network protocol byte sequence was malformed.
    Protocol(String),
    /// A stored player/session record was malformed.
    Storage(String),
    /// A requested entity does not exist.
    NotFound(String),
    /// The engine is already running / not running.
    State(String),
    /// Feature is recognised but not implemented by this build.
    Unsupported(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Config(m) => write!(f, "configuration error: {m}"),
            Self::Json(m) => write!(f, "json error: {m}"),
            Self::Protocol(m) => write!(f, "protocol error: {m}"),
            Self::Storage(m) => write!(f, "storage error: {m}"),
            Self::NotFound(m) => write!(f, "not found: {m}"),
            Self::State(m) => write!(f, "invalid state: {m}"),
            Self::Unsupported(m) => write!(f, "unsupported: {m}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl Error {
    /// Stable machine-readable code, used in bridge responses.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::Config(_) => "config",
            Self::Json(_) => "json",
            Self::Protocol(_) => "protocol",
            Self::Storage(_) => "storage",
            Self::NotFound(_) => "not_found",
            Self::State(_) => "state",
            Self::Unsupported(_) => "unsupported",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable() {
        assert_eq!(Error::Config(String::new()).code(), "config");
        assert_eq!(Error::Unsupported(String::new()).code(), "unsupported");
    }

    #[test]
    fn display_includes_context() {
        let e = Error::Protocol("bad varint".to_string());
        assert!(e.to_string().contains("bad varint"));
    }
}
