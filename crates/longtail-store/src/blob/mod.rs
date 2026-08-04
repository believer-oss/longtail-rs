//! The async blob abstraction (`longtailstorelib`'s `BlobStore` / `BlobClient`
//! / `BlobObject`), reshaped Rust-idiomatically but preserving the semantics the
//! spec stubs pin down (`blobStore_test.go`, `fsstore_test.go`):
//! list-by-prefix, read/write/delete, exists, and generation-versioned
//! (optimistic-locking) writes.
//!
//! Shape vs. Go:
//! - `new_client`/`new_object` are `async` (the S3 backend awaits the SDK).
//! - `write`/`delete`/`lock_write_version` take `&mut self` on the object: the
//!   generation snapshot from `lock_write_version` lives on the object, exactly
//!   like Go's `lockedGeneration`/`metageneration` field.
//! - `write` returns `bool`: `true` = written, `false` = the conditional write
//!   lost its generation CAS (no error) — mirrors Go's `(ok, nil)`.

use async_trait::async_trait;
use bytes::Bytes;

use crate::error::StoreError;

mod fs;
mod mem;
#[cfg(feature = "s3")]
mod s3;

pub use fs::FsBlobStore;
pub use mem::MemBlobStore;
#[cfg(feature = "s3")]
pub use s3::{S3BlobStore, S3Options};

/// One listed object: its store-relative name and byte size (`BlobProperties`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobProperties {
    pub size: u64,
    pub name: String,
}

/// A blob store: a factory for [`BlobClient`]s. Cheap to clone/share (`Arc`ed by
/// the block store).
#[async_trait]
pub trait BlobStore: Send + Sync + std::fmt::Debug {
    /// Open a client. Each client owns its backend handle (Go: one blob client
    /// per remote worker); clients are created per block-store worker.
    async fn new_client(&self) -> Result<Box<dyn BlobClient>, StoreError>;

    /// Human-readable identifier (Go `String()`), e.g. `fsblob://path` or
    /// `s3://bucket/prefix`.
    fn name(&self) -> String;
}

/// A blob client: enumerate + open objects.
#[async_trait]
pub trait BlobClient: Send + Sync {
    /// A handle to an object at `path` (relative to the store prefix). Does not
    /// touch the backend.
    async fn new_object(&self, path: &str) -> Result<Box<dyn BlobObject>, StoreError>;

    /// List every object whose store-relative name starts with `prefix`
    /// (`GetObjects`). Empty store → empty vec (never an error). fs backend
    /// filters out its `._lck` lock files (fsstore.go:82).
    async fn get_objects(&self, prefix: &str) -> Result<Vec<BlobProperties>, StoreError>;

    /// Whether this backend honors the generation/metageneration optimistic
    /// lock. S3 = hardwired `false` (s3Store.go:106); mem/fs take a constructor
    /// flag (`NewMemBlobStore(_, bool)` / `NewFSBlobStore(_, bool)`).
    fn supports_locking(&self) -> bool;

    /// Human-readable identifier (Go `String()`).
    fn name(&self) -> String;
}

/// A single object handle. `lock_write_version` snapshots the generation onto
/// this handle; a subsequent `write`/`delete` is then conditional on that
/// snapshot. A handle that never locked writes/deletes unconditionally
/// (fsstore.go:255, :271, :291; memblobstore.go:115).
#[async_trait]
pub trait BlobObject: Send + Sync {
    /// `true`/`false` for present/absent; error only on real backend failure.
    async fn exists(&self) -> Result<bool, StoreError>;

    /// Snapshot the current generation for a later conditional write/delete.
    /// Returns whether the object currently exists. Errors on a lockless backend
    /// (fsstore.go:210-213) — but S3's object returns `(false, Ok)` since S3
    /// never locks (s3Store.go:141).
    async fn lock_write_version(&mut self) -> Result<bool, StoreError>;

    /// Read the object's bytes. Missing object → [`StoreError::NotFound`].
    async fn read(&self) -> Result<Vec<u8>, StoreError>;

    /// Write bytes. Returns `true` on success, `false` if a generation-locked
    /// write was prevented by a concurrent change (no error). Unlocked handles
    /// always return `true` on success.
    ///
    /// Takes owned [`Bytes`] so the payload moves into the backend body (the S3
    /// `ByteStream`, the fs `spawn_blocking` closure) without a copy; a caller
    /// that must write the same buffer more than once (the put-retry loop)
    /// reclones it for O(1) rather than copying.
    async fn write(&mut self, data: Bytes) -> Result<bool, StoreError>;

    /// Delete the object. A generation-locked delete errors on mismatch
    /// ([`StoreError::GenerationMismatch`]); an unlocked delete is unconditional.
    async fn delete(&mut self) -> Result<(), StoreError>;

    /// Human-readable identifier (Go `String()`).
    fn name(&self) -> String;
}

/// Blob-level URI dispatch — `longtailstorelib.CreateBlobStoreForURI`
/// (blobStore.go:65-91). Note this is the **blob-level** dispatcher the
/// `create_*_blob_store_from_uri` spec stubs exercise; the **block-level**
/// dispatcher lives in [`crate::uri::create_block_store_for_uri`].
///
/// - `fsblob://path`, `file://path`, and bare paths → [`FsBlobStore`].
/// - `s3://bucket/prefix` → [`S3BlobStore`] (feature `s3`).
/// - `gs://…` → [`StoreError::NotSupported`] (GCS is out of scope; a
///   deliberate divergence from Go, which constructs a GCS store).
/// - `abfs://`/`abfss://` → [`StoreError::NotSupported`] (Azure, matching Go's
///   "not yet implemented" error).
pub fn create_blob_store_for_uri(uri: &str) -> Result<Box<dyn BlobStore>, StoreError> {
    // Special-case: filepaths do not always parse as URLs (Go checks fsblob://
    // and UNC prefixes before url.Parse).
    if let Some(rest) = uri.strip_prefix("fsblob://") {
        return Ok(Box::new(FsBlobStore::new(rest, false)));
    }
    // Windows UNC prefix `\\?\` — treat as a filesystem path.
    if uri.starts_with("\\\\?\\") || uri.starts_with('\\') {
        return Ok(Box::new(FsBlobStore::new(uri, false)));
    }

    // Scheme detection: `scheme://rest`. Anything without a recognized scheme is
    // a bare filesystem path (incl. Windows `c:\...`).
    if let Some((scheme, rest)) = split_scheme(uri) {
        match scheme {
            "gs" => {
                return Err(StoreError::NotSupported(format!(
                    "gs:// (GCS) blob stores are not supported; uri `{uri}`"
                )));
            }
            "abfs" => {
                return Err(StoreError::NotSupported(
                    "azure Gen1 storage not implemented".into(),
                ));
            }
            "abfss" => {
                return Err(StoreError::NotSupported(
                    "azure Gen2 storage not implemented".into(),
                ));
            }
            "file" => {
                // `file://host/path` → Go joins Host+Path. `rest` here is
                // everything after `file://`.
                return Ok(Box::new(FsBlobStore::new(rest, false)));
            }
            "s3" => {
                #[cfg(feature = "s3")]
                {
                    return Ok(Box::new(S3BlobStore::from_uri(uri)?));
                }
                #[cfg(not(feature = "s3"))]
                {
                    return Err(StoreError::NotSupported(
                        "s3:// support was compiled out (feature `s3`)".into(),
                    ));
                }
            }
            _ => {
                // Unknown scheme (Go falls through to a filesystem store; we keep
                // the fs fallback for `c:\...`-style paths but reject genuine
                // unknown schemes to surface typos as the spec intends).
                if scheme.len() == 1 {
                    // Windows drive letter like `c:` — a path, not a scheme.
                    return Ok(Box::new(FsBlobStore::new(uri, false)));
                }
                return Err(StoreError::InvalidUri {
                    uri: uri.to_string(),
                    reason: format!("unknown scheme `{scheme}`"),
                });
            }
        }
    }

    // No scheme → filesystem path.
    Ok(Box::new(FsBlobStore::new(uri, false)))
}

/// Split `scheme://rest`. Returns `None` if there is no `://` separator. The
/// scheme is lowercased for matching. A bare `c:\path` has no `//` so returns
/// `None` (treated as a path).
pub(crate) fn split_scheme(uri: &str) -> Option<(&str, &str)> {
    let idx = uri.find("://")?;
    let scheme = &uri[..idx];
    if scheme.is_empty() || !scheme.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    Some((scheme, &uri[idx + 3..]))
}
