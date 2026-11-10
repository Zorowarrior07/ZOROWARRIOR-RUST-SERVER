//! Persistent player profiles.
//!
//! A profile is the durable record of a player identity. It is created on the
//! first **successful** join and updated on every join/logout. Values are never
//! synthesised: if a field has no real value the profile reports that honestly.
//!
//! Storage wiring lives in [`crate::admin::PlayerStore`]; this module owns the
//! data shape and its invariants.

use crate::error::{Error, Result};
use crate::identity::{IdentityMode, PlayerIdentity, Username, Uuid};
use crate::json::Json;
use crate::time::{parse_utc, Timestamp};

/// Durable record for one player identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerProfile {
    /// Player UUID (online-mode UUID or derived offline UUID).
    uuid: Uuid,
    /// Most recently observed username.
    username: Username,
    /// Usernames seen for this UUID, oldest first.
    previous_usernames: Vec<String>,
    /// First successful join.
    first_join: Timestamp,
    /// Most recent successful join.
    last_join: Timestamp,
    /// Most recent logout, if the player has ever left.
    last_logout: Option<Timestamp>,
    /// Accumulated playtime in whole seconds.
    total_playtime_seconds: u64,
    /// Number of successful joins.
    join_count: u64,
    /// Whether the player is online *right now* according to the engine.
    online: bool,
    /// Identity mode observed at the last join.
    identity_mode: IdentityMode,
    /// Last observed address, only when `store-ip-addresses` is enabled.
    last_ip: Option<String>,
}

impl PlayerProfile {
    /// Creates the profile for a first-ever join.
    #[must_use]
    pub fn new(identity: &PlayerIdentity, now: Timestamp, ip: Option<String>) -> Self {
        Self {
            uuid: identity.uuid(),
            username: identity.username().clone(),
            previous_usernames: Vec::new(),
            first_join: now,
            last_join: now,
            last_logout: None,
            total_playtime_seconds: 0,
            join_count: 1,
            online: true,
            identity_mode: identity.mode(),
            last_ip: ip,
        }
    }

    /// Records a join: updates username history, counters and online state.
    pub fn record_join(&mut self, identity: &PlayerIdentity, now: Timestamp, ip: Option<String>) {
        if identity.username() != &self.username {
            let old = self.username.to_string();
            if !self.previous_usernames.iter().any(|u| u.eq_ignore_ascii_case(&old)) {
                self.previous_usernames.push(old);
            }
            self.username = identity.username().clone();
        }
        self.last_join = now;
        self.online = true;
        self.join_count = self.join_count.saturating_add(1);
        self.identity_mode = identity.mode();
        if let Some(ip) = ip {
            self.last_ip = Some(ip);
        }
    }

    /// Records a logout and returns the session duration that was added to
    /// playtime. Calling this while offline is a no-op returning `0`.
    pub fn record_logout(&mut self, now: Timestamp) -> u64 {
        if !self.online {
            return 0;
        }
        let duration = self.last_join.saturating_until(now);
        self.total_playtime_seconds = self.total_playtime_seconds.saturating_add(duration);
        self.last_logout = Some(now);
        self.online = false;
        duration
    }

    /// UUID.
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Current username.
    #[must_use]
    pub const fn username(&self) -> &Username {
        &self.username
    }

    /// Previous usernames, oldest first.
    #[must_use]
    pub fn previous_usernames(&self) -> &[String] {
        &self.previous_usernames
    }

    /// First join timestamp.
    #[must_use]
    pub const fn first_join(&self) -> Timestamp {
        self.first_join
    }

    /// Last join timestamp.
    #[must_use]
    pub const fn last_join(&self) -> Timestamp {
        self.last_join
    }

    /// Last logout timestamp.
    #[must_use]
    pub const fn last_logout(&self) -> Option<Timestamp> {
        self.last_logout
    }

    /// Total playtime in seconds.
    #[must_use]
    pub const fn total_playtime_seconds(&self) -> u64 {
        self.total_playtime_seconds
    }

    /// Join count.
    #[must_use]
    pub const fn join_count(&self) -> u64 {
        self.join_count
    }

    /// Whether the profile is marked online.
    #[must_use]
    pub const fn online(&self) -> bool {
        self.online
    }

    /// Identity mode from the last join.
    #[must_use]
    pub const fn identity_mode(&self) -> IdentityMode {
        self.identity_mode
    }

    /// Last observed address, when IP storage is enabled.
    #[must_use]
    pub fn last_ip(&self) -> Option<&str> {
        self.last_ip.as_deref()
    }

    /// Clears the stored address (used when IP storage is disabled).
    pub fn clear_ip(&mut self) {
        self.last_ip = None;
    }

    /// Marks the profile offline without counting playtime.
    ///
    /// Used by crash recovery, where the true logout time is unknown.
    pub fn force_offline(&mut self) {
        self.online = false;
    }

    /// Serialises the profile to JSON.
    #[must_use]
    pub fn to_json(&self) -> Json {
        let mut m = Json::object();
        m.insert("uuid".into(), Json::String(self.uuid.to_string()));
        m.insert("username".into(), Json::String(self.username.to_string()));
        m.insert(
            "previous_usernames".into(),
            Json::Array(
                self.previous_usernames
                    .iter()
                    .map(|u| Json::String(u.clone()))
                    .collect(),
            ),
        );
        m.insert("first_join".into(), Json::String(self.first_join.to_string()));
        m.insert("last_join".into(), Json::String(self.last_join.to_string()));
        m.insert(
            "last_logout".into(),
            self.last_logout.map_or(Json::Null, |t| Json::String(t.to_string())),
        );
        m.insert(
            "total_playtime_seconds".into(),
            Json::Number(self.total_playtime_seconds as f64),
        );
        m.insert("join_count".into(), Json::Number(self.join_count as f64));
        m.insert("online".into(), Json::Bool(self.online));
        m.insert(
            "identity_mode".into(),
            Json::String(self.identity_mode.as_str().to_string()),
        );
        m.insert(
            "last_ip".into(),
            self.last_ip.as_deref().map_or(Json::Null, |ip| Json::String(ip.to_string())),
        );
        Json::Object(m)
    }

    /// Parses a profile, rejecting documents with missing or invalid fields.
    ///
    /// # Errors
    /// Returns [`Error::Storage`] naming the offending field.
    pub fn from_json(value: &Json) -> Result<Self> {
        fn field<'a>(value: &'a Json, key: &str) -> Result<&'a Json> {
            value
                .get(key)
                .ok_or_else(|| Error::Storage(format!("player profile is missing `{key}`")))
        }
        fn string_field(value: &Json, key: &str) -> Result<String> {
            field(value, key)?
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| Error::Storage(format!("player profile `{key}` must be a string")))
        }
        fn time_field(value: &Json, key: &str) -> Result<Timestamp> {
            let text = string_field(value, key)?;
            parse_utc(&text).map(Timestamp::from_unix).map_err(|e| {
                Error::Storage(format!("player profile `{key}` is not a valid timestamp: {e}"))
            })
        }
        fn uint_field(value: &Json, key: &str) -> Result<u64> {
            field(value, key)?
                .as_u64()
                .ok_or_else(|| Error::Storage(format!("player profile `{key}` must be a non-negative integer")))
        }

        let uuid = Uuid::parse(&string_field(value, "uuid")?)?;
        let username = Username::parse(&string_field(value, "username")?)
            .map_err(|e| Error::Storage(format!("player profile `username`: {e}")))?;
        // Order is meaningful (oldest first), so de-duplicate without sorting.
        let mut previous_usernames: Vec<String> = Vec::new();
        if let Some(arr) = field(value, "previous_usernames").ok().and_then(Json::as_array) {
            for item in arr {
                if let Some(name) = item.as_str() {
                    if !previous_usernames.iter().any(|u| u == name) {
                        previous_usernames.push(name.to_string());
                    }
                }
            }
        }
        let first_join = time_field(value, "first_join")?;
        let last_join = time_field(value, "last_join")?;
        let last_logout = match field(value, "last_logout")? {
            Json::Null => None,
            other => {
                let text = other.as_str().ok_or_else(|| {
                    Error::Storage("player profile `last_logout` must be a timestamp or null".into())
                })?;
                Some(
                    parse_utc(text)
                        .map(Timestamp::from_unix)
                        .map_err(|e| Error::Storage(format!("player profile `last_logout`: {e}")))?,
                )
            }
        };
        let total_playtime_seconds = uint_field(value, "total_playtime_seconds")?;
        let join_count = uint_field(value, "join_count")?;
        let online = field(value, "online")?
            .as_bool()
            .ok_or_else(|| Error::Storage("player profile `online` must be a boolean".into()))?;
        let identity_mode = match string_field(value, "identity_mode")?.as_str() {
            "online" => IdentityMode::Online,
            "offline" => IdentityMode::Offline,
            other => {
                return Err(Error::Storage(format!(
                    "player profile `identity_mode` must be `online` or `offline`, got {other:?}"
                )));
            }
        };
        let last_ip = match field(value, "last_ip") {
            Ok(Json::Null) | Err(_) => None,
            Ok(other) => Some(
                other
                    .as_str()
                    .ok_or_else(|| Error::Storage("player profile `last_ip` must be a string or null".into()))?
                    .to_string(),
            ),
        };

        if last_join < first_join {
            return Err(Error::Storage(format!(
                "player profile {uuid}: last_join precedes first_join"
            )));
        }
        if let Some(logout) = last_logout {
            if logout < first_join {
                return Err(Error::Storage(format!(
                    "player profile {uuid}: last_logout precedes first_join"
                )));
            }
        }

        Ok(Self {
            uuid,
            username,
            previous_usernames,
            first_join,
            last_join,
            last_logout,
            total_playtime_seconds,
            join_count,
            online,
            identity_mode,
            last_ip,
        })
    }

    /// Human-readable summary used by `/playerinfo`.
    ///
    /// # Arguments
    /// * `now` — current time, used to show live playtime for online players.
    /// * `session_age` — for an online player, seconds since the current
    ///   session started; `None` when offline or unknown.
    #[must_use]
    pub fn summary(&self, now: Timestamp, session_age: Option<u64>) -> Vec<String> {
        let mut lines = Vec::new();
        lines.push(format!("UUID: {}", self.uuid));
        lines.push(format!("Username: {}", self.username));
        if !self.previous_usernames.is_empty() {
            lines.push(format!(
                "Previously known as: {}",
                self.previous_usernames.join(", ")
            ));
        }
        lines.push(format!("Identity mode: {}", self.identity_mode.as_str()));
        lines.push(format!("First join: {}", self.first_join));
        lines.push(format!("Last join: {}", self.last_join));
        lines.push(format!(
            "Last logout: {}",
            self.last_logout.map_or_else(|| "never".to_string(), |t| t.to_string())
        ));
        let total = self
            .total_playtime_seconds
            .saturating_add(session_age.unwrap_or(0));
        lines.push(format!(
            "Playtime: {} ({total} seconds)",
            format_duration(total)
        ));
        lines.push(format!("Join count: {}", self.join_count));
        lines.push(format!(
            "Online: {}",
            if self.online { "yes" } else { "no" }
        ));
        if let Some(ip) = &self.last_ip {
            lines.push(format!("Last IP: {ip}"));
        }
        if self.online {
            if let Some(age) = session_age {
                lines.push(format!("Current session: {}", format_duration(age)));
            }
        } else {
            let _ = now;
        }
        lines
    }
}

/// Formats seconds as `1d 02h 03m 04s` (omitting leading zero units).
#[must_use]
pub fn format_duration(seconds: u64) -> String {
    let d = seconds / 86_400;
    let h = (seconds % 86_400) / 3600;
    let m = (seconds % 3600) / 60;
    let s = seconds % 60;
    if d > 0 {
        format!("{d}d {h:02}h {m:02}m {s:02}s")
    } else if h > 0 {
        format!("{h}h {m:02}m {s:02}s")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::offline_uuid;

    fn identity(name: &str) -> PlayerIdentity {
        PlayerIdentity::offline(Username::parse(name).unwrap())
    }

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_unix(secs)
    }

    #[test]
    fn new_profile_is_online_and_counts_first_join() {
        let p = PlayerProfile::new(&identity("Notch"), at(1_000), None);
        assert!(p.online());
        assert_eq!(p.join_count(), 1);
        assert_eq!(p.total_playtime_seconds(), 0);
        assert_eq!(p.uuid(), offline_uuid("Notch"));
        assert_eq!(p.last_logout(), None);
    }

    #[test]
    fn join_logout_accumulates_playtime() {
        let mut p = PlayerProfile::new(&identity("Notch"), at(1_000), None);
        assert_eq!(p.record_logout(at(1_600)), 600);
        assert_eq!(p.total_playtime_seconds(), 600);
        assert!(!p.online());

        p.record_join(&identity("Notch"), at(2_000), None);
        assert!(p.online());
        assert_eq!(p.join_count(), 2);
        assert_eq!(p.record_logout(at(2_250)), 250);
        assert_eq!(p.total_playtime_seconds(), 850);
    }

    #[test]
    fn duplicate_logout_does_not_double_count() {
        let mut p = PlayerProfile::new(&identity("Notch"), at(1_000), None);
        assert_eq!(p.record_logout(at(1_100)), 100);
        assert_eq!(p.record_logout(at(9_999)), 0);
        assert_eq!(p.total_playtime_seconds(), 100);
    }

    #[test]
    fn username_changes_are_tracked() {
        let mut p = PlayerProfile::new(&identity("Notch"), at(1_000), None);
        p.record_logout(at(1_100));
        p.record_join(&identity("Notch2"), at(2_000), None);
        assert_eq!(p.username().as_str(), "Notch2");
        assert_eq!(p.previous_usernames(), &["Notch".to_string()]);
    }

    #[test]
    fn json_roundtrip_preserves_everything() {
        let mut p = PlayerProfile::new(&identity("Notch"), at(1_700_000_000), Some("203.0.113.7".into()));
        p.record_logout(at(1_700_003_600));
        let text = p.to_json().to_pretty();
        let parsed = PlayerProfile::from_json(&Json::parse(&text).unwrap()).unwrap();
        assert_eq!(parsed, p);
        assert_eq!(parsed.total_playtime_seconds(), 3_600);
        assert_eq!(parsed.last_ip(), Some("203.0.113.7"));
    }

    #[test]
    fn malformed_profiles_are_rejected_with_field_context() {
        let base = PlayerProfile::new(&identity("Notch"), at(1_000), None).to_json();

        let mut missing = base.as_object().unwrap().clone();
        missing.remove("uuid");
        let err = PlayerProfile::from_json(&Json::Object(missing)).unwrap_err();
        assert!(err.to_string().contains("uuid"), "{err}");

        let mut bad_uuid = base.as_object().unwrap().clone();
        bad_uuid.insert("uuid".into(), Json::String("nope".into()));
        assert!(PlayerProfile::from_json(&Json::Object(bad_uuid)).is_err());

        let mut bad_version = base.as_object().unwrap().clone();
        bad_version.insert("identity_mode".into(), Json::String("maybe".into()));
        assert!(PlayerProfile::from_json(&Json::Object(bad_version)).is_err());

        let mut bad_time = base.as_object().unwrap().clone();
        bad_time.insert("first_join".into(), Json::String("yesterday".into()));
        let err = PlayerProfile::from_json(&Json::Object(bad_time)).unwrap_err();
        assert!(err.to_string().contains("first_join"), "{err}");

        let mut negative = base.as_object().unwrap().clone();
        negative.insert("join_count".into(), Json::Number(-1.0));
        assert!(PlayerProfile::from_json(&Json::Object(negative)).is_err());
    }

    #[test]
    fn impossible_timelines_rejected() {
        let p = PlayerProfile::new(&identity("Notch"), at(2_000), None).to_json();
        // last_join before first_join
        let mut m = p.as_object().unwrap().clone();
        m.insert(
            "last_join".into(),
            Json::String(Timestamp::from_unix(500).to_string()),
        );
        assert!(PlayerProfile::from_json(&Json::Object(m)).is_err());
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(59), "59s");
        assert_eq!(format_duration(61), "1m 01s");
        assert_eq!(format_duration(3_661), "1h 01m 01s");
        assert_eq!(format_duration(90_061), "1d 01h 01m 01s");
    }

    #[test]
    fn summary_reports_live_session_zero_safely() {
        let p = PlayerProfile::new(&identity("Notch"), at(1_000), None);
        let lines = p.summary(at(1_500), Some(500)).join("\n");
        assert!(lines.contains("Online: yes"), "{lines}");
        assert!(lines.contains("500 seconds"), "{lines}");
        assert!(lines.contains("Identity mode: offline"), "{lines}");
    }
}
