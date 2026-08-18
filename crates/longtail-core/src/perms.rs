//! POSIX-style permission bits (`docs/format-spec.md` §7).

use core::fmt::{self, Write as _};

/// A permission field as stored in a VersionIndex / FileInfos entry.
///
/// The low 9 bits are the standard POSIX `rwxrwxrwx` triplets (§7); bits 9-15
/// are "unused, always 0 in practice" but a wild file could set them and C
/// round-trips them verbatim (raw `memcpy`). This newtype therefore **preserves
/// all 16 bits** on parse/serialize and never masks to the low 9.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Permissions(pub u16);

impl Permissions {
    // §7 bit constants (octal, matching `src/longtail.h:314-324`).
    pub const OTHER_EXECUTE: u16 = 0o0001;
    pub const OTHER_WRITE: u16 = 0o0002;
    pub const OTHER_READ: u16 = 0o0004;
    pub const GROUP_EXECUTE: u16 = 0o0010;
    pub const GROUP_WRITE: u16 = 0o0020;
    pub const GROUP_READ: u16 = 0o0040;
    pub const USER_EXECUTE: u16 = 0o0100;
    pub const USER_WRITE: u16 = 0o0200;
    pub const USER_READ: u16 = 0o0400;

    /// Mask covering the 9 meaningful POSIX bits.
    pub const POSIX_MASK: u16 = 0o0777;

    /// Construct from raw bits (all 16 preserved).
    #[inline]
    pub const fn from_bits(bits: u16) -> Self {
        Permissions(bits)
    }

    /// The raw 16-bit value.
    #[inline]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// True if any of the given bits are set.
    #[inline]
    pub const fn contains(self, bits: u16) -> bool {
        (self.0 & bits) == bits
    }
}

impl From<u16> for Permissions {
    fn from(v: u16) -> Self {
        Permissions(v)
    }
}

impl From<Permissions> for u16 {
    fn from(p: Permissions) -> u16 {
        p.0
    }
}

impl fmt::Display for Permissions {
    /// Render the low 9 bits as a `rwxrwxrwx` string (a `-` for each cleared
    /// bit). High bits, if set, are ignored for display but preserved in the
    /// stored value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const TRIPLETS: [(u16, char); 9] = [
            (Permissions::USER_READ, 'r'),
            (Permissions::USER_WRITE, 'w'),
            (Permissions::USER_EXECUTE, 'x'),
            (Permissions::GROUP_READ, 'r'),
            (Permissions::GROUP_WRITE, 'w'),
            (Permissions::GROUP_EXECUTE, 'x'),
            (Permissions::OTHER_READ, 'r'),
            (Permissions::OTHER_WRITE, 'w'),
            (Permissions::OTHER_EXECUTE, 'x'),
        ];
        for (bit, ch) in TRIPLETS {
            f.write_char(if self.0 & bit != 0 { ch } else { '-' })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_rwx() {
        assert_eq!(Permissions(0o644).to_string(), "rw-r--r--");
        assert_eq!(Permissions(0o755).to_string(), "rwxr-xr-x");
        assert_eq!(Permissions(0o777).to_string(), "rwxrwxrwx");
        assert_eq!(Permissions(0o000).to_string(), "---------");
    }

    #[test]
    fn high_bits_preserved_but_not_displayed() {
        let p = Permissions(0xF1FF); // high nibble set + all posix bits
        assert_eq!(p.bits(), 0xF1FF);
        assert_eq!(p.to_string(), "rwxrwxrwx");
    }
}
