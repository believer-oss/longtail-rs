# The pure-Rust longtail port

How this workspace was built and why. It is a pure-Rust reimplementation of
[longtail](https://github.com/DanEngelbrecht/longtail) (the C library) and the parts of
[golongtail](https://github.com/DanEngelbrecht/golongtail) we use — byte-compatible with the
existing on-disk formats and stores. For the exact wire formats see
[`docs/format-spec.md`](format-spec.md); for measured performance see
[`docs/bench-2026-07-05.md`](bench-2026-07-05.md). C citations are against longtail `@96241fe`
and golongtail `@49a20e1`.

## Motivation

The C library was replaced rather than wrapped because:

- Small edge-case bugs are awkward to fix in the C code.
- The C FFI surface is shaped around golongtail's architecture, making it painful to bind
  safely from Rust.
- The C `bikeshed` job API does not mesh with the tokio async workers the AWS SDK relies on,
  costing efficiency and precise control over threading.

The **paramount constraint** is 100% verified byte-compatibility with existing data stores —
`.lvi` version indexes, `.lsi` store indexes, and `.lsb` stored blocks in S3 and in local
caches. The consumers are a Tauri game launcher/downloader (drives the download path; wants a
caller-owned tokio runtime, caller-supplied AWS credentials that refresh mid-operation, and
structured errors) and a CI/CD pipeline that needs a drop-in golongtail CLI.

## What was ported, and where it lives

| Crate | Role |
|---|---|
| `longtail-core` | Sync, no tokio. The four on-disk formats (byte-cursor unaligned little-endian codecs), `FileInfos` scan/sort, the HPCDC chunker (exact port) plus FastCDC (benchmarking only), the hash layer (blake3, blake2s, meow parse-only), the compression registry, the store-index algebra, and version build/diff. |
| `longtail-store` | tokio-native. Blob stores (fs/mem/S3), the `RemoteBlockStore` actor, the `Cache`/`Compress` block-store decorators, and optimistic store-index sync (fs lock + S3 shard-merge). |
| `longtail` | The facade: `downsync`/`upsync` operations, the `ChangeVersion2` apply flow, the error tree, and progress/cancellation. |
| `longtail-cli` | A `clap` binary — the golongtail CLI replacement. |
| `longtail-sys`, `longtail-ffi` | **Legacy.** The C bindings and their safe wrappers, retained only as the reference oracle for differential regression testing; scheduled for deletion after one production release cycle. |

## Architecture

`longtail-core` is pure sync. CPU-bound work (chunk/hash/compress, decompress/verify) runs on a
**per-operation `rayon::ThreadPool`** the caller sizes (the successor to the old
`LONGTAIL_WORKER_COUNT`); CPU work never goes through `spawn_blocking`.

`longtail-store` mirrors golongtail's actor model:

- **One index-owner task** owns the in-memory store index behind an `mpsc` command channel — no
  shared-state lock on the index (the equivalent of Go's `contentIndexWorker`).
- **Worker tasks** share one cheaply-cloned `aws_sdk_s3::Client`, bounded by a `Semaphore` (the
  worker-count equivalent of Go's `remoteWorker` pool).
- **Prefetch** is a `Mutex<HashMap<u64, Shared<future>>>`, so get-coalescing falls out of
  `Shared` for free (this structurally subsumes the C `shareblockstore`), plus a byte-denominated
  `Semaphore` for the 512 MiB prefetch budget. The async→CPU bridge is `pool.spawn` + a
  `oneshot`.

The public API is a plain `async fn downsync(...)` (and a `_blocking` convenience) that runs on
the caller's ambient runtime — the library never creates a runtime. Credentials are held as a
provider/`Client`, never as a snapshot, so the AWS SDK's lazy credentials cache refreshes
mid-operation on long transfers. Cancellation is a `CancellationToken` (re-exported from the
facade as `longtail::CancellationToken`) — checked between block launches in apply and per-asset
in the scan, so a cancel leaves the target resumable; it doubles as the pause primitive (pause =
cancel + keep the tree/cache, resume = re-invoke, which re-scans + diffs + fetches only the delta
from cache). The CLI wires ctrl-c to it for a graceful stop. Progress is a callback
trait (`ProgressSink`) whose `Progress` sample carries two dimensions at once — an item count
(blocks for the download apply loop, files for the target scan) and a byte count — so a consumer
can show item progress and an approximate data rate together (the CLI collapses both onto one
bar — count plus rate/ETA; a GUI could draw a double bar). The download byte figure is
decompressed bytes materialized, so its rate runs a
little above raw wire bytes. Rayon loops poll the cancel token between work items. `BlockStore`
is an async, dyn-dispatched trait, so the download stack composes at runtime as
`Compress(Cache(Remote(S3)))`.

The store follows golongtail's remotestore model: a retry ladder of {0, 100 ms, 250 ms, 500 ms,
1 s, 2 s}, and store-index sync that is optimistic locking on fs (lock → read → merge → write →
retry on a generation change) versus shard-merge-on-read on S3 (write `store_<sha256>.lsi`, then
merge every discovered shard when reading).

## How compatibility was verified

The approach is layered: **committed golden fixtures** generated by a pinned upstream golongtail
CLI (v0.4.5 — provenance and per-file SHA256 recorded in `fixtures/manifest.json`);
**differential testing** against the C library (via `longtail-ffi`) over the corpus and
proptest-generated inputs; a **three-way end-to-end differential** (pure Rust vs C-via-FFI vs a
spawned golongtail binary) that compares full tree manifests; **cross-implementation interop**
in both directions; a **concurrent mixed Rust+Go writer** chaos test against minio; **miri** over
the sync format/algorithm code; and **proptest round-trip fixpoints**. "100% verified
compatibility" is the conjunction of eight gates:

| # | Gate |
|---|---|
| ① | Every committed fixture round-trips byte-identically through pure Rust. |
| ② | Chunk boundaries are identical to C's **streaming** chunker on `chunker.input` + the full corpus, at every target size. |
| ③ | All four hash kinds equal C over the corpus (blake3/blake2). |
| ④ | Every fixture `.lsb` decompresses correctly, with verifying hashes and size sums. |
| ⑤ | Pure-Rust downsync of every fixture version → tree identical to golongtail's. |
| ⑥ | Rust upsync → golongtail downsync round-trips identically (and the reverse). |
| ⑦ | A Rust-written `.lvi` is byte-identical to golongtail's for identical input. |
| ⑧ | Store-index shards interoperate under concurrent Rust+Go writers. |

There is one deliberate **non-gate**: the compressed payload bytes of newly-written blocks are
not compared. Block identity is the hash of the chunk-hash array, not the compressed bytes, so
codec output drift across zstd/brotli/lz4 versions is harmless to a store — only correct
*decoding* of every ID is required.

## Performance

Cold and warm downsync are at golongtail parity (≈1.00× at 8 remote workers after the
download-path fixes below); a 1 GiB cold downsync completes and tree-verifies; the
micro-benchmarks (chunk/hash/compress and the index codecs) are within target of the C library.
Incremental downloads are currently scan-bound (see the roadmap). Full methodology and numbers
are in [`docs/bench-2026-07-05.md`](bench-2026-07-05.md).

## Deliberate divergences from C/Go

Each is intentional; the end state stays compatible.

- **Prefetch budget debited at dispatch, not at completion.** golongtail debits at fetch
  completion without waiters (`remotestore.go:387-389`); the Rust store acquires the permit when
  a prefetch is dispatched. This is stricter, safer accounting — forward progress never depends
  on budget availability, and any working set completes with any budget ≥ 1 permit.
- **Lock guards never unlink.** A `flock` guard releases the OS lock only and never removes the
  `._lck` file. Unlinking a locked path breaks mutual exclusion by inode replacement and throws
  sharing violations on Windows; the lock files carry no state, so a leftover file never wedges
  the store.
- **Stale-generation clearing on `init-remote-store`.** When the store object is absent under the
  flock, the fs blob store clears a stale `store.lsi.gen` sidecar, fixing a create-CAS that would
  otherwise spin forever after a manual `store.lsi` deletion (exactly the `init-remote-store`
  case).
- **Strictness beyond C on malformed input.** Readers reject `AssetChunkIndexCount < ChunkCount`
  and trailing bytes, and return typed errors where C silently wraps 32-bit arithmetic.
- **Correct `GetExistingStoreIndex` tags.** The kept block's tag is taken from its own
  `block_tags[b]` (matching `Longtail_MakeBlockIndex`, `longtail.c:9145`) rather than C's read of
  `m_BlockTags` indexed by the chunk offset (`longtail.c:7307`), which is a latent bug (below).
- **`clone-store` honours the already-cloned skip.** golongtail calls its `validateOneVersion`
  with swapped arguments so the skip never fires and every version is always re-cloned; the port
  implements the intended behaviour, so re-runs are genuinely cheaper. The end state is identical.
- **`cp` assembles the whole asset then writes once**, which is correct for files > 128 MiB that
  golongtail truncates via per-segment overwrites.
- **`prune --dry-run` phrasing** adds a trailing newline golongtail omits (harmless).
- **HPCDC rejects an out-of-range target.** For target `avg` above ≈ 9.31M the C `(uint32_t)`
  cast of the discriminator is undefined (the expression crosses its denominator pole), so no
  compatible behaviour exists; the Rust chunker returns a typed error instead.
- **S3 Transfer Acceleration defaults off.** The legacy FFI `get_with_cache` path hardcoded
  `s3_transfer_acceleration = Some(true)`; `S3Options::default()` sets `transfer_acceleration:
  false`. Acceleration requires the bucket to opt in and adds cost, so `false` is the safer
  library default — callers that want the old throughput set it explicitly (the launcher does).
- **Bounded block cache with access-time LRU.** `CacheBlockStore` takes an optional byte budget
  (`DownsyncOptions`/`GetOptions::cache_size_limit`, CLI `--cache-size-limit`). When set, each
  cached `.lrb` file's mtime is stamped on every access — including cache *hits* — so it is a true
  last-access clock, and a post-run sweep (`evict_cache_dir`, on `close`) deletes least-recently-used
  blocks down to the budget. This supersedes the legacy FFI `get_with_cache` prune, which sorted by
  `max(mtime, atime)` and never refreshed a hit block's timestamp (so hot blocks were evicted first),
  and the unused C block-count `LRUBlockStoreAPI`. The cache-dir `store.lsi` stays advisory: an
  evicted block is transparently re-fetched and re-cached on next read (the C library's authoritative
  cache index meant a deleted file was never rewritten — regression-tested here).

## Upstream findings

Reported here so issues can be filed against upstream longtail/golongtail:

1. **`Longtail_GetExistingStoreIndex` reads the wrong tag.** It indexes `m_BlockTags` with the
   *chunk* offset (`longtail.c:7307`) instead of the block index, so it reads the wrong tag slot —
   or out of bounds — whenever a kept block's chunk offset differs from its block index.
2. **`zstd_low` resolves to a nonsensical level.** `LONGTAIL_ZSTD_LOW_COMPRESSION_TYPE` (a real
   level constant) is shadowed by the `#define` of the type ID in `longtail_zstd.c`, so
   `SettingsIDToCompressionSetting` returns the type ID as the ZSTD level for the LOW case. It is
   harmless only because golongtail maps both `zstd_low` and `zstd_high` to `zstd_max`.
3. **`clone-store` never skips already-cloned versions.** `validateOneVersion` is called with
   swapped arguments (`cmd_clonestore.go:398` vs `:21-25`), so the skip check never fires and
   every version is re-cloned on every run.
4. **HPCDC discriminator is UB for large targets.** Above `avg` ≈ 9.32M the cast
   `(uint32_t)(avg / (-1.42888852e-7*avg + 1.33237515))` is undefined — the expression crosses
   its denominator pole / exceeds `u32` range.
5. **`get_existing_store_index_sync` missed-wake race.** The C sync path can intermittently hang
   an operation; the differential harness works around it with a watchdog + retry.

Related: `cmd_cp.go` overwrites per segment and does not check the write error, truncating files
larger than 128 MiB.

## Dropped and deferred

**Dropped** (not carried forward): `memtracer` (Rust ownership + heaptrack/dhat); the `bikeshed`
job API (→ rayon + tokio); `shareblockstore` (→ `Shared`-future get-coalescing); the legacy
`ChangeVersion` (v1) write path; meow-hash *write* support (parse-without-verify only —
production stores are blake3); the 26-function `StorageAPI` vtable (→ a narrow internal trait,
with `memstorage` kept only as a test double); and the C platform layer of threads/atomics/
mmap/locks (→ `std` + `fs4`).

**Deferred** (real functionality, postponed): the GCS (`gs://`) blob store — our stores are S3 +
fs and GCS cannot be tested here, so `gs://` returns a clear "not supported"; `ArchiveIndex` +
`pack`/`unpack` (behind an `archive` feature, droppable); the `clone-store` zip fallback; and
`blockstorestorage` — rather than
port its 1.6k-line virtual filesystem, `ls` is a pure index walk and `cp` is a targeted block
fetch.

## Roadmap

- **Incremental-scan redesign (the biggest win).** Incremental downloads are dominated by the
  target scan: at 1 GiB, building the target index takes ≈ 1434 ms versus ≈ 37 ms to apply — the
  scan is ~97% of the wall. It is worth ≈ 330 ms (~70%) of the 384 MiB incremental cell (≈ 1.4 s
  at 1 GiB) and ≈ 250 MiB of peak RSS. The fix is a streaming and/or mtime/size-short-circuiting
  target scan (as golongtail does).
- **Re-test the cold-S3 latency hypothesis.** With the prefetch-budget deadlock fixed, the "async
  plane wins on cold, S3-like latency" hypothesis is now measurable — run the minio recipe
  against the fixed path.
- **Compression micro-cell benchmarking**, and a **bounded prefetch dispatcher** (preflight
  currently spawns one parked task per block — fine at current scale, worth revisiting for
  multi-GB working sets).

## Safety posture

`#![forbid(unsafe_code)]` is enforced on every default-member library and binary target:
`longtail-core`, `longtail-store`, `longtail`, `longtail-cli`, `xtask`, and the `longtail-testkit`
and `longtail-bench` library targets. The only `unsafe` in the pure-Rust workspace lives outside
those library targets, and all of it is justified:

- `unsafe { libc::umask(0o022) }` pinned in 8 integration-test files — sound because one test
  binary is one process.
- The testkit differential shim's `unsafe extern` block — under `tests/`, behind the
  `differential` feature.
- Five `libc` blocks (`wait4`/`kill`/`rusage`) in `support/longtail-bench/src/bin/e2e.rs`, a
  binary target that the library's `forbid` does not cover.

There is no `memmap2` unsafe: the filesystem path uses `std` + `fs4`, so the mmap unsafe that was
originally anticipated never materialized.
