# 04 · Performance, memory, and cross-crate invariants review
- **Reviewed at:** `456274d` · **Lead model:** opus · **Workers:** 3 × fable
- **Slice:** the benchmark instrument, the three perf documents, peak-memory drivers on both
  paths, invariants that cross a crate boundary, facade↔store backpressure, and release packaging ·
  **Confidence:** covered well on memory/packaging and the instrument; covered thinly on
  wall-clock claims (every number in the bench docs is a measurement I am not permitted to re-run).

## Findings index

| ID | P | Dimension | One line | file:line | Verdict |
|---|---|---|---|---|---|
| PERF-01 | P1 | hardening | The roadmap's "biggest win" (mtime/size target scan) is unimplementable from the `.lvi`, violates R7's I2, and its own numbers were measured in a config the product never uses | `docs/rust-port.md:196-200`, `downsync.rs:70-78` | CONFIRMED |
| PERF-02 | P2 | memory | Scan peak is `worker_count × min(largest_asset, target_chunk_size × 1024)`; `worker_count` is uncapped and `target_chunk_size` comes from the **source `.lvi`**, so a store can silently undo the streaming-chunker win | `version.rs:42,107-115,164-169` | CONFIRMED |
| PERF-03 | P2 | memory | `prune_blocks` still clones the whole union store index — the one `base.clone()` the download and upsync fixes left behind | `remote.rs:557` → `:757` | CONFIRMED |
| PERF-04 | P2 | perf | Freeing the union after `GetExistingContent` buys peak by re-downloading and re-parsing **every shard** at flush, once per CAS attempt | `remote.rs:701-709`, `sync.rs:286-288` | CONFIRMED |
| PERF-05 | P2 | memory | The GET path is `Vec<u8>` end-to-end: 2× wire size per fetch **and** per cache hit, 3× per cache miss. `Bytes` landed on the write side only | `block.rs:134`, `cache.rs:96,120` | CONFIRMED |
| PERF-06 | P2 | complexity | The `chunk_asset` range-split contract is documented on a function with **zero production callers**; the live path is a second copy in another crate with no equivalence test | `build.rs:24-25,33`, `version.rs:98` | CONFIRMED |
| PERF-07 | P2 | perf | The e2e harness does not re-run `prep()` before a retry, so a retried "cold" run measures a partially-populated target and is recorded as cold | `e2e.rs:139-153,557-568` | CONFIRMED |
| PERF-08 | P2 | perf | No `[profile.release]` exists: 883,401 B (6.6% of `.text`) of byte-identical duplicate symbols, and ≈6.3 MiB (23.8% of the binary) of strippable symbol tables — in a binary that ships inside a Tauri app | `Cargo.toml:1-13`, `10-release.txt:193,231` | CONFIRMED |
| PERF-09 | P2 | hardening | The harness reports medians of n=5 with no dispersion; a cell whose runs all exit non-zero prints `NaN` and is otherwise indistinguishable from a good cell | `e2e.rs:521-532,556-569,830-835` | CONFIRMED |
| PERF-10 | P2 | perf | `write_content` allocates each block payload with `Vec::new()` although the exact size is already computed two lines above | `upsync.rs:236,254,279` | CONFIRMED |
| PERF-11 | P2 | memory | Download peak has three unreconciled budgets; the only byte-denominated one is a fixed 512 MiB with no knob on any caller path | `remote.rs:70`, `apply.rs:182`, `options.rs:69-75` | CONFIRMED |
| PERF-12 | P2 | perf | CI never builds `--release` and no workflow has any cargo caching; every job recompiles the full AWS SDK tree | `.github/workflows/rust.yaml` | CONFIRMED |
| PERF-13 | P3 | hardening | The prefetch budget is denominated in decompressed bytes but bounds compressed memory — safe direction, undocumented, and it under-uses the budget by `1/ratio` | `remote.rs:519`, `store_index.rs:476` | CONFIRMED |
| PERF-14 | P3 | perf | Two full copies of the compressed body per put remain (frame prepend, `.lsb` serialize) | `compress.rs:339-342`, `block.rs:149` | CONFIRMED |
| PERF-15 | P3 | perf | 30 MB of committed fixtures is paid by every clone and every one of the six uncached CI jobs | `13-fixtures.txt` | CONFIRMED |

Documentation findings: `PERF-DOC-01` … `PERF-DOC-06`, in their own section. The three named
deliverables are answered in `## Deliverable` sections before the findings.

## Scope

**Read in full:** `support/longtail-bench/{Cargo.toml,src/lib.rs,src/bin/e2e.rs,src/bin/merge_mem.rs}`;
`docs/{bench-2026-08-03.md,put-path-memory.md}`; `crates/longtail/src/{options.rs,version.rs}`;
`crates/longtail/src/downsync.rs:1-239`; `crates/longtail/src/apply.rs:60-283`;
`crates/longtail/src/upsync.rs:1-305`; `crates/longtail-store/src/remote.rs:55-232,240-374,440-540,548-580,660-834`;
`crates/longtail-core/src/{block.rs:110-152,compress.rs:285-352,build.rs:1-120,pack.rs:55-150,store_index.rs:160-229}`;
root `Cargo.toml`; `.github/workflows/rust.yaml`.

**Skimmed:** `docs/bench-2026-07-05.md` (claim inventory delegated to a worker and spot-verified);
`docs/rust-port.md` §Roadmap + §Safety posture; `crates/longtail-store/src/{cache.rs:88-135,blob/fs.rs:300-335,sync.rs:270-300}`.

**Read but filed elsewhere:** the `## Findings index` of `docs/review/{01,02,03,07}-*.md`, and R3's
Deliverable 1 (admission-control contract) in full — my Deliverable 2 builds on it.

**Excluded:** the criterion micro-benches' *numbers* (I cannot run them; their setup is verified,
their results are not); everything under `support/longtail-{sys,ffi}`; the S3 credential path (R3);
`path_filter.rs` (R7 owns it — I cite only its binary-size contribution).

## Verification performed

Evidence-pack artifacts consulted: `MANIFEST.md`, `10-release.txt`, `17-bloat.txt`,
`17b-bloat-fn.txt`, `12-loc.txt`, `13-fixtures.txt`, `03b-bench-compile.txt`.

Two measurements are mine, not the pack's, and are labelled as such wherever cited:

1. **Duplicate-symbol census** over `target/release/longtail` —
   `nm -S --defined-only target/release/longtail | awk '($3=="t"||$3=="T"){s=strtonum("0x"$2); c[$4]++; z[$4]=s} END{for(k in c) if(c[k]>1) t+=(c[k]-1)*z[k]; print t}'`
   → **883,401 bytes across 3,614 redundant copies of 1,457 distinct mangled symbols.**
   Grouping is on the *mangled* name, so distinct monomorphizations that demangle alike are not
   conflated. **Provenance caveat:** the on-disk binary at review time (31,768,384 B, 23:14) is the
   `cargo bloat` rebuild, not the 27,811,160 B artifact `10-release.txt:193` recorded at 23:13.
   Both report the same `.text` (12.9 MiB / 13,461,319 B), so the ratio is comparable; the absolute
   file-size arithmetic below uses the pack's number only.
2. **Section census** via `readelf -S -W` on the same binary, cross-checked against `size -A` in
   `10-release.txt:195-231`.

**What I could not verify:** every wall-clock, throughput and peak-RSS number in the three perf
documents. They are measurements; I am not permitted to run cargo. What I *could* verify — and did,
for all of them — is whether the code path each number describes still exists in the shape the
document claims. Where it does not, the finding is stated as staleness of the *claim*, never as a
correction of the *number*. Four experiment requests at the end would close the remaining gaps.

---

## Deliverable 1 — invariants that cross a crate boundary

Each row: the invariant, where it is established, where it is consumed, and what enforces it.

| # | Invariant | Established | Consumed | Enforcement |
|---|---|---|---|---|
| I-A | `max_hash_size == target_chunk_size as u64 * 1024`, and the chunker must be `HpcdcChunker::from_target(target_chunk_size)` | `version.rs:41-42` (live), `build.rs:159-160` (dead) | `build.rs:33` `chunk_asset`, `version.rs:98` `chunk_asset_streaming` | **None.** Two independent parameters joined only by a doc comment. See PERF-06. |
| I-B | `per_asset_chunks[a]` aligns 1:1 with `file_infos` asset order | `version.rs:37-39` (sort, then `FileInfos::from_scanned_entries` over the *same* sorted vec) | `build.rs:73` `assemble_version_index` | Structural in the one live caller (the same `entries` feeds both), but the function accepts any slice and silently substitutes `&[]` for a short one (`build.rs:101`). Cross-ref `ALG-08`. |
| I-C | Block geometry: `DEFAULT_TARGET_BLOCK_SIZE` 8 MiB, `DEFAULT_MAX_CHUNKS_PER_BLOCK` 1024, `DEFAULT_MIN_BLOCK_USAGE_PERCENT` 80 | `upsync.rs:31-36` (facade) | `pack.rs:65` `create_store_index` via `upsync.rs:137-143`; the 80 goes to `get_existing_content` at `upsync.rs:133` | Values are plain `u32` parameters; core validates neither (`ALG-04`: zero is accepted where C returns `EINVAL`). The comment at `upsync.rs:34-36` correctly notes downsync passes 0 for the usage percent — that *is* the contract and it is written down. |
| I-D | The *download* side's per-block memory is set by the *upload* side's `target_block_size`, and by `target_chunk_size` read out of the source `.lvi` | Producer: `upsync.rs:141`. Consumer: `downsync.rs:107` → `version.rs:42`; `apply.rs:215-218` | `apply.rs:182` semaphore, `version.rs:115` scan buffer | **None, and no cap exists on either side.** This is PERF-02 and PERF-11. |
| I-E | `worker_count`/`remote_worker_count` size *four* things: the rayon pool (`version.rs:164-174`), the store's `worker_sem`, apply's block semaphore (`downsync.rs:144-145` → `apply.rs:182`), and — indirectly — the scan's peak memory (I-D) | `options.rs:54-57` | as above | Documented at `uri.rs:89-91` for the store/apply pair ("so callers can bound their own concurrency to the same value without introducing a second knob"). The **memory** consequence of the same number is documented nowhere. |
| I-F | Byte-identity of `merge_consuming` with `merge` (the shard name is `sha256(to_bytes())`) | `store_index.rs:332`+ | `sync.rs:175,227,293` | **Well enforced** — two proptests plus `sync_fixtures`. The best-guarded invariant in my slice. |

The two that need action are I-A (PERF-06) and I-D (PERF-02, PERF-11). I-C is a documentation gap
only: the constants are correct and correctly placed; nothing states that the block-geometry choice
made at upload time is what an end-user machine pays for at download time.

---

## Deliverable 2 — backpressure across the facade/store boundary

R3's admission-control contract (`docs/review/03-store-concurrency.md`, Deliverable 1) is the supply
side and I take it as given: two budgets, `worker_sem` item-denominated over *all* block I/O and
`prefetch_sem` byte-denominated over *background prefetch only*, with liveness guaranteed by
`worker_count ≥ 1` alone. R3's conclusion — the clamp bug class is closed — holds; I re-read
`remote.rs:286,519-520` and agree. **This section is the demand side only.**

**Can facade demand exceed store supply?** No — not in the liveness sense. Every facade request
either finds a dispatched entry to await or claims the hash and fetches inline
(`remote.rs:452-482`), and the inline path touches no byte budget. There is no demand shape that can
park behind the prefetch budget. The deadlock's structural cause is genuinely gone.

**What the facade *can* do is out-spend the store's accounting.** Three budgets bound download
memory, and no two are in the same unit:

| # | Budget | Unit | Value | Bounds |
|---|---|---|---|---|
| 1 | `prefetch_sem` | decompressed bytes | fixed 512 MiB (`remote.rs:70`) | compressed blocks fetched but not yet consumed |
| 2 | `worker_sem` | in-flight fetches | `resolved_worker_count` (8 for S3, per R3) | wire buffer + parsed copy, ≈2× wire size each |
| 3 | apply's `Semaphore` | in-flight block tasks | the *same* `resolved_worker_count` (`downsync.rs:144-145`) | one **decompressed** payload each (`apply.rs:215-218`) |

Peak block memory ≈ `512 MiB × ratio` + `W × 2 × wire_size` + `W × target_block_size`. With the
defaults (8 MiB blocks, zstd ≈0.5, W = 8) that is ≈ 256 + 64 + 64 = **384 MiB**, which is consistent
with the 594 MiB measured at w8 in `bench-2026-07-05.md:428` once the union index and allocator
overhead are added. The model checks out against the one number available to check it against.

Three properties of that formula are worth writing down because nothing in the tree does:

- **Term 3's coefficient is store-controlled.** `target_block_size` is chosen by whoever ran
  `upsync` (`upsync.rs:141`), is carried in the store index, and has no ceiling anywhere. A store
  packed at 64 MiB blocks multiplies the apply term by 8 on every downloader, including the Tauri
  app, with no diagnostic.
- **Term 1 has no knob on any caller path.** `DownsyncOptions::max_prefetch_bytes` exists but
  `options.rs:69-75` says in terms: *"Deliberately not exposed as a CLI flag"*, and the only
  non-test caller passes `None` (`downsync.rs:155-163`). 512 MiB is 12.5% of a 4 GiB laptop before
  the target scan, the union index, or the write plan.
- **Terms 2 and 3 share a number that means two different things.** `--remote-worker-count` is
  documented as I/O concurrency; raising it from 8 to 32 for a fast link also quadruples the
  decompressed payloads held in apply. `uri.rs:89-91` explains the coupling as a feature (one knob)
  and is right about concurrency; the memory half is unstated.

That is PERF-11. It is not a bug — it is an unwritten cost model for the product's primary path.

**One real gap in the enqueue shape**, distinct from R3's `STORE-08`: `apply.rs:84` passes the
*entire* retargetted store index to `preflight_get` in one call. The prefetch budget parks the
resulting fetches correctly, so this is bounded in bytes — but the *write plan* built at
`apply.rs:98-129` is materialized in full first, one `BlockWrite` (with a cloned path `String`,
`apply.rs:122`) per chunk occurrence across the whole download. For a 100 GB delta at 32 KiB chunks
that is ~3.2 M entries, ~48 B each plus a path clone, held for the entire apply. Nothing bounds it;
it is proportional to delta size, not to concurrency. Filed under lower-priority because at the
384 MiB–1 GiB scales measured it is under a megabyte, but it is the one download-side allocation
with no budget of any kind attached.

---

## Deliverable 3 — co-signing R7's `OPS-01`

**I co-sign it, and I own the roadmap item that would break it.**

R7's two invariants, re-verified independently:

- **I1 (cache-delete-before-mutate).** `downsync.rs:174-176` deletes `.longtail.index.cache.lvi`
  before `change_version2`; `downsync.rs:218-220` rewrites it only after the apply, the flush, and
  the optional validate have all returned `Ok`. Every `?` between those two points leaves no cache.
  Confirmed.
- **I2 (content-hash, not size+mtime).** The target index is built by
  `create_version_index_from_folder` (`downsync.rs:117-126`), which opens every asset
  (`version.rs:63`) and hashes every byte through `chunk_asset_streaming` (`version.rs:65,119-123`);
  `assemble_version_index` folds those chunk hashes into a per-asset content hash
  (`build.rs:106-110`). The diff at `downsync.rs:168` compares those. A torn file has the right size
  — `create_file_sized` pre-allocates it (R7's `OPS-10`) — and a *newer* mtime than the source,
  because this run created it. Confirmed.

`docs/rust-port.md:196-200` proposes: *"The fix is a streaming and/or mtime/size-short-circuiting
target scan (as golongtail does)."* Three separate problems, in ascending order of consequence.

**(a) Half of it already shipped.** The "streaming" half landed as `c13a4d1`
(`version.rs:86-128`). The roadmap still proposes it. Its RSS figure (≈250 MiB) was measured before
that commit.

**(b) The numbers were measured in a configuration the product does not use.** Both e2e download
arms pass `--no-cache-target-index` (`e2e.rs:396` for Rust, `:439` for golongtail). With the default
`cache_target_index: true` (`options.rs:100`) and a cache file present, `downsync.rs:72-78` supplies
the target index from the cache and **the scan does not run at all** (`downsync.rs:113-114` takes the
`effective_target_index` branch). So the "~97% of incremental wall" figure describes the uncached
path. In production the uncached path is reached in exactly two situations: the first download into
a folder, and **a re-run after a previous run failed or was cancelled** — because I1 deleted the
cache and the failed run never rewrote it. The optimization the roadmap calls "the biggest win" is
therefore worth almost nothing on the steady-state incremental path, and is worth its full 330 ms
precisely on the resume path — the one path where the content hash is load-bearing.

**(c) It cannot be built from the `.lvi` even if you wanted it.** `VersionIndex` carries
`path_hashes`, `content_hashes`, `asset_sizes`, chunk arrays, `name_offsets`, `permissions`,
`name_data` — and **no timestamp of any kind**. So an mtime short-circuit has nothing to compare
against. It would need a new sidecar recording `(path, size, mtime)` per asset, written during
apply. That sidecar is a second thing that must be durable-and-consistent with the target tree, i.e.
it re-creates the exact failure class I1 was designed to prevent, with a weaker oracle. Adding an
mtime field to the `.lvi` instead is a format change — `COMPAT-RISK`, and the wrong trade for a
cache-population optimization.

The parenthetical **"(as golongtail does)" is unsubstantiated in the sources available here**:
`support/longtail-sys/longtail/src/longtail.c` (the pinned v0.3.3-era submodule) contains zero
occurrences of `mtime`, `st_mtime`, `ModifiedTime` or `LastWriteTime`, and the `.lvi` golongtail
writes has no timestamp field to consult.

**What would have to be true to make it safe.** All three, not any one:

1. The short-circuit key must be a value that a *partial* write cannot forge. Size cannot be
   (pre-allocated). Mtime cannot be (the torn file is the newest thing in the tree).
2. Any sidecar carrying that key must be written with the same discipline as I1 — deleted before the
   first mutation, rewritten only after a fully successful run — which means it can only ever be a
   *second copy* of information the `.lvi` cache already provides, and therefore buys nothing on the
   path that has the cache and is unavailable on the path that does not.
3. There must be a regression test that cancels mid-apply, corrupts one asset in place while
   preserving its size and bumping its mtime, re-runs, and asserts the file is repaired. R7 notes
   the only existing resume test disables `cache_target_index`; no test covers this at all.

**Recommendation:** rewrite the roadmap bullet. Strike the mtime/size clause. Strike "streaming"
(done). Keep an honest remainder: *the uncached target scan costs ≈1.4 s per GiB and is reached on
first download and after a failed run; the tractable win is parallel-read tuning and avoiding the
second full scan under `--validate`, not weakening the completeness oracle.* That is PERF-01.

---

## Findings

### `PERF-01` — the roadmap's headline optimization is unimplementable, unsafe, and measured in the wrong configuration

**P1** · `hardening` · CONFIRMED · `COMPAT-RISK` (the `.lvi`-field variant only)

- **Where:** `docs/rust-port.md:196-200`; premises at `crates/longtail/src/downsync.rs:70-78,113-126,174-176,218-220`, `crates/longtail/src/version.rs:63-65`, `support/longtail-bench/src/bin/e2e.rs:396,439`
- **What:** A keeper document directs future work at an optimization that (a) is half already done,
  (b) is quantified from a benchmark configuration the product never runs in, and (c) would replace
  a content-hash completeness oracle with size+mtime — which a torn file satisfies by construction.
- **Failure scenario:** An engineer implements the roadmap item. A downsync is cancelled mid-apply
  (`apply.rs:189-191` stops launching; in-flight blocks complete, partially-written ones do not).
  I1 already deleted the cache, so the next run scans. The short-circuit sees the correct
  `asset_sizes[i]` (pre-allocated by `create_file_sized`) and a fresh mtime, marks the asset
  unchanged, and downsync reports success over a corrupt file. The corruption is silent and
  permanent — a subsequent run also short-circuits.
- **Evidence:** `VersionIndex` has no timestamp field (struct read in full via
  `support/longtail-bench/src/lib.rs:159-174` and `version_index.rs`); `grep -c 'mtime\|st_mtime' support/longtail-sys/longtail/src/longtail.c` → 0;
  `e2e.rs:396` passes `--no-cache-target-index` on the Rust leg of every download scenario.
- **Recommendation:** Rewrite the bullet as described in Deliverable 3. Separately, record I1 and I2
  as invariants in `docs/rust-port.md` next to the roadmap, so the next reader meets the constraint
  before the idea.
- **Tradeoff / risk:** None to the code. The cost is admitting that incremental downsync's biggest
  measured number applies to a configuration the CLI defaults away from.
- **Effort:** S (doc) / L (any safe version of the optimization).
- **Regression test to add:** cancel mid-apply with `cache_target_index` **on** (its default),
  truncate-and-refill one asset in place to preserve size and bump mtime, re-run, assert byte
  equality with the source. No existing test does this — R7 notes `smoke.rs:42` disables the cache.

### `PERF-02` — scan peak is `W × min(largest_asset, target_chunk_size × 1024)`, and a store controls both factors

**P2** · `memory` · CONFIRMED

- **Where:** `crates/longtail/src/version.rs:42` (`max_hash_size`), `:107-115` (the buffer),
  `:53-74` (one per rayon worker), `:164-174` (`build_pool`); source of `target_chunk_size` on the
  download path: `crates/longtail/src/downsync.rs:107`
- **What:** `chunk_asset_streaming` allocates `buf` per asset and `resize`s it to
  `min(asset_size, max_hash_size)` where `max_hash_size = target_chunk_size × 1024`. One such buffer
  is live per rayon worker. `build_pool(0)` — the default, since `worker_count: 0`
  (`options.rs:102`) — is `num_cpus::get()` with **no clamp**, unlike the store's worker count which
  R3 records as clamped to 8 for S3.
- **Failure scenario:** Two independent multipliers, neither capped.
  (i) *Thread count.* On a 32-thread build agent, the default scan holds 32 × 32 MiB = **1 GiB** of
  chunker buffers before any block I/O.
  (ii) *Chunk size.* `target_chunk_size` on downsync is read out of the **source `.lvi`**
  (`downsync.rs:107`), not chosen by the downloader. A version published with
  `--target-chunk-size 1048576` gives `max_hash_size` = 1 GiB, at which point
  `min(asset_size, max_hash_size)` degenerates to `asset_size` and the scan is back to holding whole
  assets per worker — exactly the pre-`c13a4d1` behaviour the streaming rewrite removed.
- **Evidence:** `version.rs:42` `(target_chunk_size as u64).saturating_mul(1024)`; `:115`
  `buf.resize(job_size, 0)` with `job_size = remaining.min(max_hash_size)` at `:113`; `:165-169`
  `num_cpus::get().max(1)` with no upper bound. `bench-2026-08-03.md:103-106` asserts the peak is
  "independent of asset size" — true only while `target_chunk_size × 1024 < largest asset`.
- **Recommendation:** Clamp the scan buffer to a fixed ceiling independent of `target_chunk_size`
  and read the part in sub-slices; the C part-boundary rule that makes streaming byte-identical
  constrains *chunk boundaries*, not read granularity, so a smaller read buffer inside the same part
  is still byte-identical. Failing that, clamp `build_pool`'s default the way
  `resolved_worker_count` clamps the store's, and state the `W × max_hash_size` formula in the
  `chunk_asset_streaming` doc comment (it currently says "peak memory is a single part", singular).
- **Tradeoff / risk:** Sub-slicing inside a part touches the byte-gate-critical path. Gate:
  `crates/longtail/tests/lvi_byte_gate.rs` + `upsync_byte_gate.rs` (unix-only per `ALG-01`, so this
  change is unguarded on Windows). The pool clamp alone is risk-free.
- **Effort:** S (clamp) / M (sub-slicing).
- **Regression test to add:** scan a tree with one asset larger than `max_hash_size` at
  `target_chunk_size = 1048576` and assert the chunk list equals the whole-buffer `chunk_asset`
  result — which also closes PERF-06.

### `PERF-03` — `prune_blocks` still clones the whole union index

**P2** · `memory` · CONFIRMED

- **Where:** `crates/longtail-store/src/remote.rs:557` → `:667` → `:744-761`, clone at `:757`
- **What:** `prune_blocks` calls `get_index_snapshot()`, which sends `IndexCommand::GetIndex`,
  which runs `merged_index`; with `added` empty — and the comment at `remote.rs:555-556` states it
  always is on this path — that returns `base.clone()`. `GetIndex` now has exactly one caller in the
  workspace, and it is this one. The identical clone was removed from the upsync path (`f2edb58`,
  `existing_content` at `:803-825`) and from the download prefetch path (`78c2c46`, `block_sizes` at
  `:768-792`). Prune was missed.
- **Failure scenario:** `prune-store-blocks` against a Fellowship-scale store. The owner holds the
  loaded union (≈1.3 GB, or ≈2.6 GB for the documented two-shard steady state), `source` is a full
  clone, `pruned` is a near-full subset, and `overwrite_remote_store_index` then serializes it — four
  index-sized allocations where three would do. On the 8 GB host `put-path-memory.md:17-19` records
  as the OOM machine, that is the difference between fitting and not.
- **Evidence:** `remote.rs:756-757` `if added.is_empty() { return Ok(base.clone()); }`;
  `rg 'get_index_snapshot|\.get_index\('` over `crates/` returns `remote.rs:557` as the only call
  site. Neither perf document mentions the residual — `bench-2026-08-03.md:117-127` describes the
  clone as removed.
- **Recommendation:** Serve prune inside the owner the way `existing_content` and `block_sizes`
  already are: an `IndexCommand::Prune { keep }` that computes `base.prune(&keep)` against the
  borrowed union and replies with the pruned subset only. That is the third instance of a pattern
  already established twice.
- **Tradeoff / risk:** `prune` is a destructive command (`OPS-03`, `STORE-01/02`); this changes only
  where the computation runs, not what it computes. Gate: `crates/longtail-store/tests/remotestore_spec.rs`.
- **Effort:** S.
- **Regression test to add:** a `merge_mem`-style peak assertion is not practical in the suite; instead
  assert structurally that `prune_blocks` issues no `GetIndex` (e.g. an owner-command counter in the
  test harness, which `actor_behavior.rs` is already shaped for).

### `PERF-04` — dropping the union after `GetExistingContent` is paid for with a full re-download at flush

**P2** · `perf` · CONFIRMED

- **Where:** `crates/longtail-store/src/remote.rs:701-709` (the drop), `:838-865` (`persist`),
  `crates/longtail-store/src/sync.rs:286-288` (`try_add` re-reads)
- **What:** After a ReadWrite store answers `GetExistingContent`, the owner sets `index = None` to
  free the union during block writes. The comment at `:701-706` justifies this by noting "persist
  re-reads from the backend". It does: `persist` → `add_to_remote_store_index` → `try_add`, whose
  first act is `read_store_store_index_with_items(client)` — a full re-list, re-download and re-parse
  of every `.lsi` shard.
- **Failure scenario:** An upsync of a small delta into a Fellowship-scale store reads and parses the
  entire ~2.6 GB two-shard union **twice** per successful run, and once more per CAS retry
  (`try_add` sits inside the retry ladder that `sync.rs:299-301` documents as retrying immediately on
  a lost CAS). On S3 that is egress and wall time, not just CPU; the flush is also the phase most
  likely to contend, so the retry multiplier applies exactly when the store is busiest.
- **Evidence:** `remote.rs:707-709` `index = None`; `sync.rs:287`
  `read_store_store_index_with_items(client).await?`; `put-path-memory.md:90-91` independently states
  "Every flush re-downloads all `.lsi` from S3 — no local caching". `bench-2026-08-03.md` measures
  peak RSS for this scenario and never reports the I/O.
- **Recommendation:** This is a deliberate memory-for-I/O trade and may well be the right one — but
  it is currently recorded as a pure win. Either (a) state the cost in the `remote.rs:701-706`
  comment and in `bench-2026-08-03.md`, or (b) keep the union and drop it *after* the flush's merge
  consumes it, which `merge_consuming` (`sync.rs:293`) now makes cheap. Do not do (b) blind — it
  raises peak by one union, which is the number the whole P1 series was about.
- **Tradeoff / risk:** (a) is free. (b) trades back the measured 843→433 win; needs the
  `merge_mem`-style measurement before it is considered.
- **Effort:** S for (a), M for (b).
- **Regression test to add:** none needed for (a). For any future (b), an assertion on the store's
  blob-read count across a full upsync (the `BlockStoreStats` counters at `remote.rs:343` already
  count block gets; shard reads are uncounted — adding that counter is itself worthwhile).

### `PERF-05` — the GET path is `Vec<u8>` end-to-end: 2× wire size per fetch and per cache hit, 3× per cache miss

**P2** · `memory` · CONFIRMED

- **Where:** `crates/longtail-core/src/block.rs:125` (`payload: Vec<u8>`), `:134`
  (`data[consumed..].to_vec()`); parse sites `crates/longtail-store/src/remote.rs:353` and
  `crates/longtail-store/src/cache.rs:96`; write-back `cache.rs:120` (`block.to_bytes()`)
- **What:** `StoredBlock::from_bytes` copies the entire payload out of the read buffer. The read
  buffer is still alive at that moment, so the transient is 2× the wire block. This happens on every
  remote fetch **and** on every local cache hit. On a cache *miss* the freshly fetched block is then
  re-serialized in full by `to_bytes()` purely to write it back — a third full copy.
- **Failure scenario:** Cost, not failure. A 384 MiB version at 8 MiB blocks and zstd ≈0.5 is 48
  blocks × ≈4 MiB wire. Per cold run: 192 MiB copied by the parse plus 192 MiB allocated-and-copied
  by the cache write-back. A fully warm run pays 192 MiB (fs read) + 192 MiB (parse) with no network
  at all — the warm-cache case is *entirely* memcpy. Scaled to the 1 GiB cell that is ≈1 GiB of
  avoidable copying per warm downsync.
- **Evidence:** `block.rs:134` read in full; `cache.rs:94-98` (read → `from_bytes` in one `&&`
  chain, both buffers live) and `cache.rs:120` `wb.write(block.to_bytes().into())`. The write side
  was converted to `Bytes` by `8e28d81` (`blob/mod.rs`, `fs.rs:332`, `s3.rs:398`); the read side was
  not — `BlobObject::read` still returns `Vec<u8>` (`fs.rs:306`, `s3.rs:359`).
- **Recommendation:** Complete the `8e28d81` change on the read side: `BlobObject::read -> Bytes`
  and `StoredBlock.payload: Bytes`. `from_bytes` then becomes `data.slice(consumed..)` — O(1) —
  and the cache write-back can hand the *original* wire `Bytes` to the cache object instead of
  re-serializing. That removes all three copies with no new abstraction; `bytes` is already a
  dependency of both crates.
- **Tradeoff / risk:** `StoredBlock.payload` is a public field of a `longtail-core` type; changing
  it is a breaking API change (`18-semver.txt`: no baseline exists yet, so now is the cheapest
  moment). No bytes on disk change. Gates: `crates/longtail-store/tests/blobstore_spec.rs`,
  `sync_fixtures.rs`, and the `.lsb` golden fixtures.
- **Effort:** M.
- **Regression test to add:** extend `blobstore_spec.rs` to assert `read` returns a `Bytes` whose
  slice is shared with the block payload (pointer identity via `Bytes::as_ptr` on a fs store).

### `PERF-06` — the range-split contract is documented on a function nothing calls, and duplicated in another crate with no equivalence test

**P2** · `complexity` · CONFIRMED

- **Where:** `crates/longtail-core/src/build.rs:24-25` (the contract), `:33-61` (`chunk_asset`),
  `:152-166` (`create_version_index`); the live implementation at
  `crates/longtail/src/version.rs:98-128`
- **What:** `build.rs:24-25` states the invariant: *"`chunker` must be
  `HpcdcChunker::from_target(target_chunk_size)`; `max_hash_size` must be
  `(target_chunk_size as u64) * 1024`"* — two parameters whose relationship the type system does not
  express. Since `c13a4d1` the facade no longer calls `chunk_asset`: it has its own
  `chunk_asset_streaming`, which re-derives `max_hash_size` at `version.rs:42` and re-implements the
  part loop at `:106-126`. `rg 'chunk_asset\b'` finds `chunk_asset`'s only caller to be
  `create_version_index` (`build.rs:163`), whose only callers are tests (`build_diff.rs:42`).
- **Failure scenario:** Two implementations of a byte-gate-critical algorithm, one of them
  unreachable from production, with nothing asserting they agree. A future edit to either — say,
  fixing the `max_hash_size.max(1)` guard, which both have at `build.rs:39` and `version.rs:105` —
  can land in one and not the other. The byte gates exercise only the facade copy, so the divergence
  is invisible until someone calls the core API (which is `pub` and exported at `lib.rs:63`) and gets
  different chunk boundaries, hence a different `.lvi`, hence a different `content_hash`, hence a
  spurious full re-download. `ALG-11` separately records that the range-split arithmetic has no
  direct test at all.
- **Evidence:** the `rg` results above; `version.rs:87-97` explicitly documents itself as
  "byte-identically to `longtail_core::chunk_asset`" — an equivalence claimed in prose and nowhere
  asserted.
- **Recommendation:** Two steps, both small. (1) Make the pair unforgeable: a
  `ChunkParams { chunker, max_hash_size }` in `longtail-core`, constructed only by
  `ChunkParams::from_target(target_chunk_size)`, taken by both functions. (2) Add the equivalence
  test: for a handful of asset sizes straddling `max_hash_size` boundaries, assert
  `chunk_asset(&buf, …) == chunk_asset_streaming(Cursor::new(&buf), …)`. That single test also
  discharges `ALG-11` and is the natural home for PERF-02's regression case.
- **Tradeoff / risk:** `chunk_asset`'s signature is public; changing it is a breaking change, again
  cheapest before a baseline exists. The behaviour does not change, so no compat gate is at risk.
- **Effort:** S.
- **Regression test to add:** the equivalence test above, parameterized over
  `{0, 1, max_hash_size-1, max_hash_size, max_hash_size+1, 3×max_hash_size}` bytes.

### `PERF-07` — the e2e harness does not re-prepare state before a retry, so retried "cold" runs are not cold

**P2** · `perf` · CONFIRMED

- **Where:** `support/longtail-bench/src/bin/e2e.rs:557-568` (`measure_cell`), `:139-153`
  (`run_watched`)
- **What:** `measure_cell` calls `prep()` once per iteration, then `run_watched`, which may invoke
  the command up to `MAX_ATTEMPTS = 3` times. `prep()` is not called between attempts. For the cold
  scenario `prep` is `rm(&target)` (`:644`); for incremental it is a full untimed v1 downsync
  (`:712-722`); for upsync it restores the pristine seed `store.lsi` (`:776-781`). A second attempt
  therefore runs against whatever the failed attempt left behind — a partially materialized target,
  or an already-merged store index — and if it succeeds, **its wall time is recorded as the cell's
  sample**.
- **Failure scenario:** The harness's own doc comment (`e2e.rs:27-30`) says retries exist because
  "the ffi `get_existing_store_index_sync` missed-wake race can hang a run". So the implementation
  most likely to retry is the one the Rust legs are being compared against, and its recorded samples
  are the ones most likely to be biased *downward* — a partially-populated target means fewer blocks
  to fetch and fewer bytes to write. Every three-way comparison in `bench-2026-07-05.md` §4/§9 rests
  on this loop.
- **Evidence:** `:559` `let t = run_watched(make, timeout, …);` with `prep()` at `:558` outside the
  attempt loop; `run_watched`'s loop at `:139-153` calls only `make()`.
- **Recommendation:** Pass `prep` into `run_watched` and call it at the top of each attempt. Two
  lines. Separately, record the attempt number alongside each sample so a contaminated cell is
  visible in the output table rather than silently averaged.
- **Tradeoff / risk:** None — it makes retried runs cost more wall-clock, which is the point.
- **Effort:** S.
- **Regression test to add:** not testable in the suite; instead assert in the report banner that
  `retries == 0` for every cell whose numbers are quoted in a document.

### `PERF-08` — there is no `[profile.release]`, and the cost is measurable in the shipped binary

**P2** · `perf` · CONFIRMED

- **Where:** root `Cargo.toml:1-13` (no `[profile.*]` section exists anywhere —
  `grep -rn '\[profile' --include=*.toml` over the tree returns nothing); measurements from
  `target/review-evidence/10-release.txt:193,209-212,231` and the census in §Verification performed
- **What:** The release profile is stock: `opt-level = 3`, `lto = false`, `codegen-units = 16`,
  `panic = "unwind"`, `strip = "none"`, `debug = false`. The binary ships inside a Tauri app.
- **Failure scenario:** Cost, quantified three ways.
  1. **Duplicate code across codegen units.** 883,401 bytes — **6.6% of the 13,461,319-byte
     `.text`** — are byte-identical symbols with the *same mangled name* emitted more than once;
     3,614 redundant copies of 1,457 symbols. The largest are our own async fns:
     `longtail::downsync::downsync::{{closure}}` at 21,819 B emitted 3×,
     `longtail::clonestore::clone_store::{{closure}}` at 16,090 B × 3,
     `longtail::upsync::upsync::{{closure}}` at 8,232 B × 3, plus `ZSTD_decompressSequences` × 4 and
     `HUF_decompress4X4` × 3. `codegen-units = 1` with `lto = "thin"` collapses this class.
  2. **Symbol tables.** `10-release.txt:193` records the artifact at 27,811,160 B; `:231` records
     the loadable total at 21,192,758 B. The ≈6,618,402-byte difference (**23.8% of the file**) is
     `.symtab` + `.strtab`, removable by `strip = "symbols"` at zero runtime cost — you lose
     symbolized backtraces, which a shipped desktop binary does not surface anyway.
  3. **Unwind tables.** `.gcc_except_table` 1,096,120 + `.eh_frame` 1,469,952 + `.eh_frame_hdr`
     225,452 = **2,791,524 B, 13.2% of the loadable image** (`10-release.txt:209-211`).
- **Evidence:** as cited. Independently, `17-bloat.txt:268-309` gives the per-crate split — `std`
  1.3 MiB, `aws_lc_sys` 1.3 MiB (of which `aes_gcm_{encrypt,decrypt}_avx512` alone are 332.0 KiB
  *each*, `17b-bloat-fn.txt:7-8`), `aws_sdk_s3` 966.2 KiB, our own `longtail` 772.8 KiB and
  `longtail_store` 621.7 KiB, `zstd_sys` 321.1 KiB, `brotli` 164.9 KiB.
- **Recommendation:** Add, and measure:
  ```toml
  [profile.release]
  lto = "thin"
  codegen-units = 1
  strip = "symbols"
  ```
  Those three are behaviour-preserving. `panic = "abort"` would reclaim most of item 3 but **is not
  behaviour-preserving here**: tokio currently converts a panicking spawned task into a `JoinError`
  (`apply.rs:225-230` handles exactly that), whereas `panic = "abort"` kills the process. Given
  `STORE-12` (no `catch_unwind` anywhere, so a codec panic already propagates), that may be the
  honest semantics — but it is a decision, not a profile tweak, and it must be taken deliberately.
- **Tradeoff / risk:** `codegen-units = 1` + thin LTO costs release build time (`10-release.txt:190`
  records 41.27 s today from cold deps; expect materially more). Since CI never builds `--release`
  (PERF-12) that cost currently lands only on whoever cuts the Tauri build.
- **Effort:** S to add, then measure.
- **Regression test to add:** a CI size budget — build `--release` and fail if the stripped binary
  exceeds a committed threshold. That single job also closes PERF-12's first half.

### `PERF-09` — the harness reports medians without dispersion, and a fully-failed cell is nearly indistinguishable from a good one

**P2** · `perf` · CONFIRMED

- **Where:** `support/longtail-bench/src/bin/e2e.rs:521-532` (`median`), `:556-569`
  (`measure_cell`), `:830-835` (the report row)
- **What:** Each cell reports the median of at most 5 samples and nothing else — no min/max, no
  spread, no sample list. Two consequences follow from `measure_cell`'s accounting:
  a run that fails with a non-zero exit after all 3 attempts is counted in **neither** `walls` nor
  `timeouts` (`:562-566` only handles `t.ok` and `t.timed_out`), so a cell where every iteration
  exited non-zero prints `n_ok = 0, timeouts = 0` and `median` of an empty slice — `f64::NAN`,
  rendered by `{:.0}` as `NaN`. Visible if you look; identical in shape to a real row if you skim.
- **Failure scenario:** `bench-2026-08-03.md:61-64` reports 843 → 433 → 305 MiB and asks the reader
  to believe a 433→305 step. With n=5 medians and no spread, a reader cannot distinguish that from
  noise — the document itself notes wall time "swings ~20% with ambient load" (`:55-57`) and offers
  no equivalent statement for RSS beyond "stable run-to-run". The instrument cannot substantiate
  either claim.
- **Evidence:** `:525` `v.sort_by(...)` then `:527-531` returns the middle element only; `:832-834`
  prints five `{:.0}` fields; `:522-524` returns `NAN` for an empty slice.
- **Recommendation:** Report min / median / max per cell (three more `{:.0}` fields, one line of
  code), and count non-zero-exit runs in a third column so a broken cell is loud. Neither changes
  what is measured.
- **Tradeoff / risk:** None.
- **Effort:** S.
- **Regression test to add:** none; this is instrument hygiene.

### `PERF-10` — `write_content` allocates each block payload unreserved although the exact size is computed two lines above

**P2** · `perf` · CONFIRMED

- **Where:** `crates/longtail/src/upsync.rs:254` (`let mut payload: Vec<u8> = Vec::new();`),
  `:279` (`payload.resize(start + cs, 0)`), `:236` (the same sum, already computed for progress)
- **What:** The per-block loop starts a fresh unreserved `Vec` and grows it one chunk at a time by
  `resize`. The block's exact final size is `Σ missing.chunk_sizes[off..off+count]`, available from
  `off`/`count` read at `:252-253` — and `:236` already sums that array wholesale for the progress
  denominator. Growing by doubling costs, amortized, roughly one extra full-payload memcpy per
  block.
- **Failure scenario:** Cost. Uploading 10 GiB of new content at the default 8 MiB blocks is ~1,280
  blocks; the avoidable realloc traffic is ~10 GiB of memcpy. It also raises transient peak: during
  a doubling realloc both the old and new buffers are live, so peak momentarily reaches ~1.5× the
  block payload on top of the compressor's output buffer — on the same path
  `put-path-memory.md` spent a sprint reducing.
- **Evidence:** `:254` and `:279` read in full; `:236`
  `let total_bytes: u64 = missing.chunk_sizes.iter().map(|&s| s as u64).sum();`.
- **Recommendation:** `let mut payload = Vec::with_capacity(exact);` where `exact` is the per-block
  slice sum. Note the zero-fill at `:279` is *not* avoidable in safe code — `read_exact` needs
  initialized memory — so this removes the realloc copy, not the memset. Say so in the commit
  message so the next reader does not chase the memset.
- **Tradeoff / risk:** None; identical bytes.
- **Effort:** S.
- **Regression test to add:** none needed (`upsync_byte_gate.rs` already covers the output).

### `PERF-11` — three unreconciled memory budgets on the download path, one of them fixed at 512 MiB with no knob

**P2** · `memory` · CONFIRMED

- **Where:** `crates/longtail-store/src/remote.rs:70` (512 MiB), `crates/longtail/src/apply.rs:182`
  (apply semaphore), `crates/longtail/src/downsync.rs:144-145` (both sized from the same number),
  `crates/longtail/src/options.rs:69-75` (the budget is deliberately not a CLI flag)
- **What:** See Deliverable 2 for the full derivation. Peak block memory is
  `512 MiB × compression_ratio` + `W × 2 × wire_size` + `W × target_block_size`, where `W` is
  `resolved_worker_count` and `target_block_size` is a property of whoever wrote the store.
- **Failure scenario:** A 4 GiB Windows laptop running the Tauri app against a store packed at
  64 MiB blocks: term 3 alone is 8 × 64 MiB = 512 MiB, term 1 is a further ~256 MiB, and the only
  mitigation available to the user is `--remote-worker-count`, which also throttles download
  throughput. There is no way to cap term 1 at all from the CLI or from `DownsyncOptions` as the
  facade uses it.
- **Evidence:** `remote.rs:70` `DEFAULT_MAX_PREFETCH_BYTES: usize = 512 * 1024 * 1024`;
  `options.rs:73-74` "Deliberately not exposed as a CLI flag"; `downsync.rs:160` passes
  `opts.max_prefetch_bytes`, which the CLI never sets. Model cross-checked against the 594 MiB
  measured at w8 in `bench-2026-07-05.md:428`.
- **Recommendation:** Two things, both cheap. (1) Write the formula down — in
  `DownsyncOptions`' doc comment, since that is where a Tauri integrator will look. (2) Expose the
  prefetch budget. The reason it is hidden ("correctness must never depend on this value") is
  exactly the reason it is *safe* to expose: R3's liveness invariant proves any budget ≥ 1 works.
  Hiding a knob that cannot break anything, on the one term the operator cannot otherwise influence,
  is the wrong default.
- **Tradeoff / risk:** Exposing the flag adds a CLI surface golongtail does not have; `OPS-06`
  already tracks CLI-surface divergence, so name it distinctly (e.g. `--max-prefetch-bytes`) and
  document it as a Rust-port extension in `docs/rust-port.md` §Deliberate divergences.
- **Effort:** S.
- **Regression test to add:** `crates/longtail/tests/deadlock_regression.rs:50` already drives the
  budget; extend it with a tiny-budget CLI-level case once the flag exists.

### `PERF-12` — CI never builds `--release` and no workflow caches anything

**P2** · `perf` · CONFIRMED

- **Where:** `.github/workflows/rust.yaml` — six jobs, all `actions/checkout@v4`, none with
  `actions/cache`, `Swatinem/rust-cache` or `sccache`, none passing `--release`
  (`grep -rn 'actions/cache\|rust-cache\|sccache\|--release' .github/workflows/*.yaml` returns
  nothing)
- **What:** The configuration that ships — `--release`, with whatever `[profile.release]` ends up
  saying — is never compiled by any gate. And every job rebuilds the entire dependency graph from
  scratch; `10-release.txt:3-188` shows that graph is 188 crates including the full AWS SDK,
  `aws-lc-sys` (which runs cmake), and `zstd-sys`.
- **Failure scenario:** (i) A change that only breaks under optimization — an `opt-level`-sensitive
  UB, an LTO-exposed symbol conflict, or simply a release-only compile error — reaches the Tauri
  build unchallenged. This is not hypothetical: the moment PERF-08's profile lands, `codegen-units = 1`
  + LTO is a compilation mode nothing in CI exercises. (ii) Six jobs × a cold 188-crate build on
  every PR is the dominant cost of the pipeline, and it is pure waste.
- **Evidence:** as cited. Note `01b-clippy-ws.txt`'s MANIFEST caveat makes the same point from the
  other direction: the observed 13 s is a warm-cache artifact that CI can never reproduce.
- **Recommendation:** Add `Swatinem/rust-cache` to every job, and add one `cargo build --release -p longtail-cli`
  step with the binary-size budget from PERF-08. R8 owns CI generally — this is filed here because
  the mission assigns me the release-build gap specifically; defer to R8 on placement.
- **Tradeoff / risk:** Cache poisoning across toolchain bumps; `rust-cache` keys on the toolchain, so
  low.
- **Effort:** S.
- **Regression test to add:** the size budget is the test.

### `PERF-13` — the prefetch budget's unit is not the unit of the memory it bounds

**P3** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/remote.rs:519` (`estimate` from `block_sizes`),
  `crates/longtail-core/src/store_index.rs:476` (`block_payload_sizes` = Σ chunk sizes = the
  **decompressed** size), against `remote.rs:315-321` (the entry holds a `StoredBlock` whose
  `payload` is the **compressed** frame, because `RemoteBlockStore` sits *below* `CompressBlockStore`
  in the `Compress(Cache(Remote))` stack, `uri.rs:105-106`)
- **What:** `max_prefetch_bytes` is spent in decompressed bytes and holds compressed bytes. The
  error is conservative — 512 MiB of budget holds ≈512 × ratio MiB of RAM — so it is not a leak. It
  does mean the budget systematically under-admits by `1/ratio`: on a zstd store at ratio 0.5 the
  prefetcher runs at half the intended depth, and the constant named "512 MiB of prefetch memory"
  describes neither the memory nor the prefetch depth.
- **Failure scenario:** No failure; a silent throughput ceiling and a constant whose name misleads
  the next person tuning it.
- **Evidence:** `remote.rs:781` `base.block_payload_sizes(block_hashes)`, whose doc at
  `store_index.rs:476` is Σ chunk sizes; `remote.rs:327` stores the result of `fetch_stored_block`,
  which parses the raw `.lsb` (`:353`) with no decode.
- **Recommendation:** Do not change the accounting — conservative is right for a memory budget. Add
  one sentence at `remote.rs:70` saying the budget is denominated in decompressed size and therefore
  over-accounts compressed stores by `1/ratio`. Cross-ref R3's `STORE-10` (the cache byte budget) —
  these are the tree's two byte budgets and neither states its unit precisely.
- **Tradeoff / risk:** None.
- **Effort:** S.
- **Regression test to add:** none.

### `PERF-14` — two full copies of the compressed body remain on every put

**P3** · `perf` · CONFIRMED

- **Where:** `crates/longtail-core/src/compress.rs:339-342` (frame prepend),
  `crates/longtail-core/src/block.rs:142-151` (`to_bytes`, the copy at `:149`), reached from
  `crates/longtail-store/src/remote.rs:398`
- **What:** `encode_block_payload` allocates an exactly-sized output and copies the whole compressed
  body into it purely to prepend an 8-byte header; `StoredBlock::to_bytes` then copies that framed
  payload again to concatenate it after the block index. With `--compression-algorithm none` there is
  a *third*: `compress.rs:335` `raw.to_vec()`.
- **Failure scenario:** Cost. At 8 MiB blocks and ratio 0.5 that is ~8 MiB copied per block beyond
  the compression itself; the serial put loop (`upsync.rs:248-297`) means it is on the critical path,
  not overlapped.
- **Evidence:** all three sites read in full. `put-path-memory.md:72-77` counted three such copies
  and `8e28d81` removed one (the S3 body `to_vec`); the remaining two are as described.
- **Recommendation:** Reserve `FRAME_HEADER_SIZE` at the front of the codec's output buffer and fill
  it in place, or return `(header, body)` and let `to_bytes` write both — either removes one copy.
  Removing the second needs `payload: Bytes` (PERF-05), so sequence them.
- **Tradeoff / risk:** `COMPAT-RISK` is low but nonzero — this rewrites the bytes of a `.lsb`
  payload's assembly, not its content. Gate: the `.lsb` golden fixtures and
  `crates/longtail/tests/upsync_byte_gate.rs`.
- **Effort:** S.
- **Regression test to add:** covered by the existing byte gates.

### `PERF-15` — 30 MB of committed fixtures on six uncached CI jobs

**P3** · `perf` · CONFIRMED

- **Where:** `fixtures/` — `13-fixtures.txt` records **112 files, 29.09 MiB** verified, 30 MB on
  disk, 114 tracked paths; six jobs in `.github/workflows/rust.yaml`, none cached
- **What:** Every clone and every CI checkout transfers 30 MB of binary fixtures. With no caching
  (PERF-12) this is paid six times per PR on top of the full dependency rebuild.
- **Failure scenario:** Cost only. Worth stating because the fixtures are load-bearing for
  compatibility (`FMT-008`, `ALG-05`) and must not be deleted — the fix is caching and, if it ever
  matters, Git LFS, not fewer fixtures.
- **Evidence:** `13-fixtures.txt`; `du -sh fixtures` → 30M.
- **Recommendation:** Fix PERF-12 first; re-measure before doing anything fixture-specific.
- **Tradeoff / risk:** LFS would complicate the `xtask verify-fixtures` flow; do not pursue unless
  measurement justifies it.
- **Effort:** S.
- **Regression test to add:** none.

## Lower-priority observations

- **Write-plan size is unbounded by anything.** `apply.rs:98-129` materializes one `BlockWrite`
  (48 B + a cloned path `String`, `apply.rs:122`) per chunk *occurrence* across the whole download,
  before any block is fetched. Proportional to delta size, not to concurrency; ~3.2 M entries for a
  100 GB delta at 32 KiB chunks. Under a megabyte at the scales measured, so no finding — but it is
  the only download allocation with no budget attached, and `apply.rs:101` + `:152` each build the
  same path `String` a second and third time for the same asset.
- **`version.rs:39` `entries.clone()`** deep-clones the whole scan list (one `String` per entry)
  only because `entries` is still needed for the `par_iter` at `:53`. Cross-ref `ALG-17`, which owns
  it; `FileInfos::from_scanned_entries` taking `&[FileEntry]` would remove it, and it runs twice
  under `--validate`.
- **`pack.rs:123`** `StoreIndex::from_block_indexes(&blocks)` copies every block's chunk arrays into
  the packed index while `blocks` is still live — a transient 2× the missing-content chunk arrays
  (~16 MB at 650 k missing chunks). Small, but it is the same "hold both representations" shape
  `4583371` fixed for the union merge.
- **`upsync.rs:220` and `apply.rs:92`** build large `HashMap`s with no capacity hint although the
  bound is known (`version_index.chunk_hashes.len()` and the retargetted index's chunk count).
  ~48 B/entry × ~650 k = ~31 MB each, plus rehash traffic.
- **`upsync.rs:174`** uses the allocating `merge` for the version-local `.lsi` even though `existing`
  is dead immediately afterwards — `merge_consuming` applies here too and was not wired in by
  `4583371` (which wired `sync.rs:175,227,293`).
- **`support/longtail-bench` is a default member**, so `cargo build` builds `merge_mem`
  (`Cargo.toml:97-99`, no `required-features`). `e2e` and `ffi-driver` are gated on `differential`
  (`:74-86`), which is the relevant precision for R6's safety-posture claim — `docs/rust-port.md:218`
  says the `libc` blocks live in "a binary target that the library's `forbid` does not cover", which
  is true, but omits that the target is not built by default. Filed to R6.
- **`aws_lc_sys` contributes 1.3 MiB of `.text`** (10.0%), of which two AVX-512 AES-GCM functions are
  332.0 KiB each (`17b-bloat-fn.txt:7-8`). Reported as a fact; the contract forbids recommending a
  dependency swap without a security or correctness reason, and there is none.
- **`brotli`'s encoder is compiled in** and partly monomorphized into our crate:
  `brotli::enc::block_splitter::BrotliSplitBlock` (51.8 KiB) and
  `brotli::enc::backward_references::BrotliCreateBackwardReferences` (26.9 KiB) are attributed to
  `longtail_core`, i.e. ~25% of `longtail_core`'s 316.3 KiB is brotli encoder code. It is needed —
  `--compression-algorithm brotli` must encode — so this is a fact, not a recommendation.
- **`regex` costs ~840 KiB** (`regex_automata` 437.9 + `regex_syntax` 225.6 + `aho_corasick` 176.7).
  `crates/longtail/Cargo.toml` takes `regex = "1"` with default features for `path_filter.rs`;
  `regex-lite` is *already in the tree* at 54.9 KiB via the AWS SDK. R7 owns `path_filter.rs` and
  `OPS-19` records it at 13.7% coverage with zero CLI tests — changing its engine before it is
  tested would be reckless, so this is a note for after `OPS-19` is discharged.

## Comments & documentation issues

### `PERF-DOC-01` — `store_index.rs`: the `Longtail_MergeStoreIndex` byte-identity contract now documents a private helper, and `merge` has no docs at all

**P2** · `idiom` · CONFIRMED

- **Where:** `crates/longtail-core/src/store_index.rs:189-227`
- **What:** Lines 189-216 are the doc comment for `merge` — the C line-by-line derivation of hash
  identifier, error, block order/dedup, offset rebuild and version, prefaced by "**byte-for-byte on
  the success path** (the S3 store-index shard name is the sha256 of these bytes, so byte-identity is
  load-bearing)". Lines 217-219 append three more `///` lines describing `reserve_capacity`, and
  `fn reserve_capacity` follows at `:220`. The whole block therefore documents the private helper.
  `pub fn merge` at `:229` has **zero** doc comment.
- **Evidence:** read in full; `2cce0cd` is the commit that inserted `reserve_capacity`
  (`git show --stat 2cce0cd` → `store_index.rs | 22 ++++++++`).
- **Recommendation:** Move `:217-219` to immediately above `:220` and restore `:189-216` above
  `:229`. This is the single most compat-critical doc comment in `longtail-core` and rustdoc
  currently renders it on an invisible helper. Related: `MANIFEST.md`'s pre-identified rustdoc defect
  #2 (public `merge_consuming` docs linking to private `Self::is_canonical`) is the same commit
  family's doc handling.
- **Effort:** S.

### `PERF-DOC-02` — `put-path-memory.md` contradicts itself and is stale from line 8 to line 216

**P2** · `complexity` · CONFIRMED

- **Where:** `docs/put-path-memory.md:8-9` and `:213-216` versus `:218-254`
- **What:** The preamble states, unqualified and in the present tense, "**there are no upsync
  benchmarks in the repo yet — measuring alongside any fix work is a prerequisite for trusting these
  estimates**", repeated at `:213-216` ("the upload path does not [have benchmarks]"). The appended
  Resolution section cites measurements from exactly such benchmarks — the `upsync` arm of
  `e2e.rs:739-801` and `merge_mem.rs`, both committed. Worse, the entire body between them is
  pre-fix: `:46-47` says items 3 and 4 are "Not done" (both landed); `:140-154` presents the
  whole-file-read OOM vector in the present tense under the heading "New OOM vector unique to the
  Rust port" (both vectors closed by `c703b32`/`c13a4d1`); `:170-193` prices a two-day sprint that
  has already happened; `:209-211` says "One place the port is currently **worse** than golongtail"
  about a fixed problem. Line `:11` pins the document to `@a459c3b`, **11 commits behind HEAD**.
- **Failure scenario:** A reader — the intended audience is a go/no-go decision-maker — reads the
  first two thirds and concludes the port has an unfixed multi-GB-pak OOM vector and an unfixed
  5.2 GB flush peak. Both are wrong at HEAD. The Resolution that corrects them is 218 lines in.
- **Recommendation:** This document has served its purpose. Fold the surviving content — the
  two-file steady-state explanation (`:21-36`, which is genuinely useful and not recorded elsewhere)
  and the Resolution table — into `docs/rust-port.md`, and delete the rest. `CLAUDE.md` already
  classes everything outside the four keeper docs as "slated for deletion or folding-in"; this is the
  clearest instance. If it must survive as a record, move the Resolution to the top and mark every
  section below it `[HISTORICAL — pre-fix]`.
- **Effort:** S.

### `PERF-DOC-03` — `bench-2026-07-05.md`'s incremental-downsync explanation describes code that no longer exists

**P2** · `complexity` · CONFIRMED

- **Where:** `docs/bench-2026-07-05.md:244-248` and `:470-474`
- **What:** Both passages attribute the incremental cell's cost to "the target scan reads+chunks
  whole assets, so the single 256 MiB asset is held in memory" and "the whole-asset target rescan in
  `version.rs`". `c13a4d1` replaced that scan with `chunk_asset_streaming`
  (`version.rs:86-128`), whose peak is one `max_hash_size` part per worker. The RSS figures for those
  cells (276 MiB at `:230`, 295 MiB at `:449`) were measured against the old code and were never
  re-measured. Same issue at `:402-429`: §9.1's GET RSS of 594 MiB predates both `78c2c46` and
  `8e28d81`, each of which reduces memory on that path.
- **Recommendation:** Annotate the affected cells with the commit they predate, or re-run §9.2 and
  §9.1 (experiment #2 below). Do not silently keep quoting them — `docs/rust-port.md:196-200` already
  quotes the 250 MiB figure downstream, which is how PERF-01 got its stale premise.
- **Effort:** S to annotate, M to re-measure.

### `PERF-DOC-04` — `bench-2026-08-03.md`'s `merge_mem` result cannot be reproduced from the document

**P2** · `hardening` · CONFIRMED

- **Where:** `docs/bench-2026-08-03.md:160-169` versus `support/longtail-bench/src/bin/merge_mem.rs:14-15,25,42`
- **What:** The document names the tool ("`longtail-bench --bin merge_mem`") and reports two
  332 MiB/shard indexes producing a 6,000,000-block union. The bin's `blocks` argument **defaults to
  2,000,000** (`:25`), which its own size formula (`:42`, `16 + blocks*20 + blocks*cpb*12`) puts at
  ~200 MiB/shard and a 4 M-block union. Reproducing the published numbers requires
  `merge_mem consuming 3000000`, which the document never states. Secondary: `:168` says
  "**−331 MiB**" while its own table (`:165-166`) differs by 332.
- **Recommendation:** State the full command. The neighbouring §3 does exactly this (`:40-51`) and is
  the model.
- **Effort:** S.

### `PERF-DOC-05` — `bench-2026-08-03.md`'s real-S3 comparison has no stated method

**P2** · `hardening` · CONFIRMED

- **Where:** `docs/bench-2026-08-03.md:139-142`
- **What:** "rust ~5.6 GB median vs go ~6.4 GB, and rust's *worst* run beat go's *best*" — no sample
  count, no host, no command line, no date. It is the only production-scale number in either bench
  document and the only evidence for the claim that the read path is already below golongtail; it is
  also the one used to size the `merge_consuming` payoff at `:170-172`. Every other number in the
  document states its method; this one does not.
- **Recommendation:** Add n, host, and the exact `validate-version` invocation, or demote the
  paragraph to "an unrepeated observation" and stop deriving from it.
- **Effort:** S.

### `PERF-DOC-06` — `bench-2026-08-03.md:103-106` overstates asset-size independence

**P2** · `complexity` · CONFIRMED

- **Where:** `docs/bench-2026-08-03.md:103-106`; contradicted by `crates/longtail/src/version.rs:42,113-115`
- **What:** "Peak is now bounded by *one block payload* … and *one HPCDC part* (~`max_hash_size`,
  32 MiB default) on the scan side — **independent of asset size**." The scan buffer is
  `min(asset_size, max_hash_size)` and `max_hash_size` is `target_chunk_size × 1024`, so the claim
  holds only while `target_chunk_size × 1024` is smaller than the largest asset. It is also per-rayon-worker,
  which the sentence omits — the peak is `W ×` that quantity. See PERF-02.
- **Recommendation:** State the bound as `worker_count × min(largest_asset, target_chunk_size × 1024)`.
  It is still an excellent result at the defaults; it is just not unconditional.
- **Effort:** S.

## Hardening backlog

Ranked by ratio of risk closed to effort.

1. **`chunk_asset` ↔ `chunk_asset_streaming` equivalence test** (PERF-06). One test closes a
   cross-crate duplication, discharges `ALG-11`, and gives PERF-02 a home. Highest value in this
   document.
2. **A `ChunkParams` newtype** (PERF-06) making `(chunker, max_hash_size)` unforgeable — the only
   invariant in my slice with *zero* enforcement of any kind.
3. **Resume-after-cancel test with `cache_target_index` at its default** (PERF-01) — R7 notes the
   existing test disables it, so the production configuration of the download path's most
   safety-critical behaviour is untested.
4. **A CI release build with a binary-size budget** (PERF-08 + PERF-12) — makes the shipped
   configuration a gated one and turns every future size regression loud.
5. **Assert `retries == 0` in the e2e report** (PERF-07) — makes contaminated benchmark cells
   impossible to quote by accident.
6. **min/median/max in the e2e report** (PERF-09) — the cheapest way to make every future perf claim
   falsifiable.
7. **A shard-read counter in `BlockStoreStats`** (PERF-04) — the flush's I/O amplification is
   currently invisible to every test and every report.

## Verified good

- **The prefetch liveness argument holds.** I re-derived R3's invariant independently from
  `remote.rs:274-329,444-492,494-531` and agree: budget is acquired at dispatch, entries are created
  only after acquisition, and a demand get can always claim a queued hash and fetch inline. Neither
  the facade's unbatched `preflight_get(&store_index.block_hashes)` at `apply.rs:84` nor any
  `apply_concurrency` value can construct a demand that waits on the budget.
- **`merge_consuming` is the best-guarded change in the last five commits.** Byte-identity is
  load-bearing (the shard name is `sha256(to_bytes())`), it is asserted by two proptests over
  canonical *and* arbitrary inputs plus targeted unit tests, and the non-canonical fallback to the
  allocating `merge` is explicit at `store_index.rs:332`+. All three flush/read call sites
  (`sync.rs:175,227,293`) were converted together.
- **The `8e28d81` owned-`Bytes` write path is complete on the write side.** `blob/mod.rs:97`
  `write(data: Bytes)`, `fs.rs:332-360` moves it into `spawn_blocking` with no copy, `s3.rs:398-409`
  `ByteStream::from(data)` with no copy, and the put-retry ladder re-clones for O(1)
  (`remote.rs:401,412`). The commit message's claim matches the code exactly.
- **`78c2c46` did what it says.** `GetBlockSizes` (`remote.rs:124,670,768-792`) +
  `StoreIndex::block_payload_sizes` replace the union clone in `preflight_get`, and the result map is
  bounded by the working set rather than the store. The one residual `base.clone()` is PERF-03's, on
  a different path.
- **`f2edb58` did what it says.** `existing_content` (`remote.rs:803-825`) derives the subset from
  the borrowed `base` with no clone when `added` is empty, and the union is dropped after a ReadWrite
  reply (`:707-709`).
- **`c703b32` did what it says.** `write_content` reads each chunk's byte range positionally
  (`upsync.rs:276-285`) with one cached file handle (`:245,263-272`); no whole-asset buffer survives.
- **`wait4` per-child measurement is methodologically sound for what it claims.** `ru_maxrss` from
  `wait4(pid, …)` is that child's process-lifetime high-water mark in KiB on Linux; all three
  implementations are single-process, so there is no descendant contamination; `RUSAGE_SELF` and
  `RUSAGE_CHILDREN` — both of which *would* cross-contaminate — are correctly avoided, exactly as
  `e2e.rs:10-15` argues. Page-cache pages are not counted in RSS, so the pre-warm at `:599,683`
  does not inflate the numbers. `std::mem::forget(child)` at `:83` leaks no OS resource on Unix
  (`Child` holds a pid, not an fd). The 20 ms poll (`:85`) quantizes *wall*, which
  `bench-2026-07-05.md:377-385` already analyses honestly. **The instrument measures what the
  documents claim it measures.** Its defects are PERF-07 (retry state) and PERF-09 (reporting), not
  the measurement primitive.
- **A worker lead I rejected.** Both allocation-sweep workers reported that `fs.rs:319-325`
  (`Vec::new()` + `read_to_end`) pays doubling-realloc growth for lack of a capacity hint. It does
  not: `impl Read for std::fs::File` overrides `read_to_end` to reserve from the file's metadata
  length first. Recorded here so the next session does not re-file it.

## Experiments requested

| # | Hypothesis | Exact command | What result would change the finding |
|---|---|---|---|
| 1 | `lto = "thin"` + `codegen-units = 1` + `strip = "symbols"` removes ≥ the 883,401 B of duplicate symbols plus the ~6.6 MB symbol table, i.e. ≥ 25% of the 27,811,160 B binary | Add the three keys to a `[profile.release]` in the root `Cargo.toml`, then `cargo build --release -p longtail-cli && ls -l target/release/longtail && size -A target/release/longtail`; compare against `10-release.txt:193,231`. Record wall time from the same run. | A reduction under ~10%, or a release build time over ~5 minutes, would make PERF-08 a P3 rather than a P2. |
| 2 | The incremental download cell's RSS (276/295 MiB) and the "~97% of wall is the scan" split are both materially different after `c13a4d1`'s streaming scan | `LONGTAIL_BENCH_SCENARIOS=incremental LONGTAIL_BENCH_DATA_SIZE_MB=1024 cargo run --release -p longtail-bench --features differential --bin e2e` | If RSS and the scan share are unchanged, PERF-DOC-03 shrinks to a note; if the scan share drops below ~80%, PERF-01's "biggest win" framing is dead on the numbers as well as on correctness. |
| 3 | The default-configuration incremental download (target-index cache **on**) spends a negligible fraction of wall in the scan, because the scan does not run | As #2 but with the `--no-cache-target-index` flags removed from `e2e.rs:396,439` — i.e. run the configuration the product ships. Compare `build_target_index` phase timings. | If the cached path still spends significant time in `build_target_index`, my Deliverable 3 claim that the roadmap item is near-worthless in the default configuration is wrong. |
| 4 | Peak download RSS is linear in `target_block_size`, per Deliverable 2's model | Upsync the standard 1 GiB dataset three times at `--target-block-size` 8388608 / 33554432 / 134217728 into three stores, then `/usr/bin/time -v ./target/release/longtail downsync --remote-worker-count 8 …` against each | If peak does not scale with the third term, the model is wrong and PERF-11's severity drops. |

## Open questions for the maintainer

1. **Is `target_block_size` fixed by the pipeline, or can a store arrive with a much larger one?**
   PERF-02 and PERF-11 both hinge on this. If it is contractually 8 MiB across every store the Tauri
   app will ever see, both drop a priority level and become documentation items.
2. **Should `docs/put-path-memory.md` and the two `bench-*.md` files survive at all?** `CLAUDE.md`
   says only four docs are keepers. My recommendation is: fold the two-file steady-state explanation
   and the final measured table into `docs/rust-port.md`, delete the rest, and start a single
   `docs/perf.md` that records only current numbers with their commit. Three overlapping,
   partly-contradictory perf documents are already producing wrong downstream claims (PERF-01).
3. **Was the prefetch budget hidden from the CLI for a reason beyond the one stated?**
   `options.rs:73-74` says "deliberately", and the stated rationale (correctness must not depend on
   it) is precisely why exposing it is safe. If there is another reason, it should be written down.
4. **Is `longtail_core::chunk_asset`/`create_version_index` meant to stay public?** If it is a
   supported entry point, PERF-06's equivalence test is mandatory. If it is only the tests' fixture
   builder, it should be `#[cfg(test)]` or moved to testkit and the duplication disappears.

## Files read

Absolute paths, all under `/home/chris/work/longtail-rs/cm/rust-port`:

- `Cargo.toml`, `CLAUDE.md`
- `docs/rust-port.md` (§Roadmap, §Safety posture), `docs/bench-2026-07-05.md` (skimmed + targeted),
  `docs/bench-2026-08-03.md`, `docs/put-path-memory.md`
- `docs/review/01-format-codecs.md`, `docs/review/02-algorithms-and-oracle.md`,
  `docs/review/03-store-concurrency.md`, `docs/review/07-operations-cli.md` (findings indexes;
  R3's Deliverable 1 and `STORE-08` in full)
- `crates/longtail-core/src/build.rs`, `src/block.rs`, `src/compress.rs`, `src/pack.rs`,
  `src/store_index.rs`
- `crates/longtail-store/src/remote.rs`, `src/sync.rs`, `src/cache.rs`, `src/blob/fs.rs`,
  `src/uri.rs`, `src/lib.rs`, `Cargo.toml`
- `crates/longtail/src/options.rs`, `src/downsync.rs`, `src/version.rs`, `src/apply.rs`,
  `src/upsync.rs`, `Cargo.toml`
- `crates/longtail-cli/Cargo.toml`
- `support/longtail-bench/Cargo.toml`, `src/lib.rs`, `src/bin/e2e.rs`, `src/bin/merge_mem.rs`
- `.github/workflows/rust.yaml`
- `target/review-evidence/`: `MANIFEST.md`, `REVIEWER-CONTRACT.md`, `10-release.txt`,
  `17-bloat.txt`, `17b-bloat-fn.txt`, `12-loc.txt`, `13-fixtures.txt`
- Read-only binary inspection of `target/release/longtail` via `nm` and `readelf`
