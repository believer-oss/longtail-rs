//! `CreateVersionDiff` + `GetRequiredChunkHashes` (`Longtail_CreateVersionDiff`,
//! longtail.c:7493-7756; `Longtail_GetRequiredChunkHashes`, longtail.c:4349-4418).
//!
//! **Direction convention (unambiguous):** [`create_version_diff`] takes
//! `from` = C's `source_version_index` (the current on-disk / target-folder
//! index) and `to` = C's `target_version_index` (the desired version being
//! downsynced). golongtail calls `CreateVersionDiff(hash, targetVersionIndex,
//! sourceVersionIndex)` — i.e. `from = targetVersionIndex`, `to =
//! sourceVersionIndex` (cmd_downsync.go:248). `removed`/content-modified `source`
//! indexes point into `from`; `added`/`target` indexes point into `to`.

use std::collections::HashMap;
use std::collections::HashSet;

use crate::version_index::VersionIndex;

/// The set of changes to turn `from` into `to` (`Longtail_VersionDiff`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionDiff {
    /// Assets present in `from` but absent from `to` — to delete. Indexes into
    /// `from`. Sorted **long-to-short** by path length (deepest first).
    pub source_removed_asset_indexes: Vec<u32>,
    /// Assets present in `to` but absent from `from` — to create. Indexes into
    /// `to`. Sorted **short-to-long** (shallowest first, parents before kids).
    pub target_added_asset_indexes: Vec<u32>,
    /// Same-path, differing content-hash assets. Parallel arrays: entry `k`'s
    /// index in `from` / `to`.
    pub source_content_modified_asset_indexes: Vec<u32>,
    pub target_content_modified_asset_indexes: Vec<u32>,
    /// Same-path, differing permission assets. Parallel arrays.
    pub source_permissions_modified_asset_indexes: Vec<u32>,
    pub target_permissions_modified_asset_indexes: Vec<u32>,
}

/// Build a `path_hash -> asset_index` lookup (last wins on the theoretical
/// collision, matching C's `LookupTable_Put`).
fn path_hash_lut(vi: &VersionIndex) -> HashMap<u64, u32> {
    let mut lut = HashMap::with_capacity(vi.asset_count() as usize);
    for (i, &h) in vi.path_hashes.iter().enumerate() {
        lut.insert(h, i as u32);
    }
    lut
}

/// Path byte length per asset (incl. a directory's trailing `/`), for the sorts.
fn path_lengths(vi: &VersionIndex) -> Vec<u32> {
    (0..vi.asset_count() as usize)
        .map(|i| vi.path_bytes(i).map(|b| b.len() as u32).unwrap_or(0))
        .collect()
}

/// Compute the diff turning `from` into `to` (see module docs for the direction
/// convention). Pure; O((A₁+A₂)·log) via sorted-hash merge (longtail.c:7625).
pub fn create_version_diff(from: &VersionIndex, to: &VersionIndex) -> VersionDiff {
    let from_lut = path_hash_lut(from);
    let to_lut = path_hash_lut(to);
    let from_len = path_lengths(from);
    let to_len = path_lengths(to);

    // Sorted copies of the path-hash arrays (ascending unsigned — CompareHashes).
    let mut from_sorted = from.path_hashes.clone();
    from_sorted.sort_unstable();
    let mut to_sorted = to.path_hashes.clone();
    to_sorted.sort_unstable();

    let mut diff = VersionDiff::default();

    let (mut i, mut j) = (0usize, 0usize);
    while i < from_sorted.len() && j < to_sorted.len() {
        let sh = from_sorted[i];
        let th = to_sorted[j];
        if sh == th {
            let s_ai = from_lut[&sh];
            let t_ai = to_lut[&th];
            if from.content_hashes[s_ai as usize] != to.content_hashes[t_ai as usize] {
                diff.source_content_modified_asset_indexes.push(s_ai);
                diff.target_content_modified_asset_indexes.push(t_ai);
            }
            if from.permissions[s_ai as usize] != to.permissions[t_ai as usize] {
                diff.source_permissions_modified_asset_indexes.push(s_ai);
                diff.target_permissions_modified_asset_indexes.push(t_ai);
            }
            i += 1;
            j += 1;
        } else if sh < th {
            diff.source_removed_asset_indexes.push(from_lut[&sh]);
            i += 1;
        } else {
            diff.target_added_asset_indexes.push(to_lut[&th]);
            j += 1;
        }
    }
    while i < from_sorted.len() {
        diff.source_removed_asset_indexes
            .push(from_lut[&from_sorted[i]]);
        i += 1;
    }
    while j < to_sorted.len() {
        diff.target_added_asset_indexes.push(to_lut[&to_sorted[j]]);
        j += 1;
    }

    // Removed: long-to-short (length desc, tie larger-index-first) —
    // SortPathLongToShort (longtail.c:7386). Added: short-to-long (length asc,
    // tie smaller-index-first) — SortPathShortToLong (longtail.c:7346).
    diff.source_removed_asset_indexes.sort_by(|&a, &b| {
        let (la, lb) = (from_len[a as usize], from_len[b as usize]);
        lb.cmp(&la).then(b.cmp(&a))
    });
    diff.target_added_asset_indexes.sort_by(|&a, &b| {
        let (la, lb) = (to_len[a as usize], to_len[b as usize]);
        la.cmp(&lb).then(a.cmp(&b))
    });

    diff
}

/// The deduped set of chunk hashes needed to materialize the added +
/// content-modified assets, read from `to` (the desired version index — C's
/// `target_version_index`). Added-asset chunks first (in `target_added` order),
/// then content-modified-asset chunks, first-occurrence wins
/// (`Longtail_GetRequiredChunkHashes`, longtail.c:4349).
pub fn get_required_chunk_hashes(to: &VersionIndex, diff: &VersionDiff) -> Vec<u64> {
    let mut seen: HashSet<u64> = HashSet::new();
    let mut out: Vec<u64> = Vec::new();

    let mut collect = |ai: u32| {
        let ai = ai as usize;
        let start = to.asset_chunk_index_starts[ai] as usize;
        let count = to.asset_chunk_counts[ai] as usize;
        for k in 0..count {
            let cidx = to.asset_chunk_indexes[start + k] as usize;
            let h = to.chunk_hashes[cidx];
            if seen.insert(h) {
                out.push(h);
            }
        }
    };

    for &ai in &diff.target_added_asset_indexes {
        collect(ai);
    }
    for &ai in &diff.target_content_modified_asset_indexes {
        collect(ai);
    }
    out
}
