//! Narrow internal local-fs helpers over `std::fs` (+ positional
//! `FileExt::write_at`/`seek_write`) — NOT a port of the 26-fn StorageAPI vtable.
//! The recursive walker, permission mapping, and the
//! concurrent-chunk-write equivalent all live here with the facade; `longtail-core`
//! stays I/O-free.

use std::fs;
use std::path::{Path, PathBuf};

use longtail_core::{FileEntry, Permissions};

use crate::error::LongtailError;
use crate::path_filter::RegexPathFilter;

/// The low-9-bit POSIX permission mask (`st_mode & 0x1FF`, longtail_platform.c:2162).
const PERM_MASK: u32 = 0x1FF;

/// Read a file's permission bits as longtail stores them.
#[cfg(unix)]
pub fn mode_of(meta: &fs::Metadata) -> u16 {
    use std::os::unix::fs::PermissionsExt;
    (meta.permissions().mode() & PERM_MASK) as u16
}

/// Windows fallback: synthesize `0444`/`0666` (+ execute/`0555`/`0777` for dirs)
/// exactly as `Longtail_GetEntryProperties` (format-spec §7).
#[cfg(not(unix))]
pub fn mode_of(meta: &fs::Metadata) -> u16 {
    let mut m: u16 = 0o444;
    if meta.is_dir() {
        m |= 0o111;
    }
    if !meta.permissions().readonly() {
        m |= 0o222;
    }
    m
}

/// Recursively scan `root` into [`FileEntry`]s (dirs included, size 0), honoring
/// `filter`. Root-relative, `/`-separated paths without a trailing slash (the
/// trailing `/` for dirs is synthesized by `FileInfos`). The root itself is not
/// included. Mirrors `Longtail_GetFilesRecursively2` (longtail.c:1806).
pub fn scan_folder(root: &Path, filter: &RegexPathFilter) -> Result<Vec<FileEntry>, LongtailError> {
    let mut out = Vec::new();
    if root.exists() {
        scan_dir(root, "", filter, &mut out)?;
    }
    Ok(out)
}

fn scan_dir(
    dir: &Path,
    rel_prefix: &str,
    filter: &RegexPathFilter,
    out: &mut Vec<FileEntry>,
) -> Result<(), LongtailError> {
    let read = fs::read_dir(dir).map_err(|e| LongtailError::io(format!("read_dir {dir:?}"), e))?;
    // Collect + sort by name for deterministic traversal (FileInfos re-sorts
    // globally, but a stable input keeps generation reproducible).
    let mut names: Vec<PathBuf> = Vec::new();
    for entry in read {
        let entry = entry.map_err(|e| LongtailError::io(format!("dir entry in {dir:?}"), e))?;
        names.push(entry.path());
    }
    names.sort();

    for path in names {
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let rel = if rel_prefix.is_empty() {
            name.clone()
        } else {
            format!("{rel_prefix}/{name}")
        };
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| LongtailError::io(format!("stat {path:?}"), e))?;
        let is_dir = meta.is_dir();
        // Skip symlinks / specials (none in the corpus).
        if !is_dir && !meta.is_file() {
            continue;
        }
        if filter.include(&rel, is_dir) {
            out.push(FileEntry {
                relative_path: rel.clone(),
                size: if is_dir { 0 } else { meta.len() },
                permissions: Permissions(mode_of(&meta)),
                is_dir,
            });
        }
        if is_dir {
            scan_dir(&path, &rel, filter, out)?;
        }
    }
    Ok(())
}

/// Read an asset's bytes (empty for a directory).
pub fn read_asset(root: &Path, rel_path: &str, is_dir: bool) -> Result<Vec<u8>, LongtailError> {
    if is_dir {
        return Ok(Vec::new());
    }
    let path = root.join(rel_path);
    fs::read(&path).map_err(|e| LongtailError::io(format!("read {path:?}"), e))
}

/// `mkdir -p` for a directory asset.
pub fn create_dir(root: &Path, rel_path: &str) -> Result<(), LongtailError> {
    let path = root.join(rel_path);
    fs::create_dir_all(&path).map_err(|e| LongtailError::io(format!("mkdir {path:?}"), e))
}

/// Ensure the parent directory of `root/rel_path` exists.
pub fn ensure_parent(root: &Path, rel_path: &str) -> Result<(), LongtailError> {
    let path = root.join(rel_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| LongtailError::io(format!("mkdir {parent:?}"), e))?;
    }
    Ok(())
}

/// Create/truncate a file to its final size (first-touch semantic;
/// concurrentchunkwrite.c:108 `OpenWriteFile(path, m_FileSize)`), ensuring its
/// parent directory exists. Returns the open handle for positional writes.
pub fn create_file_sized(
    root: &Path,
    rel_path: &str,
    final_size: u64,
) -> Result<fs::File, LongtailError> {
    ensure_parent(root, rel_path)?;
    let path = root.join(rel_path);
    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| LongtailError::io(format!("create {path:?}"), e))?;
    if final_size > 0 {
        file.set_len(final_size)
            .map_err(|e| LongtailError::io(format!("set_len {path:?}"), e))?;
    }
    Ok(file)
}

/// Open an already-created file for positional writes (no truncation), as C's
/// `OpenAppendFile` reopen (concurrentchunkwrite.c:76).
pub fn open_for_write(root: &Path, rel_path: &str) -> Result<fs::File, LongtailError> {
    let path = root.join(rel_path);
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .map_err(|e| LongtailError::io(format!("open {path:?}"), e))
}

/// Positional write of `data` at absolute `offset` in `file` (pwrite / seek_write).
pub fn write_at(file: &fs::File, offset: u64, data: &[u8]) -> Result<(), LongtailError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_all_at(data, offset)
            .map_err(|e| LongtailError::io("write_at", e))
    }
    #[cfg(not(unix))]
    {
        use std::os::windows::fs::FileExt;
        let mut written = 0usize;
        while written < data.len() {
            let n = file
                .seek_write(&data[written..], offset + written as u64)
                .map_err(|e| LongtailError::io("seek_write", e))?;
            if n == 0 {
                return Err(LongtailError::io(
                    "seek_write",
                    std::io::Error::new(std::io::ErrorKind::WriteZero, "short write"),
                ));
            }
            written += n;
        }
        Ok(())
    }
}

/// Apply POSIX permission bits (low 9 bits) to `root/rel_path` (`retain_permissions`).
pub fn set_permissions(
    root: &Path,
    rel_path: &str,
    perms: Permissions,
) -> Result<(), LongtailError> {
    let path = root.join(rel_path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            &path,
            fs::Permissions::from_mode((perms.bits() as u32) & PERM_MASK),
        )
        .map_err(|e| LongtailError::io(format!("chmod {path:?}"), e))
    }
    #[cfg(not(unix))]
    {
        // Windows: only the read-only attribute round-trips (format-spec §7).
        let mut p = fs::metadata(&path)
            .map_err(|e| LongtailError::io(format!("stat {path:?}"), e))?
            .permissions();
        let writable = (perms.bits() & 0o222) != 0;
        p.set_readonly(!writable);
        fs::set_permissions(&path, p).map_err(|e| LongtailError::io(format!("chmod {path:?}"), e))
    }
}

/// Add the user-write bit if missing (so a removal can proceed;
/// CleanUpRemoveAssets, longtail.c:7845).
#[cfg(unix)]
fn ensure_user_writable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mode = meta.permissions().mode();
        if mode & 0o200 == 0 {
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o200));
        }
    }
}

#[cfg(not(unix))]
fn ensure_user_writable(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let mut p = meta.permissions();
        p.set_readonly(false);
        let _ = fs::set_permissions(path, p);
    }
}

/// Remove an asset (file or directory) at `root/rel_path`. Directories are
/// removed only when empty (matching C's `RemoveDir`); the removal ensures
/// user-write first. Returns `Ok(true)` on success, `Ok(false)` if it still
/// exists afterwards (retryable), `Err` on a hard error.
pub fn remove_asset(root: &Path, rel_path: &str, is_dir: bool) -> Result<bool, LongtailError> {
    let path = root.join(rel_path);
    if is_dir {
        if !path.exists() {
            return Ok(true);
        }
        if !path.is_dir() {
            // Replaced by a non-dir since the source scan; not our job.
            return Ok(true);
        }
        ensure_user_writable(&path);
        match fs::remove_dir(&path) {
            Ok(()) => Ok(true),
            Err(_) if !path.exists() => Ok(true),
            Err(_) => Ok(false), // likely non-empty; a retry pass may clear it
        }
    } else {
        if !path.exists() {
            return Ok(true);
        }
        if !path.is_file() {
            return Ok(true);
        }
        ensure_user_writable(&path);
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(_) if !path.exists() => Ok(true),
            Err(e) => Err(LongtailError::io(format!("remove {path:?}"), e)),
        }
    }
}

/// Strip a trailing `/` from a version-index path (dir paths carry one).
pub fn strip_trailing_slash(p: &str) -> &str {
    p.strip_suffix('/').unwrap_or(p)
}

/// Whether a file exists at `path`.
pub fn file_exists(path: &Path) -> bool {
    path.is_file()
}

/// Read a `.lvi`/`.lsi`/get-config from a URI: a local path (or `file://`),
/// or `s3://bucket/key` (feature `s3`). golongtail reads these via its blob
/// store abstraction (`ReadFromURI`); local paths never go through a URI parser
/// (folderscanner.go:115 uses a plain file read for target-index paths).
pub async fn read_from_uri(
    uri: &str,
    #[allow(unused)] s3_options: &S3OptionsArg,
) -> Result<Vec<u8>, LongtailError> {
    if let Some(rest) = uri.strip_prefix("file://") {
        return read_local(rest);
    }
    if let Some(rest) = uri.strip_prefix("fsblob://") {
        return read_local(rest);
    }
    if uri.starts_with("s3://") {
        #[cfg(feature = "s3")]
        {
            return read_s3(uri, s3_options).await;
        }
        #[cfg(not(feature = "s3"))]
        {
            return Err(LongtailError::UnsupportedUri {
                uri: uri.to_string(),
                reason: "s3:// support was compiled out".into(),
            });
        }
    }
    if let Some((scheme, _)) = split_scheme(uri)
        && scheme.len() > 1
    {
        return Err(LongtailError::UnsupportedUri {
            uri: uri.to_string(),
            reason: format!("unsupported uri scheme `{scheme}`"),
        });
    }
    read_local(uri)
}

fn read_local(path: &str) -> Result<Vec<u8>, LongtailError> {
    fs::read(path).map_err(|e| LongtailError::io(format!("read {path}"), e))
}

fn split_scheme(uri: &str) -> Option<(&str, &str)> {
    let idx = uri.find("://")?;
    Some((&uri[..idx], &uri[idx + 3..]))
}

/// Alias so the s3-feature `read_from_uri` signature is stable either way.
#[cfg(feature = "s3")]
pub type S3OptionsArg = longtail_store::S3Options;
#[cfg(not(feature = "s3"))]
pub type S3OptionsArg = ();

#[cfg(feature = "s3")]
async fn read_s3(uri: &str, options: &longtail_store::S3Options) -> Result<Vec<u8>, LongtailError> {
    use longtail_store::{BlobStore, S3BlobStore};
    // Split into a parent-directory URI and the object basename.
    let after = &uri["s3://".len()..];
    let (parent, name) = match after.rfind('/') {
        Some(pos) => (&uri[..("s3://".len() + pos)], &after[pos + 1..]),
        None => {
            return Err(LongtailError::UnsupportedUri {
                uri: uri.to_string(),
                reason: "s3 uri missing object key".into(),
            });
        }
    };
    let store = S3BlobStore::from_uri_with_options(parent, options.clone())?;
    let client = store.new_client().await?;
    let obj = client.new_object(name).await?;
    obj.read().await.map_err(LongtailError::from)
}

/// Write `bytes` to a URI: a local path (or `file://`), or `s3://bucket/key`
/// (feature `s3`). Mirrors golongtail's `WriteToURI` (longtailutils.go:342):
/// split into a parent-directory URI + object basename, then write via the blob
/// store. Used by upsync/put/clone-store to write `.lvi`/`.lsi`/get-config.
pub async fn write_to_uri(
    uri: &str,
    bytes: &[u8],
    #[allow(unused)] s3_options: &S3OptionsArg,
) -> Result<(), LongtailError> {
    if let Some(rest) = uri.strip_prefix("file://") {
        return write_local(Path::new(rest), bytes);
    }
    if let Some(rest) = uri.strip_prefix("fsblob://") {
        return write_local(Path::new(rest), bytes);
    }
    if uri.starts_with("s3://") {
        #[cfg(feature = "s3")]
        {
            return write_s3(uri, bytes, s3_options).await;
        }
        #[cfg(not(feature = "s3"))]
        {
            return Err(LongtailError::UnsupportedUri {
                uri: uri.to_string(),
                reason: "s3:// support was compiled out".into(),
            });
        }
    }
    if let Some((scheme, _)) = split_scheme(uri)
        && scheme.len() > 1
    {
        return Err(LongtailError::UnsupportedUri {
            uri: uri.to_string(),
            reason: format!("unsupported uri scheme `{scheme}`"),
        });
    }
    write_local(Path::new(uri), bytes)
}

#[cfg(feature = "s3")]
async fn write_s3(
    uri: &str,
    bytes: &[u8],
    options: &longtail_store::S3Options,
) -> Result<(), LongtailError> {
    use longtail_store::{BlobStore, S3BlobStore};
    let after = &uri["s3://".len()..];
    let (parent, name) = match after.rfind('/') {
        Some(pos) => (&uri[..("s3://".len() + pos)], &after[pos + 1..]),
        None => {
            return Err(LongtailError::UnsupportedUri {
                uri: uri.to_string(),
                reason: "s3 uri missing object key".into(),
            });
        }
    };
    let store = S3BlobStore::from_uri_with_options(parent, options.clone())?;
    let client = store.new_client().await?;
    let mut obj = client.new_object(name).await?;
    obj.write(bytes).await?;
    Ok(())
}

/// Delete an object at a URI (local path / `file://` / `s3://`). Best-effort:
/// a missing object is not an error. Used by clone-store's zip fallback cleanup
/// and prune paths that operate through URIs.
#[allow(dead_code)]
pub async fn delete_uri(
    uri: &str,
    #[allow(unused)] s3_options: &S3OptionsArg,
) -> Result<(), LongtailError> {
    if let Some(rest) = uri.strip_prefix("file://") {
        return delete_local(Path::new(rest));
    }
    if !uri.starts_with("s3://") {
        return delete_local(Path::new(uri));
    }
    #[cfg(feature = "s3")]
    {
        use longtail_store::{BlobStore, S3BlobStore};
        let after = &uri["s3://".len()..];
        if let Some(pos) = after.rfind('/') {
            let parent = &uri[..("s3://".len() + pos)];
            let name = &after[pos + 1..];
            let store = S3BlobStore::from_uri_with_options(parent, s3_options.clone())?;
            let client = store.new_client().await?;
            let mut obj = client.new_object(name).await?;
            let _ = obj.delete().await;
        }
        Ok(())
    }
    #[cfg(not(feature = "s3"))]
    {
        Err(LongtailError::UnsupportedUri {
            uri: uri.to_string(),
            reason: "s3:// support was compiled out".into(),
        })
    }
}

/// Write a version index to a local `.lvi` file (cache write-back).
pub fn write_local(path: &Path, bytes: &[u8]) -> Result<(), LongtailError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| LongtailError::io(format!("mkdir {parent:?}"), e))?;
    }
    fs::write(path, bytes).map_err(|e| LongtailError::io(format!("write {path:?}"), e))
}

/// Delete a local file if it exists (cache-index deletion before mutating).
pub fn delete_local(path: &Path) -> Result<(), LongtailError> {
    if path.exists() {
        fs::remove_file(path).map_err(|e| LongtailError::io(format!("delete {path:?}"), e))?;
    }
    Ok(())
}
