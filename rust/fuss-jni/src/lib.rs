//! JNI bridge between the Kotlin app and `fuss-core`.
//!
//! Design rules:
//! * One JNI call per *batch* of work, never per file. Listings and searches are
//!   encoded into a single `ByteArray` (see `fuss_core::codec`).
//! * Strings are taken from the JVM as UTF-8 directly into a `CString`-free
//!   path so there is exactly one copy per path.
//! * No JVM calls are made on worker threads; callbacks are polled instead
//!   (Cancellation is a shared `AtomicBool` read by the Rust worker).
//! * Every entry point catches panics and converts them into a Kotlin exception
//!   instead of aborting the process.

use fuss_core::analyze;
use fuss_core::archive;
use fuss_core::cancel::CancelToken;
use fuss_core::codec::Writer;
use fuss_core::entry::{sort_entries, Entry, SortKey};
use fuss_core::error::{ErrorCode, FsError};
use fuss_core::fsops::{self, CollisionMode, OpStats};
use fuss_core::listing::{list_dir, ListOptions};
use fuss_core::path::normalize_absolute;
use fuss_core::search::{search, SearchOptions};
use jni::objects::{JByteArray, JClass, JObject, JString};
use jni::sys::{jboolean, jbyteArray, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};


// ---------------------------------------------------------------------------
// Cancel handles
// ---------------------------------------------------------------------------

/// Monotonic handle table for cancel tokens handed to Kotlin. Kotlin only ever
/// passes the `long` back, so no JVM object references are held natively.
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static TOKENS: OnceLock<Mutex<Vec<(u64, Arc<CancelToken>)>>> = OnceLock::new();

fn token_table() -> &'static Mutex<Vec<(u64, Arc<CancelToken>)>> {
    TOKENS.get_or_init(|| Mutex::new(Vec::new()))
}

fn insert_token(token: Arc<CancelToken>) -> u64 {
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    let mut guard = token_table().lock().unwrap_or_else(|e| e.into_inner());
    // Opportunistically prune finished tokens to keep the table tiny.
    if guard.len() > 64 {
        guard.retain(|(_, t)| !t.is_cancelled());
    }
    guard.push((handle, token));
    handle
}

fn token_for(handle: u64) -> Arc<CancelToken> {
    let guard = token_table().lock().unwrap_or_else(|e| e.into_inner());
    guard
        .iter()
        .find(|(h, _)| *h == handle)
        .map(|(_, t)| t.clone())
        .unwrap_or_else(CancelToken::new)
}

// ---------------------------------------------------------------------------
// Helpers: exceptions, strings, panic guard
// ---------------------------------------------------------------------------

fn jstr(env: &mut JNIEnv, s: &JString) -> Option<String> {
    env.get_string(s).ok().map(|s| s.into())
}

fn throw_code(env: &mut JNIEnv, e: &FsError) {
    let class = match env.find_class("cloud/fuss/core/FussException") {
        Ok(c) => c,
        Err(_) => {
            let _ = env.throw_new("java/io/IOException", e.to_string());
            return;
        }
    };
    let msg = env.new_string(e.message.clone()).ok();
    // FussException(String code, String message, String path)
    let path_str = e.path.clone().unwrap_or_default();
    let path = env.new_string(path_str).ok();
    let code = env.new_string(e.code.as_str()).ok();
    if let (Some(m), Some(p), Some(c)) = (msg, path, code) {
        let _ = env.new_object(
            class,
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
            &[(&m).into(), (&p).into(), (&c).into()],
        );
    } else {
        let _ = env.throw_new("java/io/IOException", e.to_string());
    }
}

fn throw_msg(env: &mut JNIEnv, msg: impl AsRef<str>) {
    let _ = env.throw_new("java/lang/RuntimeException", msg.as_ref());
}

macro_rules! guarded {
    ($env:expr, $body:expr) => {{
        match catch_unwind(AssertUnwindSafe(|| $body)) {
            Ok(v) => v,
            Err(p) => {
                let msg = if let Some(s) = p.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = p.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "native panic".to_string()
                };
                throw_msg(&mut $env, format!("Fuss engine panic: {msg}"));
                return std::ptr::null_mut();
            }
        }
    }};
}

fn encode_error(code: &str, message: &str) -> Vec<u8> {
    let mut w = Writer::with_capacity(64);
    w.u8(1); // status = error
    w.str(code);
    w.str(message);
    w.into_vec()
}

fn encode_ok(w: Writer) -> Vec<u8> {
    let mut out = Writer::with_capacity(w.len() + 5);
    out.u8(0); // status = ok
    out.bytes(&w.into_vec());
    out.into_vec()
}

fn to_jbytes(env: &mut JNIEnv, bytes: Vec<u8>) -> jbyteArray {
    match env.byte_array_from_slice(&bytes) {
        Ok(a) => a.into_raw(),
        Err(e) => {
            throw_msg(env, format!("failed to allocate byte array: {e}"));
            std::ptr::null_mut()
        }
    }
}

fn path_of(env: &mut JNIEnv, s: &JString) -> Option<PathBuf> {
    let raw = jstr(env, s)?;
    match normalize_absolute(&raw) {
        Ok(p) => Some(p),
        Err(e) => {
            throw_code(env, &e);
            None
        }
    }
}

fn string_vec(env: &mut JNIEnv, arr: JObject) -> Option<Vec<PathBuf>> {
    let arr = jni::objects::JObjectArray::from(arr);
    let len = env.get_array_length(&arr).ok()?;
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let item = env.get_object_array_element(&arr, i).ok()?;
        let jstring = JString::from(item);
        let s = jstr(env, &jstring)?;
        match normalize_absolute(&s) {
            Ok(p) => out.push(p),
            Err(e) => {
                throw_code(env, &e);
                return None;
            }
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Lifecycle & diagnostics
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_version(mut env: JNIEnv, _c: JClass) -> jstring {
    match env.new_string(format!(
        "{} (rustc {})",
        fuss_core::VERSION,
        option_env!("RUSTC_VERSION").unwrap_or("unknown")
    )) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Runs the engine self-test; returns "name=ok;name=fail" style report or
/// throws. Used by the app's diagnostics screen and by instrumented tests.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_selfTest(mut env: JNIEnv, _c: JClass) -> jstring {
    let report = match catch_unwind(|| fuss_core::self_test()) {
        Ok(r) => r,
        Err(_) => vec![("panic".to_string(), false)],
    };
    let text: String = report
        .iter()
        .map(|(n, ok)| format!("{n}={}", if *ok { "ok" } else { "FAIL" }))
        .collect::<Vec<_>>()
        .join(";");
    match env.new_string(text) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_newCancelToken(_env: JNIEnv, _c: JClass) -> jlong {
    insert_token(CancelToken::new()) as jlong
}

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_cancelToken(_env: JNIEnv, _c: JClass, handle: jlong) {
    token_for(handle as u64).cancel();
}

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_freeCancelToken(_env: JNIEnv, _c: JClass, handle: jlong) {
    let mut guard = token_table().lock().unwrap_or_else(|e| e.into_inner());
    guard.retain(|(h, _)| *h != handle as u64);
}

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_progress(_env: JNIEnv, _c: JClass, handle: jlong) -> jlong {
    token_for(handle as u64).processed() as jlong
}

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_progressTotal(_env: JNIEnv, _c: JClass, handle: jlong) -> jlong {
    token_for(handle as u64).total_hint() as jlong
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

/// Lists a directory.
///
/// Returns an encoded buffer:
///   status u8 (0 ok, 1 error)
///   ok:    has_more u8, count u32, entries..., total_scanned u64
///   error: code str, message str
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_listDir(
    mut env: JNIEnv,
    _c: JClass,
    path: JString,
    show_hidden: jboolean,
    sort_key: jni::sys::jint,
    ascending: jboolean,
    limit: jni::sys::jint,
    offset: jni::sys::jint,
) -> jbyteArray {
    guarded!(env, {
        let p = match path_of(&mut env, &path) {
            Some(p) => p,
            None => return std::ptr::null_mut(),
        };
        let token = CancelToken::new();
        let opts = ListOptions {
            show_hidden: show_hidden == JNI_TRUE,
            follow_symlinks: false,
            offset: offset.max(0) as usize,
            limit: limit.max(0) as usize,
        };
        match list_dir(&p, opts, &token) {
            Ok(mut listing) => {
                sort_entries(&mut listing.entries, SortKey::from_u8(sort_key as u8), ascending == JNI_TRUE);
                let mut w = Writer::with_capacity(listing.entries.len() * 64 + 32);
                w.u8(if listing.has_more { 1 } else { 0 });
                w.u32(listing.entries.len() as u32);
                for e in &listing.entries {
                    w.entry(e);
                }
                w.u64(listing.total_scanned as u64);
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

/// Stat a single entry (used to refresh one row without re-listing).
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_statEntry(mut env: JNIEnv, _c: JClass, path: JString) -> jbyteArray {
    guarded!(env, {
        let p = match path_of(&mut env, &path) {
            Some(p) => p,
            None => return std::ptr::null_mut(),
        };
        match fsops::stat_entry(&p) {
            Ok(e) => {
                let mut w = Writer::with_capacity(96);
                w.entry(&e);
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

// ---------------------------------------------------------------------------
// Paths / storage
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_exists(mut env: JNIEnv, _c: JClass, path: JString) -> jboolean {
    match path_of(&mut env, &path) {
        Some(p) => {
            if std::fs::symlink_metadata(&p).is_ok() {
                JNI_TRUE
            } else {
                JNI_FALSE
            }
        }
        None => JNI_FALSE,
    }
}

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_isDirectory(mut env: JNIEnv, _c: JClass, path: JString) -> jboolean {
    match path_of(&mut env, &path) {
        Some(p) => {
            if std::fs::metadata(&p).map(|m| m.is_dir()).unwrap_or(false) {
                JNI_TRUE
            } else {
                JNI_FALSE
            }
        }
        None => JNI_FALSE,
    }
}

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_isReadable(mut env: JNIEnv, _c: JClass, path: JString) -> jboolean {
    match path_of(&mut env, &path) {
        Some(p) => match std::fs::read_dir(&p) {
            Ok(_) => JNI_TRUE,
            Err(_) => JNI_FALSE,
        },
        None => JNI_FALSE,
    }
}

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_canonicalize(mut env: JNIEnv, _c: JClass, path: JString) -> jstring {
    let raw = match jstr(&mut env, &path) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };
    let p = PathBuf::from(&raw);
    let resolved = std::fs::canonicalize(&p)
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_else(|_| p.to_string_lossy().into_owned());
    match env.new_string(resolved) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// MIME type for a name; empty string means unknown.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_mimeType(mut env: JNIEnv, _c: JClass, name: JString, is_dir: jboolean) -> jstring {
    let n = match jstr(&mut env, &name) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };
    let m = fuss_core::ext::mime_type(&n, is_dir == JNI_TRUE);
    match env.new_string(m) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free space (bytes) available on the filesystem containing `path`.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_freeSpace(mut env: JNIEnv, _c: JClass, path: JString) -> jlong {
    let p = match path_of(&mut env, &path) {
        Some(p) => p,
        None => return 0,
    };
    free_space_of(&p) as jlong
}

fn free_space_of(p: &std::path::Path) -> u64 {
    #[repr(C)]
    #[allow(non_camel_case_types)]
    struct statvfs_t {
        f_bsize: u64,
        f_frsize: u64,
        f_blocks: u64,
        f_bfree: u64,
        f_bavail: u64,
        f_files: u64,
        f_ffree: u64,
        f_favail: u64,
        f_fsid: u64,
        f_flag: u64,
        f_namemax: u64,
        spare: [i32; 6],
    }
    extern "C" {
        fn statvfs(path: *const u8, buf: *mut statvfs_t) -> i32;
    }
    use std::ffi::CString;
    let mut target = p;
    // Walk up to the nearest existing ancestor (statvfs fails on missing paths).
    loop {
        if target.exists() {
            break;
        }
        match target.parent() {
            Some(par) => target = par,
            None => return 0,
        }
    }
    let c = match CString::new(target.as_os_str().to_string_lossy().as_bytes()) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let mut buf = std::mem::MaybeUninit::<statvfs_t>::zeroed();
    let rc = unsafe { statvfs(c.as_ptr() as *const u8, buf.as_mut_ptr()) };
    if rc != 0 {
        return 0;
    }
    let b = unsafe { buf.assume_init() };
    b.f_bavail.saturating_mul(if b.f_frsize > 0 { b.f_frsize } else { b.f_bsize })
}

/// Total size (bytes) of the filesystem containing `path` -- free + used.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_totalSpace(mut env: JNIEnv, _c: JClass, path: JString) -> jlong {
    let p = match path_of(&mut env, &path) {
        Some(p) => p,
        None => return 0,
    };
    total_space_of(&p) as jlong
}

fn total_space_of(p: &std::path::Path) -> u64 {
    #[repr(C)]
    #[allow(non_camel_case_types)]
    struct statvfs_t {
        f_bsize: u64,
        f_frsize: u64,
        f_blocks: u64,
        f_bfree: u64,
        f_bavail: u64,
        f_files: u64,
        f_ffree: u64,
        f_favail: u64,
        f_fsid: u64,
        f_flag: u64,
        f_namemax: u64,
        spare: [i32; 6],
    }
    extern "C" {
        fn statvfs(path: *const u8, buf: *mut statvfs_t) -> i32;
    }
    use std::ffi::CString;
    let mut target = p;
    loop {
        if target.exists() {
            break;
        }
        match target.parent() {
            Some(par) => target = par,
            None => return 0,
        }
    }
    let c = match CString::new(target.as_os_str().to_string_lossy().as_bytes()) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let mut buf = std::mem::MaybeUninit::<statvfs_t>::zeroed();
    let rc = unsafe { statvfs(c.as_ptr() as *const u8, buf.as_mut_ptr()) };
    if rc != 0 {
        return 0;
    }
    let b = unsafe { buf.assume_init() };
    b.f_blocks.saturating_mul(if b.f_frsize > 0 { b.f_frsize } else { b.f_bsize })
}

/// Mount points (encoded: count u32, then for each: path str, fs_type str,
/// device str, is_emulated u8). Used to offer "Internal storage" and SD cards.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_mounts(mut env: JNIEnv, _c: JClass) -> jbyteArray {
    guarded!(env, {
        let mounts = fuss_core::listing::mount_points();
        let mut w = Writer::with_capacity(64 + mounts.len() * 48);
        w.u32(mounts.len() as u32);
        for m in &mounts {
            w.str(&m.path.to_string_lossy());
            w.str(&m.fs_type);
            w.str(&m.device);
            w.u8(if m.is_emulated { 1 } else { 0 });
        }
        to_jbytes(&mut env, w.into_vec())
    })
}

// ---------------------------------------------------------------------------
// Mutation
// ---------------------------------------------------------------------------

fn op_result_bytes(env: &mut JNIEnv, stats: &OpStats, outputs: Vec<String>) -> jbyteArray {
    let mut w = Writer::with_capacity(64 + outputs.len() * 64);
    w.u64(stats.files);
    w.u64(stats.dirs);
    w.u64(stats.bytes);
    w.u64(stats.errors);
    w.u64(stats.skipped);
    match &stats.first_error {
        Some(e) => {
            w.u8(1);
            w.str(e.code.as_str());
            w.str(&e.message);
        }
        None => {
            w.u8(0);
            w.str("");
            w.str("");
        }
    }
    w.u32(outputs.len() as u32);
    for o in &outputs {
        w.str(o);
    }
    to_jbytes(env, encode_ok(w))
}

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_createFile(mut env: JNIEnv, _c: JClass, dir: JString, name: JString) -> jstring {
    let d = match path_of(&mut env, &dir) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };
    let n = match jstr(&mut env, &name) {
        Some(n) => n,
        None => return std::ptr::null_mut(),
    };
    match fsops::create_file(&d, &n) {
        Ok(p) => match env.new_string(p.to_string_lossy()) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        Err(e) => {
            throw_code(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_createDir(mut env: JNIEnv, _c: JClass, dir: JString, name: JString) -> jstring {
    let d = match path_of(&mut env, &dir) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };
    let n = match jstr(&mut env, &name) {
        Some(n) => n,
        None => return std::ptr::null_mut(),
    };
    match fsops::create_dir(&d, &n) {
        Ok(p) => match env.new_string(p.to_string_lossy()) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        Err(e) => {
            throw_code(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_rename(mut env: JNIEnv, _c: JClass, path: JString, new_name: JString) -> jstring {
    let p = match path_of(&mut env, &path) {
        Some(p) => p,
        None => return std::ptr::null_mut(),
    };
    let n = match jstr(&mut env, &new_name) {
        Some(n) => n,
        None => return std::ptr::null_mut(),
    };
    match fsops::rename_path(&p, &n) {
        Ok(p) => match env.new_string(p.to_string_lossy()) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        Err(e) => {
            throw_code(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

/// Batch copy. `mode`: 0 fail, 1 overwrite, 2 rename. Blocking; call from a
/// background thread with a cancel handle created beforehand.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_copyBatch(
    mut env: JNIEnv,
    _c: JClass,
    sources: JObject,
    dest_dir: JString,
    mode: jni::sys::jint,
    cancel_handle: jlong,
) -> jbyteArray {
    guarded!(env, {
        let srcs = match string_vec(&mut env, sources) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let dest = match path_of(&mut env, &dest_dir) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        let r = fsops::copy_batch(&srcs, &dest, CollisionMode::from_u8(mode as u8), token);
        op_result_bytes(&mut env, &r.stats, r.outputs)
    })
}

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_moveBatch(
    mut env: JNIEnv,
    _c: JClass,
    sources: JObject,
    dest_dir: JString,
    mode: jni::sys::jint,
    cancel_handle: jlong,
) -> jbyteArray {
    guarded!(env, {
        let srcs = match string_vec(&mut env, sources) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let dest = match path_of(&mut env, &dest_dir) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        let r = fsops::move_batch(&srcs, &dest, CollisionMode::from_u8(mode as u8), token);
        op_result_bytes(&mut env, &r.stats, r.outputs)
    })
}

#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_deleteBatch(
    mut env: JNIEnv,
    _c: JClass,
    sources: JObject,
    cancel_handle: jlong,
) -> jbyteArray {
    guarded!(env, {
        let srcs = match string_vec(&mut env, sources) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        let r = fsops::delete_batch(&srcs, token);
        op_result_bytes(&mut env, &r.stats, r.outputs)
    })
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

/// Encodes properties: name, path, is_dir, size, modified, mode, uid, gid,
/// inode, nlink, is_symlink, target, child_count, mime.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_properties(mut env: JNIEnv, _c: JClass, path: JString) -> jbyteArray {
    guarded!(env, {
        let p = match path_of(&mut env, &path) {
            Some(p) => p,
            None => return std::ptr::null_mut(),
        };
        match fsops::properties(&p, true) {
            Ok(pr) => {
                let mut w = Writer::with_capacity(256);
                w.str(&pr.name);
                w.str(&pr.path);
                w.u8(if pr.is_dir { 1 } else { 0 });
                w.u64(pr.size);
                w.i64(pr.modified);
                w.u32(pr.mode);
                w.u32(pr.uid);
                w.u32(pr.gid);
                w.u64(pr.inode);
                w.u64(pr.hard_links);
                w.u8(if pr.is_symlink { 1 } else { 0 });
                w.str(pr.symlink_target.as_deref().unwrap_or(""));
                w.u64(pr.child_count);
                w.str(&pr.mime);
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

/// Directory size; encoded: files u64, dirs u64, bytes u64.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_dirSize(mut env: JNIEnv, _c: JClass, path: JString, cancel_handle: jlong) -> jbyteArray {
    guarded!(env, {
        let p = match path_of(&mut env, &path) {
            Some(p) => p,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        match analyze::dir_size(&p, &token) {
            Ok(s) => {
                let mut w = Writer::with_capacity(24);
                w.u64(s.files);
                w.u64(s.dirs);
                w.u64(s.bytes);
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Search `root`. Encoded: count u32, entries (path str + entry), scanned u64,
/// truncated u8, elapsed_ms u64.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_search(
    mut env: JNIEnv,
    _c: JClass,
    root: JString,
    query: JString,
    include_hidden: jboolean,
    max_results: jni::sys::jint,
    cancel_handle: jlong,
) -> jbyteArray {
    guarded!(env, {
        let r = match path_of(&mut env, &root) {
            Some(r) => r,
            None => return std::ptr::null_mut(),
        };
        let q = match jstr(&mut env, &query) {
            Some(q) => q,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        let opts = SearchOptions {
            query: q,
            include_hidden: include_hidden == JNI_TRUE,
            max_results: if max_results <= 0 { 20_000 } else { max_results as usize },
            ..SearchOptions::new("")
        };
        match search(&r, &opts, &token) {
            Ok(res) => {
                let mut w = Writer::with_capacity(res.entries.len() * 72 + 32);
                w.u32(res.entries.len() as u32);
                for (path, e) in &res.entries {
                    w.str(&path.to_string_lossy());
                    w.entry(e);
                }
                w.u64(res.scanned_dirs + res.scanned_files);
                w.u8(if res.truncated { 1 } else { 0 });
                w.u64(res.elapsed_ms as u64);
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

/// Largest files: count u32, then (path str, size u64, modified i64) each.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_largestFiles(mut env: JNIEnv, _c: JClass, root: JString, limit: jni::sys::jint, cancel_handle: jlong) -> jbyteArray {
    guarded!(env, {
        let r = match path_of(&mut env, &root) {
            Some(r) => r,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        match analyze::largest_files(&r, limit.max(1) as usize, &token) {
            Ok(v) => {
                let mut w = Writer::with_capacity(v.len() * 64 + 8);
                w.u32(v.len() as u32);
                for h in &v {
                    w.str(&h.path);
                    w.u64(h.size);
                    w.i64(h.modified);
                }
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

/// Empty directories: count u32, then path str each.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_emptyDirs(mut env: JNIEnv, _c: JClass, root: JString, limit: jni::sys::jint, cancel_handle: jlong) -> jbyteArray {
    guarded!(env, {
        let r = match path_of(&mut env, &root) {
            Some(r) => r,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        match analyze::empty_dirs(&r, limit.max(1) as usize, &token) {
            Ok(v) => {
                let mut w = Writer::with_capacity(v.len() * 48 + 8);
                w.u32(v.len() as u32);
                for p in &v {
                    w.str(p);
                }
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

/// Type statistics: count u32, then (kind u8, file_count u64, bytes u64) each.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_typeStats(mut env: JNIEnv, _c: JClass, root: JString, cancel_handle: jlong) -> jbyteArray {
    guarded!(env, {
        let r = match path_of(&mut env, &root) {
            Some(r) => r,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        match analyze::type_stats(&r, &token) {
            Ok(v) => {
                let mut w = Writer::with_capacity(v.len() * 24 + 8);
                w.u32(v.len() as u32);
                for s in &v {
                    w.u8(s.kind.as_u8());
                    w.u64(s.count);
                    w.u64(s.bytes);
                }
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

/// Duplicate groups: count u32, then (size u64, wasted u64, path_count u32, paths...).
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_duplicates(mut env: JNIEnv, _c: JClass, root: JString, cancel_handle: jlong) -> jbyteArray {
    guarded!(env, {
        let r = match path_of(&mut env, &root) {
            Some(r) => r,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        match analyze::find_duplicates(&r, 1, u64::MAX / 2, &token) {
            Ok(v) => {
                let mut w = Writer::with_capacity(v.len() * 96 + 8);
                w.u32(v.len() as u32);
                for g in &v {
                    w.u64(g.size);
                    w.u64(g.wasted);
                    w.u32(g.paths.len() as u32);
                    for p in &g.paths {
                        w.str(p);
                    }
                }
                w.u64(g_sum_wasted(&v));
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

fn g_sum_wasted(v: &[analyze::DuplicateGroup]) -> u64 {
    v.iter().map(|g| g.wasted).sum()
}

/// Per-child directory sizes: count u32, then (path str, bytes u64, files u64, dirs u64).
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_childDirSizes(mut env: JNIEnv, _c: JClass, root: JString, cancel_handle: jlong) -> jbyteArray {
    guarded!(env, {
        let r = match path_of(&mut env, &root) {
            Some(r) => r,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        match analyze::child_dir_sizes(&r, &token) {
            Ok(v) => {
                let mut w = Writer::with_capacity(v.len() * 64 + 8);
                w.u32(v.len() as u32);
                for s in &v {
                    w.str(&s.path);
                    w.u64(s.bytes);
                    w.u64(s.files);
                    w.u64(s.dirs);
                }
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

// ---------------------------------------------------------------------------
// Archives
// ---------------------------------------------------------------------------

/// List archive contents: format str, total_bytes u64, count u32,
/// then (path str, is_dir u8, size u64, modified i64, compressed u64).
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_listArchive(mut env: JNIEnv, _c: JClass, path: JString, cancel_handle: jlong) -> jbyteArray {
    guarded!(env, {
        let p = match path_of(&mut env, &path) {
            Some(p) => p,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        match archive::list(&p, &token) {
            Ok(c) => {
                let mut w = Writer::with_capacity(c.entries.len() * 56 + 64);
                w.str(c.format);
                w.u64(c.total_bytes);
                w.u64(c.total_compressed);
                w.u32(c.entries.len() as u32);
                for e in &c.entries {
                    w.str(&e.path);
                    w.u8(if e.is_dir { 1 } else { 0 });
                    w.u64(e.size);
                    w.i64(e.modified);
                    w.u64(e.compressed_size);
                }
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

/// Extract archive to dest: files u64, dirs u64, bytes u64, skipped u64,
/// error_count u32, errors...
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_extractArchive(mut env: JNIEnv, _c: JClass, path: JString, dest: JString, cancel_handle: jlong) -> jbyteArray {
    guarded!(env, {
        let p = match path_of(&mut env, &path) {
            Some(p) => p,
            None => return std::ptr::null_mut(),
        };
        let d = match path_of(&mut env, &dest) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        match archive::extract(&p, &d, &token) {
            Ok(s) => {
                let mut w = Writer::with_capacity(64 + s.errors.len() * 64);
                w.u64(s.files);
                w.u64(s.dirs);
                w.u64(s.bytes);
                w.u64(s.skipped);
                w.u32(s.errors.len() as u32);
                for e in &s.errors {
                    w.str(e);
                }
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

/// Create archive from sources. format: "zip", "tar", "tar.gz".
/// Returns bytes written (u64) on success.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_createArchive(
    mut env: JNIEnv,
    _c: JClass,
    sources: JObject,
    archive_path: JString,
    format: JString,
    cancel_handle: jlong,
) -> jbyteArray {
    guarded!(env, {
        let srcs = match string_vec(&mut env, sources) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let out = match path_of(&mut env, &archive_path) {
            Some(p) => p,
            None => return std::ptr::null_mut(),
        };
        let fmt = match jstr(&mut env, &format) {
            Some(f) => f,
            None => return std::ptr::null_mut(),
        };
        let token = token_for(cancel_handle as u64);
        let res = match fmt.as_str() {
            "zip" => archive::create_zip(&srcs, &out, &token),
            "tar" => archive::create_tar(&srcs, &out, false, &token),
            "tar.gz" => archive::create_tar(&srcs, &out, true, &token),
            other => Err(FsError::new(ErrorCode::Unsupported, format!("Unsupported archive format '{other}'"))),
        };
        match res {
            Ok(n) => {
                let mut w = Writer::with_capacity(8);
                w.u64(n);
                to_jbytes(&mut env, encode_ok(w))
            }
            Err(e) => to_jbytes(&mut env, encode_error(e.code.as_str(), &e.message)),
        }
    })
}

/// True if the path looks like an archive this engine can open. Used to route
/// taps in the UI to the archive browser.
#[no_mangle]
pub extern "system" fn Java_cloud_fuss_core_FussNative_isArchive(mut env: JNIEnv, _c: JClass, path: JString) -> jboolean {
    let p = match path_of(&mut env, &path) {
        Some(p) => p,
        None => return JNI_FALSE,
    };
    let fmt = archive::detect(&p);
    if fmt != archive::ArchiveFormat::Unsupported {
        return JNI_TRUE;
    }
    if archive::is_tar(&p) {
        return JNI_TRUE;
    }
    JNI_FALSE
}

// Silence unused-import warnings that only appear on some cfg combinations.
#[allow(dead_code)]
fn _keep(_: &Entry, _: &JByteArray) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_handles_roundtrip() {
        let h = insert_token(CancelToken::new());
        assert!(!token_for(h).is_cancelled());
        token_for(h).cancel();
        assert!(token_for(h).is_cancelled());
        let mut guard = token_table().lock().unwrap();
        guard.retain(|(id, _)| *id != h);
        drop(guard);
        // Unknown handles must be safe no-ops, never panics.
        assert!(!token_for(h).is_cancelled());
    }

    #[test]
    fn progress_visible_through_handle() {
        let h = insert_token(CancelToken::new());
        token_for(h).add_processed(42);
        assert_eq!(token_for(h).processed(), 42);
    }

    #[test]
    fn error_encoding_is_decodable() {
        let bytes = encode_error("not_found", "gone");
        let mut r = fuss_core::codec::Reader::new(&bytes);
        assert_eq!(r.u8(), Some(1));
        assert_eq!(r.str().as_deref(), Some("not_found"));
        assert_eq!(r.str().as_deref(), Some("gone"));
    }

    #[test]
    fn ok_encoding_roundtrips_payload() {
        let mut w = fuss_core::codec::Writer::with_capacity(16);
        w.u64(7);
        let bytes = encode_ok(w);
        let mut r = fuss_core::codec::Reader::new(&bytes);
        assert_eq!(r.u8(), Some(0));
        let payload = r.bytes().unwrap();
        let mut inner = fuss_core::codec::Reader::new(payload);
        assert_eq!(inner.u64(), Some(7));
    }
}
