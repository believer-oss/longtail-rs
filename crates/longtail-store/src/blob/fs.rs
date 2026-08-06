//! Filesystem blob store — a port of `fsstore.go` (+ the unix lock in
//! `fsstore_unix_amd64.go`).
//!
//! Locking uses `fs4` advisory locks (flock on unix, matching Go's
//! `syscall.Flock`) on a per-object `<path>._lck` file, with the generation
//! state in a `<path>.gen` sidecar and atomic writes via temp-file + rename.
//!
//! **Lock guard invariant (trust-critical):** [`FlockGuard`]'s
//! `Drop` releases the OS lock ONLY and is panic-free — it NEVER unlinks the
//! `._lck` file. Unlinking a flock'd path breaks mutual exclusion by inode
//! replacement (holder A unlinks, newcomer C recreates+locks a fresh inode while
//! waiter B still holds the old one) and throws sharing violations on Windows —
//! the exact bug class behind the ffi's `fsstore.rs:49` panic. The lock files
//! carry no state; the OS lock releases on `close()`/process death, so a
//! leftover `._lck` file never wedges the store.
//!
//! **Windows interop caveat** (documented divergence): golongtail on Windows
//! uses exclusive-open `CreateFile` retry loops, not `LockFileEx`; `fs4` uses
//! `LockFileEx`. Mixed Rust+Go fs-store writers on Windows therefore do not
//! mutually exclude. Accepted — the mixed-writer gate is minio-only;
//! Linux interop (flock both sides) is sound.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use bytes::Bytes;

use async_trait::async_trait;
use fs4::fs_std::FileExt;

use super::{BlobClient, BlobObject, BlobProperties, BlobStore};
use crate::error::StoreError;

/// A filesystem-backed blob store rooted at `prefix`.
#[derive(Debug, Clone)]
pub struct FsBlobStore {
    prefix: PathBuf,
    enable_locking: bool,
}

impl FsBlobStore {
    /// `NewFSBlobStore(prefix, enable_locking)`.
    pub fn new(prefix: impl AsRef<Path>, enable_locking: bool) -> FsBlobStore {
        FsBlobStore {
            prefix: prefix.as_ref().to_path_buf(),
            enable_locking,
        }
    }
}

#[async_trait]
impl BlobStore for FsBlobStore {
    async fn new_client(&self) -> Result<Box<dyn BlobClient>, StoreError> {
        Ok(Box::new(FsBlobClient {
            prefix: self.prefix.clone(),
            enable_locking: self.enable_locking,
        }))
    }

    fn name(&self) -> String {
        format!("fsblob://{}", self.prefix.display())
    }
}

#[derive(Clone)]
struct FsBlobClient {
    prefix: PathBuf,
    enable_locking: bool,
}

#[async_trait]
impl BlobClient for FsBlobClient {
    async fn new_object(&self, path: &str) -> Result<Box<dyn BlobObject>, StoreError> {
        let full = self.prefix.join(path);
        Ok(Box::new(FsBlobObject {
            path: full,
            enable_locking: self.enable_locking,
            // Go: metageneration starts at -1 (never locked).
            metageneration: -1,
        }))
    }

    async fn get_objects(&self, prefix: &str) -> Result<Vec<BlobProperties>, StoreError> {
        let root = self.prefix.clone();
        let prefix = prefix.to_string();
        tokio::task::spawn_blocking(move || get_objects_sync(&root, &prefix))
            .await
            .map_err(|e| StoreError::Backend(format!("join error: {e}")))?
    }

    fn supports_locking(&self) -> bool {
        self.enable_locking
    }

    fn name(&self) -> String {
        format!("fsblob://{}", self.prefix.display())
    }
}

/// Recursively list files under `root`, returning store-relative names that
/// start with `prefix`. Filters `._lck` lock files (fsstore.go:82).
///
/// A missing root lists as empty — that is the "store does not exist yet" case
/// and Go's `filepath.Walk` swallow (callback returns nil on err) is right for
/// it. Every *other* error is returned. Go swallowing them is a bug rather than
/// a compatibility requirement: a listing is not a byte format, and this
/// listing feeds `get_store_store_indexes`, where a silently short list of
/// `store_*.lsi` shards is a silently narrowed store index. Blocks that exist
/// become invisible, which surfaces later as "chunk not in the store index" on
/// a download, or as deleted blocks via prune.
fn get_objects_sync(root: &Path, prefix: &str) -> Result<Vec<BlobProperties>, StoreError> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let mut is_root = true;
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            // An absent store root is an empty store, not a failure.
            Err(e) if is_root && e.kind() == std::io::ErrorKind::NotFound => {
                is_root = false;
                continue;
            }
            Err(e) => {
                return Err(StoreError::io(format!("list dir {}", dir.display()), e));
            }
        };
        is_root = false;
        for entry in entries {
            let entry =
                entry.map_err(|e| StoreError::io(format!("dir entry in {}", dir.display()), e))?;
            let path = entry.path();
            let meta = entry
                .metadata()
                .map_err(|e| StoreError::io(format!("stat {}", path.display()), e))?;
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            let name_os = path.to_string_lossy();
            if name_os.ends_with("._lck") {
                continue;
            }
            // leafPath = path[len(root)+1:] — store-relative, forward slashes.
            let leaf = match path.strip_prefix(root) {
                Ok(p) => normalize(&p.to_string_lossy()),
                Err(_) => continue,
            };
            if leaf.len() < prefix.len() {
                continue;
            }
            if leaf.starts_with(prefix) {
                out.push(BlobProperties {
                    size: meta.len(),
                    name: leaf,
                });
            }
        }
    }
    Ok(out)
}

/// Normalize a filesystem path to forward slashes with no doubled separators
/// (the unix branch of `NormalizeFileSystemPath`).
fn normalize(p: &str) -> String {
    p.replace('\\', "/").replace("//", "/")
}

struct FsBlobObject {
    path: PathBuf,
    enable_locking: bool,
    /// Go `metageneration`; `-1` = never locked.
    metageneration: i64,
}

impl FsBlobObject {
    fn lock_path(&self) -> PathBuf {
        let mut s = self.path.clone().into_os_string();
        s.push("._lck");
        PathBuf::from(s)
    }

    fn gen_path(&self) -> PathBuf {
        let mut s = self.path.clone().into_os_string();
        s.push(".gen");
        PathBuf::from(s)
    }
}

/// An advisory-lock guard. Drop releases the OS lock ONLY (never unlinks).
struct FlockGuard {
    file: File,
}

impl Drop for FlockGuard {
    fn drop(&mut self) {
        // Release the flock explicitly; closing the fd (on `File` drop) also
        // releases it. Never unlink the lock file. Panic-free by construction
        // (`unlock` returns a `Result` we ignore).
        let _ = FileExt::unlock(&self.file);
    }
}

/// Acquire an exclusive advisory lock on `<path>._lck`, creating the parent dir
/// and lock file. Mirrors `fsBlobObject.lockFile` (fsstore.go:189-205).
fn lock_file(path: &Path, lock_path: &Path) -> Result<FlockGuard, StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| StoreError::io(format!("mkdir {}", parent.display()), e))?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|e| StoreError::io(format!("open lock {}", lock_path.display()), e))?;
    FileExt::lock_exclusive(&file)
        .map_err(|e| StoreError::io(format!("flock {}", lock_path.display()), e))?;
    Ok(FlockGuard { file })
}

fn read_meta_generation(gen_path: &Path) -> Result<i64, StoreError> {
    match std::fs::read(gen_path) {
        Ok(data) if data.len() >= 8 => {
            let mut b = [0u8; 8];
            b.copy_from_slice(&data[..8]);
            Ok(i64::from_le_bytes(b))
        }
        Ok(_) => Ok(0),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(StoreError::io(format!("read {}", gen_path.display()), e)),
    }
}

fn set_meta_generation(gen_path: &Path, generation: i64) -> Result<(), StoreError> {
    std::fs::write(gen_path, generation.to_le_bytes())
        .map_err(|e| StoreError::io(format!("write {}", gen_path.display()), e))
}

/// Atomic write: temp file in the same dir, then rename over the target.
fn atomic_write(path: &Path, data: &[u8]) -> Result<(), StoreError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|e| StoreError::io(format!("mkdir {}", parent.display()), e))?;
    let mut tmp = path.to_path_buf().into_os_string();
    tmp.push(format!(".tmp.{}", std::process::id()));
    // Make the temp name unique per attempt to avoid collisions between racing
    // in-process writers to the same key.
    tmp.push(format!(".{:x}", fastrand_like()));
    let tmp = PathBuf::from(tmp);
    {
        let mut f = File::create(&tmp)
            .map_err(|e| StoreError::io(format!("create {}", tmp.display()), e))?;
        f.write_all(data)
            .map_err(|e| StoreError::io(format!("write {}", tmp.display()), e))?;
        f.sync_all().ok();
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        StoreError::io(format!("rename to {}", path.display()), e)
    })
}

/// A cheap non-crypto counter for temp-file uniqueness (no `rand` dep in the
/// library crate). Combines time + an atomic counter.
fn fastrand_like() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static CTR: AtomicU64 = AtomicU64::new(0);
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    t ^ CTR.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)
}

#[async_trait]
impl BlobObject for FsBlobObject {
    async fn exists(&self) -> Result<bool, StoreError> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || match std::fs::metadata(&path) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(StoreError::io(format!("stat {}", path.display()), e)),
        })
        .await
        .map_err(|e| StoreError::Backend(format!("join error: {e}")))?
    }

    async fn lock_write_version(&mut self) -> Result<bool, StoreError> {
        if !self.enable_locking {
            return Err(StoreError::LockingNotSupported(self.name()));
        }
        let path = self.path.clone();
        let lock_path = self.lock_path();
        let gen_path = self.gen_path();
        let (exists, metageneration) =
            tokio::task::spawn_blocking(move || -> Result<(bool, i64), StoreError> {
                let _guard = lock_file(&path, &lock_path)?;
                let exists = std::fs::metadata(&path).is_ok();
                let meta = if exists {
                    read_meta_generation(&gen_path)?
                } else {
                    // Object absent → any lingering `.gen` is stale (the object
                    // was deleted, e.g. `init-remote-store` after removing
                    // `store.lsi`). Clear it so the create-CAS in `write()` has a
                    // consistent generation-0 baseline; otherwise the stale
                    // generation never matches and `add_to_remote_store_index`
                    // spins forever. Done under the flock, so a concurrent
                    // creator still wins the CAS via `.gen` bumping to 1.
                    let _ = std::fs::remove_file(&gen_path);
                    0
                };
                Ok((exists, meta))
            })
            .await
            .map_err(|e| StoreError::Backend(format!("join error: {e}")))??;
        self.metageneration = metageneration;
        Ok(exists)
    }

    async fn read(&self) -> Result<Vec<u8>, StoreError> {
        let path = self.path.clone();
        let lock_path = self.lock_path();
        let enable_locking = self.enable_locking;
        tokio::task::spawn_blocking(move || -> Result<Vec<u8>, StoreError> {
            let _guard = if enable_locking {
                Some(lock_file(&path, &lock_path)?)
            } else {
                None
            };
            let mut f = match File::open(&path) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(StoreError::NotFound(path.display().to_string()));
                }
                Err(e) => return Err(StoreError::io(format!("open {}", path.display()), e)),
            };
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)
                .map_err(|e| StoreError::io(format!("read {}", path.display()), e))?;
            Ok(buf)
        })
        .await
        .map_err(|e| StoreError::Backend(format!("join error: {e}")))?
    }

    async fn write(&mut self, data: Bytes) -> Result<bool, StoreError> {
        let path = self.path.clone();
        let lock_path = self.lock_path();
        let gen_path = self.gen_path();
        let enable_locking = self.enable_locking;
        let metageneration = self.metageneration;
        // `data` is already owned (`Bytes`, `Send + 'static`); move it straight
        // into the blocking closure — no `to_vec()` copy as the old `&[u8]`
        // signature required.
        tokio::task::spawn_blocking(move || -> Result<bool, StoreError> {
            let _guard = if enable_locking {
                Some(lock_file(&path, &lock_path)?)
            } else {
                None
            };
            if enable_locking && metageneration != -1 {
                let current = read_meta_generation(&gen_path)?;
                if current != metageneration {
                    return Ok(false);
                }
            }
            atomic_write(&path, &data)?;
            if enable_locking && metageneration != -1 {
                set_meta_generation(&gen_path, metageneration + 1)?;
            }
            Ok(true)
        })
        .await
        .map_err(|e| StoreError::Backend(format!("join error: {e}")))?
    }

    async fn delete(&mut self) -> Result<(), StoreError> {
        let path = self.path.clone();
        let lock_path = self.lock_path();
        let gen_path = self.gen_path();
        let enable_locking = self.enable_locking;
        let metageneration = self.metageneration;
        tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
            let _guard = if enable_locking {
                Some(lock_file(&path, &lock_path)?)
            } else {
                None
            };
            if enable_locking && metageneration != -1 {
                let current = read_meta_generation(&gen_path)?;
                if current != metageneration {
                    return Err(StoreError::GenerationMismatch(path.display().to_string()));
                }
            }
            std::fs::remove_file(&path)
                .map_err(|e| StoreError::io(format!("remove {}", path.display()), e))?;
            // Best-effort .gen cleanup (fsstore.go:307-316). With locking on, a
            // real error propagates; without, a missing .gen is fine.
            match std::fs::remove_file(&gen_path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) if enable_locking => {
                    Err(StoreError::io(format!("remove {}", gen_path.display()), e))
                }
                Err(_) => Ok(()),
            }
        })
        .await
        .map_err(|e| StoreError::Backend(format!("join error: {e}")))?
    }

    fn name(&self) -> String {
        format!("fsblob://{}", self.path.display())
    }
}
