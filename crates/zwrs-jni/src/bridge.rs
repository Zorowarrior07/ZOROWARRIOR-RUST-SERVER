//! Safe wrapper over the raw JNI calls.
//!
//! Every exported function goes through [`bridge_call`], which:
//!
//! * catches Rust panics so a bug cannot take down the JVM,
//! * turns errors into `RuntimeException`/`IllegalStateException` on the Java
//!   side with a stable message,
//! * never leaves a pending Java exception behind when returning a value.
//!
//! String results are returned as JSON so the Java side has one parser and one
//! contract; no Java objects are constructed inside the native library.

use std::ffi::CString;
use std::os::raw::c_void;

use zwrs_engine::BuildInfo;

use crate::sys;

/// Class and method names, mirrored by `io.zorowarrior.zwrs.NativeBridge`.
const RUNTIME_EXCEPTION: &str = "java/lang/RuntimeException";
const ILLEGAL_STATE: &str = "java/lang/IllegalStateException";
const SUPPORTED_EXCEPTION: &str = "java/lang/UnsupportedOperationException";

fn throw(env: *mut c_void, class_name: &str, message: &str) {
    // `CString::new` fails on interior NULs; strip them rather than panic.
    let sanitised = message.replace('\0', " ");
    let Ok(class) = CString::new(class_name) else {
        return;
    };
    let Ok(msg) = CString::new(sanitised) else {
        return;
    };
    // SAFETY: `env` is provided by the JVM for this call; the strings are valid
    // NUL-terminated C strings for the duration of the call.
    unsafe {
        match sys::find_class(env, &class) {
            Some(clazz) => {
                sys::throw_new(env, clazz, &msg);
            }
            None => {
                // Could not even find the exception class: clear whatever
                // FindClass raised so we do not mask the original problem.
                if sys::exception_check(env) {
                    sys::exception_clear(env);
                }
            }
        }
    }
}

fn new_java_string(env: *mut c_void, value: &str) -> sys::JString {
    let sanitised = value.replace('\0', " ");
    match CString::new(sanitised) {
        Ok(c) => unsafe { sys::new_string_utf(env, &c) }.unwrap_or(std::ptr::null_mut()),
        Err(_) => std::ptr::null_mut(),
    }
}

/// A bridge call result: a JSON string or an error class + message.
pub(crate) enum BridgeError {
    Runtime(String),
    IllegalState(String),
    /// Reserved for capabilities the engine deliberately refuses (as opposed to
    /// failing). No bridge method returns it yet; the class mapping is covered by
    /// a unit test so it cannot rot before the first real use.
    #[cfg_attr(not(test), allow(dead_code))]
    Unsupported(String),
}

impl BridgeError {
    fn parts(&self) -> (&'static str, &str) {
        match self {
            Self::Runtime(m) => (RUNTIME_EXCEPTION, m),
            Self::IllegalState(m) => (ILLEGAL_STATE, m),
            Self::Unsupported(m) => (SUPPORTED_EXCEPTION, m),
        }
    }
}

/// Runs `body`, converting its result into a Java string.
pub(crate) fn bridge_call(
    env: *mut c_void,
    body: impl FnOnce() -> Result<String, BridgeError>,
) -> sys::JString {
    if env.is_null() {
        return std::ptr::null_mut();
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(Ok(json)) => new_java_string(env, &json),
        Ok(Err(error)) => {
            let (class, message) = error.parts();
            throw(env, class, message);
            std::ptr::null_mut()
        }
        Err(panic) => {
            let detail = panic_message(&panic);
            throw(
                env,
                RUNTIME_EXCEPTION,
                &format!("internal error in the ZWRS native engine: {detail}"),
            );
            std::ptr::null_mut()
        }
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Reads a Java `String` argument as Rust text.
///
/// # Safety
/// `env` must be valid and `value` a `jstring` or null.
pub(crate) unsafe fn read_jstring(env: *mut c_void, value: sys::JString) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let chars = sys::UtfChars::new(env, value)?;
    Some(chars.to_string_lossy())
}

/// Build info JSON for `nativeBuildInfo`.
pub(crate) fn build_info_json() -> String {
    let info = BuildInfo::current();
    let mut map = zwrs_core::Json::parse(&info.to_json()).unwrap_or(zwrs_core::Json::Null);
    if let zwrs_core::Json::Object(ref mut object) = map {
        object.insert(
            "jni_version".into(),
            zwrs_core::Json::Number(f64::from(crate::JNI_BRIDGE_VERSION)),
        );
        object.insert(
            "native_crate_version".into(),
            zwrs_core::Json::String(env!("CARGO_PKG_VERSION").to_string()),
        );
        // Facts the launcher needs to build the plugin-facing Server object and
        // the status ping; read from the engine rather than duplicated in Java.
        object.insert(
            "max_players".into(),
            zwrs_core::Json::Number(f64::from(zwrs_core::config::Config::default().max_players)),
        );
        object.insert(
            "minecraft_version".into(),
            zwrs_core::Json::String(zwrs_engine::VERSION_NAME.to_string()),
        );
        object.insert(
            "motd".into(),
            zwrs_core::Json::String(zwrs_core::config::Config::default().motd.clone()),
        );
    }
    map.to_compact()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_info_is_json_with_bridge_version() {
        let text = build_info_json();
        let doc = zwrs_core::Json::parse(&text).expect("valid json");
        assert_eq!(
            doc.get("jni_version").and_then(zwrs_core::Json::as_u64),
            Some(u64::from(crate::JNI_BRIDGE_VERSION))
        );
        assert!(doc.get("platform").and_then(zwrs_core::Json::as_str).is_some());
    }

    #[test]
    fn error_classes_are_distinct() {
        let (a, _) = BridgeError::Runtime(String::new()).parts();
        let (b, _) = BridgeError::IllegalState(String::new()).parts();
        let (c, _) = BridgeError::Unsupported(String::new()).parts();
        assert_ne!(a, b);
        assert_ne!(b, c);
    }

    #[test]
    fn panic_payloads_are_rendered() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("boom");
        assert_eq!(panic_message(&payload), "boom");
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("bang"));
        assert_eq!(panic_message(&payload), "bang");
    }
}
