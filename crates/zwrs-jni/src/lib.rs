//! JNI bridge: `io.zorowarrior.zwrs.NativeBridge` -> Rust engine.
//!
//! # Contract
//!
//! The Java launcher (`ZOROWARRIOR-Rust-Server.jar`) loads this shared library
//! and calls the `native` methods of `io.zorowarrior.zwrs.NativeBridge`. Every
//! method returns JSON (or a primitive) and throws a Java exception on failure:
//!
//! | Java method | Rust entry point |
//! |---|---|
//! | `nativeBridgeVersion()` | [`Java_io_zorowarrior_zwrs_NativeBridge_nativeBridgeVersion`] |
//! | `nativeBuildInfo()` | [`Java_io_zorowarrior_zwrs_NativeBridge_nativeBuildInfo`] |
//! | `nativeSelfTest()` | [`Java_io_zorowarrior_zwrs_NativeBridge_nativeSelfTest`] |
//! | `nativeStart(dir, port[, onlineMode])` | [`Java_io_zorowarrior_zwrs_NativeBridge_nativeStart`] |
//! | `nativeCommand(line)` | [`Java_io_zorowarrior_zwrs_NativeBridge_nativeCommand`] |
//! | `nativeStop()` | [`Java_io_zorowarrior_zwrs_NativeBridge_nativeStop`] |
//! | `nativeState()` | [`Java_io_zorowarrior_zwrs_NativeBridge_nativeState`] |
//!
//! The engine runs on its own threads. `nativeStart` returns as soon as the
//! listening socket is bound, and `nativeStop` performs a graceful shutdown
//! (sessions closed, player data saved) before returning.
//!
//! # Safety
//!
//! All `extern "C"` functions below are `unsafe` in Rust's model. They are
//! written defensively: null environment or argument checks, panic catching, and
//! no unwinding across the FFI boundary.

#![deny(missing_docs)]

pub mod bridge;
mod host;
pub mod sys;

use std::os::raw::c_void;
use std::sync::{Arc, Mutex, OnceLock};

use zwrs_core::config::Config;
use zwrs_core::log::Level;
use zwrs_engine::{ServerHandle, SharedState};

use bridge::{bridge_call, read_jstring, BridgeError};

/// Version of the Java <-> Rust bridge contract.
///
/// Bump this whenever the JSON shapes or method set change; the Java side
/// refuses to run against a mismatched native library.
pub const JNI_BRIDGE_VERSION: u32 = 2;

/// Global engine handle. The JVM loads the library once, so a process-wide
/// singleton matches the Java `System.load` model.
static ENGINE: OnceLock<Mutex<Option<RunningEngine>>> = OnceLock::new();

struct RunningEngine {
    state: Arc<SharedState>,
    handle: ServerHandle,
    directory: String,
}

fn engine_slot() -> &'static Mutex<Option<RunningEngine>> {
    ENGINE.get_or_init(|| Mutex::new(None))
}

/// `nativeBridgeVersion() -> int`
///
/// # Safety
/// Called by the JVM.
#[no_mangle]
pub unsafe extern "C" fn Java_io_zorowarrior_zwrs_NativeBridge_nativeBridgeVersion(
    _env: *mut c_void,
    _class: *mut c_void,
) -> i32 {
    i32::try_from(JNI_BRIDGE_VERSION).unwrap_or(0)
}

/// `nativeBuildInfo() -> String` (JSON)
///
/// # Safety
/// Called by the JVM with a valid environment.
#[no_mangle]
pub unsafe extern "C" fn Java_io_zorowarrior_zwrs_NativeBridge_nativeBuildInfo(
    env: *mut c_void,
    _class: *mut c_void,
) -> sys::JString {
    bridge_call(env, || Ok(bridge::build_info_json()))
}

/// A real cross-boundary self-test: the JVM hands over a string, Rust computes
/// the vanilla offline UUID with it and returns JSON containing both the echo
/// and the derived UUID. The Java side asserts the same value independently, so
/// a mismatch proves the bridge is corrupt.
///
/// `nativeSelfTest(String username) -> String`
///
/// # Safety
/// Called by the JVM with a valid environment and a `jstring` (may be null).
#[no_mangle]
pub unsafe extern "C" fn Java_io_zorowarrior_zwrs_NativeBridge_nativeSelfTest(
    env: *mut c_void,
    _class: *mut c_void,
    username: sys::JString,
) -> sys::JString {
    let name = read_jstring(env, username);
    bridge_call(env, || {
        let name = name.ok_or_else(|| BridgeError::Runtime("self-test requires a username".into()))?;
        let parsed = zwrs_core::Username::parse(&name)
            .map_err(|e| BridgeError::Runtime(format!("invalid test username {name:?}: {e}")))?;
        let uuid = zwrs_core::offline_uuid(parsed.as_str());
        let mut map = zwrs_core::Json::object();
        map.insert("echo".into(), zwrs_core::Json::String(parsed.to_string()));
        map.insert("offline_uuid".into(), zwrs_core::Json::String(uuid.to_string()));
        map.insert(
            "bridge_version".into(),
            zwrs_core::Json::Number(f64::from(JNI_BRIDGE_VERSION)),
        );
        map.insert(
            "engine_version".into(),
            zwrs_core::Json::String(zwrs_engine::VERSION.to_string()),
        );
        Ok(zwrs_core::Json::Object(map).to_compact())
    })
}

/// `nativeStart(String directory, int port, boolean onlineMode) -> String`
///
/// Starts the engine in `directory`. `port == 0` asks the OS for a free port,
/// which is what the integration tests use. Returns JSON describing the bound
/// address and capability report.
///
/// # Safety
/// Called by the JVM with a valid environment and a `jstring` (may be null).
#[no_mangle]
pub unsafe extern "C" fn Java_io_zorowarrior_zwrs_NativeBridge_nativeStart(
    env: *mut c_void,
    _class: *mut c_void,
    directory: sys::JString,
    port: i32,
    online_mode: u8,
) -> sys::JString {
    let dir = read_jstring(env, directory);
    bridge_call(env, || {
        let dir = dir
            .ok_or_else(|| BridgeError::Runtime("server directory must not be null".into()))?;
        if port < 0 || port > 65_535 {
            return Err(BridgeError::Runtime(format!("port out of range: {port}")));
        }
        let mut slot = engine_slot()
            .lock()
            .map_err(|_| BridgeError::IllegalState("engine state is poisoned".into()))?;
        if let Some(existing) = slot.as_ref() {
            return Err(BridgeError::IllegalState(format!(
                "the engine is already running in {}; stop it before starting another server",
                existing.directory
            )));
        }

        let root = std::path::PathBuf::from(&dir);
        std::fs::create_dir_all(root.join("config"))
            .map_err(|e| BridgeError::Runtime(format!("could not create {dir}/config: {e}")))?;
        let config_dir = root.join("config");
        let mut config = Config::load(&config_dir)
            .map_err(|e| BridgeError::Runtime(e.to_string()))?;
        // The launcher owns the process, so its explicit arguments win.
        config.root_dir = root.clone();
        config.data_dir = root.join("data");
        config.plugin_dir = root.join("plugins");
        config.log_dir = root.join("logs");
        config.config_dir = config_dir;
        config.port = u16::try_from(port).map_err(|_| BridgeError::Runtime("invalid port".into()))?;
        config.online_mode = online_mode != 0;
        config
            .validate()
            .map_err(|e| BridgeError::Runtime(e.to_string()))?;

        zwrs_core::log::set_level(config.log_level);
        let level = Level::Info;
        zwrs_core::log::log(
            level,
            "zwrs::jni",
            &format!("starting engine for {dir} (port {port}, online-mode {})", config.online_mode),
        );

        let state = zwrs_engine::prepare(config)
            .map_err(|e| BridgeError::Runtime(format!("startup failed: {e}")))?;
        let handle = zwrs_engine::start(Arc::clone(&state))
            .map_err(|e| BridgeError::Runtime(e.to_string()))?;
        let address = handle.address().to_string();

        *slot = Some(RunningEngine {
            state: Arc::clone(&state),
            handle,
            directory: dir.clone(),
        });

        let mut map = zwrs_core::Json::object();
        map.insert("address".into(), zwrs_core::Json::String(address));
        map.insert("port".into(), zwrs_core::Json::Number(f64::from(state.config.port)));
        map.insert(
            "online_mode".into(),
            zwrs_core::Json::Bool(state.config.online_mode),
        );
        map.insert(
            "plugin_dir".into(),
            zwrs_core::Json::String(state.config.plugin_dir.display().to_string()),
        );
        map.insert(
            "data_dir".into(),
            zwrs_core::Json::String(state.config.data_dir.display().to_string()),
        );
        let caps = zwrs_engine::plugins::capabilities(
            state.config.java_plugins_enabled,
            state.config.wasm_plugins_enabled,
        );
        let mut capabilities = zwrs_core::Json::object();
        for (key, value) in caps {
            capabilities.insert(key.to_string(), zwrs_core::Json::String(value));
        }
        map.insert("capabilities".into(), zwrs_core::Json::Object(capabilities));
        Ok(zwrs_core::Json::Object(map).to_compact())
    })
}

/// `nativeCommand(String line) -> String` (JSON `{lines, keep_running, recognised}`)
///
/// Runs a console command against the live engine.
///
/// # Safety
/// Called by the JVM with a valid environment.
#[no_mangle]
pub unsafe extern "C" fn Java_io_zorowarrior_zwrs_NativeBridge_nativeCommand(
    env: *mut c_void,
    _class: *mut c_void,
    line: sys::JString,
) -> sys::JString {
    let line = read_jstring(env, line);
    bridge_call(env, || {
        let line = line.unwrap_or_default();
        let slot = engine_slot()
            .lock()
            .map_err(|_| BridgeError::IllegalState("engine state is poisoned".into()))?;
        let Some(engine) = slot.as_ref() else {
            return Err(BridgeError::IllegalState(
                "the engine is not running; call nativeStart first".into(),
            ));
        };
        let outcome = zwrs_engine::admin::execute(&engine.state, &line);
        let mut map = zwrs_core::Json::object();
        map.insert(
            "lines".into(),
            zwrs_core::Json::Array(
                outcome
                    .lines
                    .iter()
                    .map(|l| zwrs_core::Json::String(l.clone()))
                    .collect(),
            ),
        );
        map.insert("keep_running".into(), zwrs_core::Json::Bool(outcome.keep_running));
        map.insert("recognised".into(), zwrs_core::Json::Bool(outcome.recognised));
        Ok(zwrs_core::Json::Object(map).to_compact())
    })
}

/// `nativeState() -> String` (JSON snapshot for the Java side)
///
/// # Safety
/// Called by the JVM with a valid environment.
#[no_mangle]
pub unsafe extern "C" fn Java_io_zorowarrior_zwrs_NativeBridge_nativeState(
    env: *mut c_void,
    _class: *mut c_void,
) -> sys::JString {
    bridge_call(env, || {
        let slot = engine_slot()
            .lock()
            .map_err(|_| BridgeError::IllegalState("engine state is poisoned".into()))?;
        let mut map = zwrs_core::Json::object();
        match slot.as_ref() {
            None => {
                map.insert("running".into(), zwrs_core::Json::Bool(false));
            }
            Some(engine) => {
                use std::sync::atomic::Ordering::Relaxed;
                map.insert("running".into(), zwrs_core::Json::Bool(engine.handle.is_running()));
                map.insert("plugin_host".into(), zwrs_core::Json::Bool(host::is_installed()));
                map.insert(
                    "address".into(),
                    zwrs_core::Json::String(engine.handle.address().to_string()),
                );
                map.insert(
                    "directory".into(),
                    zwrs_core::Json::String(engine.directory.clone()),
                );
                map.insert(
                    "online_players".into(),
                    zwrs_core::Json::Number(engine.state.online_count() as f64),
                );
                map.insert(
                    "uptime_seconds".into(),
                    zwrs_core::Json::Number(engine.state.uptime_seconds() as f64),
                );
                map.insert(
                    "connections_accepted".into(),
                    zwrs_core::Json::Number(
                        engine.state.metrics.connections_accepted.load(Relaxed) as f64,
                    ),
                );
                map.insert(
                    "status_pings".into(),
                    zwrs_core::Json::Number(engine.state.metrics.status_pings.load(Relaxed) as f64),
                );
                map.insert(
                    "logins_admitted".into(),
                    zwrs_core::Json::Number(engine.state.metrics.logins_admitted.load(Relaxed) as f64),
                );
                map.insert(
                    "logins_refused".into(),
                    zwrs_core::Json::Number(engine.state.metrics.logins_refused.load(Relaxed) as f64),
                );
            }
        }
        Ok(zwrs_core::Json::Object(map).to_compact())
    })
}

/// `nativeStop() -> String` (JSON `{stopped: true, online_players_closed: n}`)
///
/// Gracefully stops the engine: sessions are closed, playtime is credited and
/// player data is written before this returns.
///
/// # Safety
/// Called by the JVM with a valid environment.
#[no_mangle]
pub unsafe extern "C" fn Java_io_zorowarrior_zwrs_NativeBridge_nativeStop(
    env: *mut c_void,
    _class: *mut c_void,
) -> sys::JString {
    bridge_call(env, || {
        let mut slot = engine_slot()
            .lock()
            .map_err(|_| BridgeError::IllegalState("engine state is poisoned".into()))?;
        let mut map = zwrs_core::Json::object();
        match slot.take() {
            None => {
                map.insert("stopped".into(), zwrs_core::Json::Bool(false));
                map.insert(
                    "message".into(),
                    zwrs_core::Json::String("the engine was not running".to_string()),
                );
            }
            Some(mut engine) => {
                let online = engine.state.online_count();
                engine
                    .handle
                    .stop()
                    .map_err(|e| BridgeError::Runtime(format!("shutdown problem: {e}")))?;
                map.insert("stopped".into(), zwrs_core::Json::Bool(true));
                map.insert(
                    "online_players_closed".into(),
                    zwrs_core::Json::Number(online as f64),
                );
                map.insert(
                    "directory".into(),
                    zwrs_core::Json::String(engine.directory),
                );
            }
        }
        Ok(zwrs_core::Json::Object(map).to_compact())
    })
}

/// `nativeInstallHost(hook) -> String` (JSON `{installed: true}`)
///
/// Registers the Java plugin host object (`io.zorowarrior.zwrs.plugin.EngineHooks`)
/// as the receiver of player events and unknown console commands. Called by the
/// launcher once the plugin runtime is ready; before that every hook is a no-op.
///
/// Fails (throws) when the class is missing, which means the JAR and the native
/// library come from different builds.
///
/// # Safety
/// Called by the JVM with a valid environment and a live `hook` reference.
#[no_mangle]
pub unsafe extern "C" fn Java_io_zorowarrior_zwrs_NativeBridge_nativeInstallHost(
    env: *mut c_void,
    _class: *mut c_void,
    hook: *mut c_void,
) -> sys::JString {
    bridge_call(env, || {
        let mut slot = engine_slot()
            .lock()
            .map_err(|_| BridgeError::IllegalState("engine state is poisoned".into()))?;
        let Some(engine) = slot.as_ref() else {
            return Err(BridgeError::IllegalState(
                "the engine must be running before a plugin host is installed".into(),
            ));
        };
        host::install(env, hook).map_err(BridgeError::Runtime)?;
        zwrs_engine::server::set_listener(&engine.state, Some(Arc::new(host::JavaHost)));
        let mut map = zwrs_core::Json::object();
        map.insert("installed".into(), zwrs_core::Json::Bool(true));
        map.insert(
            "host".into(),
            zwrs_core::Json::String("io.zorowarrior.zwrs.plugin.EngineHooks".into()),
        );
        Ok(zwrs_core::Json::Object(map).to_compact())
    })
}

/// `nativeRemoveHost() -> String` (JSON `{installed: false}`)
///
/// Detaches the plugin host: events stop being delivered and unknown commands are
/// reported as unknown again. Safe to call when no host is installed.
///
/// # Safety
/// Called by the JVM with a valid environment.
#[no_mangle]
pub unsafe extern "C" fn Java_io_zorowarrior_zwrs_NativeBridge_nativeRemoveHost(
    env: *mut c_void,
    _class: *mut c_void,
) -> sys::JString {
    bridge_call(env, || {
        let slot = engine_slot()
            .lock()
            .map_err(|_| BridgeError::IllegalState("engine state is poisoned".into()))?;
        host::uninstall(env);
        if let Some(engine) = slot.as_ref() {
            zwrs_engine::server::set_listener(&engine.state, None);
        }
        let mut map = zwrs_core::Json::object();
        map.insert("installed".into(), zwrs_core::Json::Bool(false));
        Ok(zwrs_core::Json::Object(map).to_compact())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_version_is_positive() {
        let version = unsafe {
            Java_io_zorowarrior_zwrs_NativeBridge_nativeBridgeVersion(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(version, i32::try_from(JNI_BRIDGE_VERSION).unwrap());
    }

    #[test]
    fn calls_without_a_jvm_fail_closed() {
        // A null environment must not crash: bridge_call returns null.
        let result = unsafe {
            Java_io_zorowarrior_zwrs_NativeBridge_nativeBuildInfo(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert!(result.is_null());
    }

}
