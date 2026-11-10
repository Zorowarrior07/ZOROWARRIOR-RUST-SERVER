//! Server configuration.
//!
//! # File format
//!
//! Configuration lives in `config/zwrs.properties` as `key = value` lines.
//! Properties were chosen over JSON for this file because it is meant to be
//! hand-edited on a phone: comments are allowed, order does not matter, and a
//! single typo cannot break the whole document. Player data (which machines
//! read and write) is JSON.
//!
//! ```text
//! # comments start with '#' or ';'
//! port = 25565
//! motd = A ZOROWARRIOR server
//! online-mode = true
//! ```
//!
//! Unknown keys are **reported as errors** rather than ignored, so a
//! misspelled key cannot silently do nothing. All errors are collected so the
//! operator sees every problem at once.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::log::Level;

/// Default Minecraft port.
pub const DEFAULT_PORT: u16 = 25565;
/// Name of the configuration file inside the config directory.
pub const CONFIG_FILE_NAME: &str = "zwrs.properties";

/// Oldest protocol version ZWRS accepts connections from.
pub const MIN_SUPPORTED_PROTOCOL: i32 = 47; // 1.8.x
/// Newest protocol version ZWRS accepts connections from.
pub const MAX_SUPPORTED_PROTOCOL: i32 = 772; // 1.21.7/1.21.8

/// Fully validated server configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Display name used in the status response and version string.
    pub server_name: String,
    /// TCP port to bind.
    pub port: u16,
    /// Address to bind; `0.0.0.0` accepts connections from other devices.
    pub bind_address: String,
    /// Message of the day shown in the client's server list.
    pub motd: String,
    /// Whether players must authenticate with Mojang/Microsoft.
    ///
    /// When `false`, UUIDs are derived from usernames and **anyone** can
    /// connect using any username. Only use offline mode on a trusted network.
    pub online_mode: bool,
    /// Maximum simultaneous players advertised and enforced.
    pub max_players: u32,
    /// Chunk view distance in chunks (2–32).
    pub view_distance: u8,
    /// Simulation distance in chunks (2–32, never larger than `view_distance`).
    pub simulation_distance: u8,
    /// Directory holding `config/`, `data/`, `plugins/`, `logs/`.
    pub root_dir: PathBuf,
    /// Directory containing this configuration file.
    pub config_dir: PathBuf,
    /// Directory for player and session data.
    pub data_dir: PathBuf,
    /// Directory scanned for plugins (Java `.jar` and Rust `.wasm`).
    pub plugin_dir: PathBuf,
    /// Directory for rotated log files written by the launcher.
    pub log_dir: PathBuf,
    /// Minimum severity that is emitted.
    pub log_level: Level,
    /// Seconds between automatic data directory snapshots; `0` disables.
    pub backup_interval_seconds: u64,
    /// How many snapshots to retain.
    pub backup_keep: u32,
    /// Longest accepted username, clamped to vanilla's 16.
    pub session_join_timeout_seconds: u64,
    /// Seconds a connection may stay silent before it is dropped.
    pub connection_timeout_seconds: u64,
    /// Whether to persist the last known IP address in player profiles.
    ///
    /// Disabled by default: IP addresses are personal data.
    pub store_ip_addresses: bool,
    /// Enables the Java plugin host (Bukkit-compatible bridge).
    pub java_plugins_enabled: bool,
    /// Enables loading Rust `.wasm` plugins.
    pub wasm_plugins_enabled: bool,
    /// Optional override of the maximum supported protocol version.
    pub max_protocol_override: Option<i32>,
}

impl Default for Config {
    fn default() -> Self {
        let root = PathBuf::from(".");
        Self {
            server_name: "ZOROWARRIOR Rust Server".to_string(),
            port: DEFAULT_PORT,
            bind_address: "0.0.0.0".to_string(),
            motd: "A ZOROWARRIOR Rust Server".to_string(),
            online_mode: true,
            max_players: 20,
            view_distance: 8,
            simulation_distance: 6,
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            plugin_dir: root.join("plugins"),
            log_dir: root.join("logs"),
            root_dir: root,
            log_level: Level::Info,
            backup_interval_seconds: 3600,
            backup_keep: 5,
            session_join_timeout_seconds: 30,
            connection_timeout_seconds: 30,
            store_ip_addresses: false,
            java_plugins_enabled: true,
            wasm_plugins_enabled: true,
            max_protocol_override: None,
        }
    }
}

impl Config {
    /// Path of `players.json`.
    #[must_use]
    pub fn players_file(&self) -> PathBuf {
        self.data_dir.join("players").join("players.json")
    }

    /// Path of `sessions.json`.
    #[must_use]
    pub fn sessions_file(&self) -> PathBuf {
        self.data_dir.join("players").join("sessions.json")
    }

    /// Directory holding per-player profile documents.
    #[must_use]
    pub fn profiles_dir(&self) -> PathBuf {
        self.data_dir.join("players").join("profiles")
    }

    /// Path of the properties file.
    #[must_use]
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join(CONFIG_FILE_NAME)
    }

    /// Highest protocol version this build accepts.
    #[must_use]
    pub fn max_protocol(&self) -> i32 {
        match self.max_protocol_override {
            Some(v) => v.clamp(MIN_SUPPORTED_PROTOCOL, MAX_SUPPORTED_PROTOCOL),
            None => MAX_SUPPORTED_PROTOCOL,
        }
    }

    /// Renders the default configuration file contents.
    #[must_use]
    pub fn render_default_file() -> String {
        let d = Self::default();
        let mut out = String::new();
        out.push_str("# ZOROWARRIOR Rust Server configuration\n");
        out.push_str("# Lines starting with '#' or ';' are comments. Unknown keys are rejected.\n");
        out.push('\n');
        out.push_str("# --- identity and binding -------------------------------------------\n");
        out.push_str(&format!("server-name = {}\n", d.server_name));
        out.push_str(&format!("port = {}\n", d.port));
        out.push_str(&format!("bind-address = {}\n", d.bind_address));
        out.push_str(&format!("motd = {}\n", d.motd));
        out.push_str("# true = Mojang/Microsoft authentication (recommended for public servers)\n");
        out.push_str("# false = offline mode: UUIDs are derived from usernames and are NOT verified\n");
        out.push_str(&format!("online-mode = {}\n", d.online_mode));
        out.push_str(&format!("max-players = {}\n", d.max_players));
        out.push_str("\n# --- world ------------------------------------------------------------\n");
        out.push_str(&format!("view-distance = {}\n", d.view_distance));
        out.push_str(&format!("simulation-distance = {}\n", d.simulation_distance));
        out.push_str("\n# --- paths (relative to the server directory) -------------------------\n");
        out.push_str("data-dir = data\n");
        out.push_str("plugin-dir = plugins\n");
        out.push_str("log-dir = logs\n");
        out.push_str("\n# --- logging ----------------------------------------------------------\n");
        out.push_str("# error | warn | info | debug | trace\n");
        out.push_str(&format!("log-level = {}\n", d.log_level.as_str().to_ascii_lowercase()));
        out.push_str("\n# --- data protection --------------------------------------------------\n");
        out.push_str("# Seconds between data snapshots (0 disables periodic backups).\n");
        out.push_str(&format!("backup-interval-seconds = {}\n", d.backup_interval_seconds));
        out.push_str(&format!("backup-keep = {}\n", d.backup_keep));
        out.push_str("# Store last-seen IP addresses in player profiles. Off by default (personal data).\n");
        out.push_str(&format!("store-ip-addresses = {}\n", d.store_ip_addresses));
        out.push_str("\n# --- connections ------------------------------------------------------\n");
        out.push_str(&format!(
            "connection-timeout-seconds = {}\n",
            d.connection_timeout_seconds
        ));
        out.push_str(&format!(
            "session-join-timeout-seconds = {}\n",
            d.session_join_timeout_seconds
        ));
        out.push_str("\n# --- extensions -------------------------------------------------------\n");
        out.push_str(&format!("java-plugins-enabled = {}\n", d.java_plugins_enabled));
        out.push_str(&format!("wasm-plugins-enabled = {}\n", d.wasm_plugins_enabled));
        out.push_str(&format!(
            "# Clamp the newest accepted protocol version ({}–{}).\n",
            MIN_SUPPORTED_PROTOCOL, MAX_SUPPORTED_PROTOCOL
        ));
        out.push_str("# max-protocol-override = 772\n");
        out
    }

    /// Loads configuration from `dir/zwrs.properties`, creating the file with
    /// defaults when it does not exist.
    ///
    /// # Errors
    /// Returns [`Error::Config`] describing every invalid key or value.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(CONFIG_FILE_NAME);
        if !path.exists() {
            std::fs::create_dir_all(dir)?;
            crate::storage::write_atomic(&path, Self::render_default_file().as_bytes())?;
            let mut cfg = Self::default();
            cfg.config_dir = dir.to_path_buf();
            cfg.root_dir = dir.parent().unwrap_or(Path::new(".")).to_path_buf();
            return Ok(cfg);
        }
        let text = std::fs::read_to_string(&path)?;
        let mut cfg = Self::parse(&text)?;
        cfg.config_dir = dir.to_path_buf();
        cfg.root_dir = dir.parent().unwrap_or(Path::new(".")).to_path_buf();
        Ok(cfg)
    }

    /// Parses properties text into a validated configuration.
    ///
    /// # Errors
    /// Returns [`Error::Config`] with a semicolon-separated list of problems.
    pub fn parse(text: &str) -> Result<Self> {
        let mut cfg = Self::default();
        let mut problems: Vec<String> = Vec::new();
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();

        for (idx, raw) in text.lines().enumerate() {
            let line_no = idx + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                problems.push(format!("line {line_no}: expected `key = value`"));
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            if key.is_empty() {
                problems.push(format!("line {line_no}: empty key"));
                continue;
            }
            if let Some(prev) = seen.insert(key.clone(), line_no) {
                problems.push(format!(
                    "line {line_no}: duplicate key `{key}` (already set on line {prev})"
                ));
                continue;
            }
            if let Err(msg) = cfg.apply(&key, value) {
                problems.push(format!("line {line_no}: `{key}`: {msg}"));
            }
        }

        if problems.is_empty() {
            cfg.validate()?;
            Ok(cfg)
        } else {
            Err(Error::Config(problems.join("; ")))
        }
    }

    fn apply(&mut self, key: &str, value: &str) -> std::result::Result<(), String> {
        fn bool_of(v: &str) -> std::result::Result<bool, String> {
            match v.to_ascii_lowercase().as_str() {
                "true" | "yes" | "on" | "1" => Ok(true),
                "false" | "no" | "off" | "0" => Ok(false),
                other => Err(format!("expected true/false, got `{other}`")),
            }
        }
        fn int_of<T: std::str::FromStr>(v: &str) -> std::result::Result<T, String> {
            v.parse::<T>().map_err(|_| format!("expected a number, got `{v}`"))
        }
        match key {
            "server-name" => self.server_name = value.to_string(),
            "port" => self.port = int_of(value)?,
            "bind-address" => self.bind_address = value.to_string(),
            "motd" => self.motd = value.to_string(),
            "online-mode" => self.online_mode = bool_of(value)?,
            "max-players" => self.max_players = int_of(value)?,
            "view-distance" => self.view_distance = int_of(value)?,
            "simulation-distance" => self.simulation_distance = int_of(value)?,
            "data-dir" => self.data_dir = PathBuf::from(value),
            "plugin-dir" => self.plugin_dir = PathBuf::from(value),
            "log-dir" => self.log_dir = PathBuf::from(value),
            "log-level" => {
                self.log_level = Level::parse(value)
                    .ok_or_else(|| format!("unknown level `{value}` (error|warn|info|debug|trace)"))?;
            }
            "backup-interval-seconds" => self.backup_interval_seconds = int_of(value)?,
            "backup-keep" => self.backup_keep = int_of(value)?,
            "connection-timeout-seconds" => self.connection_timeout_seconds = int_of(value)?,
            "session-join-timeout-seconds" => self.session_join_timeout_seconds = int_of(value)?,
            "store-ip-addresses" => self.store_ip_addresses = bool_of(value)?,
            "java-plugins-enabled" => self.java_plugins_enabled = bool_of(value)?,
            "wasm-plugins-enabled" => self.wasm_plugins_enabled = bool_of(value)?,
            "max-protocol-override" => {
                let v: i32 = int_of(value)?;
                self.max_protocol_override = Some(v);
            }
            other => return Err(format!("unknown key `{other}`")),
        }
        Ok(())
    }

    /// Checks cross-field invariants.
    ///
    /// # Errors
    /// Returns [`Error::Config`] listing every violation.
    pub fn validate(&self) -> Result<()> {
        let mut problems = Vec::new();
        if self.server_name.trim().is_empty() {
            problems.push("server-name must not be empty".to_string());
        }
        if self.server_name.len() > 64 {
            problems.push("server-name must be at most 64 characters".to_string());
        }
        if self.motd.len() > 256 {
            problems.push("motd must be at most 256 characters".to_string());
        }
        if self.port == 0 {
            problems.push("port must be 1–65535".to_string());
        }
        if self.bind_address.trim().is_empty() {
            problems.push("bind-address must not be empty".to_string());
        }
        if self.max_players == 0 || self.max_players > 10_000 {
            problems.push("max-players must be 1–10000".to_string());
        }
        if !(2..=32).contains(&self.view_distance) {
            problems.push("view-distance must be 2–32".to_string());
        }
        if !(2..=32).contains(&self.simulation_distance) {
            problems.push("simulation-distance must be 2–32".to_string());
        }
        if self.simulation_distance > self.view_distance {
            problems.push(format!(
                "simulation-distance ({}) must not exceed view-distance ({})",
                self.simulation_distance, self.view_distance
            ));
        }
        if self.backup_keep == 0 || self.backup_keep > 1000 {
            problems.push("backup-keep must be 1–1000".to_string());
        }
        if self.connection_timeout_seconds < 5 || self.connection_timeout_seconds > 600 {
            problems.push("connection-timeout-seconds must be 5–600".to_string());
        }
        if self.session_join_timeout_seconds < 5 || self.session_join_timeout_seconds > 300 {
            problems.push("session-join-timeout-seconds must be 5–300".to_string());
        }
        if self.data_dir.as_os_str().is_empty()
            || self.plugin_dir.as_os_str().is_empty()
            || self.log_dir.as_os_str().is_empty()
        {
            problems.push("data-dir, plugin-dir and log-dir must not be empty".to_string());
        }
        if let Some(proto) = self.max_protocol_override {
            if !(MIN_SUPPORTED_PROTOCOL..=MAX_SUPPORTED_PROTOCOL).contains(&proto) {
                problems.push(format!(
                    "max-protocol-override must be {MIN_SUPPORTED_PROTOCOL}–{MAX_SUPPORTED_PROTOCOL}"
                ));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(Error::Config(problems.join("; ")))
        }
    }

    /// Creates the directory layout this configuration expects.
    ///
    /// # Errors
    /// Propagates I/O failures.
    pub fn ensure_directories(&self) -> Result<()> {
        for dir in [
            &self.data_dir,
            &self.profiles_dir(),
            &self.plugin_dir,
            &self.log_dir,
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let cfg = Config::default();
        cfg.validate().expect("defaults must validate");
        assert_eq!(cfg.port, DEFAULT_PORT);
        assert!(cfg.online_mode, "safe default is authenticated mode");
        assert!(!cfg.store_ip_addresses, "ip storage is opt-in");
    }

    #[test]
    fn default_file_parses_back_to_defaults() {
        let text = Config::render_default_file();
        let parsed = Config::parse(&text).expect("default file must parse");
        // Paths are rendered relative, so compare the fields the file controls.
        assert_eq!(parsed.port, Config::default().port);
        assert_eq!(parsed.motd, Config::default().motd);
        assert_eq!(parsed.log_level, Level::Info);
        assert_eq!(parsed.data_dir, PathBuf::from("data"));
        parsed.validate().unwrap();
    }

    #[test]
    fn parses_overrides_and_comments() {
        let text = "# comment\n; also comment\n\nport=25566\nmotd = Hello  World \nonline-mode = false\n";
        let cfg = Config::parse(text).unwrap();
        assert_eq!(cfg.port, 25566);
        assert_eq!(cfg.motd, "Hello  World");
        assert!(!cfg.online_mode);
    }

    #[test]
    fn unknown_key_is_an_error() {
        let err = Config::parse("prot = 25565").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("prot"), "{msg}");
        assert!(msg.contains("unknown key"), "{msg}");
    }

    #[test]
    fn duplicate_key_is_an_error() {
        let err = Config::parse("port=1\nport=2").unwrap_err();
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn multiple_errors_all_reported() {
        let err = Config::parse("port=abc\nview-distance=99\nbogus=1").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("port"), "{msg}");
        assert!(msg.contains("bogus"), "{msg}");

        // Cross-field problems are reported by `validate` with every violation.
        let err = Config::parse("view-distance=99\nsimulation-distance=40").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("view-distance"), "{msg}");
        assert!(msg.contains("simulation-distance"), "{msg}");
    }

    #[test]
    fn cross_field_validation() {
        let mut cfg = Config::default();
        cfg.view_distance = 4;
        cfg.simulation_distance = 8;
        assert!(cfg.validate().is_err());

        cfg.simulation_distance = 4;
        assert!(cfg.validate().is_ok());

        cfg.port = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn protocol_override_is_clamped() {
        let mut cfg = Config::default();
        cfg.max_protocol_override = Some(763);
        assert_eq!(cfg.max_protocol(), 763);
        cfg.max_protocol_override = None;
        assert_eq!(cfg.max_protocol(), MAX_SUPPORTED_PROTOCOL);
    }

    #[test]
    fn booleans_accept_common_spellings() {
        for truthy in ["true", "yes", "on", "1", "TRUE"] {
            let cfg = Config::parse(&format!("online-mode = {truthy}")).unwrap();
            assert!(cfg.online_mode);
        }
        for falsy in ["false", "no", "off", "0"] {
            let cfg = Config::parse(&format!("online-mode = {falsy}")).unwrap();
            assert!(!cfg.online_mode);
        }
        assert!(Config::parse("online-mode = maybe").is_err());
    }
}
