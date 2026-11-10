//! Minimal, self-contained JNI bindings.
//!
//! The project builds without third-party crates (no `jni` crate), so the few
//! JNI entry points ZWRS needs are declared here. The layout is taken from the
//! JDK's `jni.h`:
//!
//! ```c
//! typedef const struct JNINativeInterface_ *JNIEnv;
//! ```
//!
//! `JNIEnv` is therefore a pointer to a pointer to a table of function
//! pointers. The indices below are the slot numbers inside
//! `struct JNINativeInterface_`; they were verified against OpenJDK 17's
//! `jni.h` with `offsetof` (see `docs/ARCHITECTURE.md`, "JNI ABI").
//!
//! Everything here is `unsafe` because it dereferences JVM-provided pointers.
//! The safety requirements are documented per function and the wrapper in
//! [`crate::bridge`] is the only code allowed to call them.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

/// Opaque JVM object reference.
pub type JObject = *mut c_void;
/// Opaque class reference.
pub type JClass = *mut c_void;
/// UTF-16 string reference.
pub type JString = *mut c_void;
/// Status code returned by JNI functions.
pub type JInt = c_int;
/// Boolean status code.
pub type JBoolean = u8;

/// Slot numbers in `struct JNINativeInterface_`.
mod slot {
    /// `GetVersion`
    pub const GET_VERSION: usize = 4;
    /// `FindClass`
    pub const FIND_CLASS: usize = 6;
    /// `ThrowNew`
    pub const THROW_NEW: usize = 14;
    /// `ExceptionOccurred`
    pub const EXCEPTION_OCCURRED: usize = 15;
    /// `ExceptionClear`
    pub const EXCEPTION_CLEAR: usize = 17;
    /// `DeleteLocalRef`
    pub const DELETE_LOCAL_REF: usize = 23;
    /// `NewStringUTF`
    pub const NEW_STRING_UTF: usize = 167;
    /// `GetStringUTFLength`
    pub const GET_STRING_UTF_LENGTH: usize = 168;
    /// `GetStringUTFChars`
    pub const GET_STRING_UTF_CHARS: usize = 169;
    /// `ReleaseStringUTFChars`
    pub const RELEASE_STRING_UTF_CHARS: usize = 170;
    /// `ExceptionCheck`
    pub const EXCEPTION_CHECK: usize = 228;
}

/// Type-erased function pointer slot.
type Slot = *const c_void;

#[inline]
unsafe fn table(env: *mut c_void) -> *const Slot {
    // env -> *JNINativeInterface_
    #[allow(clippy::cast_ptr_alignment)]
    let table_ptr = env.cast::<*const Slot>();
    *table_ptr
}

#[inline]
unsafe fn function(env: *mut c_void, index: usize) -> Slot {
    *table(env).add(index)
}

/// `env->GetVersion()`
///
/// # Safety
/// `env` must be a valid `JNIEnv` for the calling thread.
pub unsafe fn get_version(env: *mut c_void) -> JInt {
    let f: unsafe extern "C" fn(*mut c_void) -> JInt = std::mem::transmute(function(env, slot::GET_VERSION));
    f(env)
}

/// `env->FindClass(name)`
///
/// # Safety
/// `env` must be valid; `name` must be a valid NUL-terminated string.
pub unsafe fn find_class(env: *mut c_void, name: &CStr) -> Option<JClass> {
    let f: unsafe extern "C" fn(*mut c_void, *const c_char) -> JClass =
        std::mem::transmute(function(env, slot::FIND_CLASS));
    let class = f(env, name.as_ptr());
    if class.is_null() {
        None
    } else {
        Some(class)
    }
}

/// `env->ThrowNew(class, msg)`
///
/// # Safety
/// `env` and `class` must be valid; `msg` must be NUL-terminated.
pub unsafe fn throw_new(env: *mut c_void, class: JClass, msg: &CStr) -> JInt {
    let f: unsafe extern "C" fn(*mut c_void, JClass, *const c_char) -> JInt =
        std::mem::transmute(function(env, slot::THROW_NEW));
    f(env, class, msg.as_ptr())
}

/// `env->ExceptionOccurred()`
///
/// # Safety
/// `env` must be valid.
pub unsafe fn exception_occurred(env: *mut c_void) -> Option<JObject> {
    let f: unsafe extern "C" fn(*mut c_void) -> JObject =
        std::mem::transmute(function(env, slot::EXCEPTION_OCCURRED));
    let thrown = f(env);
    if thrown.is_null() {
        None
    } else {
        Some(thrown)
    }
}

/// `env->ExceptionCheck()`
///
/// # Safety
/// `env` must be valid.
pub unsafe fn exception_check(env: *mut c_void) -> bool {
    let f: unsafe extern "C" fn(*mut c_void) -> JBoolean =
        std::mem::transmute(function(env, slot::EXCEPTION_CHECK));
    f(env) != 0
}

/// `env->ExceptionClear()`
///
/// # Safety
/// `env` must be valid.
pub unsafe fn exception_clear(env: *mut c_void) {
    let f: unsafe extern "C" fn(*mut c_void) =
        std::mem::transmute(function(env, slot::EXCEPTION_CLEAR));
    f(env);
}

/// `env->DeleteLocalRef(obj)`
///
/// # Safety
/// `env` must be valid and `obj` a local reference owned by this thread.
pub unsafe fn delete_local_ref(env: *mut c_void, obj: JObject) {
    let f: unsafe extern "C" fn(*mut c_void, JObject) =
        std::mem::transmute(function(env, slot::DELETE_LOCAL_REF));
    f(env, obj);
}

/// `env->NewStringUTF(utf)`
///
/// # Safety
/// `env` must be valid; `utf` must be NUL-terminated and must not contain
/// interior NUL bytes (use modified-UTF-8 or chunk the string otherwise).
pub unsafe fn new_string_utf(env: *mut c_void, utf: &CStr) -> Option<JString> {
    let f: unsafe extern "C" fn(*mut c_void, *const c_char) -> JString =
        std::mem::transmute(function(env, slot::NEW_STRING_UTF));
    let string = f(env, utf.as_ptr());
    if string.is_null() {
        None
    } else {
        Some(string)
    }
}

/// RAII guard around `GetStringUTFChars` / `ReleaseStringUTFChars`.
pub struct UtfChars<'a> {
    env: *mut c_void,
    string: JString,
    ptr: *const c_char,
    marker: std::marker::PhantomData<&'a ()>,
}

impl UtfChars<'_> {
    /// Borrows the modified-UTF-8 bytes of `string`.
    ///
    /// # Safety
    /// `env` must be valid and `string` a valid `jstring` for this thread.
    pub unsafe fn new(env: *mut c_void, string: JString) -> Option<Self> {
        let f: unsafe extern "C" fn(*mut c_void, JString, *mut JBoolean) -> *const c_char =
            std::mem::transmute(function(env, slot::GET_STRING_UTF_CHARS));
        let ptr = f(env, string, std::ptr::null_mut());
        if ptr.is_null() {
            return None;
        }
        Some(Self {
            env,
            string,
            ptr,
            marker: std::marker::PhantomData,
        })
    }

    /// The borrowed bytes, which may contain NUL-free modified UTF-8.
    ///
    /// # Safety
    /// The JVM guarantees the pointer is valid until this guard is dropped.
    pub unsafe fn to_bytes(&self) -> Vec<u8> {
        let len_fn: unsafe extern "C" fn(*mut c_void, JString) -> JInt =
            std::mem::transmute(function(self.env, slot::GET_STRING_UTF_LENGTH));
        let len = us_len(len_fn(self.env, self.string));
        debug_assert!(len < 16 * 1024 * 1024, "implausible string length");
        let slice = std::slice::from_raw_parts(self.ptr.cast::<u8>(), len);
        slice.to_vec()
    }

    /// The borrowed string as UTF-8, replacing invalid sequences.
    ///
    /// # Safety
    /// Same as [`Self::to_bytes`].
    pub unsafe fn to_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.to_bytes()).into_owned()
    }
}

impl Drop for UtfChars<'_> {
    fn drop(&mut self) {
        unsafe {
            let f: unsafe extern "C" fn(*mut c_void, JString, *const c_char) =
                std::mem::transmute(function(self.env, slot::RELEASE_STRING_UTF_CHARS));
            f(self.env, self.string, self.ptr);
        }
    }
}

#[allow(clippy::cast_sign_loss)]
fn us_len(raw: JInt) -> usize {
    if raw <= 0 {
        0
    } else {
        raw as usize
    }
}

/// Returns whether the JVM defined by `env` runs a supported version (8+).
///
/// # Safety
/// `env` must be valid.
pub unsafe fn is_supported_jni_version(env: *mut c_void) -> bool {
    // 0x0001_0006 = JNI_VERSION_1_6, 0x0001_0008 = JNI_VERSION_1_8, etc.
    let version = get_version(env);
    version >= 0x0001_0006
}
