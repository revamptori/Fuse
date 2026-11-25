//! Fuss filesystem engine.
//!
//! Everything filesystem-related lives here: listing, search, file operations,
//! storage analysis, archive handling, mount discovery. The Android app talks to
//! this crate exclusively through the small JNI surface in `fuss-jni`; the same
//! crate also builds as a plain host crate for unit tests and benchmarks
//! (`fuss-bench`).
//!
//! Conventions:
//! * Paths are absolute filesystem paths. SAF documents are exposed by the
//!   Kotlin layer as `/proc/self/fd/<fd>` paths, so this crate never needs
//!   Android-specific code.
//! * All long-running functions take a `CancelToken`.
//! * Errors carry a stable `ErrorCode` plus a message.
//! * Nothing allocates proportionally to file size: copies, hashing and archive
//!   I/O all stream through fixed buffers.

pub mod analyze;
pub mod archive;
pub mod cancel;
pub mod codec;
pub mod entry;
pub mod error;
pub mod ext;
pub mod fsops;
pub mod listing;
pub mod path;
pub mod prefs;
pub mod search;

pub use cancel::CancelToken;
pub use entry::{Entry, SortKey};
pub use error::{ErrorCode, FsError, Result};
pub use listing::{ListOptions, Listing};

/// Engine version, reported to the UI so the user can confirm the native side
/// is loaded and current.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Quick self-test used by the Kotlin smoke test and by CI: round-trips a
/// temporary file through every core path and reports what worked.
pub fn self_test() -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let dir = std::env::temp_dir().join(format!("fuss-selftest-{}", std::process::id()));
    let ok = std::fs::create_dir_all(&dir).is_ok();
    out.push(("create temp dir".to_string(), ok));
    if !ok {
        return out;
    }
    let token = CancelToken::new();
    let f = crate::fsops::create_file(&dir, "a.txt");
    out.push(("create file".to_string(), f.is_ok()));
    if let Ok(ref p) = f {
        out.push(("write".to_string(), std::fs::write(p, b"fuss").is_ok()));
    }
    let list = crate::listing::list_dir(&dir, ListOptions::default(), &token);
    out.push((
        "list".to_string(),
        list.map(|l| l.entries.len() == 1).unwrap_or(false),
    ));
    let copy = f.as_ref().ok().map(|p| {
        crate::fsops::copy_file(
            p,
            &dir.join("b.txt"),
            crate::fsops::CollisionMode::Fail,
            &token,
        )
    });
    out.push(("copy".to_string(), matches!(copy, Some(Ok(_)))));
    out.push((
        "readback".to_string(),
        std::fs::read(dir.join("b.txt"))
            .map(|b| b == b"fuss")
            .unwrap_or(false),
    ));
    let zip_path = dir.join("t.zip");
    let made = crate::archive::create_zip(&[dir.join("a.txt")], &zip_path, &token);
    out.push(("zip create".to_string(), made.is_ok()));
    out.push((
        "zip list".to_string(),
        crate::archive::list(&zip_path, &token)
            .map(|c| c.entries.len() == 1)
            .unwrap_or(false),
    ));
    out.push((
        "delete".to_string(),
        crate::fsops::delete_path(&dir, &token, &mut crate::fsops::OpStats::default()).is_ok(),
    ));
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn self_test_all_pass() {
        for (name, ok) in super::self_test() {
            assert!(ok, "self test failed: {name}");
        }
    }
}
