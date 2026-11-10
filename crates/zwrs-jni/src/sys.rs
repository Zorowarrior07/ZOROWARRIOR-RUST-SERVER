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
/// Opaque method-id reference.
pub type JMethodId = *mut c_void;

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
    /// `GetObjectClass`
    pub const GET_OBJECT_CLASS: usize = 31;
    /// `GetMethodID`
    pub const GET_METHOD_ID: usize = 33;
    /// `CallObjectMethod` (varargs ABI)
    pub const CALL_OBJECT_METHOD: usize = 34;
    /// `ExceptionDescribe`
    pub const EXCEPTION_DESCRIBE: usize = 16;
    /// `NewGlobalRef`
    pub const NEW_GLOBAL_REF: usize = 21;
    /// `NewLocalRef`
    pub const NEW_LOCAL_REF: usize = 25;
    /// `DeleteGlobalRef`
    pub const DELETE_GLOBAL_REF: usize = 22;
    /// `CallVoidMethod` (varargs ABI)
    pub const CALL_VOID_METHOD: usize = 61;
    /// `CallStaticVoidMethod` (varargs ABI). Verified with `offsetof`: the
    /// `...V` and `...A` variants are separate slots (141 / 142 / 143), so a
    /// wrong index here silently calls the array-taking function with a vararg
    /// pointer and crashes the JVM.
    pub const CALL_STATIC_VOID_METHOD: usize = 141;
    /// `CallStaticObjectMethod` (varargs ABI). Its `...V`/`...A` variants are
    /// 115 and 116.
    pub const CALL_STATIC_OBJECT_METHOD: usize = 114;
    /// `GetStaticMethodID`
    pub const GET_STATIC_METHOD_ID: usize = 113;
    /// `GetJavaVM`
    pub const GET_JAVA_VM: usize = 219;
}

/// Slot numbers in `struct JNIInvokeInterface_` (`JavaVM`).
mod vm_slot {
    /// `AttachCurrentThread`
    pub const ATTACH_CURRENT_THREAD: usize = 4;
    /// `DetachCurrentThread`
    pub const DETACH_CURRENT_THREAD: usize = 5;
    /// `GetEnv`
    pub const GET_ENV: usize = 6;
    /// `AttachCurrentThreadAsDaemon`
    pub const ATTACH_AS_DAEMON: usize = 7;
}

/// `JNI_VERSION_1_8`, the version requested when attaching a thread.
pub const JNI_VERSION_1_8: JInt = 0x0001_0008;

/// The `JavaVM` pointer handed to a native method (`env->GetJavaVM`).
///
/// A `JavaVM` is valid for the lifetime of the process once obtained, which is
/// what makes it safe to keep in the embedded plugin host: the engine's own
/// threads use it to attach before calling back into Java.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JavaVm(*mut c_void);

// Safety: the JVM guarantees a `JavaVM*` may be used from any thread. Every
// operation still goes through AttachCurrentThread/DetachCurrentThread.
unsafe impl Send for JavaVm {}
unsafe impl Sync for JavaVm {}

impl JavaVm {
    /// Wraps a raw `JavaVM*` obtained from `env->GetJavaVM`.
    ///
    /// # Safety
    /// `ptr` must have come from `GetJavaVM` and must not be null.
    #[must_use]
    pub const unsafe fn from_raw(ptr: *mut c_void) -> Self {
        Self(ptr)
    }

    /// # Safety
    /// The pointer must be valid.
    #[must_use]
    pub const unsafe fn as_raw(&self) -> *mut c_void {
        self.0
    }

    /// `vm->AttachCurrentThread(...)`, returning the thread's `JNIEnv`.
    ///
    /// Returns `None` when the JVM refuses the attach; the caller must then skip
    /// the Java call instead of using a null `env`.
    ///
    /// # Safety
    /// Must be called on a thread that is not already attached, or harmlessly
    /// twice (the JVM returns the existing environment).
    pub unsafe fn attach_current_thread(&self) -> Option<*mut c_void> {
        if self.0.is_null() {
            return None;
        }
        let table = *self.0.cast::<*const Slot>();
        let f: unsafe extern "C" fn(*mut c_void, *mut *mut c_void, *mut c_void) -> JInt =
            std::mem::transmute(*table.add(vm_slot::ATTACH_CURRENT_THREAD));
        let mut env: *mut c_void = std::ptr::null_mut();
        let code = f(self.0, std::ptr::addr_of_mut!(env), std::ptr::null_mut());
        if code != 0 || env.is_null() {
            None
        } else {
            Some(env)
        }
    }

    /// Attaches the calling thread as a **daemon**.
    ///
    /// This matters for shutdown: a thread attached with plain
    /// `AttachCurrentThread` counts as a non-daemon thread, so the JVM waits for
    /// it at `DestroyJavaVM` — the process then hangs after a clean stop even
    /// though the engine has exited. Daemon attachment avoids that, which is why
    /// every engine-thread callback uses this entry point.
    ///
    /// # Safety
    /// Must be called on a thread that is not already attached, or harmlessly
    /// twice (the JVM returns the existing environment).
    pub unsafe fn attach_current_thread_as_daemon(&self) -> Option<*mut c_void> {
        if self.0.is_null() {
            return None;
        }
        let table = *self.0.cast::<*const Slot>();
        let f: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> JInt =
            std::mem::transmute(*table.add(vm_slot::ATTACH_AS_DAEMON));
        let mut env: *mut c_void = std::ptr::null_mut();
        let code = f(self.0, std::ptr::addr_of_mut!(env));
        if code != 0 || env.is_null() {
            None
        } else {
            Some(env)
        }
    }

    /// `vm->GetEnv(...)`: the already-attached environment for this thread.
    ///
    /// # Safety
    /// Same contract as [`Self::attach_current_thread`].
    pub unsafe fn get_env(&self) -> Option<*mut c_void> {
        if self.0.is_null() {
            return None;
        }
        let table = *self.0.cast::<*const Slot>();
        let f: unsafe extern "C" fn(*mut c_void, *mut *mut c_void, JInt) -> JInt =
            std::mem::transmute(*table.add(vm_slot::GET_ENV));
        let mut env: *mut c_void = std::ptr::null_mut();
        let code = f(self.0, std::ptr::addr_of_mut!(env), JNI_VERSION_1_8);
        if code != 0 || env.is_null() {
            None
        } else {
            Some(env)
        }
    }

    /// `vm->DetachCurrentThread()`.
    ///
    /// # Safety
    /// Only call this from a thread this code attached, and never from the JVM's
    /// own main thread.
    pub unsafe fn detach_current_thread(&self) -> bool {
        if self.0.is_null() {
            return false;
        }
        let table = *self.0.cast::<*const Slot>();
        let f: unsafe extern "C" fn(*mut c_void) -> JInt =
            std::mem::transmute(*table.add(vm_slot::DETACH_CURRENT_THREAD));
        f(self.0) == 0
    }
}

/// `env->GetJavaVM()`.
///
/// # Safety
/// `env` must be valid for the calling thread.
pub unsafe fn get_java_vm(env: *mut c_void) -> Option<JavaVm> {
    let f: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> JInt =
        std::mem::transmute(function(env, slot::GET_JAVA_VM));
    let mut vm: *mut c_void = std::ptr::null_mut();
    let code = f(env, std::ptr::addr_of_mut!(vm));
    if code != 0 || vm.is_null() {
        None
    } else {
        Some(JavaVm(vm))
    }
}

/// `env->GetObjectClass(obj)`.
///
/// # Safety
/// `env` must be valid; `obj` must be a local or global reference.
pub unsafe fn get_object_class(env: *mut c_void, obj: JObject) -> Option<JClass> {
    let f: unsafe extern "C" fn(*mut c_void, JObject) -> JClass =
        std::mem::transmute(function(env, slot::GET_OBJECT_CLASS));
    let class = f(env, obj);
    if class.is_null() {
        None
    } else {
        Some(class)
    }
}

/// `env->GetStaticMethodID(class, name, signature)`.
///
/// # Safety
/// `env` and `class` must be valid; `name`/`signature` must be NUL-terminated.
pub unsafe fn get_static_method_id(
    env: *mut c_void,
    class: JClass,
    name: &CStr,
    signature: &CStr,
) -> Option<JMethodId> {
    let f: unsafe extern "C" fn(*mut c_void, JClass, *const c_char, *const c_char) -> JMethodId =
        std::mem::transmute(function(env, slot::GET_STATIC_METHOD_ID));
    let id = f(env, class, name.as_ptr(), signature.as_ptr());
    if id.is_null() {
        None
    } else {
        Some(id)
    }
}

/// `env->CallStaticVoidMethod(class, method, arg)`.
///
/// The variadic JNI entry point receives the trailing argument exactly as the C
/// caller would pass it: a `jobject`-sized value on the stack. This wrapper is
/// therefore only used with a single reference argument.
///
/// # Safety
/// `env`/`class`/`method` must be valid and the method must take one object
/// argument. Any exception left pending belongs to the caller.
pub unsafe fn call_static_void_method(
    env: *mut c_void,
    class: JClass,
    method: JMethodId,
    arg: JObject,
) {
    let f: unsafe extern "C" fn(*mut c_void, JClass, JMethodId, JObject) =
        std::mem::transmute(function(env, slot::CALL_STATIC_VOID_METHOD));
    f(env, class, method, arg);
}

/// `env->CallObjectMethod(obj, method)` (no arguments).
///
/// # Safety
/// `env`/`obj`/`method` must be valid and the method must take no arguments.
pub unsafe fn call_object_method(env: *mut c_void, obj: JObject, method: JMethodId) -> Option<JObject> {
    let f: unsafe extern "C" fn(*mut c_void, JObject, JMethodId) -> JObject =
        std::mem::transmute(function(env, slot::CALL_OBJECT_METHOD));
    let result = f(env, obj, method);
    if result.is_null() {
        None
    } else {
        Some(result)
    }
}

/// `env->GetMethodID(class, name, signature)`.
///
/// # Safety
/// `env`/`class` must be valid; `name`/`signature` NUL-terminated.
pub unsafe fn get_method_id(
    env: *mut c_void,
    class: JClass,
    name: &CStr,
    signature: &CStr,
) -> Option<JMethodId> {
    let f: unsafe extern "C" fn(*mut c_void, JClass, *const c_char, *const c_char) -> JMethodId =
        std::mem::transmute(function(env, slot::GET_METHOD_ID));
    let id = f(env, class, name.as_ptr(), signature.as_ptr());
    if id.is_null() {
        None
    } else {
        Some(id)
    }
}

/// `env->ExceptionDescribe()`: prints the pending exception and its stack trace
/// to the JVM's error stream. Destructive-safe: it does not clear the exception.
///
/// # Safety
/// `env` must be valid.
pub unsafe fn exception_describe(env: *mut c_void) {
    let f: unsafe extern "C" fn(*mut c_void) =
        std::mem::transmute(function(env, slot::EXCEPTION_DESCRIBE));
    f(env);
}

/// `env->CallStaticObjectMethod(class, method, arg)`.
///
/// # Safety
/// Same contract as [`call_static_void_method`]; the method must return an object.
pub unsafe fn call_static_object_method(
    env: *mut c_void,
    class: JClass,
    method: JMethodId,
    arg: JObject,
) -> Option<JObject> {
    let f: unsafe extern "C" fn(*mut c_void, JClass, JMethodId, JObject) -> JObject =
        std::mem::transmute(function(env, slot::CALL_STATIC_OBJECT_METHOD));
    let result = f(env, class, method, arg);
    if result.is_null() {
        None
    } else {
        Some(result)
    }
}

/// `env->DeleteGlobalRef(obj)`.
///
/// # Safety
/// `obj` must be a global (or weak global) reference.
pub unsafe fn delete_global_ref(env: *mut c_void, obj: JObject) {
    let f: unsafe extern "C" fn(*mut c_void, JObject) =
        std::mem::transmute(function(env, slot::DELETE_GLOBAL_REF));
    f(env, obj);
}

/// `env->NewGlobalRef(obj)`.
///
/// # Safety
/// `env` must be valid; `obj` may be null (JNI returns null).
pub unsafe fn new_global_ref(env: *mut c_void, obj: JObject) -> Option<JObject> {
    let f: unsafe extern "C" fn(*mut c_void, JObject) -> JObject =
        std::mem::transmute(function(env, slot::NEW_GLOBAL_REF));
    let global = f(env, obj);
    if global.is_null() {
        None
    } else {
        Some(global)
    }
}

/// Every slot this crate uses, measured against OpenJDK 17 `jni.h`.
///
/// Exposed so a test can assert the numbers stay in sync with the documentation
/// and so a reviewer can diff them against `docs/ARCHITECTURE.md`.
pub const fn slot_table() -> &'static [(usize, &'static str)] {
    &[
        (4, "GetVersion"),
        (6, "FindClass"),
        (14, "ThrowNew"),
        (15, "ExceptionOccurred"),
        (16, "ExceptionDescribe"),
        (17, "ExceptionClear"),
        (21, "NewGlobalRef"),
        (22, "DeleteGlobalRef"),
        (23, "DeleteLocalRef"),
        (25, "NewLocalRef"),
        (31, "GetObjectClass"),
        (33, "GetMethodID"),
        (34, "CallObjectMethod"),
        (61, "CallVoidMethod"),
        (113, "GetStaticMethodID"),
        (114, "CallStaticObjectMethod"),
        (141, "CallStaticVoidMethod"),
        (167, "NewStringUTF"),
        (168, "GetStringUTFLength"),
        (169, "GetStringUTFChars"),
        (170, "ReleaseStringUTFChars"),
        (219, "GetJavaVM"),
        (228, "ExceptionCheck"),
    ]
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
#[derive(Debug)]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The constants and the documented table must agree. This is the guard
    /// against the class of bug that crashed a JVM here once: a wrong slot index
    /// is not a compile error, it is a segfault at runtime.
    #[test]
    fn slot_constants_match_the_measured_table() {
        let table = slot_table();
        let lookup = |name: &str| {
            table
                .iter()
                .find(|(_, label)| *label == name)
                .map(|(index, _)| *index)
        };
        assert_eq!(Some(slot::GET_VERSION), lookup("GetVersion"));
        assert_eq!(Some(slot::FIND_CLASS), lookup("FindClass"));
        assert_eq!(Some(slot::THROW_NEW), lookup("ThrowNew"));
        assert_eq!(Some(slot::EXCEPTION_OCCURRED), lookup("ExceptionOccurred"));
        assert_eq!(Some(slot::EXCEPTION_DESCRIBE), lookup("ExceptionDescribe"));
        assert_eq!(Some(slot::EXCEPTION_CLEAR), lookup("ExceptionClear"));
        assert_eq!(Some(slot::NEW_GLOBAL_REF), lookup("NewGlobalRef"));
        assert_eq!(Some(slot::DELETE_GLOBAL_REF), lookup("DeleteGlobalRef"));
        assert_eq!(Some(slot::DELETE_LOCAL_REF), lookup("DeleteLocalRef"));
        assert_eq!(Some(slot::NEW_LOCAL_REF), lookup("NewLocalRef"));
        assert_eq!(Some(slot::GET_OBJECT_CLASS), lookup("GetObjectClass"));
        assert_eq!(Some(slot::GET_METHOD_ID), lookup("GetMethodID"));
        assert_eq!(Some(slot::CALL_OBJECT_METHOD), lookup("CallObjectMethod"));
        assert_eq!(Some(slot::CALL_VOID_METHOD), lookup("CallVoidMethod"));
        assert_eq!(Some(slot::GET_STATIC_METHOD_ID), lookup("GetStaticMethodID"));
        assert_eq!(
            Some(slot::CALL_STATIC_OBJECT_METHOD),
            lookup("CallStaticObjectMethod")
        );
        assert_eq!(
            Some(slot::CALL_STATIC_VOID_METHOD),
            lookup("CallStaticVoidMethod")
        );
        assert_eq!(Some(slot::NEW_STRING_UTF), lookup("NewStringUTF"));
        assert_eq!(
            Some(slot::GET_STRING_UTF_LENGTH),
            lookup("GetStringUTFLength")
        );
        assert_eq!(Some(slot::GET_STRING_UTF_CHARS), lookup("GetStringUTFChars"));
        assert_eq!(
            Some(slot::RELEASE_STRING_UTF_CHARS),
            lookup("ReleaseStringUTFChars")
        );
        assert_eq!(Some(slot::GET_JAVA_VM), lookup("GetJavaVM"));
        assert_eq!(Some(slot::EXCEPTION_CHECK), lookup("ExceptionCheck"));
    }

    /// The variadic and array variants of the static call family are adjacent
    /// but distinct; using the `A` slot for a vararg call crashes the JVM.
    #[test]
    fn static_call_variants_are_not_conflated() {
        assert_ne!(slot::CALL_STATIC_OBJECT_METHOD, 115);
        assert_ne!(slot::CALL_STATIC_OBJECT_METHOD, 116);
        assert_eq!(slot::CALL_STATIC_VOID_METHOD, 141);
        // CallStaticVoidMethodV = 142, CallStaticVoidMethodA = 143.
        assert!(slot::CALL_STATIC_VOID_METHOD < 142);
    }
}
