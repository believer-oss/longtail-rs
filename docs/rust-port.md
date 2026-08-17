# The pure-Rust longtail port

How this workspace was built and why. It is a pure-Rust reimplementation of
[longtail](https://github.com/DanEngelbrecht/longtail) (the C library) and the parts of
[golongtail](https://github.com/DanEngelbrecht/golongtail) we use — byte-compatible with the
existing on-disk formats and stores. For the exact wire formats see
[`docs/format-spec.md`](format-spec.md); for measured performance see §Performance below. C
citations are against longtail `@96241fe` and golongtail `@49a20e1`.

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
structured errors) and a CI/CD pipeline that needs a golongtail-compatible CLI.

## What was ported, and where it lives

| Crate | Role |
|---|---|
| `longtail-core` | Sync, no tokio. The four on-disk formats (byte-cursor unaligned little-endian codecs), `FileInfos` scan/sort, the HPCDC chunker (exact port) plus FastCDC (benchmarking only), the hash layer (blake3, blake2s, meow parse-only), the compression registry, the store-index algebra, and version build/diff. |
| `longtail-store` | tokio-native. Blob stores (fs/mem/S3), the `RemoteBlockStore` actor, the `Cache`/`Compress` block-store decorators, and optimistic store-index sync (fs lock + S3 shard-merge). |
| `longtail` | The facade: `downsync`/`upsync` operations, the `ChangeVersion2` apply flow, the error tree, and progress/cancellation. |
| `longtail-cli` | A `clap` binary, installed as `longtail-rs` — the golongtail CLI replacement. |
| `longtail-sys`, `longtail-ffi` | **Legacy.** The C bindings and their safe wrappers. Not a dependency of anything shipped — they exist as the reference oracle for differential regression testing, which is worth running for as long as both implementations write the same stores. |

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
Incremental downloads are scan-bound *when the target scan runs* — the benchmark forces it, while
the default cached target index skips it (see the Roadmap). On the upload path, flush-path peak RSS
is roughly halved against golongtail after the block-write and store-index changes.

The compat-critical micro paths — the HPCDC chunker (block identity), the hashes, and the index
codecs — all measured within ±10 % of C, at parity or faster. Two measurements settled decisions
that would otherwise get revisited:

- **FastCDC stays benchmarking-only.** It chunks ~2.85× faster but shows *no* dedup advantage (the
  download-avoided fraction is identical), and block identity is the HPCDC boundary — adopting it
  would orphan the dedup in every existing store. A throughput-only win does not justify that.
- **Owned index structs stay.** Parse and serialize are not a hot spot (4–26 GiB/s), so the
  borrow-from-buffer redesign that would complicate every codec buys nothing.

Numbers come from dated runs on stated hardware, and the reports live outside the repo — see
`## Common commands` in `CLAUDE.md` for how to reproduce them. Re-measure rather than trusting a
figure here if a decision depends on it.

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
`pack`/`unpack` (behind an `archive` feature, droppable); the `clone-store` zip fallback;
`blockstorestorage` — rather than
port its 1.6k-line virtual filesystem, `ls` is a pure index walk and `cp` is a targeted block
fetch; and `If-None-Match` on the store-index PUT, which mirrors Go: `supports_locking()` is
false and the write is an unconditional `PutObject`, leaving the HEAD-then-PUT window open. The
`BlobObject::write -> bool` CAS-lost contract already exists, so wiring the header and mapping
412 is small — but shard names are content-addressed, so it would only ever dedupe *identical*
shards. Two writers producing different merged shards (each `base + its own new blocks`, neither
a superset of the other) is exactly the case that leaves two shards behind, and the header does
not prevent it; that needs a canonical-key CAS instead.

## CLI compatibility

The binary is **`longtail-rs`**, deliberately not `longtail`: golongtail installs under that name
and both will sit on the same machines through a switchover, so a script that picked up the wrong
one would be hard to diagnose. Everything else is golongtail v0.4.5's surface verbatim, so a
pipeline step ports by changing the program name.

- **Commands and flags** keep golongtail's spelling. Nine subcommands also answer to golongtail's
  camelCase alternative (`validate`, `printVersionIndex`, `printStoreIndex`, `stats`, `dump`,
  `init`, `createVersionStoreIndex`, `cloneStore`, `pruneStore`), and `version` works as a
  subcommand as well as a flag. `--help` is the authority on the current flag set;
  [`docs/cli.md`](cli.md) is the task-oriented guide.
- **Globals** include golongtail's logging and stats flags. `--show-store-stats` is a second
  spelling of `--show-stats`; `--log-file-path`, `--[no-]log-to-console` and `--log-coloring` do
  what they say. The `--mem-trace` trio is accepted and does nothing — it instruments the C
  allocator this implementation does not use — and says so on stderr rather than producing an
  empty file.

Behaviour a pipeline has to know before switching, beyond the missing commands in §Dropped and
deferred:

- **`--use-legacy-write` returns a typed error.** Only the `ChangeVersion2` write path is
  implemented, so a pipeline must not pass it.
- **`clone-store --hash-algorithm` / `--compression-algorithm` are accepted and ignored** — the
  hash and compression tags come from the source version index. golongtail ignores them here too.
- **`clone-store` skips already-cloned versions**, where v0.4.5's swapped arguments meant the skip
  never fired (§Upstream findings). Re-running is cheaper; the end state is identical.
- **minio and other S3-compatible endpoints** need virtual-host bucket addressing, because
  golongtail's AWS SDK never sets path-style. Run minio with `MINIO_DOMAIN=<host>` and use an
  endpoint whose host resolves `<bucket>.<host>` — `127.0.0.1.nip.io` does. Real S3 is unaffected.

## Trust boundary

**A longtail store is trusted for content integrity.** Hashes in these formats are
deduplication keys, not authentication tags, and nothing in the format carries a publisher
signature. This is inherited from the C implementation, not a property of this port, and it is not
fixable within the on-disk format.

Concretely, on the download path:

- A block's `block_hash` is computed over its **chunk-hash array only** (`longtail-core/src/pack.rs`)
  — not the payload, not `chunk_sizes`, not the compression tag.
- The read-side check compares a fetched block's *self-declared* `block_hash` field against the one
  requested. It proves the object claims to be the block asked for; it does not re-derive anything
  from the bytes.
- Nothing on the read path re-hashes decoded chunk bytes unless
  [`DownsyncOptions::verify_chunks`] is set.

So a party who can write one `.lsb` object can replace an asset's contents — keeping `chunk_hashes`
and `chunk_sizes` intact, which the apply path requires — and every layer reports success.
`validate` does not close this: it re-scans the target and compares against the **source version
index's** own `content_hashes`, so it proves the download matches the index it was given, not that
the index is authentic.

`verify_chunks` (off by default) re-hashes each chunk against the version index before writing,
turning a substituted payload into a `Corrupt`-class error. Read what it does and does not cover:
it authenticates blocks **against the `.lvi`**, which is itself an unsigned object usually in the
same bucket. It defends against a tampered or corrupted block — the realistic cases being storage
corruption, a compromised CDN, or a leaked credential scoped to block objects — and not against
someone who rewrites the version index too, since they would simply recompute the chunk hashes.

Closing it end to end needs an authenticity root **outside** these formats: a publisher signature
over the `.lvi`, or its hash delivered over a channel the block store cannot influence. That is a
detached artifact rather than a format change, so it costs no compatibility. With such a pin in
place the chain completes — pinned `.lvi` → chunk hashes → payload bytes — and `verify_chunks`
becomes the link that carries it to disk rather than a partial measure.

Verification strength follows the version's hash algorithm: blake3 (the default) and blake2s are
cryptographic; meow is parse-only here (`longtail-core/src/hash.rs`), so a meow-hashed version
cannot be verified by this implementation at all.

## Resume invariants

"Pause = cancel and keep the target folder; resume = re-run the same command" holds because of two
properties. Both are easy to break by a change that looks like an improvement, so they are written
here rather than left to be inferred from the code.

- **I1 — the cached target index is deleted before the target is mutated, and rewritten only after a
  successful apply** (`crates/longtail/src/downsync.rs`). That file short-circuits the target scan
  entirely, so one that outlived an interrupted run would tell the next run the target is already
  the desired version: it would write nothing and exit 0 over a torn tree. Writing it earlier — for
  "crash resilience", say — inverts the guarantee. Pinned by
  `smoke.rs::resume_with_the_target_index_cache_enabled`, which is the only test that runs with the
  cache at its default; the rest disable it, and all of them pass with the ordering reversed.
- **I2 — the target scan's completeness test is a content hash**, not a cheaper proxy
  (`crates/longtail/src/version.rs`). It is what lets a re-run distinguish a finished asset from a
  half-written one of the same length. See the Roadmap note below for why the obvious cheaper proxy
  is not available.

The limitation the pair does not cover: damage done to the tree *behind* a cache index written by a
completed run is invisible to a cached re-run, which diffs the cache and finds nothing to do. The
recovery is a run without the cache (a full scan), which sees the truth. Pinned by
`smoke.rs::a_stale_cache_index_hides_damage_a_full_scan_finds`.

## Roadmap

- **Incremental-scan cost, and why the obvious fix is not available.** When the target scan runs, it
  dominates an incremental download: at 1 GiB, building the target index takes ≈ 1434 ms against
  ≈ 37 ms to apply. Two things bound how much that is worth. It is measured with
  `--no-cache-target-index` (`support/longtail-bench/src/bin/e2e.rs:396,439`), while the default is
  the cache *on* — and a cached target index skips the scan outright, so the number describes the
  cold case rather than the common one. The per-asset streaming half is already done
  (`crates/longtail/src/version.rs`, one `max_hash_size` part in memory at a time); what remains is
  the per-asset chunk lists, sized by worker count.

  A short-circuit keyed on size or mtime is **not** a safe way to close the rest, and cannot be
  written as stated. `VersionIndex` carries no timestamp field, and the C implementation records
  none either, so there is nothing to compare an mtime against without changing the format — which
  compatibility forbids. Size alone is worse than useless here: step 5b pre-allocates every
  write-plan file to its final size before any block arrives, so a torn file matches its recorded
  size **by construction**, and a scan that trusts size skips exactly the assets that need
  rewriting. That turns a resumable interruption into silent, permanent corruption, since the
  following run short-circuits too. Any future short-circuit must key on state written before the
  mutation and cleared after success — never on a property a half-written file already satisfies.
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
