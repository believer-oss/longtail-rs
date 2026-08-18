//! Golden fixture tests (pure lane): every committed `.lvi`, `.lsi`, and `.lsb`
//! parses and re-serializes **byte-identical** through the pure-Rust
//! `longtail-core` codecs. This is compat gate ① for the format layer — no
//! native library involved.
//!
//! Parsing is cheap, so we do ALL fixtures (all 16 `.lvi`, all 29 `.lsi`
//! including the two `sharded/` shards, and every `.lsb`), including the
//! deliberately odd-count cases (`many-files`, single-asset indexes) that leave
//! `u64` arrays on 4-byte boundaries. Paths resolve via the `paths` module,
//! never cwd.

use std::path::{Path, PathBuf};

use longtail_core::{StoreIndex, StoredBlock, VersionIndex};
use longtail_testkit::paths::fixtures_dir;

/// Every file under `root` with the given extension, sorted by path.
fn collect(root: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root).min_depth(1).sort_by_file_name() {
        let entry = entry.unwrap();
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|e| e.to_str()) == Some(ext)
        {
            out.push(entry.path().to_path_buf());
        }
    }
    out
}

#[test]
fn all_version_indexes_roundtrip_byte_identical() {
    let files = collect(&fixtures_dir(), "lvi");
    assert_eq!(files.len(), 16, "expected 16 committed .lvi fixtures");
    let mut failures = Vec::new();
    for f in &files {
        let orig = std::fs::read(f).unwrap();
        match VersionIndex::from_bytes(&orig) {
            Ok(vi) => {
                let written = vi.to_bytes();
                if written != orig {
                    failures.push(format!(
                        "{}: round-trip differs ({} -> {} bytes)",
                        f.display(),
                        orig.len(),
                        written.len()
                    ));
                }
            }
            Err(e) => failures.push(format!("{}: parse failed: {e}", f.display())),
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    eprintln!("{} .lvi round-tripped byte-identical", files.len());
}

#[test]
fn all_store_indexes_roundtrip_byte_identical() {
    let files = collect(&fixtures_dir(), "lsi");
    assert_eq!(files.len(), 29, "expected 29 committed .lsi fixtures");
    let mut failures = Vec::new();
    for f in &files {
        let orig = std::fs::read(f).unwrap();
        match StoreIndex::from_bytes(&orig) {
            Ok(si) => {
                let written = si.to_bytes();
                if written != orig {
                    failures.push(format!(
                        "{}: round-trip differs ({} -> {} bytes)",
                        f.display(),
                        orig.len(),
                        written.len()
                    ));
                }
            }
            Err(e) => failures.push(format!("{}: parse failed: {e}", f.display())),
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    eprintln!("{} .lsi round-tripped byte-identical", files.len());
}

#[test]
fn all_stored_blocks_roundtrip_byte_identical() {
    let files = collect(&fixtures_dir(), "lsb");
    assert!(
        !files.is_empty(),
        "expected committed .lsb fixtures, found none"
    );
    let mut failures = Vec::new();
    for f in &files {
        let orig = std::fs::read(f).unwrap();
        match StoredBlock::from_bytes(&orig) {
            Ok(sb) => {
                let written = sb.to_bytes();
                if written != orig {
                    failures.push(format!("{}: round-trip differs", f.display()));
                }
            }
            Err(e) => failures.push(format!("{}: parse failed: {e}", f.display())),
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    eprintln!("{} .lsb round-tripped byte-identical", files.len());
}
