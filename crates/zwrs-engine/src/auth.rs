//! Player authentication.
//!
//! # What is implemented, and what is not
//!
//! Online-mode authentication (Mojang/Microsoft session service) requires
//! HTTPS, RSA key exchange and an X.509 server keypair. **This build does not
//! implement it.** Rather than pretending, [`RejectingOnlineAuthenticator`]
//! refuses logins with an explicit, actionable message. Status pings and
//! protocol negotiation still work, so operators can verify reachability.
//!
//! With `online-mode = false` the engine uses [`OfflineAuthenticator`], which
//! applies the vanilla offline UUID derivation. That identity is **not
//! authenticated**: anyone who knows a username can use it.

use std::fmt;

use zwrs_core::identity::{IdentityMode, PlayerIdentity, Username};

/// Why a login attempt was not admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// Online-mode authentication is not available in this build.
    OnlineModeUnsupported,
    /// The username failed validation.
    InvalidUsername(String),
    /// The server is full.
    ServerFull {
        /// Configured maximum.
        max: u32,
    },
    /// Any other reason, already formatted for the player.
    Refused(String),
}

impl AuthError {
    /// Message shown in the client's disconnect screen.
    #[must_use]
    pub fn kick_reason(&self) -> String {
        match self {
            Self::OnlineModeUnsupported => concat!(
                "This ZOROWARRIOR Rust Server build does not implement online-mode ",
                "authentication yet, so it cannot verify your account.\n",
                "Set `online-mode = false` in config/zwrs.properties to allow ",
                "unauthenticated (offline) logins on a trusted network."
            )
            .to_string(),
            Self::InvalidUsername(msg) => format!("Invalid username: {msg}"),
            Self::ServerFull { max } => format!("The server is full ({max} players)."),
            Self::Refused(msg) => msg.clone(),
        }
    }

    /// Stable code for logs and the bridge API.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::OnlineModeUnsupported => "online_mode_unsupported",
            Self::InvalidUsername(_) => "invalid_username",
            Self::ServerFull { .. } => "server_full",
            Self::Refused(_) => "refused",
        }
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.kick_reason())
    }
}

impl std::error::Error for AuthError {}

/// Turns a claimed username into an identity, or refuses the login.
pub trait Authenticator: Send + Sync + fmt::Debug {
    /// Authenticates `username` for a client speaking `protocol`.
    ///
    /// # Errors
    /// Returns [`AuthError`] when the login must be refused.
    fn authenticate(&self, username: &str, protocol: i32) -> Result<PlayerIdentity, AuthError>;

    /// Short description used in startup logs and `/version`.
    fn describe(&self) -> &'static str;

    /// Identity mode this authenticator produces.
    fn mode(&self) -> IdentityMode;
}

/// Offline-mode authenticator: derives the UUID from the username.
#[derive(Debug, Clone, Copy, Default)]
pub struct OfflineAuthenticator;

impl Authenticator for OfflineAuthenticator {
    fn authenticate(&self, username: &str, _protocol: i32) -> Result<PlayerIdentity, AuthError> {
        let name = Username::parse(username)
            .map_err(|e| AuthError::InvalidUsername(e.to_string()))?;
        Ok(PlayerIdentity::offline(name))
    }

    fn describe(&self) -> &'static str {
        "offline mode (UUIDs derived from usernames; identities are NOT authenticated)"
    }

    fn mode(&self) -> IdentityMode {
        IdentityMode::Offline
    }
}

/// Online-mode authenticator that refuses every login, because session
/// authentication is not implemented.
#[derive(Debug, Clone, Copy, Default)]
pub struct RejectingOnlineAuthenticator;

impl Authenticator for RejectingOnlineAuthenticator {
    fn authenticate(&self, _username: &str, _protocol: i32) -> Result<PlayerIdentity, AuthError> {
        Err(AuthError::OnlineModeUnsupported)
    }

    fn describe(&self) -> &'static str {
        "online mode (authentication is NOT implemented in this build; logins are refused)"
    }

    fn mode(&self) -> IdentityMode {
        IdentityMode::Online
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zwrs_core::identity::offline_uuid;

    #[test]
    fn offline_mode_derives_vanilla_uuid() {
        let id = OfflineAuthenticator
            .authenticate("Notch", 47)
            .expect("offline login");
        assert_eq!(id.uuid(), offline_uuid("Notch"));
        assert_eq!(id.mode(), IdentityMode::Offline);
        assert!(OfflineAuthenticator.describe().contains("NOT authenticated"));
    }

    #[test]
    fn offline_mode_rejects_bad_names() {
        for bad in ["", "ab", "way_too_long_for_minecraft", "sp ace"] {
            let err = OfflineAuthenticator.authenticate(bad, 47).unwrap_err();
            assert_eq!(err.code(), "invalid_username");
        }
    }

    #[test]
    fn online_mode_refuses_and_explains() {
        let err = RejectingOnlineAuthenticator
            .authenticate("Notch", 763)
            .unwrap_err();
        assert_eq!(err.code(), "online_mode_unsupported");
        assert!(err.kick_reason().contains("online-mode = false"));
    }
}
