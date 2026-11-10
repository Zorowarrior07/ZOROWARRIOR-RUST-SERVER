//! Administration: console commands and status reporting.
//!
//! Commands are plain text so they work identically from an interactive
//! console, a pipe, or the JNI bridge. Every command answers from real state:
//! `/playerinfo` reads the persisted profile, `/players` reads live sessions.

use std::sync::Arc;

use zwrs_core::admin::parse_player_argument;
use zwrs_core::log::Level;
use zwrs_core::profile::format_duration;
use zwrs_core::time::Timestamp;

use crate::plugins::{self, PluginScan};
use crate::server::SharedState;

/// Result of one command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    /// Lines to print.
    pub lines: Vec<String>,
    /// Whether the engine should keep running.
    pub keep_running: bool,
    /// Whether the command was recognised.
    pub recognised: bool,
}

impl CommandOutcome {
    fn ok(lines: Vec<String>) -> Self {
        Self {
            lines,
            keep_running: true,
            recognised: true,
        }
    }

    fn unknown(command: &str) -> Self {
        Self {
            lines: vec![
                format!("Unknown command `{command}`. Try /help."),
            ],
            keep_running: true,
            recognised: false,
        }
    }
}

/// Executes one console command line (with or without a leading `/`).
///
/// Commands that change the server (`/reload`, `/stop`) report what they did;
/// `/reload` re-reads the plugin directory and player data, and `/stop` asks the
/// engine to shut down gracefully.
pub fn execute(state: &Arc<SharedState>, line: &str) -> CommandOutcome {
    let trimmed = line.trim().trim_start_matches('/').trim();
    if trimmed.is_empty() {
        return CommandOutcome::ok(Vec::new());
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or("").to_ascii_lowercase();
    let argument = parts.next().unwrap_or("").trim();

    match command.as_str() {
        "help" | "?" => CommandOutcome::ok(help_lines()),
        "version" | "ver" => CommandOutcome::ok(version_lines(state)),
        "status" => CommandOutcome::ok(status_lines(state)),
        "players" | "list" => {
            let players = match state.players.lock() {
                Ok(players) => players,
                Err(poisoned) => poisoned.into_inner(),
            };
            CommandOutcome::ok(players.players_lines())
        }
        "playerinfo" | "pi" => {
            if argument.is_empty() {
                return CommandOutcome::ok(vec![
                    "Usage: /playerinfo <username>".to_string(),
                ]);
            }
            let sessions = match state.sessions.lock() {
                Ok(sessions) => sessions,
                Err(poisoned) => poisoned.into_inner(),
            };
            let now = Timestamp::now();
            let parsed = parse_player_argument(argument).ok();
            let lookup = parsed
                .as_ref()
                .map_or(argument, zwrs_core::identity::Username::as_str);
            let session_age = parsed.as_ref().and_then(|name| {
                sessions
                    .all()
                    .iter()
                    .find(|s| s.is_active() && s.username() == name)
                    .map(|s| s.started_at().saturating_until(now))
            });
            drop(sessions);
            let players = match state.players.lock() {
                Ok(players) => players,
                Err(poisoned) => poisoned.into_inner(),
            };
            let mut lines = players.playerinfo_lines(lookup, now, session_age);
            if parsed.is_none() {
                lines.insert(
                    0,
                    format!("`{argument}` is not a valid username, so it was matched as stored text."),
                );
            }
            CommandOutcome::ok(lines)
        }
        "plugins" => CommandOutcome::ok(plugin_lines(&scan_plugins(state))),
        "reload" => {
            let mut lines = Vec::new();
            {
                let players = match state.players.lock() {
                    Ok(players) => players,
                    Err(poisoned) => poisoned.into_inner(),
                };
                match players.save() {
                    Ok(()) => lines.push(format!(
                        "Player data flushed to {}",
                        state.config.players_file().display()
                    )),
                    Err(e) => lines.push(format!("Could not flush player data: {e}")),
                }
            }
            let scan = scan_plugins(state);
            lines.push(format!(
                "Plugin directory rescan: {} artifact(s), {} loadable.",
                scan.plugins.len(),
                scan.loadable_count()
            ));
            lines.push(
                "Configuration is not hot-reloaded in this build; restart the server to apply config/zwrs.properties changes."
                    .to_string(),
            );
            CommandOutcome::ok(lines)
        }
        "stop" | "shutdown" | "end" => CommandOutcome {
            lines: vec![
                "Stopping the engine: closing sessions and saving player data.".to_string(),
            ],
            keep_running: false,
            recognised: true,
        },
        other => CommandOutcome::unknown(other),
    }
}

/// Scans the configured plugin directory.
#[must_use]
pub fn scan_plugins(state: &SharedState) -> PluginScan {
    match plugins::scan(&state.config.plugin_dir) {
        Ok(scan) => scan,
        Err(e) => {
            let mut scan = PluginScan::default();
            scan.directory_present = state.config.plugin_dir.exists();
            scan.problems.push(format!(
                "could not read {}: {e}",
                state.config.plugin_dir.display()
            ));
            scan
        }
    }
}

fn help_lines() -> Vec<String> {
    vec![
        "Available commands:".to_string(),
        "  /help              show this help".to_string(),
        "  /version           engine version, platform and protocol range".to_string(),
        "  /status            uptime, connections, players, plugins".to_string(),
        "  /players           players online right now".to_string(),
        "  /playerinfo <name> persisted profile for a player".to_string(),
        "  /plugins           discovered Java and Rust plugin artifacts".to_string(),
        "  /reload            flush player data and rescan plugins".to_string(),
        "  /stop              graceful shutdown".to_string(),
        "Commands can be typed with or without a leading '/'.".to_string(),
    ]
}

fn version_lines(state: &SharedState) -> Vec<String> {
    let info = crate::BuildInfo::current();
    vec![
        format!("{} {}", zwrs_core::PRODUCT, info.version),
        format!("Engine: Rust ({}) — the Rust engine runs the server.", info.platform),
        format!("Build profile: {}", info.profile),
        format!(
            "Minecraft protocol: {}–{} ({} is advertised)",
            info.min_protocol, info.max_protocol, crate::VERSION_NAME
        ),
        format!("Authentication: {}", state.authenticator.describe()),
        "Play state (world, chunks, entities, inventory): NOT implemented in this build.".to_string(),
    ]
}

fn status_lines(state: &SharedState) -> Vec<String> {
    let sessions = match state.sessions.lock() {
        Ok(sessions) => sessions,
        Err(poisoned) => poisoned.into_inner(),
    };
    let online = sessions.online_count();
    let retained = sessions.len();
    drop(sessions);

    let known = match state.players.lock() {
        Ok(players) => players.len(),
        Err(poisoned) => poisoned.into_inner().len(),
    };

    let bound = state
        .bound_address
        .lock()
        .ok()
        .and_then(|a| *a)
        .map_or_else(|| format!("{}:{}", state.config.bind_address, state.config.port), |a| a.to_string());

    let scan = scan_plugins(state);
    let metrics = &state.metrics;
    use std::sync::atomic::Ordering::Relaxed;

    let mut lines = vec![
        format!("Server: {}", state.config.server_name),
        format!("Address: {bound}"),
        format!("Uptime: {}", format_duration(state.uptime_seconds())),
        format!(
            "Players: {online}/{} online, {known} known profile(s), {retained} session record(s) retained",
            state.config.max_players
        ),
        format!(
            "Connections: {} accepted, {} status pings, {} logins admitted, {} refused",
            metrics.connections_accepted.load(Relaxed),
            metrics.status_pings.load(Relaxed),
            metrics.logins_admitted.load(Relaxed),
            metrics.logins_refused.load(Relaxed),
        ),
        format!(
            "Errors: {} protocol error(s)",
            metrics.protocol_errors.load(Relaxed)
        ),
        format!(
            "Plugins: {} artifact(s) in {} ({} loadable)",
            scan.plugins.len(),
            state.config.plugin_dir.display(),
            scan.loadable_count()
        ),
        format!("Data: {}", state.config.data_dir.display()),
        format!("Auth mode: {}", state.authenticator.describe()),
    ];
    for note in scan.problems {
        lines.push(format!("  plugin problem: {note}"));
    }
    lines
}

fn plugin_lines(scan: &PluginScan) -> Vec<String> {
    let mut lines = scan.summary_lines();
    lines.push(format!(
        "Plugin API: Rust/WASM manifest v{} (validation only in this build); Java plugins are inspected by the Java host.",
        plugins::PLUGIN_API_VERSION
    ));
    lines
}

/// Logs the startup banner.
pub fn print_banner(state: &SharedState) {
    let info = crate::BuildInfo::current();
    zwrs_core::log::log(
        Level::Info,
        "zwrs",
        &format!(
            "{} {} starting on {} ({})",
            zwrs_core::PRODUCT,
            info.version,
            info.platform,
            std::env::consts::OS
        ),
    );
    zwrs_core::log::log(
        Level::Info,
        "zwrs",
        &format!(
            "data={} plugins={} online-mode={}",
            state.config.data_dir.display(),
            state.config.plugin_dir.display(),
            state.config.online_mode
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(tag: &str) -> (std::path::PathBuf, Arc<SharedState>) {
        crate::server::tests::temp_state(tag, |_| {})
    }

    #[test]
    fn help_and_unknown_commands() {
        let (root, state) = state("admin-help");
        let help = execute(&state, "/help");
        assert!(help.recognised);
        assert!(help.lines.iter().any(|l| l.contains("/playerinfo")));

        let unknown = execute(&state, "/frobnicate");
        assert!(!unknown.recognised);
        assert!(unknown.lines[0].contains("Unknown command"));
        crate::server::tests::cleanup(&root);
    }

    #[test]
    fn stop_requests_shutdown() {
        let (root, state) = state("admin-stop");
        assert!(!execute(&state, "/stop").keep_running);
        crate::server::tests::cleanup(&root);
    }

    #[test]
    fn playerinfo_requires_an_argument() {
        let (root, state) = state("admin-pi");
        let out = execute(&state, "/playerinfo");
        assert!(out.lines[0].contains("Usage"));
        crate::server::tests::cleanup(&root);
    }

    #[test]
    fn playerinfo_reports_stored_profile_after_real_join() {
        let (root, state) = state("admin-pi2");
        {
            let mut players = state.players.lock().unwrap();
            let id = zwrs_core::identity::PlayerIdentity::offline(
                zwrs_core::identity::Username::parse("Notch").unwrap(),
            );
            players.record_join(&id, Timestamp::now(), None).unwrap();
        }
        let out = execute(&state, "/playerinfo Notch");
        let text = out.lines.join("\n");
        assert!(text.contains("Username: Notch"), "{text}");
        assert!(text.contains("Identity mode: offline"), "{text}");
        assert!(text.contains("Online: yes"), "{text}");
        crate::server::tests::cleanup(&root);
    }

    #[test]
    fn playerinfo_unknown_player_says_so() {
        let (root, state) = state("admin-pi3");
        let out = execute(&state, "/playerinfo Nobody");
        assert!(out.lines.join("\n").contains("No stored profile"));
        crate::server::tests::cleanup(&root);
    }

    #[test]
    fn status_reports_real_counts() {
        let (root, state) = state("admin-status");
        let out = execute(&state, "/status");
        let text = out.lines.join("\n");
        assert!(text.contains("Players: 0/20 online"), "{text}");
        assert!(text.contains("0 known profile(s)"), "{text}");
        assert!(text.contains("Auth mode:"), "{text}");
        crate::server::tests::cleanup(&root);
    }

    #[test]
    fn version_does_not_overclaim() {
        let (root, state) = state("admin-version");
        let text = execute(&state, "/version").lines.join("\n");
        assert!(text.contains("Rust engine runs the server"), "{text}");
        assert!(text.to_lowercase().contains("not implemented"), "{text}");
        crate::server::tests::cleanup(&root);
    }

    #[test]
    fn reload_flushes_data() {
        let (root, state) = state("admin-reload");
        let out = execute(&state, "/reload");
        let text = out.lines.join("\n");
        assert!(text.contains("Player data flushed"), "{text}");
        assert!(text.contains("not hot-reloaded"), "{text}");
        crate::server::tests::cleanup(&root);
    }
}
