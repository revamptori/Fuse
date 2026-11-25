//! Cooperative cancellation.
//!
//! Long-running operations (recursive copy, search, size analysis, archive
//! extraction) poll a `CancelToken`. The Kotlin side sets it from the UI's
//! cancel button or when a screen is left. The token is plain atomics, so
//! polling costs a relaxed load.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct CancelToken {
    cancelled: AtomicBool,
    /// Bytes/items processed so far, readable from the UI thread without locks.
    processed: AtomicU64,
    total_hint: AtomicU64,
}

impl CancelToken {
    pub fn new() -> Arc<CancelToken> {
        Arc::new(CancelToken::default())
    }

    #[inline]
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn add_processed(&self, n: u64) {
        self.processed.fetch_add(n, Ordering::Relaxed);
    }

    #[inline]
    pub fn set_processed(&self, n: u64) {
        self.processed.store(n, Ordering::Relaxed);
    }

    #[inline]
    pub fn processed(&self) -> u64 {
        self.processed.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn set_total_hint(&self, n: u64) {
        self.total_hint.store(n, Ordering::Relaxed);
    }

    #[inline]
    pub fn total_hint(&self) -> u64 {
        self.total_hint.load(Ordering::Relaxed)
    }

    /// Returns `Err(Cancelled)` if cancellation was requested.
    #[inline]
    pub fn check(&self) -> crate::error::Result<()> {
        if self.is_cancelled() {
            Err(crate::error::FsError::cancelled())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_flow() {
        let t = CancelToken::new();
        assert!(t.check().is_ok());
        t.cancel();
        assert_eq!(t.check().unwrap_err().code, crate::error::ErrorCode::Cancelled);
        t.add_processed(5);
        t.add_processed(7);
        assert_eq!(t.processed(), 12);
    }
}
