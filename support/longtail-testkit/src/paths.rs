//! Path resolution helpers.
//!
//! Cargo runs test binaries with the process cwd set to the crate's own
//! manifest directory (which sits two levels below the repo root for
//! `support/*` crates), so nothing here may use cwd-relative paths. Instead we
//! resolve everything from a compile-time `CARGO_MANIFEST_DIR` by walking up to
//! the workspace root (the directory whose `Cargo.toml` declares
//! `[workspace]`).

use std::path::{Path, PathBuf};

/// Walk up from `start` until we find a `Cargo.toml` that contains a
/// `[workspace]` table; return that directory. Panics if none is found.
pub fn find_workspace_root(start: &Path) -> PathBuf {
    let mut dir = start;
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.is_file()
            && std::fs::read_to_string(&cargo_toml)
                .map(|c| c.contains("[workspace]"))
                .unwrap_or(false)
        {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => panic!(
                "could not find workspace root walking up from {}",
                start.display()
            ),
        }
    }
}

/// The workspace root, resolved from this crate's manifest directory.
pub fn workspace_root() -> PathBuf {
    find_workspace_root(Path::new(env!("CARGO_MANIFEST_DIR")))
}

/// The committed `fixtures/` directory at the workspace root.
pub fn fixtures_dir() -> PathBuf {
    workspace_root().join("fixtures")
}

/// The upstream `chunker.input` test fixture inside the longtail submodule.
pub fn upstream_chunker_input() -> PathBuf {
    workspace_root().join("support/longtail-sys/longtail/test/testdata/chunker.input")
}
