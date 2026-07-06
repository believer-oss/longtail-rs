//! Stage 7 byte-gates 1-4 through the **full pure-Rust upsync path** (temp
//! stores only — the committed `fixtures/` are read-only). For each cell:
//!
//! 1. `.lvi` byte-identical to the committed fixture (re-assert of gate ⑦
//!    through upsync, not just `create_version_index`).
//! 2. Block-name set identical: the `chunks/<4hex>/0x<hash>.lsb` names written
//!    equal the block hashes named by the committed version-local `.lsi`.
//! 3. Version-local `.lsi` (`merge(existing, missing)`) byte-identical to the
//!    committed `<cell>-store.lsi`.
//! 4. `comp-none` `.lsb` file bytes identical (tag 0 → no codec drift).
//!
//! Chain cells (v1→v2→v3 to one store) exercise the accumulation-order surface:
//! a chain gate-3 *byte* mismatch implicates Stage 4's deterministic block
//! accumulation order (vs golongtail's completion order), NOT packing. The test
//! asserts full byte-identity (it holds: the fixtures were generated with
//! --worker-count 1, so golongtail's accumulation was sequential and coincides
//! with merge(existing, missing) order) — but that is corpus-provenance-specific,
//! not a theorem for multi-worker-accumulated production stores; a chain-only
//! failure here still means "check accumulation order first".

#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use longtail::{UpsyncOptions, upsync};
use longtail_core::StoreIndex;
use longtail_testkit::corpus;
use longtail_testkit::paths::fixtures_dir;

fn pin_umask_022() {
    // SAFETY: single libc call; contained to this test binary.
    unsafe {
        libc::umask(0o022);
    }
}

fn hash_name(id: u32) -> &'static str {
    match id {
        longtail_core::hash::BLAKE3_ID => "blake3",
        longtail_core::hash::BLAKE2S_ID => "blake2",
        _ => panic!("unexpected hash id"),
    }
}

/// Collect the set of `0x<hash>.lsb` basenames present under `store/chunks`.
fn store_block_names(store_dir: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let chunks = store_dir.join("chunks");
    collect_lsb(&chunks, &mut out);
    out
}

fn collect_lsb(dir: &Path, out: &mut BTreeSet<String>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_lsb(&p, out);
            } else if p.extension().and_then(|x| x.to_str()) == Some("lsb")
                && let Some(name) = p.file_name().and_then(|n| n.to_str())
            {
                out.insert(name.to_string());
            }
        }
    }
}

/// Block-name set named by a committed `.lsi`.
fn lsi_block_names(lsi_path: &Path) -> BTreeSet<String> {
    let si = StoreIndex::from_bytes(&std::fs::read(lsi_path).unwrap()).unwrap();
    si.block_hashes
        .iter()
        .map(|h| format!("0x{h:016x}.lsb"))
        .collect()
}

struct Fresh {
    /// Committed cell dir under `fixtures/stores/`.
    cell: &'static str,
    source: fn() -> Vec<&'static str>,
    hash_id: u32,
    comp: &'static str,
    target: u32,
}

fn fresh_cells() -> Vec<Fresh> {
    use longtail_core::hash::{BLAKE2S_ID, BLAKE3_ID};
    vec![
        Fresh {
            cell: "comp-none",
            source: corpus::zoo_small,
            hash_id: BLAKE3_ID,
            comp: "none",
            target: 32768,
        },
        Fresh {
            cell: "comp-lz4",
            source: corpus::zoo_small,
            hash_id: BLAKE3_ID,
            comp: "lz4",
            target: 32768,
        },
        Fresh {
            cell: "comp-zstd_min",
            source: corpus::zoo_small,
            hash_id: BLAKE3_ID,
            comp: "zstd_min",
            target: 32768,
        },
        Fresh {
            cell: "comp-zstd_max",
            source: corpus::zoo_small,
            hash_id: BLAKE3_ID,
            comp: "zstd_max",
            target: 32768,
        },
        Fresh {
            cell: "comp-brotli",
            source: corpus::zoo_small,
            hash_id: BLAKE3_ID,
            comp: "brotli",
            target: 32768,
        },
        Fresh {
            cell: "comp-brotli_text",
            source: corpus::zoo_small,
            hash_id: BLAKE3_ID,
            comp: "brotli_text",
            target: 32768,
        },
        Fresh {
            cell: "chunk-1024",
            source: corpus::zoo_medium,
            hash_id: BLAKE3_ID,
            comp: "zstd",
            target: 1024,
        },
        Fresh {
            cell: "chunk-131072",
            source: corpus::zoo_medium,
            hash_id: BLAKE3_ID,
            comp: "zstd",
            target: 131072,
        },
        Fresh {
            cell: "blake2",
            source: corpus::zoo_small,
            hash_id: BLAKE2S_ID,
            comp: "zstd",
            target: 32768,
        },
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn upsync_byte_gates_fresh_cells() {
    pin_umask_022();
    let tmp = tempfile::tempdir().unwrap();
    let corpus_root = tmp.path().join("corpus");
    corpus::generate_all(&corpus_root);
    let fx = fixtures_dir();

    let mut failures: Vec<String> = Vec::new();
    for cell in fresh_cells() {
        let src = tmp.path().join(format!("src-{}", cell.cell));
        corpus::copy_entries(&corpus_root, &src, &(cell.source)());

        let store_dir = tmp.path().join(format!("store-{}", cell.cell));
        let target_lvi = tmp.path().join(format!("{}.lvi", cell.cell));
        let out_lsi = tmp.path().join(format!("{}.lsi", cell.cell));

        let mut opts = UpsyncOptions::new(
            src.to_str().unwrap(),
            store_dir.to_str().unwrap(),
            target_lvi.to_str().unwrap(),
        );
        opts.compression_algorithm = cell.comp.to_string();
        opts.hash_algorithm = hash_name(cell.hash_id).to_string();
        opts.target_chunk_size = cell.target;
        opts.version_local_store_index_path = Some(out_lsi.to_str().unwrap().to_string());
        upsync(opts)
            .await
            .unwrap_or_else(|e| panic!("upsync {} failed: {e:?}", cell.cell));

        let committed_lvi = fx.join(format!("stores/{}/zoo.lvi", cell.cell));
        let committed_lsi = fx.join(format!("stores/{}/zoo-store.lsi", cell.cell));
        let committed_store = fx.join(format!("stores/{}/store", cell.cell));

        // Gate 1: .lvi bytes.
        if std::fs::read(&target_lvi).unwrap() != std::fs::read(&committed_lvi).unwrap() {
            failures.push(format!("{}: GATE1 .lvi byte mismatch", cell.cell));
        }
        // Gate 2: block-name set == names from committed .lsi.
        let got_names = store_block_names(&store_dir);
        let want_names = lsi_block_names(&committed_lsi);
        if got_names != want_names {
            failures.push(format!(
                "{}: GATE2 block-name set mismatch (got {:?}, want {:?})",
                cell.cell, got_names, want_names
            ));
        }
        // Gate 3: version-local .lsi bytes.
        if std::fs::read(&out_lsi).unwrap() != std::fs::read(&committed_lsi).unwrap() {
            failures.push(format!("{}: GATE3 .lsi byte mismatch", cell.cell));
        }
        // Gate 4: comp-none .lsb bytes.
        if cell.comp == "none" {
            for name in &got_names {
                let sub = &name[2..6];
                let got = store_dir.join("chunks").join(sub).join(name);
                let want = committed_store.join("chunks").join(sub).join(name);
                if std::fs::read(&got).unwrap() != std::fs::read(&want).unwrap() {
                    failures.push(format!("{}: GATE4 .lsb {} byte mismatch", cell.cell, name));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "byte-gate failures:\n{}",
        failures.join("\n")
    );
}

/// `default/zoo` cell: its store also holds the chain blocks, so gate-2 uses the
/// committed `zoo-store.lsi` block set (exactly zoo's blocks). Fresh store here.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn upsync_byte_gate_default_zoo() {
    pin_umask_022();
    let tmp = tempfile::tempdir().unwrap();
    let corpus_root = tmp.path().join("corpus");
    corpus::generate_all(&corpus_root);
    let fx = fixtures_dir();

    let src = tmp.path().join("src-zoo-all");
    corpus::copy_entries(&corpus_root, &src, &corpus::zoo_all());
    let store_dir = tmp.path().join("store-zoo");
    let target_lvi = tmp.path().join("zoo.lvi");
    let out_lsi = tmp.path().join("zoo.lsi");

    let mut opts = UpsyncOptions::new(
        src.to_str().unwrap(),
        store_dir.to_str().unwrap(),
        target_lvi.to_str().unwrap(),
    );
    opts.version_local_store_index_path = Some(out_lsi.to_str().unwrap().to_string());
    upsync(opts).await.unwrap();

    let committed_lvi = fx.join("stores/default/zoo.lvi");
    let committed_lsi = fx.join("stores/default/zoo-store.lsi");
    assert_eq!(
        std::fs::read(&target_lvi).unwrap(),
        std::fs::read(&committed_lvi).unwrap(),
        "GATE1 default zoo .lvi"
    );
    assert_eq!(
        store_block_names(&store_dir),
        lsi_block_names(&committed_lsi),
        "GATE2 default zoo block set"
    );
    assert_eq!(
        std::fs::read(&out_lsi).unwrap(),
        std::fs::read(&committed_lsi).unwrap(),
        "GATE3 default zoo .lsi"
    );
}

/// Chain cells: sequential upsync v1→v2→v3 into one temp store. Gate 1 (.lvi)
/// and the block SET for each version's `.lsi` must match; gate-3 byte-identity
/// is reported (chain order depends on Stage 4 accumulation, see module docs).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn upsync_byte_gate_chain() {
    pin_umask_022();
    let tmp = tempfile::tempdir().unwrap();
    let corpus_root = tmp.path().join("corpus");
    corpus::generate_all(&corpus_root);
    let fx = fixtures_dir();

    let store_dir = tmp.path().join("store-chain");
    let mut gate3_byte_matches: Vec<(&str, bool)> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for v in ["v1", "v2", "v3"] {
        let src = corpus_root.join("chain").join(v);
        let target_lvi = tmp.path().join(format!("chain-{v}.lvi"));
        let out_lsi = tmp.path().join(format!("chain-{v}.lsi"));

        let mut opts = UpsyncOptions::new(
            src.to_str().unwrap(),
            store_dir.to_str().unwrap(),
            target_lvi.to_str().unwrap(),
        );
        opts.version_local_store_index_path = Some(out_lsi.to_str().unwrap().to_string());
        upsync(opts)
            .await
            .unwrap_or_else(|e| panic!("upsync chain {v}: {e:?}"));

        let committed_lvi = fx.join(format!("stores/default/chain-{v}.lvi"));
        let committed_lsi = fx.join(format!("stores/default/chain-{v}-store.lsi"));

        // Gate 1: .lvi bytes (must match).
        if std::fs::read(&target_lvi).unwrap() != std::fs::read(&committed_lvi).unwrap() {
            failures.push(format!("chain-{v}: GATE1 .lvi mismatch"));
        }
        // Gate 3 by SET (must match) + byte-identity (reported).
        let got = StoreIndex::from_bytes(&std::fs::read(&out_lsi).unwrap()).unwrap();
        let want = StoreIndex::from_bytes(&std::fs::read(&committed_lsi).unwrap()).unwrap();
        let got_set: BTreeSet<u64> = got.block_hashes.iter().copied().collect();
        let want_set: BTreeSet<u64> = want.block_hashes.iter().copied().collect();
        if got_set != want_set {
            failures.push(format!("chain-{v}: GATE3 block SET mismatch"));
        }
        let byte_match = std::fs::read(&out_lsi).unwrap() == std::fs::read(&committed_lsi).unwrap();
        gate3_byte_matches.push((v, byte_match));
        // Byte-identity holds empirically for the chain too (Stage 4's
        // deterministic block-hash-sorted accumulation matches the committed
        // fixture's order); assert it as the strong gate.
        if !byte_match {
            failures.push(format!(
                "chain-{v}: GATE3 .lsi byte mismatch (block SET matched — \
                 implicates Stage 4 accumulation order, not packing)"
            ));
        }
    }

    eprintln!("chain gate-3 byte-identity by version: {gate3_byte_matches:?}");
    assert!(
        failures.is_empty(),
        "chain byte-gate failures:\n{}",
        failures.join("\n")
    );
}

// Silence unused-import warnings under some cfgs.
#[allow(dead_code)]
fn _unused(_: PathBuf) {}
