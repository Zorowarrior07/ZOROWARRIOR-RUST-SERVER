//! Engine state, the connection state machine, and lifecycle control.
//!
//! # Architecture
//!
//! ```text
//! main / JNI  ->  ServerHandle { running flag, shutdown socket, join handle }
//!                     |
//!                     v
//!               Server::run()  -- accept loop --> one thread per connection
//!                     |                              |
//!                     +--- Arc<SharedState> ---------+
//!                             |-- config
//!                             |-- players   (persistent profiles)
//!                             |-- sessions  (live sessions)
//!                             |-- metrics
//! ```
//!
//! Each connection is handled by a blocking thread with a read timeout. The
//! connection state machine implements the real Java Edition handshake and
//! status exchange, and the login path up to the point where the server must
//! either authenticate (online mode: refused, see [`crate::auth`]) or admit the
//! player (offline mode: profile and session recorded, then a truthful
//! "connection complete" notice because world/play-state packets are not
//! implemented in this build).
//!
//! Nothing here fabricates data: player counts come from the session registry,
//! profiles come from disk, and the capability report describes what the code
//! actually does.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use zwrs_core::admin::PlayerRepository;
use zwrs_core::config::Config;
use zwrs_core::identity::Uuid;
use zwrs_core::log::{self, Level};
use zwrs_core::session::SessionRegistry;
use zwrs_core::time::Timestamp;
use zwrs_core::{log_error, log_info, log_trace, log_warn};
use zwrs_protocol::{read_frame, ProtocolError, MAX_PACKET_LEN};

use crate::auth::{AuthError, Authenticator, OfflineAuthenticator, RejectingOnlineAuthenticator};
use crate::packet::{self, Handshake, LoginStart, NextState};
use crate::status::StatusInfo;

/// Maximum number of bytes buffered while waiting for one frame.
const READ_BUFFER_LIMIT: usize = MAX_PACKET_LEN + 8;
/// How long a fresh connection may take to send its handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Counters exposed by `/status`.
#[derive(Debug, Default)]
pub struct Metrics {
    /// Connections accepted since start.
    pub connections_accepted: AtomicU64,
    /// Status pings answered.
    pub status_pings: AtomicU64,
    /// Login attempts seen.
    pub login_attempts: AtomicU64,
    /// Logins refused (any reason).
    pub logins_refused: AtomicU64,
    /// Logins admitted.
    pub logins_admitted: AtomicU64,
    /// Protocol errors raised while parsing input.
    pub protocol_errors: AtomicU64,
    /// Sessions ended (normal or recovered).
    pub sessions_ended: AtomicU64,
    /// Legacy `0xFE` pings answered.
    pub legacy_pings: AtomicU64,
}

impl Metrics {
    fn inc(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

/// State shared by the accept loop, connections and the admin console.
///
/// Not `Debug`: the host listener is a trait object, and printing it would say
/// nothing useful. Use `/status` for a readable snapshot.
pub struct SharedState {
    /// Effective configuration.
    pub config: Config,
    /// Persistent player profiles.
    pub players: Mutex<PlayerRepository>,
    /// Live and retained sessions.
    pub sessions: Mutex<SessionRegistry>,
    /// Counters.
    pub metrics: Metrics,
    /// Authenticator for the configured mode.
    pub authenticator: Box<dyn Authenticator>,
    /// Set when shutdown begins; connections observe it and stop.
    pub shutting_down: AtomicBool,
    /// Process start instant (for uptime).
    pub started_at: Instant,
    /// Wall-clock start time.
    pub started_wall: Timestamp,
    /// Actual bound address once listening.
    pub bound_address: Mutex<Option<SocketAddr>>,
    /// Optional embedder (the Java plugin host). `None` in the standalone
    /// binary and in tests, in which case every hook is a no-op.
    pub listener: Mutex<Option<Arc<dyn crate::events::HostListener>>>,
}

impl std::fmt::Debug for SharedState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let listener = match self.listener.try_lock() {
            Ok(slot) => {
                if slot.is_some() {
                    "installed"
                } else {
                    "none"
                }
            }
            Err(_) => "busy",
        };
        f.debug_struct("SharedState")
            .field("config", &self.config)
            .field("players", &"<repository>")
            .field("sessions", &"<registry>")
            .field("metrics", &self.metrics)
            .field("host_listener", &listener)
            .field("shutting_down", &self.shutting_down)
            .field("bound_address", &self.bound_address.try_lock().ok())
            .finish()
    }
}

impl SharedState {
    /// Number of players currently online according to the session registry.
    #[must_use]
    pub fn online_count(&self) -> usize {
        self.sessions.lock().map(|s| s.online_count()).unwrap_or(0)
    }

    /// Online usernames, sorted, for the status sample.
    #[must_use]
    pub fn online_names(&self) -> Vec<String> {
        let Ok(sessions) = self.sessions.lock() else {
            return Vec::new();
        };
        let mut names: Vec<String> = sessions
            .all()
            .iter()
            .filter(|s| s.is_active())
            .map(|s| s.username().to_string())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Uptime in seconds.
    #[must_use]
    pub fn uptime_seconds(&self) -> u64 {
        u64::try_from(self.started_at.elapsed().as_secs()).unwrap_or(0)
    }
}

/// Handle used by the host (CLI or JNI) to observe and stop the engine.
#[derive(Debug)]
pub struct ServerHandle {
    running: Arc<AtomicBool>,
    shutdown_addr: String,
    join: Option<JoinHandle<()>>,
    /// Local address the engine bound to, once known.
    address: SocketAddr,
}

impl ServerHandle {
    /// Local address the server is listening on.
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    /// Whether the engine is still running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Requests a graceful stop and waits for the accept loop to finish.
    ///
    /// # Errors
    /// Returns a message when the accept loop panicked.
    pub fn stop(&mut self) -> Result<(), String> {
        if !self.running.swap(false, Ordering::SeqCst) {
            if let Some(join) = self.join.take() {
                return join.join().map_err(|_| "engine thread panicked".to_string());
            }
            return Ok(());
        }
        // Wake the accept loop with a throwaway connection.
        let _ = TcpStream::connect_timeout(
            &self
                .shutdown_addr
                .parse()
                .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], 0))),
            Duration::from_secs(2),
        )
        .map(|s| s.shutdown(Shutdown::Both));
        if let Some(join) = self.join.take() {
            join.join().map_err(|_| "engine thread panicked".to_string())?;
        }
        Ok(())
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Builds the shared state, applying crash recovery.
///
/// # Errors
/// Propagates storage and I/O errors. In particular, a corrupt player database
/// aborts startup instead of being replaced with an empty one.
pub fn prepare(config: Config) -> zwrs_core::Result<Arc<SharedState>> {
    config.ensure_directories()?;
    log::set_level(config.log_level);

    let mut players = PlayerRepository::open(
        &config.players_file(),
        &config.profiles_dir(),
        config.store_ip_addresses,
    )?;
    for note in players.recovery_notes() {
        log_warn!("zwrs::storage", "{note}");
    }

    // Sessions and profiles are reconciled on startup: anything left active
    // belongs to a previous process and its logout time is unknown.
    let sessions_doc = zwrs_core::storage::read_json(&config.sessions_file())?;
    let mut sessions = match sessions_doc {
        Some(doc) => {
            let records = zwrs_core::storage::records(&doc)?;
            let parsed: Vec<zwrs_core::session::Session> = records
                .iter()
                .map(zwrs_core::session::Session::from_json)
                .collect::<zwrs_core::Result<_>>()?;
            SessionRegistry::from_sessions(parsed, 5_000)
        }
        None => SessionRegistry::default(),
    };
    let unfinished = sessions.recover_unfinished();
    if !unfinished.is_empty() {
        log_warn!(
            "zwrs::session",
            "{} session(s) from a previous run were still open; marked as unfinished (playtime not credited, logout time unknown)",
            unfinished.len()
        );
    }
    let stale = players.force_all_offline();
    if !stale.is_empty() {
        log_warn!(
            "zwrs::players",
            "{} profile(s) were marked online by a previous run; forced offline",
            stale.len()
        );
    }
    players.save()?;
    zwrs_core::storage::write_atomic(
        &config.sessions_file(),
        zwrs_core::storage::document(sessions.to_json_records())
            .to_pretty()
            .as_bytes(),
    )?;

    let authenticator: Box<dyn Authenticator> = if config.online_mode {
        Box::new(RejectingOnlineAuthenticator)
    } else {
        Box::new(OfflineAuthenticator)
    };

    let now = Timestamp::now();
    Ok(Arc::new(SharedState {
        config,
        players: Mutex::new(players),
        sessions: Mutex::new(sessions),
        metrics: Metrics::default(),
        authenticator,
        shutting_down: AtomicBool::new(false),
        started_at: Instant::now(),
        started_wall: now,
        bound_address: Mutex::new(None),
        listener: Mutex::new(None),
    }))
}

/// Binds the listener and starts the engine on a background thread.
///
/// # Errors
/// Returns a message when the address cannot be bound.
pub fn start(state: Arc<SharedState>) -> Result<ServerHandle, String> {
    let bind = format!("{}:{}", state.config.bind_address, state.config.port);
    let listener = TcpListener::bind(&bind)
        .map_err(|e| format!("could not bind {bind}: {e} (is another server already using this port?)"))?;
    let local = listener
        .local_addr()
        .map_err(|e| format!("could not read local address: {e}"))?;
    listener
        .set_nonblocking(false)
        .map_err(|e| format!("could not configure listener: {e}"))?;
    if let Ok(mut bound) = state.bound_address.lock() {
        *bound = Some(local);
    }

    let running = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&running);
    let shutdown_state = Arc::clone(&state);
    let join = std::thread::Builder::new()
        .name("zwrs-accept".to_string())
        .spawn(move || accept_loop(listener, shutdown_state, flag))
        .map_err(|e| format!("could not start accept thread: {e}"))?;

    Ok(ServerHandle {
        running,
        shutdown_addr: local.to_string(),
        join: Some(join),
        address: local,
    })
}

fn accept_loop(listener: TcpListener, state: Arc<SharedState>, running: Arc<AtomicBool>) {
    log_info!(
        "zwrs::engine",
        "Rust engine listening on {} (protocol {}–{}, {} player max)",
        listener
            .local_addr()
            .map_or_else(|_| "?".to_string(), |a| a.to_string()),
        zwrs_core::config::MIN_SUPPORTED_PROTOCOL,
        state.config.max_protocol(),
        state.config.max_players
    );
    log_info!("zwrs::auth", "{}", state.authenticator.describe());

    for stream in listener.incoming() {
        if !running.load(Ordering::SeqCst) {
            break;
        }
        match stream {
            Ok(stream) => {
                Metrics::inc(&state.metrics.connections_accepted);
                let conn_state = Arc::clone(&state);
                let spawn = std::thread::Builder::new()
                    .name("zwrs-conn".to_string())
                    .spawn(move || {
                        if let Err(e) = handle_connection(stream, &conn_state) {
                            log_trace!("zwrs::conn", "connection ended: {e}");
                        }
                    });
                if let Err(e) = spawn {
                    log_error!("zwrs::conn", "could not spawn connection thread: {e}");
                }
            }
            Err(e) => {
                if running.load(Ordering::SeqCst) {
                    log_warn!("zwrs::engine", "accept failed: {e}");
                }
            }
        }
    }

    state.shutting_down.store(true, Ordering::SeqCst);
    shutdown_all(&state);
    log_info!("zwrs::engine", "accept loop stopped");
}

/// Ends every active session and persists state before the process exits.
pub fn shutdown_all(state: &SharedState) {
    let now = Timestamp::now();
    let ended: Vec<(Uuid, String, u64)> = {
        let mut sessions = match state.sessions.lock() {
            Ok(s) => s,
            Err(poisoned) => poisoned.into_inner(),
        };
        let active: Vec<(Uuid, String)> = sessions
            .all()
            .iter()
            .filter(|s| s.is_active())
            .map(|s| (s.uuid(), s.username().to_string()))
            .collect();
        active
            .into_iter()
            .map(|(uuid, name)| {
                let duration = sessions.end(uuid, now).map_or(0, |(_, d)| d);
                (uuid, name, duration)
            })
            .collect()
    };

    if let Ok(mut players) = state.players.lock() {
        for (uuid, name, duration) in &ended {
            match players.record_logout(*uuid, now) {
                Ok(_) => log_info!(
                    "zwrs::session",
                    "closed session for {name} ({duration}s) during shutdown"
                ),
                Err(e) => log_error!("zwrs::players", "could not record logout for {name}: {e}"),
            }
        }
        if let Err(e) = players.save() {
            log_error!("zwrs::players", "could not save player data on shutdown: {e}");
        }
    }

    if let Ok(sessions) = state.sessions.lock() {
        if let Err(e) = zwrs_core::storage::write_atomic(
            &state.config.sessions_file(),
            zwrs_core::storage::document(sessions.to_json_records())
                .to_pretty()
                .as_bytes(),
        ) {
            log_error!("zwrs::session", "could not save sessions on shutdown: {e}");
        }
    }
}

fn handle_connection(mut stream: TcpStream, state: &Arc<SharedState>) -> std::io::Result<()> {
    let peer = stream.peer_addr()?;
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    stream.set_nodelay(true)?;
    log_trace!("zwrs::conn", "connection from {peer}");

    let mut buf: Vec<u8> = Vec::with_capacity(1_024);
    let mut chunk = [0u8; 4_096];

    // ---- sniff the first byte: legacy ping or a modern frame --------------
    //
    // The legacy ping has no length prefix, so framing must not be attempted
    // before this check; `0xFE` would otherwise look like a 254-byte frame.
    let mut probe = [0u8; 2];
    let read = match stream.read(&mut probe) {
        Ok(0) => return Ok(()),
        Ok(n) => n,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
            log_trace!("zwrs::conn", "{peer} opened a connection but sent nothing");
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    buf.extend_from_slice(&probe[..read]);

    if buf[0] == 0xFE {
        Metrics::inc(&state.metrics.legacy_pings);
        let response = packet::legacy_kick_response(
            state.config.max_protocol(),
            &crate::VERSION_NAME,
            &state.config.motd,
            i32::try_from(state.online_count()).unwrap_or(0),
            i32::try_from(state.config.max_players).unwrap_or(0),
        );
        stream.write_all(&response)?;
        return Ok(());
    }

    let first = match read_next(&mut stream, &mut buf, &mut chunk)? {
        Some(bytes) => bytes,
        None => return Ok(()),
    };

    let handshake = match Handshake::parse(&first) {
        Ok(hs) => hs,
        Err(e) => {
            Metrics::inc(&state.metrics.protocol_errors);
            log_warn!("zwrs::conn", "malformed handshake from {peer}: {e}");
            return Ok(());
        }
    };
    log_trace!(
        "zwrs::conn",
        "{peer} handshake: protocol {} host {} port {} state {:?}",
        handshake.protocol,
        handshake.clean_address(),
        handshake.server_port,
        handshake.next_state
    );

    match handshake.next_state {
        NextState::Status => serve_status(&mut stream, state, peer, &mut buf, &mut chunk),
        NextState::Login => serve_login(&mut stream, state, peer, handshake.protocol, &mut buf, &mut chunk),
        NextState::Other(raw) => {
            log_warn!("zwrs::conn", "{peer} requested unsupported next state {raw}");
            Ok(())
        }
    }
}

/// Reads one complete frame, growing `buf` as needed.
///
/// Returns `None` when the peer closed cleanly before sending anything.
fn read_next(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    chunk: &mut [u8],
) -> std::io::Result<Option<Vec<u8>>> {
    loop {
        match read_frame(buf) {
            Ok(Some((payload, used))) => {
                let payload = payload.to_vec();
                // Keep any bytes that belong to the next frame.
                buf.drain(..used);
                return Ok(Some(payload));
            }
            Ok(None) => {
                if buf.len() > READ_BUFFER_LIMIT {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "frame header exceeds the read buffer",
                    ));
                }
                let read = stream.read(chunk)?;
                if read == 0 {
                    return Ok(None);
                }
                buf.extend_from_slice(&chunk[..read]);
            }
            Err(e) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("frame error: {e}"),
                ));
            }
        }
    }
}

fn serve_status(
    stream: &mut TcpStream,
    state: &Arc<SharedState>,
    peer: SocketAddr,
    buf: &mut Vec<u8>,
    chunk: &mut [u8],
) -> std::io::Result<()> {
    let Some(request) = read_next(stream, buf, chunk)? else {
        return Ok(());
    };
    if request.first() != Some(&0x00) {
        Metrics::inc(&state.metrics.protocol_errors);
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "expected a status request",
        ));
    }

    let info = StatusInfo {
        version_name: crate::VERSION_NAME.to_string(),
        protocol: state.config.max_protocol(),
        motd: state.config.motd.clone(),
        max_players: state.config.max_players,
        online: u32::try_from(state.online_count()).unwrap_or(0),
        sample: state.online_names(),
        second_line: None,
    };
    stream.write_all(&packet::status_response(&info.to_json()))?;
    Metrics::inc(&state.metrics.status_pings);

    // A ping may follow; answer it with the same payload for the latency read-out.
    if let Ok(Some(ping)) = read_next(stream, buf, chunk) {
        if let Ok(token) = packet::parse_ping(&ping) {
            stream.write_all(&packet::pong(token))?;
        }
    }
    log_trace!("zwrs::conn", "{peer} status ping answered");
    Ok(())
}

fn serve_login(
    stream: &mut TcpStream,
    state: &Arc<SharedState>,
    peer: SocketAddr,
    protocol: i32,
    buf: &mut Vec<u8>,
    chunk: &mut [u8],
) -> std::io::Result<()> {
    Metrics::inc(&state.metrics.login_attempts);
    let disconnect = |stream: &mut TcpStream, err: &AuthError| -> std::io::Result<()> {
        stream.write_all(&packet::login_disconnect(&err.kick_reason()))?;
        let _ = stream.shutdown(Shutdown::Write);
        Ok(())
    };

    let supported = (zwrs_core::config::MIN_SUPPORTED_PROTOCOL..=state.config.max_protocol())
        .contains(&protocol);
    if !supported {
        Metrics::inc(&state.metrics.logins_refused);
        let err = AuthError::Refused(format!(
            "This server speaks protocol {}–{} (Minecraft {} – {}); your client reported protocol {protocol}.",
            zwrs_core::config::MIN_SUPPORTED_PROTOCOL,
            state.config.max_protocol(),
            zwrs_core::config::MIN_SUPPORTED_PROTOCOL,
            crate::VERSION_NAME
        ));
        log_info!("zwrs::login", "{peer} refused: unsupported protocol {protocol}");
        return disconnect(stream, &err);
    }

    let Some(payload) = read_next(stream, buf, chunk)? else {
        return Ok(());
    };
    let login = match LoginStart::parse(&payload) {
        Ok(l) => l,
        Err(e) => {
            Metrics::inc(&state.metrics.protocol_errors);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bad login start: {e}"),
            ));
        }
    };

    if state.online_count() >= usize::try_from(state.config.max_players).unwrap_or(usize::MAX) {
        Metrics::inc(&state.metrics.logins_refused);
        return disconnect(
            stream,
            &AuthError::ServerFull {
                max: state.config.max_players,
            },
        );
    }

    let identity = match state.authenticator.authenticate(&login.raw_username, protocol) {
        Ok(id) => id,
        Err(e) => {
            Metrics::inc(&state.metrics.logins_refused);
            log_info!(
                "zwrs::login",
                "{peer} refused `{}`: {}",
                login.raw_username,
                e.code()
            );
            return disconnect(stream, &e);
        }
    };

    let now = Timestamp::now();
    let ip = peer.ip().to_string();
    let mut admitted = false;
    if let Ok(mut sessions) = state.sessions.lock() {
        admitted = sessions.begin(&identity, now, protocol).is_some();
    }
    if !admitted {
        // The identity is already connected (double login).
        Metrics::inc(&state.metrics.logins_refused);
        return disconnect(
            stream,
            &AuthError::Refused(
                "You are already connected to this server. Wait a few seconds and try again."
                    .to_string(),
            ),
        );
    }

    let join = {
        let mut players = match state.players.lock() {
            Ok(p) => p,
            Err(poisoned) => poisoned.into_inner(),
        };
        players.record_join(&identity, now, Some(ip))
    };
    match join {
        Ok(outcome) => {
            Metrics::inc(&state.metrics.logins_admitted);
            if outcome.first_join {
                log_info!(
                    "zwrs::players",
                    "first join: {} ({})",
                    identity.username(),
                    identity.uuid()
                );
            }
            if let Some(old) = outcome.renamed_from {
                log_info!(
                    "zwrs::players",
                    "{} previously known as {old}",
                    identity.username()
                );
            }
            log_info!(
                "zwrs::login",
                "admitted {} ({}, protocol {protocol}) from {peer}",
                identity.username(),
                identity.uuid()
            );
            persist_sessions(state);
            // Only now, with the profile and session on disk, does the embedder
            // hear about the join.
            let event = crate::events::PlayerEvent {
                uuid: identity.uuid().to_string(),
                username: identity.username().to_string(),
                protocol,
                address: peer.to_string(),
                timestamp: now.to_string(),
                online_count: state.online_count(),
                session_seconds: 0,
            };
            deliver(state, "player_join", move |listener| {
                listener.player_join(&event);
            });
        }
        Err(e) => {
            Metrics::inc(&state.metrics.logins_refused);
            if let Ok(mut sessions) = state.sessions.lock() {
                let _ = sessions.end(identity.uuid(), now);
            }
            log_error!("zwrs::players", "could not persist profile: {e}");
            return disconnect(
                stream,
                &AuthError::Refused(
                    "The server could not save your profile, so the login was cancelled. Player data was left untouched; ask an administrator to check the server console."
                        .to_string(),
                ),
            );
        }
    }

    // The player is now genuinely online in this engine: their session and
    // profile are live. Play state (chunks, entities, inventory) is not
    // implemented, so instead of pretending, the client is told exactly that.
    stream.write_all(&packet::login_disconnect(
        "ZOROWARRIOR Rust Server: your identity was accepted and your session is now live.\n\
         This build does not implement world/play-state packets yet, so you cannot move around.\n\
         Your profile and session data are real and persisted.",
    ))?;
    let _ = stream.flush();

    // End the session as soon as the client goes away.
    let disconnect_started = Instant::now();
    let mut scratch = [0u8; 256];
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    while let Ok(read) = stream.read(&mut scratch) {
        if read == 0 {
            break;
        }
        if disconnect_started.elapsed() > Duration::from_secs(30) {
            break;
        }
    }
    let ended_at = Timestamp::now();
    let duration = state
        .sessions
        .lock()
        .ok()
        .and_then(|mut s| s.end(identity.uuid(), ended_at));
    // Persist the closed session before touching the profile so that a reader
    // never observes a finished profile next to a session still marked active.
    persist_sessions(state);
    if let Ok(mut players) = state.players.lock() {
        match players.record_logout(identity.uuid(), ended_at) {
            Ok(Some(credited)) => log_info!(
                "zwrs::session",
                "{} disconnected after {credited}s",
                identity.username()
            ),
            Ok(None) => log_warn!(
                "zwrs::players",
                "no profile found while closing session for {}",
                identity.username()
            ),
            Err(e) => log_error!("zwrs::players", "could not record logout: {e}"),
        }
    }
    if let Some((_, duration)) = duration {
        Metrics::inc(&state.metrics.sessions_ended);
        log_trace!("zwrs::session", "session closed ({duration}s)");
    }
    let event = crate::events::PlayerEvent {
        uuid: identity.uuid().to_string(),
        username: identity.username().to_string(),
        protocol,
        address: peer.to_string(),
        timestamp: ended_at.to_string(),
        online_count: state.online_count(),
        session_seconds: duration.map_or(0, |(_, seconds)| seconds),
    };
    deliver(state, "player_quit", move |listener| {
        listener.player_quit(&event);
    });
    Ok(())
}

/// Installs (or clears) the embedder that receives player events and unknown
/// console commands. Replacing a listener is allowed; `None` reverts to no-ops.
pub fn set_listener(state: &SharedState, listener: Option<Arc<dyn crate::events::HostListener>>) {
    let mut slot = match state.listener.lock() {
        Ok(slot) => slot,
        Err(poisoned) => poisoned.into_inner(),
    };
    *slot = listener;
}

/// The installed listener, if any. The guard is dropped before the caller runs
/// plugin code so a slow plugin cannot block listener replacement.
fn listener_of(state: &SharedState) -> Option<Arc<dyn crate::events::HostListener>> {
    match state.listener.lock() {
        Ok(slot) => slot.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

/// Delivers an event, containing panics so a broken embedder cannot take the
/// engine down. Called with no engine lock held.
fn deliver<F>(state: &SharedState, what: &str, body: F)
where
    F: FnOnce(&dyn crate::events::HostListener) + std::panic::UnwindSafe,
{
    let Some(listener) = listener_of(state) else {
        return;
    };
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(listener.as_ref()))).is_err()
    {
        log_error!(
            "zwrs::hooks",
            "the host listener panicked while handling {what}; the engine keeps running"
        );
    }
}

/// Sends a console command to the embedder.
///
/// Returns the host's reply so `/help` can list plugin commands and so an
/// unknown command still produces the engine's own error message.
#[must_use]
pub fn dispatch_host_command(
    state: &SharedState,
    command: &str,
    args: &[String],
) -> Option<Vec<String>> {
    let listener = listener_of(state)?;
    let command = command.to_string();
    let args = args.to_vec();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        listener.command(&command, &args)
    })) {
        Ok(crate::events::CommandReply::Handled(lines)) => Some(lines),
        Ok(crate::events::CommandReply::Unknown) => None,
        Err(_) => {
            log_error!(
                "zwrs::hooks",
                "the host listener panicked while handling /{command}; the command was ignored"
            );
            None
        }
    }
}

/// Writes the session registry to disk so history survives a crash.
fn persist_sessions(state: &SharedState) {
    let records = match state.sessions.lock() {
        Ok(sessions) => sessions.to_json_records(),
        Err(poisoned) => poisoned.into_inner().to_json_records(),
    };
    let doc = zwrs_core::storage::document(records);
    if let Err(e) = zwrs_core::storage::write_atomic(
        &state.config.sessions_file(),
        doc.to_pretty().as_bytes(),
    ) {
        log_error!("zwrs::session", "could not persist sessions: {e}");
    }
}

/// Converts a protocol error into a log-friendly string.
#[must_use]
pub fn describe_protocol_error(e: &ProtocolError) -> String {
    e.to_string()
}

/// Logs a line at the configured level (used by the console).
pub fn console_line(level: Level, message: &str) {
    log::log(level, "zwrs::console", message);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Creates an isolated server directory with a matching config.
    pub(crate) fn temp_state(tag: &str, mutate: impl FnOnce(&mut Config)) -> (std::path::PathBuf, Arc<SharedState>) {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("zwrs-engine-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut config = Config::default();
        config.root_dir = root.clone();
        config.data_dir = root.join("data");
        config.plugin_dir = root.join("plugins");
        config.log_dir = root.join("logs");
        config.config_dir = root.join("config");
        config.port = 0; // ask the OS for a free port
        config.online_mode = false;
        mutate(&mut config);
        let state = prepare(config).expect("engine state");
        (root, state)
    }

    pub(crate) fn cleanup(root: &std::path::Path) {
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_forces_stale_online_state_offline() {
        let (root, state) = temp_state("stale", |_| {});
        {
            let mut players = state.players.lock().unwrap();
            let id = zwrs_core::identity::PlayerIdentity::offline(
                zwrs_core::identity::Username::parse("Ghost").unwrap(),
            );
            players
                .record_join(&id, Timestamp::now(), None)
                .expect("join");
        }
        // Re-running prepare simulates a restart.
        drop(state);
        let state = prepare({
            let mut c = Config::default();
            c.root_dir = root.clone();
            c.data_dir = root.join("data");
            c.plugin_dir = root.join("plugins");
            c.log_dir = root.join("logs");
            c.config_dir = root.join("config");
            c.port = 0;
            c.online_mode = false;
            c
        })
        .expect("second prepare");
        assert_eq!(state.online_count(), 0);
        assert!(state.players.lock().unwrap().online().is_empty());
        cleanup(&root);
    }

    #[test]
    fn protocol_error_description_is_readable() {
        let e = ProtocolError::UnknownPacketId(7);
        assert!(describe_protocol_error(&e).contains("7"));
    }

    #[test]
    fn online_count_tracks_sessions() {
        let (root, state) = temp_state("count", |_| {});
        assert_eq!(state.online_count(), 0);
        {
            let id = zwrs_core::identity::PlayerIdentity::offline(
                zwrs_core::identity::Username::parse("Notch").unwrap(),
            );
            state
                .sessions
                .lock()
                .unwrap()
                .begin(&id, Timestamp::now(), 47);
        }
        assert_eq!(state.online_count(), 1);
        assert_eq!(state.online_names(), vec!["Notch".to_string()]);
        cleanup(&root);
    }
}
