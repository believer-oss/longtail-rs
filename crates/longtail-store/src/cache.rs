//! [`CacheBlockStore`] — a two-tier local/remote block cache (port of
//! `cacheblockstore`).
//!
//! **Warm-cache compat:** existing launcher caches were
//! written by C's FSBlockStore as `chunks/<first-4-hex>/0x<hash16>.lrb`
//! (golongtail passes an empty extension → C's default `.lrb`). This store uses
//! the same block-path scheme with the `.lrb` extension and probes block files
//! directly. The stored bytes are byte-identical to the `.lsb` stored-block
//! output — only the extension differs (the passthrough property).
//!
//! The cache-dir `store.lsi` C maintains is treated as **advisory**: this store
//! never reads or trusts it (content queries forward to the remote), so a stale
//! cache index cannot cause a wrong answer. Deliberate compat choice, cheap to
//! change since caches are disposable.
//!
//! Composition: compression is outermost, so a
//! `CacheBlockStore` stores whatever bytes the remote returns — **compressed**
//! blocks. It never (de)compresses; that is [`crate::compress::CompressBlockStore`]'s
//! job one layer up.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use longtail_core::{StoreIndex, StoredBlock};

use crate::blob::{BlobClient, BlobStore, FsBlobStore};
use crate::block_store::{BlockStore, StatsSnapshot};
use crate::error::StoreError;

/// A local filesystem cache in front of a remote [`BlockStore`].
pub struct CacheBlockStore {
    cache_client: Arc<dyn BlobClient>,
    /// The cache root on disk (blocks live under `<root>/chunks/…`). Kept so the
    /// access-time/eviction paths can touch and enumerate files directly — the
    /// cache is always local fs.
    cache_root: PathBuf,
    /// Optional byte budget. `Some` enables access-time tracking (touch-on-hit)
    /// and a post-run LRU eviction sweep on [`close`](CacheBlockStore::close);
    /// `None` = unbounded (no tracking, zero extra overhead).
    size_limit: Option<u64>,
    remote: Arc<dyn BlockStore>,
}

impl CacheBlockStore {
    /// Open a cache rooted at `cache_dir` (blocks stored as `.lrb`) in front of
    /// `remote`. When `size_limit` is `Some(bytes)`, the cache tracks per-block
    /// access time (via file mtime) and evicts least-recently-used blocks down
    /// to the budget when the store closes.
    pub async fn new(
        cache_dir: impl AsRef<Path>,
        remote: Arc<dyn BlockStore>,
        size_limit: Option<u64>,
    ) -> Result<CacheBlockStore, StoreError> {
        let cache_root = cache_dir.as_ref().to_path_buf();
        let store = FsBlobStore::new(&cache_root, false);
        let cache_client: Arc<dyn BlobClient> = Arc::from(store.new_client().await?);
        Ok(CacheBlockStore {
            cache_client,
            cache_root,
            size_limit,
            remote,
        })
    }
}

/// Cache block path: `chunks/<first-4-hex>/0x<16-hex>.lrb` (C's FSBlockStore
/// default extension).
fn cache_block_path(block_hash: u64) -> String {
    let file_name = format!("0x{block_hash:016x}.lrb");
    let sub = &file_name[2..6];
    format!("chunks/{sub}/{file_name}")
}

#[async_trait]
impl BlockStore for CacheBlockStore {
    async fn put_stored_block(&self, block: StoredBlock) -> Result<(), StoreError> {
        // Write-through to the cache (skip-if-exists), then the remote.
        let key = cache_block_path(block.block_index.block_hash);
        let mut obj = self.cache_client.new_object(&key).await?;
        if !obj.exists().await? {
            let _ = obj.write(block.to_bytes().into()).await; // best-effort cache fill
        }
        self.remote.put_stored_block(block).await
    }

    async fn get_stored_block(&self, block_hash: u64) -> Result<StoredBlock, StoreError> {
        let key = cache_block_path(block_hash);
        // Probe the cache file directly.
        // Cache hit only when the file exists, parses, and matches the hash;
        // otherwise (corrupt/mismatched) fall through to the remote.
        let obj = self.cache_client.new_object(&key).await?;
        // Tracked so the write-back below can tell "not cached" from "cached but
        // unusable" — the two want opposite things from a skip-if-exists.
        let present = obj.exists().await.unwrap_or(false);
        if present
            && let Ok(data) = obj.read().await
            && let Ok(block) = StoredBlock::from_bytes(&data)
            && block.block_index.block_hash == block_hash
        {
            // Cache hit: stamp the file's mtime = now so it reads as "last
            // accessed now" for the LRU eviction sweep. Only when a budget is
            // set (unbounded caches keep zero overhead). Awaited so a later
            // close-time eviction sees the fresh mtime; a hit already saved a
            // remote fetch, so one blocking-pool hop is negligible. Best-effort.
            if self.size_limit.is_some() {
                let path = self.cache_root.join(&key);
                let _ = tokio::task::spawn_blocking(move || {
                    std::fs::OpenOptions::new()
                        .write(true)
                        .open(&path)
                        .and_then(|f| f.set_modified(SystemTime::now()))
                })
                .await;
            }
            return Ok(block);
        }
        // Miss → fetch from remote and write back to the cache.
        let block = self.remote.get_stored_block(block_hash).await?;
        let mut wb = self.cache_client.new_object(&key).await?;
        if present {
            // The file is there and did not parse, or named a different block.
            // Skip-if-exists would leave those bytes in place and re-fetch this
            // block on every read for the life of the cache — a permanent, silent
            // miss rather than the one-off a truncated download should be. The
            // write is atomic (temp + rename), so no delete is needed first.
            tracing::warn!(
                block_hash = format_args!("{block_hash:#018x}"),
                "replacing an unusable cache entry"
            );
            let _ = wb.write(block.to_bytes().into()).await;
        } else if !wb.exists().await.unwrap_or(false) {
            let _ = wb.write(block.to_bytes().into()).await; // best-effort write-back
        }
        Ok(block)
    }

    async fn preflight_get(&self, block_hashes: &[u64]) -> Result<(), StoreError> {
        // Only prefetch blocks not already cached (best-effort filter).
        let mut missing = Vec::new();
        for &h in block_hashes {
            let key = cache_block_path(h);
            let cached = match self.cache_client.new_object(&key).await {
                Ok(obj) => obj.exists().await.unwrap_or(false),
                Err(_) => false,
            };
            if !cached {
                missing.push(h);
            }
        }
        self.remote.preflight_get(&missing).await
    }

    async fn get_existing_content(
        &self,
        chunk_hashes: &[u64],
        min_block_usage_percent: u32,
    ) -> Result<StoreIndex, StoreError> {
        self.remote
            .get_existing_content(chunk_hashes, min_block_usage_percent)
            .await
    }

    async fn prune_blocks(&self, keep_block_hashes: &[u64]) -> Result<u32, StoreError> {
        self.remote.prune_blocks(keep_block_hashes).await
    }

    async fn flush(&self) -> Result<(), StoreError> {
        self.remote.flush().await
    }

    async fn close(&self) -> Result<(), StoreError> {
        self.remote.close().await?;
        // Post-run LRU eviction: after the store closes (all write-backs done),
        // trim the on-disk cache down to the byte budget, oldest-access-first.
        if let Some(max_bytes) = self.size_limit {
            let root = self.cache_root.clone();
            match tokio::task::spawn_blocking(move || evict_cache_dir(&root, max_bytes)).await {
                Ok(Ok(report)) => {
                    if report.deleted_files > 0 {
                        tracing::info!(
                            bytes_before = report.bytes_before,
                            bytes_after = report.bytes_after,
                            deleted_files = report.deleted_files,
                            deleted_bytes = report.deleted_bytes,
                            max_bytes,
                            "cache eviction trimmed the block cache to its size limit"
                        );
                    }
                }
                Ok(Err(e)) => tracing::warn!("cache eviction failed: {e}"),
                Err(e) => tracing::warn!("cache eviction task failed to join: {e}"),
            }
        }
        Ok(())
    }

    fn stats(&self) -> StatsSnapshot {
        self.remote.stats()
    }
}

/// The result of an [`evict_cache_dir`] sweep.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EvictionReport {
    /// Total `.lrb` bytes under `chunks/` before the sweep.
    pub bytes_before: u64,
    /// Number of block files deleted.
    pub deleted_files: u64,
    /// Total bytes freed by deletion.
    pub deleted_bytes: u64,
    /// Total `.lrb` bytes remaining after the sweep.
    pub bytes_after: u64,
}

/// One enumerated cache block file.
struct CacheFile {
    path: PathBuf,
    size: u64,
    /// Last-access time, taken from the file's mtime (which the cache stamps on
    /// every access when a size limit is set).
    mtime: SystemTime,
}

/// LRU-evict the local block cache under `cache_root/chunks` down to `max_bytes`.
///
/// Sums the size of every `.lrb` block file; if the total exceeds `max_bytes`,
/// deletes least-recently-used first (oldest mtime, ties broken by larger size)
/// until the total is within budget. Individual delete failures are logged and
/// skipped, not fatal. Only files under `chunks/` are considered — the advisory
/// cache-dir `store.lsi` is never touched.
///
/// Synchronous (filesystem I/O); callers on an async runtime should invoke it
/// via `spawn_blocking`. This is the port of the legacy FFI `get_with_cache`
/// prune, keyed on mtime-as-access-time rather than `max(mtime, atime)`.
pub fn evict_cache_dir(cache_root: &Path, max_bytes: u64) -> Result<EvictionReport, StoreError> {
    let chunks_root = cache_root.join("chunks");
    let mut files: Vec<CacheFile> = Vec::new();
    collect_cache_files(&chunks_root, &mut files);

    let bytes_before: u64 = files.iter().map(|f| f.size).sum();
    let mut report = EvictionReport {
        bytes_before,
        bytes_after: bytes_before,
        ..EvictionReport::default()
    };
    if bytes_before <= max_bytes {
        return Ok(report);
    }

    // Least-recently-used first: oldest mtime, ties → larger file first (frees
    // more per deletion, matching the FFI's timestamp-then-size ordering).
    files.sort_by(|a, b| a.mtime.cmp(&b.mtime).then_with(|| b.size.cmp(&a.size)));

    let mut current = bytes_before;
    for f in &files {
        if current <= max_bytes {
            break;
        }
        match std::fs::remove_file(&f.path) {
            Ok(()) => {
                current -= f.size;
                report.deleted_files += 1;
                report.deleted_bytes += f.size;
            }
            Err(e) => tracing::warn!("cache eviction: unable to delete {}: {e}", f.path.display()),
        }
    }
    report.bytes_after = current;
    Ok(report)
}

/// Recursively collect `.lrb`-scheme block files under `dir` (path, size,
/// mtime). Unreadable entries are skipped. A missing dir yields nothing.
fn collect_cache_files(dir: &Path, out: &mut Vec<CacheFile>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect_cache_files(&entry.path(), out);
        } else if meta.is_file() {
            // mtime is the access clock; fall back to UNIX_EPOCH (evict-first)
            // if the platform cannot report it.
            let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            out.push(CacheFile {
                path: entry.path(),
                size: meta.len(),
                mtime,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Write a cache block file of `size` bytes at `chunks/<sub>/0x…lrb` with a
    /// given mtime, and return its path.
    fn write_block(root: &Path, hash: u64, size: usize, mtime: SystemTime) -> PathBuf {
        let path = root.join(cache_block_path(hash));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![0u8; size]).unwrap();
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_modified(mtime).unwrap();
        path
    }

    #[test]
    fn evict_removes_least_recently_used_until_under_cap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        // Three 100-byte blocks, oldest → newest access time.
        let oldest = write_block(root, 0x1111_0000_0000_0001, 100, base);
        let middle = write_block(
            root,
            0x2222_0000_0000_0002,
            100,
            base + Duration::from_secs(10),
        );
        let newest = write_block(
            root,
            0x3333_0000_0000_0003,
            100,
            base + Duration::from_secs(20),
        );

        // Cap at 250 → must drop the single oldest (300 → 200 ≤ 250).
        let report = evict_cache_dir(root, 250).unwrap();
        assert_eq!(report.bytes_before, 300);
        assert_eq!(report.deleted_files, 1);
        assert_eq!(report.deleted_bytes, 100);
        assert_eq!(report.bytes_after, 200);
        assert!(!oldest.exists(), "LRU block should be evicted");
        assert!(middle.exists() && newest.exists(), "newer blocks kept");
    }

    #[test]
    fn evict_is_noop_when_under_cap() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let p = write_block(root, 0xabcd_0000_0000_0001, 500, base);
        let report = evict_cache_dir(root, 1_000).unwrap();
        assert_eq!(report.deleted_files, 0);
        assert_eq!(report.bytes_after, 500);
        assert!(p.exists());
    }

    #[test]
    fn evict_missing_dir_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        // No chunks/ subdir at all.
        let report = evict_cache_dir(dir.path(), 0).unwrap();
        assert_eq!(report, EvictionReport::default());
    }
}
