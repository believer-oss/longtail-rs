//! Store-index synchronization — the compat-critical merge logic, both flavors.
//!
//! Ports the store-index machinery of `remotestore.go`:
//! - **Locking flavor** (fs, and mem with locking): the blob-object scheme —
//!   `LockWriteVersion` on `store.lsi` (flock `store.lsi._lck` + `store.lsi.gen`
//!   generation sidecar) → read → [`StoreIndex::merge`] → conditional write →
//!   retry on generation conflict (`tryAddRemoteStoreIndexWithLocking`,
//!   remotestore.go:1113-1192).
//! - **Lockless flavor** (S3, and mem/fs without locking): write
//!   `store_<sha256hex-of-serialized-bytes>.lsi` shards; readers list prefix
//!   `store`, keep `.lsi` suffixes, and [`StoreIndex::merge`] everything
//!   (canonical `store.lsi` + shards). The write path merges all discovered
//!   items, writes ONE consolidated shard, short-circuits if that key already
//!   exists, then best-effort deletes the superseded items
//!   (`tryWriteRemoteStoreIndex`, remotestore.go:1194-1258).
//!
//! ⚠ The `fs_store_index_sync_with_locking` spec-stub doc-comment attributes the
//! fs lock to `store.lsi.sync`; that is **C's FSBlockStore** lock, a different
//! component. The Go mechanism the cited test actually uses is the
//! `store.lsi._lck`/`store.lsi.gen` blob-object scheme implemented here.
//!
//! Shard naming is **byte-defined**: `sha256` over the exact serialized index
//! bytes (`store_%x.lsi`, remotestore.go:1213). Because [`StoreIndex::merge`] /
//! [`StoreIndex::from_block_indexes`] are byte-identical to C, these names match
//! the committed `fixtures/stores/sharded/` shards.

use std::time::Duration;

use longtail_core::{BlockIndex, StoreIndex, StoredBlock};
use sha2::{Digest, Sha256};

use crate::blob::{BlobClient, BlobStore};
use crate::error::StoreError;

/// How the store index is accessed (`remotestore.go:26-33`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    /// Read/write with a forced rebuild of the store index by scanning `.lsb`
    /// blocks, then writing the rebuilt index back.
    Init,
    /// Read/write; the store index is read (not rebuilt).
    ReadWrite,
    /// Read-only; puts/prune are rejected.
    ReadOnly,
}

/// The read/list retry ladder (`ReadBlobWithRetry`, longtailutils.go:426;
/// `getStoreStoreIndexes`, remotestore.go:1677). Not-found short-circuits before
/// this ladder is consulted.
pub(crate) const READ_RETRY_DELAYS: [Duration; 6] = [
    Duration::from_millis(0),
    Duration::from_millis(100),
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_millis(1000),
    Duration::from_millis(2000),
];

/// The block path scheme `chunks/<first-4-hex>/0x<16-hex>.lsb`
/// (`getBlockPath`, remotestore.go:1941; `longtail_fsblockstore.c` GetBlockPath).
/// `base` is normally `"chunks"`.
pub fn block_path(base: &str, block_hash: u64) -> String {
    let file_name = format!("0x{block_hash:016x}.lsb");
    let sub = &file_name[2..6];
    format!("{base}/{sub}/{file_name}")
}

/// `ReadBlobWithRetry` (longtailutils.go:401): exists-precheck (not-found → no
/// retry), then read with the fixed ladder; a mid-loop not-found also
/// short-circuits. Returns the bytes and the retry count (for stats).
pub(crate) async fn read_blob_with_retry(
    client: &dyn BlobClient,
    key: &str,
) -> Result<(Vec<u8>, u32), StoreError> {
    let obj = client.new_object(key).await?;
    if !obj.exists().await? {
        return Err(StoreError::NotFound(key.to_string()));
    }
    let mut retry_count: u32 = 0;
    loop {
        match obj.read().await {
            Ok(data) => return Ok((data, retry_count)),
            Err(e) if e.is_not_found() => return Err(e),
            Err(e) => {
                if retry_count as usize == READ_RETRY_DELAYS.len() {
                    return Err(e);
                }
                sleep(READ_RETRY_DELAYS[retry_count as usize]).await;
                retry_count += 1;
            }
        }
    }
}

async fn sleep(d: Duration) {
    if d.is_zero() {
        // Yield so paused-time tests still make progress without advancing time.
        tokio::task::yield_now().await;
    } else {
        tokio::time::sleep(d).await;
    }
}

/// The shard key for a store index: `store_<sha256hex>.lsi` over its exact
/// serialized bytes.
pub(crate) fn shard_key(index_bytes: &[u8]) -> String {
    let digest = Sha256::digest(index_bytes);
    format!("store_{digest:x}.lsi")
}

/// `getStoreStoreIndexes` (remotestore.go:1665): list prefix `store` with the
/// read retry ladder, keep non-empty `.lsi` objects. Not-found → empty.
async fn get_store_store_indexes(client: &dyn BlobClient) -> Result<Vec<String>, StoreError> {
    let mut retry_count = 0usize;
    let blobs = loop {
        match client.get_objects("store").await {
            Ok(b) => break b,
            Err(e) if e.is_not_found() => return Ok(Vec::new()),
            Err(e) => {
                if retry_count == READ_RETRY_DELAYS.len() {
                    return Err(e);
                }
                sleep(READ_RETRY_DELAYS[retry_count]).await;
                retry_count += 1;
            }
        }
    };
    let mut items: Vec<String> = blobs
        .into_iter()
        .filter(|b| b.size > 0 && b.name.ends_with(".lsi"))
        .map(|b| b.name)
        .collect();
    // Deterministic merge order (Go's is list-order, which for S3 is
    // lexicographic and for the map backend is nondeterministic). Sorting keeps
    // the merged bytes stable regardless of backend listing order.
    items.sort();
    Ok(items)
}

/// `readStoreStoreIndexFromPath` (remotestore.go:1637): read + parse one shard.
async fn read_store_index_from_path(
    client: &dyn BlobClient,
    key: &str,
) -> Result<(StoreIndex, u32), StoreError> {
    let (data, retries) = read_blob_with_retry(client, key).await?;
    if data.is_empty() {
        return Err(StoreError::NotFound(key.to_string()));
    }
    Ok((StoreIndex::from_bytes(&data)?, retries))
}

/// `mergeStoreIndexItems` (remotestore.go:1707). Returns `(index, used_items)`;
/// `None` signals a mid-scan change (a listed item vanished) — the caller
/// rescans.
async fn merge_store_index_items(
    client: &dyn BlobClient,
    items: &[String],
    retry_counter: &mut u32,
) -> Result<Option<(StoreIndex, Vec<String>)>, StoreError> {
    let mut acc: Option<StoreIndex> = None;
    let mut used = Vec::new();
    for item in items {
        let parsed = read_store_index_from_path(client, item).await;
        let (tmp, retries) = match parsed {
            Ok(v) => v,
            Err(e) if e.is_not_found() => return Ok(None), // vanished → rescan
            Err(e) => return Err(e),
        };
        *retry_counter += retries;
        acc = Some(match acc {
            None => tmp,
            Some(existing) => existing.merge(&tmp)?,
        });
        used.push(item.clone());
    }
    Ok(acc.map(|idx| (idx, used)))
}

/// `readStoreStoreIndexWithItems` (remotestore.go:1750). Lists + merges all
/// store-index items; rescans if items change mid-merge; empty store → an empty
/// store index. Returns `(index, used_items, total_retries)`.
pub(crate) async fn read_store_store_index_with_items(
    client: &dyn BlobClient,
) -> Result<(StoreIndex, Vec<String>, u32), StoreError> {
    let mut retries = 0u32;
    loop {
        let items = get_store_store_indexes(client).await?;
        if items.is_empty() {
            return Ok((StoreIndex::empty(0), Vec::new(), retries));
        }
        match merge_store_index_items(client, &items, &mut retries).await? {
            Some((index, used)) => {
                if used.is_empty() {
                    continue; // items changed mid-scan → rescan
                }
                return Ok((index, used, retries));
            }
            None => continue, // a listed item vanished → rescan
        }
    }
}

/// `tryAddRemoteStoreIndexWithLocking` (remotestore.go:1113). Returns
/// `(ok, new_index)`; `ok == false` (no error) means the generation CAS lost —
/// the caller retries.
async fn try_add_with_locking(
    client: &dyn BlobClient,
    add: &StoreIndex,
) -> Result<(bool, Option<StoreIndex>), StoreError> {
    let key = "store.lsi";
    let mut obj = client.new_object(key).await?;
    let exists = obj.lock_write_version().await?;
    if exists {
        let existing = obj.read().await;
        let existing = match existing {
            Ok(d) => d,
            Err(e) if e.is_not_found() => return Ok((false, None)),
            Err(e) => return Err(e),
        };
        let remote = StoreIndex::from_bytes(&existing)?;
        drop(existing); // the serialized source bytes are done once parsed
        let merged = remote.merge(add)?;
        drop(remote); // the pre-merge union is not needed once `merged` exists
        let bytes = merged.to_bytes();
        let ok = obj.write(bytes.into()).await?;
        if !ok {
            return Ok((false, None));
        }
        Ok((true, Some(merged)))
    } else {
        let bytes = add.to_bytes();
        let ok = obj.write(bytes.into()).await?;
        Ok((ok, None))
    }
}

/// `tryWriteRemoteStoreIndex` (remotestore.go:1194): write ONE consolidated
/// shard `store_<sha256>.lsi`; skip if that key already exists (among the listed
/// items or on the backend); on success best-effort delete the superseded
/// items. Returns whether the store now holds `index`.
async fn try_write_shard(
    client: &dyn BlobClient,
    index: &StoreIndex,
    existing_items: &[String],
) -> Result<bool, StoreError> {
    let bytes = index.to_bytes();
    let key = shard_key(&bytes);

    // Already present among the items we merged from → nothing to do.
    if existing_items.iter().any(|i| i == &key) {
        return Ok(true);
    }

    let mut obj = client.new_object(&key).await?;
    if obj.exists().await? {
        return Ok(false); // a concurrent writer produced the identical shard
    }
    let ok = obj.write(bytes.into()).await?;
    if !ok {
        return Ok(false);
    }

    // Best-effort delete of the superseded shards (errors ignored, matching Go).
    for item in existing_items {
        if item == &key {
            continue;
        }
        if let Ok(mut old) = client.new_object(item).await {
            let _ = old.delete().await;
        }
    }
    Ok(true)
}

/// `tryAddRemoteStoreIndex` (remotestore.go:1260): dispatch locking vs lockless.
async fn try_add(
    client: &dyn BlobClient,
    add: &StoreIndex,
) -> Result<(bool, Option<StoreIndex>), StoreError> {
    if client.supports_locking() {
        return try_add_with_locking(client, add).await;
    }
    let (existing, items, _retries) = read_store_store_index_with_items(client).await?;
    // `read_store_store_index_with_items` always yields a valid (possibly empty)
    // index, so — matching Go's reachable branch — we always merge then write.
    let merged = existing.merge(add)?;
    // `existing` (a full union copy) is no longer needed once `merged` exists —
    // free it before serialization so the flush peak is `merged` + the `to_bytes`
    // buffer, not `existing` + `merged` + buffer.
    drop(existing);
    let ok = try_write_shard(client, &merged, &items).await?;
    Ok((ok, Some(merged)))
}

/// `addToRemoteStoreIndex` (remotestore.go:1299): retry `try_add` until it wins;
/// hard errors are tolerated up to 3 times (then propagated). A lost CAS /
/// shard race retries immediately (no sleep — the lock/CAS serializes writers).
///
/// Public: this is the store-index merge primitive that the download/upload
/// paths (and the
/// concurrent-writer chaos test) drive directly. Returns the new consolidated
/// index when one was produced (`None` for the locking flavor's first write).
pub async fn add_to_remote_store_index(
    client: &dyn BlobClient,
    add: &StoreIndex,
) -> Result<Option<StoreIndex>, StoreError> {
    let mut error_retries = 0u32;
    loop {
        match try_add(client, add).await {
            Ok((true, new_index)) => return Ok(new_index),
            Ok((false, _)) => {} // conflict → retry
            Err(e) => {
                error_retries += 1;
                if error_retries == 3 {
                    return Err(e);
                }
            }
        }
    }
}

/// Read the current merged store index (canonical `store.lsi` + all shards).
/// Public convenience over [`read_store_store_index_with_items`] for the
/// download/upload paths
/// and the chaos test's convergence check.
pub async fn read_merged_store_index(client: &dyn BlobClient) -> Result<StoreIndex, StoreError> {
    let (index, _used, _retries) = read_store_store_index_with_items(client).await?;
    Ok(index)
}

/// `tryOverwriteRemoteStoreIndex` (remotestore.go:1432) — make `index` the sole
/// authoritative store index (used by prune, which must REPLACE not merge).
/// - Locking flavor: lock `store.lsi`, write it unconditionally.
/// - Lockless flavor: write the `store_<sha256>.lsi` shard for `index` (if not
///   already present), then delete every *other* `store*.lsi` item.
async fn try_overwrite(client: &dyn BlobClient, index: &StoreIndex) -> Result<bool, StoreError> {
    if client.supports_locking() {
        let mut obj = client.new_object("store.lsi").await?;
        obj.lock_write_version().await?;
        return obj.write(index.to_bytes().into()).await;
    }
    let items = get_store_store_indexes(client).await?;
    let bytes = index.to_bytes();
    let key = shard_key(&bytes);
    if !items.iter().any(|i| i == &key) {
        let mut obj = client.new_object(&key).await?;
        if !obj.exists().await? {
            let ok = obj.write(bytes.into()).await?;
            if !ok {
                return Ok(false);
            }
        }
    }
    for item in &items {
        if item == &key {
            continue;
        }
        if let Ok(mut old) = client.new_object(item).await {
            let _ = old.delete().await;
        }
    }
    Ok(true)
}

/// `tryOverwriteStoreIndexWithRetry` (remotestore.go:1460): retry [`try_overwrite`]
/// on a lost CAS; hard errors tolerated up to 3 times. Public: the prune path
/// calls this to overwrite the store index BEFORE deleting blocks.
pub async fn overwrite_remote_store_index(
    client: &dyn BlobClient,
    index: &StoreIndex,
) -> Result<(), StoreError> {
    let mut error_retries = 0u32;
    loop {
        match try_overwrite(client, index).await {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(e) => {
                error_retries += 1;
                if error_retries == 3 {
                    return Err(e);
                }
            }
        }
    }
}

/// `getStoreIndexFromBlocks` (remotestore.go:1482): read each `.lsb`, parse its
/// block index, keep only blocks whose stored hash matches their path, and
/// build one store index. **Rust-assembled order is sorted by block hash** for
/// determinism (golongtail's order is map/parallel nondeterminism, so there is
/// no Go order to match).
async fn get_store_index_from_blocks(
    client: &dyn BlobClient,
    block_keys: &[String],
) -> Result<StoreIndex, StoreError> {
    let mut blocks: Vec<BlockIndex> = Vec::with_capacity(block_keys.len());
    for key in block_keys {
        let (data, _retries) = match read_blob_with_retry(client, key).await {
            Ok(v) => v,
            Err(e) if e.is_not_found() => continue,
            Err(e) => return Err(e),
        };
        // Parse just the block index (the payload tail is ignored).
        let stored = match StoredBlock::from_bytes(&data) {
            Ok(s) => s,
            Err(_) => continue, // unparseable → skip (Go skips on parse error)
        };
        let bi = stored.block_index;
        let expected = block_path("chunks", bi.block_hash);
        if normalize(key) == expected {
            blocks.push(bi);
        }
        // else: name does not match content hash → skip (remotestore.go:1561).
    }
    blocks.sort_by_key(|b| b.block_hash);
    Ok(StoreIndex::from_block_indexes(&blocks)?)
}

fn normalize(p: &str) -> String {
    p.replace('\\', "/").replace("//", "/")
}

/// `buildStoreIndexFromStoreBlocks` (remotestore.go:1605): list every non-empty
/// `.lsb`, then rebuild.
async fn build_store_index_from_store_blocks(
    client: &dyn BlobClient,
) -> Result<StoreIndex, StoreError> {
    let blobs = client.get_objects("").await?;
    let mut items: Vec<String> = blobs
        .into_iter()
        .filter(|b| b.size > 0 && b.name.ends_with(".lsb"))
        .map(|b| b.name)
        .collect();
    items.sort();
    get_store_index_from_blocks(client, &items).await
}

/// `readRemoteStoreIndex` (remotestore.go:1853): the top-level index load.
/// - `Init`: rebuild from `.lsb` blocks, then persist the rebuilt index.
/// - otherwise: merge-on-read all `store*.lsi` items (both flavors — the
///   locking flavor's canonical `store.lsi` is just another matching item);
///   not-found falls back to an empty store index.
pub(crate) async fn read_remote_store_index(
    blob_store: &dyn BlobStore,
    client: &dyn BlobClient,
    access_type: AccessType,
) -> Result<StoreIndex, StoreError> {
    let _ = blob_store; // reserved for the optional-store-index-paths arg
    if access_type == AccessType::Init {
        let rebuilt = build_store_index_from_store_blocks(client).await?;
        match add_to_remote_store_index(client, &rebuilt).await {
            Ok(Some(persisted)) => return Ok(persisted),
            Ok(None) => return Ok(rebuilt),
            Err(_) => return Ok(rebuilt), // persist failure is non-fatal (Go logs)
        }
    }
    match read_store_store_index_with_items(client).await {
        Ok((index, _used, _retries)) => Ok(index),
        Err(e) if e.is_not_found() => Ok(StoreIndex::empty(0)),
        Err(e) => Err(e),
    }
}
