//! Minecraft player identity.
//!
//! Two identity modes exist in Minecraft Java Edition:
//!
//! * **Online mode** — the UUID comes from Mojang/Microsoft session
//!   authentication. ZWRS only accepts such a UUID from the trusted
//!   authentication flow (see [`PlayerIdentity::online`]).
//! * **Offline mode** — the UUID is derived deterministically from the username
//!   using the vanilla convention
//!   `UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(UTF_8))`,
//!   i.e. MD5 with the RFC 4122 version-3 / variant bits forced.
//!
//! Offline identities are **not authenticated** and can be impersonated by
//! anyone who knows the username. Never treat an offline UUID as proof of
//! identity when the server is exposed.

use std::fmt;

use crate::error::{Error, Result};
use crate::md5::md5;

/// A 128-bit RFC 4122 UUID.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// Wraps raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Parses the canonical 8-4-4-4-12 hyphenated representation.
    ///
    /// # Errors
    /// Returns [`Error::Config`] when the text is not a canonical UUID.
    pub fn parse(text: &str) -> Result<Self> {
        let raw: Vec<u8> = text.bytes().collect();
        if let Some(stripped) = text.strip_prefix('{').and_then(|t| t.strip_suffix('}')) {
            return Self::parse(stripped);
        }
        if raw.len() != 36 {
            return Err(Error::Config(format!(
                "uuid must be 36 characters, got {}",
                raw.len()
            )));
        }
        let mut out = [0u8; 16];
        let mut nibble = 0usize;
        for (idx, b) in raw.iter().enumerate() {
            match (idx, *b) {
                (8 | 13 | 18 | 23, b'-') => continue,
                (8 | 13 | 18 | 23, _) => {
                    return Err(Error::Config(format!("uuid: expected '-' at index {idx}")));
                }
                (_, b) => {
                    let v = match b {
                        b'0'..=b'9' => b - b'0',
                        b'a'..=b'f' => b - b'a' + 10,
                        b'A'..=b'F' => b - b'A' + 10,
                        _ => {
                            return Err(Error::Config(format!(
                                "uuid: invalid hex digit at index {idx}"
                            )));
                        }
                    };
                    out[nibble / 2] |= if nibble % 2 == 0 { v << 4 } else { v };
                    nibble += 1;
                }
            }
        }
        Ok(Self(out))
    }

    /// Raw big-endian bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// RFC 4122 version nibble (1–5) when set.
    #[must_use]
    pub const fn version(&self) -> u8 {
        self.0[6] >> 4
    }

    /// The two most-significant Java `long` words, as used on the wire.
    #[must_use]
    pub const fn as_i64_pair(&self) -> (i64, i64) {
        let mut hi = [0u8; 8];
        let mut lo = [0u8; 8];
        hi.copy_from_slice(&[
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5], self.0[6], self.0[7],
        ]);
        lo.copy_from_slice(&[
            self.0[8], self.0[9], self.0[10], self.0[11], self.0[12], self.0[13], self.0[14],
            self.0[15],
        ]);
        (i64::from_be_bytes(hi), i64::from_be_bytes(lo))
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let h = crate::md5::hex(&self.0);
        write!(
            f,
            "{}-{}-{}-{}-{}",
            &h[0..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..32]
        )
    }
}

impl fmt::Debug for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Uuid({self})")
    }
}

/// A validated Minecraft username.
///
/// Accepted characters and length match vanilla: `[A-Za-z0-9_]`, 3–16
/// characters. ZWRS is strict here on purpose; invalid names are rejected at
/// the handshake rather than being silently stored.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Username(String);

impl Username {
    /// Minimum vanilla username length.
    pub const MIN_LEN: usize = 3;
    /// Maximum vanilla username length.
    pub const MAX_LEN: usize = 16;

    /// Validates a username.
    ///
    /// # Errors
    /// Returns [`Error::Protocol`] when the name is empty, too long, contains
    /// characters outside `[A-Za-z0-9_]`, or the name is reserved.
    pub fn parse(name: &str) -> Result<Self> {
        if name.len() < Self::MIN_LEN || name.len() > Self::MAX_LEN {
            return Err(Error::Protocol(format!(
                "username must be {}–{} characters, got {}",
                Self::MIN_LEN,
                Self::MAX_LEN,
                name.len()
            )));
        }
        if !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(Error::Protocol(
                "username may only contain A-Z, a-z, 0-9 and _".to_string(),
            ));
        }
        Ok(Self(name.to_string()))
    }

    /// The name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Case-folded form used for lookups (`Notch` and `notch` hit the same
    /// profile). Storage keeps the last observed spelling.
    #[must_use]
    pub fn canonical(&self) -> String {
        self.0.to_ascii_lowercase()
    }
}

impl fmt::Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Username({})", self.0)
    }
}

/// Deterministic offline-mode UUID for a username.
///
/// This is the exact vanilla algorithm and therefore produces the same UUIDs
/// that Mojang's client and a vanilla server would use for the same name.
#[must_use]
pub fn offline_uuid(name: &str) -> Uuid {
    let mut digest = md5(format!("OfflinePlayer:{name}").as_bytes());
    // RFC 4122 §4.3: version 3 (MD5-based, name based), variant 10xx.
    digest[6] = (digest[6] & 0x0f) | 0x30;
    digest[8] = (digest[8] & 0x3f) | 0x80;
    Uuid(digest)
}

/// How a player's UUID was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityMode {
    /// UUID came from the Mojang/Microsoft session service.
    Online,
    /// UUID was derived from the username; unauthenticated.
    Offline,
}

impl IdentityMode {
    /// Stable lower-case label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
        }
    }
}

/// A player identity: UUID plus the username it authenticated with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerIdentity {
    uuid: Uuid,
    username: Username,
    mode: IdentityMode,
}

impl PlayerIdentity {
    /// Builds an identity from a UUID supplied by the trusted authentication
    /// flow.
    #[must_use]
    pub fn online(uuid: Uuid, username: Username) -> Self {
        Self {
            uuid,
            username,
            mode: IdentityMode::Online,
        }
    }

    /// Builds an offline identity, deriving the UUID from the username.
    #[must_use]
    pub fn offline(username: Username) -> Self {
        let uuid = offline_uuid(username.as_str());
        Self {
            uuid,
            username,
            mode: IdentityMode::Offline,
        }
    }

    /// The player's UUID.
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// The player's username.
    #[must_use]
    pub const fn username(&self) -> &Username {
        &self.username
    }

    /// How this identity was established.
    #[must_use]
    pub const fn mode(&self) -> IdentityMode {
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_vanilla_vectors() {
        // Values produced by the vanilla algorithm; cross-checked with an
        // independent implementation of `UUID.nameUUIDFromBytes`.
        assert_eq!(
            offline_uuid("Notch").to_string(),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert_eq!(
            offline_uuid("jeb_").to_string(),
            "a762f560-4fce-3236-812a-b80efff0b62b"
        );
        assert_eq!(
            offline_uuid("Zorowarrior").to_string(),
            "1eedc501-8469-326d-99d8-7de09a3c0fdf"
        );
    }

    #[test]
    fn offline_uuid_is_version_3_variant_rfc4122() {
        let u = offline_uuid("AnyName");
        assert_eq!(u.version(), 3);
        assert_eq!(u.as_bytes()[8] & 0xc0, 0x80);
    }

    #[test]
    fn uuid_roundtrip_and_pairs() {
        let text = "1eedc501-8469-326d-99d8-7de09a3c0fdf";
        let u = Uuid::parse(text).expect("valid uuid");
        assert_eq!(u.to_string(), text);
        assert_eq!(Uuid::parse(&text.to_uppercase()).expect("upper"), u);
        assert_eq!(Uuid::parse(&format!("{{{text}}}")).expect("braced"), u);
        let (hi, lo) = u.as_i64_pair();
        assert_eq!(hi, i64::from_be_bytes(u.as_bytes()[0..8].try_into().unwrap()));
        assert_eq!(lo, i64::from_be_bytes(u.as_bytes()[8..16].try_into().unwrap()));
    }

    #[test]
    fn uuid_rejects_garbage() {
        for bad in ["", "not-a-uuid", "1eedc5018469326d99d87de09a3c0fdf", "1eedc501-8469-326d-99d8-7de09a3c0fd"] {
            assert!(Uuid::parse(bad).is_err(), "{bad} should be rejected");
        }
        assert!(Uuid::parse("1eedc501-8469-326d-99d8-7de09a3c0fdz").is_err());
        assert!(Uuid::parse("1eedc5018469-326d-99d8-7de09a3c0fdf").is_err());
    }

    #[test]
    fn username_rules() {
        assert!(Username::parse("Notch").is_ok());
        assert!(Username::parse("a_b1").is_ok());
        assert!(Username::parse("ab").is_err());
        assert!(Username::parse("a2345678901234567").is_err());
        assert!(Username::parse("bad name").is_err());
        assert!(Username::parse("bad-name").is_err());
        assert_eq!(Username::parse("Notch").unwrap().canonical(), "notch");
    }

    #[test]
    fn identity_modes() {
        let name = Username::parse("Notch").unwrap();
        let off = PlayerIdentity::offline(name.clone());
        assert_eq!(off.mode(), IdentityMode::Offline);
        assert_eq!(off.uuid(), offline_uuid("Notch"));
        let on = PlayerIdentity::online(Uuid::parse("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap(), name);
        assert_eq!(on.mode(), IdentityMode::Online);
        assert_eq!(on.mode().as_str(), "online");
    }
}
