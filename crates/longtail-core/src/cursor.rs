//! Private little-endian byte cursor.
//!
//! All on-disk formats are packed struct-of-arrays with **no alignment
//! padding** (`docs/format-spec.md` cross-cutting rules): a `u64` array may
//! begin on a 4-byte boundary. We therefore read and write every scalar through
//! `from_le_bytes`/`to_le_bytes` over byte subslices and never reinterpret the
//! buffer as `&[u64]` (which would be UB on the unaligned case). This keeps
//! `longtail-core` `#![forbid(unsafe_code)]`.

use crate::error::FormatError;

/// Multiply two `usize`s, returning [`FormatError::SizeOverflow`] on overflow.
/// Header counts are attacker-controlled, so every size computation is checked.
#[inline]
pub(crate) fn checked_mul(a: usize, b: usize) -> Result<usize, FormatError> {
    a.checked_mul(b).ok_or(FormatError::SizeOverflow)
}

/// Add two `usize`s, returning [`FormatError::SizeOverflow`] on overflow.
#[inline]
pub(crate) fn checked_add(a: usize, b: usize) -> Result<usize, FormatError> {
    a.checked_add(b).ok_or(FormatError::SizeOverflow)
}

/// A forward-only reader over a borrowed byte buffer.
pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    /// Number of bytes consumed so far (also the offset of the next byte).
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    /// The remaining, unconsumed bytes.
    pub(crate) fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }

    /// Consume exactly `n` bytes, or fail with a truncation error.
    fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        let end = checked_add(self.pos, n)?;
        if end > self.data.len() {
            return Err(FormatError::Truncated {
                expected: end,
                actual: self.data.len(),
            });
        }
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    pub(crate) fn u32(&mut self) -> Result<u32, FormatError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, FormatError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub(crate) fn u16_vec(&mut self, n: usize) -> Result<Vec<u16>, FormatError> {
        let bytes = checked_mul(n, 2)?;
        let b = self.take(bytes)?;
        Ok(b.chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect())
    }

    pub(crate) fn u32_vec(&mut self, n: usize) -> Result<Vec<u32>, FormatError> {
        let bytes = checked_mul(n, 4)?;
        let b = self.take(bytes)?;
        Ok(b.chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect())
    }

    pub(crate) fn u64_vec(&mut self, n: usize) -> Result<Vec<u64>, FormatError> {
        let bytes = checked_mul(n, 8)?;
        let b = self.take(bytes)?;
        Ok(b.chunks_exact(8)
            .map(|c| u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
            .collect())
    }
}

/// A growable little-endian byte writer.
pub(crate) struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub(crate) fn with_capacity(cap: usize) -> Self {
        Writer {
            buf: Vec::with_capacity(cap),
        }
    }

    pub(crate) fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn u32_slice(&mut self, s: &[u32]) {
        for &v in s {
            self.u32(v);
        }
    }

    pub(crate) fn u64_slice(&mut self, s: &[u64]) {
        for &v in s {
            self.u64(v);
        }
    }

    pub(crate) fn bytes(&mut self, s: &[u8]) {
        self.buf.extend_from_slice(s);
    }

    pub(crate) fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}
