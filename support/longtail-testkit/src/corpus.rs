//! Deterministic edge-case corpus generator.
//!
//! Content is generated with `rand_chacha::ChaCha20Rng` (algorithmically stable
//! across releases — never `StdRng`). The seed for a corpus case is the 32-byte
//! `blake3("longtail-corpus:" + case_id)`, so every case is reproducible
//! independent of generation order. The corpus is regenerated on demand and is
//! NOT committed; the fixtures derived from it (via the pinned golongtail CLI)
//! ARE committed.
//!
//! **Linux-only for generation**: POSIX permission bits are applied with
//! `set_permissions`, which is only faithful on Linux. Windows CI verifies
//! round-trips; it never generates.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use rand_chacha::ChaCha20Rng;
use rand_core::{RngCore, SeedableRng};

use crate::data;

pub const KIB: usize = 1024;
pub const MIB: usize = 1024 * 1024;

/// A ChaCha20 RNG seeded deterministically from a case id.
fn seeded_rng(case_id: &str) -> ChaCha20Rng {
    let mut input = b"longtail-corpus:".to_vec();
    input.extend_from_slice(case_id.as_bytes());
    let seed: [u8; 32] = *blake3::hash(&input).as_bytes();
    ChaCha20Rng::from_seed(seed)
}

/// `n` deterministic pseudo-random bytes for `case_id`.
fn random_bytes(case_id: &str, n: usize) -> Vec<u8> {
    let mut rng = seeded_rng(case_id);
    let mut v = vec![0u8; n];
    rng.fill_bytes(&mut v);
    v
}

fn write_file(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    let mut f = fs::File::create(&path).unwrap_or_else(|e| panic!("create {rel}: {e}"));
    f.write_all(bytes)
        .unwrap_or_else(|e| panic!("write {rel}: {e}"));
}

#[cfg(unix)]
fn set_mode(root: &Path, rel: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join(rel);
    fs::set_permissions(&path, fs::Permissions::from_mode(mode))
        .unwrap_or_else(|e| panic!("chmod {rel}: {e}"));
}

#[cfg(not(unix))]
fn set_mode(_root: &Path, _rel: &str, _mode: u32) {}

/// Generate a 1 MiB blob of word-like, highly compressible pseudo-text.
fn compressible_text(case_id: &str, target_len: usize) -> Vec<u8> {
    const WORDS: &[&str] = &[
        "the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog", "longtail", "chunk",
        "block", "store", "index", "version", "hash", "delta", "content", "asset", "folder",
        "compress",
    ];
    let mut rng = seeded_rng(case_id);
    let mut out = Vec::with_capacity(target_len + 16);
    while out.len() < target_len {
        let w = WORDS[(rng.next_u32() as usize) % WORDS.len()];
        out.extend_from_slice(w.as_bytes());
        out.push(b' ');
        if (rng.next_u32() & 0x0f) == 0 {
            out.push(b'\n');
        }
    }
    out.truncate(target_len);
    out
}

/// The 200-character long file stem (before `.txt`).
fn long_name() -> String {
    "a".repeat(200)
}

/// Top-level corpus entry names, used to select subsets for the fixture matrix.
/// A name may be a single file or a whole directory tree; [`copy_entries`]
/// materializes them by name.
pub mod entries {
    pub const EMPTY_FILE: &str = "empty-file";
    pub const ONE_BYTE: &str = "one-byte";
    pub const WIN_47: &str = "win-47";
    pub const WIN_48: &str = "win-48";
    pub const WIN_49: &str = "win-49";
    pub const MIN_CHUNK: &str = "min-chunk";
    pub const MAX_EDGE_MINUS: &str = "max-edge-minus";
    pub const MAX_EDGE: &str = "max-edge";
    pub const MAX_EDGE_PLUS: &str = "max-edge-plus";
    pub const BIG_STREAM: &str = "big-stream";
    pub const MULTI_BLOCK: &str = "multi-block";
    pub const MANY_FILES: &str = "many-files";
    pub const DUP_A: &str = "dup-a.bin";
    pub const DUP_B: &str = "dup-b.bin";
    pub const REPETITIVE: &str = "repetitive.bin";
    pub const COMPRESSIBLE: &str = "compressible.txt";
    pub const INCOMPRESSIBLE: &str = "incompressible.bin";
    pub const PERMS: &str = "perms";
    pub const EMPTY_DIR: &str = "empty-dir";
    pub const DEEP: &str = "deep";
    pub const NAMES: &str = "names";
}

/// Every top-level zoo entry name, in a stable order.
pub fn zoo_all() -> Vec<&'static str> {
    use entries::*;
    vec![
        EMPTY_FILE,
        ONE_BYTE,
        WIN_47,
        WIN_48,
        WIN_49,
        MIN_CHUNK,
        MAX_EDGE_MINUS,
        MAX_EDGE,
        MAX_EDGE_PLUS,
        BIG_STREAM,
        MULTI_BLOCK,
        MANY_FILES,
        DUP_A,
        DUP_B,
        REPETITIVE,
        COMPRESSIBLE,
        INCOMPRESSIBLE,
        PERMS,
        EMPTY_DIR,
        DEEP,
        NAMES,
    ]
}

/// Small subset (comp-* and blake2 cells): the zoo minus the three biggest
/// pieces (`multi-block`, `big-stream`, `repetitive.bin`).
pub fn zoo_small() -> Vec<&'static str> {
    use entries::*;
    zoo_all()
        .into_iter()
        .filter(|n| ![MULTI_BLOCK, BIG_STREAM, REPETITIVE].contains(n))
        .collect()
}

/// Medium subset (chunk-* cells): the zoo minus `multi-block` and
/// `incompressible.bin` — keeps `big-stream` (1 MiB, exercises many chunks at
/// varying target sizes) and `repetitive.bin`, but stays within budget by not
/// duplicating a second incompressible megabyte.
pub fn zoo_medium() -> Vec<&'static str> {
    use entries::*;
    zoo_all()
        .into_iter()
        .filter(|n| ![MULTI_BLOCK, INCOMPRESSIBLE].contains(n))
        .collect()
}

/// Disjoint subset A for the synthesized sharded store.
pub fn sharded_subset_a() -> Vec<&'static str> {
    use entries::*;
    vec![MIN_CHUNK, WIN_49]
}

/// Disjoint subset B for the synthesized sharded store.
pub fn sharded_subset_b() -> Vec<&'static str> {
    use entries::*;
    vec![MAX_EDGE, ONE_BYTE]
}

/// Union of the two sharded subsets, for building the combined version index
/// that spans both shards (exercising merge-on-read).
pub fn sharded_union() -> Vec<&'static str> {
    let mut v = sharded_subset_a();
    v.extend(sharded_subset_b());
    v
}

/// Deterministically reproduce the raw bytes of a single top-level zoo *file*
/// case, without writing the whole corpus to disk. Returns `None` for names
/// that are directories or not single-file cases. Single source of truth for
/// the seed logic, shared by the corpus writer and the boundary self-validation
/// test.
pub fn case_bytes(name: &str) -> Option<Vec<u8>> {
    use entries::*;
    let bytes = match name {
        EMPTY_FILE => Vec::new(),
        ONE_BYTE => vec![0x42],
        WIN_47 => random_bytes(WIN_47, 47),
        WIN_48 => random_bytes(WIN_48, 48),
        WIN_49 => random_bytes(WIN_49, 49),
        MIN_CHUNK => random_bytes(MIN_CHUNK, 4096),
        MAX_EDGE_MINUS => random_bytes(MAX_EDGE_MINUS, 65535),
        MAX_EDGE => random_bytes(MAX_EDGE, 65536),
        MAX_EDGE_PLUS => random_bytes(MAX_EDGE_PLUS, 65537),
        BIG_STREAM => random_bytes(BIG_STREAM, MIB),
        MULTI_BLOCK => random_bytes(MULTI_BLOCK, 9 * MIB),
        DUP_A | DUP_B => random_bytes("dup-content", 8 * KIB),
        REPETITIVE => {
            let pattern = random_bytes(REPETITIVE, 64 * KIB);
            let mut v = Vec::with_capacity(64 * KIB * 32);
            for _ in 0..32 {
                v.extend_from_slice(&pattern);
            }
            v
        }
        COMPRESSIBLE => compressible_text(COMPRESSIBLE, MIB),
        INCOMPRESSIBLE => random_bytes(INCOMPRESSIBLE, MIB),
        _ => return None,
    };
    Some(bytes)
}

/// Generate the full edge-case zoo into `root` (created if missing).
pub fn generate_zoo(root: &Path) {
    use entries::*;
    fs::create_dir_all(root).expect("create corpus root");

    // Single-file cases, sourced from the shared `case_bytes` generators so the
    // boundary self-validation test can reproduce identical bytes.
    for name in [
        EMPTY_FILE,
        ONE_BYTE,
        WIN_47,
        WIN_48,
        WIN_49,
        MIN_CHUNK,
        MAX_EDGE_MINUS,
        MAX_EDGE,
        MAX_EDGE_PLUS,
        BIG_STREAM,
        MULTI_BLOCK,
        DUP_A,
        DUP_B,
        REPETITIVE,
        COMPRESSIBLE,
        INCOMPRESSIBLE,
    ] {
        write_file(root, name, &case_bytes(name).expect("case bytes"));
    }

    // Multi-block by chunk COUNT: 1100 tiny distinct files (>1024 chunks).
    for i in 0..1100 {
        let rel = format!("{MANY_FILES}/file-{i:04}.bin");
        let bytes = random_bytes(&rel, 64);
        write_file(root, &rel, &bytes);
    }

    // Permission variants (Linux-only faithful).
    write_file(root, "perms/mode-644.txt", b"mode 644 file\n");
    write_file(root, "perms/mode-755.sh", b"#!/bin/sh\necho perms\n");
    write_file(root, "perms/mode-444.txt", b"read only file\n");
    set_mode(root, "perms/mode-644.txt", 0o644);
    set_mode(root, "perms/mode-755.sh", 0o755);
    set_mode(root, "perms/mode-444.txt", 0o444);

    // Empty directory.
    fs::create_dir_all(root.join(EMPTY_DIR)).expect("create empty-dir");

    // Deep nesting (10 levels) with a leaf.
    let mut deep = PathBuf::from(DEEP);
    for i in 1..=10 {
        deep = deep.join(format!("l{i}"));
    }
    let deep_leaf = deep.join("leaf.txt");
    write_file(root, deep_leaf.to_str().unwrap(), b"deep leaf\n");

    // Long + UTF-8 names.
    write_file(root, &format!("names/{}.txt", long_name()), b"long name\n");
    write_file(root, "names/héllo-wörld-日本語.txt", b"utf8 name\n");
}

/// Generate the v1/v2/v3 version chain into `root/chain/{v1,v2,v3}`.
pub fn generate_chain(root: &Path) {
    for (id, entries) in data::chain_versions() {
        let vdir = root.join("chain").join(id);
        fs::create_dir_all(&vdir).expect("create chain version dir");
        for e in entries {
            write_file(&vdir, e.path, e.content.as_bytes());
            if let Some(mode) = e.mode {
                set_mode(&vdir, e.path, mode);
            }
        }
    }
}

/// Generate the entire corpus (zoo + chain) into `root`.
pub fn generate_all(root: &Path) {
    generate_zoo(root);
    generate_chain(root);
}

/// Copy the named top-level zoo entries from `corpus_root` into `dest`
/// (created), preserving directory trees, file contents, and permissions.
/// Used by the fixture matrix to build per-cell subset source folders.
pub fn copy_entries(corpus_root: &Path, dest: &Path, names: &[&str]) {
    fs::create_dir_all(dest).expect("create subset dest");
    for name in names {
        let src = corpus_root.join(name);
        let dst = dest.join(name);
        copy_recursive(&src, &dst);
    }
}

fn copy_recursive(src: &Path, dst: &Path) {
    let meta = fs::symlink_metadata(src).unwrap_or_else(|e| panic!("stat {}: {e}", src.display()));
    if meta.is_dir() {
        fs::create_dir_all(dst).expect("mkdir");
        for entry in fs::read_dir(src).expect("read_dir") {
            let entry = entry.expect("dir entry");
            copy_recursive(&entry.path(), &dst.join(entry.file_name()));
        }
        copy_mode(src, dst);
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).expect("mkdir parent");
        }
        fs::copy(src, dst).unwrap_or_else(|e| panic!("copy {}: {e}", src.display()));
        copy_mode(src, dst);
    }
}

#[cfg(unix)]
fn copy_mode(src: &Path, dst: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(src) {
        let mode = meta.permissions().mode();
        let _ = fs::set_permissions(dst, fs::Permissions::from_mode(mode));
    }
}

#[cfg(not(unix))]
fn copy_mode(_src: &Path, _dst: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_is_deterministic() {
        // Same seed → identical bytes across calls.
        assert_eq!(
            random_bytes("big-stream", 128),
            random_bytes("big-stream", 128)
        );
        assert_ne!(random_bytes("a", 64), random_bytes("b", 64));
    }

    #[test]
    fn subsets_are_disjoint_and_sized() {
        assert_eq!(zoo_all().len(), 21);
        let a = sharded_subset_a();
        let b = sharded_subset_b();
        for x in &a {
            assert!(!b.contains(x), "shard subsets must be disjoint");
        }
    }
}
