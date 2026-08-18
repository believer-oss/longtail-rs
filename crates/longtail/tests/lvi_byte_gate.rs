//! Compat gate ⑦ (pulled forward for the corpus): the pure-Rust
//! `create_version_index` over the regenerated corpus produces **byte-identical**
//! `.lvi` to every committed non-`meow` fixture (15 of 16).
//!
//! Linux-only (POSIX permission generation); skipped under miri (fs/fixture).

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use longtail::{
    RegexPathFilter, compression_type_for_name, create_version_index_from_folder, make_hasher,
};
use longtail_core::hash::{BLAKE2S_ID, BLAKE3_ID};
use longtail_testkit::corpus;
use longtail_testkit::paths::fixtures_dir;
use tokio_util::sync::CancellationToken;

/// Pin the process umask to 022 — the environment the committed fixtures were
/// generated under — so `fs::write`/`create_dir` default modes (and thus the
/// scanned `.lvi` permission bits) are deterministic regardless of the dev's
/// umask. Files/dirs with intentional explicit modes (`perms/`, chain
/// `script.sh`) are set via `chmod` and are unaffected by umask.
fn pin_umask_022() {
    // SAFETY: `umask` is a simple libc call with no memory safety concerns; this
    // is a single-test binary so the process-global change is contained.
    unsafe {
        libc::umask(0o022);
    }
}

struct Cell {
    /// Fixture `.lvi` relative to `fixtures/`.
    lvi: &'static str,
    /// Which source folder to build (see `build_source`).
    source: Source,
    hash_id: u32,
    comp: &'static str,
    target: u32,
}

enum Source {
    Zoo(fn() -> Vec<&'static str>),
    Subset(&'static [&'static str]),
    Chain(&'static str),
    ShardedUnion,
}

fn cells() -> Vec<Cell> {
    use corpus::entries::*;
    vec![
        Cell {
            lvi: "stores/comp-none/zoo.lvi",
            source: Source::Zoo(corpus::zoo_small),
            hash_id: BLAKE3_ID,
            comp: "none",
            target: 32768,
        },
        Cell {
            lvi: "stores/comp-lz4/zoo.lvi",
            source: Source::Zoo(corpus::zoo_small),
            hash_id: BLAKE3_ID,
            comp: "lz4",
            target: 32768,
        },
        Cell {
            lvi: "stores/comp-zstd_min/zoo.lvi",
            source: Source::Zoo(corpus::zoo_small),
            hash_id: BLAKE3_ID,
            comp: "zstd_min",
            target: 32768,
        },
        Cell {
            lvi: "stores/comp-zstd_max/zoo.lvi",
            source: Source::Zoo(corpus::zoo_small),
            hash_id: BLAKE3_ID,
            comp: "zstd_max",
            target: 32768,
        },
        Cell {
            lvi: "stores/comp-brotli/zoo.lvi",
            source: Source::Zoo(corpus::zoo_small),
            hash_id: BLAKE3_ID,
            comp: "brotli",
            target: 32768,
        },
        Cell {
            lvi: "stores/comp-brotli_text/zoo.lvi",
            source: Source::Zoo(corpus::zoo_small),
            hash_id: BLAKE3_ID,
            comp: "brotli_text",
            target: 32768,
        },
        Cell {
            lvi: "stores/chunk-1024/zoo.lvi",
            source: Source::Zoo(corpus::zoo_medium),
            hash_id: BLAKE3_ID,
            comp: "zstd",
            target: 1024,
        },
        Cell {
            lvi: "stores/chunk-131072/zoo.lvi",
            source: Source::Zoo(corpus::zoo_medium),
            hash_id: BLAKE3_ID,
            comp: "zstd",
            target: 131072,
        },
        Cell {
            lvi: "stores/blake2/zoo.lvi",
            source: Source::Zoo(corpus::zoo_small),
            hash_id: BLAKE2S_ID,
            comp: "zstd",
            target: 32768,
        },
        Cell {
            lvi: "stores/default/zoo.lvi",
            source: Source::Zoo(corpus::zoo_all),
            hash_id: BLAKE3_ID,
            comp: "zstd",
            target: 32768,
        },
        Cell {
            lvi: "stores/default/chain-v1.lvi",
            source: Source::Chain("v1"),
            hash_id: BLAKE3_ID,
            comp: "zstd",
            target: 32768,
        },
        Cell {
            lvi: "stores/default/chain-v2.lvi",
            source: Source::Chain("v2"),
            hash_id: BLAKE3_ID,
            comp: "zstd",
            target: 32768,
        },
        Cell {
            lvi: "stores/default/chain-v3.lvi",
            source: Source::Chain("v3"),
            hash_id: BLAKE3_ID,
            comp: "zstd",
            target: 32768,
        },
        Cell {
            lvi: "get-configs/version.lvi",
            source: Source::Subset(&[MIN_CHUNK, DUP_A]),
            hash_id: BLAKE3_ID,
            comp: "zstd",
            target: 32768,
        },
        Cell {
            lvi: "sharded/version.lvi",
            source: Source::ShardedUnion,
            hash_id: BLAKE3_ID,
            comp: "zstd",
            target: 32768,
        },
    ]
}

#[test]
fn lvi_byte_gate_all_cells() {
    pin_umask_022();
    let tmp = tempfile::tempdir().unwrap();
    let corpus_root = tmp.path().join("corpus");
    corpus::generate_all(&corpus_root);

    let pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap(),
    );
    let filter = RegexPathFilter::new(None, None).unwrap();
    let cancel = CancellationToken::new();

    // sharded fixture lives under stores/sharded/version.lvi
    let fx = fixtures_dir();

    let mut failures: Vec<String> = Vec::new();
    let mut passes = 0usize;
    let cells = cells();
    let total = cells.len();
    for cell in cells {
        let src = tmp
            .path()
            .join(format!("src-{}", cell.lvi.replace('/', "_")));
        build_source(&corpus_root, &src, &cell.source);

        let hasher = make_hasher(cell.hash_id).unwrap();
        let tag = compression_type_for_name(cell.comp).unwrap();
        let vi = create_version_index_from_folder(
            &src,
            &filter,
            hasher.as_ref(),
            cell.target,
            tag,
            &pool,
            &cancel,
            None,
        )
        .unwrap_or_else(|e| panic!("create version index for {}: {e:?}", cell.lvi));

        let fixture_path = fixture_lvi_path(&fx, cell.lvi);
        let expected = std::fs::read(&fixture_path)
            .unwrap_or_else(|e| panic!("read fixture {}: {e}", fixture_path.display()));
        let got = vi.to_bytes();
        if got == expected {
            passes += 1;
        } else {
            failures.push(format!(
                "{}: byte mismatch (got {} bytes, expected {} bytes)",
                cell.lvi,
                got.len(),
                expected.len()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        ".lvi byte-gate: {}/{} cells passed; failures:\n{}",
        passes,
        total,
        failures.join("\n")
    );
    assert_eq!(passes, total, "expected all {total} cells to pass");
}

fn fixture_lvi_path(fixtures: &Path, rel: &str) -> PathBuf {
    // Most cells are under stores/…; get-configs and sharded are at fixtures/….
    if rel.starts_with("get-configs/") {
        fixtures.join(rel)
    } else if rel.starts_with("sharded/") {
        fixtures.join("stores").join(rel)
    } else {
        fixtures.join(rel)
    }
}

fn build_source(corpus_root: &Path, dest: &Path, source: &Source) {
    match source {
        Source::Zoo(f) => corpus::copy_entries(corpus_root, dest, &f()),
        Source::Subset(names) => corpus::copy_entries(corpus_root, dest, names),
        Source::ShardedUnion => corpus::copy_entries(corpus_root, dest, &corpus::sharded_union()),
        Source::Chain(v) => {
            // The chain versions live at corpus_root/chain/{v}.
            let chain_dir = corpus_root.join("chain").join(v);
            copy_dir(&chain_dir, dest);
        }
    }
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), &to).unwrap();
        }
    }
}
