//! Async blob and block store backends (filesystem, S3, in-memory) and store
//! concurrency for the pure-Rust longtail port. Tokio-native.
//!
//! Layers (bottom → top):
//! - [`blob`] — the async `BlobStore`/`BlobClient`/`BlobObject` abstraction with
//!   mem/fs/S3 backends and generation-versioned writes (`blobStore_test.go`,
//!   `fsstore_test.go`).
//! - [`sync`] — store-index synchronization: the optimistic-locking flavor (fs +
//!   mem-with-locking) and the lockless shard/merge-on-read flavor (S3 +
//!   mem/fs-without-locking), plus [`sync::AccessType`] and the `chunks/…/.lsb`
//!   block path scheme.
//! - [`block_store`] — the async, dyn-dispatchable [`block_store::BlockStore`]
//!   trait + atomic [`block_store::BlockStoreStats`].
//! - [`remote`] — the [`remote::RemoteBlockStore`] actor (index-owner task,
//!   semaphore-bounded workers, coalescing prefetch with a byte budget, flush).
//! - [`cache`] / [`compress`] — the `.lrb` cache and rayon-bridged compression
//!   decorators.
//! - [`uri`] — the block-level URI dispatcher (`Compress(Cache(Remote(…)))`).
#![forbid(unsafe_code)]

pub mod blob;
pub mod block_store;
pub mod cache;
pub mod compress;
pub mod error;
pub mod remote;
pub mod sync;
pub mod uri;

pub use blob::{
    BlobClient, BlobObject, BlobProperties, BlobStore, FsBlobStore, MemBlobStore,
    create_blob_store_for_uri,
};
pub use block_store::{BlockStore, BlockStoreStats, StatsSnapshot};
pub use cache::CacheBlockStore;
pub use compress::CompressBlockStore;
pub use error::StoreError;
pub use remote::RemoteBlockStore;
pub use sync::{
    AccessType, add_to_remote_store_index, block_path, overwrite_remote_store_index,
    read_merged_store_index,
};
pub use uri::create_block_store_for_uri;

#[cfg(feature = "s3")]
pub use blob::{S3BlobStore, S3Options};
