//! Canonical fixture data maps: the v1/v2/v3 version chain (extended from
//! golongtail's `commands_test.go` maps) plus the "layer" set, as a single
//! source of truth for the corpus generator and differential tests.
//!
//! The v1→v2→v3 chain is deliberately constructed so that, taken together, it
//! exercises every diff operation the download path must handle: a file added,
//! a file deleted, content modified, a file renamed, a permission change
//! (0644→0755), and a file moved between directories.
//!
//! Note: the ffi crate keeps its own copy of the original (unextended) maps in
//! `support/longtail-ffi/tests/common/mod.rs`; that copy is left duplicated for
//! now (it belongs to the ffi crate's own test tree and does not depend on this
//! crate).

/// One file in a version-chain snapshot. `mode` is the POSIX permission bits to
/// apply on Linux (ignored on other platforms); `None` means the default
/// (0644 for files).
#[derive(Debug, Clone)]
pub struct ChainEntry {
    pub path: &'static str,
    pub content: &'static str,
    pub mode: Option<u32>,
}

const fn f(path: &'static str, content: &'static str) -> ChainEntry {
    ChainEntry {
        path,
        content,
        mode: None,
    }
}

const fn fm(path: &'static str, content: &'static str, mode: u32) -> ChainEntry {
    ChainEntry {
        path,
        content,
        mode: Some(mode),
    }
}

/// Chain version 1.
pub fn chain_v1() -> Vec<ChainEntry> {
    vec![
        f("empty-file", ""),
        f("abitoftext.txt", "this is a test file"),
        f(
            "folder/abitoftextinasubfolder.txt",
            "this is a test file in a subfolder",
        ),
        f(
            "folder/anotherabitoftextinasubfolder.txt",
            "this is a second test file in a subfolder",
        ),
        f(
            "to-delete.txt",
            "this file is present in v1 and removed in v2",
        ),
        f(
            "to-rename.txt",
            "this file keeps its bytes but changes name",
        ),
        f("to-move.txt", "this file moves between directories in v3"),
        fm("script.sh", "#!/bin/sh\necho hello\n", 0o644),
    ]
}

/// Chain version 2: adds files, deletes `to-delete.txt`, modifies
/// `abitoftext.txt`, renames `to-rename.txt`→`renamed.txt`, and flips
/// `script.sh` from 0644 to 0755.
pub fn chain_v2() -> Vec<ChainEntry> {
    vec![
        f("empty-file", ""),
        f("abitoftext.txt", "this is a test file, now modified in v2"),
        f(
            "folder/abitoftextinasubfolder.txt",
            "this is a test file in a subfolder",
        ),
        f(
            "folder/anotherabitoftextinasubfolder.txt",
            "this is a second test file in a subfolder",
        ),
        // to-delete.txt is gone (deleted)
        f("renamed.txt", "this file keeps its bytes but changes name"),
        f("to-move.txt", "this file moves between directories in v3"),
        fm("script.sh", "#!/bin/sh\necho hello\n", 0o755),
        f("stuff.txt", "we have some stuff"),
        f(
            "folder2/anotherabitoftextinasubfolder2.txt",
            "and some more text that we need",
        ),
    ]
}

/// Chain version 3: adds `morestuff.txt` and moves `to-move.txt` into `folder/`.
pub fn chain_v3() -> Vec<ChainEntry> {
    vec![
        f("empty-file", ""),
        f("abitoftext.txt", "this is a test file, now modified in v2"),
        f(
            "folder/abitoftextinasubfolder.txt",
            "this is a test file in a subfolder",
        ),
        f(
            "folder/anotherabitoftextinasubfolder.txt",
            "this is a second test file in a subfolder",
        ),
        f("renamed.txt", "this file keeps its bytes but changes name"),
        f(
            "folder/to-move.txt",
            "this file moves between directories in v3",
        ),
        fm("script.sh", "#!/bin/sh\necho hello\n", 0o755),
        f("stuff.txt", "we have some stuff"),
        f(
            "folder2/anotherabitoftextinasubfolder2.txt",
            "and some more text that we need",
        ),
        f("morestuff.txt", "we have even more stuff"),
    ]
}

/// Ordered list of `(chain-version-id, entries)`.
pub fn chain_versions() -> Vec<(&'static str, Vec<ChainEntry>)> {
    vec![("v1", chain_v1()), ("v2", chain_v2()), ("v3", chain_v3())]
}
