//! Self-validation suite.
//!
//! These tests prove the reference C library (driven ONLY through
//! `longtail-ffi`) deterministically reproduces every committed golden BEFORE
//! any pure-Rust port code exists. A red test here is a FINDING to report, not
//! something to tune away.
//!
//! The whole file compiles to nothing without the `differential` feature, so
//! the pure test lane never touches the native library.
#![cfg(feature = "differential")]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use longtail_ffi::{StoreIndex, StoredBlock, VersionIndex};

/// Serializes the downsync tests. Concurrent ffi downsyncs in one process are
/// not safe (shared C job-system state, plus a racy per-object `.lck` remove in
/// the fs blob store), so only one downsync runs at a time. Read-only tests
/// (round-trip, boundary) still run in parallel. Poison is ignored so one
/// failing test does not cascade.
static DOWNSYNC_LOCK: Mutex<()> = Mutex::new(());

fn downsync_guard() -> std::sync::MutexGuard<'static, ()> {
    DOWNSYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Recursively copy a directory tree. Used to give the downsync tests a private
/// copy of a committed store, since the C fs block store and the Rust fs blob
/// store both create lock files (`store.lsi.sync` / `.lck`) inside the store
/// dir even on read — the committed `fixtures/` must stay pristine.
fn copy_tree(src: &Path, dst: &Path) {
    for entry in walkdir::WalkDir::new(src).min_depth(1) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
        } else if entry.file_type().is_file() {
            if let Some(p) = target.parent() {
                std::fs::create_dir_all(p).unwrap();
            }
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Every file under `root`, regardless of extension.
fn collect_all_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root).min_depth(1).sort_by_file_name() {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            out.push(entry.path().to_path_buf());
        }
    }
    out
}
use longtail_testkit::boundary::{BoundaryTable, ChunkEntry, PATH_STREAMING};
use longtail_testkit::corpus;
use longtail_testkit::differential::{
    asset_chunks, boundary_table_buffer, boundary_table_streaming, downsync_version,
    read_version_index,
};
use longtail_testkit::paths::fixtures_dir;
use longtail_testkit::tree_manifest::TreeManifest;

fn collect_files(root: &Path, ext: &str) -> Vec<PathBuf> {
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

// Store-index files are named either `store.lsi`, `store_<sha>.lsi`, or
// `<name>-store.lsi` — all share the `.lsi` extension.
fn collect_lsi(root: &Path) -> Vec<PathBuf> {
    collect_files(root, "lsi")
}

/// §5.1 — every committed `.lvi` round-trips byte-identically through the C
/// reader+writer.
#[test]
fn round_trip_version_indexes() {
    let fixtures = fixtures_dir();
    let files = collect_files(&fixtures, "lvi");
    assert!(!files.is_empty(), "no .lvi fixtures found");
    let mut failures = Vec::new();
    for f in &files {
        let orig = std::fs::read(f).unwrap();
        let mut buf = orig.clone();
        let vi = match VersionIndex::new_from_buffer(&mut buf) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("{}: parse failed ({e})", f.display()));
                continue;
            }
        };
        let written = vi.write_to_buffer().expect("write version index");
        if written != orig {
            failures.push(format!(
                "{}: round-trip differs ({} -> {} bytes)",
                f.display(),
                orig.len(),
                written.len()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "version index round-trip failures:\n{}",
        failures.join("\n")
    );
    eprintln!("round-trip OK for {} .lvi files", files.len());
}

/// §5.1 — every committed `.lsi` (canonical, shard, and per-version) round-trips
/// byte-identically.
#[test]
fn round_trip_store_indexes() {
    let fixtures = fixtures_dir();
    let files = collect_lsi(&fixtures);
    assert!(!files.is_empty(), "no .lsi fixtures found");
    let mut failures = Vec::new();
    for f in &files {
        let orig = std::fs::read(f).unwrap();
        let si = match StoreIndex::new_from_buffer(&orig) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("{}: parse failed ({e})", f.display()));
                continue;
            }
        };
        let written = si.write_to_buffer().expect("write store index");
        if written != orig {
            failures.push(format!(
                "{}: round-trip differs ({} -> {} bytes)",
                f.display(),
                orig.len(),
                written.len()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "store index round-trip failures:\n{}",
        failures.join("\n")
    );
    eprintln!("round-trip OK for {} .lsi files", files.len());
}

/// §5.1 — a sample of committed `.lsb` stored blocks round-trip byte-identically.
#[test]
fn round_trip_stored_blocks() {
    let fixtures = fixtures_dir();
    // Sample across a compressed store and an uncompressed store.
    let mut lsb = Vec::new();
    for cell in ["default/store", "comp-none/store", "comp-brotli/store"] {
        let dir = fixtures.join("stores").join(cell).join("chunks");
        lsb.extend(collect_files(&dir, "lsb").into_iter().take(3));
    }
    assert!(!lsb.is_empty(), "no .lsb fixtures found");
    let mut failures = Vec::new();
    for f in &lsb {
        let orig = std::fs::read(f).unwrap();
        let mut buf = orig.clone();
        let sb = match StoredBlock::new_from_buffer(&mut buf) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("{}: parse failed ({e})", f.display()));
                continue;
            }
        };
        let written = sb.to_bytes().expect("write stored block");
        if written != orig {
            failures.push(format!("{}: round-trip differs", f.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "stored block round-trip failures:\n{}",
        failures.join("\n")
    );
    eprintln!("round-trip OK for {} sampled .lsb files", lsb.len());
}

/// §5.2 — re-running the C chunker reproduces every committed boundary table
/// exactly (both entry points). This is what Windows CI runs to prove the C
/// library chunks identically there.
#[test]
fn boundary_tables_reproduce() {
    let fixtures = fixtures_dir();
    let bdir = fixtures.join("boundaries");
    let tables = collect_files(&bdir, "json");
    assert!(!tables.is_empty(), "no boundary tables found");

    let chunker_input = std::fs::read(fixtures.join("chunker.input")).unwrap();

    let mut checked = 0;
    for path in &tables {
        let committed = BoundaryTable::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
        let data: Vec<u8> = if committed.input_id == "chunker.input" {
            chunker_input.clone()
        } else {
            corpus::case_bytes(&committed.input_id)
                .unwrap_or_else(|| panic!("no case bytes for {}", committed.input_id))
        };
        let recomputed = if committed.chunker_path == PATH_STREAMING {
            boundary_table_streaming(&committed.input_id, &data, committed.target_chunk_size)
        } else {
            boundary_table_buffer(&committed.input_id, &data, committed.target_chunk_size)
        };
        assert_eq!(
            recomputed,
            committed,
            "boundary table mismatch for {}",
            path.display()
        );
        checked += 1;
    }
    eprintln!("reproduced {checked} boundary tables");
}

/// §4 anchor — the streaming boundary table for each single-file corpus case
/// must match the chunk hashes+sizes golongtail recorded for that file in the
/// full-corpus `.lvi` (which golongtail produced via its own streaming path).
/// This proves the ffi-driven streaming chunker equals golongtail's streaming
/// output, independent of any regression against the committed tables.
#[test]
fn streaming_boundaries_match_lvi() {
    let fixtures = fixtures_dir();
    let zoo = read_version_index(&fixtures.join("stores/default/zoo.lvi"));

    let cases = [
        "big-stream",
        "compressible.txt",
        "incompressible.bin",
        "repetitive.bin",
        "win-47",
        "win-48",
        "win-49",
        "max-edge-minus",
        "max-edge",
        "max-edge-plus",
    ];
    for case in cases {
        let table_path = fixtures
            .join("boundaries")
            .join(format!("{}.t32768.streaming.json", case.replace('/', "_")));
        let table =
            BoundaryTable::from_json(&std::fs::read_to_string(&table_path).unwrap()).unwrap();

        let lvi_chunks = asset_chunks(&zoo, case)
            .unwrap_or_else(|| panic!("asset {case} not present in zoo.lvi"));
        let lvi_entries: Vec<ChunkEntry> = {
            let mut offset = 0u64;
            lvi_chunks
                .into_iter()
                .map(|(hash, size)| {
                    let e = ChunkEntry {
                        offset,
                        size,
                        hash_hex: format!("{hash:016x}"),
                    };
                    offset += size as u64;
                    e
                })
                .collect()
        };
        assert_eq!(
            table.chunks, lvi_entries,
            "streaming boundary table for {case} does not match zoo.lvi chunk hashes/sizes"
        );
    }
    eprintln!(
        "streaming boundary anchor confirmed against zoo.lvi for {} cases",
        cases.len()
    );
}

/// §3 compression caveat — the block tags actually written into each cell's
/// store index match the compression ID that cell requested (treating produced
/// tags as ground truth). Guards against the zstd_high/low→zstd_max aliasing and
/// any codec-name→ID drift.
#[test]
fn block_tags_match_compression() {
    let fixtures = fixtures_dir();
    // (cell, expected block tag). IDs from docs/format-spec.md §4.
    let cells: [(&str, u32); 8] = [
        ("comp-none", 0x0000_0000),
        ("comp-lz4", 0x6C7A_3432),         // "lz42"
        ("comp-zstd_min", 0x7A74_6431),    // "ztd1"
        ("comp-zstd_max", 0x7A74_6433),    // "ztd3"
        ("comp-brotli", 0x6274_6C31),      // "btl1" generic default
        ("comp-brotli_text", 0x6274_6C62), // "btlb" text default
        ("blake2", 0x7A74_6432),           // zstd default "ztd2"
        ("default", 0x7A74_6432),          // zstd default "ztd2"
    ];
    for (cell, expected) in cells {
        let bytes = std::fs::read(fixtures.join("stores").join(cell).join("store/store.lsi"))
            .unwrap_or_else(|e| panic!("read {cell} store.lsi: {e}"));
        let si = StoreIndex::new_from_buffer(&bytes).unwrap();
        let tags = si.get_block_tags();
        assert!(!tags.is_empty(), "{cell}: no blocks");
        for t in &tags {
            assert_eq!(
                *t, expected,
                "{cell}: block tag {t:#010x} != expected {expected:#010x}"
            );
        }
    }
    eprintln!(
        "block tags match requested compression for {} cells",
        cells.len()
    );
}

/// §5.3 — downsync every chain version (and the full zoo) from the default
/// store and assert the resulting tree matches the committed source-of-truth
/// manifest (mode-masked on non-Linux).
#[test]
fn downsync_reproduces_trees() {
    let _serial = downsync_guard();
    let fixtures = fixtures_dir();
    // Operate on a private copy so read-created lock files never touch fixtures/.
    let store_tmp = tempfile::tempdir().unwrap();
    let store = store_tmp.path().join("store");
    copy_tree(&fixtures.join("stores/default/store"), &store);
    let store_uri = store.to_string_lossy().into_owned();
    let mask_mode = !cfg!(target_os = "linux");

    let cases = [
        ("chain-v1", "chain-v1.json"),
        ("chain-v2", "chain-v2.json"),
        ("chain-v3", "chain-v3.json"),
        ("zoo", "zoo.json"),
    ];
    for (lvi_stem, manifest_name) in cases {
        let tmp = tempfile::tempdir().unwrap();
        let source_lvi = fixtures
            .join("stores/default")
            .join(format!("{lvi_stem}.lvi"));
        downsync_version(&store_uri, &source_lvi, tmp.path(), None, None)
            .unwrap_or_else(|e| panic!("downsync {lvi_stem}: {e}"));

        let got = TreeManifest::capture(tmp.path()).unwrap();
        let expected = TreeManifest::from_json(
            &std::fs::read_to_string(fixtures.join("manifests").join(manifest_name)).unwrap(),
        )
        .unwrap();
        got.compare(&expected, mask_mode)
            .unwrap_or_else(|e| panic!("tree mismatch for {lvi_stem}: {e}"));
    }
    eprintln!("downsync tree reproduction OK for {} versions", cases.len());
}

/// §5.3 — the cache-path variant: downsynced cache blocks are byte-identical to
/// the store's `.lsb` (the passthrough property).
#[test]
fn downsync_cache_blocks_are_passthrough() {
    let _serial = downsync_guard();
    let fixtures = fixtures_dir();
    // Operate on a private copy so read-created lock files never touch fixtures/.
    let store_tmp = tempfile::tempdir().unwrap();
    let store = store_tmp.path().join("store");
    copy_tree(&fixtures.join("stores/default/store"), &store);
    let store_uri = store.to_string_lossy().into_owned();

    let tmp = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let source_lvi = fixtures.join("stores/default/chain-v1.lvi");
    downsync_version(
        &store_uri,
        &source_lvi,
        tmp.path(),
        None,
        Some(cache.path()),
    )
    .expect("downsync with cache");

    // The cache blockstore is created with an empty block extension, so cache
    // blocks are `chunks/<top4>/0x<hash>` (no `.lsb`), while the store's are
    // `chunks/<top4>/0x<hash>.lsb`. Match by the `0x<hash>` stem and assert the
    // block bytes are byte-identical (the passthrough property).
    let cache_chunks = cache.path().join("chunks");
    let cache_blocks = collect_all_files(&cache_chunks);
    assert!(!cache_blocks.is_empty(), "cache produced no blocks");
    for cb in &cache_blocks {
        // Cache blocks are `0x<hash>.lrb`; store blocks are `0x<hash>.lsb`.
        let stem = cb.file_stem().unwrap().to_string_lossy().into_owned(); // "0x<hash>"
        let hexpart = stem.strip_prefix("0x").unwrap_or(&stem);
        let store_block = store
            .join("chunks")
            .join(&hexpart[..4])
            .join(format!("{stem}.lsb"));
        let cache_bytes = std::fs::read(cb).unwrap();
        let store_bytes = std::fs::read(&store_block)
            .unwrap_or_else(|e| panic!("store block {} missing: {e}", store_block.display()));
        assert_eq!(
            cache_bytes, store_bytes,
            "cache block {stem} differs from store"
        );
    }
    eprintln!(
        "cache passthrough OK: {} blocks byte-identical to store",
        cache_blocks.len()
    );
}

/// §5.4 — the synthesized sharded store (no canonical `store.lsi`, two
/// `store_<sha>.lsi` shards) is well-formed: downsync via the merge-on-read
/// (`fsblob://`) path succeeds and produces the correct tree.
///
/// Linux only. `fsblob://` is the one URI form that routes through the legacy
/// ffi crate's own blob store rather than C's — the other downsync tests here
/// pass a plain path — and that listing is not Windows-clean; it carries a
/// `// TODO: Windows strings may fail here` and returns an internal error
/// against a shard-only store. That is the retired oracle, not the port: our
/// merge-on-read is covered on Windows by
/// `crates/longtail/tests/downsync_e2e.rs::sharded_store_merge_on_read`, and
/// this fixture check still runs in full on Linux. Not worth repairing a crate
/// scheduled for deletion.
#[cfg_attr(
    windows,
    ignore = "legacy ffi blob-store listing is not Windows-clean; covered on linux"
)]
#[test]
fn sharded_store_reads() {
    let _serial = downsync_guard();
    let fixtures = fixtures_dir();
    // Operate on a private copy so read-created `.lck` files never touch fixtures/.
    let sharded_tmp = tempfile::tempdir().unwrap();
    let sharded = sharded_tmp.path().join("sharded");
    copy_tree(&fixtures.join("stores/sharded"), &sharded);
    let store_uri = format!("fsblob://{}", sharded.to_string_lossy());
    let source_lvi = sharded.join("version.lvi");

    let tmp = tempfile::tempdir().unwrap();
    downsync_version(&store_uri, &source_lvi, tmp.path(), None, None)
        .expect("downsync from sharded store");

    let got = TreeManifest::capture(tmp.path()).unwrap();
    let expected = TreeManifest::from_json(
        &std::fs::read_to_string(fixtures.join("manifests/sharded-union.json")).unwrap(),
    )
    .unwrap();
    got.compare(&expected, !cfg!(target_os = "linux"))
        .expect("sharded downsync tree mismatch");
    eprintln!("sharded merge-on-read downsync OK");
}
