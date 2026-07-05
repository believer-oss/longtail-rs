//! Core on-disk formats and algorithms (chunking, hashing, compression) for the
//! pure-Rust longtail port. Sync, no tokio.
//!
//! Stage 2 provides the **format layer**: byte-cursor unaligned little-endian
//! codecs for [`VersionIndex`], [`StoreIndex`], [`BlockIndex`], and
//! [`StoredBlock`], the in-memory [`FileInfos`] structure (sort + name-blob
//! building), a [`Permissions`] type, [`StoreIndex::merge`], and the
//! [`FormatError`] type. The layer is `bytes in → structs out → bytes out`:
//! there is no file or storage I/O in the public API.
//!
//! ## Design notes
//!
//! - **PERF (owned structs).** Every format parses into owned Rust structs
//!   (`Vec<u64>`/`Vec<u32>`/`Vec<u16>`/`Vec<u8>`) and serialization is a pure
//!   function of the parsed content. This owned-struct representation was chosen
//!   for correctness and simplicity; revisit zero-copy views only if Stage 6
//!   benchmarking/profiling shows format parse/serialize as a genuine hot spot —
//!   do not pre-optimize now.
//! - **Round-trip fidelity beats normalization.** Structs preserve exactly what
//!   was parsed (raw `name_data` blob + offsets verbatim, all 16 permission
//!   bits, no sorting/dedup on read), so every committed fixture re-serializes
//!   byte-identically (compat gate ①).
//! - **Zero unsafe.** `#![forbid(unsafe_code)]` is enforced; all scalar access
//!   goes through the private byte cursor using `from_le_bytes`/`to_le_bytes`,
//!   never a `&[u64]` reinterpret cast (the formats are packed and `u64` arrays
//!   can land on 4-byte boundaries).
//! - **Malformed input never panics.** All size computations use checked
//!   arithmetic over attacker-controlled header counts and return
//!   [`FormatError`].
#![forbid(unsafe_code)]

mod cursor;

pub mod block;
pub mod error;
pub mod file_infos;
pub mod perms;
pub mod store_index;
pub mod version_index;

pub use block::{BlockIndex, StoredBlock};
pub use error::FormatError;
pub use file_infos::{FileEntry, FileInfos};
pub use perms::Permissions;
pub use store_index::StoreIndex;
pub use version_index::VersionIndex;

/// Current `.lvi` on-disk version (`LONGTAIL_VERSION(0,0,2)`).
pub const VERSION_INDEX_VERSION: u32 = version_index::VERSION;
/// Current `.lsi` on-disk version (`LONGTAIL_VERSION(1,0,0)`).
pub const STORE_INDEX_VERSION: u32 = store_index::VERSION;
