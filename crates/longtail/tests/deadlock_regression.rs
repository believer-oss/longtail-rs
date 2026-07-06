//! Stage 7a deadlock regression (CI-able, cheap): a full fixture downsync with
//! a prefetch budget far below the working set. Pre-Fix-1, `preflight_get`
//! acquired the WHOLE working set's budget before any block was consumed, so
//! any budget smaller than Σ block sizes parked forever (the Stage 6 1 GiB
//! deadlock, `rust-port-6-results.md` §1) — this test hangs pre-fix and
//! completes post-fix. Liveness invariant: any working set completes with ANY
//! budget ≥ 1 permit; the budget bounds prefetch memory, never progress.
//!
//! Linux-only (fixture manifests carry POSIX permissions); skipped under miri.

#![cfg(unix)]

use std::path::Path;
use std::time::Duration;

use longtail::{DownsyncOptions, downsync};
use longtail_testkit::paths::fixtures_dir;
use longtail_testkit::tree_manifest::TreeManifest;

fn pin_umask() {
    unsafe {
        libc::umask(0o022);
    }
}

fn zoo_manifest() -> TreeManifest {
    let p = fixtures_dir().join("manifests/zoo.json");
    TreeManifest::from_json(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// Hard timeout: generous vs the ~1 s fixture downsync, tiny vs a real hang.
const GUARD: Duration = Duration::from_secs(120);

/// Downsync the committed `default` fixture store with `budget`, guarded by a
/// hard timeout, and verify the resulting tree against the committed manifest.
async fn downsync_zoo_with_budget(target: &Path, budget: usize) {
    let fx = fixtures_dir();
    let mut opts = DownsyncOptions::new(
        vec![
            fx.join("stores/default/zoo.lvi")
                .to_string_lossy()
                .into_owned(),
        ],
        fx.join("stores/default/store")
            .to_string_lossy()
            .into_owned(),
        target.to_string_lossy().into_owned(),
    );
    opts.cache_target_index = false;
    opts.max_prefetch_bytes = Some(budget);

    tokio::time::timeout(GUARD, downsync(opts))
        .await
        .unwrap_or_else(|_| {
            panic!("downsync deadlocked with prefetch budget {budget} (Fix 1 regression)")
        })
        .expect("downsync");

    TreeManifest::capture(target)
        .unwrap()
        .compare(&zoo_manifest(), false)
        .expect("tree matches the committed zoo manifest");
}

/// The uncompressed size (Σ chunk_sizes — the budget-permit estimate) of the
/// largest block in the committed store index.
fn max_block_size_in_fixture_store() -> usize {
    let lsi = fixtures_dir().join("stores/default/store/store.lsi");
    let idx = longtail_core::StoreIndex::from_bytes(&std::fs::read(&lsi).unwrap()).unwrap();
    let mut max = 0usize;
    for b in 0..idx.block_count() as usize {
        let count = idx.block_chunk_counts[b] as usize;
        let offset = idx.block_chunks_offsets[b] as usize;
        let sz: usize = idx.chunk_sizes[offset..offset + count]
            .iter()
            .map(|&s| s as usize)
            .sum();
        max = max.max(sz);
    }
    assert!(max > 0, "fixture store index has no blocks");
    max
}

/// Variant 1: budget ≈ one block (1 MiB), working set = the whole zoo version
/// (Σ block sizes ≫ 1 MiB). Pre-fix this hangs in preflight; post-fix the
/// budget only throttles background prefetch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tiny_budget_downsync_completes() {
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    downsync_zoo_with_budget(&tmp.path().join("out"), 1024 * 1024).await;
}

/// Variant 2 (boundary): budget = exactly one max-size block, so at most one
/// background prefetch is dispatched at a time and every other block parks
/// until claimed or freed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_max_block_budget_downsync_completes() {
    pin_umask();
    let budget = max_block_size_in_fixture_store();
    let tmp = tempfile::tempdir().unwrap();
    downsync_zoo_with_budget(&tmp.path().join("out"), budget).await;
}
