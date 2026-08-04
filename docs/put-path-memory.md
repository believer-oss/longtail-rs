# PUT-path memory audit — Rust port vs golongtail

> **Purpose.** This document records how the pure-Rust port compares to golongtail on
> the six memory bottlenecks identified during a live upsync OOM incident at
> Fellowship scale (2×1.29 GB `store_*.lsi` per shared shard). It exists to inform a
> go/no-go on 2-3 days of targeted PUT-path work versus a workaround (isolated
> per-consumer stores). Numbers are static source estimates unless noted;
> **there are no upsync benchmarks in the repo yet — measuring alongside any fix
> work is a prerequisite for trusting these estimates.**
>
> Refs are against Rust port `@a459c3b`, golongtail `@49a20e1`, longtail `@96241fe`.

## Background

At Fellowship scale each `data-{YYYY-MM}` shared shard accumulates two content-addressed
`store_*.lsi` files of ~1.29 GB each (see §Two-file steady state below). golongtail's
upsync flush allocates 5-6× total `store_*.lsi` size in native memory plus ~2.6 GB
Go-heap transient during read → measured OOM on a c6id.xlarge (8 GB). Signing runs
uncover this on any month past a few hundred MB of accumulated blocks.

### Two-file steady state (both implementations share this)

Every finished monthly shard has exactly two `store_*.lsi` files with near-identical
mtimes. This is not a race artifact — it is the lockless read-modify-write protocol
for stores without CAS. In `tryAddRemoteStoreIndex`
(`golongtail/remotestore/remotestore.go:1260-1297`; `sync.rs:239-287` in the port),
a writer deletes only files it has read and merged — because a concurrent writer's
fresh file is a superset of *its own* additions and deleting it would permanently
lose those block-index entries. When two writers overlap, both `store_*.lsi` files
survive; every subsequent reader must union both.

The pattern's month-end mtimes are a tautology: puts to a shard end when the shard
rolls over, and the final state is frozen. During active months the shard likely
oscillates around 2 files as well (matched by the observed size progression from
16 MB in 2024 to ~1.29 GB in mid-2026). Solo puts collapse to 1 file. Nothing in
the codebase is month-aware — the monthly sharding is a pipeline convention only.

## The six opportunities

Ranked evaluation of the improvements identified during the OOM investigation.

| # | Opportunity | Status in Rust port | Refs |
|---|---|---|---|
| 1 | Byte-budget semaphore on compose→upload | N/A (structurally moot) | `crates/longtail/src/upsync.rs:244-297`; `crates/longtail-store/src/remote.rs:202,227,266` |
| 2 | Free uncompressed post-compress; right-size compressed | **Done** | `crates/longtail-store/src/compress.rs:52-64`; `crates/longtail-core/src/compress.rs:333-344` |
| 3 | Streaming/partitioned store-index merge | Not done, better constants | `crates/longtail-store/src/sync.rs:155-201,239-287`; `crates/longtail-store/src/remote.rs:723-750` |
| 4 | Drop union after `GetExistingContent` | Not done — slightly worse than Go | `crates/longtail-store/src/remote.rs:633,693-710,743` |
| 5 | If-None-Match on store-index PUT | Not done, mirrors Go, limited payoff | `crates/longtail-store/src/blob/s3.rs:313-315,352-355,393-403`; `crates/longtail-store/src/sync.rs:252-259` |
| 6 | Sized, rewindable S3 upload bodies | **Done** | `crates/longtail-store/src/blob/s3.rs:393-403,376-390` |

### 1 — Byte-budget semaphore on compose→upload

Go queues 16+8W block references, each pinning ~2.2× block-size (uncompressed +
compressBound). The Rust port issues puts **serially** (`upsync.rs:244-297`): compose,
`await` compress, `await` PUT, next block. At most one block is ever in flight; only
small `BlockIndex` messages cross the index channel (cap `64 + 8×workers`,
`remote.rs:202`). Go's failure mode cannot occur here.

Cost of the current design: compression and S3 PUTs never overlap. `worker_sem`
(`remote.rs:70`) exists but is never contended by puts. If puts are pipelined for
throughput later, the byte-budget pattern already exists in-tree for GET
(`prefetch_sem`, `acquire_many_owned` at `remote.rs:227,266-269`) — reuse that
before adding depth.

### 2 — Free uncompressed post-compress; right-size compressed

Fully resolved. `CompressBlockStore::put_stored_block` (`compress.rs:52-64`) moves
the uncompressed payload into the rayon closure; it is freed the moment
`encode_block_payload` returns. The framing codec allocates an **exactly-sized**
`Vec::with_capacity(8 + compressed.len())` and drops zstd's compressBound-capacity
buffer (`longtail-core/src/compress.rs:339-343`).

Residual: two avoidable full-copies of the compressed bytes per put —
`StoredBlock::to_bytes()` at `remote.rs:376` (`block.rs:142-150`) and the body copy
`data.to_vec()` at `s3.rs:398`. That's ~3× *compressed* size transient per in-flight
block (~24 MB at 8 MB blocks, ×1 block max). Harmless today; switching to
`bytes::Bytes` would zero-copy both.

### 3 — Streaming/partitioned store-index merge

Not implemented. The port keeps Go's merge-on-read architecture with better constants:
`sync.rs:155-177` is pairwise-incremental — shard bytes are dropped after parse, the
old accumulator after each merge — so load peak ≈ acc + shard + merged-out ≈ 3 union
copies (Go holds everything).

Flush (`try_add`, `sync.rs:274-287`) still materializes `existing` + `merged` +
`to_bytes()` serialized buffer **simultaneously**, plus the owner's retained union
(item 4), ≈ 4 unions. At Fellowship scale (union ~1.3 GB in memory; layout is
byte-identical to on-disk, `store_index.rs:31-45`) that's **~5.2 GB peak** vs
golongtail's >8 GB. Every flush re-downloads all `.lsi` from S3
(`sync.rs:281`) — no local caching.

Byte-identical output is load-bearing (shard name is sha256 of the serialized
buffer, `sync.rs:106-109`), but a streaming serializer preserves that.

`StoreIndex::merge`/`from_block_indexes` never pre-reserve (`store_index.rs:240-263,297-321`);
Vec doubling adds realloc overshoot at GB scale.

### 4 — Drop union after `GetExistingContent`

Not implemented, and **marginally worse than Go**: `merged_index` returns
`base.clone()` at `remote.rs:707` — a full copy of the union per `GetExistingContent`
or `GetIndex` query, so the union transiently exists ×2 during upsync step 3. The
owner also keeps the union for the store's lifetime (`remote.rs:633`), and flush
never uses it (persist re-reads from S3, `remote.rs:743`).

Fix: compute the missing-blocks subset inside the owner (no clone) and drop the
union after replying under ReadWrite. Trivial — half a day, tens of lines. Saves
1.3-2.6 GB off flush peak. Best payoff-to-effort in the list.

### 5 — If-None-Match on store-index PUT

Not implemented, faithfully mirrors Go: `supports_locking()` returns false, `write`
is an unconditional PutObject and always returns `Ok(true)`; the HEAD-then-PUT
race window at `sync.rs:252-259` is intact.

Mechanically cheap to add: the `BlobObject::write → bool` CAS-lost contract already
exists (`blob/mod.rs:88-91`), and both retry ladders are in place. Wire
`.if_none_match("*")` and map 412 → `Ok(false)`. ~30-60 lines.

**Honest caveat:** shard names are content-addressed (sha256 of the serialized
index bytes), so If-None-Match only dedupes *identical* shards. The Fellowship
two-file scenario is two writers producing **different** merged shards (each has
`base + their_own_new_blocks`, so neither is a superset of the other). If-None-Match
would not have prevented this. Preventing the two-file state requires a canonical-key
CAS redesign — not this header. Item 5 remains a nice hygiene improvement but is
not on the OOM path.

### 6 — Sized, rewindable S3 upload bodies

Fully resolved. All bodies are `ByteStream` from owned in-memory buffers —
sized, rewindable, retry-safe (`blob/s3.rs:393-403,376-390`). No `wrap_stream`,
no unsized `AsyncRead`. Utrace-style bug cannot occur.

Bonus improvements over Go: stalled-stream protection defaults on
(`s3.rs:96-103,232-237`); `get_objects` paginates `ListObjectsV2` where
`golongtail/longtailstorelib/s3Store.go:92-103` reads a single page and silently
drops any results past 1000 keys.

## New OOM vector unique to the Rust port

**Whole-file asset reads during chunking and packing.** `crates/longtail/src/version.rs:64`
uses `fs::read` on each asset (`fs_util.rs:100-106`) across an N-wide rayon pool;
`write_content` also caches one whole asset (`upsync.rs:259-267`). Peak memory is
`N × largest_file`. On multi-GB paks × 4 workers on an 8 GB node this alone OOMs
the build node — independent of anything store-index related. Go streams via its
HPCDC chunker.

The `--enable-file-mapping` flag is parsed and plumbed but never consulted by the
library (dead code, same state as the native lib at `longtail/src/longtail.c:2453`
which hardcodes 0).

Fixing this is a prerequisite before trusting the port for large-asset workloads
regardless of what happens with the store-index path.

## Additional findings

- **Unbounded prefetch task spawn** — one tokio task per preflighted block
  (`remote.rs:502-510`); acknowledged in the roadmap (`docs/rust-port.md`).
- **GET-side transient copy** — `StoredBlock::from_bytes` copies the payload tail
  (`block.rs:132-137`), 2× block transient per fetch. Corresponds to the "GET RSS
  higher than Go" note in `bench-2026-07-05.md` §9.1.
- **History worth knowing** — the original prefetch budget deadlocked any >512 MiB
  cold download (`bench-2026-07-05.md` §4.1); fixed in §9.1 (now 1.00× golongtail
  wall at 1 GiB), at the cost of higher GET RSS (594 vs 274 MiB at w8, by design,
  budget-bounded).

## Sizing and prioritization

| Path | Flush peak at Fellowship scale | Time | Notes |
|---|---|---|---|
| golongtail current binary | >8 GB | — | Measured OOM on c6id.xlarge |
| Rust port as-is | ~5.2 GB (static estimate) | — | Marginal, no headroom; whole-file OOM risk on paks |
| Rust port + item 4 + `base.clone()` fix | ~2.5-3.8 GB | ~½ day | 1.3-2.6 GB saved by drop-union + no query-clone |
| Rust port + items 3 (cheap) + 4 | ~1.5-2.5 GB | ~1.5 days | Consume `existing` into merge, pre-reserve, drop before serialize |
| Rust port + items 3 + 4 + ranged-read in `write_content` | ~1.5-2.5 GB + pak safety | ~2.5 days | Removes whole-file OOM vector during upload |
| Rust port + full streaming index serializer + items 4, whole-file fixes | ~1 GB | ~4-5 days | Optimal |

Priority order for a two-day sprint:

1. **Item 4 + `base.clone()`** at `remote.rs:707` (~½ day, tens of LoC). Biggest ratio.
2. **Item 3 cheap version**: consume `existing` into the merge, pre-reserve
   `StoreIndex::merge`/`from_block_indexes`, drop the merged buffer before
   `to_bytes()` (~1 day, few hundred LoC).
3. **Ranged reads in `write_content`** at `upsync.rs:259-267` (~1 day) — necessary
   for large-pak safety; independent of the store-index work.
4. Streaming chunker to remove the `fs::read` in `version.rs:64` (~2-3 days) —
   defer if not on the critical path.
5. Full streaming index serializer (~2-3 days) — defer; the cheap version is
   likely sufficient.
6. Item 5 If-None-Match — defer or drop. Doesn't affect the OOM scenario.
7. Add put pipelining behind a byte-budget semaphore for throughput
   (~1-2 days, ~150-250 lines) — separate concern from memory.

## Bottom line

The per-block pipeline (items 1, 2, 6) is already solved in the Rust port — items
2 and 6 properly, item 1 by the blunt instrument of a serial pipeline (trading
memory for throughput). The store-index problem (items 3, 4, 5) is not solved
architecturally: the port faithfully reproduces Go's merge-on-read design with
somewhat better transient behavior, giving an estimated ~5.2 GB flush peak at
Fellowship's 2×1.29 GB scale versus Go's measured >8 GB. That would *probably*
survive where golongtail OOMs, but with little headroom.

**~1.5 days of targeted work (items 4 + cheap item 3) brings the flush peak to
~1.5-2.5 GB** — comfortable on an 8 GB node. Add the ranged-read fix and the port
is broadly production-ready for the signed-build workload.

One place the port is currently **worse** than golongtail: whole-file asset reads
during chunking and packing (N-wide rayon). For multi-GB paks that alone can OOM
the build node and must be addressed before trusting the port on real workloads.

Estimates in this document are static source analysis. **Add upsync benchmarks
to the repo alongside any of this work** so future changes have measurement to
regress against — the download path already has this (`bench-2026-07-05.md`);
the upload path does not.

## Resolution (2026-08-03) — landed and measured

The plan in this doc was executed; measurements are in
[`bench-2026-08-03.md`](bench-2026-08-03.md). Status by opportunity:

| # | opportunity | status |
|---|---|---|
| 1 | Byte-budget semaphore on compose→upload | already solved (serial pipeline) |
| 2 | Free uncompressed post-compress | already solved |
| 3 | Streaming/partitioned store-index merge | **cheap version landed** — consume `existing` into the merge, drop before serialize, pre-reserve output Vecs (`2cce0cd`). Full streaming serializer (P2b) *not needed by the numbers* — see below. |
| 4 | Drop union after `GetExistingContent` | **landed** (`f2edb58`) — compute the covering subset against `base` directly, drop the retained union after a ReadWrite reply. |
| 5 | If-None-Match on store-index PUT | deferred — doesn't affect the OOM scenario. |
| 6 | Sized, rewindable S3 upload bodies | already solved; **improved** — `BlobObject::write` now takes owned `Bytes` (blob-write path no longer copies the body; the S3 GET drops a double-copy). |
| — | Rust-only whole-file asset reads (scan + pack) | **landed** — ranged reads in `write_content` (P1c, `c703b32`) + streaming scan chunker (P2a, `c13a4d1`). Peak now independent of asset size. |

**Measured outcome** (256 MiB flush scenario; full numbers in the bench doc):
flush peak RSS **843 → 305 MiB (−64%)** across P1/P2a + the owned-`Bytes` blob-I/O
follow-up. The port went from **57% heavier** than golongtail on this path to **43%
lighter** (305 vs 535 MiB), while staying faster (wall parity-or-better every step). A
full upsync of a 512 MiB single asset peaks at **50 MB** RSS (was ~512 MB), closing the
multi-GB-pak OOM vector.

**The ~5.2 GB estimate above was pre-fix static analysis.** Measured baseline peak was
~3.3× the index size (≈4.3 GB extrapolated to 1.29 GB); post-fix it is ~1.2× (≈1.5 GB) —
comfortable on an 8 GB node with headroom. This is the *fs* number; on S3 the owned-`Bytes`
write removes two further copies the fs bench can't exercise. The full streaming
`store.lsi` serializer (P2b) is deferred: the remaining peak is the live index plus one
`to_bytes()` buffer, and a streaming serializer needs a two-pass hash-then-stream (the
shard key is `sha256(to_bytes())`) for a partial win — revisit only if a future shard
size erodes the margin.
