//! Standalone entry point for the Rust engine.
//!
//! Users normally start ZWRS with `java -jar ZOROWARRIOR-Rust-Server.jar`, which
//! loads the shared library through JNI. This binary exists so the engine can
//! also be run, tested and debugged directly:
//!
//! ```text
//! zwrs-engine [--dir <server directory>] [--status] [--version]
//! ```
//!
//! `--status` prints the same information as the `/status` console command and
//! exits, which is what CI and health checks use.

use std::io::{BufRead, Write};
use std::sync::Arc;

use zwrs_core::config::Config;
use zwrs_core::log::Level;
use zwrs_core::storage::write_atomic;
use zwrs_core::Timestamp;
use zwrs_engine::{admin, prepare, start, BuildInfo, SharedState};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut root = std::path::PathBuf::from(".");
    let mut status_only = false;
    let mut print_version = false;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--dir" | "-d" => {
                index += 1;
                match args.get(index) {
                    Some(dir) => root = std::path::PathBuf::from(dir),
                    None => fail("--dir requires a path"),
                }
            }
            "--status" => status_only = true,
            "--version" | "-v" => print_version = true,
            "--help" | "-h" => {
                println!(
                    "{} {}\n\nUsage: zwrs-engine [--dir <server directory>] [--status] [--version]\n\n  --dir <path>  server directory containing config/ and data/ (default: .)\n  --status      print server status and exit\n  --version     print version information and exit\n\nThe engine is normally started by ZOROWARRIOR-Rust-Server.jar; this binary is for direct use and diagnostics.",
                    zwrs_core::PRODUCT,
                    BuildInfo::current().version
                );
                return;
            }
            other => fail(&format!("unknown argument `{other}` (try --help)")),
        }
        index += 1;
    }

    if print_version {
        println!("{}", BuildInfo::current().to_json());
        return;
    }

    let config = match Config::load(&root.join("config")) {
        Ok(mut config) => {
            // Paths in the properties file are relative to the server directory.
            if config.root_dir.as_os_str().is_empty() || config.root_dir == std::path::Path::new(".") {
                config.root_dir = root.clone();
                config.data_dir = root.join(
                    config
                        .data_dir
                        .file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("data")),
                );
                config.plugin_dir = root.join(
                    config
                        .plugin_dir
                        .file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("plugins")),
                );
                config.log_dir = root.join(
                    config
                        .log_dir
                        .file_name()
                        .unwrap_or_else(|| std::ffi::OsStr::new("logs")),
                );
            }
            config
        }
        Err(e) => fail(&format!("configuration problem: {e}")),
    };

    let state = match prepare(config) {
        Ok(state) => state,
        Err(e) => fail(&format!("startup failed: {e}")),
    };

    admin::print_banner(&state);

    if status_only {
        for line in admin::execute(&state, "/status").lines {
            println!("{line}");
        }
        return;
    }

    let mut handle = match start(Arc::clone(&state)) {
        Ok(handle) => handle,
        Err(e) => fail(&e),
    };

    write_pid_file(&state);

    // Console: stdin lines are commands; EOF (or /stop) triggers shutdown.
    run_console(&state, &mut handle);

    zwrs_core::log_info!("zwrs", "engine stopped");
}

fn run_console(state: &Arc<SharedState>, handle: &mut zwrs_engine::ServerHandle) {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        if !handle.is_running() {
            break;
        }
        let next = lines.next();
        match next {
            Some(Ok(line)) => {
                let outcome = admin::execute(state, &line);
                for text in &outcome.lines {
                    if !text.is_empty() {
                        let _ = writeln!(std::io::stdout(), "{text}");
                    }
                }
                let _ = std::io::stdout().flush();
                if !outcome.keep_running {
                    break;
                }
            }
            Some(Err(e)) => {
                zwrs_core::log_warn!("zwrs::console", "stdin error: {e}");
                break;
            }
            None => break,
        }
    }
    if let Err(e) = handle.stop() {
        zwrs_core::log_error!("zwrs", "shutdown problem: {e}");
    }
}

fn write_pid_file(state: &SharedState) {
    let pid = std::process::id();
    let mut doc = zwrs_core::Json::object();
    doc.insert("pid".into(), zwrs_core::Json::Number(f64::from(pid)));
    doc.insert(
        "started_at".into(),
        zwrs_core::Json::String(Timestamp::now().to_string()),
    );
    doc.insert(
        "version".into(),
        zwrs_core::Json::String(BuildInfo::current().version.into()),
    );
    let path = state.config.data_dir.join("engine.pid.json");
    if let Err(e) = write_atomic(&path, zwrs_core::Json::Object(doc).to_pretty().as_bytes()) {
        zwrs_core::log_warn!("zwrs", "could not write {}: {e}", path.display());
    }
}

fn fail(message: &str) -> ! {
    zwrs_core::log::set_level(Level::Error);
    zwrs_core::log_error!("zwrs", "{message}");
    std::process::exit(1);
}
