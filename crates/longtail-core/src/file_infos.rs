//! `FileInfos` — the in-memory scanned-file structure that feeds VersionIndex
//! construction (Stage 5). Not an on-disk format.
//!
//! Stage 2 delivers the pure, I/O-free deterministic parts: the exact C sort
//! order and the canonical name-blob building. Verified against
//! `Longtail_GetFilesRecursively2` (longtail.c:1806-1893) and
//! `SortScannedPaths` (longtail.c:1604-1632); mirrors `struct
//! Longtail_FileInfos` (longtail.h:1684).

use crate::error::FormatError;
use crate::perms::Permissions;

/// One caller-supplied entry to build a [`FileInfos`] from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Root-relative, `/`-separated, case-preserved path **without** any
    /// trailing slash — even for directories (the trailing `/` is synthesized
    /// during name-blob building, never part of the sort key).
    pub relative_path: String,
    /// Byte size of the asset; `0` for directories.
    pub size: u64,
    pub permissions: Permissions,
    pub is_dir: bool,
}

/// The deterministic, I/O-free counterpart of `struct Longtail_FileInfos`:
/// a sorted set of assets with a canonical NUL-terminated name blob.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileInfos {
    /// `m_PathData` — concatenated NUL-terminated names, directories carrying a
    /// trailing `/` before the NUL.
    pub path_data: Vec<u8>,
    /// `m_PathStartOffsets` — byte offset of each asset's name in `path_data`.
    pub path_start_offsets: Vec<u32>,
    /// `m_Sizes` — per-asset byte size (`0` for directories).
    pub sizes: Vec<u64>,
    /// `m_Permissions` — per-asset permission bits.
    pub permissions: Vec<Permissions>,
}

impl FileInfos {
    /// Build from caller-supplied entries using the **scan-path semantics**
    /// (`Longtail_GetFilesRecursively2`): a single global byte-wise `strcmp`
    /// sort over the bare relative path, then a canonical cumulative name blob
    /// with a trailing `/` inserted before the NUL for directories.
    ///
    /// This is distinct from C's caller-supplied-path variant
    /// (`LongtailPrivate_MakeFileInfos`, longtail.c:1382), which copies paths
    /// verbatim with **no** sorting and **no** trailing-`/` insertion. That
    /// verbatim variant is a Stage 5 concern — the two must not be conflated
    /// (`docs/format-spec.md` §1 caveat).
    pub fn from_scanned_entries(mut entries: Vec<FileEntry>) -> FileInfos {
        // Byte-wise strcmp over the bare relative path (no trailing slash),
        // case-sensitive, single global sort (longtail.c:1631). Rust's `[u8]`
        // ordering is lexicographic unsigned-byte order, matching `strcmp`
        // exactly (names contain no embedded NUL). Stable sort for determinism;
        // distinct assets cannot share a bare path so ties do not occur in
        // valid input.
        entries.sort_by(|a, b| a.relative_path.as_bytes().cmp(b.relative_path.as_bytes()));

        let mut path_data = Vec::new();
        let mut path_start_offsets = Vec::with_capacity(entries.len());
        let mut sizes = Vec::with_capacity(entries.len());
        let mut permissions = Vec::with_capacity(entries.len());

        for e in &entries {
            path_start_offsets.push(path_data.len() as u32);
            sizes.push(e.size);
            permissions.push(e.permissions);
            path_data.extend_from_slice(e.relative_path.as_bytes());
            if e.is_dir {
                path_data.push(b'/'); // directories get a trailing slash …
            }
            path_data.push(0); // … then the NUL terminator.
        }

        FileInfos {
            path_data,
            path_start_offsets,
            sizes,
            permissions,
        }
    }

    /// Number of assets.
    pub fn count(&self) -> u32 {
        self.path_start_offsets.len() as u32
    }

    /// `m_PathDataSize` — byte length of the name blob.
    pub fn path_data_size(&self) -> u32 {
        self.path_data.len() as u32
    }

    /// Raw name bytes of asset `i` (directories end with `/`, no NUL).
    pub fn path_bytes(&self, i: usize) -> Result<&[u8], FormatError> {
        let count = self.path_start_offsets.len();
        let offset = *self
            .path_start_offsets
            .get(i)
            .ok_or(FormatError::IndexOutOfBounds { index: i, count })?
            as usize;
        if offset > self.path_data.len() {
            return Err(FormatError::NameOffsetOutOfBounds {
                offset,
                len: self.path_data.len(),
            });
        }
        let tail = &self.path_data[offset..];
        match tail.iter().position(|&b| b == 0) {
            Some(nul) => Ok(&tail[..nul]),
            None => Err(FormatError::UnterminatedName { offset }),
        }
    }

    /// Path of asset `i` decoded as UTF-8 (directories end with `/`).
    pub fn path(&self, i: usize) -> Result<&str, FormatError> {
        let bytes = self.path_bytes(i)?;
        let offset = self.path_start_offsets[i] as usize;
        std::str::from_utf8(bytes).map_err(|_| FormatError::InvalidUtf8 { offset })
    }

    /// Byte size of asset `i`.
    pub fn size(&self, i: usize) -> Result<u64, FormatError> {
        let count = self.sizes.len();
        self.sizes
            .get(i)
            .copied()
            .ok_or(FormatError::IndexOutOfBounds { index: i, count })
    }

    /// Permissions of asset `i`.
    pub fn permissions(&self, i: usize) -> Result<Permissions, FormatError> {
        let count = self.permissions.len();
        self.permissions
            .get(i)
            .copied()
            .ok_or(FormatError::IndexOutOfBounds { index: i, count })
    }

    /// Whether asset `i` is a directory (its name ends with `/`).
    pub fn is_dir(&self, i: usize) -> Result<bool, FormatError> {
        Ok(self.path_bytes(i)?.last() == Some(&b'/'))
    }
}
