//! A store shared with a concurrent golongtail writer, and a backend that can
//! under-report what it holds.
//!
//! Both have the same shape: a partial observation that looks like a complete
//! one. Neither is a byte-format question, so the byte-compatibility tests say
//! nothing about them, and both stay invisible until they surface much later as
//! a confusing failure — a download reporting "chunk not in the store index" for
//! content that is present, or a format error naming a file that is valid a
//! moment later.
//!
//! These need no S3 endpoint; the minio tests cover the happy path of mixed
//! writers, and these cover the unhappy ones.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use longtail_core::{BlockIndex, StoreIndex};
use longtail_store::blob::{BlobClient, BlobObject, BlobProperties, BlobStore, MemBlobStore};
use longtail_store::{StoreError, add_to_remote_store_index, read_merged_store_index};

fn sample_index() -> StoreIndex {
    StoreIndex::from_block_indexes(&[BlockIndex {
        block_hash: 0x0011_2233_4455_6677,
        hash_identifier: longtail_core::hash::BLAKE3_ID,
        tag: 0,
        chunk_hashes: vec![1, 2],
        chunk_sizes: vec![10, 20],
    }])
    .unwrap()
}

// --- a torn `store.lsi` from a concurrent writer ----------------------------

/// Truncates the first `tears_remaining` reads of any `.lsi` object to half its
/// length, standing in for golongtail's in-place `ioutil.WriteFile`
/// (`fsstore.go:266` — no temp+rename) observed mid-write by a reader holding no
/// lock, which is what a `ReadOnly` downsync does.
#[derive(Debug)]
struct TearingStore {
    inner: MemBlobStore,
    tears_remaining: Arc<AtomicUsize>,
}

#[async_trait]
impl BlobStore for TearingStore {
    async fn new_client(&self) -> Result<Box<dyn BlobClient>, StoreError> {
        Ok(Box::new(TearingClient {
            inner: self.inner.new_client().await?,
            tears_remaining: self.tears_remaining.clone(),
        }))
    }
    fn name(&self) -> String {
        "tearing".into()
    }
}

struct TearingClient {
    inner: Box<dyn BlobClient>,
    tears_remaining: Arc<AtomicUsize>,
}

#[async_trait]
impl BlobClient for TearingClient {
    async fn new_object(&self, path: &str) -> Result<Box<dyn BlobObject>, StoreError> {
        Ok(Box::new(TearingObject {
            inner: self.inner.new_object(path).await?,
            tears_remaining: self.tears_remaining.clone(),
            is_index: path.ends_with(".lsi"),
        }))
    }
    async fn get_objects(&self, prefix: &str) -> Result<Vec<BlobProperties>, StoreError> {
        self.inner.get_objects(prefix).await
    }
    fn supports_locking(&self) -> bool {
        self.inner.supports_locking()
    }
    fn name(&self) -> String {
        self.inner.name()
    }
}

struct TearingObject {
    inner: Box<dyn BlobObject>,
    tears_remaining: Arc<AtomicUsize>,
    is_index: bool,
}

#[async_trait]
impl BlobObject for TearingObject {
    async fn exists(&self) -> Result<bool, StoreError> {
        self.inner.exists().await
    }
    async fn lock_write_version(&mut self) -> Result<bool, StoreError> {
        self.inner.lock_write_version().await
    }
    async fn read(&self) -> Result<Vec<u8>, StoreError> {
        let data = self.inner.read().await?;
        if !self.is_index {
            return Ok(data);
        }
        loop {
            let n = self.tears_remaining.load(Ordering::SeqCst);
            if n == 0 {
                return Ok(data);
            }
            if self
                .tears_remaining
                .compare_exchange(n, n - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                // A prefix of a valid index: passes the `size > 0` filter,
                // fails `StoreIndex::from_bytes`.
                return Ok(data[..data.len() / 2].to_vec());
            }
        }
    }
    async fn write(&mut self, data: bytes::Bytes) -> Result<bool, StoreError> {
        self.inner.write(data).await
    }
    async fn delete(&mut self) -> Result<(), StoreError> {
        self.inner.delete().await
    }
    fn name(&self) -> String {
        self.inner.name()
    }
}

/// A half-written store index must be retried, not treated as terminal. Parsing
/// outside the read retry ladder fails the whole operation with a format error
/// and then succeeds on the next invocation, which presents as a flake rather
/// than as the race it is.
#[tokio::test]
async fn torn_store_index_read_is_retried_not_fatal() {
    let mem = MemBlobStore::new("", false);
    let seed = mem.new_client().await.unwrap();
    add_to_remote_store_index(&*seed, &sample_index())
        .await
        .expect("seed the store index");

    let tearing = TearingStore {
        inner: mem,
        tears_remaining: Arc::new(AtomicUsize::new(1)),
    };
    let client = tearing.new_client().await.unwrap();
    let merged = read_merged_store_index(&*client)
        .await
        .expect("a torn index must be retried, not fatal");
    assert_eq!(merged.block_hashes, sample_index().block_hashes);
}

/// The ladder is finite: a genuinely corrupt index still fails, with the format
/// error rather than a timeout. Without this, "retry on parse failure" could
/// become "hang on corruption" unnoticed.
#[tokio::test]
async fn permanently_corrupt_store_index_still_fails() {
    let mem = MemBlobStore::new("", false);
    let seed = mem.new_client().await.unwrap();
    add_to_remote_store_index(&*seed, &sample_index())
        .await
        .expect("seed the store index");

    let tearing = TearingStore {
        inner: mem,
        tears_remaining: Arc::new(AtomicUsize::new(usize::MAX)),
    };
    let client = tearing.new_client().await.unwrap();
    let err = read_merged_store_index(&*client)
        .await
        .expect_err("a permanently corrupt index must fail");
    assert!(
        matches!(err, StoreError::Format(_)),
        "expected the format error to survive the ladder, got: {err:?}"
    );
}

// --- a listing must not silently under-report -------------------------------

/// An unreadable directory inside the store must fail the listing rather than
/// yield a short one.
///
/// The listing feeds `get_store_store_indexes`, so an invisible `store_*.lsi`
/// shard narrows the merged store index. On the `add` path that self-heals, but
/// on `try_overwrite` it does not, and prune then deletes the blocks the
/// invisible shard referenced. Go's `filepath.Walk` swallows these errors; that
/// is Go's bug rather than a compatibility requirement, because a listing is not
/// a byte format.
#[cfg(unix)]
#[tokio::test]
async fn unreadable_directory_fails_the_listing_instead_of_shortening_it() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("store");
    std::fs::create_dir_all(root.join("chunks/aaaa")).unwrap();
    std::fs::write(root.join("store_0000.lsi"), b"not-parsed-here").unwrap();

    let blocked = root.join("chunks/aaaa");
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root ignores the mode bits; skip rather than assert something untrue.
    if std::fs::read_dir(&blocked).is_ok() {
        eprintln!("skipping: this user can read a 0o000 directory (running as root?)");
        return;
    }

    let store = longtail_store::FsBlobStore::new(root, false);
    let client = store.new_client().await.unwrap();
    let err = client
        .get_objects("")
        .await
        .expect_err("an unreadable directory must fail the listing");
    assert!(
        matches!(err, StoreError::Io { .. }),
        "expected an io error naming the directory, got: {err:?}"
    );

    // Restore so the tempdir can be cleaned up.
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The counterpart: an absent store root is an empty store, not an error —
/// otherwise every "create the store on first write" path breaks.
#[tokio::test]
async fn absent_store_root_still_lists_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let store = longtail_store::FsBlobStore::new(tmp.path().join("does-not-exist"), false);
    let client = store.new_client().await.unwrap();
    let listed = client
        .get_objects("")
        .await
        .expect("an absent store root is an empty store");
    assert!(listed.is_empty());
}

// --- a store cannot choose this process's memory use -------------------------

/// An object larger than the read ceiling is refused instead of being read into
/// memory.
///
/// A blob's length is whatever the store says it is, and the backends read one
/// in a single allocation. The prefetch byte budget does not bound it: that is
/// computed from the store index's *declared* chunk sizes, which have no
/// relationship to the object actually served.
#[tokio::test]
async fn an_oversized_object_is_refused_rather_than_read() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::write(root.join("big"), vec![0u8; 4096]).unwrap();

    let store = longtail_store::FsBlobStore::new(&root, false).with_max_read_bytes(1024);
    let client = store.new_client().await.unwrap();
    let obj = client.new_object("big").await.unwrap();

    let err = obj
        .read()
        .await
        .expect_err("4 KiB must exceed a 1 KiB ceiling");
    assert!(
        err.to_string().contains("ceiling"),
        "expected the ceiling refusal, got: {err}"
    );
}

/// The ceiling is a limit, not a fixed size: anything at or under it still
/// reads. Without this, a ceiling of zero would satisfy the test above.
#[tokio::test]
async fn an_object_within_the_ceiling_still_reads() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::write(root.join("ok"), vec![7u8; 1024]).unwrap();

    let store = longtail_store::FsBlobStore::new(&root, false).with_max_read_bytes(1024);
    let client = store.new_client().await.unwrap();
    let data = client
        .new_object("ok")
        .await
        .unwrap()
        .read()
        .await
        .expect("exactly at the ceiling must be accepted");
    assert_eq!(data.len(), 1024);
}

/// A deployment that genuinely writes larger blocks can raise the ceiling — the
/// escape hatch has to work, or the cap becomes a compatibility break for stores
/// built with a large `--target-block-size`.
#[tokio::test]
async fn the_ceiling_can_be_raised_for_stores_that_need_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::write(root.join("big"), vec![0u8; 4096]).unwrap();

    let store = longtail_store::FsBlobStore::new(&root, false).with_max_read_bytes(8192);
    let client = store.new_client().await.unwrap();
    let data = client
        .new_object("big")
        .await
        .unwrap()
        .read()
        .await
        .expect("a raised ceiling must admit the object");
    assert_eq!(data.len(), 4096);
}
