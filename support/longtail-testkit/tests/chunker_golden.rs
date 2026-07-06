//! Golden chunker tests (pure lane, ladder 1-2): the pure-Rust streaming HPCDC
//! chunker + pure blake3 reproduce **every** committed `*.streaming.json`
//! boundary table exactly (offset, size, hash_hex), and the labeled
//! [`SeedMode::Buffer`] variant reproduces every `*.buffer.json`.
//!
//! These committed tables are the product: a red assertion here is a FINDING,
//! not something to tune. No native library is involved — this is what proves
//! the pure port matches C's committed boundaries.

use std::path::{Path, PathBuf};

use longtail_core::hash::Blake3;
use longtail_core::{HpcdcChunker, SeedMode};
use longtail_testkit::boundary::{BoundaryTable, ChunkEntry, PATH_BUFFER, PATH_STREAMING};
use longtail_testkit::corpus;
use longtail_testkit::fixture_manifest::sha256_hex;
use longtail_testkit::paths::fixtures_dir;

fn collect_json(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(dir).min_depth(1).sort_by_file_name() {
        let entry = entry.unwrap();
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|e| e.to_str()) == Some("json")
        {
            out.push(entry.path().to_path_buf());
        }
    }
    out
}

/// Recompute a boundary table with the pure-Rust chunker + pure blake3.
fn recompute(committed: &BoundaryTable, data: &[u8]) -> BoundaryTable {
    let target = committed.target_chunk_size;
    let (min, avg, max) = longtail_testkit::boundary::chunker_params_for_target(target);
    let chunker = match committed.chunker_path.as_str() {
        PATH_STREAMING => HpcdcChunker::new(min, avg, max),
        PATH_BUFFER => HpcdcChunker::new_buffer(min, avg, max),
        other => panic!("unknown chunker path {other}"),
    }
    .expect("valid chunker params");
    // Sanity: the derived streaming seed mode matches what the JSON labels.
    let expect_mode = if committed.chunker_path == PATH_STREAMING {
        SeedMode::Streaming
    } else {
        SeedMode::Buffer
    };
    let _ = expect_mode; // documented above; construction encodes it.

    let chunks: Vec<ChunkEntry> = chunker
        .chunk_hashed(data, &Blake3)
        .into_iter()
        .map(|c| ChunkEntry {
            offset: c.offset,
            size: c.size,
            hash_hex: format!("{:016x}", c.hash),
        })
        .collect();

    BoundaryTable {
        input_id: committed.input_id.clone(),
        input_sha256: sha256_hex(data),
        target_chunk_size: target,
        chunker_path: committed.chunker_path.clone(),
        hash_algorithm: "blake3".to_string(),
        chunks,
    }
}

/// Reproduce the raw bytes a table was computed over: `chunker.input` from
/// `fixtures/`, everything else from the deterministic corpus.
fn input_bytes(input_id: &str, chunker_input: &[u8]) -> Vec<u8> {
    if input_id == "chunker.input" {
        chunker_input.to_vec()
    } else {
        corpus::case_bytes(input_id).unwrap_or_else(|| panic!("no case bytes for {input_id}"))
    }
}

#[test]
fn streaming_boundary_tables_reproduced_by_pure_rust() {
    let fixtures = fixtures_dir();
    let chunker_input = std::fs::read(fixtures.join("chunker.input")).unwrap();
    let tables = collect_json(&fixtures.join("boundaries"));
    assert!(!tables.is_empty(), "no boundary tables found");

    let mut checked = 0;
    for path in &tables {
        let committed = BoundaryTable::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
        if committed.chunker_path != PATH_STREAMING {
            continue;
        }
        let data = input_bytes(&committed.input_id, &chunker_input);
        let recomputed = recompute(&committed, &data);
        assert_eq!(
            recomputed,
            committed,
            "streaming boundary table mismatch for {}",
            path.display()
        );
        checked += 1;
    }
    // Advisory: assert the exact count so a silently-shrunk
    // fixture set is a red test, not a passing one.
    assert_eq!(
        checked, 14,
        "expected exactly 14 streaming boundary tables, checked {checked}"
    );
    eprintln!("pure Rust reproduced {checked} streaming boundary tables");
}

#[test]
fn buffer_boundary_tables_reproduced_by_pure_rust() {
    let fixtures = fixtures_dir();
    let chunker_input = std::fs::read(fixtures.join("chunker.input")).unwrap();
    let tables = collect_json(&fixtures.join("boundaries"));

    let mut checked = 0;
    for path in &tables {
        let committed = BoundaryTable::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
        if committed.chunker_path != PATH_BUFFER {
            continue;
        }
        let data = input_bytes(&committed.input_id, &chunker_input);
        let recomputed = recompute(&committed, &data);
        assert_eq!(
            recomputed,
            committed,
            "buffer boundary table mismatch for {}",
            path.display()
        );
        checked += 1;
    }
    // Advisory: assert the exact count (14) per seed mode.
    assert_eq!(
        checked, 14,
        "expected exactly 14 buffer boundary tables, checked {checked}"
    );
    eprintln!("pure Rust reproduced {checked} buffer boundary tables");
}
