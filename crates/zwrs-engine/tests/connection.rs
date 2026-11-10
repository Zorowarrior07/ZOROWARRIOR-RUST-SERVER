//! End-to-end tests against a real TCP socket.
//!
//! These tests start the actual engine on an OS-assigned port and talk to it
//! with hand-built packets, exactly as a Minecraft client would. They assert on
//! persisted files, not on log strings or mocks.
//!
//! A real Minecraft client is **not** used here; that is recorded separately in
//! `docs/COMPATIBILITY.md` as "not tested with a real client".

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use zwrs_core::config::Config;
use zwrs_core::json::Json;
use zwrs_core::identity::Username;
use zwrs_core::{offline_uuid, PlayerIdentity};
use zwrs_engine::{prepare, start, SharedState};
use zwrs_protocol::{read_frame, PacketReader, PacketWriter};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Harness {
    root: PathBuf,
    state: Arc<SharedState>,
    handle: Option<zwrs_engine::ServerHandle>,
}

impl Harness {
    fn new(tag: &str, mutate: impl FnOnce(&mut Config)) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "zwrs-it-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let state = prepare(Self::config(&root, mutate)).expect("engine state");
        let handle = start(Arc::clone(&state)).expect("engine start");
        Self {
            root,
            state,
            handle: Some(handle),
        }
    }

    fn config(root: &Path, mutate: impl FnOnce(&mut Config)) -> Config {
        let mut config = Config::default();
        config.root_dir = root.to_path_buf();
        config.data_dir = root.join("data");
        config.plugin_dir = root.join("plugins");
        config.log_dir = root.join("logs");
        config.config_dir = root.join("config");
        config.port = 0;
        config.online_mode = false;
        config.bind_address = "127.0.0.1".to_string();
        mutate(&mut config);
        config
    }

    fn addr(&self) -> std::net::SocketAddr {
        self.handle.as_ref().expect("running").address()
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect_timeout(&self.addr(), Duration::from_secs(5))
            .expect("connect to engine");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
    }

    fn players_doc(&self) -> Json {
        let text = std::fs::read_to_string(self.state.config.players_file()).expect("players.json");
        Json::parse(&text).expect("players.json must be valid json")
    }

    fn sessions_doc(&self) -> Json {
        let text =
            std::fs::read_to_string(self.state.config.sessions_file()).expect("sessions.json");
        Json::parse(&text).expect("sessions.json must be valid json")
    }

    fn record_for(&self, uuid: &str) -> Option<Json> {
        self.players_doc()
            .get("records")
            .and_then(Json::as_array)?
            .iter()
            .find(|r| r.get("uuid").and_then(Json::as_str) == Some(uuid))
            .cloned()
    }

    fn stop(&mut self) {
        if let Some(mut handle) = self.handle.take() {
            handle.stop().expect("clean stop");
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.stop();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn read_packet(stream: &mut TcpStream) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match read_frame(&buf) {
            Ok(Some((payload, _))) => return payload.to_vec(),
            Ok(None) => {
                let n = stream.read(&mut chunk).expect("read from server");
                if n == 0 {
                    panic!("connection closed while waiting for a packet");
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(e) => panic!("frame error: {e}"),
        }
    }
}

fn handshake(stream: &mut TcpStream, protocol: i32, state: i32) {
    let payload = PacketWriter::with_id(0x00)
        .varint(protocol)
        .string("127.0.0.1")
        .u16(25565)
        .varint(state)
        .into_frame();
    stream.write_all(&payload).unwrap();
}

fn status_ping(harness: &Harness, protocol: i32) -> (Json, i64) {
    let mut stream = harness.connect();
    handshake(&mut stream, protocol, 1);
    stream
        .write_all(&PacketWriter::with_id(0x00).into_frame())
        .unwrap();
    let response = read_packet(&mut stream);
    let mut r = PacketReader::new(&response);
    assert_eq!(r.varint().unwrap(), 0x00);
    let json = r.string(32_767).unwrap();
    let doc = Json::parse(&json).expect("status json");

    let token: i64 = 0x5EED_1234_5678_9ABC;
    stream
        .write_all(&PacketWriter::with_id(0x01).i64(token).into_frame())
        .unwrap();
    let pong = read_packet(&mut stream);
    let mut r = PacketReader::new(&pong);
    assert_eq!(r.varint().unwrap(), 0x01);
    assert_eq!(r.i64().unwrap(), token, "pong must echo the ping payload");
    (doc, token)
}

fn read_disconnect_text(stream: &mut TcpStream) -> String {
    let packet = read_packet(stream);
    let mut r = PacketReader::new(&packet);
    assert_eq!(r.varint().unwrap(), 0x00, "expected a disconnect packet");
    let text = r.string(32_767).unwrap();
    Json::parse(&text)
        .expect("disconnect reason must be JSON")
        .get("text")
        .and_then(Json::as_str)
        .expect("disconnect text")
        .to_string()
}

fn login_disconnect(harness: &Harness, protocol: i32, username: &str) -> String {
    let mut stream = harness.connect();
    handshake(&mut stream, protocol, 2);
    stream
        .write_all(&PacketWriter::with_id(0x00).string(username).into_frame())
        .unwrap();
    let packet = read_packet(&mut stream);
    let mut r = PacketReader::new(&packet);
    assert_eq!(r.varint().unwrap(), 0x00, "expected a disconnect packet");
    let text = r.string(32_767).unwrap();
    let doc = Json::parse(&text).expect("disconnect reason must be JSON");
    doc.get("text")
        .and_then(Json::as_str)
        .expect("disconnect text")
        .to_string()
}

#[test]
fn status_ping_reports_real_state() {
    let harness = Harness::new("status", |_| {});
    let (doc, _) = status_ping(&harness, 763);

    let version = doc.get("version").expect("version");
    assert_eq!(
        version.get("protocol").and_then(Json::as_i64),
        Some(i64::from(Config::default().max_protocol()))
    );
    let players = doc.get("players").expect("players");
    assert_eq!(players.get("max").and_then(Json::as_u64), Some(20));
    assert_eq!(players.get("online").and_then(Json::as_u64), Some(0));
    assert_eq!(
        doc.get("description")
            .and_then(|d| d.get("text").or(Some(d)))
            .and_then(Json::as_str),
        Some("A ZOROWARRIOR Rust Server")
    );
}

#[test]
fn legacy_ping_is_answered() {
    let harness = Harness::new("legacy", |_| {});
    let mut stream = harness.connect();
    stream.write_all(&[0xFE, 0x01]).unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 512];
    while buf.len() < 3 {
        let n = stream.read(&mut chunk).expect("legacy response");
        assert!(n > 0, "server closed before answering the legacy ping");
        buf.extend_from_slice(&chunk[..n]);
    }
    assert_eq!(buf[0], 0xFF, "legacy response starts with 0xFF");
    let declared = u16::from_be_bytes([buf[1], buf[2]]) as usize;
    while buf.len() < declared + 3 {
        let n = stream.read(&mut chunk).expect("legacy response body");
        assert!(n > 0);
        buf.extend_from_slice(&chunk[..n]);
    }
    assert_eq!(declared + 3, buf.len(), "length prefix covers the response body");
}

#[test]
fn offline_login_creates_a_real_profile_and_session() {
    let harness = Harness::new("login", |_| {});
    let reason = login_disconnect(&harness, 763, "TestPlayer");
    assert!(
        reason.contains("identity was accepted"),
        "unexpected disconnect text: {reason}"
    );

    let uuid = offline_uuid("TestPlayer").to_string();
    let record = harness
        .record_for(&uuid)
        .expect("profile must be persisted for the real UUID");
    assert_eq!(
        record.get("username").and_then(Json::as_str),
        Some("TestPlayer")
    );
    assert_eq!(
        record.get("online").and_then(Json::as_bool),
        Some(true),
        "the player was admitted, so the profile is online at this point"
    );
    assert_eq!(record.get("join_count").and_then(Json::as_u64), Some(1));
    assert_eq!(
        record.get("identity_mode").and_then(Json::as_str),
        Some("offline")
    );
    // last_ip is null because IP storage is off by default.
    assert!(matches!(record.get("last_ip"), Some(Json::Null)));

    let sessions = harness.sessions_doc();
    let records = sessions.get("records").and_then(Json::as_array).unwrap();
    assert!(!records.is_empty(), "a session must be recorded");
    // The server may still be closing the session when the packets have been
    // read, so wait for the persisted state to settle.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut closed: Option<Json> = None;
    while std::time::Instant::now() < deadline {
        if let Some(doc) = std::fs::read_to_string(harness.state.config.sessions_file())
            .ok()
            .and_then(|t| Json::parse(&t).ok())
        {
            let candidate = doc
                .get("records")
                .and_then(Json::as_array)
                .and_then(|r| r.last().cloned());
            if candidate
                .as_ref()
                .and_then(|r| r.get("state"))
                .and_then(Json::as_str)
                == Some("closed")
            {
                closed = candidate;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let session = closed.expect("the closed session must be persisted with its duration");
    assert_eq!(session.get("uuid").and_then(Json::as_str), Some(uuid.as_str()));
    assert_eq!(session.get("state").and_then(Json::as_str), Some("closed"));
    assert!(session.get("duration_seconds").and_then(Json::as_u64).is_some());
    // The server notices the closed socket asynchronously; allow it a moment.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while harness.state.online_count() != 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(harness.state.online_count(), 0, "player is offline again");

    // And the logout is written to disk, not just held in memory.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut offline_record = None;
    while std::time::Instant::now() < deadline {
        if let Some(record) = harness.record_for(&uuid) {
            if record.get("online").and_then(Json::as_bool) == Some(false) {
                offline_record = Some(record);
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let record = offline_record.expect("the logout must be persisted for the real profile");
    assert!(record.get("last_logout").and_then(Json::as_str).is_some());
}

#[test]
fn profile_and_session_survive_a_restart() {
    let root = std::env::temp_dir().join(format!(
        "zwrs-it-restart-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let uuid = offline_uuid("PersistedPlayer").to_string();
    {
        let state = prepare(Harness::config(&root, |_| {})).expect("state");
        let handle = start(Arc::clone(&state)).expect("start");
        let harness = Harness {
            root: root.clone(),
            state,
            handle: Some(handle),
        };
        login_disconnect(&harness, 763, "PersistedPlayer");
        let mut harness = harness;
        harness.stop();
        // Keep the data directory for the restart below, but do not run the
        // `Drop` cleanup that deletes the server root.
        std::mem::forget(harness);
    }

    // Restart in the same directory: the profile must still be there.
    let state = prepare(Harness::config(&root, |_| {})).expect("restart state");
    let players = state.players.lock().unwrap();
    let profile = players
        .get(offline_uuid("PersistedPlayer"))
        .expect("profile restored after restart");
    assert_eq!(profile.username().as_str(), "PersistedPlayer");
    assert_eq!(profile.join_count(), 1);
    assert!(!profile.online(), "must not be online after restart");
    assert!(
        profile.last_logout().is_some(),
        "logout timestamp persisted"
    );
    drop(players);

    let sessions_text = std::fs::read_to_string(state.config.sessions_file()).unwrap();
    let doc = Json::parse(&sessions_text).unwrap();
    let records = doc.get("records").and_then(Json::as_array).unwrap();
    assert!(
        records
            .iter()
            .any(|r| r.get("uuid").and_then(Json::as_str) == Some(uuid.as_str())),
        "session history must survive the restart"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn same_username_twice_is_rejected_while_online() {
    let harness = Harness::new("dupe", |_| {});
    // First connection: admitted, then the server waits for the client to leave.
    let mut first = harness.connect();
    handshake(&mut first, 763, 2);
    first
        .write_all(&PacketWriter::with_id(0x00).string("Dupe").into_frame())
        .unwrap();
    let _ = read_packet(&mut first); // accepted notice
    assert_eq!(harness.state.online_count(), 1);

    // Second connection with the same name must be refused.
    let reason = login_disconnect(&harness, 763, "Dupe");
    assert!(
        reason.contains("already connected"),
        "unexpected reason: {reason}"
    );
    drop(first);
}

#[test]
fn invalid_username_is_refused_with_a_clear_reason() {
    let harness = Harness::new("badname", |_| {});
    let reason = login_disconnect(&harness, 763, "bad name");
    assert!(reason.contains("Invalid username"), "{reason}");
    assert_eq!(
        harness
            .players_doc()
            .get("records")
            .and_then(Json::as_array)
            .map(<[Json]>::len),
        Some(0),
        "no profile may be created for a refused login"
    );
}

#[test]
fn unsupported_protocol_version_is_refused_and_explained() {
    let harness = Harness::new("proto", |_| {});
    let reason = login_disconnect(&harness, 5, "OldClient");
    assert!(
        reason.contains("protocol") && reason.contains("5"),
        "unexpected reason: {reason}"
    );
}

#[test]
fn online_mode_refuses_logins_instead_of_pretending() {
    let harness = Harness::new("online", |config| config.online_mode = true);
    // Status still works, which is how operators verify reachability.
    let (doc, _) = status_ping(&harness, 763);
    assert!(doc.get("version").is_some());

    let reason = login_disconnect(&harness, 763, "OnlinePlayer");
    assert!(
        reason.contains("online-mode") && reason.contains("authentication"),
        "online mode must explain itself: {reason}"
    );
    assert_eq!(
        harness
            .players_doc()
            .get("records")
            .and_then(Json::as_array)
            .map(<[Json]>::len),
        Some(0)
    );
}

#[test]
fn crash_recovery_marks_interrupted_sessions_unfinished() {
    let root = std::env::temp_dir().join(format!(
        "zwrs-it-crash-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    // Simulate a process that died while a player was online: keep a session
    // active and a profile marked online, without shutting down cleanly.
    {
        let state = prepare(Harness::config(&root, |_| {})).expect("state");
        let identity = PlayerIdentity::offline(Username::parse("Crashed").unwrap());
        let now = zwrs_core::Timestamp::now();
        state
            .sessions
            .lock()
            .unwrap()
            .begin(&identity, now, 763);
        state
            .players
            .lock()
            .unwrap()
            .record_join(&identity, now, None)
            .unwrap();
        let sessions = state.sessions.lock().unwrap().to_json_records();
        zwrs_core::storage::write_atomic(
            &state.config.sessions_file(),
            zwrs_core::storage::document(sessions).to_pretty().as_bytes(),
        )
        .unwrap();
        // Deliberately no shutdown: the files stay "dirty".
        std::mem::forget(state);
    }

    let state = prepare(Harness::config(&root, |_| {})).expect("recovered state");
    assert_eq!(state.online_count(), 0, "no player may be left online");
    let players = state.players.lock().unwrap();
    let profile = players.get(offline_uuid("Crashed")).expect("profile");
    assert!(!profile.online());
    assert_eq!(
        profile.total_playtime_seconds(),
        0,
        "playtime must not be invented for an interrupted session"
    );
    drop(players);

    let text = std::fs::read_to_string(state.config.sessions_file()).unwrap();
    let doc = Json::parse(&text).unwrap();
    let records = doc.get("records").and_then(Json::as_array).unwrap();
    assert!(records
        .iter()
        .any(|r| r.get("state").and_then(Json::as_str) == Some("unfinished")));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn server_stop_is_clean_and_idempotent() {
    let mut harness = Harness::new("stop", |_| {});
    assert!(harness.handle.as_ref().unwrap().is_running());
    harness.stop();
    assert!(harness.handle.is_none());
    // Dropping again must not panic or hang.
    harness.stop();
}

#[test]
fn malformed_handshake_does_not_kill_the_server() {
    let harness = Harness::new("malformed", |_| {});
    {
        let mut stream = harness.connect();
        // A frame whose content is not a handshake.
        stream
            .write_all(&PacketWriter::with_id(0x42).varint(1).into_frame())
            .unwrap();
        let mut buf = [0u8; 16];
        let _ = stream.read(&mut buf);
    }
    // The server is still serving status pings.
    let (doc, _) = status_ping(&harness, 754);
    assert!(doc.get("players").is_some());
}

#[test]
fn zero_length_and_oversized_frames_are_dropped() {
    let harness = Harness::new("frames", |_| {});
    {
        let mut stream = harness.connect();
        stream.write_all(&[0x00]).unwrap(); // zero-length frame
        let mut buf = [0u8; 8];
        let _ = stream.read(&mut buf);
    }
    {
        let mut stream = harness.connect();
        // Declares a 3 MiB frame and sends nothing: the server must not allocate it.
        let mut header = Vec::new();
        zwrs_protocol::write_varint(&mut header, 3 * 1024 * 1024);
        stream.write_all(&header).unwrap();
        let mut buf = [0u8; 8];
        let _ = stream.read(&mut buf);
    }
    let (doc, _) = status_ping(&harness, 763);
    assert!(doc.get("version").is_some());
}

// ---------------------------------------------------------------------------
// Host hooks (the seam the Java plugin host plugs into)
// ---------------------------------------------------------------------------

/// Records what the engine reports, so the test can assert on real events
/// instead of on log output.
#[derive(Default)]
struct RecordingListener {
    joins: std::sync::Mutex<Vec<zwrs_engine::PlayerEvent>>,
    quits: std::sync::Mutex<Vec<zwrs_engine::PlayerEvent>>,
    commands: std::sync::Mutex<Vec<(String, Vec<String>)>>,
}

impl zwrs_engine::HostListener for RecordingListener {
    fn player_join(&self, event: &zwrs_engine::PlayerEvent) {
        self.joins.lock().unwrap().push(event.clone());
    }

    fn player_quit(&self, event: &zwrs_engine::PlayerEvent) {
        self.quits.lock().unwrap().push(event.clone());
    }

    fn command(&self, command: &str, args: &[String]) -> zwrs_engine::CommandReply {
        self.commands
            .lock()
            .unwrap()
            .push((command.to_string(), args.to_vec()));
        if command == "hello" {
            zwrs_engine::CommandReply::Handled(vec![format!("hello {}", args.join(" "))])
        } else {
            zwrs_engine::CommandReply::Unknown
        }
    }
}

#[test]
fn host_listener_receives_real_join_and_quit_events() {
    let harness = Harness::new("hooks", |_| {});
    let listener = Arc::new(RecordingListener::default());
    zwrs_engine::server::set_listener(&harness.state, Some(listener.clone()));

    let mut stream = harness.connect();
    handshake(&mut stream, 772, 2);
    stream
        .write_all(&PacketWriter::with_id(0x00).string("HookProbe").into_frame())
        .unwrap();
    let text = read_disconnect_text(&mut stream);
    assert!(text.contains("identity was accepted"), "got: {text}");

    let joins = listener.joins.lock().unwrap().clone();
    assert_eq!(joins.len(), 1, "exactly one join event");
    let join = &joins[0];
    assert_eq!(join.username, "HookProbe");
    assert_eq!(join.protocol, 772);
    assert_eq!(
        join.uuid,
        zwrs_core::offline_uuid("HookProbe").to_string(),
        "the event carries the same identity that was persisted"
    );
    assert!(join.timestamp.ends_with('Z'), "ISO-8601 UTC timestamp");
    assert_eq!(join.session_seconds, 0, "a join has no duration");
    assert!(join.online_count >= 1);

    // The profile must already be on disk when the hook fires: the plugin sees
    // durable state, never a promise.
    let record = harness.record_for(&join.uuid).expect("profile persisted");
    assert_eq!(
        record.get("online").and_then(Json::as_bool),
        Some(true),
        "profile is online at event time"
    );

    drop(stream);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while listener.quits.lock().unwrap().is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let quits = listener.quits.lock().unwrap().clone();
    assert_eq!(quits.len(), 1, "exactly one quit event");
    assert_eq!(quits[0].uuid, join.uuid);
    assert!(quits[0].timestamp.ends_with('Z'));

    let record = harness.record_for(&join.uuid).expect("profile persisted");
    assert_eq!(
        record.get("online").and_then(Json::as_bool),
        Some(false),
        "the profile is already closed when the quit hook fires"
    );
}

#[test]
fn host_listener_handles_plugin_commands_and_unknown_ones() {
    let harness = Harness::new("hooks-commands", |_| {});
    let listener = Arc::new(RecordingListener::default());
    zwrs_engine::server::set_listener(&harness.state, Some(listener.clone()));

    let outcome = zwrs_engine::admin::execute(&harness.state, "/hello one two");
    assert!(outcome.recognised, "the host claimed the command");
    assert_eq!(outcome.lines, vec!["hello one two".to_string()]);
    assert_eq!(
        listener.commands.lock().unwrap().clone(),
        vec![("hello".to_string(), vec!["one".to_string(), "two".to_string()])]
    );

    // Built-in commands must never be shadowed by a plugin.
    let status = zwrs_engine::admin::execute(&harness.state, "/status");
    assert!(status.recognised);
    assert_eq!(
        listener.commands.lock().unwrap().len(),
        1,
        "built-ins are not delegated"
    );

    let unknown = zwrs_engine::admin::execute(&harness.state, "/definitely-not-a-command");
    assert!(!unknown.recognised, "still reported as unknown");
    assert!(unknown.lines[0].contains("Unknown command"));
}

#[test]
fn a_panicking_host_listener_cannot_take_the_engine_down() {
    struct Panicking;
    impl zwrs_engine::HostListener for Panicking {
        fn player_join(&self, _event: &zwrs_engine::PlayerEvent) {
            panic!("plugin bug");
        }
        fn player_quit(&self, _event: &zwrs_engine::PlayerEvent) {
            panic!("plugin bug");
        }
        fn command(&self, _command: &str, _args: &[String]) -> zwrs_engine::CommandReply {
            panic!("plugin bug");
        }
    }

    let harness = Harness::new("hooks-panic", |_| {});
    zwrs_engine::server::set_listener(&harness.state, Some(Arc::new(Panicking)));

    let text = login_disconnect(&harness, 772, "PanicProbe");
    assert!(
        text.contains("identity was accepted"),
        "the login still succeeds: {text}"
    );

    // The engine is still serving after the panic.
    let (doc, _) = status_ping(&harness, 772);
    assert!(doc.get("players").is_some());

    // And a panicking command handler degrades to "unknown command".
    let outcome = zwrs_engine::admin::execute(&harness.state, "/boom");
    assert!(!outcome.recognised);
}
