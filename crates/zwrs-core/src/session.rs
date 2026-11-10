//! Connection sessions and playtime accounting.
//!
//! A session starts when a player is **authenticated and admitted** (not when a
//! TCP connection is accepted) and ends when they disconnect or the server
//! shuts down. Each session records its own start/end so operators can audit
//! playtime instead of trusting a single counter.
//!
//! # Crash recovery policy
//!
//! If the process dies while players are online, their sessions stay `active`
//! in `sessions.json`. On the next start [`SessionRegistry::recover_unfinished`]
//! closes them with `end = None`, `state = Unfinished`, and **does not** add
//! playtime, because the true logout time is unknown. Fabricating a duration
//! would corrupt playtime statistics, so the engine records the uncertainty and
//! lets `/playerinfo` show that the previous session ended abnormally. Profiles
//! are also forced offline so no player is reported online after a restart.

use crate::error::Result;
use crate::identity::{PlayerIdentity, Username, Uuid};
use crate::json::Json;
use crate::time::{parse_utc, Timestamp};

/// Lifecycle state of a recorded session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Player is connected right now.
    Active,
    /// Player disconnected normally; duration is known.
    Closed,
    /// Server stopped/crashed while the player was online; duration unknown.
    Unfinished,
}

impl SessionState {
    /// Stable label used in JSON and admin output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Closed => "closed",
            Self::Unfinished => "unfinished",
        }
    }

    /// Parses the label.
    ///
    /// # Errors
    /// Returns [`crate::Error::Storage`] for unknown labels.
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "active" => Ok(Self::Active),
            "closed" => Ok(Self::Closed),
            "unfinished" => Ok(Self::Unfinished),
            other => Err(crate::Error::Storage(format!(
                "unknown session state {other:?}"
            ))),
        }
    }
}

/// One player connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Monotonic-per-process identifier, stable in storage.
    id: u64,
    uuid: Uuid,
    username: Username,
    started_at: Timestamp,
    ended_at: Option<Timestamp>,
    state: SessionState,
    /// Protocol version the client reported.
    protocol: i32,
}

impl Session {
    /// Opens a new active session.
    #[must_use]
    pub fn open(id: u64, identity: &PlayerIdentity, started_at: Timestamp, protocol: i32) -> Self {
        Self {
            id,
            uuid: identity.uuid(),
            username: identity.username().clone(),
            started_at,
            ended_at: None,
            state: SessionState::Active,
            protocol,
        }
    }

    /// Session id.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Player UUID.
    #[must_use]
    pub const fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Username at session start.
    #[must_use]
    pub const fn username(&self) -> &Username {
        &self.username
    }

    /// Start timestamp.
    #[must_use]
    pub const fn started_at(&self) -> Timestamp {
        self.started_at
    }

    /// End timestamp, when known.
    #[must_use]
    pub const fn ended_at(&self) -> Option<Timestamp> {
        self.ended_at
    }

    /// State.
    #[must_use]
    pub const fn state(&self) -> SessionState {
        self.state
    }

    /// Protocol version.
    #[must_use]
    pub const fn protocol(&self) -> i32 {
        self.protocol
    }

    /// Whether the session is still active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self.state, SessionState::Active)
    }

    /// Recorded duration in seconds, or `None` when the duration is unknown
    /// (active or unfinished sessions).
    #[must_use]
    pub fn duration_seconds(&self) -> Option<u64> {
        self.ended_at
            .map(|end| self.started_at.saturating_until(end))
    }

    /// Closes the session normally, returning the credited duration.
    pub fn close(&mut self, at: Timestamp) -> u64 {
        if !self.is_active() {
            return 0;
        }
        let duration = self.started_at.saturating_until(at);
        self.ended_at = Some(at);
        self.state = SessionState::Closed;
        duration
    }

    /// Marks a session that was interrupted by a crash or hard shutdown.
    pub fn mark_unfinished(&mut self) {
        if self.is_active() {
            self.ended_at = None;
            self.state = SessionState::Unfinished;
        }
    }

    /// Serialises to JSON.
    #[must_use]
    pub fn to_json(&self) -> Json {
        let mut m = Json::object();
        m.insert("id".into(), Json::Number(self.id as f64));
        m.insert("uuid".into(), Json::String(self.uuid.to_string()));
        m.insert("username".into(), Json::String(self.username.to_string()));
        m.insert(
            "started_at".into(),
            Json::String(self.started_at.to_string()),
        );
        m.insert(
            "ended_at".into(),
            self.ended_at.map_or(Json::Null, |t| Json::String(t.to_string())),
        );
        m.insert("state".into(), Json::String(self.state.as_str().to_string()));
        m.insert("protocol".into(), Json::Number(f64::from(self.protocol)));
        m.insert(
            "duration_seconds".into(),
            self.duration_seconds().map_or(Json::Null, |d| Json::Number(d as f64)),
        );
        Json::Object(m)
    }

    /// Parses a session record.
    ///
    /// # Errors
    /// Returns [`crate::Error::Storage`] naming the invalid field.
    pub fn from_json(value: &Json) -> Result<Self> {
        fn string_field(value: &Json, key: &str) -> Result<String> {
            value
                .get(key)
                .and_then(Json::as_str)
                .map(str::to_string)
                .ok_or_else(|| crate::Error::Storage(format!("session `{key}` must be a string")))
        }
        let id = value
            .get("id")
            .and_then(Json::as_u64)
            .ok_or_else(|| crate::Error::Storage("session `id` must be an unsigned integer".into()))?;
        let uuid = Uuid::parse(&string_field(value, "uuid")?)?;
        let username = Username::parse(&string_field(value, "username")?)?;
        let started_at = parse_utc(&string_field(value, "started_at")?)
            .map(Timestamp::from_unix)
            .map_err(|e| crate::Error::Storage(format!("session `started_at`: {e}")))?;
        let ended_at = match value.get("ended_at") {
            Some(Json::Null) | None => None,
            Some(other) => {
                let text = other
                    .as_str()
                    .ok_or_else(|| crate::Error::Storage("session `ended_at` must be a timestamp or null".into()))?;
                Some(
                    parse_utc(text)
                        .map(Timestamp::from_unix)
                        .map_err(|e| crate::Error::Storage(format!("session `ended_at`: {e}")))?,
                )
            }
        };
        let state = SessionState::parse(&string_field(value, "state")?)?;
        let protocol = value
            .get("protocol")
            .and_then(Json::as_i64)
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| crate::Error::Storage("session `protocol` must be an integer".into()))?;

        if let Some(end) = ended_at {
            if end < started_at {
                return Err(crate::Error::Storage(format!(
                    "session {id}: ended_at precedes started_at"
                )));
            }
        }
        if state == SessionState::Closed && ended_at.is_none() {
            return Err(crate::Error::Storage(format!(
                "session {id}: state `closed` requires an `ended_at` timestamp"
            )));
        }
        if state == SessionState::Active && ended_at.is_some() {
            return Err(crate::Error::Storage(format!(
                "session {id}: state `active` must not have an `ended_at` timestamp"
            )));
        }

        Ok(Self {
            id,
            uuid,
            username,
            started_at,
            ended_at,
            state,
            protocol,
        })
    }
}

/// Registry of sessions for the running server.
#[derive(Debug)]
pub struct SessionRegistry {
    sessions: Vec<Session>,
    active: Vec<u64>,
    next_id: u64,
    /// Sessions kept in memory; older ones are dropped after being persisted.
    max_retained: usize,
}

impl Default for SessionRegistry {
    fn default() -> Self {
        Self::new(5_000)
    }
}

impl SessionRegistry {
    /// Creates a registry retaining at most `max_retained` records.
    #[must_use]
    pub fn new(max_retained: usize) -> Self {
        Self {
            sessions: Vec::new(),
            active: Vec::new(),
            next_id: 1,
            max_retained: max_retained.max(1),
        }
    }

    /// Rebuilds a registry from persisted sessions.
    #[must_use]
    pub fn from_sessions(sessions: Vec<Session>, max_retained: usize) -> Self {
        let next_id = sessions.iter().map(|s| s.id()).max().unwrap_or(0) + 1;
        let active = sessions
            .iter()
            .filter(|s| s.is_active())
            .map(Session::id)
            .collect();
        let mut registry = Self {
            sessions,
            active,
            next_id,
            max_retained: max_retained.max(1),
        };
        registry.trim();
        registry
    }

    /// All retained sessions, oldest first.
    #[must_use]
    pub fn all(&self) -> &[Session] {
        &self.sessions
    }

    /// Ids of currently active sessions.
    #[must_use]
    pub fn active_ids(&self) -> &[u64] {
        &self.active
    }

    /// Number of players currently online.
    #[must_use]
    pub fn online_count(&self) -> usize {
        self.active.len()
    }

    /// The active session for a player, if any.
    #[must_use]
    pub fn active_for(&self, uuid: Uuid) -> Option<&Session> {
        self.sessions
            .iter()
            .find(|s| s.is_active() && s.uuid() == uuid)
    }

    /// Opens a session unless the player already has an active one.
    ///
    /// Returns the new session id, or `None` when a session was already active
    /// (which also means the caller must not credit playtime twice).
    pub fn begin(&mut self, identity: &PlayerIdentity, now: Timestamp, protocol: i32) -> Option<u64> {
        if self.active_for(identity.uuid()).is_some() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.sessions.push(Session::open(id, identity, now, protocol));
        self.active.push(id);
        self.trim();
        Some(id)
    }

    /// Ends the active session for a player, returning the credited duration.
    ///
    /// `None` means there was no active session (nothing to credit).
    pub fn end(&mut self, uuid: Uuid, now: Timestamp) -> Option<(u64, u64)> {
        let id = *self.active.iter().find(|&&id| {
            self.sessions
                .iter()
                .any(|s| s.id() == id && s.uuid() == uuid)
        })?;
        self.active.retain(|&a| a != id);
        let session = self.sessions.iter_mut().find(|s| s.id() == id)?;
        let duration = session.close(now);
        Some((id, duration))
    }

    /// Closes every remaining active session as unfinished (crash recovery).
    ///
    /// Returns the ids that were affected.
    pub fn recover_unfinished(&mut self) -> Vec<u64> {
        let mut affected = Vec::new();
        for session in &mut self.sessions {
            if session.is_active() {
                session.mark_unfinished();
                affected.push(session.id());
            }
        }
        self.active.clear();
        affected
    }

    /// Sessions belonging to one player, newest first.
    #[must_use]
    pub fn history_for(&self, uuid: Uuid, limit: usize) -> Vec<&Session> {
        self.sessions
            .iter()
            .rev()
            .filter(|s| s.uuid() == uuid)
            .take(limit)
            .collect()
    }

    /// Total number of sessions ever recorded by this registry instance.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Whether any sessions are retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Snapshot for persistence.
    #[must_use]
    pub fn to_json_records(&self) -> Vec<Json> {
        self.sessions.iter().map(Session::to_json).collect()
    }

    fn trim(&mut self) {
        if self.sessions.len() <= self.max_retained {
            return;
        }
        let excess = self.sessions.len() - self.max_retained;
        // Never drop an active session, even if it is the oldest.
        let mut removed = 0;
        let mut keep = Vec::with_capacity(self.sessions.len());
        for session in self.sessions.drain(..) {
            if removed < excess && !session.is_active() {
                removed += 1;
            } else {
                keep.push(session);
            }
        }
        self.sessions = keep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(name: &str) -> PlayerIdentity {
        PlayerIdentity::offline(Username::parse(name).unwrap())
    }

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_unix(secs)
    }

    #[test]
    fn session_lifecycle() {
        let mut reg = SessionRegistry::default();
        let id = reg.begin(&ident("Notch"), at(100), 47).expect("session");
        assert_eq!(reg.online_count(), 1);
        assert!(reg.active_for(ident("Notch").uuid()).is_some());
        let (ended_id, duration) = reg.end(ident("Notch").uuid(), at(400)).expect("end");
        assert_eq!(ended_id, id);
        assert_eq!(duration, 300);
        assert_eq!(reg.online_count(), 0);
        assert_eq!(reg.history_for(ident("Notch").uuid(), 5).len(), 1);
    }

    #[test]
    fn no_duplicate_active_sessions() {
        let mut reg = SessionRegistry::default();
        assert!(reg.begin(&ident("Notch"), at(100), 47).is_some());
        assert!(reg.begin(&ident("Notch"), at(110), 47).is_none());
        assert_eq!(reg.online_count(), 1);
        assert!(reg.end(ident("Notch").uuid(), at(200)).is_some());
    }

    #[test]
    fn rapid_reconnect_creates_a_second_session() {
        let mut reg = SessionRegistry::default();
        reg.begin(&ident("Notch"), at(100), 47);
        reg.end(ident("Notch").uuid(), at(150));
        reg.begin(&ident("Notch"), at(151), 47);
        assert_eq!(reg.online_count(), 1);
        assert_eq!(reg.history_for(ident("Notch").uuid(), 10).len(), 2);
        // Newest first.
        let history = reg.history_for(ident("Notch").uuid(), 10);
        assert_eq!(history[0].started_at(), at(151));
    }

    #[test]
    fn ending_unknown_player_is_none() {
        let mut reg = SessionRegistry::default();
        assert!(reg.end(ident("Ghost").uuid(), at(1)).is_none());
    }

    #[test]
    fn crash_recovery_marks_unfinished_without_playtime() {
        let mut reg = SessionRegistry::default();
        reg.begin(&ident("Notch"), at(100), 47);
        reg.begin(&ident("Jeb"), at(120), 754);
        let affected = reg.recover_unfinished();
        assert_eq!(affected.len(), 2);
        assert_eq!(reg.online_count(), 0);
        for session in reg.all() {
            assert_eq!(session.state(), SessionState::Unfinished);
            assert_eq!(session.duration_seconds(), None);
        }
    }

    #[test]
    fn persisted_sessions_roundtrip() {
        let mut reg = SessionRegistry::default();
        reg.begin(&ident("Notch"), at(100), 47);
        reg.end(ident("Notch").uuid(), at(700));
        let records = reg.to_json_records();
        let text = Json::Array(records).to_pretty();
        let parsed = Json::parse(&text).unwrap();
        let sessions: Vec<Session> = parsed
            .as_array()
            .unwrap()
            .iter()
            .map(Session::from_json)
            .collect::<Result<_>>()
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].duration_seconds(), Some(600));
        assert_eq!(sessions[0].state(), SessionState::Closed);

        let rebuilt = SessionRegistry::from_sessions(sessions, 10);
        assert_eq!(rebuilt.online_count(), 0);
        // Ids continue from the restored maximum.
        let mut rebuilt = rebuilt;
        let id = rebuilt.begin(&ident("Jeb"), at(800), 47).unwrap();
        assert_eq!(id, 2);
    }

    #[test]
    fn active_sessions_restore_as_active() {
        let mut reg = SessionRegistry::default();
        reg.begin(&ident("Notch"), at(100), 47);
        let sessions: Vec<Session> = reg
            .to_json_records()
            .iter()
            .map(Session::from_json)
            .collect::<Result<_>>()
            .unwrap();
        let rebuilt = SessionRegistry::from_sessions(sessions, 10);
        assert_eq!(rebuilt.online_count(), 1);
    }

    #[test]
    fn malformed_sessions_rejected() {
        let good = Session::open(1, &ident("Notch"), at(100), 47).to_json();

        let mut m = good.as_object().unwrap().clone();
        m.insert("state".into(), Json::String("zombie".into()));
        assert!(Session::from_json(&Json::Object(m)).is_err());

        let mut m = good.as_object().unwrap().clone();
        m.insert("ended_at".into(), Json::String(Timestamp::from_unix(50).to_string()));
        m.insert("state".into(), Json::String("closed".into()));
        let err = Session::from_json(&Json::Object(m)).unwrap_err();
        assert!(err.to_string().contains("precedes"), "{err}");

        let mut m = good.as_object().unwrap().clone();
        m.insert("state".into(), Json::String("closed".into()));
        assert!(Session::from_json(&Json::Object(m)).is_err());

        let mut m = good.as_object().unwrap().clone();
        m.insert("uuid".into(), Json::String("garbage".into()));
        assert!(Session::from_json(&Json::Object(m)).is_err());
    }

    #[test]
    fn retention_never_drops_active_sessions() {
        let mut reg = SessionRegistry::new(2);
        reg.begin(&ident("Stay"), at(100), 47);
        for i in 0..10 {
            let who = ident(&format!("Player{i:02}"));
            reg.begin(&who, at(200 + i), 47);
            reg.end(who.uuid(), at(300 + i));
        }
        assert_eq!(reg.len(), 2);
        assert_eq!(reg.online_count(), 1, "the active session must survive trim");
        assert!(reg.active_for(ident("Stay").uuid()).is_some());
    }
}
