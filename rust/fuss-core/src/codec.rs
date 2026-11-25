//! Compact binary encoding of engine results.
//!
//! Result sets are encoded once into a single `Vec<u8>` and handed to Kotlin as
//! one byte array. Kotlin decodes it into lightweight value objects. This keeps
//! JNI calls proportional to *chunks* of work rather than to individual files,
//! which is the difference between a smooth listing of 50 000 files and a
//! frozen UI.

use crate::entry::Entry;

/// Writer with little-endian primitives.
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn with_capacity(cap: usize) -> Writer {
        Writer { buf: Vec::with_capacity(cap) }
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    pub fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn str(&mut self, s: &str) {
        // Clamp to u32::MAX for safety; real names never come close.
        let bytes = s.as_bytes();
        let len = bytes.len().min(u32::MAX as usize);
        self.u32(len as u32);
        self.buf.extend_from_slice(&bytes[..len]);
    }

    pub fn bytes(&mut self, b: &[u8]) {
        let len = b.len().min(u32::MAX as usize);
        self.u32(len as u32);
        self.buf.extend_from_slice(&b[..len]);
    }

    pub fn entry(&mut self, e: &Entry) {
        self.u32(e.name.len() as u32);
        self.buf.extend_from_slice(e.name.as_bytes());
        let mut flags = 0u8;
        if e.is_dir {
            flags |= 1;
        }
        if e.is_symlink {
            flags |= 2;
        }
        if e.stat_ok {
            flags |= 4;
        }
        if e.is_hidden() {
            flags |= 8;
        }
        self.u8(flags);
        self.u8(e.kind.as_u8());
        self.u64(e.size);
        self.i64(e.modified);
        self.u32(e.mode);
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}

/// Reader mirroring `Writer`. All accessors are bounds-checked; malformed input
/// yields `None` instead of panicking (the data originates from Rust, but the
/// decoder is also used in tests and benchmarks, so safety is cheap here).
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Reader<'a> {
        Reader { buf, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        if end > self.buf.len() {
            return None;
        }
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Some(s)
    }

    pub fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    pub fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| {
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            u64::from_le_bytes(a)
        })
    }

    pub fn i64(&mut self) -> Option<i64> {
        self.u64().map(|v| v as i64)
    }

    pub fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec()).ok()
    }

    pub fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }

    pub fn entry(&mut self) -> Option<Entry> {
        let name = self.str()?;
        let flags = self.u8()?;
        let kind = crate::ext::FileKind::from_u8(self.u8()?);
        let size = self.u64()?;
        let modified = self.i64()?;
        let mode = self.u32()?;
        Some(Entry {
            name,
            is_dir: flags & 1 != 0,
            is_symlink: flags & 2 != 0,
            stat_ok: flags & 4 != 0,
            kind,
            size,
            modified,
            mode,
        })
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Entry;

    #[test]
    fn roundtrip() {
        let e = Entry {
            name: "héllo wörld.txt".into(),
            is_dir: false,
            is_symlink: true,
            stat_ok: true,
            size: 1234567890123,
            modified: 1_700_000_000,
            mode: 0o100644,
            kind: crate::ext::FileKind::Text,
        };
        let mut w = Writer::with_capacity(64);
        w.entry(&e);
        let bytes = w.into_vec();
        let mut r = Reader::new(&bytes);
        let d = r.entry().unwrap();
        assert_eq!(d.name, e.name);
        assert_eq!(d.size, e.size);
        assert_eq!(d.mode, e.mode);
        assert!(d.is_symlink);
        assert!(r.at_end());
    }

    #[test]
    fn malformed_is_none() {
        let mut r = Reader::new(&[1, 2]);
        assert!(r.u32().is_none());
        assert!(r.entry().is_none());
    }
}
