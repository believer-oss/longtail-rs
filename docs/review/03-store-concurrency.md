# 03 · Async store & concurrency review

- **Reviewed at:** `456274d` · **Lead model:** opus · **Workers:** 5 × fable
- **Slice:** all of `longtail-store` (src + 7 test files), `deadlock_regression.rs`, and
  `longtail/src/apply.rs` on the concurrency/cancellation axis only ·
  **Confidence:** covered well for `remote.rs`/`sync.rs`/`cache.rs`/`compress.rs`/`blob/fs.rs`;
  **covered thinly for `blob/s3.rs`** — `s3_spec.rs` self-skips without
  `LONGTAIL_TEST_S3_ENDPOINT` and `15-coverage/summary.txt:23` puts `s3.rs` at **24.49 % region /
  30.95 % line**, the lowest in the crate. Every S3 claim below is read-from-source, not
  observed-in-test.

## Findings index

| ID | P | Dimension | One line | file:line | Verdict |
|---|---|---|---|---|---|
| `STORE-01` | P1 | hardening | `prune` ignores shard-delete errors, then deletes the blocks those shards reference → dangling index entries | `sync.rs:357-364` + `remote.rs:563` | CONFIRMED |
| `STORE-02` | P1 | hardening | `prune_blocks` swallows every block-delete error and reports success; the survivors become permanently unreclaimable | `remote.rs:572-576` | CONFIRMED |
| `STORE-03` | P1 | hardening | fs `atomic_write` discards the fsync error and never fsyncs the parent dir — an ACKed store-index write can vanish on power loss | `blob/fs.rs:240,242` | CONFIRMED |
| `STORE-04` | P1 | hardening | every block-get error is flattened to `Backend`, breaking the `NotAuthorized`/`Network` contract `longtail/src/lib.rs:82-85` advertises | `remote.rs:490,626-632` | CONFIRMED |
| `STORE-05` | P1 | hardening | both listing backends silently drop entries; an omitted `.lsi` shard silently narrows the store index | `blob/fs.rs:109-118`, `blob/s3.rs:303-307` | CONFIRMED |
| `STORE-06` | P2 | hardening | a torn `store.lsi` from a golongtail writer hard-fails the Rust reader — the parse sits outside the retry ladder | `sync.rs:145-149` | CONFIRMED |
| `STORE-07` | P2 | hardening | Windows mixed Rust+Go fs writers do not mutually exclude (`LockFileEx` vs `CreateFile`) → ACKed lost update | `blob/fs.rs:17-21` | CONFIRMED |
| `STORE-08` | P2 | perf | `preflight_get` holds the prefetch mutex across the whole enqueue loop and spawns one detached task per block | `remote.rs:511-530` | CONFIRMED |
| `STORE-09` | P2 | hardening | a dropped `get_stored_block` leaks its map entry and any held worker permit — latent, unreachable today | `remote.rs:485-486` | CONFIRMED |
| `STORE-10` | P2 | memory | the cache byte budget is enforced only in `close()`, which the cancel/error path never reaches | `cache.rs:163` | CONFIRMED |
| `STORE-11` | P2 | hardening | a payload-truncated cache entry is served as a hit; for a compressed store the get then fails with no fall-through | `cache.rs:94-98` | CONFIRMED |
| `STORE-12` | P2 | hardening | no `catch_unwind`, no rayon `panic_handler` anywhere → a codec panic aborts the process | `compress.rs:43-47` | CONFIRMED |
| `STORE-13` | P2 | hardening | a caller-supplied `Client` silently discards `force_path_style` / accelerate / **stalled-stream opt-out** | `blob/s3.rs:196-198` | CONFIRMED |
| `STORE-14` | P2 | hardening | cancellation is polled-only; two apply loops have no checkpoint at all | `apply.rs:147-158,267-280` | CONFIRMED |
| `STORE-15` | P2 | hardening | `clone-store` builds a token nothing cancels; `put` installs no handler | `clonestore.rs:107`, `main.rs:627/664/738` | CONFIRMED |
| `STORE-16` | P2 | hardening | the owner's fallback persist discards its error and is unreachable from the `*_blocking` wrappers | `remote.rs:656-662` | CONFIRMED |
| `STORE-17` | P2 | perf | a worker permit is held across the full 6-rung read ladder including ≈3.85 s of sleeps | `remote.rs:339` + `sync.rs:80-92` | CONFIRMED |
| `STORE-18` | P3 | idiom | three `index.as_ref().unwrap()` encode a sound invariant the compiler could enforce instead | `remote.rs:755,780,816` | CONFIRMED |
| `STORE-19` | P3 | hardening | 14 `as` casts, zero `checked_*`/`try_from` in the crate; two have reachable truncation | `remote.rs:519`, `blob/s3.rs:299` | CONFIRMED |
| `STORE-20` | P3 | complexity | four unreachable error arms, because no `Semaphore` in the crate is ever closed | `remote.rs:288,339,386`, `sync.rs:196` | CONFIRMED |

## Scope

**Read in full:** `longtail-store/src/{remote,sync,cache,compress,error,lib,block_store,uri}.rs`,
`longtail-store/src/blob/{mod,fs,mem,s3}.rs`, all seven `longtail-store/tests/*.rs`,
`longtail/tests/deadlock_regression.rs`, `longtail/src/apply.rs`.

**Read along the declared secondary axis (concurrency/cancellation only):**
`longtail-cli/src/main.rs:460-620`, `longtail/src/{options,downsync,upsync,clonestore,prune,lib}.rs`
at the token/flush/close/prune call sites. Filesystem semantics and ordering in `apply.rs` are R7's;
ordering-of-writes findings are cross-referenced, not duplicated.

**Read as the compat oracle:** `/home/chris/github/golongtail` at `49a20e1` —
`longtailstorelib/fsstore.go:120-320` and `fsstore_windows_amd64.go:39,55`.

**Excluded:** `longtail-core` (R1/R2), `version.rs` scan concurrency (R4 — noted as a
cross-reference under Lower-priority), format bytes (R1), CLI UX/exit codes (R7), release story (R8).

## Verification performed

Evidence-pack artifacts consulted: `MANIFEST.md`, `03-test.txt` (which of my tests ran and their
timings), `15-coverage/summary.txt:20-30` (per-file coverage, quoted verbatim below),
`12-loc.txt`, `05-tree.txt` (the S3 rustls pinning claim). I did **not** run cargo.

Quoted coverage for my slice, from `15-coverage/summary.txt`:

| file | region % | line % |
|---|---|---|
| `blob/s3.rs` | **24.49 %** | **30.95 %** |
| `uri.rs` | 56.83 % | 52.17 % |
| `blob/mem.rs` | 68.57 % | 73.53 % |
| `blob/fs.rs` | 75.79 % | 84.16 % |
| `blob/mod.rs` | 77.97 % | 59.09 % |
| `remote.rs` | 79.65 % | 82.06 % |
| `sync.rs` | 82.49 % | 86.24 % |
| `cache.rs` | 94.14 % | 94.93 % |
| `compress.rs` | 100.00 % | 100.00 % |
| `block_store.rs`, `error.rs` | 100.00 % | 100.00 % |

**Could not verify:** (a) anything about real S3 wire behaviour — the two behavioural
`s3_spec.rs` tests took the skip path (see the last row of "Verified good"); (b) Windows lock
semantics empirically — the divergence is proven from both sources, the *consequence* is reasoning;
(c) `rayon`'s spawned-job panic default and `Runtime::drop` task-discard semantics are
library-documented behaviour I read the code against, not behaviour I observed. Both are listed as
experiments.

---

## Deliverable 1 — the store's admission-control contract

Quotable. R4 should check facade demand against exactly this; the fixed prefetch deadlock happened
in the gap between the two halves.

> **The store has two independent admission budgets and they are not interchangeable.**
>
> 1. **`worker_sem`** (`remote.rs:231`) — `Semaphore::new(worker_count.max(1))`, one permit per
>    in-flight block I/O, acquired by `fetch_stored_block` (`:339`) and `put_stored_block`
>    (`:386`). It is **item**-denominated and it bounds *both* prefetch and demand traffic. Every
>    block fetch — background or demand — passes through it.
> 2. **`prefetch_sem`** (`remote.rs:233`) — `Semaphore::new(budget)` where
>    `budget = max_prefetch_bytes.min(Semaphore::MAX_PERMITS).min(u32::MAX as usize)` (`:224-226`),
>    default `DEFAULT_MAX_PREFETCH_BYTES = 512 MiB` (`:70`). It is **byte**-denominated and it gates
>    **background prefetch only**. Demand `get_stored_block` never touches it.
>
> **The liveness invariant** — the one the deadlock fix encodes:
>
> > *A single item larger than the entire budget must still make progress.* Concretely:
> > `permits = estimate.min(self.max_prefetch_bytes).max(1)` (`remote.rs:520`) clamps any request
> > to at most the semaphore's own total, so no `acquire_many` can ever be unsatisfiable; and
> > budget is acquired **at dispatch, not at enqueue** (`:286` before the map entry is created at
> > `:315`), so a budget-parked prefetch is never something a consumer can wait on. Together:
> > **any working set completes with any budget ≥ 1 permit.**
>
> **Consequences a caller must respect.** Progress never depends on `max_prefetch_bytes`; it
> depends only on `worker_count ≥ 1`. Conversely the byte budget is *not* a memory cap on the
> operation: peak block memory is `512 MiB` (unconsumed prefetches) **plus** `worker_count ×
> block_size` (demand fetches in flight) **plus** whatever the facade holds — apply.rs's own
> `Semaphore` (`apply.rs:182`) admits `apply_concurrency` decompressed payloads concurrently, and
> that is a *separate*, unrelated budget with the same numeric source (`resolved_worker_count`).
> Nothing anywhere reconciles the two.

**Every permit-count-vs-budget clamp in the repo, audited.** The mission asked me to find the
others; there is exactly one `acquire_many` in the codebase and it is the clamped one.

| site | primitive | count per acquire | bound | clamped? |
|---|---|---|---|---|
| `remote.rs:286` | `prefetch_sem.acquire_many_owned(permits)` | `estimate.min(max_prefetch_bytes).max(1)` | `budget` | **yes**, `:520` + `:224-226`. Provably always satisfiable. |
| `remote.rs:339`, `:386` | `worker_sem.acquire()` | 1 | `worker_count.max(1) ≥ 1` | n/a — 1 ≤ bound by construction |
| `apply.rs:201-204` | `sem.acquire_owned()` | 1 | `apply_concurrency.max(1) ≥ 1` | n/a — same |
| `sync.rs` retry ladders (`:310-323`, `:376-388`) | none — plain unbounded retry loops | — | — | no permits involved; see Lower-priority for the starvation note |
| rayon pools (`version.rs:170`, `cp.rs:100`, `inspect.rs:57,80`, `prune.rs:31`) | `ThreadPoolBuilder::num_threads(n)` | — | worker count | no byte budget is ever converted to a thread count anywhere |

So the clamp bug class is closed. The **residual** risk is not a missing clamp — it is that the two
byte-shaped budgets (store prefetch, cache size) and the two item-shaped ones (store workers, apply
concurrency) have no single owner. That is `STORE-10` and the note above.

## Deliverable 2 — durability, three questions, store side

R7 answers the same three for the target side; the synthesis pass can diff them. "Recoverable by
the next run" means a plain re-run of the same command; "recoverable by golongtail" means a
concurrent or subsequent Go writer/reader is not wedged.

### Power loss

| what was in flight | left behind | next run | golongtail |
|---|---|---|---|
| `.lsb` block PUT (S3) | nothing or the whole object — S3 PUT is atomic | fine | fine |
| `.lsb` block write (fs) | `<name>.tmp.<pid>.<hex>` orphan, or the final name — `atomic_write` temp+rename (`fs.rs:225-246`) | orphan is invisible to readers (fails the `.lsb`/`.lsi` suffix filters, `sync.rs:130`/`:434`) but is **never garbage-collected** | fine |
| consolidated shard written, superseded shards not yet deleted (`sync.rs:262` → `:268`) | new shard **plus** old inputs | merge-on-read is a union → correct; the next writer consolidates and deletes the leftovers. Self-heals. | fine — same protocol |
| locking-flavour `store.lsi` renamed, `.gen` not yet bumped (`fs.rs:353` → `:355`) | valid merged `store.lsi`, stale `.gen` | readers never consult `.gen` → readable. A live competitor holding the pre-crash snapshot passes its CAS and overwrites — but the crashed writer was never ACKed, so this is an unacked in-flight loss, and the `.lsb` blocks remain, so `--access-type init` rebuilds. | fine |
| **anything the fs backend called successful** | **possibly nothing** — `STORE-03`: `sync_all()`'s error is discarded and the parent dir is never fsynced, so a returned `Ok` is a commit-to-page-cache, not to platter | the store index reverts to its previous generation; blocks remain → `init` rebuild heals | fine |

The reverse ordering — `.gen` bumped before the data lands — **cannot occur**: the compare, the
rename and the bump are all inside one flock critical section in one `spawn_blocking` closure
(`fs.rs:347-356`), in that order. Verified against golongtail `fsstore.go:254-276`, which is
identical.

### SIGKILL

Same table as power loss, minus the `STORE-03` row: page-cache contents survive the process, so
every "successful" write is durable. Additionally:

- The flock is released — the OS drops it when the fd closes at process exit; `FlockGuard`
  (`fs.rs:178-185`) never unlinks the `._lck` file, so a leftover file cannot wedge the store.
  (This is `rust-port.md:119-121`'s documented divergence, and it is correct.)
- Accumulated block indexes not yet flushed are lost from the index. The blocks are on the store as
  orphans; `put_stored_block`'s skip-if-exists (`remote.rs:395`) makes the re-run cheap.
- `queued`/`entries` prefetch state is process-local; nothing on the store.

### ENOSPC

This is the weakest of the three.

- `atomic_write`'s `write_all` fails (`fs.rs:238`) → the error **propagates**, but the temp file is
  **left on disk**: the `remove_file(&tmp)` cleanup exists only in the rename-failure closure
  (`:243`). So ENOSPC during a store-index write leaks a temp file *and* the disk is still full.
  Nothing ever collects `.tmp` orphans under the store root.
- The block **cache** dir is the common ENOSPC victim, and there it is handled correctly by
  accident: cache writes are `let _ =` best-effort (`cache.rs:83`, `:120`), so a full cache disk
  degrades to "no caching" instead of failing the download; and `collect_cache_files`
  (`cache.rs:262-283`) has no extension filter, so leaked `.tmp` files *do* count against the byte
  budget and *are* eventually deleted by the sweep — see `STORE-DOC-05`, where the doc claims a
  filter that does not exist.
- `set_meta_generation` (`fs.rs:219-222`) is a plain non-atomic `std::fs::write` of 8 bytes. Under
  ENOSPC it can leave a 0-byte `.gen`, which `read_meta_generation` reads as generation **0**
  (`:213`, the `Ok(_)` arm) rather than erroring. golongtail's `getMetaGeneration`
  (`fsstore.go:148-159`) would instead panic on the short slice. Rust is more lenient; the
  consequence is a silent generation reset, which the flock still serialises, so it is not a lost
  update — but it is silent.

**Recoverable by a concurrent golongtail writer** in every row above: the on-store artefacts are
byte-identical in shape (see Deliverable 4), and both implementations tolerate absence and treat
the shard set as a union.

## Deliverable 3 — the whole cancellation chain

```
main.rs:525 install_cancel_handler()   →  tokio_util::sync::CancellationToken
  └─ tokio::spawn: while ctrl_c().await.is_ok() { n+=1; if n==1 { cancel() } else { exit(130) } }
       installed ONLY at main.rs:627 (downsync), :664 (get), :738 (upsync)
options.rs:66/:201/:236   pub cancel: Option<CancellationToken>   (default None at :107/:268/:314)
  └─ downsync.rs:85 / upsync.rs:60:  opts.cancel.clone().unwrap_or_default()
       → None becomes a fresh token nobody holds: cancellation silently does nothing, never panics
polled checks (there is NOT ONE select! or .cancelled().await anywhere in the repo):
  downsync.rs:103, :136        apply.rs:133, :189, :258, :430        upsync.rs:249   version.rs:57
longtail-store / longtail-core:  ZERO references to the token.  Cancellation stops at the facade.
```

**A second Ctrl-C hard-kills the process**: `main.rs:530` is a `while` loop, not a one-shot, and the
`else` arm at `:538` is `std::process::exit(130)` — no destructors, no flush, no eviction. The first
Ctrl-C prints `"Cancelling… finishing in-flight blocks (ctrl-c again to force quit)"` and calls
`watch.cancel()`. The default SIGINT disposition is never restored, so between the two signals the
process is only killable through this loop.

**Worst-case stop latency.** Because every check is polled, cancellation cannot interrupt any single
`.await`:

| checkpoint | work between consecutive checks | latency |
|---|---|---|
| `apply.rs:133` | one `create_dir` or one 0-byte file | µs–ms |
| `apply.rs:147-158` | **no check** — one `create_file_sized` per write-set asset, for the whole set | unbounded in file count (`STORE-14`) |
| `apply.rs:189` | one launched block; the check precedes `sem.acquire_owned().await` so one extra block still launches | then the drain at `:248` waits for **all** in-flight blocks |
| after the break | in-flight blocks are **drained, never aborted** (deliberate — resumable target) | the slowest of ≤ `apply_concurrency` blocks: one S3 GET including the 6-rung ladder, ≈3.85 s of sleeps plus up to 7 attempt durations |
| `apply.rs:258` | post-drain re-check, for a cancel that fired with no "next block" left | — |
| `apply.rs:430` | one `remove_asset`, per asset per retry pass (≤10) | µs–ms |
| `apply.rs:267-280` | **no check** — the permissions pass | one `set_permissions` per changed asset |

**Into the actor: it does not arrive.** The detached prefetch tasks (`remote.rs:521`) hold their own
`Arc` clones and keep fetching to natural completion; `drain_prefetch` is reached only from
`flush()`/`close()` (`:582`, `:595`), and **the cancel path never calls either** —
`downsync.rs:196-197` sits after the `?`. The actor stops only as a side effect of the store `Arc`
dropping, at which point `rx.recv()` returns `None` and the fallback persist at `:660` runs —
except in the `*_blocking` wrappers, where it cannot (`STORE-16`).

**On cancel the store is left:** partial target intact and resumable (by design, `lib.rs:71-80`),
`.lrb` cache entries valid but **not evicted to budget** (`STORE-10`), for upsync the uploaded
`.lsb` blocks present as orphans not yet in the store index, and any in-flight prefetch permits
released only by process exit.

## Deliverable 4 — the `.gen` / shard-layout half of the upgrade & rollback statement

R1 owns format versions, R8 owns the release story. My half is the *store layout* — and it is the
good news of this review. I diffed against golongtail `49a20e1`.

**Can golongtail read a store this CLI wrote? Yes.** All four layout artefacts are
byte-for-byte the same mechanism:

| artefact | Rust | golongtail `49a20e1` | verdict |
|---|---|---|---|
| shard name | `format!("store_{sha256:x}.lsi")` over the exact serialized bytes, 64 lowercase hex (`sync.rs:106-109`) | `fmt.Sprintf("store_%x.lsi", sha256)` (`remotestore.go:1213`) | identical; pinned by `sync_fixtures.rs:41` against `fixtures/stores/sharded/` |
| shard discovery | list prefix `store`, keep `size > 0 && ends_with(".lsi")`, then **sort** (`sync.rs:128-137`) | same filter, list order | identical set; the sort is a documented determinism divergence, and merge-on-read is a union so a differently-ordered peer converges |
| `.gen` sidecar | `<path>.gen`, 8 bytes **LE i64**, missing → 0, `-1` = "never locked" (`fs.rs:206-222`, `:156`) | `metapath := path + ".gen"`; `binary.LittleEndian.PutUint64`; `metageneration = -1` sentinel (`fsstore.go:150-167`, `:67`) | identical |
| lock file | `<path>._lck`, never unlinked, filtered out of listings (`fs.rs:160-164`, `:124`) | `lockPath := path + "._lck"`, filtered at `fsstore.go:82` | identical |
| CAS order | check gen → rename data → bump gen, all in one flock section (`fs.rs:347-356`) | check gen → `WriteFile` → `setMetaGeneration` under `defer Unlock` (`fsstore.go:254-276`) | identical ordering |

**Can this CLI read a store golongtail wrote? Yes, with one caveat.** golongtail's fs write is
`ioutil.WriteFile` — **in place**, no temp+rename (`fsstore.go:266`). A Rust reader with locking
disabled (which is what a `ReadOnly` downsync uses, `uri.rs:166-168`) can therefore observe a torn
`store.lsi`, and `STORE-06` makes that a hard failure rather than a retry. Rust→Go is strictly
safer than Go→Rust here, because Rust always writes temp+rename.

**Rollback (this CLI's store → an older golongtail):** safe. Nothing new is written. The only
artefacts a golongtail-only world will not recognise are cosmetic: leftover `._lck` files (which
golongtail also leaves) and `.tmp.<pid>.<hex>` orphans from crashed Rust writes, which fail both
suffix filters on both sides.

**Two shape divergences worth knowing.** (i) A *lockless-configured* client pointed at an fs store
folds the canonical `store.lsi` into a shard and deletes it (`sync.rs:268-275`) — content preserved,
convergent, mirrors Go, but the on-disk shape flips. Which flavour you get depends on
`enable_locking`, and the two public URI entry points disagree about it: `uri.rs:168` derives it
from access type while `blob/mod.rs` hardcodes `false` in all five fs constructions
(`:122,126,151,171,182`) — see `STORE-DOC-06`. (ii) **On Windows, mixed Rust+Go fs writers do not
mutually exclude** — `STORE-07`. That is the one real rollback/upgrade hazard in this half, and it
is missing from the keeper docs (`STORE-DOC-01`).

**`.longtail.index.cache.lvi` from an older build** is the target side (R7/R1). My half — the
*cache dir's* `store.lsi` — is advisory by design: `evict_cache_dir` never touches it
(`cache.rs:218-219`) and an evicted block is transparently re-fetched and re-cached
(`decorators_integration.rs:57-91` regression-tests exactly this against the C library's
authoritative-cache-index bug). A stale cache-dir `store.lsi` from any build is therefore inert.
The `.lrb` layout is `chunks/<4 lowercase hex>/0x<16 lowercase zero-padded hex>.lrb`
(`cache.rs:70-74`) — a warm cache written by the C library is read directly.

---

## Findings

### `STORE-01` — `prune` can leave the store index referencing blocks it just deleted

**P1** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/sync.rs:357-364` and `crates/longtail-store/src/remote.rs:563-577`
- **What:** `prune_blocks` overwrites the store index first, then deletes the orphaned blocks —
  correct ordering, and `remote.rs:560-562` says so. But `try_overwrite`'s job of making the new
  index *authoritative* is best-effort: it deletes every other `store*.lsi` item with
  `let _ = old.delete().await` (`sync.rs:362`) and returns `Ok(true)` at `:365` **regardless of
  whether any delete succeeded**. `overwrite_remote_store_index` therefore reports success on a
  store that still holds pre-prune shards.
- **Failure scenario:** a stale shard `store_OLD.lsi` fails to delete — an S3 `AccessDenied` on
  `s3:DeleteObject` (a read-mostly IAM policy), a Windows sharing violation because a concurrent
  reader has the file open, or the shard was never listed at all (`STORE-05`). `prune_blocks` then
  proceeds to `remote.rs:567-577` and deletes the `.lsb` files that `store_OLD.lsi` references.
  Merge-on-read now unions `store_OLD.lsi` back in, so `get_existing_content` reports blocks that
  no longer exist, and the next `downsync` fails with `NotFound` on a `chunks/**.lsb` fetch. This is
  the exact state `remote.rs:560-562` claims is impossible.
- **Evidence:** `sync.rs:345` lists, `:357-364` deletes with the error discarded, `:365` returns
  `Ok(true)` unconditionally; `remote.rs:563` awaits that, `:567-577` deletes blocks. Nothing
  re-reads or verifies the index afterwards. `prune_store_with_locking` /
  `prune_store_without_locking` (`remotestore_spec.rs:306-313`) exercise only the happy path — no
  test injects a delete failure anywhere in the crate.
- **Recommendation:** make `try_overwrite` return the delete outcome and have
  `overwrite_remote_store_index` fail the prune if any superseded item survived. At minimum,
  re-read the merged index after the overwrite and assert it equals `pruned` before deleting a
  single block; a mismatch should abort with the surviving shard named. Also `tracing::warn!` each
  ignored delete (the module currently emits no tracing at all — `STORE-DOC-07`).
- **Tradeoff / risk:** golongtail has the same best-effort shape, so failing harder is a behavioural
  divergence — but in the safe direction (refuse to prune vs. corrupt the index), and it must be
  added to `rust-port.md` §Deliberate divergences. Not a byte-compat change.
- **Effort:** M
- **Regression test to add:** a `BlobStore` decorator whose `delete` fails for one specific
  `store_*.lsi` key; assert `prune_store` returns `Err` and that no `.lsb` was deleted. The
  companion case — delete succeeds for the shard but fails for a block — is `STORE-02`.

### `STORE-02` — `prune_blocks` reports success on partial failure, and the survivors become unreclaimable

**P1** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/remote.rs:572-576`
- **What:**
  ```rust
  if let Ok(mut obj) = self.client.new_object(&key).await
      && obj.delete().await.is_ok()
  {
      pruned_count += 1;
  }
  ```
  Every failure — `new_object` and `delete` alike — is swallowed. `prune_blocks` returns a count,
  never an error.
- **Failure scenario:** an IAM policy denies `s3:DeleteObject`. `prune_store` reports
  "pruned 0 blocks" and exits 0; the operator reads that as "nothing to prune". Worse, the deletion
  is now *permanently* unrecoverable: `source` on the next run comes from
  `get_index_snapshot()` (`:557`), i.e. the already-overwritten index, which no longer lists those
  hashes — so no future `prune_blocks` will ever consider them again. The bytes are paid for
  forever, and only a manual `prune-store-blocks --blocks-root-path` sweep finds them.
- **Evidence:** `remote.rs:557` reads the pre-prune index into `source`; `:563` overwrites the store
  index; `:567-577` iterates `source.block_hashes` and silently skips failures. The identical
  swallow exists in the facade at `prune.rs:428-430` (R4/R7's file — cross-reference, not a second
  finding). No test injects a block-delete failure.
- **Recommendation:** collect the failures and return them. `prune_blocks` should return
  `(pruned, failed)` or an error naming the first failure; the CLI must not print a bare success
  when `failed > 0`. Separately, delete concurrently under `worker_sem` instead of serially — a
  100k-block prune is currently 100k sequential round trips.
- **Tradeoff / risk:** a stricter return type is a `BlockStore` trait signature change (one method,
  three implementors). No byte-compat impact.
- **Effort:** S for the reporting, M for the trait change.
- **Regression test to add:** a blob-store decorator that fails `delete` for a fixed hash; assert
  `prune_blocks` surfaces it and that the CLI exits non-zero.

### `STORE-03` — the fs backend discards its fsync error and never fsyncs the parent directory

**P1** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/blob/fs.rs:240` and `:242`
- **What:**
  ```rust
  f.write_all(data).map_err(...)?;
  f.sync_all().ok();                       // :240 — error discarded
  }
  std::fs::rename(&tmp, path).map_err(...) // :242 — no fsync of `path.parent()` after
  ```
  `atomic_write` is the write primitive for **every** fs blob: `store.lsi`, every `store_*.lsi`
  shard, every `.lsb` block, and every `.lrb` cache entry.
- **Failure scenario:** `sync_all` fails with EIO or a delayed-allocation ENOSPC. The rename still
  happens, the call returns `Ok(true)`, `add_to_remote_store_index` returns `Ok`, and the CLI exits
  0 — while the new `store.lsi` content was never handed to stable storage. On power loss the store
  index reverts to its previous generation, and every block uploaded in that session is an orphan
  the index does not mention. Independently, with no parent-directory fsync, even a *successful*
  `sync_all` does not make the rename durable: on ext4 with `data=writeback`, or on XFS, a crash
  can leave the directory entry absent, i.e. the object simply disappears.
- **Evidence:** I read every line of `fs.rs`; `:240` is the only `sync` call in the file, and there
  is no `File::open(parent)` + `sync_all()` anywhere. `blobstore_spec.rs` has no crash-consistency
  test and no I/O-fault injection; `fs.rs` function coverage is 61.70 % (`15-coverage/summary.txt:20`).
- **Recommendation:** propagate the `sync_all` error, and fsync the parent directory after the
  rename (on Unix; on Windows the rename is already metadata-journaled, so gate it with `#[cfg]`).
  The cost is one extra fsync per store-index write — negligible, since there is exactly one per
  flush. For `.lsb`/`.lrb` bulk writes, keeping the current behaviour behind an explicit
  `durable: bool` is defensible; silently discarding the error is not.
- **Tradeoff / risk:** a per-block parent fsync would be a real throughput cost on an upsync to a
  local store; scope the dir-fsync to the store-index path if that shows up in `10-release.txt`
  timings. No bytes change, so no compat risk.
- **Effort:** S
- **Regression test to add:** hard to test crash-consistency in-process; the tractable half is a
  `sync_all`-failure injection (write to a path on a full tmpfs) asserting `write()` returns `Err`.
  File the dir-fsync as a reviewed invariant plus a comment, not a test.

### `STORE-04` — every block-get error is flattened, breaking the documented `NotAuthorized` / `Network` contract

**P1** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/remote.rs:490` and `:626-632`
- **What:** `get_stored_block`'s error arm is `Err(e) => Err(clone_store_error(&e))`, and
  `clone_store_error` preserves only two variants:
  ```rust
  match e {
      StoreError::NotFound(s)  => StoreError::NotFound(s.clone()),
      StoreError::BadFormat(s) => StoreError::BadFormat(s.clone()),
      other => StoreError::Backend(format!("{other}")),
  }
  ```
  So `NotAuthorized`, `Network`, `Io { source }`, `AccessViolation` and `Compress` all collapse into
  `Backend(String)` — on **every** get, not just coalesced ones, because the `Shared` future always
  wraps the error in an `Arc`.
- **Failure scenario:** `crates/longtail/src/lib.rs:82-85` documents the opposite as a public
  guarantee: *"Re-exported so a facade-only consumer can match on the store-error classes
  (`StoreError::NotAuthorized` / `Network` / `NotFound`)"*, and `error.rs:70-82` justifies the split
  as existing precisely *"so a consumer can render 'check your connection' vs 'credentials
  rejected'"*. The Tauri app doing a 40 GB download hits a 403 on one `chunks/**.lsb` — credentials
  rotated mid-transfer and the refresh failed — and `matches!(e, StoreError::NotAuthorized(_))`
  is false. It cannot distinguish "re-authenticate" from "retry later", which is the one decision
  the split was built for. The human-readable message survives inside the string; only programmatic
  dispatch is lost. Note the store-index load path is *not* flattened (it replies through the
  oneshot directly, `:247`), so the first error a user hits is typed and the per-block ones are not
  — the worst possible inconsistency.
- **Evidence:** `remote.rs:485-491`; `error.rs:70-86`; `longtail/src/lib.rs:82-85`. No test asserts
  any error variant out of `get_stored_block` other than `NotFound` (`apply.rs:930`), so nothing
  catches this.
- **Recommendation:** extend `clone_store_error` to preserve `NotAuthorized`, `Network`,
  `LockingNotSupported`, `GenerationMismatch`, `NotSupported`, `AccessViolation` and `InvalidUri`
  (all are `Clone`-able payloads or unit), and add an `#[allow]`-free exhaustive `match` with no
  catch-all so a new variant is a compile error. `Io` and `Format`/`Compress` genuinely cannot be
  cloned — give them a dedicated lossy arm that at least records the `io::ErrorKind`.
- **Tradeoff / risk:** none — the function is private and the change is additive.
- **Effort:** S
- **Regression test to add:** a blob store whose `read` returns `StoreError::NotAuthorized`; assert
  `get_stored_block` returns `NotAuthorized`, both on the demand path and on a coalesced path with
  two concurrent waiters. Add the exhaustive-match assertion by deleting the `other =>` arm.

### `STORE-05` — both listing backends can silently return a short list, and that silently narrows the store index

**P1** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/blob/fs.rs:109-118` and `crates/longtail-store/src/blob/s3.rs:303-307`
- **What:** the fs walker `continue`s past a failed `read_dir` (`:111`), a failed per-entry
  `io::Error` via `entries.flatten()` (`:113`), and a failed `entry.metadata()` (`:117`) — three
  silent skips, returning `Ok(short_list)`. The S3 pager breaks out of its loop if
  `is_truncated().unwrap_or(false)` is true but `next_continuation_token()` is `None` (`:303-307`),
  also returning `Ok(short_list)`. Both feed `get_store_store_indexes` (`sync.rs:113-138`).
- **Failure scenario:** one `store_*.lsi` shard is invisible for a moment — a transient EIO, an NFS
  hiccup, a permission-restricted file, or on Windows another process holding it open.
  `merge_store_index_items` merges the visible shards and `read_store_store_index_with_items`
  returns a store index that is **missing that shard's blocks, with no error**. On the `add` path
  this self-heals, and that is worth stating precisely: `try_write_shard` deletes only the items it
  actually merged (`existing_items`, `sync.rs:294` → `:268-275`), so the invisible shard survives and
  the next read picks it up. **On the `try_overwrite` path it does not self-heal** — the invisible
  shard is not deleted, and `STORE-01` then deletes the blocks it references. Independently, a
  narrowed index makes a `downsync` fail with `chunk … required by … not in the store index`
  (`apply.rs:114`) for content that is present on the store.
- **Evidence:** `fs.rs:105-144` read in full — `Ok(out)` at `:143` regardless of how many entries
  were skipped. `s3.rs:303-307` read in full. `sync.rs:130`'s `size > 0` filter cannot compensate
  because an omitted entry has no size to filter. The S3 half is PLAUSIBLE-reachability rather than
  CONFIRMED: the S3 API contract says `IsTruncated=true` always carries a token, so it needs a
  non-conformant S3-compatible endpoint (minio, Ceph, R2) — but the store explicitly supports custom
  `endpoint_url` (`s3.rs:90`), and `s3.rs` is at 24.49 % coverage.
- **Recommendation:** `get_objects` must distinguish "listed everything" from "listed what it
  could". Return the error for the fs walker (Go's `filepath.Walk` swallow that
  `fs.rs:102-104` cites is Go's bug, not a compat requirement — a listing is not a byte format).
  For S3, `is_truncated && token.is_none()` must be a hard `StoreError::Backend`, never a `break`.
  If parity with Go's swallow is judged mandatory, then at absolute minimum
  `get_store_store_indexes` must not be built on a lossy primitive.
- **Tradeoff / risk:** failing a listing that used to succeed will surface real permission
  misconfigurations as errors instead of mysterious "chunk not in store index" failures — a strict
  improvement in diagnosability, but it is a behaviour change on stores with unreadable stray files.
  **Not** a byte change; no existing gate covers it, which is itself the finding.
- **Effort:** S
- **Regression test to add:** an fs store with a `chmod 000` file inside `store/`; assert
  `read_merged_store_index` errors rather than silently returning a narrowed index. For S3, a
  `StaticReplayClient` returning `IsTruncated=true` with no token (this needs no minio, so it fits
  the per-PR lane).

### `STORE-06` — a torn `store.lsi` from a golongtail writer hard-fails the Rust reader

**P2** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/sync.rs:141-150`
- **What:** `read_store_index_from_path` calls `read_blob_with_retry` — which retries reads through
  a 6-rung ladder — and then parses **outside** that ladder:
  ```rust
  let (data, retries) = read_blob_with_retry(client, key).await?;
  if data.is_empty() { return Err(StoreError::NotFound(key.to_string())); }
  Ok((StoreIndex::from_bytes(&data)?, retries))
  ```
  A `FormatError` from `from_bytes` is terminal. `merge_store_index_items` only rescans for
  `is_not_found` (`:166`), so a parse failure propagates all the way out and fails the operation.
- **Failure scenario:** golongtail writes `store.lsi` **in place** — `ioutil.WriteFile`,
  `fsstore.go:266`, no temp+rename. A Rust `ReadOnly` downsync has locking disabled
  (`uri.rs:166-168`) so it takes no flock on read (`fs.rs:311-315`), reads a half-written
  `store.lsi`, `from_bytes` rejects it, and the download dies with a format error naming a file that
  is perfectly valid a millisecond later. Retrying the whole command works, which makes this look
  like a flake.
- **Evidence:** `sync.rs:145-149`; `fs.rs:311-315`; `uri.rs:166-168`; golongtail `fsstore.go:266`
  (verified from source at `49a20e1`). The `size > 0` filter at `sync.rs:130` passes a partial file.
  No mixed-writer fs test exists — `sync_fixtures.rs` uses `MemBlobStore` only
  (`sync_fixtures.rs:69`), and the mixed-writer gate is minio-only.
- **Recommendation:** move the parse inside the retry ladder — treat a `FormatError` on a
  store-index read as retryable, exactly as a transport error is. A store index that is *genuinely*
  corrupt then fails after 6 rungs with the same error, which is the correct outcome; a torn one
  succeeds on rung 2.
- **Tradeoff / risk:** adds up to ≈3.85 s before reporting a genuinely corrupt index. Retrying a
  read changes no bytes. Compat-safe.
- **Effort:** S
- **Regression test to add:** a blob-store decorator whose first `read` of `store.lsi` returns a
  valid index truncated to half its length and whose second returns the whole thing; assert
  `read_merged_store_index` succeeds.

### `STORE-07` — Windows mixed Rust+Go fs writers do not mutually exclude

**P2** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/blob/fs.rs:17-21` (and `:30`, `:201`)
- **What:** `fs4::fs_std::FileExt::lock_exclusive` uses `LockFileEx` on Windows. golongtail's
  Windows lock is a `syscall.CreateFile` exclusive-open retry loop
  (`fsstore_windows_amd64.go:39`, `:55`). These are different mechanisms and do not exclude each
  other.
- **Failure scenario:** on Windows, a Rust `upsync` and a golongtail `upsync` run against the same
  fs store concurrently. Both acquire "the lock" (different mechanisms), both read `.gen` = G, both
  pass their own generation check (`fs.rs:349` / `fsstore.go:260`), both write `store.lsi`, both bump
  `.gen` to G+1. One writer's merged index overwrites the other's, and that writer received a
  **success** — an ACKed lost update, not an unacked in-flight one. This is the only ACKed-loss
  scenario I found. The `.lsb` blocks survive, so `--access-type init` rebuilds a correct index;
  until someone does that, the store index is missing one session's blocks.
- **Evidence:** `fs.rs:17-21` documents it as an accepted divergence; I verified the Go side from
  source (`fsstore_windows_amd64.go:39,55` — `syscall.CreateFile`, no `LockFileEx` anywhere in
  `longtailstorelib/`). CI runs the pure lane and the differential lane on Windows, but the
  mixed-writer chaos test is minio-only (`.github/workflows/s3-minio.yaml`), i.e. S3-flavour,
  i.e. lockless — so **no gate covers this at all**.
- **Recommendation:** it is a legitimate accept, but it must be an *informed* one: (a) put it in
  `rust-port.md` §Deliberate divergences (`STORE-DOC-01`); (b) state in `readme.md` that concurrent
  mixed Rust/Go writers to a **filesystem** store on Windows are unsupported — S3 stores are
  unaffected because both sides are lockless and content-addressed there; (c) if a mixed
  Windows fleet is actually planned, the cheap fix is to adopt Go's exclusive-`CreateFile` retry on
  the Windows `#[cfg]` path instead of `fs4`.
- **Tradeoff / risk:** switching to `CreateFile` on Windows means hand-rolling a retry loop and
  losing `fs4`'s cross-platform uniformity, for a case the production topology (S3) never hits.
  Documenting is the right first move.
- **Effort:** S to document, M to change the mechanism.
- **Regression test to add:** none is practical in-process. This is a documentation + support-matrix
  item, and that is worth saying explicitly rather than filing an untestable test.

### `STORE-08` — `preflight_get` holds the prefetch mutex across the whole enqueue loop and spawns one detached task per block

**P2** · `perf` · CONFIRMED

- **Where:** `crates/longtail-store/src/remote.rs:511-530`
- **What:** `let mut st = self.prefetch.lock().await;` at `:511`, then a `for` loop over **all**
  `block_hashes` that does a map lookup, a set insert, an arithmetic clamp and a
  `tokio::spawn(dispatch_prefetch(...))` with five `Arc` clones per iteration — with the guard held
  to the end of the function at `:531`.
- **Failure scenario:** `apply.rs:84` calls
  `store.preflight_get(&store_index.block_hashes)` with the *entire* retargetted store index in one
  call — no batching. Two costs follow. (i) Every concurrent `get_stored_block` blocks on
  `self.prefetch.lock()` at `:453` for the whole loop, so the first `apply_concurrency` block tasks
  all stall behind the enqueue of the last block. (ii) The spawn count equals the working-set block
  count, all detached, all parked on `prefetch_sem` (512 MiB / ~8 MiB ≈ 64 run, the rest wait), each
  holding five `Arc`s. For a 100 GB delta that is ~12.5 k tasks; the memory is a few MB, so this is
  a latency and scheduler-pressure finding, not an OOM one — but the shape scales with remote input
  and has no cap.
- **Evidence:** `remote.rs:511-531` read in full; the guard is a `tokio::sync::MutexGuard` and there
  is no `.await` inside the loop, so this is a long critical section rather than a deadlock.
  `apply.rs:84` passes the whole set. No test preflights more than 8 hashes
  (`prefetch_budget.rs:115-147`), so the contention is invisible to the suite.
- **Recommendation:** collect the `(hash, permits)` pairs under the lock, drop the guard, then spawn
  outside it — a two-line change that removes the contention entirely. Separately, replace
  one-task-per-block with a small fixed pool of dispatcher tasks draining the `queued` set
  (this is closer to Go's bounded `remoteWorker` pool, so it is a convergence, not a
  re-architecture). Do **not** change where the permit is acquired — that is the deadlock fix.
- **Tradeoff / risk:** the spawn-outside-the-lock half is free. The dispatcher-pool half touches the
  liveness argument and needs the `prefetch_budget.rs` suite green plus both
  `deadlock_regression.rs` variants; defer it unless a profile justifies it.
- **Effort:** S for the lock scope, M for the pool.
- **Regression test to add:** assert that a `get_stored_block` for hash X completes while a
  `preflight_get` of 10 000 hashes is in progress, under a `timeout` well below the enqueue time.

### `STORE-09` — a dropped `get_stored_block` leaks its map entry and any held worker permit

**P2** · `hardening` · CONFIRMED (latent — not reachable through today's API)

- **Where:** `crates/longtail-store/src/remote.rs:485-486`
- **What:**
  ```rust
  let result = fut.await;                                   // :485
  self.prefetch.lock().await.entries.remove(&block_hash);   // :486
  ```
  The removal is *after* the await, with no guard. If the caller's future is dropped at `:485`, the
  `PrefetchEntry` stays in `entries` forever. For a **demand-claimed** entry the `Shared` wraps the
  real `fetch_stored_block` future (`:464-472`), so the map's clone keeps a *suspended* fetch alive —
  including the `worker_sem` permit it acquired at `:339`. Nobody polls it, so the permit is never
  released.
- **Failure scenario:** the hazard needs store reuse across a dropped operation. Cancel or time out
  `worker_count` concurrent gets, then reuse the same store: every `worker_sem` permit is held by an
  orphaned suspended future and the next fetch blocks forever. **This is not reachable today** — the
  store is constructed inside each op (`downsync.rs:157`) and dropped with it, and `close()`'s
  `drain_prefetch` (`:595`, `:618-622`) clears `entries` and releases the permits. It is reachable
  the moment any API hands a store to a caller who reuses it, and nothing in the code says so.
  Note the *dispatched-prefetch* path is immune by design: its `Shared` wraps a oneshot, not the
  fetch, so the spawned task drives it independently (`:304-314`) — that asymmetry is worth keeping
  and worth documenting.
- **Evidence:** `remote.rs:452-491` traced in full; `Arc::try_unwrap` at `:489` confirms the
  intended handle accounting. `prefetch_budget.rs` has no test that drops a get mid-flight
  (confirmed across all seven test files).
- **Recommendation:** make the removal drop-safe — a small guard struct holding
  `Arc<Mutex<PrefetchState>>` + the hash whose `Drop` removes the entry, or restructure so the
  demand path also drives its fetch from a spawned task (making both paths symmetric). Then state
  the store-lifetime invariant in the module doc, because the current safety rests on it.
- **Tradeoff / risk:** a `Drop` impl cannot take an async lock; use `try_lock` with a
  fall-back-to-spawn, or switch `PrefetchState` to a `std::sync::Mutex` (it is never held across an
  await today — verified — so that is viable and would also fix `STORE-08`'s guard problem).
- **Effort:** M
- **Regression test to add:** spawn a `get_stored_block`, abort it while gated (the `GatedStore`
  harness at `prefetch_budget.rs:307-383` is exactly the tool), then assert
  `worker_sem.available_permits()` recovers and a subsequent get of a *different* block completes
  under a timeout.

### `STORE-10` — the cache byte budget is enforced only in `close()`, which the cancel and error paths never reach

**P2** · `memory` · CONFIRMED

- **Where:** `crates/longtail-store/src/cache.rs:163`
- **What:** `evict_cache_dir` runs only inside `CacheBlockStore::close`, and only when
  `size_limit.is_some()`. There is no per-write or per-read enforcement, so during a run the cache
  grows without bound.
- **Failure scenario:** two compounding cases. (i) A single download larger than
  `--cache-size-limit` writes the whole thing to disk and only trims at the end — so the limit is a
  post-hoc trim, not a cap, and peak disk use is the download size regardless of the flag. On a
  Tauri user's laptop that is the difference between "uses 10 GB" and "fills the disk". (ii)
  `downsync.rs:196-197` calls `flush()`/`close()` **after** the `?` on `change_version2`, so any
  error — including `Cancelled` — skips `close()` entirely and therefore skips eviction entirely. A
  user who starts and cancels a large download repeatedly accumulates cache with the limit never
  once applied, and a second Ctrl-C (`main.rs:538` `process::exit(130)`) guarantees it.
- **Evidence:** `cache.rs:159-183` (eviction is inside `close`, gated on `size_limit`);
  `cache.rs:39-42`'s own field doc says "a post-run LRU eviction sweep on close";
  `downsync.rs:191` is the `?`, `:196-197` the flush/close. `decorators_integration.rs:126-173`
  tests eviction only via an explicit `close()` on the success path.
- **Recommendation:** two independent fixes. (a) Run eviction on the error path too — a
  `scopeguard`/`Drop`-style teardown, or an explicit `close()` before returning `Err` in
  `downsync.rs`. (b) Add a coarse mid-run check: track bytes written since the last sweep and
  re-sweep every N bytes (N ≈ the budget/4), so the limit is a real ceiling. Also fix the doc — see
  `STORE-DOC-08`, since `rust-port.md`'s bullet describes the sweep without saying it is post-hoc
  and skipped on failure.
- **Tradeoff / risk:** mid-run eviction races concurrent reads. That race is already benign and
  proven so — reader `exists` at `cache.rs:94` → evictor unlink at `:247` → reader `File::open` at
  `:95` returns `NotFound` (`fs.rs:318-320`) → the let-chain fails → fall through to the remote at
  `:117`. So the mechanism is safe; the cost is occasional refetch of a block evicted between probe
  and open.
- **Effort:** S for (a), M for (b).
- **Regression test to add:** for (a), run a `downsync` whose apply fails partway with
  `cache_size_limit` set, and assert the cache dir is within budget afterwards. For (b), a unit test
  that writes 3× the budget through `put_stored_block` and asserts the on-disk total never exceeds
  budget + N.

### `STORE-11` — a payload-truncated cache entry is served as a valid hit

**P2** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/cache.rs:94-98`
- **What:** the cache-hit test is: file exists, `StoredBlock::from_bytes` parses, and
  `block.block_index.block_hash == block_hash`. `from_bytes` accepts any payload length, including
  0 (`longtail-core` `block.rs:129-134`), and `block_hash` hashes the block *index*, not the
  payload — so `:97` is a name-consistency check, not an integrity check. A file with an intact
  index header and a truncated payload passes all three.
- **Failure scenario:** a `.lrb` is truncated — ENOSPC during the write-back (`cache.rs:120`,
  best-effort so the error is discarded), a power loss with `STORE-03`'s missing dir fsync, or a
  crash that landed the rename but not the data. On the next run the cache returns `Ok(block)` with
  a short payload. For a **compressed** store the `CompressBlockStore` decode above it then fails
  the whole `get_stored_block` with `StoreError::Compress` — and because the cache already returned
  `Ok`, there is **no fall-through to the remote and no eviction of the bad entry**, so the
  download fails identically on every retry until the user manually deletes the cache. For an
  **uncompressed** store the truncation passes silently into chunk assembly.
- **Evidence:** `cache.rs:88-123` read in full; the `if` chain at `:94-98` is the only validation.
  Contrast `remote.rs:362-367`, which *does* re-check the hash against the path for remote reads.
  `decorators_integration.rs:57-91` tests the evicted-by-*deletion* case (which recovers correctly)
  but never a corrupt or truncated file — confirmed across all four tests in that file.
- **Recommendation:** add a length check on the cache-hit path: compare
  `block.payload.len()` against `Σ block_index.chunk_sizes`, which the index already carries. A
  mismatch should delete the entry and fall through to the remote — the same self-healing shape the
  existing `from_bytes`-failure path already has. That is a cheap arithmetic check, no re-hashing.
- **Tradeoff / risk:** none for a compressed store, where payload length is the *compressed* size
  and the check must therefore be `<= Σ chunk_sizes` rather than `==`. Get that inequality right or
  the check will reject valid compressed entries — that is the one hazard here, and it argues for
  doing the check as `payload.is_empty() && Σ chunk_sizes > 0` plus letting the decoder's existing
  `compressed_size` check catch the rest.
- **Effort:** S
- **Regression test to add:** write a valid `.lrb`, truncate it to the block-index length + 4 bytes,
  then `get_stored_block`; assert the block comes back correct (from the remote) and that the cache
  entry was rewritten.

### `STORE-12` — no `catch_unwind` and no rayon panic handler: a codec panic aborts the process

**P2** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/compress.rs:43-47`
- **What:** the whole rayon↔tokio bridge:
  ```rust
  let (tx, rx) = oneshot::channel();
  pool.spawn(move || {
      let _ = tx.send(f());
  });
  rx.await.expect("rayon codec task dropped its result")
  ```
  If `f()` panics, the unwind drops `tx` unsent, `rx.await` yields `Err(RecvError)` and the
  `expect` panics on the caller's tokio task. Simultaneously the panic escapes rayon's
  fire-and-forget `spawn`, where — with **no `panic_handler` configured on any `ThreadPoolBuilder`
  in the repository** — rayon's documented default is to abort the process. The abort wins the race.
- **Failure scenario:** a bug or an allocation failure inside `encode_block_payload` /
  `decode_block_payload` takes down the whole Tauri app process rather than surfacing a
  `StoreError::Compress` the app can report. The codecs document a never-panic contract
  (`longtail-core/src/compress.rs:291`), so this is a defence-in-depth gap rather than a live bug —
  but the blast radius is maximal and the input is remote-supplied bytes.
- **Evidence:** `compress.rs:37-48` quoted in full above; repo-wide grep for `panic_handler` and
  `catch_unwind` returns nothing, and every `ThreadPoolBuilder` (`version.rs:170`, `cp.rs:100`,
  `inspect.rs:57,80`, `prune.rs:31`, `decorators_integration.rs:201`, `lvi_byte_gate.rs:169`) sets
  only `num_threads`. No `panic = "abort"` in any `Cargo.toml`, so the tokio side would otherwise
  contain the panic. rayon's default is library-documented behaviour, listed as experiment #2.
- **Recommendation:** wrap the closure body in `std::panic::catch_unwind(AssertUnwindSafe(f))` and
  map `Err` to a `CompressError`/`StoreError::Backend` carrying the panic message. That turns the
  worst case from "process abort" into "one block fails". Optionally set a `panic_handler` on the
  pools as well, but `catch_unwind` at the bridge is the one place that fixes every pool at once.
- **Tradeoff / risk:** `catch_unwind` needs `AssertUnwindSafe`, which is exactly the kind of
  assertion that deserves a comment. The closure owns its payload and touches no shared mutable
  state, so it is genuinely unwind-safe. No compat impact.
- **Effort:** S
- **Regression test to add:** a test-only codec that panics, asserting `get_stored_block` returns
  `Err` and the process survives. Also assert `on_pool` still resolves when the caller's future was
  dropped (no `expect` panic on a detached send).

### `STORE-13` — a caller-supplied S3 `Client` silently discards the stalled-stream opt-out

**P2** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/blob/s3.rs:196-198`
- **What:**
  ```rust
  async fn build_client(&self) -> Client {
      if let Some(client) = &self.options.client {
          return client.clone();
      }
  ```
  This early return precedes `force_path_style` (`:225-227`), `transfer_acceleration` (`:228-230`)
  **and** the stalled-stream-protection block at `:231-239` — whose own comment claims it is
  *"Authoritative regardless of any inherited sdk_config setting"*. It is not authoritative when
  `options.client` is `Some`.
- **Failure scenario:** commit `456274d` just added `--no-stalled-stream-protection` as an escape
  hatch for users whose links trip SSP's 5 s grace on slow-but-alive downloads. The CLI supplies
  `S3Options` with no `client`, so the flag works there. A library embedder — the Tauri app, which
  the module doc's whole credentials-provider design exists to serve — that constructs its own
  `aws_sdk_s3::Client` and passes it in gets the flag silently ignored, along with path-style
  addressing and acceleration. The user sets the flag, the symptom persists, and nothing logs why.
- **Evidence:** `s3.rs:195-241` read in full; the early return is unconditional and none of the
  three settings can be reached after it. `s3_spec.rs` never constructs `S3Options` with a `client`
  (I read all three tests), and `s3.rs` is at 24.49 % region coverage.
- **Recommendation:** either apply the overlay to a supplied client (`Client::from_conf` on a
  builder derived from `client.config()`), or — simpler and more honest — reject the combination:
  if `options.client.is_some()` and any of the three settings differ from their defaults, return
  `StoreError::InvalidUri`/a config error naming the ignored setting. At minimum `tracing::warn!`
  each ignored setting. Fix the `:231-234` comment either way (`STORE-DOC-04`).
- **Tradeoff / risk:** overlaying onto a supplied client can fight the embedder's deliberate
  choices; the loud-rejection option is safer and cheaper.
- **Effort:** S
- **Regression test to add:** construct `S3Options { client: Some(c), stalled_stream_protection:
  false, .. }` and assert the constructor errors (or that the warning fires). This needs no network
  and belongs in the per-PR lane.

### `STORE-14` — cancellation cannot interrupt an await, and two apply loops have no checkpoint at all

**P2** · `hardening` · CONFIRMED

- **Where:** `crates/longtail/src/apply.rs:147-158` and `:267-280` (secondary axis: concurrency
  only — R7 owns the UX and exit codes)
- **What:** every cancellation site in the repo is a polled `cancel.is_cancelled()`; there is not a
  single `tokio::select!` or `.cancelled().await` anywhere, and neither `longtail-store` nor
  `longtail-core` references the token. Within `apply.rs` the four documented checkpoints
  (`:133`, `:189`, `:258`, `:430`) leave two loops uncovered: step 5b, the pre-create-and-truncate
  pass over the entire write set (`:147-158`), and step 7, the permissions pass (`:267-280`).
- **Failure scenario:** a version with 500 k files. Ctrl-C during step 5b does nothing at all until
  all 500 k `create_file_sized` calls finish — the token is set, the first Ctrl-C printed
  "Cancelling…", and the process appears hung. The user presses Ctrl-C again, hits
  `main.rs:538`'s `process::exit(130)`, and now `flush()`/`close()` never run: no cache eviction
  (`STORE-10`), and for an upsync no store-index persist. Separately, on the block path
  the worst-case stop latency after the check at `:189` is the slowest in-flight block including
  `sync.rs:50-57`'s ≈3.85 s of ladder sleeps plus up to 7 attempt durations — so tens of seconds on
  a flaky link, entirely uninterruptible.
- **Evidence:** the full check-site table under Deliverable 3, built by exhaustive grep over
  `crates/` and verified at each `apply.rs` line. `apply.rs:147-158` and `:267-280` read in full —
  no `cancel` reference in either.
- **Recommendation:** add `if cancel.is_cancelled() { return Err(Cancelled) }` to both loops — a
  two-line change, and the pre-create loop is the one that actually matters. For the block path,
  race the token against the fetch in `apply.rs`'s block task
  (`select! { _ = cancel.cancelled() => …, r = store.get_stored_block(h) => … }`) so a cancel does
  not wait out the retry ladder. Do **not** thread the token into `longtail-store`: dropping the
  future is the right cancellation primitive there, and that is what the select does — but see
  `STORE-09` first, because dropping a get is exactly the leak path.
- **Tradeoff / risk:** the `select!` makes cancellation drop an in-flight `get_stored_block`, which
  today leaves a leaked entry and permit (`STORE-09`). Land `STORE-09` first or the two changes
  compose badly. The checkpoint additions alone are risk-free.
- **Effort:** S for the checkpoints, M for the select (gated on `STORE-09`).
- **Regression test to add:** cancel during step 5b on a many-file version and assert the operation
  returns within a bounded time; assert a cancel during a gated block fetch returns without waiting
  out the ladder.

### `STORE-15` — `clone-store` builds a cancellation token nothing cancels, and `put` installs no handler

**P2** · `hardening` · CONFIRMED

- **Where:** `crates/longtail/src/clonestore.rs:107` and `crates/longtail-cli/src/main.rs:627,664,738`
- **What:** `clonestore.rs:107` is `let cancel = CancellationToken::new();`, created locally and
  passed into `write_content` (`:183`) — nothing ever calls `.cancel()` on it, so every
  `is_cancelled()` it reaches is permanently false. Separately, `install_cancel_handler()` is called
  at exactly three sites: `main.rs:627` (downsync), `:664` (get), `:738` (upsync). `put`, `cp`,
  `clone-store`, `validate-version` and every prune command install nothing, so SIGINT keeps the OS
  default and the first Ctrl-C kills the process outright.
- **Failure scenario:** `put` uploads blocks to S3 and then persists the store index. A Ctrl-C
  mid-`put` kills the process immediately: `close()` never runs, so the uploaded `.lsb` blocks are
  orphans not referenced by the store index. That is the same end state as a SIGKILL — recoverable
  by re-running (`put_stored_block`'s skip-if-exists makes it cheap) but the graceful path that
  `downsync`/`upsync` get exists and simply was not wired. `clone-store` is worse in kind: the code
  reads as cancel-aware, so a future maintainer will reasonably assume it is.
- **Evidence:** `clonestore.rs:100-112` and `:178-192` read directly; `main.rs:625-630`, `:662-666`,
  `:736-740` and the exhaustive `rg` for `install_cancel_handler` (three hits). `PutOptions` has no
  `cancel` field.
- **Recommendation:** either plumb a real token into `clone-store` and `put` (`clone-store` is the
  longest-running command in the CLI, so it wants one most), or delete
  `clonestore.rs:107`'s token and make `write_content` take the absence explicitly so the code stops
  claiming a capability it lacks. Install the handler for every long-running command.
- **Tradeoff / risk:** none — additive.
- **Effort:** S
- **Regression test to add:** a CLI-level assertion that every subcommand which can write to a
  remote store sets `opts.cancel` — cheap to write as a match over the subcommand enum, and it
  fails loudly when a new command is added.

### `STORE-16` — the actor's fallback persist discards its error and cannot run from the `*_blocking` wrappers

**P2** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/remote.rs:656-662`
- **What:**
  ```rust
  None => {
      // Dropped without close(): do the final persist as a fallback.
      let _ = persist(&mut index, &mut added, &*client, access_type, true).await;
      break;
  }
  ```
  Two problems. The error is discarded, so a failed final persist is completely silent. And the arm
  only runs if the detached owner task is *polled again* after `index_tx` drops.
- **Failure scenario:** `upsync_blocking` builds its own runtime and does
  `runtime.block_on(upsync(opts))`. On any error path — including `Cancelled` — `upsync` returns
  `Err` without reaching `flush()`/`close()` (`upsync.rs:160-161`), the store `Arc` drops inside the
  async fn, and `index_tx` closes. But `block_on` has already completed its future and returns; the
  local `runtime` then drops, and `Runtime::drop` discards pending tasks without polling them. **The
  fallback persist never runs.** Conversely, on an ambient runtime (the Tauri path) it *does* run —
  which means the store keeps being mutated *after* the operation returned an error, invisibly to
  the caller. An app that responds to the error by deleting the store directory, or by starting a
  retry, races a background writer it does not know exists.
- **Evidence:** `remote.rs:218-220` (the owner is `tokio::spawn`ed, handle discarded), `:655-663`;
  `upsync.rs:160-161`; `downsync.rs:196-197`; `lib.rs:103-109` (`downsync_blocking` builds and drops
  a local runtime). `actor_behavior.rs` and `remotestore_spec.rs` always call `close()` — the
  drop-without-close path is untested in all seven files.
- **Recommendation:** retain the `JoinHandle` from `:218` on `RemoteBlockStore` and await it in
  `close()`, so shutdown is observable; log the fallback persist's error with `tracing::error!`
  instead of `let _`; and have the facade call `close()` on the error path (which also fixes
  `STORE-10`) so the fallback stops being load-bearing. Then either drop the fallback arm or
  document that it is a best-effort safety net that does not fire under `*_blocking`.
- **Tradeoff / risk:** awaiting the owner in `close()` makes `close()` able to hang if the owner is
  stuck in a persist; bound it with a timeout. No byte-compat impact.
- **Effort:** M
- **Regression test to add:** construct a ReadWrite store, `put_stored_block`, drop the store
  *without* `close()` on an ambient runtime, yield, and assert the store index contains the block —
  then assert the same scenario under a `Builder::new_current_thread` runtime that is dropped
  immediately, documenting that it does not.

### `STORE-17` — a worker permit is held across the whole read retry ladder, including its sleeps

**P2** · `perf` · CONFIRMED

- **Where:** `crates/longtail-store/src/remote.rs:339-345` and `crates/longtail-store/src/sync.rs:80-92`
- **What:** `fetch_stored_block` acquires `worker_sem` at `:339` and holds it (a `let _permit`
  binding, dropped at function end) across `read_blob_with_retry`, which sleeps through
  `READ_RETRY_DELAYS = [0, 100 ms, 250 ms, 500 ms, 1 s, 2 s]` between attempts — ≈3.85 s of pure
  sleeping for a fully-retried read, plus up to 7 attempt durations.
- **Failure scenario:** S3 returns transient 503 `SlowDown` on a burst. With the default
  `worker_count` of `num_cpus::get().clamp(1, 8)` = 8 for S3, eight blocks in simultaneous backoff
  hold all eight permits while doing nothing. Every other block fetch — including demand fetches on
  the critical path — is blocked behind sleeping workers, so throughput drops to zero for seconds at
  a time, and the AWS SDK's own retry ladder compounds underneath. golongtail's worker pool has the
  same shape (a busy worker is a busy worker), so this is faithful rather than a regression, but
  the Rust version can do better cheaply.
- **Evidence:** `remote.rs:339-345`; `sync.rs:50-57` and `:80-92`. `actor_behavior.rs:131-155`
  exercises 3 rungs under `start_paused`, so the wall-clock cost is invisible to the suite; no test
  covers ladder exhaustion at all.
- **Recommendation:** release the permit around the sleep — restructure `read_blob_with_retry` so
  the caller re-acquires per attempt, or pass a closure that drops the guard before sleeping. The
  semaphore is meant to bound *concurrent I/O*, not concurrent waiting.
- **Tradeoff / risk:** releasing and re-acquiring lets a retrying block lose its place to fresh
  work, which is usually what you want under load-shedding but changes fairness. It also increases
  peak concurrency slightly during a storm (a retrying block re-enters the queue), so measure
  against the S3 lane rather than assuming.
- **Effort:** M
- **Regression test to add:** under `start_paused`, with `worker_count = 1`, assert that a block in
  ladder backoff does not prevent a *different* block from being fetched.

### `STORE-18` — three `index.as_ref().unwrap()` encode a sound invariant the compiler could enforce

**P3** · `idiom` · CONFIRMED (the invariant holds — proof below)

- **Where:** `crates/longtail-store/src/remote.rs:755`, `:780`, `:816`
- **What:** the mission asked me to prove or break this. **It holds.** Each unwrap is immediately
  preceded by the identical three lines:
  ```rust
  if index.is_none() {
      let loaded = sync::read_remote_store_index(&**blob_store, client, access_type).await?;
      *index = Some(loaded);
  }
  let base = index.as_ref().unwrap();
  ```
  `index: &mut Option<StoreIndex>` is a local of `index_owner`'s single-task loop — there is no
  sharing, no re-entrancy, and no `.await` between the assignment and the unwrap. If it was `None`
  it is now `Some`; if the load failed, `?` returned. The only place `index` is reset to `None`
  (`:707-709`, after `GetExistingContent` on a non-ReadOnly store) is a different match arm, after
  the reply is sent. So all three unwraps are unreachable-panic.
- **Failure scenario:** none today. The cost is that the invariant is enforced by three copies of a
  hand-written prelude rather than by types: a future edit that adds an `.await`, an early
  `continue`, or a second mutation point between the assignment and the unwrap turns a silent
  refactor into a panic in the actor task — which then dies detached (`:218`, handle discarded) and
  every subsequent command returns the opaque `WorkerGone`.
- **Evidence:** `remote.rs:744-825` read in full; `:652`, `:665`, `:707-709` are the only writes to
  `index`. `actor_behavior.rs` confirms no test targets this (nor can any, through the public API).
  `remotestore_spec.rs:176-188` does incidentally exercise the drop-then-reload transition.
- **Recommendation:** collapse each to a single expression that cannot be wrong —
  ```rust
  let base = match index {
      Some(i) => i,
      None => index.insert(sync::read_remote_store_index(...).await?),
  };
  ```
  and factor the three copies into one helper. That removes the unwrap, the duplication, and the
  invariant comment all at once.
- **Tradeoff / risk:** none. Pure refactor.
- **Effort:** S
- **Regression test to add:** none needed — the point is that the type system replaces the test.
  Do add a `#[should_panic`-free] assertion that the actor surfaces a *distinguishable* error if it
  dies, i.e. retain the `JoinHandle` per `STORE-16`.

### `STORE-19` — 14 casts, zero `checked_*`/`try_from`; two have reachable truncation

**P3** · `hardening` · CONFIRMED · coordinate with R1 and R6 — I own the store half only

- **Where:** `crates/longtail-store/src/remote.rs:519` and `crates/longtail-store/src/blob/s3.rs:299`
- **What:** the mission's count (9 `as u64`, 4 `as usize`, **zero** `checked_*`/`try_from`, against
  56 guards in `longtail-core`) matches what I enumerated: 14 casts total across
  `sync.rs:85,88` · `remote.rs:226,352,368,371,431,433,519,520,787` · `blob/s3.rs:299` ·
  `blob/mem.rs:83` · `blob/fs.rs:256`. Eleven are provably safe widenings or bounded values. Two
  are worth naming:
  - `remote.rs:519` — `size_by_hash.get(&hash).copied().unwrap_or(1).max(1) as usize`: `u64 → usize`.
    On a 32-bit target a block whose `Σ chunk_sizes` exceeds `u32::MAX` truncates, e.g.
    `0x1_0000_0000 → 0`, and the following `.max(1)` then yields 1 permit for a 4 GiB block —
    a silent budget under-count, **not** a hang (liveness holds because the clamp at `:520` is
    against the budget, not the value).
  - `blob/s3.rs:299` — `object.size().unwrap_or_default() as u64`: the SDK gives `Option<i64>` from
    the remote's `ListObjectsV2` response, and a negative value sign-flips (`-1 →
    18446744073709551615`). I traced every consumer of `BlobProperties::size`: `sync.rs:130` and
    `:434` use it only as `> 0`, and `cache.rs:228/:240/:249` operate on locally-`stat`ed files, not
    S3 responses. **So it is currently harmless** — which is exactly the kind of thing that stops
    being true when someone adds size-based logic.
- **Failure scenario:** neither is a live bug. The finding is the *absence of the guard habit*: the
  store crate has none of the `try_from`/`checked_*` discipline `longtail-core` has, on a crate
  whose entire input surface is remote-supplied bytes.
- **Evidence:** my own enumeration (14 sites, listed above) plus the mission-supplied counts;
  `remote.rs:224-226` proves `:520`'s `as u32` safe (budget is pre-clamped to
  `min(Semaphore::MAX_PERMITS).min(u32::MAX as usize)`).
- **Recommendation:** `try_from` at both named sites, with the S3 one rejecting a negative size as
  `StoreError::Backend` rather than sign-flipping. Then add a crate-level
  `#![warn(clippy::cast_possible_truncation, clippy::cast_sign_loss)]` so the next cast has to
  justify itself — that is the durable fix, and it is what makes this worth filing at all.
- **Tradeoff / risk:** the lint will fire on the eleven benign widenings; each needs a one-line
  `#[allow]` with a reason, which is the point.
- **Effort:** S
- **Regression test to add:** a `StaticReplayClient` list response with `Size: -1`, asserting
  `get_objects` errors. No network needed, so it fits the per-PR lane and partly offsets `s3.rs`'s
  24.49 % coverage.

### `STORE-20` — four unreachable error arms, because no `Semaphore` in the crate is ever closed

**P3** · `complexity` · CONFIRMED

- **Where:** `crates/longtail-store/src/remote.rs:286-289`, `:339-342`, `:386-390`, and
  `crates/longtail-store/src/sync.rs:196-198`
- **What:** `Semaphore::close()` is never called anywhere in the crate (verified by grep), so
  `acquire`/`acquire_many_owned` can never return `Err(AcquireError)`. That makes
  `remote.rs:288`'s `Err(_) => return, // semaphore closed — store torn down` and the two
  `map_err(|_| StoreError::WorkerGone)` arms at `:342` and `:390` dead code — including the comment
  claiming a teardown path that does not exist (`close()` at `:591-606` does not close either
  semaphore). Separately `sync.rs:196-198`'s `if used.is_empty() { continue; }` is provably
  unreachable: `acc` is `Some` only after at least one `used.push` (`:170-178`).
- **Failure scenario:** no runtime failure. The cost is that a reader trying to reason about
  teardown is told a mechanism exists when it does not, and the `WorkerGone` mapping suggests the
  worker semaphore participates in shutdown. On `remote.rs:288` specifically, the dead arm hides a
  real question: **what does tear the store down?** The answer is `Drop` of `PrefetchState`, and
  nothing says so.
- **Evidence:** `remote.rs:286-289`, `:339-342`, `:386-390`, `:591-606`; `sync.rs:189-203`;
  repo-wide grep for `Semaphore` `.close()` returns nothing. `prefetch_budget.rs`'s worker
  independently flagged `:288` as unreachable.
- **Recommendation:** either close `prefetch_sem` and `worker_sem` in `close()` — which would make
  the arms live, give the store a real teardown signal, and let a parked dispatch task abandon
  promptly instead of waiting for a permit — or delete the arms and replace the comments with
  "these semaphores are never closed; teardown is `Drop`". The first is better: it is a genuine
  improvement to shutdown latency and it makes `STORE-09`'s permit accounting self-healing.
  Keep `sync.rs:196-198` but mark it `debug_assert!`-style defensive rather than a silent
  `continue`.
- **Tradeoff / risk:** closing the semaphores makes in-flight fetches fail with `WorkerGone` during
  a concurrent `close()` — which is arguably correct, but it is a behaviour change on the
  close-races-a-get path, and `close()` is `&self`. Needs the `prefetch_budget.rs` suite green.
- **Effort:** S to document, M to actually close them.
- **Regression test to add:** if the semaphores are closed, assert a `get_stored_block` racing
  `close()` returns `WorkerGone` rather than hanging.

---

## Lower-priority observations

1. **`try_write_shard`'s ignored deletes are benign — say so.** `sync.rs:273` discards delete errors
   too, but unlike `try_overwrite` (`STORE-01`) it is safe by construction: a writer deletes only
   the items it merged into its own surviving superset shard, so a surviving leftover is re-merged
   by the next reader. That asymmetry between the two functions is load-bearing and undocumented.
2. **Unbounded, backoff-free retry loops.** `add_to_remote_store_index` (`sync.rs:310-323`) and
   `overwrite_remote_store_index` (`:376-388`) retry a lost CAS forever with no sleep and no
   iteration cap, and `read_store_store_index_with_items` (`:189-203`) rescans forever. Global
   progress holds (each round someone commits); per-caller starvation under sustained churn does
   not. Every affected test (`remotestore_spec.rs:382-406`, `blobstore_spec.rs:209`,
   `s3_spec.rs:183`) has **no timeout**, so a livelock regression hangs CI rather than failing it.
3. **Repeated `preflight_get` for an already-consumed hash causes a redundant refetch.** Traced:
   preflight enqueues H → demand get consumes it → entry removed → a *second* preflight re-enqueues
   H → the still-parked first dispatch task wins the `queued.remove` and refetches a block already
   delivered. Only one wasted fetch, and `apply.rs:84` preflights once, so the cost is
   theoretical — but it is untested (`prefetch_budget.rs` never double-preflights).
4. **Prefetched-but-never-demanded blocks are paid for.** `apply.rs:92` maps each chunk to exactly
   one block, so a store with duplicate chunks across blocks can leave a retargetted block in
   `store_index.block_hashes` (hence preflighted and fetched) that `block_writes` never keys on.
   Real S3 egress for bytes never used, released only at `flush()`.
5. **`flush()` throws away unconsumed prefetches** (`remote.rs:582` → `drain_prefetch`), so a
   mid-operation flush costs a refetch of everything in flight. Faithful to Go's `flushPrefetch`;
   noted so nobody adds a periodic flush without knowing.
6. **Cache layer has no request coalescing.** Two tasks missing the same block both fetch from the
   remote and both write the entry (`cache.rs:117-121`). Coalescing exists one layer down in
   `RemoteBlockStore`, so the duplicate *fetch* is avoided — but the duplicate `to_bytes()`
   serialization and disk write are not.
7. **`?` polarity on three cache-infrastructure calls.** `cache.rs:81`, `:82` and `:118` use `?`
   where the surrounding code is deliberately best-effort; `:118` in particular would fail a get
   *after* the remote fetch already succeeded. Latent only because `FsBlobClient::new_object`
   (`fs.rs:74-82`) is infallible.
8. **A blocking flock cannot be cancelled and occupies a blocking-pool thread.** `fs.rs:201` uses
   `lock_exclusive`, not `try_lock`, inside `spawn_blocking`; a contended store parks a pool thread
   indefinitely.
9. **`spawn_blocking` closures survive cancellation.** Dropping any fs blob future leaves the
   closure running: a cancelled `write()` can still land the object and bump `.gen` after the caller
   moved on. Benign here (the pair stays atomic), but it means "cancelled" never implies "did not
   happen" for the fs backend.
10. **The rebuild path skips the list retry ladder.** `build_store_index_from_store_blocks`
    (`sync.rs:431`) calls `get_objects("")` bare, while `get_store_store_indexes` (`:116`) wraps the
    identical call in the 6-rung ladder. A transient list failure fails `--access-type init`
    outright.
11. **`sync.rs` emits no `tracing` at all**, so every swallowed error in the module
    (`:273`, `:362`, `:457`) is invisible even at `RUST_LOG=trace`.
12. **`blob/mem.rs`'s 8 `.lock().unwrap()` do not matter.** Asked directly: every critical section
    (`mem.rs:77,110,115,129,137,168`) is await-free and contains only allocations, which abort
    rather than panic, so poisoning is unreachable; the two map `unwrap`s at `:142`/`:160` are
    guarded by an `exists` check inside the same critical section. It is a sound test double.
    `sync_fixtures.rs:69` using it for shard merge-on-read is fine — the *limitation* that matters
    is different: mem has no await points, so "concurrent" writers interleave far less than on a
    real backend, which is why the fs variants in `remotestore_spec.rs:396-406` carry the real value.
13. **`resolved_worker_count` classifies only literal `"s3"` as networked** (`uri.rs:92-103`), so a
    URI that `resolve_backend` would reject still gets a local worker count. Benign, documented.
14. **`split_scheme` silently falls back to a filesystem path** for any scheme containing a
    non-alphanumeric byte or a single-slash typo (`blob/mod.rs:191`): `s3:/bucket` becomes a local
    directory named `s3:/bucket`. No percent-decoding anywhere in `mod.rs` or `uri.rs`.
15. **Cross-reference, R4:** `version.rs:53`'s `pool.install(par_iter)` is called synchronously from
    async `downsync` (`downsync.rs:117-127`), so the calling tokio worker thread joins the rayon
    pool and does hashing for the whole target scan — per `rust-port.md`'s own roadmap, ~97 % of an
    incremental download's wall time. On the Tauri ambient runtime that blocks a runtime worker for
    minutes. Not my file; flagged for R4.
16. **Cross-reference, R4/R7:** `prune.rs:428-430` repeats `STORE-02`'s silent-delete-swallow, and
    `prune_store_blocks` derives its keep-set from a **single** `--store-index-path` file
    (`prune.rs:402-404`) rather than the merged shard union — point it at one shard of a sharded
    store and it deletes blocks the other shards reference. golongtail has the same contract, so
    this is parity, but it is a destructive command with a sharp edge.
17. **Stats are decorator-blind.** `CacheBlockStore` and `CompressBlockStore` forward `stats()`
    verbatim (`cache.rs:185-187`, `compress.rs:107-109`), so cache hit/miss rate — the single most
    useful number for tuning `--cache-size-limit` — is unobservable.
18. **The rustls pinning comment is still accurate.** Checked against `05-tree.txt` per the mission:
    `longtail-store/Cargo.toml:45-49` justifies `default-features = false` on `aws-sdk-s3` as
    dodging the legacy hyper-0.14 + rustls-0.21 connector and its unmaintained
    `rustls-webpki 0.101.7`. The tree shows only the modern `default-https-client` path
    (rustls 0.23 + aws-lc), and `06-audit.json`/`16-deny.txt` report `advisories ok`. No action.

## Comments & documentation issues

### `STORE-DOC-01` — the Windows mixed-writer divergence is missing from `rust-port.md`

**P1** · `hardening` · CONFIRMED

- **Where:** `docs/rust-port.md:111-155` (§Deliberate divergences) vs.
  `crates/longtail-store/src/blob/fs.rs:17-21`
- **What:** `fs.rs:17-21` calls itself a "documented divergence" and describes the highest-
  consequence one in my slice — Rust `LockFileEx` vs golongtail `syscall.CreateFile`, meaning mixed
  Windows fs writers do not mutually exclude. `rust-port.md`'s divergence list contains
  "Lock guards never unlink" and "Stale-generation clearing on `init-remote-store`" but **nothing
  about the lock mechanism itself**. The only keeper-doc mentions of Windows locking
  (`rust-port.md:119-121`, `:185`) are about the unlink invariant and `fs4`'s adoption.
- **Failure scenario:** the contract asks for exactly this class. An operator reads
  §Deliberate divergences to decide whether a Windows CI runner may share a filesystem store with a
  legacy golongtail job, sees nothing prohibiting it, and gets `STORE-07`'s ACKed lost update. The
  source comment is not where that decision gets made.
- **Evidence:** grep for `LockFileEx|CreateFile` across the four keeper docs returns nothing;
  `fs.rs:17-21` read in full; the Go mechanism verified at `fsstore_windows_amd64.go:39,55`.
- **Recommendation:** add a §Deliberate divergences bullet naming the two mechanisms, the
  consequence (ACKed index lost update), the blast radius (fs stores only — S3 is lockless on both
  sides so it is unaffected), the recovery (`--access-type init` rebuilds from `.lsb` ground truth),
  and the gate gap (the mixed-writer chaos test is minio-only, so nothing covers it).
- **Effort:** S
- **Regression test to add:** none — this is the documentation half of `STORE-07`.

### `STORE-DOC-02` — `remote.rs:560-562` claims prune can never leave dangling index entries

**P1** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/remote.rs:560-562`
- **What:** *"Overwrite the store index FIRST, then delete orphan blocks (remotestore.go:655-684
  ordering — a crash leaves harmless orphans, never dangling index entries)."* The claim is true for
  a **crash** and false for a **delete failure**: `STORE-01` shows the delete-error path produces
  exactly the dangling entries the comment promises are impossible.
- **Failure scenario:** the comment is the reason a reader stops looking. It is load-bearing
  documentation of a safety property on a destructive operation, and it overstates.
- **Evidence:** `remote.rs:560-577`; `sync.rs:357-365`.
- **Recommendation:** narrow it to what is true — *"a crash between the two leaves harmless orphans.
  A **failed** shard delete does not: see `try_overwrite`, whose deletes are best-effort."* — and
  fix the code per `STORE-01`.
- **Effort:** S
- **Regression test to add:** covered by `STORE-01`.

### `STORE-DOC-03` — `longtail/src/lib.rs:82-85` promises error classes the block path destroys

**P1** · `hardening` · CONFIRMED

- **Where:** `crates/longtail/src/lib.rs:82-85` and `crates/longtail-store/src/error.rs:70-82`
- **What:** both places document a public guarantee — match on `NotAuthorized` / `Network` /
  `NotFound` through `LongtailError::Store(_)`, explicitly *"without string-matching"*. `STORE-04`
  shows `get_stored_block` flattens the first two into `Backend` on every error.
- **Failure scenario:** an integrator builds credential-refresh UX against a documented contract the
  primary download path does not honour, and discovers it only in production.
- **Evidence:** `lib.rs:82-85`; `error.rs:70-86`; `remote.rs:490` + `:626-632`.
- **Recommendation:** fix the code (`STORE-04`). If that is deferred, the doc must say the guarantee
  holds for store-index errors but not per-block ones — a caveat nobody will want to write, which is
  the argument for just fixing it.
- **Effort:** S
- **Regression test to add:** covered by `STORE-04`.

### `STORE-DOC-04` — `s3.rs:231-234` says "Authoritative regardless of any inherited sdk_config setting"

**P2** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/blob/s3.rs:231-234`
- **What:** the comment asserts the stalled-stream-protection setting always wins. `STORE-13` shows
  the `options.client` early return at `:196-198` skips it entirely.
- **Failure scenario:** the comment is the reason someone debugging a non-functioning
  `--no-stalled-stream-protection` will look elsewhere.
- **Evidence:** `s3.rs:195-241` read in full.
- **Recommendation:** *"Authoritative for every client this function builds. A caller-supplied
  `options.client` bypasses this (and `force_path_style`/`accelerate`) — see the early return
  above."* Then fix per `STORE-13`.
- **Effort:** S

### `STORE-DOC-05` — `evict_cache_dir`'s doc claims a `.lrb` filter the code does not have

**P2** · `idiom` · CONFIRMED

- **Where:** `crates/longtail-store/src/cache.rs:213` and `:260`
- **What:** `:213` says *"Sums the size of every `.lrb` block file"* and `:260` says
  *"Recursively collect `.lrb`-scheme block files"*. `collect_cache_files` (`:262-283`) has **no
  extension filter** — every regular file under `chunks/` is collected, counted, and deletable.
- **Failure scenario:** the divergence is currently *beneficial* — it is the only thing that ever
  reclaims the `.tmp` orphans `STORE-03`/ENOSPC leaves behind. But someone reading the doc will
  "fix" the code to match it and silently create an unbounded leak.
- **Evidence:** `cache.rs:205-235` and `:258-283` read in full; the only filters are `is_dir` /
  `is_file`.
- **Recommendation:** keep the behaviour, fix the words: *"Collects **every** file under `chunks/`,
  not just `.lrb` — deliberately, so leaked `*.tmp.*` files from interrupted writes are reclaimed
  against the same budget. The cache-dir `store.lsi` sits outside `chunks/` and is never touched."*
- **Effort:** S

### `STORE-DOC-06` — the two public URI entry points disagree about fs locking, undocumented

**P2** · `complexity` · CONFIRMED

- **Where:** `crates/longtail-store/src/blob/mod.rs:122,126,151,171,182` vs
  `crates/longtail-store/src/uri.rs:166-168`
- **What:** `create_blob_store_for_uri` hardcodes `FsBlobStore::new(..., false)` at all five fs
  construction sites; `resolve_backend` derives `fs_locking = (access_type != ReadOnly)`. So the two
  public paths to "open a store from a URI" produce different locking flavours — and therefore
  different on-disk store-index shapes (`store.lsi` + `.gen` vs `store_<sha>.lsi` shards).
  `uri.rs:166-168` explains its own choice; nothing explains the split.
- **Failure scenario:** today only `prune.rs:406` uses the `mod.rs` entry point and it only
  lists/deletes `.lsb` blocks, so nothing writes an index through it. The next caller that does
  will write shards into a store an operator expects to hold `store.lsi`. Both remain readable by
  both implementations (merge-on-read unions all `store*.lsi`), so this is a shape surprise, not a
  correctness bug — which is precisely why it needs a comment rather than a fix.
- **Evidence:** the five `mod.rs` sites and the five `uri.rs` sites enumerated by grep;
  `prune.rs:396-440` read in full.
- **Recommendation:** document on `create_blob_store_for_uri` that it is the *blob-level* helper and
  always disables locking, so it must not be used to construct a store-index writer; point at
  `create_block_store_for_uri` for that. Consider `#[doc(hidden)]` or a rename if it is really only
  a test/blob-inspection helper.
- **Effort:** S

### `STORE-DOC-07` — `sync.rs:457`'s comment claims a log the module cannot emit

**P3** · `idiom` · CONFIRMED

- **Where:** `crates/longtail-store/src/sync.rs:457`
- **What:** `Err(_) => return Ok(rebuilt), // persist failure is non-fatal (Go logs)`. The Rust code
  logs nothing — `sync.rs` has no `tracing` import and no logging call anywhere. So an
  `--access-type init` whose rebuilt index failed to persist looks like a clean success.
- **Failure scenario:** `init-remote-store` reports success; the store index was never written; the
  next `downsync` sees an empty index and reports every chunk missing. Nothing in the logs connects
  the two.
- **Evidence:** `sync.rs:452-458`; grep for `tracing` in `sync.rs` returns nothing.
- **Recommendation:** actually log it (`tracing::warn!` with the error), and while there, the two
  best-effort delete swallows at `:273` and `:362`. The comment then becomes true.
- **Effort:** S

### `STORE-DOC-08` — `rust-port.md`'s cache-LRU bullet omits that the budget is post-hoc and skipped on failure

**P2** · `memory` · CONFIRMED

- **Where:** `docs/rust-port.md:146-155`
- **What:** the bullet describes *"a post-run sweep (`evict_cache_dir`, on `close`) deletes
  least-recently-used blocks down to the budget"* — accurate as far as it goes, and the
  mtime-on-every-access claim is verified true (`cache.rs:99-113`). What it omits is the part that
  changes user-visible behaviour: the budget is **not a ceiling** (peak on-disk use during a run is
  the download size regardless of the flag) and it is **not applied at all** on any error or
  cancellation path, because `downsync.rs:196-197`'s `close()` sits after the `?` — see `STORE-10`.
- **Failure scenario:** a user sets `--cache-size-limit 10G` on a 40 GB download and runs out of
  disk, or cancels repeatedly and finds the cache far past the limit. The doc is what they will have
  read.
- **Evidence:** `rust-port.md:146-155`; `cache.rs:159-183`; `downsync.rs:191,196-197`.
- **Recommendation:** add one sentence: *"the limit is a post-run trim, not a live ceiling — peak
  usage during a run is unbounded, and a run that errors or is cancelled skips the sweep entirely."*
  Then fix per `STORE-10` and shorten the doc back.
- **Effort:** S

### `STORE-DOC-09` — `blob/mod.rs:186-187` says the scheme is lowercased; it is not

**P3** · `idiom` · CONFIRMED

- **Where:** `crates/longtail-store/src/blob/mod.rs:186-187`
- **What:** *"The scheme is lowercased for matching."* `split_scheme` (`:188-195`) does
  `let scheme = &uri[..idx];` with no case folding, so `S3://bucket` matches no arm, and — because
  its length is 2, not 1 — falls to `:173-176` and errors as `unknown scheme`. `FILE://x` likewise.
- **Failure scenario:** a user pastes `S3://bucket/path` from a document and gets
  "unknown scheme `S3`" while the doc says case does not matter.
- **Evidence:** `mod.rs:184-195` quoted verbatim above.
- **Recommendation:** either lowercase it (matching the doc, and arguably matching URI RFCs) or
  delete the sentence. Lowercasing is the friendlier fix and cannot break a store — the scheme is
  not part of any on-disk format.
- **Effort:** S

### `STORE-DOC-10` — `remote.rs`'s liveness invariant is restated in five places

**P3** · `complexity` · CONFIRMED

- **Where:** `crates/longtail-store/src/remote.rs:17-44`, `:93-105`, `:265-272`, `:444-451`, `:505-518`
- **What:** the same argument — budget gates background prefetch only, demand always proceeds,
  acquire-at-dispatch, oversize clamps to the whole budget — is written out five times, with
  overlapping `remotestore.go` line citations each time. The 52-line module header is longer than
  several functions in the file.
- **Failure scenario:** not a bug; a maintenance hazard. Five copies of a subtle invariant will
  drift, and the next person to change the prefetch path has to diff five prose passages to know
  which is current. The content itself is *good* — commit-pinned at `:2` (`@49a20e1`), which makes
  every line citation checkable in a year, and that practice should be kept.
- **Evidence:** the five ranges read in full; the overlap is near-verbatim between `:17-44` and
  `:505-518`.
- **Recommendation:** state the invariant **once** — the "Deliverable 1" contract block in this
  document is roughly the right length — in the module header, and reduce the four call-site
  comments to a pointer plus the one fact local to that site (e.g. at `:520`, just *"clamped so a
  single oversize block is always acquirable — see the module header's liveness invariant"*). Keep
  every `remotestore.go` citation; drop the duplicated prose around them.
- **Effort:** S

## Hardening backlog

Ranked by value per unit of effort. The theme: this crate's tests prove **liveness** thoroughly and
**failure handling** almost not at all.

1. **I/O fault injection at the blob layer.** A `FailingBlobStore` decorator parameterised by
   (operation, key pattern, error) is the single highest-value addition — it unlocks regression tests
   for `STORE-01`, `STORE-02`, `STORE-04`, `STORE-05` and `STORE-11` at once. Nothing in the crate
   currently injects an ENOSPC, EACCES, or non-`NotFound` error anywhere; `actor_behavior.rs`'s
   `FlakyStore` fails only `read()`, and only transiently.
2. **The success-path prefetch permit release.** `remote.rs:483-486` is untested and every current
   test would still pass if it regressed, because the demand path bypasses the budget entirely.
   Assert `prefetch_sem.available_permits()` returns to the budget after a dispatched prefetch is
   consumed by a get.
3. **Cancellation tests.** Drop an in-flight `get_stored_block` (`STORE-09`), drop a store without
   `close()` (`STORE-16`), cancel during `apply.rs`'s pre-create loop (`STORE-14`). The
   `GatedStore` harness at `prefetch_budget.rs:307-383` already does the hard part.
4. **Retry-ladder exhaustion**, read and put. `FlakyStore` uses 3 failures against a 6-rung ladder,
   so nothing proves a permanently failing backend eventually returns `Err` instead of retrying
   forever; `PUT_RETRY_DELAYS` (`remote.rs:76-80`) and every `put_*` stat counter are exercised by
   no test in the workspace.
5. **Corrupt-bytes coverage.** `remote.rs:353-360` (unparseable `.lsb` → `BadFormat`) and the
   truncated-cache-entry path (`STORE-11`) are untested; `block_scanning`'s "bad" blocks are *valid*
   blocks at wrong paths, which is a different branch.
6. **Timeouts on the chaos tests.** `remotestore_spec.rs:382-406`, `blobstore_spec.rs:209` and
   `s3_spec.rs:183` drive unbounded retry loops with no `tokio::time::timeout`. A livelock
   regression hangs CI instead of failing it — copy the `GUARD` pattern from
   `deadlock_regression.rs:32`, which gets this exactly right.
7. **Make `s3_spec.rs`'s skip visible.** `minio_options()` returning `None` produces a `return`, so
   nextest records **PASS** — `03-test.txt:280-281` shows both behavioural S3 tests "passing" in
   0.004 s with no endpoint configured. A typo'd env var in `s3-minio.yaml` greens the entire S3
   behavioural suite silently. Either `#[ignore]` them by default, or have the scheduled workflow
   assert a positive marker (e.g. require the tests to write a sentinel file the job then checks).
8. **De-flake `concurrent_gets_coalesce_with_dispatched_prefetch`** (`prefetch_budget.rs:388-443`).
   Its `tokio::time::sleep(Duration::from_millis(50))` at `:426` is load-bearing on a
   `multi_thread` runtime: it must let all 8 spawned gets reach the coalescing lock before the gate
   opens. A starved CI worker makes a late get fetch inline and `get_count == 1` fails at `:437`.
   Replace with a barrier or a second watch channel. The other six tests in the file are
   `start_paused` and genuinely deterministic — this is the only flake risk in the store suite.
9. **`loom` over `PrefetchState`.** The `queued`/`entries` mutual exclusion (`remote.rs:93-105`) is
   the crate's one genuinely subtle invariant. I hand-traced all four interleavings of
   `preflight_get` / `dispatch_prefetch` / `get_stored_block` and they hold, but that proof lives
   only in this document. `loom` would make it a gate. Note the model needs `PrefetchState` behind a
   `std::sync::Mutex`, which `STORE-09`'s fix would do anyway.
10. **A cast lint.** `#![warn(clippy::cast_possible_truncation, clippy::cast_sign_loss)]` on
    `longtail-store`, with a reasoned `#[allow]` on each of the eleven benign widenings
    (`STORE-19`). Coordinate with R1/R6 so the crate-level policy is uniform.
11. **`min_block_usage_percent` is `0` in every single call** across all seven test files
    (`remotestore_spec.rs:106,153,177,184,196,257,294,298`). The filter is completely untested
    behaviourally. Owned by R1 for the algebra, but the store is the only caller.
12. **`with_store_index_override`** (`remote.rs:181-195`) is never constructed with an override in
    any store test — the ReadOnly seed path at `:213-217` is exercised only via the facade, if at
    all.

## Verified good

Do not redo these.

- **The prefetch clamp and the liveness invariant are correct and provably so.**
  `permits = estimate.min(self.max_prefetch_bytes).max(1) as u32` (`remote.rs:520`) can never exceed
  the semaphore's total, because the budget is pre-clamped to
  `min(Semaphore::MAX_PERMITS).min(u32::MAX as usize)` at `:224-226`. The `as u32` is therefore safe
  on both 32- and 64-bit. `acquire_many` can never be unsatisfiable.
- **It is the only `acquire_many` in the repo.** Every other permit acquisition takes exactly 1
  against a bound of `≥ 1` (`remote.rs:339`, `:386`; `apply.rs:201`). The clamp bug class is closed —
  full table under Deliverable 1.
- **`PrefetchState`'s "never in both sets" invariant holds.** All transitions happen under the one
  mutex: `preflight_get` inserts into `queued` only if `entries` lacks the hash (`:513`), and both
  `dispatch_prefetch` (`:294`→`:315`) and `get_stored_block` (`:463`→`:473`) remove from `queued`
  before inserting into `entries`. I traced all four orderings of the dispatch/demand race; every
  one is correct. The `debug_assert!` at `:299` cannot fire.
- **The `index.as_ref().unwrap()` invariant holds** at all three sites — see `STORE-18` for the
  proof. The finding there is idiom, not a bug.
- **The `Arc::try_unwrap` handle accounting at `remote.rs:489` is correct.** `fut.await` at `:485`
  consumes the local `Shared` clone by value; `:486` then drops the map's clone, which for the
  single-consumer case is the last handle, so `try_unwrap` succeeds and the block moves out
  copy-free. The comment at `:483-484` describes this accurately.
- **The store index / block ordering in `prune_blocks` is right** (`remote.rs:560-563` before
  `:567`): index first, blocks second, so a *crash* leaves orphans not dangling entries. Only the
  delete-*failure* path breaks it (`STORE-01`).
- **`.gen` bump ordering is right and matches Go exactly**: check generation → rename data → bump
  generation, all inside one flock critical section (`fs.rs:347-356` vs `fsstore.go:254-276`). The
  inverse window — generation advanced before the data landed — does not exist.
- **The cache eviction / concurrent read TOCTOU recovers cleanly.** Reader `exists` at
  `cache.rs:94` → evictor `remove_file` at `:247` → reader `File::open` at `:95` → `fs.rs:318-320`
  maps to `NotFound` → the let-chain fails → falls through to the remote at `:117`. No error
  escapes. `decorators_integration.rs:57-91` regression-tests the evicted-underneath case, which is
  also the guard against the C library's authoritative-cache-index bug.
- **The `.lrb` cache layout is C-compatible**: `chunks/<4 lowercase hex>/0x<16 lowercase
  zero-padded hex>.lrb` (`cache.rs:70-74`), so an existing warm cache written by the C library is
  read directly.
- **All four on-store layout artefacts are byte-identical to golongtail `49a20e1`** — shard name,
  shard discovery filter, `.gen` encoding, `._lck` naming. Diffed against source; table under
  Deliverable 4.
- **The lockless flavour is safe without any conditional write.** There is no S3 conditional PUT, no
  ETag, no `If-None-Match` anywhere — and it does not need one: shard keys are the SHA-256 of their
  own content, so two writers colliding on a key necessarily carry identical bytes, and a writer
  deletes only the items it merged into its own surviving superset shard (`sync.rs:287`→`:294`→
  `:268-275`). That last clause is the load-bearing invariant and it holds.
- **The deadlock regression tests are well built and would catch a revert.**
  `deadlock_regression.rs:32`'s `GUARD = 120 s` with an explicit panic message means a regression
  **fails cleanly, never hangs**; the second variant's budget is computed as exactly the largest
  block's `Σ chunk_sizes` from the committed `.lsi` (`:67-82`), so it keeps guarding even if the
  fixture shrinks. All three plausible revert shapes trip an assertion. Six of the seven
  `prefetch_budget.rs` tests use `start_paused`, so a parked future auto-advances virtual time to
  the deadline and fails fast instead of hanging.
- **`blob/mem.rs` is a sound test double** — no reachable lock poisoning, no reachable map unwrap.
  Details in Lower-priority #12.
- **`compress.rs`'s deliberate avoidance of `spawn_blocking`** is correctly implemented: the rayon
  job never blocks on a tokio future, and the progress forwarder in `version.rs:141-150` uses
  `try_lock` specifically so a rayon worker never blocks. The gap is panic containment
  (`STORE-12`), not the bridge design.
- **The `aws-sdk-s3` `default-features = false` pinning comment is still accurate** per
  `05-tree.txt` — see Lower-priority #18. No action.
- **`03-test.txt` context:** 217 tests run, 217 passed, 2 skipped; no FAIL, LEAK, or SLOW lines. The
  2 skips are the `#[ignore]`d archive tests in `longtail-cli/tests/commands_spec.rs:1339,1346`, not
  mine. **But** `s3_blob_round_trip` and `s3_store_index_sync` are recorded PASS at 0.004 s each
  (`03-test.txt:280-281`) — that is the internal env-gated skip, indistinguishable from a real pass.
  Read together with `s3.rs`'s 24.49 % coverage, this is why my S3 confidence is "thin".

## Experiments requested

| # | Hypothesis | Exact command | What would change the finding |
|---|---|---|---|
| 1 | `STORE-03`: a lost rename is observable, i.e. the missing parent-dir fsync is not merely theoretical on the filesystems we ship on. | On a scratch ext4 volume mounted `data=writeback`: run `cargo run -p longtail-cli -- upsync --storage-uri <fs path> …`, then force a crash before the writeback window closes (`echo b > /proc/sysrq-trigger` in a VM), remount, and check whether `store.lsi` exists and parses. Repeat on XFS and on the Windows CI runner's NTFS. | If the rename is durably journaled on every filesystem we support, `STORE-03` reduces to the discarded-`sync_all`-error half and drops to P2. |
| 2 | `STORE-12`: rayon's default for a panic escaping `ThreadPool::spawn` with no `panic_handler` is process abort, not a swallowed panic. | A scratch bin: build a `rayon::ThreadPool` with `ThreadPoolBuilder::new().num_threads(1).build()`, `pool.spawn(\|\| panic!("x"))`, sleep, print "survived"; run it and record the exit status. | If rayon swallows the panic and keeps the pool alive, the outcome is the caller-side `expect` panic only (contained at the tokio task boundary), and `STORE-12` drops to P3. |
| 3 | `STORE-16`: `Runtime::drop` discards the detached `index_owner` task without polling it, so the fallback persist never runs under `*_blocking`. | A scratch test: build a `new_multi_thread` runtime, `block_on` a future that spawns a task which sets an `AtomicBool` after a `yield_now`, return from `block_on`, drop the runtime, then assert the flag. | If the flag is set, the fallback persist *does* run under `*_blocking` and `STORE-16` reduces to the discarded-error half plus the post-return-mutation concern. |
| 4 | `STORE-08`: the enqueue-under-lock latency is measurable at production working-set sizes. | `cargo bench -p longtail-bench` with a synthetic 100 k-block store index, timing `preflight_get` and the p99 latency of a concurrent `get_stored_block` issued during it. | If the concurrent get's p99 is unaffected, `STORE-08` drops to a Lower-priority observation. |
| 5 | `STORE-05` (S3 half): whether any S3-compatible endpoint we actually target returns `IsTruncated=true` with no continuation token. | Against the minio in `s3-minio.yaml`, plus Cloudflare R2 and Ceph RGW if reachable: list a prefix with > 1000 objects using `max-keys=100` and dump every response's `IsTruncated`/`NextContinuationToken` pair. | A conformant result everywhere leaves the S3 half as defensive hardening (P3); a non-conformant one raises it and makes the fs half the lesser of the two. |

## Open questions for the maintainer

1. **Is a store handle ever reused across two operations, now or planned?** `STORE-09`'s safety
   rests entirely on "no" (store constructed and dropped inside each op). If the Tauri app is ever
   given a long-lived store — for a resume, a pause/resume loop, or a queue of downloads — the
   dropped-get permit leak becomes reachable and `STORE-09` goes to P1. This single answer decides
   the priority.
2. **Are mixed Rust + golongtail writers to a *filesystem* store on Windows in scope?**
   `STORE-07`/`STORE-DOC-01`. If yes, the lock mechanism needs to change; if no, it needs to be
   written down as unsupported. The differential lane runs on Windows, which makes the answer
   ambiguous today.
3. **Should `prune` refuse to delete blocks when it could not delete a superseded shard?**
   `STORE-01`. Failing harder than golongtail is a divergence, and I recommend it, but it is a
   product call on a destructive command.
4. **Is `--cache-size-limit` meant to be a ceiling or a post-run trim?** `STORE-10`. The flag name
   and `rust-port.md:146-155` read as a ceiling; the implementation is a trim that a cancelled run
   skips entirely. The fix differs a lot depending on the intent.
5. **What is the intended `worker_count` relationship between the store and the facade?**
   `resolved_worker_count` feeds both `RemoteBlockStore`'s `worker_sem` and `apply.rs`'s separate
   `Semaphore`, so the same number bounds two independent queues and effective peak block memory is
   the product-ish of the two plus the 512 MiB prefetch budget. Is that deliberate, and has the
   resulting peak RSS been measured at Fellowship scale?
6. **Should `put` and `clone-store` be cancellable?** `STORE-15`. `clone-store` is the
   longest-running command in the CLI and currently has a token nothing cancels.

## Files read

**In full:** `crates/longtail-store/src/remote.rs`, `sync.rs`, `cache.rs`, `compress.rs`,
`error.rs`, `lib.rs`, `block_store.rs`, `uri.rs`, `blob/mod.rs`, `blob/fs.rs`, `blob/mem.rs`,
`blob/s3.rs`; `crates/longtail-store/tests/prefetch_budget.rs`, `remotestore_spec.rs`,
`blobstore_spec.rs`, `actor_behavior.rs`, `decorators_integration.rs`, `s3_spec.rs`,
`sync_fixtures.rs`; `crates/longtail/tests/deadlock_regression.rs`; `crates/longtail/src/apply.rs`.

**In part (secondary axis — concurrency and cancellation only):**
`crates/longtail-cli/src/main.rs:460-620`; `crates/longtail/src/options.rs:60-320`,
`downsync.rs:80-220`, `upsync.rs:55-180`, `clonestore.rs:100-192`, `prune.rs:200-240,396-440`,
`lib.rs:60-110`; `crates/longtail-store/Cargo.toml:40-80`.

**Reference / oracle:** `docs/rust-port.md` (§Deliberate divergences, §Upstream findings, §Dropped
and deferred, §Roadmap), `CLAUDE.md`, `docs/format-spec.md` (store-index and sharding sections);
`/home/chris/github/golongtail` @ `49a20e1` — `longtailstorelib/fsstore.go:120-320`,
`longtailstorelib/fsstore_windows_amd64.go:39,55`.

**Evidence pack:** `MANIFEST.md`, `03-test.txt`, `15-coverage/summary.txt`, `12-loc.txt`,
`05-tree.txt`, `06-audit.json`, `16-deny.txt`.
