//! The JNI bridge. Types in, types out, and nothing else.
//!
//! # The one rule
//!
//! **A panic must never cross this boundary.** Unwinding into the JVM is
//! undefined behaviour — not an exception, not a crash with a stack trace, but a
//! corrupted runtime that may fail somewhere else entirely. So every entry point
//! is wrapped in [`std::panic::catch_unwind`] and every failure becomes a thrown
//! Java exception. That is the single most important property in this file, and
//! it is the one the JVM test exercises deliberately.
//!
//! # Why so little happens here
//!
//! JNI code is the hardest kind to test: it needs a JVM, a loaded library and a
//! classpath before a single line runs. So it contains as little as possible
//! worth testing. Everything that decides anything lives in [`api`], which is
//! ordinary Rust; the tests here only have to show that the bridge passes bytes
//! through unchanged and turns errors into exceptions.
//!
//! # Exceptions
//!
//! `java.lang.IllegalStateException`, with the message the core produced. Not a
//! dedicated exception class, on purpose: `FindClass` resolves application
//! classes through the *caller's* class loader, which is absent on a thread the
//! JVM did not start — and the preview engine will call back from exactly such a
//! thread. A class that resolves everywhere is worth more than one that
//! distinguishes, until something needs the distinction.
//!
//! # Transfer
//!
//! PCM goes in as a direct `ByteBuffer`, which Rust reads in place: no copy, and
//! the JVM keeps ownership of memory it allocated. Results come back as Java
//! arrays, which does copy — once, for a few megabytes, on a call that already
//! decoded and resampled the whole file. Handing back Rust-owned memory would
//! buy that copy back at the price of a lifetime the garbage collector cannot
//! see, and a leak on any path that forgets to free it.

pub mod api;
pub mod json;

use std::panic::{catch_unwind, AssertUnwindSafe};

use jni::objects::{JByteBuffer, JClass, JString};
use jni::sys::{jbyteArray, jfloatArray, jint, jstring};
use jni::JNIEnv;

/// The exception thrown for every failure. See the module docs for why this one.
const EXCEPTION: &str = "java/lang/IllegalStateException";

/// Runs `body`, turning both errors and panics into a thrown exception.
///
/// `AssertUnwindSafe` because the closure borrows `env`, which is not
/// `UnwindSafe` and cannot be: after a caught panic the only thing done with it
/// is throwing, which needs no invariant the panic could have broken.
fn guard<T>(env: &mut JNIEnv, fallback: T, body: impl FnOnce(&mut JNIEnv) -> Result<T, String>) -> T {
    let result = catch_unwind(AssertUnwindSafe(|| body(env)));
    match result {
        Ok(Ok(value)) => value,
        Ok(Err(message)) => {
            throw(env, &message);
            fallback
        }
        Err(panic) => {
            // A panic here is a bug in this program, not bad input. It is
            // reported rather than swallowed, and it is reported as an exception
            // rather than as an abort so the app can survive one bad file.
            let what = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            throw(env, &format!("internal error: {what}"));
            fallback
        }
    }
}

fn throw(env: &mut JNIEnv, message: &str) {
    // Only one exception can be pending; throwing over one already in flight is
    // an error in itself, so an existing one is left alone.
    if env.exception_check().unwrap_or(false) {
        return;
    }
    let _ = env.throw_new(EXCEPTION, message);
}

/// The bytes behind a direct `ByteBuffer`, borrowed rather than copied.
///
/// # Safety
/// The slice is valid only while the buffer is alive and unmoved, which for a
/// direct buffer means for the duration of the call. It must not outlive it.
fn direct_bytes<'a>(env: &JNIEnv, buffer: &JByteBuffer<'a>) -> Result<&'a [u8], String> {
    if buffer.is_null() {
        return Err("audio buffer is null".to_string());
    }
    let address = env
        .get_direct_buffer_address(buffer)
        .map_err(|_| "audio must be a direct ByteBuffer — allocateDirect, not allocate".to_string())?;
    let capacity = env
        .get_direct_buffer_capacity(buffer)
        .map_err(|e| e.to_string())?;
    if address.is_null() {
        return Err("direct ByteBuffer has no address".to_string());
    }
    // SAFETY: the JVM guarantees a direct buffer's memory is stable and of the
    // reported capacity for as long as the buffer object lives, which spans this
    // call. The slice is only used before returning.
    Ok(unsafe { std::slice::from_raw_parts(address, capacity) })
}

fn string_arg(env: &mut JNIEnv, s: &JString<'_>, what: &str) -> Result<String, String> {
    if s.is_null() {
        return Err(format!("{what} is null"));
    }
    env.get_string(s)
        .map(|s| s.into())
        .map_err(|e| format!("{what}: {e}"))
}

fn to_jstring(env: &mut JNIEnv, s: &str) -> Result<jstring, String> {
    env.new_string(s).map(|s| s.into_raw()).map_err(|e| e.to_string())
}

/// `analyze(ByteBuffer audio, String name) -> String` (JSON).
///
/// # Safety
/// Called by the JVM with valid arguments; not to be called from Rust.
#[no_mangle]
pub extern "system" fn Java_org_loopslcr_Native_analyze<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass<'a>,
    audio: JByteBuffer<'a>,
    name: JString<'a>,
) -> jstring {
    guard(&mut env, std::ptr::null_mut(), |env| {
        let bytes = direct_bytes(env, &audio)?;
        let name = string_arg(env, &name, "name")?;
        let json = api::analyze(bytes, &name)?;
        to_jstring(env, &json)
    })
}

/// `plan(ByteBuffer audio, String name, String paramsJson) -> String` (JSON).
///
/// # Safety
/// Called by the JVM with valid arguments; not to be called from Rust.
#[no_mangle]
pub extern "system" fn Java_org_loopslcr_Native_plan<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass<'a>,
    audio: JByteBuffer<'a>,
    name: JString<'a>,
    params: JString<'a>,
) -> jstring {
    guard(&mut env, std::ptr::null_mut(), |env| {
        let bytes = direct_bytes(env, &audio)?;
        let name = string_arg(env, &name, "name")?;
        let params = string_arg(env, &params, "params")?;
        let json = api::plan(bytes, &name, &params)?;
        to_jstring(env, &json)
    })
}

/// `process(ByteBuffer audio, String name, String paramsJson) -> byte[]`.
///
/// # Safety
/// Called by the JVM with valid arguments; not to be called from Rust.
#[no_mangle]
pub extern "system" fn Java_org_loopslcr_Native_process<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass<'a>,
    audio: JByteBuffer<'a>,
    name: JString<'a>,
    params: JString<'a>,
) -> jbyteArray {
    guard(&mut env, std::ptr::null_mut(), |env| {
        let bytes = direct_bytes(env, &audio)?;
        let name = string_arg(env, &name, "name")?;
        let params = string_arg(env, &params, "params")?;
        let out = api::process(bytes, &name, &params)?;
        env.byte_array_from_slice(&out)
            .map(|array| array.into_raw())
            .map_err(|e| e.to_string())
    })
}

/// `peaks(ByteBuffer audio, int buckets) -> float[]`.
///
/// Interleaved `[c0min, c0max, c1min, c1max, …]` per bucket.
///
/// # Safety
/// Called by the JVM with valid arguments; not to be called from Rust.
#[no_mangle]
pub extern "system" fn Java_org_loopslcr_Native_peaks<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass<'a>,
    audio: JByteBuffer<'a>,
    buckets: jint,
) -> jfloatArray {
    guard(&mut env, std::ptr::null_mut(), |env| {
        let bytes = direct_bytes(env, &audio)?;
        let buckets = usize::try_from(buckets).map_err(|_| "buckets must not be negative".to_string())?;
        let values = api::peaks(bytes, buckets)?;

        let array = env
            .new_float_array(values.len() as jint)
            .map_err(|e| e.to_string())?;
        env.set_float_array_region(&array, 0, &values)
            .map_err(|e| e.to_string())?;
        Ok(array.into_raw())
    })
}

/// `version() -> String`. The smallest call there is, for checking that the
/// library loaded and the names match before anything harder is attempted.
///
/// # Safety
/// Called by the JVM with valid arguments; not to be called from Rust.
#[no_mangle]
pub extern "system" fn Java_org_loopslcr_Native_version<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass<'a>,
) -> jstring {
    guard(&mut env, std::ptr::null_mut(), |env| {
        to_jstring(env, env!("CARGO_PKG_VERSION"))
    })
}

/// `panicOnPurpose()`. Exists so the panic guard can be tested from Java, which
/// is the only place the guarantee actually matters.
///
/// # Safety
/// Called by the JVM with valid arguments; not to be called from Rust.
#[no_mangle]
pub extern "system" fn Java_org_loopslcr_Native_panicOnPurpose<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass<'a>,
) {
    guard(&mut env, (), |_| {
        panic!("deliberate panic, to prove it becomes an exception");
    })
}
