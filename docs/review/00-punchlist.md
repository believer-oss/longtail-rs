# 00 · Punchlist — pre-production review of the pure-Rust longtail port

- **Baseline:** `456274d` (branch `cm/rust-port`). Every `file:line` anchor in this document is
  relative to that commit; re-check anchors after any refactor.
- **Batch:** ten reviewers over one snapshot — R1 formats/codecs, R2 algorithms/oracle, R3
  store/concurrency, R4 performance/memory, R5 API/idioms, R6 security, R7 operations/CLI, R8
  CI/packaging, R9 docs/comments, R10 synthesis (this file). Source documents are
  `docs/review/01-…` through `09-…`; every row below names the one that owns it.
- **Scope of the mandate:** current state only, not commit history. **100% byte-compatibility with
  existing S3 and filesystem longtail stores outranks everything** — performance, elegance, idiom.
  A change that alters a byte written to `.lvi`/`.lsi`/`.lsb`, or which bytes are accepted on read,
  is a regression unless proven a bug fix.
- **Totals:** 163 code findings (3 P0 · 36 P1 · 82 P2 · 42 P3) plus 58 documentation findings.
  39 items gate the switchover.

## How to use this file

This punchlist is the artifact that survives the review. It is written to be actionable without
reading the nine source documents; open a source document only when you are about to fix the item
and need the full evidence trail and recommendation.

**Status protocol.** Each row carries a checkbox:

| Mark | Meaning |
|---|---|
| `[ ]` | open |
| `[~]` | in progress |
| `[x]` | done `<sha>` — the commit that closed it |
| `[-]` | wontfix `<reason>` — a one-line reason, always |

A later session updates **the punchlist row and the source document's finding header, nothing
else.** Do not rewrite finding bodies, do not renumber, do not delete a row: IDs are permanent and
later sessions reference them. A refuted finding is demoted to `P3-unverified` with the refutation
attached, never removed.

**Column conventions.** `Dim` = the six review dimensions (`perf`, `memory`, `idiom`, `complexity`,
`security`, `hardening`). `Compat` = `—` no byte-compat exposure · `RISK` = the fix itself can
change bytes written or accepted · `RISK!` = the same, **and no existing test would catch a
mistake**. `Effort` = S (hours) · M (a day or two) · L (more).

---

## Maintainer decisions and revised priority (2026-08-05)

The review was written without knowing the rollout plan. The maintainer has since supplied it, and it
**re-ranks the punchlist**. Where this section and a finding's own priority disagree, **this section
wins** — it reflects the deployment, not the code in the abstract.

### The rollout that actually determines risk

1. The launcher consumes the **library `get`/downsync path via API**, reading stores **written by
   golongtail**.
2. Possibly **one `put` CLI call** into a store that golongtail primarily writes.
3. **golongtail and longtail-rs will read and write the same S3 stores for a significant time** — this
   is a long-lived steady state, not a cutover window.
4. **Windows is the primary target** and is not tested on the maintainer's machine.
5. Nothing in this patch set has had **human** review yet; it is LLM/agentic work only. A launcher
   patch exists on a branch, and adding `upsync` to a CI workflow is under consideration.

### Decisions on the four open questions (§11)

| # | Question | Decision |
|---|---|---|
| 1 | Licensing | **MIT**, matching upstream and org precedent (every prior org release is MIT). Unblocks the `deny.toml` licence allowlist and the missing `license` fields. Attribution and the `longtail` name still warrant outside-counsel confirmation before external distribution — general information, not legal advice. |
| 2 | The oracle | **Retain, no deadline.** Keep until the port is comfortably in `main` and all internal usage is converted. |
| 3 | `fixtures/README.md` as a keeper | **Accepted** — but it must not be the de-facto home of CLI documentation. See CLI-DOCS below. |
| 4 | macOS | **Not a target today** (no hardware to test). Keep cheap allowances so a future iOS build workflow can pick it up; do **not** add a lane or spend effort now. |

### Reprioritisation, with reasons

**Raised.**

- **SEC-01 / SEC-02 (path traversal)** — the maintainer's stated top priority, and treated as a
  **correctness** problem, not merely a security one. The launcher reads indexes it did not create; a
  merely *buggy* index escaping `--target-path` is as bad as a hostile one. Tractable.
- **The untrusted-input panic cluster** (`FMT-001`, `FMT-002`, `diff.rs:129-133`) — same path, same
  reason. `diff.rs` panics *before the store is contacted*, so it is reachable from a bad index alone.
- **`ALG-01` / `CI-01` (Windows compat gates)** — Windows is the primary target and is untested
  locally, so CI is the *only* signal. Currently 3 of 8 gates run there. This is now a blocker.
- **Mixed-writer S3 interop** — the dominant risk given rollout point 3, and under-weighted by the
  original ranking: **`STORE-05`** (a short LIST silently narrows the store index; self-heals on `add`
  but **not** on `try_overwrite`) and **`STORE-06`** (golongtail writes `store.lsi` in place while
  Rust's parse sits outside the retry ladder, so a torn read hard-fails instead of retrying — an
  intermittent-failure vector for a launcher reading a golongtail-primary store). Gate ⑧
  (mixed Rust+Go writers) is currently **weekly only**; it should gate the switchover.
- **`OPS-07` / `API-06` / `API-02` (S3 endpoint threading)** — `put` writes an endpoint into the
  get-config that `get` never reads, and eight call sites ignore the flag. Directly on the two
  operations the rollout uses.

**Lowered.**

- **The `prune` cluster** — `prune` is **not used and unlikely to be**; stores are rotated instead,
  and upstream's weak implementation reflects the same disuse. Fix where straightforward, do not
  prioritise. Two carve-outs:
  - **`OPS-03` still lands** (empty/blank `--source-paths` deletes every block): a three-line
    empty-keep-set guard against an unrecoverable outcome is worth having whether or not the command
    is used.
  - **`FMT-003` is only half prune.** The same silent skip lives in `get_existing_store_index`
    (`store_index.rs:582`, `_ => continue // wild block`), reached from `remote.rs:818,823` on the
    **upsync/put** path — i.e. rollout point 2, against a golongtail-written store. That half keeps
    its priority; only the prune half is deprioritised.
- **The Windows fs-lock divergence** (`fs4` `LockFileEx` vs golongtail's exclusive-open `CreateFile`,
  `blob/fs.rs:17-19`) — **filesystem stores only.** S3 hardwires `supports_locking() == false`
  (`blob/s3.rs:10,315`, matching `s3Store.go:106`) and uses the lockless sharded
  `store_<sha256hex>.lsi` merge-on-read path, so this cannot affect the S3 rollout. It remains a
  documentation gap (absent from `rust-port.md` §Deliberate divergences), not a launch risk.
- **macOS**, **release-profile/binary-size work**, and the **perf/memory backlog** — deferred per
  decisions above and the roadmap reversal in §8.

### Two new work items this creates

- **CI-TRIGGERS** — the current schedules encode two false premises: that **upstream is a moving
  target** (it has not changed significantly in over a year) and that **this repo will undergo
  constant churn** (it will not). Weekly `fixture-freshness` and weekly `differential` runs mostly
  burn minutes proving upstream still equals itself. Re-point the heavy lanes at what testing is
  actually for: **(a)** staying interoperable with the production store, and **(b)** surviving normal
  maintenance, above all Rust dependency upgrades. Concretely: trigger the differential and
  fixture-freshness lanes on **`Cargo.lock`/`Cargo.toml` changes** (so every Renovate PR proves
  interop) plus **`workflow_dispatch`** before a switchover, and drop or heavily stretch the cron.
  Promote **gate ⑧** into that dependency-triggered set. Supersedes the cadence advice in `08-…`'s
  lane table. Owner: R8's lane structure. **P1, effort S.**
- **CLI-DOCS** — the CLI has no real documentation: 46 of 53 flags have no keeper-doc coverage and 32
  exist *only* in `switchover-checklist.md`, which the plan retires. `fixtures/README.md` becoming a
  keeper does not fix this. Author a CLI reference (generated from clap where possible, so it cannot
  drift) as its own document; that is also the precondition for retiring the checklist. Related:
  `DOCS` P1 on flag coverage. **P1, effort M.**

### Standing caveat

Everything in this branch, including `switchover-checklist.md` and this review, is **unreviewed by a
human**. The blocker tables below are a reasonable agenda for that human review; they are not a
substitute for it.

---

## 1 · Clusters — read this before the tables

Five convergences matter more than any single finding. Each is one defect class seen from several
slices; fixing them cluster-wise is cheaper and safer than fixing them row-wise.

### 1.1 The `prune` cluster — the most important structural finding in this review

**Four independent data-loss findings from three reviewers land on the same three functions.** No
single one is the bug; the shape of the code is the bug. `prune` treats every failure and every
anomaly as something to skip, and it is the one code path in the workspace that deletes customer
data.

| ID | The skip | The consequence |
|---|---|---|
| `OPS-03` **P0** | empty/blank `--source-paths` → empty keep-set, no guard | every `.lsb` in the store is deleted, exit 0, silent |
| `FMT-003` P1 | a corrupt block chunk-range is skipped when building the keep-set | that block's `.lsb` is deleted while a shipped version still references it |
| `STORE-01` P1 | a shard-delete error is discarded (`let _ = old.delete().await`, then `Ok(true)`) | the stale shard merges back on read → dangling index entries → `downsync` fails on `NotFound` |
| `STORE-02` P1 | every block-delete error is swallowed (`.is_ok()` in a let-chain) | orphaned blocks are invisible to any future `prune-store`; recoverable only via a separate `prune-store-blocks` sweep nothing signals |
| `STORE-05` P1 | a listing that silently returns short narrows the index feeding all of the above | supplies the "shard was never seen" input to `STORE-01` |

**Single remediation theme: `prune` must fail loudly and refuse to act on an anomaly.** Concretely,
one change set: (a) refuse an empty keep-set without an explicit opt-out flag; (b) make the
wild-block skip in `get_existing_store_index` an error on destructive paths; (c) collect and return
shard- and block-delete failures instead of counting successes; (d) make a short listing a hard
error. Do (a) first and alone — it is three lines and removes the catastrophic case.

The pre-delete index overwrite (`remote.rs:560-563`) is **deliberate** upstream parity — a crash
leaves harmless orphans rather than dangling entries. Keep it. The defect is the missing signal on
failure, not the ordering.

### 1.2 The untrusted-input panic cluster — a documented guarantee that is false

`crates/longtail-core/src/error.rs:4` and `src/lib.rs:40-42` promise malformed input never panics.
Six findings contradict it, all reachable from bytes fetched over the network on the production
Tauri download path.

| ID | Surface |
|---|---|
| `FMT-001` P1 | `.lvi` asset→chunk map values are never validated; `validate.rs:56` and `apply.rs:110` index them raw |
| R6 Appendix A.2 | the same gap makes **seven** call sites reachable, including `diff.rs:129-133`, which panics **before the store is contacted** |
| `FMT-002` P1 | `.lsb` payload length is never checked against Σ`chunk_sizes`; `apply.rs:342` slices past the end |
| `ALG-02` P1 | `decode_block_payload` allocates from an untrusted frame header; brotli decode is unbounded on output |
| `SEC-05` P1 | `cp.rs:82` sizes a `Vec` from a raw `u32` asset chunk count → 32 GiB request |
| `SEC-06` P1 | the *same* malformed input is a typed `Err` on the `spawn_blocking` writer and a **process abort** on the rayon codec pool |

**Single remediation theme: one validated accessor, one bound, one panic policy.** Add
`VersionIndex::asset_chunks(a) -> Result<&[u32]>` (replaces seven unguarded copies), reject a short
`.lsb` payload in `BlockIndex::read_prefix`, pass a `max_uncompressed` into
`decode_block_payload`, and install a rayon `panic_handler` so both paths fail the same way.
`cp.rs:145-152` is already the ready-made patch for `FMT-002`, in the same crate.

### 1.3 The path-traversal cluster — both P0s, one guard

`SEC-01` (write/read primitive) and `SEC-02` (delete primitive) are one missing function.
Independently verified: `is_absolute`, `Component::ParentDir`, and `.components()` return **zero
hits** across `crates/*/src`, and no test anywhere feeds `..`, an absolute path, or a drive letter
through apply. A single `safe_join(root, rel) -> Result<PathBuf>` gating the seven `fs_util` sites
closes both. See §3.

### 1.4 The Windows cluster — a green lane over a barely-tested platform

| ID | Half |
|---|---|
| `ALG-01` P1 | code: every `crates/longtail*` integration test is `#![cfg(unix)]`, so the Windows PR lane runs 3 of 8 documented compat gates |
| `CI-01` P1 | lane: `pure-windows` runs build + test + verify-fixtures only — no clippy, no bench compile-check, no facade/CLI integration test |
| R7 Named deliverable 3 | behaviour: nothing exercises permissions, deletes, resume, or the CLI on Windows |
| `CI-13` P2 | structure: xtask's pinned-golongtail channel is Linux-only (URL, SHA, lookup path), so gates ⑥/⑧ are **impossible** on Windows and their skip is green |
| `STORE-07` P2 · `OPS-08`/`OPS-09` P1 | the specific Windows behaviours nobody tests: `LockFileEx` vs `CreateFile` mixed-writer, case-insensitive path aliasing, device names and NTFS streams |

**Remediation order:** de-`cfg(unix)` the test files with mode masking (code, R2/R7) → add a
per-lane expected-test-count assertion (CI, R8) → then decide `CI-13`. The count assertion is the
control that would have caught `ALG-01` the day it happened.

### 1.5 The endpoint/config cluster — one flag, four leaks

`OPS-07` P1 (verified: 8 call sites; `read_version_index_from_uri`/`read_store_index_from_uri` take
no options) · `API-02` P1 (the signature fix — break it now, before the first tag) · `API-06` P2
(verified: `put.rs:173` writes `s3-endpoint-resolver-uri`, `get.rs` has zero occurrences) ·
`API-07` P2 (the newest S3 knob reached 4 of 12 subcommands; `put` and `clone-store`, the biggest
transfers, cannot opt out). One change: an options struct on the two readers, threaded from the
existing CLI plumbing, plus the get-config key round-trip.

### 1.6 The observability cluster — a diagnostics contract with no implementation

`API-12` P2 (the facade emits zero tracing; `tracing` is an unused dep of `longtail`) ·
`API-11`/`STORE-04` P1 (every block-get error flattens to `Backend`, breaking the
`NotAuthorized`/`Network`/`NotFound` guarantee `lib.rs:82-85` advertises; task panics surface as
`Io`) · `API-DOC-02`/`OPS-DOC-03` ("Logging is `tracing`-based" is unearned). The launcher shows
"network problem — retrying" for expired credentials and "disk error" for a code bug. One change:
`ErrorClass` + `LongtailError::class()` landed together with the `STORE-04` fix that makes the
mapping truthful.

---

## 2 · Adversarial verification

Every P0 and the strongest P1s were handed to an independent verifier instructed to **disprove**
them, under one rule:

> A finding survives only if the verifier can quote the `file:line` that makes it true, or name the
> test or experiment that would decide it. Otherwise it is demoted to **P3-unverified** — never
> silently deleted.

| Finding | Verdict | What the refuter established |
|---|---|---|
| `OPS-03` P0 | **SURVIVES** | All six refutation routes fail. `prune.rs:188-206` keep-set loop, only early exit is dry-run at `:209-215`, destructive call at `:232`. `read_lines_file` (`main.rs:569-579`) returns `Ok(vec![])` for empty *and* whitespace-only. `--dry-run` is `default_value_t = false` (`main.rs:333`). `prune_blocks(&[])` has no empty-keep special case. **Worse than filed:** the store *index* is wiped first (`remote.rs:560-563`), so even blocks whose deletion fails become unreachable. Success is silent unless `--show-stats`. |
| `SEC-02` P0 | **SURVIVES**, two corrections | Chain verified link by link: default on (`options.rs:100`, `main.rs:614`), caches `source_version` not a rescan (`downsync.rs:218-220`), read back ahead of `--scan-target` (`downsync.rs:70-78,113-115`), no containment check on the delete path (`fs_util.rs:239-268`), `ensure_user_writable` actively chmods `+w` to defeat a read-only victim. **Corrections:** (a) "before any network fetch" is false — `get_existing_content`/`preflight_get` precede `delete_assets`; the accurate claim is "before any byte is written, and never rolled back". (b) The cache route needs run 1 to have completed. **Stronger variant found:** `--target-index-path` (`downsync.rs:62-66`) feeds an arbitrary `.lvi` in as `current` with **no** precondition — a pure delete primitive on the first invocation. |
| `SEC-03` P1 | **SURVIVES — understated** | `pack.rs:24-30` hashes chunk hashes only. **Worse:** `remote.rs:362` never recomputes the hash — it compares the *self-declared field parsed from the file* (`block.rs:54`) against the requested one, so the whole block index is unbound too; `cache.rs:97` is the same check. Zero chunk-byte re-hashing anywhere on the fetch/apply path. `apply.rs:317`'s cross-check of block- vs `.lvi`-declared chunk size is a `debug_assert_eq!`, stripped in release. **Nuance to carry:** `--validate` *is* a genuine content re-hash — its trust anchor is the `.lvi`, so it is a real mitigation **iff** the `.lvi` is pinned or delivered out-of-band. |
| `SEC-04` P1 | **SURVIVES** | `fs.rs:323-325` `read_to_end` with no metadata check; `s3.rs:378-381` `body.collect()` with `content_length()` never inspected. Budget is prefetch-only **by design** (`remote.rs:17-18`, `:88`); the demand path registers `_permit: None` at `:477`. **No fetched-length vs declared-size check exists anywhere**, so the "memory already committed" demotion is unavailable. Worker semaphore: `min(NumCPU,8)` s3, uncapped fs (`uri.rs:72-84`). The `max_prefetch_bytes` knob exists but `uri.rs:119` calls it test-oriented and the CLI never sets it. |
| `STORE-02` P1 | **PARTIAL** — mechanism survives, one claim refuted | Swallow confirmed verbatim at `remote.rs:572-576`; `:578` returns `Ok(pruned_count)`. Pre-delete index overwrite and index-derived `source` on later runs both confirmed. **"Permanently unreclaimable" is OVERSTATED and must be softened:** `prune_store_blocks` (`prune.rs:408-432`) enumerates actual blobs via `client.get_objects("")`, not the index, so orphans are recoverable by a separate sweep. Correct wording: *silently orphaned; unreachable by any future `prune-store`, recoverable only via a `prune-store-blocks` sweep nothing signals the operator to run.* Refuter suggests P1→P2; **kept at P1** because the swallow is also present in the recovery path (`prune.rs:427-431`), so the escape hatch has the same defect. |
| `STORE-03` P1 | **SURVIVES** | `fs.rs:240` `f.sync_all().ok();`, `:242` rename, no parent-dir fsync; function read in full at `:225-246`. `:240` is the **only** sync call in the entire workspace (grep over all `crates/*/src` + testkit + bench). All four blob classes route through it — single caller `fs.rs:353` inside `BlobObject::write`, reached by `store.lsi` (`sync.rs:229,236,343`), shards (`sync.rs:262,351`), `.lsb` (`remote.rs:401,412`), `.lrb` (`cache.rs:83,120`). No caller compensates; no durability test or flag. **Context:** mirrors golongtail's `fsstore.go`, so likely inherited parity — worth a line in the fix, does not change the analysis. |
| `STORE-05` P1 | **SURVIVES**, three refinements | All three fs skips confirmed (`fs.rs:109-111,113,115-118`, `Ok(out)` at `:143`); S3 break confirmed (`s3.rs:303-310`). **Worse:** a transient failure on the store *root* returns `Ok(vec![])` → `sync.rs:191-192` → a **completely empty** store index, no error. **Refinements:** (a) the S3 leg needs a non-compliant endpoint — AWS always sends the token; the fs leg is the realistic one, and its silence is documented Go parity. (b) `try_overwrite` re-lists at delete time (`sync.rs:345`), so the mechanism is a two-mode split, not "deletes what it saw": short at index-read → the invisible shard **is** found and deleted (metadata destroyed); short at `:345` → the shard survives and resurrects into dangling entries. (c) The block deletion is `prune_store_blocks` (`prune.rs:418-432`), not `RemoteBlockStore::prune_blocks`. |
| `SEC-05` P1 | **(a) SURVIVES**, three corrections · **(b) PARTIAL** | (a) `cp.rs:82-83` reserves from the raw u32 **before** the indexing at `:85`, so the OOB panic cannot pre-empt it; `from_bytes` has exactly four checks (`version_index.rs:148,156,169,181`) and none constrains `asset_chunk_counts` (`:192` reads it verbatim). **Corrections:** it is **32 GiB** (2³⁵ B), not 34; the malicious `.lvi` is **~64 bytes**, not "a few hundred"; and **abort is environment-dependent** — certain on Windows (commit charge, and a Windows CLI ships) and on any Linux host with RAM+swap < 32 GiB, but on a larger Linux host the lazy mapping succeeds and you get a *catchable* OOB panic instead. (b) The asymmetry table is exactly right and `apply.rs:342` is bare and reachable — but **impact downgraded** from crash to contained operation error plus a partially written file. The `cp.rs:118-127` direction is **REFUTED as reachable** (the index comes from `get_existing_content` → `from_block_indexes`, canonical by construction), confirming the finding's own concession. |
| `OPS-02` P1 | **SURVIVES** — pivotal question resolved *for* the finding | A modified asset is **not** deleted first: `diff.rs:72-91` puts a path present in both versions into `target_content_modified_asset_indexes` (`:77`) and only absent paths into `source_removed_asset_indexes` (`:86`) — disjoint sets. Step 5b therefore truncate-opens the existing file in place (`apply.rs:155` → `fs_util.rs:134-139`). `ensure_user_writable` has exactly two callers, both inside `remove_asset` (`fs_util.rs:249,262`). Windows fails too, via `set_readonly` at `fs_util.rs:207-208`. **Nuance to carry:** stock golongtail on the `ChangeVersion2` path has the *same* failure — the port's regression is specifically the loss of the `--use-legacy-write` escape, which the CLI accepts (`main.rs:133`) and the library hard-rejects (`downsync.rs:31-33`). |
| `ALG-02` P1 | **SURVIVES**, two corrections | `BrotliDecompress` has **no limit parameter at all** (`brotli-decompressor-5.0.3/src/lib.rs:82-95`) and writes into our growable `Vec` — unbounded growth proven at the dependency, not inferred. The brotli tag **is** reachable from an arbitrary `.lsb`: `compress.rs:261-268` dispatches on family prefix with no configured-codec allowlist. `longtail-store/src/compress.rs:73-74` has `block_index` in scope and ignores `Σ chunk_sizes`. **Corrections:** lz4 is bounded on *growth* but **not** on the allocation request (`vec![0; min_uncompressed_size]`); zstd is the only fully bounded codec (clamps via `upper_bound`). "Abort on failure" is environment-dependent for the same overcommit reason as `SEC-05`; the reliable lever is the brotli **bomb** (unbounded growth), which is what makes this brotli-specific. `codec_malformed.rs:111-129` flips one byte, so it can move a declared size by at most 255 — it cannot reach this. |
| `FMT-002` P1 | **SURVIVES**, one attribution corrected | `block.rs:132-139` is the whole function and its own doc says the payload "can be any length, including 0". `apply.rs:342` slice is bare; `len` is additionally **grown** by the run-merging loop at `:333-341`, widening the overflow. Reachable for both `tag == 0` (truncate the `.lsb`) and `tag != 0` (declare `uncompressed_size < Σ chunk_sizes` — `compress.rs:314` then passes). **Correction:** the catch is the inner `spawn_blocking` join at `apply.rs:224-230`, **not** `flatten_apply_task` at `:353-363` as filed; same net effect. `cp.rs:145-152` is the sibling that already does the check, proving `apply.rs` is the outlier. |

**Pre-verified by the orchestrator (no refuter spent).** `SEC-01`/`SEC-02` premise (zero
containment checks, no test) · `FMT-001` (`validate.rs:56`, `apply.rs:110` index raw) ·
`FMT-003`/`STORE-01` (`push_block` returns `Err(ChunkRangeOutOfBounds)`; `sync.rs:358-364` swallows
then returns `Ok(true)`) · `OPS-07` (8 call sites) · `API-06` (`put.rs:173` writes it, `get.rs` has
zero occurrences) · `OPS-14` (re-verified here: `clonestore.rs:195` is
`target_lvi.replace(".lvi", ".lsi")` with no exists-check between the two `write_to_uri` calls at
`:191` and `:197`) · `OPS-06` (re-verified here: `grep -c alias crates/longtail-cli/src/main.rs`
→ **0**).

---

## 3 · Switchover blockers

**39 items.** P0 = data loss, corruption, byte-compat break, hang, security hole, or a destructive
operation that can fire wrongly. P1 = fix before switchover, or accept with a **written**
mitigation. Format per item: status · ID · priority · dimension · compat · effort · owning
document, then the anchor, then the one-sentence failure scenario, then the test that would prove
it.

### 3.1 P0 — must not ship

- [~] **`SEC-01`** · P0 · `security` · `COMPAT-RISK!` · S · R6
  **FIXED in the working tree (uncommitted, 2026-08-05).** `fs_util::safe_join` added and all seven
  sites routed through it; `LongtailError::UnsafeAssetPath { path, reason }` added. Guard is lexical
  and *rejects* rather than sanitises, per R6's adjudication (parse-time rejection would break the
  `.lvi` byte fixpoint and make hostile indexes un-inspectable).
  **Now verified-by:** 9 unit tests in `fs_util` (traversal table, containment property, the
  `Path::join`-discards-root mechanism) + `crates/longtail/tests/path_traversal.rs` (5 tests through
  the public `downsync`). Windows drive/UNC/device-name rules live in `windows_unsafe_component`,
  which is compiled and unit-tested on **every** host but enforced only on Windows — `aux`, `a:b`,
  and `x ` are legal POSIX names and refusing them on unix would break real stores. Full suite,
  clippy `-D warnings`, fmt, and `verify-fixtures` all green; both byte gates unaffected.
  → set to `[x]` with the sha at commit time.
  `crates/longtail/src/fs_util.rs:104,110,116,133,150,191,240` ← `crates/longtail-core/src/version_index.rs:110`
  **Fails when:** a `.lvi` names `../../../home/user/Documents/x.docx` or an absolute path — downsync
  creates the parent, truncates, writes, and chmods that file outside `--target-path`; the
  `--source-index-path` arm of upsync reads out.
  **verified-by:** none. `is_absolute`, `Component::ParentDir`, `.components()` = **0 hits** across
  `crates/*/src`; no test feeds `..`, an absolute path, or a drive letter through apply.
  **Fix:** one `safe_join(root, rel) -> Result<PathBuf>` gating all seven sites. `COMPAT-RISK` because
  it changes which `.lvi` files are *accepted*; no gate covers it — add the proptest in §7.

- [~] **`SEC-02`** · P0 · `security` · — · S (rides SEC-01) · R6
  **FIXED in the working tree (uncommitted, 2026-08-05)** — `remove_asset` routes through the same
  `safe_join`, so the delete primitive is closed with the write primitive.
  **Now verified-by:** `path_traversal.rs::hostile_current_index_cannot_delete_outside_the_target`,
  which uses the **`--target-index-path`** variant the refuter found (no prior run, no cache) and
  asserts the victim file survives byte-for-byte *and* that downsync returns
  `UnsafeAssetPath`. Worth recording: the first draft of that test passed **vacuously** — both
  indexes were given the same constant `path_hashes`, so the diff saw one asset and attempted no
  delete at all (`assets_removed: 0`). Distinct path hashes were required to make the delete phase
  actually run. Any future test in this area must assert the operation was *attempted*, not merely
  that the victim survived.
  → set to `[x]` with the sha at commit time.
  `crates/longtail/src/downsync.rs:218-220` → `:70-78,113-115` → `crates/longtail/src/apply.rs:89,439` → `crates/longtail/src/fs_util.rs:239-268`
  **Fails when:** run 1 caches the *source* `.lvi` into the target dir; run 2 reads it back as
  `current`, diffs an escaped asset as removed, and `delete_assets` chmods it `+w` and unlinks it —
  outside the target root, before any byte is written, and the run reports success.
  **verified-by:** none. `commands_spec.rs:369-399` proves a planted cache file is read by default
  and rejected only on *parse*.
  **Also:** `--target-index-path` (`downsync.rs:62-66`) is the same delete primitive with **no**
  precondition — no prior run, no cache. Guard both.

- [ ] **`SEC-01b`** · P2 · `security` · — · S · orchestrator (2026-08-05)
  `crates/longtail/tests/path_traversal.rs::symlink_inside_target_is_followed_by_design`
  **Behaviour pinned, deliberately not "fixed".** `safe_join` is lexical, so a pre-existing symlink
  *inside* the target tree is still followed when it points outside. Verified empirically: a clean
  `link/through-link.txt` asset wrote through the symlink and `downsync` reported success.
  **Why this is not a defect:** longtail never creates symlinks (`scan_folder` skips non-file/non-dir
  entries; the format has no symlink asset type), so any symlink in the target was placed by whoever
  controls the target directory — the same operator who supplied `--target-path`. A game install whose
  asset directory is symlinked to another drive is a legitimate, common setup, and
  canonicalise-and-contain would **break** it. An attacker who can plant a symlink there already has
  write access and does not need longtail to escape.
  **Remaining work is documentation, not code:** fold this into `SEC-DOC-02`'s trust-boundary
  statement so the next optimisation pass does not "harden" it by accident. If it is ever revisited,
  the delete arm is the sharper edge (unlinking through a symlink into a user's other drive).
  **Dependency note (asked 2026-08-05):** no new crate is warranted for the guard itself —
  `std::path::Component` catches `..`, `RootDir`, and (on Windows) `C:`, `\\?\`, and UNC in one match
  arm. `path-clean`/`normpath` are actively wrong here, since they *normalise* where we must
  **reject**. **`dunce`** earns its place only in a canonicalising design: `fs::canonicalize` returns
  a `\\?\` verbatim path on Windows that will not compare equal to a normal root, and `dunce`
  is the standard fix — so it is the right call *if* `SEC-01b` is ever taken up, and unnecessary until
  then. **`cap-std`** (capability-based `openat` roots) is the structurally correct end-state, immune
  to TOCTOU rather than merely checking for it; it is a real refactor of the fs layer and belongs in a
  post-switchover discussion, not here.

- [x] **`OPS-03`** · P0 · `security` · — · S · R7
  **FIXED 74b9f7d.** `prune-store` and `prune-store-index` refuse an empty keep-set; `--allow-empty-keep-set` opts out. Checked in dry-run too. **verified-by:** `commands_spec.rs::prune_store_refuses_an_empty_keep_set` (empty and all-blank list files, `.lsb` count unchanged after each refusal, then the opt-out really does wipe the store).
  `crates/longtail/src/prune.rs:188-232`; `crates/longtail-cli/src/main.rs:569-579,816-838`
  **Fails when:** a CI step's `aws s3 ls … > versions.txt` produces an empty or blank file, and
  `prune-store --source-paths versions.txt` overwrites the store index with an empty one and deletes
  every `.lsb` — silent, total, irreversible, exit 0, `--dry-run` off by default.
  **verified-by:** none. `commands_spec.rs:1015-1069` covers only a *non-empty* keep set.
  **Fix:** three lines — refuse an empty resolved keep-set unless an explicit `--allow-empty-keep-set`
  is given. Land this one alone, first.

### 3.2 P1 — correctness and data integrity

- [x] **`FMT-001`** · P1 · `hardening` · — · S · R1 (blast radius: R6 App. A.2)
  **FIXED 8d6c731.** `VersionIndex::from_bytes` now validates the asset→chunk map once (`checked_add` on `usize`, so the 32-bit wrap-to-wrong-answer case is covered too), with two new `FormatError` variants. This also closes the `diff.rs:129-133` pre-network panic R6 found, since that is one of the six raw-indexing consumers. **verified-by:** `malformed.rs::version_index_{accepts_a_consistent_map,rejects_asset_chunk_range_past_the_map,rejects_chunk_index_past_the_chunk_arrays}`. All 112 fixtures still parse; miri clean.
  `crates/longtail-core/src/version_index.rs:169-187`, `crates/longtail-core/src/validate.rs:52-59`
  **Fails when:** a `.lvi` that parses cleanly with `asset_chunk_index_starts = [0xFFFF_FFFF]` panics
  "index out of bounds" — reachable from `validate-version`, `prune-store{,-index}`, `clone-store`,
  `ls`, `cp`, `upsync`, and via `apply.rs:110` the production Tauri download path.
  **verified-by:** none. **Fix:** one O(A+ACI) validation pass in `from_bytes`, or the shared
  `asset_chunks()` accessor — which also closes six of the seven sites R6 enumerates, including
  `diff.rs:129-133`, which panics **before the store is contacted**.

- [x] **`FMT-002`** · P1 · `hardening` · `COMPAT-RISK` · S · R1
  **FIXED 8d6c731 + a895983.** `StoredBlock::from_bytes` rejects a `tag == 0` payload shorter than Σ`chunk_sizes`, deliberately `<` and not `!=` so a longer tail is still accepted (C derives the payload size from file length). **verified-by:** `malformed.rs::stored_block_{rejects_payload_shorter_than_chunk_sizes,accepts_a_longer_payload_deliberately,payload_rule_applies_only_to_uncompressed_blocks}`.
  **Compressed arm — raised by the synthesis refuter, not the original finding — also closed:**
  `decode_block_payload` compares the decoded length against the frame's *own* declared
  `uncompressed_size`, and both numbers come from the same attacker-controlled bytes, so they agree
  happily; nothing tied either to the block index. `BlockIndex::uncompressed_len()` is now the single
  definition of the invariant, applied after decompression in `CompressBlockStore::get_stored_block`
  — the production read path, since `decode_stored_block` turns out to have no production consumer.
  **verified-by:** `decorators_integration.rs::compressed_block_shorter_than_its_chunk_sizes_is_rejected`.
  `crates/longtail-core/src/block.rs:129-139`; consumer `crates/longtail/src/apply.rs:303-306,342`
  **Fails when:** an `.lsb` whose payload is shorter than Σ`chunk_sizes` reaches `write_block_chunks`,
  which slices past the end and panics inside the write task; the operator sees "join error", not the
  corrupt block. Reachable for `tag == 0` by truncation and for `tag != 0` by declaring
  `uncompressed_size < Σ chunk_sizes`.
  **verified-by:** none. **Fix:** reject a payload *shorter* than the sum (use `>=`, not `==` — C
  derives the size from the file length and ignores a longer tail, so equality could reject a block a
  real store contains). `cp.rs:145-152` is the patch. Gate: `fixtures/` + a new short-payload fixture.

- [ ] **`FMT-003`** · P1 · `security` · — · M · R1 (cluster §1.1)
  `crates/longtail-core/src/store_index.rs:580-583,520-524`; consumer `crates/longtail/src/prune.rs:200,232`
  **Fails when:** one block's chunk range is corrupt in the store index → the block is silently
  skipped when building the keep-set → its hash never enters `keep` → its `.lsb` is deleted from the
  store, permanently, while a shipped version still references it. `--validate-versions` defaults
  `false` (`main.rs:338`).
  **verified-by:** none. **Fix:** make the wild-block skip an error on destructive paths; settle the
  crate-wide skip-vs-error policy (five `StoreIndex` methods currently do three different things).

- [ ] **`STORE-01`** · P1 · `hardening` · — · M · R3 (cluster §1.1)
  `crates/longtail-store/src/sync.rs:357-364` + `crates/longtail-store/src/remote.rs:563-577`
  **Fails when:** a stale `store_*.lsi` shard fails to delete (S3 `AccessDenied`, a Windows sharing
  violation, or it was never listed — `STORE-05`), prune proceeds to delete the blocks that shard
  references, merge-on-read unions it back, and the next `downsync` fails `NotFound` on content the
  index advertises.
  **verified-by:** none. `let _ = old.delete().await` then `Ok(true)`.

- [ ] **`STORE-02`** · P1 · `hardening` · — · S (M for the trait change) · R3 (cluster §1.1)
  `crates/longtail-store/src/remote.rs:572-576,578`; twin at `crates/longtail/src/prune.rs:427-431`
  **Fails when:** an IAM policy denies `s3:DeleteObject` — `prune-store` reports "pruned 0 blocks",
  exits 0, and the orphans are invisible to every future `prune-store` because the index was already
  rewritten without them.
  **verified-by:** none; `FlakyStore` (`actor_behavior.rs:101-117`) injects only *read* failures.
  **Wording corrected by refuter:** not "permanently unreclaimable" — recoverable via a
  `prune-store-blocks` sweep, which nothing signals the operator to run **and which carries the same
  swallow**. Kept P1 for that reason.

- [x] **`STORE-03`** · P1 · `hardening` · — · S · R3
  **FIXED 4bc9a62.** `sync_all` errors are propagated everywhere (the part with no defence). The directory fsync is scoped to the store index and its `.gen` sidecar: measured on local NVMe it costs ~0.8 ms per file and roughly doubles a small-file write, which is fine once per flush and not fine per block, since the same primitive writes every `.lsb` and `.lrb`. The sidecar now rides the same atomic path so a durable index cannot be paired with a lost generation. **verified-by:** `fs.rs::{only_index_writes_force_the_directory_entry,generation_sidecar_round_trips_through_the_atomic_path}`. **Residual, deliberate:** block directory entries are still not forced before the index that references them is committed, so a crash can leave a dangling entry; closing that wants one fsync per directory before the index write, not one per block. The `sync_all` propagation itself is not directly tested — that needs I/O fault injection.
  `crates/longtail-store/src/blob/fs.rs:240,242`
  **Fails when:** `sync_all` fails EIO or delayed-allocation ENOSPC — the rename proceeds anyway,
  `Ok(true)` propagates to exit 0, and on power loss the store index reverts a generation, orphaning
  every block uploaded that session.
  **verified-by:** none. `:240` is the **only** sync call in the workspace; no parent-dir fsync exists.
  **Note:** likely inherited golongtail parity; say so in the fix.

- [ ] **`STORE-04`** · P1 · `hardening` · — · S · R3 (cluster §1.6, with `API-11`)
  `crates/longtail-store/src/remote.rs:490,626-632`; broken promise at `crates/longtail/src/lib.rs:82-85`
  **Fails when:** the Tauri app hits a 403 on one `.lsb` mid-transfer (credentials rotated, refresh
  failed) and `matches!(e, StoreError::NotAuthorized)` is **false** — the launcher shows "check your
  connection" and retries forever against a permanent failure.
  **verified-by:** none. Fix together with `API-11`; the minio auth round-trip is the test (§10 EXP-11).

- [x] **`STORE-05`** · P1 · `hardening` · — · S · R3 (cluster §1.1)
  **FIXED 1fb6be6.** Both arms. The fs walker returns errors instead of skipping past a failed `read_dir`/entry/`metadata` (an absent store root is still an empty store — the one case Go's swallow was right about); the S3 pager treats `IsTruncated=true` with no continuation token as a hard `Backend` error rather than `break`. **verified-by:** `mixed_writer.rs::{unreadable_directory_fails_the_listing_instead_of_shortening_it,absent_store_root_still_lists_empty}` and `s3_spec.rs::truncated_listing_without_a_continuation_token_is_an_error` — the S3 one uses `StaticReplayClient`, so it runs per-PR with no minio.
  `crates/longtail-store/src/blob/fs.rs:109-118`, `crates/longtail-store/src/blob/s3.rs:303-310`
  **Fails when:** a transient EIO on the store root returns `Ok(vec![])` → `sync.rs:191-192` → a
  **completely empty** store index with no error; or one shard is invisible for a moment and the
  overwrite path destroys its block metadata (short at index-read) or resurrects it into dangling
  entries (short at delete-time re-list, `sync.rs:345`).
  **verified-by:** none. The fs leg is the realistic one and is documented Go parity; the S3 leg needs
  a non-conformant endpoint (§10 EXP-10).

- [ ] **`SEC-03`** · P1 · `security` · — · S doc / M opt-in flag · R6
  `crates/longtail-core/src/pack.rs:24-30`, `crates/longtail-store/src/remote.rs:362`, `crates/longtail/src/apply.rs:215-232`
  **Fails when:** an attacker with write access to one `.lsb` replaces a game asset or executable,
  keeping the chunk-hash bytes intact — every layer reports success, and `--validate` compares the
  result against the attacker's own `.lvi`.
  **verified-by:** none. **Inherited from C; not fixable in the format.** Deliverables: write the trust
  boundary down (`SEC-DOC-02`) and offer an opt-in `--verify-chunks`. Refuter found it *understated*:
  `remote.rs:362` compares a **self-declared header field**, never recomputing the hash, so the block
  index is unbound too; and `apply.rs:317`'s cross-check is a release-stripped `debug_assert_eq!`.

- [ ] **`SEC-04`** · P1 · `memory` · — · M · R6 (cluster §1.2)
  `crates/longtail-store/src/blob/fs.rs:323-325`, `crates/longtail-store/src/blob/s3.rs:378-381`, `crates/longtail-store/src/remote.rs:519`
  **Fails when:** a hostile or corrupted store serves one oversized `.lsb` — RSS goes to object size ×
  in-flight workers (min(NumCPU,8) s3, uncapped fs) and the Tauri app is OOM-killed with no error
  surface; the 512 MiB budget is prefetch-only *by design* and is sized from *declared* sizes.
  **verified-by:** none, and **no fetched-length vs declared-size check exists anywhere**.
  **Fix:** one `max_block_bytes` on `BlockStoreOpts`, enforced at the two transport reads and in
  `decode_block_payload`.

- [ ] **`SEC-05`** · P1 · `hardening` · — · S · R6 (cluster §1.2)
  `crates/longtail/src/cp.rs:80-88`; `crates/longtail/src/apply.rs:342` vs `crates/longtail/src/cp.rs:145-152`
  **Fails when:** `longtail cp` against a hostile ~64-byte `.lvi` with
  `asset_chunk_counts[0] = 0xFFFF_FFFF` requests a **32 GiB** `Vec` before any network I/O.
  **verified-by:** none; `malformed.rs:219-229` fuzzes only the header counts.
  **Severity depends on §10 EXP-06:** abort is certain on Windows and on Linux hosts with RAM+swap
  < 32 GiB; on a larger Linux host it degrades to a catchable OOB panic. Part (b)'s `cp.rs:118-127`
  half is **refuted as reachable**; part (b)'s `apply.rs:342` half is `FMT-002`.

- [ ] **`SEC-06`** · P1 · `hardening` · — · S · R6 (cluster §1.2)
  `crates/longtail/src/apply.rs:221-230,350-363` (caught) vs `crates/longtail-store/src/compress.rs:38-48` (not caught)
  **Fails when:** two customers report "the app closes with no error" — one is an unwind to the top,
  one is an abort on the rayon codec pool. Indistinguishable symptoms, different fixes, and only the
  second is reachable by a `panic_handler`.
  **verified-by:** none. Fix with `STORE-12`/`API-15`; convert
  `store/compress.rs:47`'s `expect` to `StoreError::WorkerGone` in the same change — R6 App. A shows
  it is the one `expect` whose reachability *changes* when the handler lands. Decide via §10 EXP-09.

- [ ] **`ALG-02`** · P1 · `memory` · — · S · R2 (cluster §1.2)
  `crates/longtail-core/src/compress.rs:239-244,302-313`; wired for every URI at `crates/longtail-store/src/uri.rs:158`
  **Fails when:** an `.lsb` tagged brotli (reachable — `compress.rs:261-268` dispatches on family
  prefix with no allowlist) carries a bomb payload: `BrotliDecompress` grows the output `Vec` with **no
  limit parameter in the API**, past the declared size, until the machine OOMs — × N concurrent
  workers. The length check at `:314` runs after.
  **verified-by:** none; `codec_malformed.rs:111-129` flips one byte and cannot move a declared size
  by more than 255. **Fix:** validate `uncompressed_size == Σ chunk_sizes` in
  `longtail-store/src/compress.rs:73-74` (which already holds `block_index`) *and* give brotli a
  bounded sink. lz4 also needs the declared-size cap; zstd is already bounded.

- [ ] **`OPS-01`** · P1 · `hardening` · — · S · R7
  `crates/longtail/src/downsync.rs:174-176,218-220`; `crates/longtail/tests/smoke.rs:42`
  **Fails when:** someone moves the cache-index write earlier for "crash resilience" — `smoke.rs`
  still passes because it *disables* `cache_target_index` (the library and CLI default), and every
  interrupted production download then resumes against a cache claiming the target is already the new
  version, writes nothing, and exits 0 with holes in the game files.
  **verified-by:** `smoke.rs::cancel_mid_transfer_then_resume` — but with the default **off**, so
  invariant I1 has zero coverage. Highest-value single test in the review.

- [ ] **`OPS-02`** · P1 · `hardening` · — · M · R7
  `crates/longtail/src/apply.rs:155` → `crates/longtail/src/fs_util.rs:134-139`
  **Fails when:** v1 ships a `0444` asset (`retain_permissions` defaults **true**), v2 changes it, and
  downsync v2 fails `EACCES` at step 5b — no partial progress, no actionable message. A modified asset
  is truncate-opened **in place**, never deleted first (`diff.rs:77` vs `:86` are disjoint sets).
  **verified-by:** none. The testkit *has* a `0444` file (`corpus.rs:259-262`) but only in the
  single-version zoo; the v1/v2/v3 chain's modified file is `0644`.
  **Note:** stock golongtail on `ChangeVersion2` fails the same way — the port's regression is the loss
  of `--use-legacy-write`, which the CLI accepts (`main.rs:133`) and the library hard-rejects.

- [ ] **`OPS-04`** · P1 · `hardening` · — · M · R7 (cluster §1.1)
  `crates/longtail/src/prune.rs:332-334` → `crates/longtail/src/fs_util.rs:455-461`
  **Fails when:** an operator ctrl-cs a slow `prune-store-index` on a multi-hundred-MB `.lsi` — the
  non-atomic `fs::write` leaves `store.lsi` a truncated prefix, and every `downsync`, `get`, and
  `validate-version` against that store fails to parse it. Recovery needs `init-remote-store`, which
  the error message does not mention.
  **verified-by:** none. Fix with `OPS-05` (no SIGINT handler on prune at all).

- [ ] **`OPS-05`** · P1 · `hardening` · — · M · R7
  `crates/longtail-cli/src/main.rs:525-543` and its three call sites `:627,:664,:738`; `crates/longtail/src/clonestore.rs:107`
  **Fails when:** ctrl-c during version 3 of a 10-version `clone-store` kills the process mid
  `write_content` — blocks written, index unflushed (`clonestore.rs:187`), a half-materialized tree on
  disk. `put`, `prune-store*` and `cp` have no handler either, and `clone_store` builds a
  `CancellationToken` **nothing ever cancels**.
  **verified-by:** none; exit code 130 has zero coverage on either path (`OPS-16`).

- [ ] **`OPS-06`** · P1 · `complexity` · — · S · R7 (cluster §1.4 of the runbook, see `DOCS-02`)
  `crates/longtail-cli/src/main.rs:27-44,46-82`; oracle `target/review-evidence/14-golongtail-help.txt:11-26,38-96`
  **Fails when:** any existing pipeline step spelled `longtail stats`, `validate`, or `init`, or one
  passing `--show-store-stats` or `--log-file-path`, dies at clap parse with exit 2 — at the *first*
  invocation after switchover, not gradually. 9 aliases, the `version` subcommand, and 8 global flags
  are absent; `grep -c alias main.rs` → **0**.
  **verified-by:** none; `commands_spec.rs` exercises only the canonical names.

- [x] **`OPS-07`** · P1 · `correctness` · — · S · R7 (cluster §1.5, fix = `API-02`)
  **FIXED 08ee171.** Both readers take `&S3OptionsArg`; all 8 call sites thread it. **verified-by:** `commands_spec.rs::s3_endpoint_flag_reaches_the_inspection_commands`, which asserts the connection targeted the configured endpoint (loopback:1) rather than merely that the command failed.
  `crates/longtail/src/inspect.rs:25-27,88-90`
  **Fails when:** a studio on MinIO runs `validate-version … --s3-endpoint-resolver-uri
  http://minio.internal:9000` — the store goes to MinIO, the `.lvi` read at `inspect.rs:55` goes to
  public AWS, and the command fails with an AWS error while demonstrably pointed at MinIO. 8
  subcommands affected; 4 of them honour the flag for the store but not the index.
  **verified-by:** none. 8 call sites independently confirmed.

- [ ] **`OPS-08`** · P1 · `security` · — · S · R7 · **PLAUSIBLE** (§10 EXP-04)
  `crates/longtail/src/apply.rs:8-18,146-158,309-345`
  **Fails when:** a Linux-authored `.lvi` holds `Content/pak0.pak` (900 MB) and `content/pak0.pak`
  (12 MB); on NTFS step 5b creates then re-truncates one file and concurrent block tasks `pwrite`
  both assets' chunks over the same range — one file of interleaved content, exit 0. Defeats the
  range-disjointness premise `apply.rs:8` states.
  **verified-by:** none. Severity depends on EXP-04.

- [ ] **`OPS-09`** · P1 · `security` · — · S · R7 · **PLAUSIBLE** (§10 EXP-05)
  `crates/longtail/src/fs_util.rs:127-145,109-112,239-269`
  **Fails when:** an asset named `nul.txt` (legal on ext4) opens the Windows NUL device — every write
  succeeds, all bytes are discarded, `assets_written` counts it, exit 0, and the file the game needs
  does not exist. `saves:autosave` lands in an NTFS alternate data stream instead.
  **verified-by:** none. Severity depends on EXP-05.

- [ ] **`OPS-10`** · P1 · `hardening` · — · M · R7
  `crates/longtail/src/fs_util.rs:140-143`; `crates/longtail/src/apply.rs:89` vs `:186-246`
  **Fails when:** a 60 GB install is updated on a volume with 8 GB free and a 12 GB net need —
  deletes-first removes the v1-only assets, sparse `set_len` preallocation *succeeds*, then `ENOSPC`
  lands mid-write. The target is neither v1 nor v2, the disk is full, and re-running reproduces it.
  **verified-by:** none. **Fix:** a free-space preflight before the deletes.

- [ ] **`OPS-14`** · P1 · `correctness` · — · S · R7
  `crates/longtail/src/clonestore.rs:191,195,197`
  **Fails when:** a `--target-paths` entry does not contain `.lvi` (e.g.
  `s3://backup/versions/game-v7.index`) — `str::replace` returns it unchanged, so
  `--create-version-local-store-index` writes the `.lsi` **over** the version index written four lines
  earlier, and reports success. A path like `archive.lvi.d/v1.lvi` has both occurrences replaced.
  **verified-by:** none; `commands_spec.rs:1160-1213` never passes the flag.
  **Fix:** `strip_suffix(".lvi")` + assert `lsi_path != target_lvi`. See §11 for whether this is
  inherited.

- [ ] **`API-01`** · P1 · `hardening` · — · S · R5
  `crates/longtail/src/options.rs:19-79,178-205,211-240`, `crates/longtail/src/error.rs:20`, `crates/longtail-core/src/error.rs:16`
  **Fails when:** the launcher pins the crate and any later added option, report field, or error
  variant becomes semver-major — HEAD itself just added one (`S3Options::stalled_stream_protection`,
  `456274d`); separately, a downstream literal construction of `DownsyncOptions` breaks the day any
  other crate in its graph enables `s3`, because the `pub s3_options` field is cfg-gated.
  **verified-by:** none, and `18-semver.txt` shows semver-checks is inert (no baseline tag).
  **Do it now:** zero `#[non_exhaustive]` exist in the workspace and the break is free before the
  first tag.

- [x] **`API-02`** · P1 · `idiom` · — · S · R5 (cluster §1.5)
  **FIXED 08ee171.** Break taken now, before any tag. A wrapper options struct was rejected: `S3Options` is already the extensible type, so the indirection buys nothing.
  `crates/longtail/src/inspect.rs:25-28,88-91`, re-exported `crates/longtail/src/lib.rs:59-64`
  **Fails when:** — this is `OPS-07`'s fix. The two URI-taking readers are the only public fns without
  S3 options, and their optionlessness also strands 3 internal callers.
  **verified-by:** none. **Take the break now**, as a `#[non_exhaustive] ReadUriOptions` so the
  signature never breaks again.

- [ ] **`API-11`** · P1 · `hardening` · — · M · R5 (cluster §1.6, with `STORE-04`)
  promise at `crates/longtail/src/lib.rs:82-85`; breaks at `crates/longtail-store/src/remote.rs:490` and `crates/longtail/src/apply.rs:358`
  **Fails when:** the launcher shows "network problem — retrying" for expired credentials and "disk
  error" for a code-bug panic; the user follows the wrong remedy in both cases.
  **verified-by:** none — the advertised contract has no API and no test.
  **Fix:** `#[non_exhaustive] enum ErrorClass` + `LongtailError::class()`, landed with `STORE-04`.

- [ ] **`PERF-01`** · P1 · `hardening` · `COMPAT-RISK` · S (to *withdraw* it) · R4 / R9 `DOCS-21`
  `docs/rust-port.md:196-200`; premises at `crates/longtail/src/downsync.rs:70-78,113-126,174-176`
  **Fails when:** an engineer implements the roadmap item — a cancelled downsync's torn files have
  *exactly* the desired size (step 5b preallocated them) and a fresh mtime, so a size+mtime
  short-circuit skips **precisely the corrupt files** and reports success. Silent, permanent, and
  self-perpetuating.
  **Additionally unimplementable as written:** `VersionIndex` has **no timestamp field**, and neither
  the pinned C source nor golongtail at `49a20e1` contains any `mtime`/`ModTime` reference — the
  parenthetical's prior art does not exist.
  **verified-by:** `OPS-01`'s missing default-config resume test is the gate that must exist *before*
  anyone attempts this. **Action:** rewrite or withdraw the roadmap item; do not implement it.

- [x] **`ALG-01`** · P1 · `hardening` · `COMPAT-RISK!` · M · R2 (cluster §1.4)
  **FIXED 84fe522 — with a correction to the finding.** Four files plus `mixed_writer` un-gated (~57 tests now build on both platforms, incl. the 39-test CLI suite); tree-manifest compares mask *only* the mode check on Windows. **`lvi_byte_gate` and `upsync_byte_gate` cannot be un-gated:** they assert a byte-identical `.lvi`, which embeds POSIX permission bits that Windows synthesizes (format-spec §7), so byte equality against umask-generated fixtures is unreachable there by construction. Both now state that next to the gate, as does `s3_interop`. So "Windows runs 3 of 8 gates" is partly a property of the artifacts, not a closable gap. **Caveat:** verified on Linux only — cross-compiling needs a C toolchain for aws-lc-sys/blake3/zstd-sys that is unavailable locally, so CI is the first real run.
  `crates/longtail/tests/lvi_byte_gate.rs:7`, `upsync_byte_gate.rs:21`, `{smoke,downsync_e2e,deadlock_regression}.rs`, `crates/longtail-cli/tests/commands_spec.rs:10`
  **Fails when:** a Windows-only regression in the scan/permission/path layer ships — e.g. a backslash
  reaching `relative_path` (0x5C vs 0x2F) reorders assets and changes `m_NameData`, `m_NameOffsets`,
  and every asset's chunk mapping: a total `.lvi` incompatibility. Nothing in the Windows per-PR lane
  looks at a `.lvi` built from a folder scan, and the Tauri download path is the primary Windows
  consumer.
  **verified-by:** the Windows lane runs 3 of the 8 documented compat gates. **This is the one item
  whose priority hinges on a product answer** — see §11.

- [x] **`CI-01`** · P1 · `hardening` · — · S (lane) · R8 (cluster §1.4)
  **FIXED 8fb846e (CI half).** Windows now runs Windows-specific clippy (the `cfg(windows)` paths in `fs_util`/`blob/fs` were linted by nobody), gets bench-compile parity, and asserts a test floor so a lane compiling almost nothing cannot report success (Linux 242, of which 62 are `cfg(unix)`-only → Windows ≈180; floor set slack at 100 until a green run reports the real number). **The code half is still open:** the six `#![cfg(unix)]` integration files must lose those gates before Windows actually runs the byte gates. Tracked as ALG-01.
  `.github/workflows/rust.yaml:42-54`
  **Fails when:** a Windows-only regression in the facade (`fs_util.rs`'s four `cfg` forks, the
  `seek_write` loop, permissions synthesis) merges green, ships in the Tauri build, and is first
  observed by a player. `pure-windows` runs build + test + verify-fixtures only.
  **verified-by:** none. **Cheapest control:** a per-lane expected-test-count assertion — it would
  have caught `ALG-01` the day it happened.

- [x] **`CI-03`** · P1 · `hardening` · — · S · R8
  **FIXED 8fb846e.** The S3 lanes set `LONGTAIL_TEST_S3_REQUIRED`, and both env-gate helpers turn a missing endpoint into a hard failure when it is set — while staying an inert skip everywhere else. Verified in both directions locally: default run skips green, `REQUIRED=1` without an endpoint fails loudly in `s3_spec` and in gate ⑧'s `s3_interop`.
  `.github/workflows/s3-minio.yaml:35-39,62,108`; `crates/longtail-store/tests/s3_spec.rs:160-163,188-191`
  **Fails when:** an env-var rename, a job-level `env:` typo, or a nextest filter change turns the
  weekly S3 lane permanently green-while-empty — the gated tests `return` rather than fail, so nextest
  records **PASS** (`03-test.txt:280-281`: `s3_spec` PASS in 0.004 s with no endpoint). `blob/s3.rs`
  is at 24.49% region and this lane is its only network exercise.
  **verified-by:** the skip itself records PASS — that *is* the finding.
  **Fix:** `LONGTAIL_TEST_S3_REQUIRED` → panic instead of return, set by the workflow.

- [ ] **`CI-06`** · P1 · `hardening` · — · M · R8
  no `--release` in any workflow (grep: 0 hits); no `[profile.release]` in the root `Cargo.toml`; `docs/switchover-checklist.md:13`
  **Fails when:** a release-only misbehaviour ships — concretely `OPS-12`'s `debug_assert_eq!`, which
  is compiled out in release and lets a block-index/payload mismatch corrupt adjacent ranges instead
  of panicking. The tested profile is not the shipped profile, and the checklist has the operator
  hand-build the shipped binary with no CI provenance.
  **verified-by:** none — the release profile has never been tested. §10 EXP-02 decides whether the
  byte gates even pass under `--release` today; a failure escalates this to P0.

### 3.3 P1 — documentation blockers

These four are P1 because the artifacts are executed or relied on at switchover. Bodies are in R9;
they also appear in §6.

- [ ] **`DOCS-01`** · P1 · `complexity` · — · M · R9 — `docs/switchover-checklist.md:23-44`.
  Executing the four-keeper plan deletes the only written description of **32 of 53** CLI flags,
  with the CI/CD pipeline as a named consumer. **Retiring `switchover-checklist.md` has this as a
  hard prerequisite.**
- [ ] **`DOCS-02`** · P1 · `hardening` · — · M · R9 — `docs/switchover-checklist.md:9,18,25-26,44`.
  The runbook a human follows to flip production is wrong in four independent ways; its
  flag-mapping table is what a pipeline author edits YAML from, and following it produces exit 2 on
  any step that used an alias (`OPS-06`). Its §Sign-off table is empty, so it has not been executed.
- [ ] **`DOCS-03`** · P1 · `hardening` · — · S · R9 — `docs/format-spec.md:3-10`.
  The authority for the paramount constraint cites a machine-local tree and points validation at a
  dead constant, so a stale citation cannot be distinguished from a moved line number.
- [ ] **`DOCS-04`** · P1 · `hardening` · — · S · R9 — `.github/workflows/` (absence).
  Nothing runs `cargo doc`; 7 live rustdoc defects (the evidence pack's 4 under-reports), three of
  them in the facade/store API a Tauri consumer reads. Defect count only goes up.
- [ ] **`DOCS-21`** · P1 · `complexity` · — · S · R9 / R4 `PERF-01` — `docs/rust-port.md:196-200`.
  The flagship roadmap item in the document `readme.md` and `CLAUDE.md` both name as the starting
  point is half-shipped, measured in a non-shipping config, and unimplementable as written.

---

## 4 · P2 — first maintenance window

82 items. Open the owning document for the evidence and recommendation. `Src` is the reviewer
number; the finding ID prefix already identifies it, so `Src` is given only where a finding is
co-owned.

### 4.1 Formats and codecs (R1, R2)

| ✔ | ID | Dim | One line | Anchor | Compat |
|---|---|---|---|---|---|
| [ ] | `FMT-004` | hardening | `to_bytes` on a struct with mismatched array lengths emits a parseable, silently-shifted `.lvi` | `version_index.rs:224` | RISK |
| [ ] | `FMT-005` | complexity | asset paths must be UTF-8 to be materialized — undocumented divergence; the format stores raw bytes | `version_index.rs:110` | RISK |
| [ ] | `FMT-006` | idiom | `Permissions::contains` documents "any bits set", implements "all bits set"; zero call sites | `perms.rs:41-45` | — |
| [ ] | `FMT-007` | hardening | zero fuzz targets over the only attacker-controlled input surface in the workspace | (absent) | — |
| [ ] | `FMT-008` | hardening | no golongtail-produced empty-index fixture: §9's first edge case is verified only against our own writer | `fixtures/` | RISK |
| [ ] | `ALG-03` | hardening | nothing checks a decoded payload against Σ`chunk_sizes` though `decode_stored_block`'s docs claim it does | `compress.rs:347` | RISK |
| [ ] | `ALG-04` | hardening | zero-valued `target_chunk_size`/`max_block_size`/`max_chunks_per_block` accepted where C returns `EINVAL` | `build.rs:39` | — |
| [ ] | `ALG-05` | hardening | the fixture-integrity gate (`Manifest::verify`) has no test proving it can fail | `fixture_manifest.rs:98` | — |
| [ ] | `ALG-06` | hardening | the per-PR boundary golden bypasses `HpcdcChunker::from_target`; the testkit keeps a second copy of the derivation | `boundary.rs:54` | RISK |
| [ ] | `ALG-07` | hardening | corpus determinism is only self-checked in-process; no KAT pins the generated bytes | `corpus.rs:346` | — |
| [ ] | `ALG-08` | hardening | `per_asset` ↔ `file_infos` alignment rests on a sort duplicated in two crates | `version.rs:38` | RISK |
| [ ] | `ALG-09` | hardening | `create_missing_content` emits duplicate chunk hashes where C dedups; reachable via `clone-store` | `pack.rs:148` | RISK |
| [ ] | `ALG-10` | hardening | the differential oracle is the **v0.3.3-era submodule**, not the v0.4.3 prebuilt the docs describe | `longtail-sys/Cargo.toml:23` | — |
| [ ] | `ALG-11` | hardening | range-split arithmetic (byte-gate-critical) has no direct test; covered only implicitly, unix-only | `build.rs:42` | RISK |
| [ ] | `ALG-12` | hardening | `store_index_block_hashes_recompute` can pass having verified zero blocks | `hash_recompute_golden.rs:105` | — |
| [ ] | `ALG-13` | hardening | gate ⑥ (`upsync_interop`) returns green doing nothing when the golongtail binary is absent | `upsync_interop.rs:47` | — |
| [ ] | `ALG-14` | hardening | `create_store_index` panics on mismatched input slice lengths while returning `Result` | `pack.rs:77` | — |

### 4.2 Store and concurrency (R3)

| ✔ | ID | Dim | One line | Anchor |
|---|---|---|---|---|
| [x] | `STORE-06` | hardening | **FIXED 1fb6be6** — the parse now sits *inside* the retry ladder, so a torn read from a concurrent golongtail writer is retried like a transport error. verified-by `mixed_writer.rs::torn_store_index_read_is_retried_not_fatal`, plus `permanently_corrupt_store_index_still_fails` so the retry cannot become a hang (fails after ~3.85 s with the format error). **Scope note:** the torn-write mechanism is `fsstore.go`'s in-place `ioutil.WriteFile`, i.e. **fs stores** — S3 `PutObject` is atomic, so this arm does not apply to an S3-shared store; the fix still helps there against transport-level truncation. | `sync.rs:145-149` |
| [ ] | `STORE-07` | hardening | Windows mixed Rust+Go fs writers do not mutually exclude (`LockFileEx` vs `CreateFile`) → ACKed lost update | `blob/fs.rs:17-21` |
| [ ] | `STORE-08` | perf | `preflight_get` holds the prefetch mutex across the whole enqueue loop and spawns one detached task per block | `remote.rs:511-530` |
| [ ] | `STORE-09` | hardening | a dropped `get_stored_block` leaks its map entry and any held worker permit — latent, unreachable today | `remote.rs:485-486` |
| [ ] | `STORE-10` | memory | the cache byte budget is enforced only in `close()`, which the cancel/error path never reaches | `cache.rs:163` |
| [ ] | `STORE-11` | hardening | a payload-truncated cache entry is served as a hit; for a compressed store the get then fails with no fall-through | `cache.rs:94-98` |
| [ ] | `STORE-12` | hardening | no `catch_unwind`, no rayon `panic_handler` anywhere → a codec panic aborts the process | `compress.rs:43-47` |
| [ ] | `STORE-13` | hardening | a caller-supplied `Client` silently discards `force_path_style` / accelerate / stalled-stream opt-out | `blob/s3.rs:196-198` |
| [ ] | `STORE-14` | hardening | cancellation is polled-only; two apply loops have no checkpoint at all | `apply.rs:147-158,267-280` |
| [ ] | `STORE-15` | hardening | `clone-store` builds a token nothing cancels; `put` installs no handler | `clonestore.rs:107`, `main.rs:627/664/738` |
| [ ] | `STORE-16` | hardening | the owner's fallback persist discards its error and is unreachable from the `*_blocking` wrappers | `remote.rs:656-662` |
| [ ] | `STORE-17` | perf | a worker permit is held across the full 6-rung read ladder including ≈3.85 s of sleeps | `remote.rs:339` + `sync.rs:80-92` |

### 4.3 Performance and memory (R4)

| ✔ | ID | Dim | One line | Anchor |
|---|---|---|---|---|
| [ ] | `PERF-02` | memory | scan peak is `worker_count × min(largest_asset, target_chunk_size × 1024)`; both inputs uncapped, one from the source `.lvi` | `version.rs:42,107-115` |
| [ ] | `PERF-03` | memory | `prune_blocks` still clones the whole union store index — the one `base.clone()` the other fixes left behind | `remote.rs:557` → `:757` |
| [ ] | `PERF-04` | perf | freeing the union after `GetExistingContent` buys peak by re-downloading and re-parsing every shard at flush, per CAS attempt | `remote.rs:701-709` |
| [ ] | `PERF-05` | memory | the GET path is `Vec<u8>` end-to-end: 2× wire size per fetch and per cache hit, 3× per miss; `Bytes` landed write-side only | `block.rs:134`, `cache.rs:96,120` |
| [ ] | `PERF-06` | complexity | the `chunk_asset` range-split contract is documented on a fn with **zero** production callers; the live path is a second copy | `build.rs:24-25`, `version.rs:98` |
| [ ] | `PERF-07` | perf | the e2e harness does not re-run `prep()` before a retry, so a retried "cold" run measures a warm target | `e2e.rs:139-153,557-568` |
| [ ] | `PERF-08` | perf | no `[profile.release]`: 883,401 B of duplicate symbols + ≈6.3 MiB of strippable symbol tables in a Tauri-embedded binary | `Cargo.toml:1-13` |
| [ ] | `PERF-09` | perf | the harness reports medians of n=5 with no dispersion; an all-failing cell prints `NaN` and looks like a good cell | `e2e.rs:521-532` |
| [ ] | `PERF-10` | perf | `write_content` allocates each block payload with `Vec::new()` although the exact size is computed two lines above | `upsync.rs:236,254,279` |
| [ ] | `PERF-11` | memory | download peak has three unreconciled budgets; the only byte-denominated one is a fixed 512 MiB with no knob | `remote.rs:70`, `apply.rs:182` |
| [ ] | `PERF-12` | perf | CI never builds `--release` and no workflow has cargo caching; every job recompiles the full AWS SDK tree | `.github/workflows/rust.yaml` |

### 4.4 API and idioms (R5)

| ✔ | ID | Dim | One line | Anchor |
|---|---|---|---|---|
| [ ] | `API-03` | hardening | crates are publishable-by-default yet unpublishable (path deps carry no `version`); no tag, so semver-checks is inert | `crates/longtail/Cargo.toml:12-13` |
| [ ] | `API-04` | idiom | unnameable public surface: four `DEFAULT_*` upsync consts back every constructor; `S3OptionsArg` types 9 pub fields yet is private | `upsync.rs:31-36` |
| [ ] | `API-05` | hardening | `CloneStoreOptions::source_zip_paths` (and the CLI flag) is accepted and silently ignored | `clonestore.rs:36`, `main.rs:893-894` |
| [x] | `API-06` | hardening | `put` writes `s3-endpoint-resolver-uri` into the get-config JSON; `get` never reads it back | `put.rs:168-174`, `get.rs:42-83` |
| [ ] | `API-07` | complexity | CLI S3/progress/cancel wiring copy-pasted 12-13×; the newest S3 knob reached 4 of 12 subcommands | `main.rs:619-626` vs `:730-737` |
| [ ] | `API-08` | complexity | the re-upload pipeline is duplicated between `upsync` and `clone_store`, diverging exactly where two filed bugs live | `upsync.rs:116-176`, `clonestore.rs:151-198` |
| [ ] | `API-12` | hardening | the library emits zero telemetry outside 4 cache-eviction events; `tracing` is an unused dep of `longtail` | `07b-machete.txt`, `downsync.rs:303` |
| [ ] | `API-13` | hardening | `downsync`/`upsync` block their polling tokio thread through `pool.install` for the whole Indexing phase | `downsync.rs:117-126` |
| [ ] | `API-14` | hardening | progress callbacks fire on rayon workers and on tokio workers under a held `std::sync::Mutex`; contract undocumented | `version.rs:69`, `apply.rs:236-243` |
| [ ] | `API-15` | hardening | every rayon pool is built without a `panic_handler`, so a detached codec panic aborts the embedding GUI | `version.rs:170`, `store/compress.rs:44` |
| [ ] | `API-16` | hardening | embedder cancellation is graceful-only with unbounded latency; task abort silently skips flush/close/eviction | `lib.rs:71-81` |

### 4.5 Security (R6)

| ✔ | ID | Dim | One line | Anchor |
|---|---|---|---|---|
| [ ] | `SEC-07` | security | `get` trusts a get-config's `storage-uri`/`source-path` as URIs and silently ignores the key `put` writes | `get.rs:43-83` |
| [ ] | `SEC-08` | security | `--s3-endpoint-resolver-uri` redirects every signed S3 request, accepts plain `http://`, and logs nothing | `main.rs:620-621` |
| [ ] | `SEC-09` | hardening | `--no-stalled-stream-protection` removes the only stall guard; no operation, read, or connect timeout exists anywhere | `blob/s3.rs:234-239` |
| [ ] | `SEC-10` | security | the apply/upsync side never `lstat`s: a pre-existing symlink under `--target-path` is followed by create/truncate/chmod/remove | `fs_util.rs:134,151,195,250` |
| [ ] | `SEC-11` | hardening | no `deny.toml`; `licenses FAILED`; no `license` field on any of 9 packages and no `LICENSE` file — **see §11.1** | `16-deny.txt:11214` |
| [ ] | `SEC-12` | hardening | the audit workflow cannot gate a PR (no `pull_request` trigger) and its path filter names `audit.yml` for a `.yaml` file | `.github/workflows/audit.yaml:2-16` |

### 4.6 Operations and CLI (R7)

| ✔ | ID | Dim | One line | Anchor |
|---|---|---|---|---|
| [ ] | `OPS-11` | hardening | `.longtail.index.cache.lvi` is written non-atomically and never fsynced; a torn cache is a hard error next run | `downsync.rs:219` |
| [ ] | `OPS-12` | hardening | the chunk-size `debug_assert_eq!` is compiled out; a mismatch corrupts adjacent ranges or panics on the slice — **see `CI-06`** | `apply.rs:317,342` |
| [ ] | `OPS-13` | hardening | the stated disjointness invariant omits its own premise (unique asset paths) and nothing validates it | `apply.rs:8` |
| [ ] | `OPS-15` | hardening | `prune-store-blocks` swallows every delete error and still reports success; the three prune commands disagree on gating | `prune.rs:427`, `main.rs:832` |
| [ ] | `OPS-16` | idiom | every non-cancel error collapses to exit 1; commands with no cancel handler die by signal, so `code()` is `None`, not 130 | `main.rs:512` |
| [ ] | `OPS-17` | correctness | `assets_written` counts write-plan files before any content lands | `apply.rs:156` |
| [ ] | `OPS-18` | idiom | `--retain-permissions`, `--scan-target`, `--cache-target-index` are parsed and never read; no `conflicts_with` on the pairs | `main.rs:108,605` |
| [ ] | `OPS-19` | hardening | `path_filter.rs` is 13.7% region-covered with zero CLI tests; a leading `**` compiles an empty regex that excludes the tree | `path_filter.rs:36` |

### 4.7 CI and packaging (R8)

| ✔ | ID | Dim | One line | Anchor |
|---|---|---|---|---|
| [x] | `CI-02` | complexity | the `formatting` PR gate depends on network + a C toolchain + libclang — **for ~6 s of C build** (EXP-01: 19 s cold vs 13 s warm) | `rust.yaml:129,135` |
| [ ] | `CI-04` | hardening | none of the four PR-gating jobs sets `timeout-minutes`; a miri or proptest hang bills the 6-hour default and blocks merges | `rust.yaml:22,42,113,147` |
| [ ] | `CI-05` | hardening | every lane rides floating toolchains; no `rust-toolchain.toml`, no `rust-version` — one bad nightly bricks clippy/fmt/miri | `rust.yaml:121,157` |
| [ ] | `CI-07` | security | `mkdata.{sh,ps1}` download and execute golongtail with no checksum and no fail-fast, duplicating xtask's pinned fetcher | `mkdata.sh:9` |
| [ ] | `CI-08` | hardening | fixture-freshness gets the submodule only as a `build.rs` side effect; a C-compile failure is swallowed into `cargo:warning` | `fixture-freshness.yaml:17`, `build.rs:414-420` |
| [x] | `CI-09` | hardening | no CI cargo invocation passes `--locked`; lanes may silently re-resolve while audit/deny vouch for the committed lock | all four workflows |
| [ ] | `CI-13` | hardening | the pinned-golongtail channel is Linux-only, so gates ⑥/⑧ and the three-way's third leg are impossible on Windows — green | `xtask/main.rs:38-41` |
| [ ] | `CI-14` | hardening | the four public `*_blocking` wrappers — the documented entry point for simple callers — have no test and no CLI use | `longtail/src/lib.rs:103-145` |

### 4.8 Documentation P2 (R9)

`DOCS-05` … `DOCS-13` — tabulated with one-liners and anchors in **§6.4**.

---

## 5 · P3 — nice to have

42 items, one line each. All are `S` effort unless noted.

| ✔ | ID | One line | Anchor |
|---|---|---|---|
| [ ] | `FMT-009` | `FileInfos` accessor error arms are dead/untested while `VersionIndex`' twins are tested | `file_infos.rs:104,112,120` |
| [ ] | `FMT-010` | `len() as u32` count casts in every `to_bytes`; the neighbouring offset uses `try_from` | `store_index.rs:436` vs `:432` |
| [ ] | `FMT-011` | `path_data.len() as u32` truncates silently past a 4 GiB name blob | `file_infos.rs:67` |
| [ ] | `FMT-012` | usage-percent `as u32` can truncate once the u32 size sums wrap (PLAUSIBLE) | `store_index.rs:597` |
| [ ] | `FMT-013` | the cursor's own truncation guard is unreachable from every caller and has no unit test | `cursor.rs:49-54` |
| [ ] | `FMT-014` | `FormatError::SizeOverflow` is unreachable on 64-bit; 56 `checked_*` guards load-bearing only on a target CI never builds | `cursor.rs:15-23` |
| [ ] | `FMT-015` | `validate_store` swallows a bad name blob and reports it as a size mismatch | `validate.rs:48` |
| [ ] | `FMT-016` | `Permissions::POSIX_MASK` has no call site; the one place that must mask re-declares it | `perms.rs:27` |
| [ ] | `ALG-15` | `pack.rs` duplicate-hash "keep last occurrence" fidelity branch is untested | `pack.rs:44` |
| [ ] | `ALG-16` | two infallible `.expect()`s in `blake2s_hash` can be made structurally impossible · `COMPAT-RISK` | `hash.rs:105` |
| [ ] | `ALG-17` | scan holds one `max_hash_size` (32 MiB default) buffer per rayon worker plus a full `entries` clone (M) | `version.rs:115` |
| [ ] | `ALG-18` | `chunker_golden.rs` computes `expect_mode` and discards it — an assertion that isn't one | `chunker_golden.rs:42` |
| [ ] | `ALG-19` | unlisted codec-family low bytes encode at silent fallback params; both fallback arms uncovered | `compress.rs:175` |
| [ ] | `ALG-20` | `merge_version_index`'s `hash_identifier`-mismatch arm is the only uncovered branch in `build.rs` | `build.rs:233` |
| [ ] | `STORE-18` | three `index.as_ref().unwrap()` encode a sound invariant the compiler could enforce instead | `remote.rs:755,780,816` |
| [ ] | `STORE-19` | 14 `as` casts, zero `checked_*`/`try_from` in the crate; two have reachable truncation | `remote.rs:519`, `blob/s3.rs:299` |
| [ ] | `STORE-20` | four unreachable error arms, because no `Semaphore` in the crate is ever closed | `remote.rs:288,339,386` |
| [ ] | `PERF-13` | the prefetch budget is denominated in decompressed bytes but bounds compressed memory — safe, undocumented, under-uses by `1/ratio` | `remote.rs:519` |
| [ ] | `PERF-14` | two full copies of the compressed body per put remain (frame prepend, `.lsb` serialize) | `compress.rs:339-342` |
| [ ] | `PERF-15` | 30 MB of committed fixtures is paid by every clone and every one of the six uncached CI jobs | `13-fixtures.txt` |
| [ ] | `API-09` | 11 verbatim cfg-gated `S3OptionsArg` bindings + 3 identical `default_s3()` fns across the op modules | `downsync.rs:92-95` |
| [ ] | `API-10` | `BlockStoreOpts` literal ×10; the 1-thread rayon pool built 5 ways with 2 error strings | `inspect.rs:56-61` vs `:78-85` |
| [ ] | `API-17` | two parallel stringly-typed phase vocabularies, both undocumented; a GUI must match magic strings | `downsync.rs:104-214` |
| [ ] | `API-18` | 16 user-facing io-error contexts format paths with `{:?}` — doubled backslashes in every Windows error | `apply.rs:81`, `fs_util.rs:105` |
| [ ] | `API-19` | `Instant::now() - Duration::from_secs(1)` panics when the process starts <1 s after boot; runs every scan | `version.rs:141` |
| [ ] | `API-20` | `discriminator_from_avg` lacks `#[allow(clippy::suboptimal_flops)]`; an applied `--fix` fuses the FMA and moves every chunk boundary · **`COMPAT-RISK!`** | `core/chunker.rs:242-245` |
| [ ] | `SEC-13` | `longtail-sys/build.rs` skips SHA256 verification whenever the zip is already on disk | `build.rs:88-91` |
| [ ] | `SEC-14` | vendor-default minio credentials inline in a workflow (values deliberately not reproduced; rotate and move to secrets) | `.github/workflows/s3-minio.yaml` |
| [ ] | `SEC-15` | cache eviction deletes *every* file under `<cache>/chunks/` regardless of extension, with no ownership marker | `cache.rs:262-283` |
| [ ] | `OPS-20` | `delete_assets`' 10-pass retry has no backoff, so it cannot ride out the transient Windows sharing violation it targets | `apply.rs:427` |
| [ ] | `CI-10` | audit.yaml watches `audit.yml` and a nonexistent `**/audit.toml`; no `pull_request` trigger, so a vulnerable dep merges | `audit.yaml:6,11` |
| [ ] | `CI-11` | publish/manifest hygiene: `publish = false` on one crate, no `license`, `longtail-ffi` 0.2.0 vs workspace 0.1.0, no MSRV | nine `Cargo.toml`s |
| [ ] | `CI-12` | Renovate is configured but unconsumed: bare `config:recommended`, 10 stale branches including `crate-tokio-vulnerability` | `renovate.json` |
| [ ] | `CI-15` | `cargo doc` runs nowhere; one of the four defects is the CLI's `[[bin]] name = "longtail"` colliding with the lib crate | `longtail-cli/Cargo.toml:7-9` |
| [ ] | `CI-16` | `uri.rs` scheme dispatch — the CLI's front door — is 56.83% region; gs/abfs rejections and the drive-letter path untested | `uri.rs:185-225` |

Plus `DOCS-14` … `DOCS-20`, tabulated in **§6.4**.

---

## 6 · Documentation and comments

58 findings across all nine documents, plus R9's 21 markdown findings. Four are switchover
blockers (§3.3); five more are P1 and listed first here.

### 6.1 P1 documentation findings not in §3.3

| ✔ | ID | Src | One line | Anchor |
|---|---|---|---|---|
| [ ] | `SEC-DOC-02` | R6 | **no keeper doc states the trust boundary** — the premise that makes `SEC-01` a P0 rather than a curiosity, and the correction to the intuition `readme.md`'s "content-addressed" framing creates | (absent) |
| [ ] | `STORE-DOC-01` | R3 | the Windows mixed-writer divergence (`STORE-07`) is missing from §Deliberate divergences | `docs/rust-port.md:111-155` |
| [ ] | `STORE-DOC-02` | R3 | `remote.rs:560-562` claims prune can *never* leave dangling index entries — `STORE-01` is exactly that case | `remote.rs:560-562` |
| [ ] | `STORE-DOC-03` | R3 | `lib.rs:82-85` promises error classes the block path destroys (`STORE-04`/`API-11`) | `longtail/src/lib.rs:82-85` |
| [ ] | `OPS-DOC-02` | R7 | the roadmap proposes an optimization that breaks an unwritten invariant (`PERF-01`/`DOCS-21`) | `docs/rust-port.md:196-200` |

### 6.2 Claims in the four keeper docs that this review proves wrong

The highest-value documentation output of the batch. Every row is a false statement in a document
that survives long-term.

| ✔ | Claim | Where | Disproved by |
|---|---|---|---|
| [ ] | "No C library is built or linked in a normal build" | `readme.md:7` | **false for the CI path** — `zstd-sys` is compiled and contributes 321.1 KiB to the shipped binary, and the PR-gating clippy compiles the whole vendored C library (EXP-01). `ALG-DOC-02`, `CI-02` |
| [ ] | "Every default-member library and binary target is `#![forbid(unsafe_code)]`" | `CLAUDE.md:101` | a false universal; `docs/rust-port.md:208-223` states it correctly. `DOCS-05`, `SEC-DOC-01` — **R6's inventory is authoritative** |
| [ ] | "malformed input never panics" | `longtail-core/src/error.rs:4`, `lib.rs:40-42` | `FMT-001`, `FMT-002`, `ALG-02`, `SEC-05`, `diff.rs:129-133`. `FMT-DOC-02`, `SEC-DOC-03` |
| [ ] | "Logging is `tracing`-based" | `CLAUDE.md` §Runtime configuration | the facade emits none; `tracing` is an unused dep. `API-DOC-02`, `OPS-DOC-03`, `API-12` |
| [ ] | `build.rs` "downloads a pinned prebuilt native library" | `CLAUDE.md`, 3 more files | `default = ["vendored"]` compiles from the submodule; the download path and its four SHA256 pins are dead. `ALG-DOC-01`, `DOCS-06`, `ALG-10` |
| [ ] | the eight-gate table's cadence and OS coverage | `docs/rust-port.md:84-96` | overstates what runs per PR; §10.3 claims a check that cannot run. `ALG-DOC-03`, `DOCS-07` |
| [ ] | the roadmap's "biggest win" | `docs/rust-port.md:196-200` | half-shipped, non-shipping measurement config, unimplementable as written. `DOCS-21`, `PERF-01` |
| [ ] | the `archive` cargo feature | `docs/rust-port.md:189` + 2 docs + 1 test | declared in no `Cargo.toml`. `DOCS-10` |
| [ ] | the spec's provenance block | `docs/format-spec.md:3-10` | machine-local tree, dead constant. `DOCS-03` |
| [ ] | `CLAUDE.md`'s lint command | `CLAUDE.md:69` vs `:46-48` | needs a C toolchain 20 lines after saying none is needed. `DOCS-17`, and `CI-02` is why |

### 6.3 All remaining documentation findings

Grouped by owner; all `S` effort unless noted. Anchors are in the owning document.

**R1 · `FMT-DOC-01`** (P2) merge byte-identity contract attached to a private helper `store_index.rs:189-227` ·
**`FMT-DOC-03`** (P2) spec §2 names an fs lock file the port deliberately does not implement `format-spec.md:202-205` ·
**`FMT-DOC-04`** (P2) store-index block order is non-deterministic; that fact lives only in a CI comment ·
**`FMT-DOC-05`** (P3) §9's misalignment bullet points at the wrong counts ·
**`FMT-DOC-06`** (P3) §3 documents C's temp-file shape as if it were part of the format ·
**`FMT-DOC-07`** (P2) the `VersionIndex` invariant doc promises more than `from_bytes` delivers `version_index.rs:30-34` ·
**`FMT-DOC-08`** (P2) `rust-port.md`'s strictness claim is incomplete in a way that matters ·
**`FMT-DOC-09`** (P3) `.lsb` has no version field and the spec does not draw the consequence.

**R2 · `ALG-DOC-04`** (P2) spec §6 misses two upstream facts R2 proved `format-spec.md:353-356,493-521` ·
**`ALG-DOC-05`** (P2) a new §Upstream-findings entry: `DiffHashes`' reorder loop ·
**`ALG-DOC-06`** (P2) undocumented divergences from C ·
**`ALG-DOC-07`** (P3) `manifest.json`'s `produced_by` label for the sharded cell is misleading ·
**`ALG-DOC-08`** (P3) `crates/` comments cite the legacy crates slated for deletion.

**R3 · `STORE-DOC-04`** (P2) `s3.rs:231-234` "Authoritative regardless of any inherited sdk_config setting" — `STORE-13` contradicts it ·
**`STORE-DOC-05`** (P2) `evict_cache_dir`'s doc claims a `.lrb` filter the code lacks (`SEC-15`) ·
**`STORE-DOC-06`** (P2) the two public URI entry points disagree about fs locking, undocumented ·
**`STORE-DOC-07`** (P3) `sync.rs:457`'s comment claims a log the module cannot emit ·
**`STORE-DOC-08`** (P2) the cache-LRU bullet omits that the budget is post-hoc and skipped on failure ·
**`STORE-DOC-09`** (P3) `blob/mod.rs:186-187` says the scheme is lowercased; it is not ·
**`STORE-DOC-10`** (P3) `remote.rs`'s liveness invariant is restated in five places.

**R4 · `PERF-DOC-01`** (P2) = `API-DOC-01`, `FMT-DOC-01` — same defect, three reviewers ·
**`PERF-DOC-02`** (P2) `put-path-memory.md` contradicts its own Resolution section and pins a stale self-SHA (= `DOCS-08`) ·
**`PERF-DOC-03`** (P2) the July bench doc's incremental-downsync explanation describes code that no longer exists ·
**`PERF-DOC-04`** (P2) `bench-2026-08-03.md`'s `merge_mem` result cannot be reproduced from the document ·
**`PERF-DOC-05`** (P2) its real-S3 comparison has no stated method ·
**`PERF-DOC-06`** (P2) `:103-106` overstates asset-size independence.

**R5 · `API-DOC-01`** (P2) `StoreIndex::merge`'s load-bearing doc is attached to a private fn ·
**`API-DOC-03`** (P3) runtime-model docs omit the two facts an embedder needs most (`API-13`, `API-14`) ·
**`API-DOC-04`** (P3) `delete_uri` doc describes a caller that doesn't exist ·
**`API-DOC-05`** (P2) rustdoc is broken, ungated, and the ratchet is cheap — adopt it (= `DOCS-04`) ·
**`API-DOC-06`** (P3) undocumented pub-item inventory (the ratchet's worklist).

**R6 · `SEC-DOC-01`** (P2) = `DOCS-05` ·
**`SEC-DOC-04`** (P2) the get-config key set is documented only in a source comment `get.rs:2-4`.

**R7 · `OPS-DOC-01`** (P2) golongtail's prune CAUTION is dropped and the prune flags have no help text ·
**`OPS-DOC-03`** (P2) "Logging is `tracing`-based" is not true of the facade ·
**`OPS-DOC-04`** (P2) a declared divergence from cited C is not in the divergences section ·
**`OPS-DOC-05`** (P2) the largest CLI-compat gap is undocumented and the Windows permission consequence unstated.

**R8 · `CI-DOC-01`** (P2) `fixtures/README.md`'s byte-exactness claim is false for `.lsi` and its regeneration claim omits the real prerequisite `fixtures/README.md:9-11` ·
**`CI-DOC-02`** (P3) `CLAUDE.md`'s differential-lane instructions send Windows developers down a dead path (`CI-13`) ·
**`CI-DOC-03`** (P3) `CLAUDE.md` says the `*_blocking` wrappers exist "for the CLI"; the CLI doesn't use them (`CI-14`) ·
**`CI-DOC-04`** (P3) `paths.rs`'s doc comment names the wrong cached filename.

**R9 · `DOCS-DOC-01`** (P2, M) 43 undocumented public option fields behind a module doc that mislabels the module `options.rs:1` ·
**`DOCS-DOC-02`** (P3) `compress.rs`'s 72-line module doc carries a format table that belongs in the spec ·
**`DOCS-DOC-03`** (P3) nine comments narrate the port or cite crates scheduled for deletion ·
**`DOCS-DOC-04`** (P3) nine permission-bit constants documented by a non-doc comment ·
**`DOCS-DOC-05`** (P2, M) adopt `#![warn(missing_docs)]` on the two library crates, not a workspace-wide `deny` ·
**`DOCS-DOC-06`** cross-reference only.

### 6.4 R9's markdown findings, P2 and P3

| ✔ | ID | P | One line | Anchor |
|---|---|---|---|---|
| [ ] | `DOCS-05` | P2 | the two keepers contradict each other on `forbid(unsafe_code)` — `CLAUDE.md` states a false universal (§6.2) | `CLAUDE.md:101` |
| [ ] | `DOCS-06` | P2 | the false "downloads a prebuilt library" build story is in three more files nobody owns | `longtail-sys/README.md:3-6` |
| [ ] | `DOCS-07` | P2 | two keepers tell the verification story with two unreconciled taxonomies; §10.3 claims a check that cannot run | `format-spec.md:655-671` |
| [ ] | `DOCS-08` | P2 | `put-path-memory.md` contradicts its own Resolution section and pins a stale self-SHA (= `PERF-DOC-02`) | `put-path-memory.md:8-9,213-216` |
| [ ] | `DOCS-09` | P2 | the newest and most decision-relevant measurements are unreachable from every keeper | `rust-port.md:7-8,109` |
| [ ] | `DOCS-10` | P2 | an `archive` cargo feature is named in 3 docs and 1 test and declared in no `Cargo.toml` | `rust-port.md:189` |
| [ ] | `DOCS-11` | P2 | the four `LONGTAIL_TEST_S3_*` vars gating the S3 lane are documented in no markdown and silently default — pairs with `CI-03` | `s3_spec.rs:131-137` |
| [ ] | `DOCS-12` | P2 | `.lrb` is a compat-bearing on-disk layout absent from the authoritative format spec | `format-spec.md:212-285` (gap) |
| [ ] | `DOCS-13` | P2 | the switchover-runbook rollback repeats a safety claim R3 proved false | `switchover-checklist.md:203-206` |
| [ ] | `DOCS-14` | P3 | `CLAUDE.md`'s `docs/` listing omits two entries | `CLAUDE.md:42` |
| [ ] | `DOCS-15` | P3 | the `s3` and `vendored` cargo features are documented nowhere | `longtail-store/Cargo.toml:7-16` |
| [ ] | `DOCS-16` | P3 | `rust-port.md` gives two different HPCDC ceilings with swapped causes | `rust-port.md:138`, `:169` |
| [ ] | `DOCS-17` | P3 | `CLAUDE.md` recommends a lint command needing a C toolchain 20 lines after saying none is needed — and `CI-02` is why | `CLAUDE.md:69` vs `:46-48` |
| [ ] | `DOCS-18` | P3 | the code hardcodes the uppercase `README.md` form, so a future lowercase one under a subdirectory is silently missed (the *root*-casing half is disproven — §8) | `readme.md` vs `fixtures/README.md` |
| [ ] | `DOCS-19` | P3 | `rust-port.md:105` forward-references a section that does not exist | `rust-port.md:105` |
| [ ] | `DOCS-20` | P3 | the July bench doc's verdict heading still says FAIL 158 lines after the doc supersedes it | `bench-2026-07-05.md:328` |

---

## 7 · Hardening backlog

Aggregated and re-ranked across all nine documents by *risk closed per unit of effort*. Items are
the tests, gates, and assertions that make the findings above unrepeatable. Several close multiple
findings at once — those come first.

| ✔ | # | Item | Closes | Effort |
|---|---|---|---|---|
| [ ] | H1 | **`safe_join` + a proptest** (`Ok(p) ⇒ p.starts_with(root)`) + the two-run delete integration test | `SEC-01`, `SEC-02` — both P0s | S |
| [ ] | H2 | **Empty-keep-set refusal + test** | `OPS-03` P0 | S |
| [ ] | H3 | **A `FailingBlobStore` decorator** parameterised by (operation, key pattern, error) — nothing in the workspace injects ENOSPC, EACCES, or a non-`NotFound` error anywhere | `STORE-01` `STORE-02` `STORE-04` `STORE-05` `STORE-11` (5 P1s) | M |
| [ ] | H4 | **The shared `VersionIndex::asset_chunks` accessor** — replaces seven copies of an unguarded loop and makes the `with_capacity` safe by construction | `FMT-001` `SEC-05` + 5 latent sites | S |
| [ ] | H5 | **Per-lane expected-test-count assertion** (nextest count vs a committed expectation, per OS) — the cheapest control against coverage evaporating via `cfg`/feature gates | would have caught `ALG-01`; guards `CI-01` | S |
| [ ] | H6 | **Anti-skip guard** (`LONGTAIL_TEST_S3_REQUIRED` → panic, not return), same pattern for the golongtail-gated tests | `CI-03` `CI-13` `ALG-13` | S |
| [ ] | H7 | **Resume test under default options** (`cache_target_index` **on**) — assert the cache file is absent after a cancel and the resumed tree matches byte-for-byte | `OPS-01`; prerequisite for `PERF-01` | S |
| [ ] | H8 | **Pre-apply version-index validation**: duplicate paths, case-insensitive duplicates, Windows-illegal names. One function, testable on Linux | `OPS-08` `OPS-09` `OPS-13` | S |
| [ ] | H9 | **Rayon `panic_handler` + a panicking-codec test**, converting `store/compress.rs:47`'s `expect` to `StoreError::WorkerGone` in the same change | `SEC-06` `STORE-12` `API-15` | S |
| [ ] | H10 | **A bounded-read cap** in `BlobObject::read` + `decode_block_payload`, with a `MemBlobStore` over-cap test | `SEC-04` `ALG-02` | M |
| [ ] | H11 | **`chunk_asset` ↔ `chunk_asset_streaming` equivalence test**, then a `ChunkParams` newtype | `PERF-06` `ALG-11` `PERF-02` | S |
| [ ] | H12 | **Timeouts + pinned nightly + `--locked`** — three mechanical workflow edits removing the three largest whole-repo availability risks | `CI-04` `CI-05` `CI-09` | S |
| [ ] | H13 | **`docs` CI job** (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace`) + `doc = false` on the CLI `[[bin]]` — precondition for the `missing_docs` ratchet | `DOCS-04` `API-DOC-05` `CI-15` | S |
| [ ] | H14 | **Negative test for the fixture gate** — corrupt one byte of a scratch copy, assert `Manifest::verify` reports it and `verify-fixtures` exits non-zero. The per-PR gate has never been shown able to fail | `ALG-05` | S |
| [ ] | H15 | **Fuzz targets.** `vi_walk` (a live reproducer for `FMT-001`, extended with the containment assertion), `lsb_parse`, `lsb_decode` under `-rss_limit_mb`, `chunker_range_equivalence`, `payload_framing`. PR replay deterministic, discovery scheduled | `FMT-007` `ALG-11` `ALG-02` | M |
| [ ] | H16 | **Release-profile byte-gate run**, weekly + release-readiness | `CI-06` `OPS-12` `PERF-08` | M |
| [ ] | H17 | **Windows permission-independent `.lvi` gate + mode-masked e2e** — de-`cfg(unix)` the six test files (`TreeManifest::compare` already supports masking, `tree_manifest.rs:89`) | `ALG-01` `CI-01` | M |
| [ ] | H18 | **`ErrorClass` + minio auth round-trip test** | `API-11` `STORE-04` | M |
| [ ] | H19 | **Corpus KAT** (16 sha256 constants) and **verification-count floors** (`assert!(checked_blocks > 0)`) | `ALG-07` `ALG-12` | S |
| [ ] | H20 | **Cancellation tests** — drop an in-flight get, drop a store without `close()`, cancel during the pre-create loop. `GatedStore` (`prefetch_budget.rs:307-383`) already does the hard part | `STORE-09` `STORE-14` `STORE-16` | M |
| [ ] | H21 | **Retry-ladder exhaustion tests**, read and put. `FlakyStore` uses 3 failures against a 6-rung ladder, so nothing proves a permanently failing backend ever returns `Err` | `STORE-17` and the untested `PUT_RETRY_DELAYS` | S |
| [ ] | H22 | **Timeouts on the chaos tests** (`remotestore_spec.rs:382-406`, `blobstore_spec.rs:209`, `s3_spec.rs:183`) — copy the `GUARD` pattern from `deadlock_regression.rs:32`, which gets it right. A livelock regression currently *hangs* CI | availability | S |
| [ ] | H23 | **`path_filter.rs` table-driven test** — the worst-covered file in R7's slice (13.7%) to respectable in an hour | `OPS-19` | S |
| [ ] | H24 | **Variant-exact assertions** where tests accept any `Err` (`malformed.rs:97,114,126,216,228`, `store_algebra.rs:235`, two merge proptests) — a parser returning the *wrong* typed error passes all of them today | latent regressions | S |
| [ ] | H25 | **Proptest strategies that generate inconsistent array lengths** — `si_strategy` always produces matching lengths, which is why four `store_index.rs` arms are unreachable in the suite | 4 uncovered arms | S |
| [ ] | H26 | **`xtask check-doc-links`, `check-features`, gate-table path check** | `DOCS-09` `DOCS-10` `DOCS-14` `DOCS-15` `DOCS-07` | S |
| [ ] | H27 | **Clap help-text test** — every argument has non-empty help | `DOCS-01` `OPS-DOC-01` | S |
| [ ] | H28 | **`deny.toml` + a `cargo deny` PR gate + `pull_request` on audit.yaml** | `SEC-11` `SEC-12` `CI-10` | S |
| [ ] | H29 | **Golden-output tests for the read-only commands** (`ls`, `print-version`, `print-store`, …) — every stdout check today is a `contains(...)` on one or two labels, so column widths and numeric formats can drift silently under pipelines | `OPS` §9 | M |
| [ ] | H30 | **De-flake `concurrent_gets_coalesce_with_dispatched_prefetch`** (`prefetch_budget.rs:426`'s 50 ms sleep is load-bearing on a multi-thread runtime) — the only flake risk in the store suite | CI stability | S |
| [ ] | H31 | **`loom` over `PrefetchState`**; **miri** already passes 96 tests over `longtail-core` (`09-miri.txt`) — keep the proptest case caps that make that tractable | assurance | M |
| [ ] | H32 | **32-bit `cargo check` lane**, *or* an explicit 64-bit-only statement in `rust-port.md` | `FMT-014`; 56 `checked_*` guards | S |

---

## 8 · Disproven, reversed, and corrected

This review overturned several things. Preserving that is as valuable as the findings — each row
below is a hypothesis someone will otherwise re-form.

*In this section only, `[x]` means **settled by this review, no action required** — not "closed by a
commit". Leave these boxes checked.*

| ✔ | What was believed | What was established | Where |
|---|---|---|---|
| [x] | **Torn files break resumability** — a file left partially written by a cancelled downsync would be skipped by the next run | **Disproven.** A torn file preallocated to its full size is still classified as *content-modified* by the diff, so the next run rewrites it. Resume-after-cancel is **correct today**, and the CLI's "run the same command to resume" (`main.rs:509`) is true. What is missing is coverage, not correctness — the only resume test disables the default `cache_target_index` (`OPS-01`) | R7 Named deliverable 1 |
| [x] | **The roadmap's "biggest win" (mtime/size target scan) is the top perf opportunity** | **Reversed on both axes.** (1) Worth ~nothing on the shipping path: in the default configuration the target scan does not run at all, so the measured prize was taken in a config the product never uses. (2) **Unimplementable as written:** `VersionIndex` has no timestamp field, and neither the pinned C source nor golongtail at `49a20e1` contains any `mtime`/`ModTime` reference — the prior art the roadmap parenthetical promises does not exist. (3) Implementing it as written would *silently corrupt* resumed downloads (`PERF-01`) | R4 `PERF-01`, R9 `DOCS-21`, orchestrator-verified |
| [x] | **The shipped build carries two crypto stacks** (`ring` and `aws-lc-rs`) | **Disproven.** `grep -c 'ring v' 05-tree.txt` → **0**. `aws-lc-rs v1.17.3` is the sole provider in the default-member tree; `ring` reaches the graph only via `reqwest` ← `longtail-sys`, a build-dep of a non-default member. This was a seeded premise, and it does not hold | R6, `05-tree.txt:646,653` |
| [x] | **`fs.rs:319-325`'s `Vec::new()` + `read_to_end` pays doubling-realloc growth** (reported by *both* of R4's allocation-sweep workers) | **Rejected by the lead.** `impl Read for std::fs::File` overrides `read_to_end` to reserve from the file's metadata length first. Recorded so the next session does not re-file it | R4 §Verified good |
| [x] | **The three `remote.rs` `index.as_ref().unwrap()` sites are latent panics**, and the prefetch clamp can request more permits than the semaphore holds | **Both disproven, with proofs.** The invariant holds at all three sites (`STORE-18` is idiom, not a bug). `permits = estimate.min(max_prefetch_bytes).max(1) as u32` can never exceed the total because the budget is pre-clamped at `remote.rs:224-226`, so `acquire_many` can never be unsatisfiable — **the prefetch-deadlock bug class is closed.** It is also the only `acquire_many` in the repo. `PrefetchState`'s "never in both sets" invariant holds across all four orderings of the dispatch/demand race, and the `debug_assert!` at `:299` cannot fire | R3 §Verified good |
| [x] | **The per-PR `formatting` gate pays a full native C build measured in minutes** (this was the orchestrator's own `MANIFEST.md` caveat, and it is what sent R8 down this path) | **Cost claim WRONG; mechanism right.** EXP-01: cold total **19 s** vs 13 s warm — a **~6 s** penalty. The mechanism is real and confirmed (submodule fetch over the network, 704 `.o` objects, three static archives, bindgen/libclang), so `CI-02` **keeps its finding and changes its argument** to fragility and doc-honesty. It stays P2: a required check depending on a third party's repo, a host C compiler, and libclang is worth removing for determinism, not for speed. **The caveat that produced the wrong framing is retracted here explicitly** | `EXP-01-cold-clippy.txt`, R8 `CI-02` |
| [x] | **`STORE-02`'s orphaned blocks are permanently unreclaimable** | **Overstated.** `prune_store_blocks` (`prune.rs:408-432`) enumerates actual blobs, not the index, so a separate sweep recovers them. Corrected wording is in the finding. Kept P1 only because that recovery path carries the *same* swallow | R10 refuter |
| [x] | **`cp.rs:118-127`'s unguarded `.lsi` block→chunk walk is exploitable** | **Refuted as reachable** — the index comes from `get_existing_content` → `from_block_indexes`, canonical by construction. This confirms `SEC-05`'s own concession; the *reachable* half is `apply.rs:342` (= `FMT-002`) | R10 refuter; R6 had already self-corrected the same claim after a worker caught it |
| [x] | **`CLAUDE.md`'s `forbid(unsafe_code)` claim needs an independent recount** | R9 **corrected itself to defer to R6's inventory** as authoritative rather than publish a competing count. Both agree the `CLAUDE.md` universal is false and `rust-port.md:208-223` is right | R9 `DOCS-05` |
| [x] | **The lowercase root `readme.md` breaks a reference on a case-sensitive filesystem** (seeded concern) | **Does not hold.** No root-`README.md` casing defect exists. `DOCS-18` survives only as the narrower, real point: the code hardcodes the uppercase form, so a future lowercase README under a subdirectory would be missed | R9 `DOCS-18` |
| [x] | **No workspace crate carries a `description`**, and **`cargo bench --no-run` never runs per-PR** (two seeded leads) | **Both wrong.** `longtail-sys` and `longtail-ffi` *do* carry `description`; the pure-linux lane runs `bench --no-run` **twice** per PR. R8 also corrected two of its own workers: `verify-fixtures` gates every PR in both pure lanes (not only the scheduled freshness workflow), and the `manifest.json` `produced_by` claim | R8 §Verification performed |
| [x] | **`lz4` and `zstd` share brotli's unbounded-decompression exposure** | **Split three ways.** zstd is fully bounded (clamps via `upper_bound`); lz4 is bounded on *growth* but still allocates from the untrusted declared size; brotli is unbounded on both — `BrotliDecompress` has no limit parameter in its signature at all. R2's CONFIRMED(brotli)/PLAUSIBLE(lz4,zstd) split was right, and R6 adjudicated it correctly | R2 `ALG-02`, R6 `SEC-04`, R10 refuter |
| [x] | **`FMT-002`'s panic is caught by `flatten_apply_task`** | Same outcome, **wrong line**: the catch is the inner `spawn_blocking` join at `apply.rs:224-230`. Fix the citation when you fix the bug | R10 refuter |
| [x] | **`SEC-02`'s delete lands before any network I/O** | **Corrected:** `get_existing_content`/`preflight_get` precede `delete_assets`. The accurate claim — no weaker — is "before any byte is written to the target, and never rolled back." A *stronger* variant was also found: `--target-index-path` is the same primitive with no precondition at all | R10 refuter |
| [x] | **`OPS-02` collapses if a modified asset is deleted before rewrite** | **Resolved for the finding.** `diff.rs:77` (content-modified) and `:86` (removed) are disjoint by construction, so a modified read-only asset is truncate-opened in place. Nuance: stock golongtail fails identically on `ChangeVersion2`; the port's regression is the loss of `--use-legacy-write` | R10 refuter |

**Verified good — do not re-review.** R3's `.gen` bump ordering (matches Go exactly, and the inverse
window does not exist), the cache-eviction/concurrent-read TOCTOU (recovers cleanly, and
`decorators_integration.rs:57-91` regression-tests it), the `.lrb` cache layout (C-compatible, so an
existing warm cache is read directly), and **all four on-store layout artefacts byte-identical to
golongtail `49a20e1`** (shard name, shard discovery filter, `.gen` encoding, `._lck` naming).
R6's Appendix A: **zero of the 19 production `unwrap`/`expect` sites is reachable from untrusted
input** — the panic risk lives in unchecked slice indexing, not in `unwrap`s. R6 also found the
path-filter regexes are not a DoS vector, and the supply chain is clean today (0 advisories over
411 deps).

---

## 9 · Landing sequence

Ordered so that no change lands before the gate that would catch its mistake. **Byte-compatibility
outranks everything: an item whose fix carries `COMPAT-RISK` must not land before its gate exists.**

### Wave 0 — land alone, today, no dependencies

1. **`OPS-03`** — the empty-keep-set refusal. Three lines, one test, removes the only
   whole-store-destruction path. Do not batch it with anything.
2. **`H1` `SEC-01`+`SEC-02`** — `safe_join` over the seven `fs_util` sites *and* `remove_asset`,
   plus the acceptance check on both index-injection routes (the cache file and
   `--target-index-path`), plus the proptest. The proptest **is** the gate; nothing existing covers
   the acceptance change.

### Wave 1 — gates and controls, before any byte-touching change

`H5` per-lane test-count assertion · `H6` anti-skip guard · `H12` timeouts + pinned nightly +
`--locked` · `H14` negative fixture-gate test · `H22` timeouts on the chaos tests · `H30`
de-flake. All `S`, all independent, all batchable in one PR per workflow file.

Then `H17` (de-`cfg(unix)`, `M`) — this is the change that makes the byte gates run on Windows at
all, so it precedes every `COMPAT-RISK` item below. Run `H16` (release-profile byte-gate) and
EXP-02 in the same wave: if the byte gates do **not** pass under `--release` today, `CI-06`
escalates to P0 and this wave blocks everything after it.

**New fixtures required in this wave**, because three later fixes change which bytes are *accepted*
and no current fixture exercises the boundary:

| Fixture | Needed by | Notes |
|---|---|---|
| an `.lsb` whose payload is 1 byte short of Σ`chunk_sizes` | `FMT-002`, `ALG-03` | must also cover the `tag != 0` form (`uncompressed_size < Σ`) |
| a golongtail-produced empty index | `FMT-008` | §9's first edge case is currently verified only against our own writer |
| a non-UTF-8 asset name from golongtail | `FMT-005` | EXP-16 decides whether this is a compat break or a doc fix |

### Wave 2 — data loss (no compat exposure; batch by file)

- **The prune cluster in one change set:** `FMT-003` `STORE-01` `STORE-02` `STORE-05` `OPS-15`,
  plus `OPS-04` (atomic index write) and `OPS-05` (SIGINT handlers). Requires `H3`
  (`FailingBlobStore`) to be testable at all — land `H3` first.
- **`STORE-03`** fsync — independent, `S`.

### Wave 3 — the untrusted-input panic cluster (sequence, do not parallelise)

1. `H4` + **`FMT-001`** + **`SEC-05`(a)** — the shared accessor touches seven call sites across
   three crates; land it before anything else edits `apply.rs`/`cp.rs`.
2. **`FMT-002`** + **`ALG-03`** — one Σ`chunk_sizes` check in `longtail-store/src/compress.rs:73-74`
   (which already holds the `BlockIndex`) covers both and simultaneously bounds the codec
   allocation. `COMPAT-RISK` — needs the Wave-1 fixture. Use `>=`, not `==`.
3. **`ALG-02`** + **`SEC-04`** + `H10` — `max_block_bytes` at the two transport reads plus a bounded
   brotli sink.
4. **`SEC-06`** + `STORE-12` + `API-15` (`H9`) — the rayon `panic_handler`, and convert
   `store/compress.rs:47`'s `expect` in the same commit.

### Wave 4 — CLI and API compatibility (batch; all touch `main.rs`)

**`API-01`** (`#[non_exhaustive]`) must land **before the first tag** — it is free today and
semver-major after. Then the endpoint cluster as one change: **`OPS-07`** + **`API-02`** +
`API-06` + `API-07`. Then **`OPS-06`** (aliases, `version`, the 8 global flags) with `OPS-16`
and `OPS-18`.

### Wave 5 — apply-path hardening

`H7` (**`OPS-01`** default-config resume test) first — it is the gate for everything downstream and
the precondition for ever revisiting `PERF-01`. Then `H8` (`OPS-08` `OPS-09` `OPS-13`), **`OPS-02`**,
**`OPS-10`**, `OPS-12` (with `CI-06`), **`OPS-14`**.

### Wave 6 — observability

`H18`: **`STORE-04`** + **`API-11`** + `API-12` in one change, because the `ErrorClass` mapping is
only truthful once the flattening is fixed.

### Wave 7 — documentation

`H13` (the `docs` job) · **`DOCS-03`** · **`DOCS-04`** · the §6.2 keeper-doc corrections ·
**`SEC-DOC-02`** (the trust boundary — write it while `SEC-03` is fresh) · **`DOCS-21`**/`PERF-01`
(rewrite or withdraw the roadmap item) · **`DOCS-01`**+**`DOCS-02`** last, because retiring
`switchover-checklist.md` has `DOCS-01` as a hard prerequisite (§11.3).

### 9.1 `COMPAT-RISK` items with no gate — the list to watch

| Item | Why the fix can change bytes | Gate today |
|---|---|---|
| **`SEC-01`** | changes which `.lvi` files are *accepted* | **none** — the H1 proptest must be written as part of the fix |
| **`ALG-01`** | the byte gates themselves do not run on Windows, so any Windows-side change to the scan/permission/path layer is ungated | **none until `H17`** — this is why `H17` precedes Wave 3 |
| **`API-20`** | an applied `clippy --fix` fuses the FMA in `discriminator_from_avg` and moves **every chunk boundary** | `lvi_byte_gate`/`upsync_byte_gate` — **unix-only**, so a Windows-side `--fix` is unguarded. Add the `#[allow]` now; it costs nothing |
| **`FMT-002`**, **`ALG-03`** | tighten what is accepted on read | `fixtures/` — but no short-payload fixture exists. Wave 1 |
| **`FMT-005`** | UTF-8 requirement on asset paths | none; no non-UTF-8 fixture exists. Wave 1 |
| **`ALG-09`** | dedup change alters bytes written by `clone-store` | `upsync_byte_gate` covers upsync, **not** clone-store |
| `FMT-004` `FMT-008` `ALG-06` `ALG-08` `ALG-11` `ALG-16` | self-consistency and derivation changes | `lvi_byte_gate` + `upsync_byte_gate` (per-PR, **unix-only**), `chunker_golden` (per-PR), `sync_fixtures` |
| **`PERF-01`** | not a fix — a change that must **not** be made without `H7` | `H7` is the gate; until it exists, treat the roadmap item as blocked |

Note throughout: the `*_differential.rs` suites are **weekly**, not per-PR. Do not treat them as a
gate for a change landing on a Tuesday.

---

## 10 · Pending experiments

~25 experiments were requested across the nine documents; **one has been run.** Ranked by *which
finding's severity the result would change*. Any finding whose severity depends on an unrun
experiment says so in its row above.

| ✔ | # | Status | Hypothesis / what it decides | Severity impact |
|---|---|---|---|---|
| [x] | EXP-01 | **RUN** | Does the PR-gating `--workspace` clippy require a cold vendored C build, and what does it cost? **Result:** mechanism CONFIRMED (submodule fetch, 704 `.o`, bindgen); cost **19 s cold vs 13 s warm** — ~6 s, not minutes. Full record: `target/review-evidence/EXP-01-cold-clippy.txt` | `CI-02` **keeps P2, argument changed** from cost to fragility + doc-honesty. The `MANIFEST.md` caveat that implied minutes is retracted (§8) |
| [ ] | EXP-02 | pending | Do the byte gates pass under `--release` today? `cargo test --release --locked -p longtail --test lvi_byte_gate --test upsync_byte_gate --test smoke` | A failure escalates **`CI-06` to P0** (the shipped profile is broken *now*). Highest-consequence unrun experiment |
| [ ] | EXP-03 | pending | Does a `.lvi` naming `../escaped.txt` write outside `--target-path`? Build the `.lvi` over a one-chunk fixture, `downsync` into `/tmp/t/inner`, `ls -l /tmp/t/escaped.txt` | If it does **not** appear, **`SEC-01` P0 → PLAUSIBLE** and we need the rejecting line. (The static premise is already verified; this is the end-to-end confirmation) |
| [ ] | EXP-04 | pending | On a case-insensitive filesystem, do two assets differing only in case produce one file of interleaved content? | **`OPS-08` P1 → P3** if downsync errors or `--validate` catches it |
| [ ] | EXP-05 | pending | Does a Linux-authored `.lvi` naming `nul.txt` / `con` / `saves:auto` write to a device or an NTFS stream on Windows with exit 0? | **`OPS-09` P1 → P3** if the open fails and the run errors |
| [ ] | EXP-06 | pending | Does `cp` against a `.lvi` with `asset_chunk_counts[0] = 0xFFFFFFFF` **abort** or error? Run under `ulimit -v` / a cgroup cap to make it deterministic | **`SEC-05`(a) P1 → P2** if the allocator returns and it becomes a growth cost. Refuter established the answer is host-dependent — run it on **Windows** and on a large-RAM Linux box |
| [ ] | EXP-07 | pending | Is `FMT-002`'s panic reachable end to end? Craft a short-payload `.lsb` in a scratch fs store and `downsync` against it | A clean typed error would mean a check exists that three readers missed → **`FMT-002` → P3** |
| [ ] | EXP-08 | pending | Empirical `OPS-03`: `printf '\n \n' > /tmp/empty.txt`, copy `fixtures/stores/default/store`, `prune-store`, then count `*.lsb` | Confirms the P0 empirically. A refusal would mean a guard was missed — but three independent reads say there is none |
| [ ] | EXP-09 | pending | Is rayon's default for a panic escaping `ThreadPool::spawn` with no `panic_handler` a process **abort**? 6-line scratch bin | If rayon swallows it, **`STORE-12` → P3** and `SEC-06`'s asymmetry narrows. Decides three findings at once |
| [ ] | EXP-10 | pending | Does any S3-compatible endpoint we target return `IsTruncated=true` with no continuation token? (minio, R2, Ceph RGW) | **`STORE-05`'s S3 leg → P3** if all are conformant; the fs leg stands regardless |
| [ ] | EXP-11 | pending | Does an S3 auth rejection surface as `StoreError::NotAuthorized` through a full `downsync`? minio lane, revoke the key | Today it should return `Backend(_)`. After the fix this **becomes** `API-11`'s regression test |
| [ ] | EXP-12 | pending | Is a lost rename observable — i.e. is the missing parent-dir fsync more than theoretical on ext4 `data=writeback`, XFS, NTFS? | **`STORE-03` → P2** (discarded-error half only) if the rename is durably journaled everywhere we ship |
| [ ] | EXP-13 | pending | Allocation sweep: do `lz4_flex` and `zstd` allocate the full declared `uncompressed_size` up front? Counting global allocator over `decode_block_payload` for all three tags | Refuter answered this from the dependency source (zstd bounded, lz4 allocates, brotli both) — run it to **confirm** and to size the cap. Would not change `ALG-02`'s priority |
| [ ] | EXP-14 | pending | Does the **default-configuration** incremental download spend a negligible fraction of wall in the target scan (because the scan does not run)? Remove `--no-cache-target-index` from `e2e.rs:396,439` | If the cached path *does* spend real time in `build_target_index`, R4's central claim about `PERF-01` being near-worthless is wrong |
| [ ] | EXP-15 | pending | Does `lto = "thin"` + `codegen-units = 1` + `strip = "symbols"` remove ≥25% of the 27,811,160 B binary? | **`PERF-08` → P3** if under ~10%, or if the release build exceeds ~5 min |
| [ ] | EXP-16 | pending | Does a non-UTF-8 asset name survive a golongtail upsync and then break the port? | A golongtail success + a Rust `InvalidUtf8` makes **`FMT-005` a compat break, arguing P1**; a golongtail refusal makes it doc-only |
| [ ] | EXP-17 | pending | Does golongtail v0.4.5 ignore `--enable-file-mapping` for version-index creation, as `96241fe` does? Two upsyncs of `fixtures/chunker.input`, `cmp` the `.lvi`s | Identical ⇒ **delete `SeedMode::Buffer`, `new_buffer`, and the 14 `*.buffer.json` tables** — removes the last FFI-only fixture dependency and simplifies the oracle exit (§11.2) |
| [ ] | EXP-18 | pending | `FMT-001` repro: build a `VersionIndex` with `asset_chunk_index_starts = [u32::MAX]`, serialize, re-parse, `validate_store` | A clean `Err` would demote **`FMT-001` to P3**. Also the seed for the `vi_walk` fuzz corpus |
| [ ] | EXP-19 | pending | Does `Runtime::drop` discard the detached `index_owner` task without polling it, so the fallback persist never runs under `*_blocking`? | **`STORE-16`** reduces to the discarded-error half if the task does run |
| [ ] | EXP-20 | pending | On Windows, does `Path::join` prefix-replace for `C:\…` and `\\server\share\…`? | Narrows **`SEC-01`'s Windows arm** to `..` only, making `OPS-09` the dominant Windows risk |
| [ ] | EXP-21 | pending | Does `cargo fuzz` find `FMT-001`/`FMT-002` in under a minute each, seeded from `fixtures/`? | Sets the CI fuzz budget (`H15`). No crash in 10 minutes would mean a reachability argument is wrong somewhere |
| [ ] | EXP-22 | pending | Does a **broken** vendored C build pass the check-only clippy gate silently (because clippy never links)? Deliberately break a C source | EXP-01 could not settle this — the C build succeeded with **0** `cargo:warning`. `CI-08`'s swallow is CONFIRMED from source but its consequence is unproven |
| [ ] | EXP-23 | pending | Does adding `#[non_exhaustive]` to the 13 options structs + 7 error enums compile the workspace unchanged? | Compile errors would enumerate the literal-construction sites **`API-01`'s Effort:S** missed |
| [ ] | EXP-24 | pending | Has the weekly s3-minio lane already been silently green-while-skipping? `gh run list --workflow s3-minio.yaml --limit 12` + grep a green run for `skipping` | A hit **proves `CI-03`'s scenario has already happened**, which is a materially stronger claim than "could" |
| [ ] | EXP-25 | pending | Are the `checked_*` guards load-bearing only on 32-bit? `cargo test -p longtail-core --target i686-unknown-linux-gnu` | Any failure upgrades **`FMT-014`** from documentation to a real portability bug |
| [ ] | EXP-26 | pending | Does the workspace still pass `cargo test` with the FFI pair removed from `[workspace.members]`, with an identical nextest list? | Confirms R2's residual-gate table and makes the oracle-exit step mechanical (§11.2) |
| [ ] | EXP-27 | pending | Is a clean `-D warnings` doc build reachable by fixing the 7 link defects + `doc = false`? Both with and without `--all-features` | Decides whether `H13`'s job is "add it" or "add it plus N fixes", and whether it needs a C toolchain |
| [ ] | EXP-28 | pending | Fix inputs, no severity change: the 16 corpus KAT sha256 constants (`ALG-07`); the `semver-checks --baseline-rev` recipe (`API-03`); whether the pinned `setup-rust-toolchain` fork enables rust-cache (`CI-02`/`PERF-12`); peak RSS vs `target_block_size` (`PERF-11`); golongtail's `cmd_get.go` handling of `s3-endpoint-resolver-uri` (`SEC-07`); Windows long-path behaviour with a derived relative target root | — |

---

## 11 · Open decisions for the maintainer

These are decision points, not findings. Each blocks or reshapes work above; none can be settled by
another reviewer.

### 11.1 Licensing — needs the repo owner, and confirmation with outside counsel

**General information, not legal advice.** The facts, all verified:

- There is **no `LICENSE` file** in the repository, and **none of the 9 workspace packages carries a
  `license` field** (`SEC-11`, `CI-11`).
- `cargo deny` reports **`licenses FAILED`** (`16-deny.txt:11214`) — `advisories`, `bans`, and
  `sources` all pass.
- The workspace **derives from** MIT-licensed upstream C, and the legacy pair **vendors and compiles**
  it (`support/longtail-sys/longtail/LICENSE.txt`). MIT carries attribution obligations that follow a
  shipped binary.
- The CLI binary is **named `longtail`**, the same name as the upstream project it replaces.
- The artifact will be **distributed inside a Tauri desktop application** and driven by a CI/CD
  pipeline.

Consequently: the license choice, the attribution mechanism for the vendored MIT code in shipped
binaries, and the naming question are all open. **This punchlist deliberately does not recommend a
license** — that is the repo owner's decision, and the attribution and naming questions in
particular are worth confirming with outside counsel before the switchover release. Once the
decision is made, `SEC-11` (`deny.toml` + `license` fields) and `CI-11` become mechanical.

### 11.2 The oracle — retain with a dated exit, and the deadline needs a date

**R2's recommendation, with R8's cost inputs, is a coherent package awaiting one decision: the exit
date.**

R2 recommends **Option B — retain, as a deprecated dependency with a written exit condition**, on
one argument: deletion permanently freezes `fixtures/`, because `xtask gen-fixtures` and
`diff-fixtures` are `#[cfg(feature = "differential")]` in their entirety. That regeneration
capability is the foundation the whole per-PR gate stands on, and fuzzing cannot supply it.

R2 also exposes a **priority inversion worth acting on regardless of the decision**: the
*trustworthy* oracle is the **golongtail v0.4.5 binary** (a SHA256-pinned download, no toolchain,
produces 78 of 112 fixtures), while the *fragile* oracle is the FFI library — pinned two minor
versions behind the fixtures, needed for only **28 JSON files**. Decoupling means moving those 28
files onto the good oracle (EXP-17 may delete 14 of them outright).

R8's cost inputs:

| Input | Value |
|---|---|
| Per-PR CI exposure to the differential lanes | **zero** — weekly + manual dispatch only |
| Weekly ceiling | ~70 runner-minutes (30 min linux + 40 min windows timeouts); **actual durations unknown** — the evidence pack was generated warm and locally |
| Per-PR exposure to the C library | `CI-02` only — **~6 s** (EXP-01), but network + C toolchain + libclang on a required check |
| Retention cost that actually matters | **fragility, not minutes**: 5 verified debts — submodule-as-`build.rs`-side-effect, C errors swallowed into `cargo:warning`, the v0.3.3-vs-v0.4.3 version skew, unverified `mkdata` downloads, and a structurally impossible Windows leg |
| Maintenance debt | `longtail-ffi` pins year-old deps (`aws-sdk-s3` 1.69 vs 1.120, `tokio` 1.43 vs 1.49), a plausible driver for part of the 20-name duplicate-version list |

**Decision needed:** (a) the exit date — R2 frames it as "spend the release cycle removing the reason
it is load-bearing", which implies **one release cycle**, but the date is not set; (b) which upstream
version is the intended oracle (the submodule says `v0.3.3-101`, the dead download path says
`v0.4.3`, the fixtures say `v0.4.5` — this one answer decides `ALG-10`, the decoupling steps, and
whether `SeedMode::Buffer` survives); (c) whether the Windows differential lane may fetch and spawn
the golongtail win32 binary, or whether Windows interop is consciously out of scope and should be
written into §Dropped-and-deferred (`CI-13`).

*If any part of this becomes a question about actual CI spend rather than engineering risk, that is a
budget question for Tim Hsu (CFO/COO), not something this review can settle.*

### 11.3 The keeper-doc list — one addition, one prerequisite

The stated keeper set is `readme.md`, `docs/format-spec.md`, `docs/rust-port.md`, plus `CLAUDE.md`.
Two challenges to it:

1. **R9 argues `fixtures/README.md` is a de-facto fifth keeper** and that treating it as disposable
   is the one part of the list it would change: it is accurate, linked, and describes **committed
   data that outlives every working document**. R1, R2, and R8 all read it as reference material.
   (It does have one defect to fix either way — `CI-DOC-01`: its byte-exactness claim is false for
   `.lsi` and its regeneration claim omits the real prerequisite.) **Recommendation: promote it.**
2. **Retiring `docs/switchover-checklist.md` has a hard prerequisite.** **32 of the CLI's 53 flags
   are documented *only* there** (`DOCS-01`), and the plan retires it. Either the flag reference
   moves to `--help` (R9's assumption, which makes `H27`'s help-text test mandatory) or it becomes a
   fifth/sixth keeper in its own right. Deleting the file first loses the only written description of
   most of the CLI's surface, with the CI/CD pipeline as a named consumer.
   Related: the runbook is **wrong in four ways** (`DOCS-02`) and its §Sign-off table is **empty**, so
   it has not yet been executed. If it *will* be executed, it must be fixed first; if the switchover
   has already happened by another route, it is deletable today and `DOCS-01`'s urgency shifts
   entirely onto the `--help` work.

### 11.4 The remaining product questions, ranked by how much work they redirect

| # | Question | Decides |
|---|---|---|
| 1 | **Is Windows a first-class production target?** | If the Tauri app ships Windows, `ALG-01` is the most important finding in this batch and should block. If Windows is CI-only assurance for a Linux/macOS product, `ALG-01` drops to P2 — and the whole Windows cluster (§1.4) re-prices |
| 2 | **Who can write to the production store?** | `SEC-01`'s *likelihood*. If the store is write-restricted to one signing CI identity and served over TLS, likelihood drops — but the guard is cheap and the consequence is code execution on end-user machines, so land it regardless. Also sets the frame for `SEC-03`'s trust-boundary text |
| 3 | **Is a store handle ever reused across two operations, now or planned?** | `STORE-09` alone: its safety rests entirely on "no". A long-lived store for a pause/resume loop or a download queue makes the dropped-get permit leak reachable → P1 |
| 4 | **Is `target_block_size` fixed by the pipeline, or can a store arrive with a much larger one?** | `PERF-02` and `PERF-11` both hinge on it. If it is contractually 8 MiB everywhere the app will look, both drop a level and become documentation |
| 5 | **Distribution intent: crates.io, or a git dep pinned by the launcher?** | `API-03`'s shape (`version` on path deps vs blanket `publish = false` + tags) and whether `API-01`'s semver discipline is load-bearing |
| 6 | **Are the bool-flag defaults right?** The oracle cannot settle them (kingpin renders `--[no-]x` without a default). `cache_target_index = true` is the consequential one — it is what puts a hidden `.longtail.index.cache.lvi` in every user's target folder and what makes `SEC-02` and `OPS-11` reachable | `OPS-18`, `OPS-11`, and part of `SEC-02`'s exposure |
| 7 | **Is a strict parse acceptable?** `FMT-001`'s cheap fix rejects a `.lvi` that C would accept (and then read out of bounds); the alternative spreads checked accessors across six files in three crates | `FMT-001`'s shape, and `FMT-004`/`FMT-005` by the same logic |
| 8 | **Should `prune`'s skip-versus-error policy be settled crate-wide?** Five `StoreIndex` methods currently do three different things with a wild block, and one of them deletes data | the whole prune cluster (§1.1) |
| 9 | **Are mixed Rust + golongtail writers to a *filesystem* store on Windows in scope?** | `STORE-07`/`STORE-DOC-01`: if yes the lock mechanism must change; if no it must be written down as unsupported. The differential lane running on Windows makes the answer ambiguous today |
| 10 | **Should `put` and `clone-store` be cancellable?** `clone-store` is the longest-running command and currently builds a token nothing cancels | `OPS-05`, `STORE-15` |
| 11 | **Does golongtail's clone-store derive the `.lsi` path with a suffix replace or a full string replace?** | whether `OPS-14` is a port bug or inherited parity. Either way, overwriting the artifact you just produced is not behaviour worth preserving |
| 12 | **Does the launcher already match on phase strings, and which ones?** | locks `API-17`'s naming before an enum ships |
| 13 | **Is `--source-index-path` on `upsync` used in production?** It is the read/exfiltration arm of `SEC-01` | if nothing uses it, removing it is cheaper than guarding it |
| 14 | **Who owns Renovate?** 10 stale branches including `crate-tokio-vulnerability` | `CI-12`: consume it or remove it |
| 15 | **What is the MSRV / toolchain policy for the Tauri build environment?** | needed to pin `rust-version` and the CI nightly meaningfully (`CI-05`) |

---

## 12 · Coverage audit

Each document's `## Files read` diffed against the ownership table. **Every `crates/*/src` file was
read by its owner** — there are no source-code gaps. The gaps are all in test and bench code, and
one documentation interior.

| Uncovered | Lines | Owner | Note |
|---|---|---|---|
| `support/longtail-testkit/tests/self_validation.rs` | 442 | R2 | not read; R2 read `chunker_golden.rs` and `hash_recompute_golden.rs` in full and `upsync_interop.rs` in part |
| `support/longtail-testkit/tests/format_differential.rs` | 431 | R2 | not read |
| `support/longtail-testkit/tests/downsync_three_way.rs` | 520 | R2 | only `:168-257` (R8) and `:360-390`, `:448-490` (an R10 refuter) |
| `support/longtail-testkit/tests/store_algebra_differential.rs` | 164 | R2 | not read |
| `support/longtail-testkit/tests/packing_differential.rs` | 128 | R2 | not read |
| `support/longtail-testkit/tests/codec_differential.rs` | 113 | R2 | not read |
| `support/longtail-testkit/tests/format_golden.rs` | 104 | R2 | not read — **this one is per-PR**, so it is a live gate nobody inspected |
| `support/longtail-testkit/tests/hash_differential.rs` | 88 | R2 | not read; `ALG` §4 recommends re-anchoring it on published BLAKE2s/BLAKE3 vectors anyway |
| `crates/longtail-cli/tests/s3_interop.rs` | 233 | R7/R8 | heads only (R9 `:1-12,:34-43`); one of the six env-gated skip sites in `CI-03` |
| `crates/longtail/tests/downsync_e2e.rs` | 185 | R7 | not named in R7's list; an R10 refuter read `resume_v1_then_v2` at `:52`. It is the natural home for the `OPS-02` and `OPS-01` tests |
| `crates/longtail-core/tests/file_infos.rs` | 132 | R1 | not named among R1's "6 core tests" |
| `crates/longtail-core/tests/codec_malformed.rs` | 131 | R1/R2 | not named; an R10 refuter read `:65-81,111-129` and found the one-byte-flip limitation that makes `ALG-02` unreachable by it |
| `support/longtail-bench/benches/{chunker,compression,hash,index_codec}.rs` | 282 | R4 | R4 read `lib.rs`, `bin/e2e.rs`, `bin/merge_mem.rs`; the four criterion benches were not read |
| `support/longtail-bench/src/bin/{dedup,ffi_driver}.rs` | 247 | R4 | not read |
| `docs/format-spec.md` §§4-5, §§7-8 — ~40 numeric constants and ~30 Rust-path references | — | R9 | **R9 flagged this itself**: the assigned worker did not return and R9 did not redo the sweep by hand. §§1-3 and §9 were checked by R1, §6 by R2, and R9 verified the provenance block, §10, the heading tree, and 9 of 101 citations (all exact). Since the spec is the authority for the paramount constraint, this is worth an hour before the doc is declared clean |
| `support/longtail-{sys,ffi}/src/**` | ~8,000 | — | **deliberate.** Legacy, non-default members, scheduled for deletion; only `build.rs` was read (R8, in full) |

**Pick-up list for the next round, in priority order:** `format_golden.rs` (a per-PR gate nobody
read) → the `docs/format-spec.md` constant sweep → `self_validation.rs` and `downsync_three_way.rs`
(they anchor the fixture story that the oracle decision turns on) → `downsync_e2e.rs` and
`s3_interop.rs` → the bench files.

---

## 13 · What this review did not cover

Stated plainly so nobody mistakes silence for assurance.

1. **Git history.** Deliberately deferred to a separate pass. The contract instructed every reviewer
   to review the current state, not intermediate states, and explicitly excluded commit hygiene. No
   reviewer looked for a regression introduced and then partly reverted, a `TODO` dropped in a
   rebase, or a divergence that entered without a decision record.
2. **Every S3 claim is read-from-source, not observed.** No reviewer ran anything against S3 or
   minio. `blob/s3.rs` sits at **24.49% region** coverage, and the S3 lane's env-gated tests
   `return` rather than fail when unconfigured, so **nextest records PASS while skipping**
   (`03-test.txt:280-281`: two behavioural S3 tests "passing" in 0.004 s with no endpoint). Treat
   `STORE-13`, `SEC-08`, `SEC-09`, `STORE-05`'s S3 leg, and every retry/timeout claim on the S3 path
   as source-derived. `CI-03` is the finding; EXP-24 would show whether it has already happened.
3. **Windows behaviour is reasoned, not observed.** `SEC-01`'s drive-letter/UNC arm, `OPS-08`
   (case-insensitive aliasing), `OPS-09` (device names, NTFS streams), `STORE-07` (mixed-writer
   locking) and `OPS-20` are argued from `std`'s documented contracts and from Go source. Two are
   filed **PLAUSIBLE** for exactly this reason. EXP-04, EXP-05, EXP-20 decide them.
4. **macOS is untested by anything.** All ten `runs-on` entries across the four workflows are
   `ubuntu-latest` or `windows-latest` — **there is no macOS lane at all**, and no reviewer had a
   macOS observation. Several findings (`OPS-02`'s EACCES, `SEC-10`'s symlink following, the
   case-insensitive-APFS half of `OPS-08`) are reachable there with CLI defaults. This is not in the
   Windows cluster and is not covered by any item above; it belongs in §11.4 question 1.
5. **No reviewer executed the code.** The contract forbade running `cargo`. Every metric comes from
   the evidence pack in `target/review-evidence/`, which was **generated warm and locally** — its
   timings are not CI timings, and `01b-clippy-ws.txt` carries an explicit warm-cache caveat.
   **27 of 28 experiments are unrun** (§10); nine findings' severities depend on them.
6. **No performance measurement was taken.** Every perf number cited traces to a committed
   `docs/bench-*.md` of varying staleness — and R4 found three of those documents mutually
   contradictory and one describing code that no longer exists (`PERF-DOC-03`, `PERF-DOC-04`,
   `DOCS-08`). Do not quote a number from them without re-measuring.
7. **Supply chain is a snapshot.** `cargo audit` and `cargo deny` outputs from one moment
   (0 advisories over 411 deps). No dynamic analysis, no transitive-dependency source review, and
   `cargo-udeps` could not run at all (it rejects `resolver = "3"` — see `07-unused.txt`).
8. **Coverage floors.** TOTAL is **77.09% region / 78.46% line**. The floor cases are
   `path_filter.rs` 13.7%, `longtail/src/lib.rs` 17.86%, `blob/s3.rs` 24.49%, `uri.rs` 56.83%, and
   `blob/fs.rs` 61.70% function. Findings exist for the first four; nobody set a policy floor
   (R8 Deliverable 3 proposes one).
9. **The `*_differential.rs` suites run weekly, not per-PR.** Any assurance attributed to them is
   assurance you get on some Monday, not on the commit you are about to merge.
10. **`pack`/`unpack` and the `.lrb` cache format** were touched only incidentally. `pack`/`unpack`
    are documented as deferred and their tests are `#[ignore]`d placeholders; `.lrb` is a
    compat-bearing on-disk layout **absent from the authoritative format spec** (`DOCS-12`) and was
    reviewed only from `cache.rs`.
