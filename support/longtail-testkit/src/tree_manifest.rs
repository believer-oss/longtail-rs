//! Tree-manifest capture and comparison.
//!
//! A tree manifest is the canonical description of a directory tree used to
//! compare a downsynced version against its source: entries sorted by path,
//! each `{path, size, mode_octal, blake3_hex}`, directories included with size
//! 0 and an empty hash. On Windows, permissions are not faithful, so the
//! comparison helper can MASK the mode field. Manifests are always *generated*
//! on Linux.

use std::path::Path;

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    /// Root-relative, `/`-joined path. Directories do NOT carry a trailing `/`.
    pub path: String,
    pub size: u64,
    /// POSIX permission bits as a 4-digit octal string, e.g. `"0644"`.
    pub mode_octal: String,
    /// Lowercase hex blake3 of the file contents; empty string for directories.
    pub blake3_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeManifest {
    pub entries: Vec<TreeEntry>,
}

#[cfg(unix)]
fn mode_of(meta: &std::fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    format!("{:04o}", meta.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn mode_of(_meta: &std::fs::Metadata) -> String {
    // Windows can't reproduce POSIX ugo bits; recorded as a placeholder and
    // masked out by comparisons on non-Linux.
    "0000".to_string()
}

impl TreeManifest {
    /// Capture `root` recursively into a sorted manifest. The root directory
    /// itself is not included; every descendant file and directory is.
    pub fn capture(root: &Path) -> std::io::Result<TreeManifest> {
        let mut entries = Vec::new();
        for entry in WalkDir::new(root).min_depth(1).sort_by_file_name() {
            let entry = entry?;
            let meta = entry.metadata()?;
            let rel = entry
                .path()
                .strip_prefix(root)
                .expect("descendant of root")
                .to_string_lossy()
                .replace('\\', "/");
            if meta.is_dir() {
                entries.push(TreeEntry {
                    path: rel,
                    size: 0,
                    mode_octal: mode_of(&meta),
                    blake3_hex: String::new(),
                });
            } else if meta.is_file() {
                let bytes = std::fs::read(entry.path())?;
                entries.push(TreeEntry {
                    path: rel,
                    size: bytes.len() as u64,
                    mode_octal: mode_of(&meta),
                    blake3_hex: blake3::hash(&bytes).to_hex().to_string(),
                });
            }
            // symlinks / others: none in the corpus, skipped.
        }
        entries.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
        Ok(TreeManifest { entries })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("serialize tree manifest")
    }

    pub fn from_json(s: &str) -> serde_json::Result<TreeManifest> {
        serde_json::from_str(s)
    }

    /// Compare against `other`. When `mask_mode` is true, `mode_octal`
    /// differences are ignored (used on Windows). Returns `Ok(())` on match or
    /// a human-readable description of the first mismatch.
    pub fn compare(&self, other: &TreeManifest, mask_mode: bool) -> Result<(), String> {
        if self.entries.len() != other.entries.len() {
            let self_paths: Vec<_> = self.entries.iter().map(|e| &e.path).collect();
            let other_paths: Vec<_> = other.entries.iter().map(|e| &e.path).collect();
            return Err(format!(
                "entry count mismatch: {} vs {}\n  left:  {:?}\n  right: {:?}",
                self.entries.len(),
                other.entries.len(),
                self_paths,
                other_paths
            ));
        }
        for (a, b) in self.entries.iter().zip(other.entries.iter()) {
            if a.path != b.path {
                return Err(format!("path mismatch: {:?} vs {:?}", a.path, b.path));
            }
            if a.size != b.size {
                return Err(format!(
                    "size mismatch for {}: {} vs {}",
                    a.path, a.size, b.size
                ));
            }
            if a.blake3_hex != b.blake3_hex {
                return Err(format!(
                    "content hash mismatch for {}: {} vs {}",
                    a.path, a.blake3_hex, b.blake3_hex
                ));
            }
            if !mask_mode && a.mode_octal != b.mode_octal {
                return Err(format!(
                    "mode mismatch for {}: {} vs {}",
                    a.path, a.mode_octal, b.mode_octal
                ));
            }
        }
        Ok(())
    }
}
