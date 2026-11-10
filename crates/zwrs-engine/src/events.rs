//! Host hooks: the seam between the Rust engine and whatever embeds it.
//!
//! The engine does not know what a plugin is. It knows that when a player's
//! login is admitted (after the profile and session have been persisted) and
//! when that session ends, somebody may want to know. That "somebody" is a
//! [`HostListener`], normally the Java plugin host reached through
//! `crates/zwrs-jni`.
//!
//! # Contract
//!
//! * Hooks are called **after** the durable state is written, so a plugin can
//!   never observe a join that was not persisted, or credit playtime twice.
//! * Hooks run on the connection thread with no engine lock held. They may read
//!   state but must not call back into the engine synchronously (doing so is
//!   documented as a deadlock risk; the JNI host only queues work).
//! * A hook that panics is contained: the engine converts it into an error log
//!   and carries on serving.
//! * With no listener installed (the standalone binary, or Java without the
//!   plugin host) every hook is a no-op. Nothing is faked.

/// One player lifecycle event, as reported to the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerEvent {
    /// Canonical UUID string (dashed).
    pub uuid: String,
    /// Username as observed at login.
    pub username: String,
    /// Protocol version the client reported.
    pub protocol: i32,
    /// Remote address (`ip:port`).
    pub address: String,
    /// UTC timestamp of the join or the quit (ISO-8601, `Z` suffix).
    pub timestamp: String,
    /// Online player count at the moment the event fired.
    pub online_count: usize,
    /// Seconds the session lasted; always `0` for a join.
    pub session_seconds: u64,
}

impl PlayerEvent {
    /// Renders the event as a JSON object for the JNI host.
    ///
    /// Built with the workspace JSON writer, never by string concatenation, so a
    /// username containing quotes or control characters cannot break the
    /// document.
    #[must_use]
    pub fn to_json(&self) -> String {
        use zwrs_core::json::Json;
        let mut object = Json::object();
        object.insert("uuid".to_string(), Json::String(self.uuid.clone()));
        object.insert("username".to_string(), Json::String(self.username.clone()));
        object.insert("protocol".to_string(), Json::Number(f64::from(self.protocol)));
        object.insert("address".to_string(), Json::String(self.address.clone()));
        object.insert("timestamp".to_string(), Json::String(self.timestamp.clone()));
        object.insert(
            "online_count".to_string(),
            Json::Number(self.online_count as f64),
        );
        object.insert(
            "session_seconds".to_string(),
            Json::Number(self.session_seconds as f64),
        );
        Json::Object(object).to_compact()
    }
}

/// What a host wants to do with a command the engine did not handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandReply {
    /// The host handled it; these are the lines to print.
    Handled(Vec<String>),
    /// The host does not know this command either.
    Unknown,
}

/// Receives engine lifecycle events and console commands.
///
/// Implementations must be cheap and non-blocking: they are called on the
/// connection thread (events) or the console thread (commands).
pub trait HostListener: Send + Sync {
    /// A login was admitted and its profile/session are on disk.
    fn player_join(&self, event: &PlayerEvent);

    /// The session ended and playtime was credited.
    fn player_quit(&self, event: &PlayerEvent);

    /// A console command the engine does not implement itself.
    ///
    /// `command` is lower-cased and has no leading slash; `args` is the rest of
    /// the line split on whitespace.
    fn command(&self, command: &str, args: &[String]) -> CommandReply;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PlayerEvent {
        PlayerEvent {
            uuid: "b50ad385-829d-3141-a216-7e7d7539ba7f".to_string(),
            username: "Notch\"quoted".to_string(),
            protocol: 772,
            address: "127.0.0.1:51234".to_string(),
            timestamp: "2026-11-10T10:00:00Z".to_string(),
            online_count: 3,
            session_seconds: 42,
        }
    }

    #[test]
    fn event_json_round_trips_through_the_reader() {
        let text = sample().to_json();
        let parsed = zwrs_core::json::Json::parse(&text).expect("valid JSON");
        let object = parsed.as_object().expect("object");
        assert_eq!(
            object.get("username").and_then(|v| v.as_str()),
            Some("Notch\"quoted"),
            "quotes in a username must be escaped, not break the document"
        );
        assert_eq!(object.get("protocol").and_then(|v| v.as_i64()), Some(772));
        assert_eq!(object.get("session_seconds").and_then(|v| v.as_i64()), Some(42));
    }

    struct Recorder {
        joins: std::sync::Mutex<Vec<String>>,
        quits: std::sync::Mutex<Vec<String>>,
    }

    impl HostListener for Recorder {
        fn player_join(&self, event: &PlayerEvent) {
            self.joins.lock().unwrap().push(event.username.clone());
        }
        fn player_quit(&self, event: &PlayerEvent) {
            self.quits.lock().unwrap().push(event.username.clone());
        }
        fn command(&self, command: &str, args: &[String]) -> CommandReply {
            if command == "greet" {
                CommandReply::Handled(vec![format!("hello {}", args.join(","))])
            } else {
                CommandReply::Unknown
            }
        }
    }

    #[test]
    fn trait_object_is_usable_from_a_shared_reference() {
        let recorder = std::sync::Arc::new(Recorder {
            joins: std::sync::Mutex::new(Vec::new()),
            quits: std::sync::Mutex::new(Vec::new()),
        });
        let listener: std::sync::Arc<dyn HostListener> = recorder.clone();
        listener.player_join(&sample());
        listener.player_quit(&sample());
        assert_eq!(recorder.joins.lock().unwrap().len(), 1);
        assert_eq!(recorder.quits.lock().unwrap().len(), 1);
        assert_eq!(
            listener.command("greet", &["a".to_string(), "b".to_string()]),
            CommandReply::Handled(vec!["hello a,b".to_string()])
        );
        assert_eq!(listener.command("nope", &[]), CommandReply::Unknown);
    }
}
