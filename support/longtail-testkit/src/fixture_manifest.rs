//! Read/write/verify `fixtures/manifest.json`.
//!
//! The manifest records the generating CLI (version, url, binary sha256, os)
//! and a sha256 for every committed file under `fixtures/` (except the manifest
//! itself). `verify` re-hashes every file and fails on mismatch, extra, or
//! missing files — the fast, network-free check every build runs.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

pub const MANIFEST_NAME: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Generator {
    pub version: String,
    pub url: String,
    pub binary_sha256: String,
    pub os: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Path relative to `fixtures/`, `/`-joined.
    pub path: String,
    pub size: u64,
    pub sha256: String,
    /// How the file was produced, e.g. `"golongtail upsync"`, `"copied"`,
    /// `"ffi chunker"`, `"testkit"`.
    pub produced_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub generator: Generator,
    pub entries: Vec<FileEntry>,
}

/// sha256 of a byte slice as lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// sha256 of a file as lowercase hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    Ok(sha256_hex(&std::fs::read(path)?))
}

/// Enumerate every committed file under `fixtures_dir` (recursively), excluding
/// the manifest itself, as `(relative_path, absolute_path)` sorted by relative
/// path.
pub fn list_fixture_files(fixtures_dir: &Path) -> Vec<(String, std::path::PathBuf)> {
    let mut out = Vec::new();
    for entry in WalkDir::new(fixtures_dir).min_depth(1).sort_by_file_name() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(fixtures_dir)
            .expect("under fixtures")
            .to_string_lossy()
            .replace('\\', "/");
        // The manifest itself and documentation are not fixtures.
        if rel == MANIFEST_NAME || rel == "README.md" {
            continue;
        }
        out.push((rel, entry.path().to_path_buf()));
    }
    out.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    out
}

impl Manifest {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("serialize manifest")
    }

    pub fn from_json(s: &str) -> serde_json::Result<Manifest> {
        serde_json::from_str(s)
    }

    pub fn load(fixtures_dir: &Path) -> std::io::Result<Manifest> {
        let s = std::fs::read_to_string(fixtures_dir.join(MANIFEST_NAME))?;
        Ok(Manifest::from_json(&s).expect("parse manifest.json"))
    }

    /// Re-hash every file under `fixtures_dir` and check it against this
    /// manifest. Returns the list of problems (empty = all good).
    pub fn verify(&self, fixtures_dir: &Path) -> Vec<String> {
        let mut problems = Vec::new();
        let mut expected: std::collections::BTreeMap<&str, &FileEntry> =
            self.entries.iter().map(|e| (e.path.as_str(), e)).collect();

        for (rel, abs) in list_fixture_files(fixtures_dir) {
            match expected.remove(rel.as_str()) {
                None => problems.push(format!("extra file not in manifest: {rel}")),
                Some(entry) => match sha256_file(&abs) {
                    Ok(got) => {
                        if got != entry.sha256 {
                            problems.push(format!(
                                "sha256 mismatch for {rel}: manifest {} vs disk {got}",
                                entry.sha256
                            ));
                        }
                        let size = abs.metadata().map(|m| m.len()).unwrap_or(0);
                        if size != entry.size {
                            problems.push(format!(
                                "size mismatch for {rel}: manifest {} vs disk {size}",
                                entry.size
                            ));
                        }
                    }
                    Err(e) => problems.push(format!("cannot hash {rel}: {e}")),
                },
            }
        }
        for missing in expected.keys() {
            problems.push(format!("missing file listed in manifest: {missing}"));
        }
        problems
    }
}
