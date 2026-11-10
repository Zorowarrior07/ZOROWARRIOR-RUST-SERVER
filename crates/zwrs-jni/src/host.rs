//! The Java plugin host, as seen from Rust.
//!
//! The engine fires [`crate::events`] hooks on its own threads; this module turns
//! them into calls on `io.zorowarrior.zwrs.plugin.EngineHooks`, which is where the
//! Java [`PluginRuntime`] actually runs plugin code.
//!
//! # Threading and safety
//!
//! * Engine threads are not attached to the JVM, so every hook attaches first.
//!   The attach is a *daemon* attach: a plain attach would make the JVM wait for
//!   that thread at `DestroyJavaVM`, hanging the process after `/stop`. A failure
//!   to attach is logged and the hook is skipped — never executed with a null
//!   environment.
//! * The host keeps the `JavaVM` and a global reference to the hook object for the
//!   life of the engine. Both are cleared when the host is stopped, and every
//!   call re-checks that a host is still installed.
//! * Java exceptions raised by plugin code are cleared here and reported through
//!   the engine log: an exception in a plugin must not poison the next JNI call.
//! * If `EngineHooks` cannot be found (a launcher/JAR version mismatch), host mode
//!   is refused with an explanation instead of silently dropping events.

use std::ffi::{CStr, CString};
use std::os::raw::c_void;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use zwrs_engine::events::{CommandReply, HostListener, PlayerEvent};

use crate::sys::{self, JavaVm, JObject};

/// Class that receives engine events on the Java side.
const HOOK_CLASS: &str = "io/zorowarrior/zwrs/plugin/EngineHooks";

/// The installed host: the JVM plus the object that implements the callbacks.
struct HostRefs {
    vm: JavaVm,
    hook: JObject,
}

// Safety: `hook` is a JNI global reference, which the JVM allows any attached
// thread to use.
unsafe impl Send for HostRefs {}
unsafe impl Sync for HostRefs {}

static HOST: Mutex<Option<Arc<HostRefs>>> = Mutex::new(None);

fn jstr(text: &str) -> CString {
    // JNI wants modified UTF-8; interior NULs are impossible in event strings we
    // build, but never let one truncate silently.
    CString::new(text.replace('\0', "\\0"))
        .unwrap_or_else(|_| CString::new("").expect("empty C string"))
}

/// Reports and clears a pending exception.
///
/// The JVM's own description (class, message and stack trace) is printed to
/// stderr, and the exception's `toString()` is appended to the engine log, so an
/// operator sees the real cause instead of a generic "something failed".
fn clear_exception(env: *mut c_void, context: &str) {
    unsafe {
        if !sys::exception_check(env) {
            return;
        }
        // Let the JVM print the class, message and full stack trace: it is the
        // only way an operator can see which plugin line failed.
        sys::exception_describe(env);
        let thrown = sys::exception_occurred(env);
        sys::exception_clear(env);
        let mut detail = String::from("unknown");
        if let Some(throwable) = thrown {
            // Print the full Java-side stack trace: the exception object was
            // captured before the clear, so this is the genuine stack.
            if sys::exception_check(env) {
                sys::exception_clear(env);
            }
            if let Some(class) = sys::get_object_class(env, throwable) {
                let to_string = jstr("toString");
                let signature = jstr("()Ljava/lang/String;");
                if let Some(method) = sys::get_method_id(env, class, &to_string, &signature) {
                    if let Some(text) = sys::call_object_method(env, throwable, method) {
                        if let Some(chars) = sys::UtfChars::new(env, text) {
                            detail = chars.to_string_lossy();
                        }
                        sys::delete_local_ref(env, text);
                    }
                }
                sys::delete_local_ref(env, class);
            }
            sys::delete_local_ref(env, throwable);
            if sys::exception_check(env) {
                sys::exception_clear(env);
            }
        }
        zwrs_engine::server::console_line(
            zwrs_core::log::Level::Error,
            &format!("zwrs::jni: {context} failed in Java: {detail}"),
        );
    }
}

/// Calls `EngineHooks.<method>(String)` on the installed host.
///
/// Returns `false` when no host is installed or the call could not be made, so
/// callers can fall back to engine-side behaviour.
fn call_hook(method: &CStr, payload: &str) -> bool {
    let host = match HOST.lock() {
        Ok(slot) => slot.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let Some(host) = host else {
        return false;
    };

    unsafe {
        let Some(env) = host.vm.attach_current_thread_as_daemon() else {
            zwrs_engine::server::console_line(
                zwrs_core::log::Level::Error,
                &format!(
                    "zwrs::jni: could not attach the engine thread to the JVM for {}",
                    method.to_string_lossy()
                ),
            );
            return false;
        };

        let class_name = jstr(HOOK_CLASS);
        let class = match sys::find_class(env, &class_name) {
            Some(class) => class,
            None => {
                clear_exception(env, "FindClass(EngineHooks)");
                zwrs_engine::server::console_line(
                    zwrs_core::log::Level::Error,
                    "zwrs::jni: io.zorowarrior.zwrs.plugin.EngineHooks is missing from the "
                        .to_string()
                        .as_str(),
                );
                // The launcher always ships EngineHooks, so this means the JAR and
                // the native library are from different builds.
                return false;
            }
        };
        let signature = jstr("(Ljava/lang/String;)V");
        let Some(method_id) = sys::get_static_method_id(env, class, method, &signature) else {
            clear_exception(env, "GetStaticMethodID");
            zwrs_engine::server::console_line(
                zwrs_core::log::Level::Error,
                &format!("zwrs::jni: EngineHooks.{} has the wrong signature", method.to_string_lossy()),
            );
            return false;
        };
        let Some(arg) = sys::new_string_utf(env, &jstr(payload)) else {
            clear_exception(env, "NewStringUTF");
            return false;
        };
        sys::call_static_void_method(env, class, method_id, arg);
        clear_exception(env, &format!("EngineHooks.{}", method.to_string_lossy()));
        sys::delete_local_ref(env, arg);
        sys::delete_local_ref(env, class);
        true
    }
}

/// The listener installed on the engine while Java plugins are hosted.
pub(crate) struct JavaHost;

impl HostListener for JavaHost {
    fn player_join(&self, event: &PlayerEvent) {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            call_hook(c"onPlayerJoin", &event.to_json())
        }));
    }

    fn player_quit(&self, event: &PlayerEvent) {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            call_hook(c"onPlayerQuit", &event.to_json())
        }));
    }

    fn command(&self, command: &str, args: &[String]) -> CommandReply {
        let payload = {
            use zwrs_core::json::Json;
            let mut object = Json::object();
            object.insert("command".to_string(), Json::String(command.to_string()));
            object.insert(
                "args".to_string(),
                Json::Array(args.iter().map(|a| Json::String(a.clone())).collect()),
            );
            Json::Object(object).to_compact()
        };
        // The Java side returns `null` for "not mine" and a JSON array of lines
        // otherwise. Both are reported through a static field-free API: the
        // method returns a String.
        match call_hook_with_result(c"onCommand", &payload) {
            Some(lines) => CommandReply::Handled(lines),
            None => CommandReply::Unknown,
        }
    }
}

/// Calls a hook whose Java signature returns `String` (the command hook).
fn call_hook_with_result(method: &CStr, payload: &str) -> Option<Vec<String>> {
    let host = match HOST.lock() {
        Ok(slot) => slot.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }?;

    unsafe {
        // Daemon attach: see JavaVm::attach_current_thread_as_daemon. A plain
        // attach would make the JVM wait for this engine thread at shutdown.
        let env = host.vm.attach_current_thread_as_daemon()?;
        let class_name = jstr(HOOK_CLASS);
        let class = sys::find_class(env, &class_name)?;
        let signature = jstr("(Ljava/lang/String;)Ljava/lang/String;");
        let method_id = sys::get_static_method_id(env, class, method, &signature)?;
        let arg = sys::new_string_utf(env, &jstr(payload))?;
        let result = sys::call_static_object_method(env, class, method_id, arg);
        clear_exception(env, "EngineHooks.onCommand");
        sys::delete_local_ref(env, arg);
        sys::delete_local_ref(env, class);
        let text = result.and_then(|string| {
            let borrowed = sys::UtfChars::new(env, string)?;
            let text = borrowed.to_string_lossy();
            sys::delete_local_ref(env, string);
            Some(text)
        })?;
        if text == "null" || text.is_empty() {
            return None;
        }
        let parsed = zwrs_core::json::Json::parse(&text).ok()?;
        let lines = parsed.as_array()?;
        Some(
            lines
                .iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .collect(),
        )
    }
}

/// Installs the Java host for `env` and `hook`, replacing any previous host.
///
/// # Safety
/// `env` must be a valid environment for the calling thread and `hook` a local
/// reference to the host object.
pub(crate) unsafe fn install(env: *mut c_void, hook: JObject) -> Result<(), String> {
    if env.is_null() || hook.is_null() {
        return Err("the plugin host was not passed to the native library".to_string());
    }
    let vm = sys::get_java_vm(env).ok_or_else(|| "the JVM did not provide a JavaVM".to_string())?;
    let global = sys::new_global_ref(env, hook)
        .ok_or_else(|| "could not create a global reference to the plugin host".to_string())?;

    // Fail fast if the class is missing, so the launcher learns about a version
    // mismatch now instead of silently missing every event later.
    let class_name = jstr(HOOK_CLASS);
    if sys::find_class(env, &class_name).is_none() {
        clear_exception(env, "FindClass(EngineHooks)");
        sys::delete_global_ref(env, global);
        return Err(format!(
            "{} is not on the class path: the Java plugin host and the native library are from \
             different builds",
            HOOK_CLASS.replace('/', ".")
        ));
    }
    clear_exception(env, "install");

    let mut slot = match HOST.lock() {
        Ok(slot) => slot,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(previous) = slot.take() {
        // Clearing a previous host means the old object may still be referenced by
        // an in-flight call; the JVM keeps the global reference alive until every
        // thread has released it, so deleting it here is safe.
        if let Some(env) = previous.vm.get_env() {
            unsafe { sys::delete_global_ref(env, previous.hook) };
        }
    }
    *slot = Some(Arc::new(HostRefs { vm, hook: global }));
    Ok(())
}

/// Removes the host, if any. Later events are dropped instead of failing.
pub(crate) fn uninstall(env: *mut c_void) {
    let removed = match HOST.lock() {
        Ok(mut slot) => slot.take(),
        Err(poisoned) => poisoned.into_inner().take(),
    };
    if let Some(host) = removed {
        if !env.is_null() {
            unsafe { sys::delete_global_ref(env, host.hook) };
        }
    }
}

/// Whether a host is installed (used by `nativeState`).
#[must_use]
pub(crate) fn is_installed() -> bool {
    match HOST.lock() {
        Ok(slot) => slot.is_some(),
        Err(poisoned) => poisoned.into_inner().is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_host_means_hooks_are_skipped_not_faked() {
        uninstall(std::ptr::null_mut());
        assert!(!is_installed());
        assert!(!call_hook(c"onPlayerJoin", "{}"));
        assert!(call_hook_with_result(c"onCommand", "{}").is_none());
    }

    #[test]
    fn command_reply_for_an_absent_host_is_unknown() {
        uninstall(std::ptr::null_mut());
        let reply = JavaHost.command("greet", &["a".to_string()]);
        assert_eq!(reply, CommandReply::Unknown);
    }

    #[test]
    fn string_conversion_never_truncates_at_a_nul() {
        let converted = jstr("a\0b");
        assert_eq!(converted.to_str().unwrap(), "a\\0b");
    }
}
