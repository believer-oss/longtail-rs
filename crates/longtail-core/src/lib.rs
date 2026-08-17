//! Core on-disk formats and algorithms (chunking, hashing, compression) for the
//! pure-Rust longtail port. Sync, no tokio.
//!
//! The **format layer** provides byte-cursor unaligned little-endian
//! codecs for [`VersionIndex`], [`StoreIndex`], [`BlockIndex`], and
//! [`StoredBlock`], the in-memory [`FileInfos`] structure (sort + name-blob
//! building), a [`Permissions`] type, [`StoreIndex::merge`], and the
//! [`FormatError`] type. The layer is `bytes in → structs out → bytes out`:
//! there is no file or storage I/O in the public API.
//!
//! The **algorithm layer** adds:
//! - [`hash`] — the [`Hash`] trait + [`Blake3`]/[`Blake2s`] + meow
//!   parse-without-verify + the ID registry ([`hash::hasher`]).
//! - [`compress`] — the [`Compressor`] trait + zstd/lz4/brotli codecs + the
//!   family-dispatch registry ([`compressor_for`]) + the compressed-block
//!   payload framing codec ([`compress::decode_block_payload`] /
//!   [`compress::encode_block_payload`]).
//! - [`chunker`] — the [`Chunker`] trait + the [`HpcdcChunker`] exact port
//!   (streaming-canonical, with a labeled [`SeedMode::Buffer`] variant) + the
//!   composed `(offset, size, hash)` API; FastCDC behind the `fastcdc` feature.
//!
//! ## Design notes
//!
//! - **PERF (owned structs).** Every format parses into owned Rust structs
//!   (`Vec<u64>`/`Vec<u32>`/`Vec<u16>`/`Vec<u8>`) and serialization is a pure
//!   function of the parsed content. This owned-struct representation was chosen
//!   for correctness and simplicity. Benchmarking settled the revisit
//!   trigger: the codecs run at 4–26 GiB/s, so realistic `.lvi`/`.lsi`
//!   parse+serialize is tens of microseconds (~0.02 % of an e2e downsync wall) —
//!   not a hot spot. Verdict: keep owned structs, do not adopt zero-copy views
//!   (measured: parse/serialize is not a hot spot — see `docs/rust-port.md` §Performance).
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
pub mod build;
pub mod chunker;
pub mod compress;
pub mod diff;
pub mod error;
pub mod file_infos;
pub mod hash;
pub mod pack;
pub mod perms;
pub mod store_index;
pub mod validate;
pub mod version_index;

pub use block::{BlockIndex, StoredBlock};
pub use build::{
    MergeVersionError, assemble_version_index, chunk_asset, create_version_index,
    merge_version_index,
};
pub use chunker::{ChunkHash, ChunkSpan, Chunker, ChunkerError, HpcdcChunker, SeedMode};
pub use compress::{CompressError, Compressor, compressor_for};
pub use diff::{VersionDiff, create_version_diff, get_required_chunk_hashes};
pub use error::FormatError;
pub use file_infos::{FileEntry, FileInfos};
pub use hash::{Blake2s, Blake3, Hash, HashError};
pub use pack::{block_hash, create_missing_content, create_store_index};
pub use perms::Permissions;
pub use store_index::StoreIndex;
pub use validate::{ValidateError, validate_store};
pub use version_index::VersionIndex;

#[cfg(feature = "fastcdc")]
pub use chunker::FastCdcChunker;

/// Current `.lvi` on-disk version (`LONGTAIL_VERSION(0,0,2)`).
pub const VERSION_INDEX_VERSION: u32 = version_index::VERSION;
/// Current `.lsi` on-disk version (`LONGTAIL_VERSION(1,0,0)`).
pub const STORE_INDEX_VERSION: u32 = store_index::VERSION;
