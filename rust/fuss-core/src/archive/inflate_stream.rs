//! Streaming (blocking) DEFLATE / GZIP adapters over `miniz_oxide`.
//!
//! `miniz_oxide` ships raw streaming primitives (`inflate`, `deflate`); these
//! thin wrappers give the archive code idiomatic `Read`/`Write` streams with a
//! fixed-size window, so memory use is constant regardless of file size.

use crate::archive::crc32::Crc32;
use miniz_oxide::deflate::core::CompressorOxide;
use miniz_oxide::inflate::stream::{inflate, InflateState};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus, StreamResult};
use std::io::{self, Read, Write};

const WINDOW: usize = 64 * 1024;

fn corrupt(msg: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

fn truncated() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "truncated compressed stream")
}

// ---------------------------------------------------------------------------
// Raw DEFLATE decoder
// ---------------------------------------------------------------------------

pub struct DeflateDecoder<R: Read> {
    inner: R,
    state: Box<InflateState>,
    in_buf: Vec<u8>,
    in_pos: usize,
    in_len: usize,
    input_eof: bool,
    finished: bool,
}

impl<R: Read> DeflateDecoder<R> {
    pub fn new(inner: R) -> Self {
        DeflateDecoder {
            inner,
            state: InflateState::new_boxed(DataFormat::Raw),
            in_buf: vec![0u8; WINDOW],
            in_pos: 0,
            in_len: 0,
            input_eof: false,
            finished: false,
        }
    }

    fn fill(&mut self) -> io::Result<bool> {
        if self.in_pos < self.in_len {
            return Ok(true);
        }
        if self.input_eof {
            return Ok(false);
        }
        loop {
            match self.inner.read(&mut self.in_buf) {
                Ok(0) => {
                    self.input_eof = true;
                    return Ok(false);
                }
                Ok(n) => {
                    self.in_pos = 0;
                    self.in_len = n;
                    return Ok(true);
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }
}

impl<R: Read> Read for DeflateDecoder<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.finished || out.is_empty() {
            return Ok(0);
        }
        loop {
            let have_input = self.fill()?;
            let flush = if have_input { MZFlush::None } else { MZFlush::Finish };
            let r: StreamResult =
                inflate(&mut self.state, &self.in_buf[self.in_pos..self.in_len], out, flush);
            self.in_pos += r.bytes_consumed;
            match r.status {
                Ok(MZStatus::StreamEnd) => {
                    self.finished = true;
                    return Ok(r.bytes_written);
                }
                Ok(_) => {
                    if r.bytes_written > 0 {
                        return Ok(r.bytes_written);
                    }
                    if !have_input {
                        return Err(truncated());
                    }
                }
                Err(MZError::Buf) if have_input => continue,
                Err(MZError::Buf) => return Err(truncated()),
                Err(_) => return Err(corrupt("corrupt deflate stream")),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Raw DEFLATE encoder
// ---------------------------------------------------------------------------

pub struct DeflateEncoder<W: Write> {
    inner: W,
    comp: Box<CompressorOxide>,
    out_buf: Vec<u8>,
    finished: bool,
}

impl<W: Write> DeflateEncoder<W> {
    /// `level` is a DEFLATE level 0-10 (6 is a good size/speed tradeoff).
    pub fn new(inner: W, level: u8) -> Self {
        DeflateEncoder {
            inner,
            comp: Box::new(CompressorOxide::new(level as u32)),
            out_buf: vec![0u8; WINDOW],
            finished: false,
        }
    }

    fn run(&mut self, flush: MZFlush, input: &[u8]) -> io::Result<()> {
        let mut consumed = 0usize;
        let mut guard = 0u32;
        loop {
            guard += 1;
            if guard > 1_000_000 {
                return Err(corrupt("deflate made no progress"));
            }
            let r: StreamResult = miniz_oxide::deflate::stream::deflate(
                &mut self.comp,
                &input[consumed..],
                &mut self.out_buf,
                flush,
            );
            consumed += r.bytes_consumed;
            if r.bytes_written > 0 {
                self.inner.write_all(&self.out_buf[..r.bytes_written])?;
            }
            match r.status {
                Ok(MZStatus::StreamEnd) => return Ok(()),
                Ok(_) => {
                    if consumed >= input.len() && r.bytes_written == 0 {
                        return Ok(());
                    }
                }
                Err(MZError::Buf) => return Ok(()),
                Err(_) => return Err(corrupt("deflate failure")),
            }
        }
    }

    pub fn finish(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        self.run(MZFlush::Finish, &[])?;
        self.inner.flush()
    }

    /// Give back the underlying writer (after `finish`).
    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: Write> Write for DeflateEncoder<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.finished {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "stream finished"));
        }
        if !buf.is_empty() {
            self.run(MZFlush::None, buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

// ---------------------------------------------------------------------------
// GZIP decoder (RFC 1952), including multi-member files
// ---------------------------------------------------------------------------

pub struct GzipDecoder<R: Read> {
    /// Input reader; wrapped in `Option` so members can be swapped out.
    inner: Option<R>,
    /// Pending decompressed request state for the current member.
    member: Option<Box<InflateState>>,
    in_buf: Vec<u8>,
    in_pos: usize,
    in_len: usize,
    member_crc: Crc32,
    member_size: u32,
    finished: bool,
}

impl<R: Read> GzipDecoder<R> {
    pub fn new(inner: R) -> Self {
        GzipDecoder {
            inner: Some(inner),
            member: None,
            in_buf: vec![0u8; WINDOW],
            in_pos: 0,
            in_len: 0,
            member_crc: Crc32::new(),
            member_size: 0,
            finished: false,
        }
    }

    fn r(&mut self) -> &mut R {
        self.inner.as_mut().expect("gzip reader taken")
    }

    fn refill(&mut self) -> io::Result<bool> {
        if self.in_pos < self.in_len {
            return Ok(true);
        }
        loop {
            // Split the borrow: read into a local, then copy into in_buf.
            let mut tmp = std::mem::take(&mut self.in_buf);
            let result = self.r().read(&mut tmp);
            self.in_buf = tmp;
            match result {
                Ok(0) => return Ok(false),
                Ok(n) => {
                    self.in_pos = 0;
                    self.in_len = n;
                    return Ok(true);
                }
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    fn read_exact_here(&mut self, out: &mut [u8]) -> io::Result<()> {
        let mut filled = 0;
        while filled < out.len() {
            if !self.refill()? {
                return Err(truncated());
            }
            let avail = self.in_len - self.in_pos;
            let n = avail.min(out.len() - filled);
            out[filled..filled + n].copy_from_slice(&self.in_buf[self.in_pos..self.in_pos + n]);
            self.in_pos += n;
            filled += n;
        }
        Ok(())
    }

    /// Start the next gzip member; returns false at clean end of file.
    fn begin_member(&mut self) -> io::Result<bool> {
        let mut hdr = [0u8; 10];
        let mut got = 0;
        while got < 10 {
            if !self.refill()? {
                if got == 0 {
                    return Ok(false);
                }
                return Err(truncated());
            }
            let avail = self.in_len - self.in_pos;
            let n = avail.min(10 - got);
            hdr[got..got + n].copy_from_slice(&self.in_buf[self.in_pos..self.in_pos + n]);
            self.in_pos += n;
            got += n;
        }
        if hdr[0] != 0x1F || hdr[1] != 0x8B || hdr[2] != 8 {
            return Err(corrupt("not a gzip stream"));
        }
        let flags = hdr[3];
        if flags & 0x04 != 0 {
            let mut xlen = [0u8; 2];
            self.read_exact_here(&mut xlen)?;
            let n = u16::from_le_bytes(xlen) as usize;
            let mut skip = vec![0u8; n];
            self.read_exact_here(&mut skip)?;
        }
        if flags & 0x08 != 0 {
            self.skip_cstr()?;
        }
        if flags & 0x10 != 0 {
            self.skip_cstr()?;
        }
        if flags & 0x02 != 0 {
            let mut crc = [0u8; 2];
            self.read_exact_here(&mut crc)?;
        }
        self.member = Some(InflateState::new_boxed(DataFormat::Raw));
        self.member_crc = Crc32::new();
        self.member_size = 0;
        Ok(true)
    }

    fn skip_cstr(&mut self) -> io::Result<()> {
        loop {
            if !self.refill()? {
                return Err(truncated());
            }
            let b = self.in_buf[self.in_pos];
            self.in_pos += 1;
            if b == 0 {
                return Ok(());
            }
        }
    }

    /// Read and validate the current member's trailer.
    fn end_member(&mut self) -> io::Result<()> {
        let mut trailer = [0u8; 8];
        self.read_exact_here(&mut trailer)?;
        let crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
        let isize = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
        if crc != self.member_crc.finish() {
            return Err(corrupt("gzip CRC mismatch"));
        }
        if isize != self.member_size {
            return Err(corrupt("gzip size mismatch"));
        }
        self.member = None;
        Ok(())
    }
}

impl<R: Read> Read for GzipDecoder<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.finished || out.is_empty() {
            return Ok(0);
        }
        loop {
            if self.member.is_none() {
                if !self.begin_member()? {
                    self.finished = true;
                    return Ok(0);
                }
            }
            let have_input = self.refill()?;
            let flush = if have_input { MZFlush::None } else { MZFlush::Finish };
            let state = self.member.as_mut().expect("member state");
            let r: StreamResult =
                inflate(state, &self.in_buf[self.in_pos..self.in_len], out, flush);
            self.in_pos += r.bytes_consumed;
            match r.status {
                Ok(MZStatus::StreamEnd) => {
                    self.member_crc.update(&out[..r.bytes_written]);
                    self.member_size = self.member_size.wrapping_add(r.bytes_written as u32);
                    self.end_member()?;
                    return Ok(r.bytes_written);
                }
                Ok(_) => {
                    if r.bytes_written > 0 {
                        self.member_crc.update(&out[..r.bytes_written]);
                        self.member_size = self.member_size.wrapping_add(r.bytes_written as u32);
                        return Ok(r.bytes_written);
                    }
                    if !have_input {
                        return Err(truncated());
                    }
                }
                Err(MZError::Buf) if have_input => continue,
                Err(MZError::Buf) => return Err(truncated()),
                Err(_) => return Err(corrupt("corrupt gzip data")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deflate_roundtrip_streaming() {
        let payload: Vec<u8> = (0..1_000_000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        let mut enc = DeflateEncoder::new(Vec::new(), 6);
        for chunk in payload.chunks(7919) {
            enc.write_all(chunk).unwrap();
        }
        enc.finish().unwrap();
        let compressed = enc.into_inner();
        let mut dec = DeflateDecoder::new(io::Cursor::new(compressed));
        let mut out = Vec::new();
        dec.read_to_end(&mut out).unwrap();
        assert_eq!(out, payload);
    }

    #[test]
    fn gzip_roundtrip_matches_spec_crc() {
        let payload = b"the quick brown fox jumps over the lazy dog".repeat(500);
        // Build a gzip file by hand: header + raw deflate + crc + size.
        let mut enc = DeflateEncoder::new(Vec::new(), 9);
        enc.write_all(&payload).unwrap();
        enc.finish().unwrap();
        let raw = enc.into_inner();
        let mut crc = Crc32::new();
        crc.update(&payload);
        let mut gz = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 0xFF];
        gz.extend_from_slice(&raw);
        gz.extend_from_slice(&crc.finish().to_le_bytes());
        gz.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        let mut dec = GzipDecoder::new(io::Cursor::new(gz.clone()));
        let mut out = Vec::new();
        dec.read_to_end(&mut out).unwrap();
        assert_eq!(out, payload);
        // Corrupt the CRC -> must fail.
        let mut bad = gz.clone();
        let n = bad.len();
        bad[n - 8] ^= 0xFF;
        let mut dec = GzipDecoder::new(io::Cursor::new(bad));
        let mut out = Vec::new();
        assert!(dec.read_to_end(&mut out).is_err());
    }

    #[test]
    fn gzip_multimember() {
        let p1 = b"first member".repeat(100);
        let p2 = b"second member".repeat(100);
        let make = |p: &[u8]| {
            let mut enc = DeflateEncoder::new(Vec::new(), 6);
            enc.write_all(p).unwrap();
            enc.finish().unwrap();
            let raw = enc.into_inner();
            let mut crc = Crc32::new();
            crc.update(p);
            let mut gz = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 0xFF];
            gz.extend_from_slice(&raw);
            gz.extend_from_slice(&crc.finish().to_le_bytes());
            gz.extend_from_slice(&(p.len() as u32).to_le_bytes());
            gz
        };
        let mut both = make(&p1);
        both.extend_from_slice(&make(&p2));
        let mut dec = GzipDecoder::new(io::Cursor::new(both));
        let mut out = Vec::new();
        dec.read_to_end(&mut out).unwrap();
        assert_eq!(out.len(), p1.len() + p2.len());
        assert!(out.starts_with(b"first member"));
        assert!(out.ends_with(b"second member"));
    }

    #[test]
    fn truncated_streams_error() {
        let payload = b"x".repeat(10_000);
        let mut enc = DeflateEncoder::new(Vec::new(), 6);
        enc.write_all(&payload).unwrap();
        enc.finish().unwrap();
        let mut raw = enc.into_inner();
        raw.truncate(raw.len() / 2);
        let mut dec = DeflateDecoder::new(io::Cursor::new(raw));
        let mut out = Vec::new();
        assert!(dec.read_to_end(&mut out).is_err());
    }
}
