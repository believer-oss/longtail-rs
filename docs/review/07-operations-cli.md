# 07 · Operations & CLI review

- **Reviewed at:** `456274d` · **Lead model:** opus · **Workers:** 4 × fable
- **Slice:** apply/downsync/upsync/get/put/cp/clonestore/prune/inspect/fs_util/path_filter + the CLI
  binary, progress renderer, and the four integration-test files · **Confidence:** covered well on
  ordering, destructive safety, flag parity and resumability; **covered thinly on Windows** (no
  Windows host available — three findings are experiment-gated)

## Findings index

| ID | P | Dimension | One line | file:line | Verdict |
|---|---|---|---|---|---|
| OPS-01 | P1 | hardening | Resume-after-cancel is sound, but the only test disables the default `cache_target_index`, and the documented roadmap optimization breaks the invariant | `downsync.rs:174`, `smoke.rs:42` | CONFIRMED |
| OPS-02 | P1 | hardening | Second downsync of a retained-permissions read-only asset fails EACCES; C's chmod-`+w`-before-open step was not ported and `--use-legacy-write` (the golongtail escape) is rejected | `apply.rs:155`, `fs_util.rs:134` | CONFIRMED |
| OPS-03 | P0 | security | `prune-store` with an empty/blank `--source-paths` list file silently deletes every block in the store — no empty-keep-set guard, no `--force`, no prompt | `prune.rs:188`, `main.rs:569` | CONFIRMED |
| OPS-04 | P1 | hardening | `prune-store-index` overwrites the master `.lsi` with a non-atomic `fs::write`, and prune installs no SIGINT handler | `prune.rs:333`, `fs_util.rs:460` | CONFIRMED |
| OPS-05 | P1 | hardening | No ctrl-c handler on `put`, `clone-store`, `prune-store*`, `cp`; `clone_store` also builds a token it never cancels | `main.rs:753`, `clonestore.rs:107` | CONFIRMED |
| OPS-06 | P1 | complexity | 9 golongtail subcommand aliases, the `version` subcommand, and 8 global flags are absent — existing CI invocations die at clap parse | `main.rs:46`, `14-golongtail-help.txt:38` | CONFIRMED |
| OPS-07 | P1 | correctness | Two option-less index readers strand `--s3-endpoint-resolver-uri` on 8 subcommands; 4 of them honour it for the store but not the index | `inspect.rs:26`, `inspect.rs:89` | CONFIRMED |
| OPS-08 | P1 | security | Case-insensitive path aliasing defeats apply's range-disjointness premise: two assets differing only in case → two concurrent writers on one file | `apply.rs:8`, `apply.rs:146` | PLAUSIBLE |
| OPS-09 | P1 | security | No validation of asset names against Windows device names / `:` / trailing dot-space: a Linux-authored `.lvi` can write into `NUL` or an NTFS alternate data stream | `fs_util.rs:133` | PLAUSIBLE |
| OPS-10 | P1 | hardening | No free-space preflight; `set_len` makes a sparse file so ENOSPC surfaces mid-write, *after* deletes-first removed the old version | `fs_util.rs:141`, `apply.rs:89` | CONFIRMED |
| OPS-11 | P2 | hardening | `.longtail.index.cache.lvi` is written non-atomically and never fsynced; a torn cache is a hard error on the next run (by design) | `downsync.rs:219`, `fs_util.rs:460` | CONFIRMED |
| OPS-12 | P2 | hardening | `debug_assert_eq!` on chunk size is compiled out; a block-index/payload mismatch corrupts adjacent ranges or panics on the payload slice | `apply.rs:317`, `apply.rs:342` | CONFIRMED |
| OPS-13 | P2 | hardening | The stated disjointness invariant omits its own premise (unique asset paths) and nothing validates it | `apply.rs:8` | CONFIRMED |
| OPS-14 | P1 | correctness | `clone-store --create-version-local-store-index` writes the `.lsi` over the `.lvi` when the target path has no `.lvi` substring | `clonestore.rs:195` | CONFIRMED |
| OPS-15 | P2 | hardening | `prune-store-blocks` swallows every delete error and still reports success; the three prune commands disagree on stream and gating | `prune.rs:427`, `main.rs:832` | CONFIRMED |
| OPS-16 | P2 | idiom | Every non-cancel error collapses to exit 1; commands with no cancel handler die by signal so `ExitStatus::code()` is `None`, not 130 | `main.rs:512` | CONFIRMED |
| OPS-17 | P2 | correctness | `assets_written` counts write-plan files before any content lands | `apply.rs:156` | CONFIRMED |
| OPS-18 | P2 | idiom | `--retain-permissions`, `--scan-target`, `--cache-target-index` are parsed and never read; no `conflicts_with` on the pairs | `main.rs:108`, `main.rs:605` | CONFIRMED |
| OPS-19 | P2 | hardening | `path_filter.rs` is 13.7% region-covered with zero CLI tests; a leading `**` compiles an empty regex that excludes the entire tree | `path_filter.rs:36`, `15-coverage/summary.txt` | CONFIRMED |
| OPS-20 | P3 | hardening | `delete_assets`' 10-pass retry has no backoff, so it cannot ride out the transient Windows sharing violation it is shaped to survive | `apply.rs:427` | CONFIRMED |

## Scope

**Read in full:** `crates/longtail/src/{apply,downsync,fs_util,get,prune,put,path_filter,version}.rs`,
`crates/longtail-cli/src/{main,progress}.rs`, `crates/longtail/src/lib.rs`,
`crates/longtail/src/clonestore.rs` (lines 85–204 in full, remainder skimmed),
`crates/longtail/src/inspect.rs`, `crates/longtail/tests/smoke.rs` (cancel/resume half),
`target/review-evidence/14-golongtail-help.txt`.

**Skimmed:** `crates/longtail/src/{cp,upsync,options,progress,error}.rs` (`cp.rs` re-read in full
for OPS-07),
`crates/longtail-cli/tests/commands_spec.rs` (targeted sections; the full 1,349 lines were swept by
a worker and spot-verified), `crates/longtail/tests/downsync_e2e.rs`,
`support/longtail-testkit/src/tree_manifest.rs`.

**Declared secondary axis** (read outside the allowlist, filed only where it changes a slice
finding): `support/longtail-sys/longtail/src/longtail.c` and
`lib/concurrentchunkwrite/longtail_concurrentchunkwrite.c` (to check apply's C citations),
`crates/longtail-core/src/block.rs:129–137` (payload validation, for OPS-12),
`crates/longtail/src/options.rs:97–100` (defaults), `docs/rust-port.md`.

**Excluded:** the store layer (`longtail-store/**`) — R3 owns concurrency/durability there;
`crates/longtail-cli/tests/s3_interop.rs` beyond the flag sweep (self-skips without
`LONGTAIL_TEST_S3_*`, so `03-test.txt` does not cover it).

## Verification performed

Evidence-pack artifacts consulted: `MANIFEST.md`, `14-golongtail-help.txt` (the flag oracle — every
parity claim below cites it or `commands_spec.rs`), `15-coverage/summary.txt`, `07b-machete.txt`
(exit 1 = findings, per the MANIFEST correction), `12-loc.txt`, `03-test.txt`.

I re-read every worker claim that entered this document against the source, and reversed two of
them: a worker reported the apply disjointness argument as sound (it is, but its premise set is
incomplete — OPS-13), and a worker read `smoke.rs::cancel_mid_transfer_then_resume` as covering the
default resume path (it disables `cache_target_index` — OPS-01).

**Could not verify:** anything Windows-specific — no Windows host, and the six highest-value
integration test files are `#![cfg(unix)]` (`crates/longtail/tests/{downsync_e2e,smoke,lvi_byte_gate,
upsync_byte_gate,deadlock_regression}.rs`, `crates/longtail-cli/tests/commands_spec.rs`), so
`03-test.txt` carries no Windows behavioural evidence either. OPS-08/09 and the long-path question
are experiment-gated. Bool-flag defaults cannot be settled from the oracle: kingpin renders
`--[no-]scan-target` without a default (contrast `--log-level="warn"`), so the port's choice of
`default = true` for `retain_permissions`/`scan_target`/`cache_target_index` (`options.rs:97–100`)
rests on the code's `cmd_downsync.go:120–135` citation, not on the oracle. That matters — see
Open questions.

---

## Named deliverable 1 — resumability, as a specification

**The hypothesis is disproven.** A file left torn mid-`write_at` — preallocated to its full size by
`create_file_sized` (`fs_util.rs:141`), with unwritten holes reading as zeros — *is* detected on the
next run. The reason is that the completeness oracle is **content, not metadata**:

1. `create_version_index_from_folder` reads and HPCDC-chunks every byte of every target file
   (`version.rs:63–65` → `chunk_asset_streaming`), so a hole changes the asset's `content_hash`.
2. `create_version_diff(&target_index, &source_version)` therefore classifies the torn asset as
   content-modified, it re-enters the write plan (`apply.rs:96`), and step 5b re-truncates it
   (`apply.rs:155`).
3. When `--cache-target-index` is on (**the default** — `options.rs:100`, `main.rs:614`), the cached
   index is deleted *before* any target mutation (`downsync.rs:174–176`, citing cmd_downsync.go:274)
   and rewritten only after apply + flush + close + optional validate all succeed
   (`downsync.rs:218–220`). A cancel or a SIGKILL anywhere in between therefore leaves **no** cache,
   so the next run falls back to the content rescan.

So the CLI's message at `main.rs:509` ("run the same command to resume") is true, and
`crates/longtail/src/lib.rs:73–81` is accurate.

**The two invariants this rests on — write them down:**

- **I1 — the cache index is deleted before the first mutation and rewritten only on full success.**
  `downsync.rs:175` is load-bearing and its failure must stay fatal (it is: `?`).
- **I2 — the target scan's completeness test is a content hash, never `(size, mtime)`.**
  A torn file has *exactly* the desired size and an mtime newer than the run started, so size and
  mtime are not merely weak evidence — they are actively misleading.

**Why this is urgent rather than reassuring:**

- `docs/rust-port.md` §Roadmap names the fix for the 97%-of-wall target scan as "a streaming and/or
  **mtime/size-short-circuiting** target scan (as golongtail does)". The obvious implementation —
  "if the file already has the desired size, treat it as present" — violates I2 and turns every
  interrupted download into silent corruption with exit 0. R4 co-signs this as a precondition on
  that work item.
- The one existing regression test, `crates/longtail/tests/smoke.rs:161–210`, **does** compare
  post-resume content byte-for-byte (`TreeManifest::compare` checks `blake3_hex`,
  `support/longtail-testkit/src/tree_manifest.rs:110`), so it *would* catch a naive short-circuit —
  but `base_opts` sets `o.cache_target_index = false` (`smoke.rs:42`). **The default configuration's
  resume path, and therefore invariant I1, has zero test coverage.** That is OPS-01.
- `commands_spec.rs:374–402` (`downsync_corrupt_target_index_cache`) is the only test that exercises
  the default-on cache, and it covers "corrupt cache ⇒ hard error", not "cancel ⇒ cache absent".

## Named deliverable 2 — target-side durability answers

Stated in the same form R3 uses for the store side, so the two can be diffed.

| Event | What survives | Verdict |
|---|---|---|
| **Ctrl-c / cancel** | Files at final size with holes; cache index already deleted (`downsync.rs:175`); block cache intact | **Safe.** Next run content-rescans and heals. Cost: a full re-read + rehash of the whole target (OPS-01 note). |
| **SIGKILL / crash mid-apply** | Same as cancel — nothing in the apply path depends on orderly shutdown, because no metadata is treated as authoritative | **Safe**, same mechanism. |
| **Power loss mid-apply** | `write_at` is `pwrite` with **no fsync anywhere** (`fs_util.rs:158–183`; `grep` for `sync_all`/`sync_data` over `crates/longtail/src` finds nothing). After a power loss a file may hold stale or zeroed data at the correct length | **Safe for the target tree** for the same reason: the rescan hashes what is actually on disk. **Not safe for `.longtail.index.cache.lvi`** — `fs::write` (`fs_util.rs:460`) is neither atomic nor synced, so a power loss just after a *successful* downsync can leave a truncated cache, and `commands_spec.rs:374` proves the next run then fails hard. See OPS-11. |
| **ENOSPC** | `set_len` (`fs_util.rs:141`) is `ftruncate`, which reserves nothing, so the write plan is *always* accepted regardless of free space; the failure lands on a `pwrite` deep into the apply — **after** deletes-first (`apply.rs:89`) already removed the previous version's assets | **Unsafe operationally.** The target is left neither the old version nor the new one, with no free-space preflight and no message telling the user the disk is full beyond the raw `io` error. See OPS-10. |

The store-side counterpart (block writes, index shard flush) is R3's; the asymmetry worth
synthesising is that the **target** side is crash-safe by construction (content is the oracle) while
the **cache index and store index** are the two unsynced, non-atomic single-file overwrites in the
system (OPS-04, OPS-11).

## Named deliverable 3 — Windows

**Where the platform difference actually lives.** Four functions, eight `cfg` attributes, all in
`crates/longtail/src/fs_util.rs`; `apply.rs` has none:

| Site | Divergence |
|---|---|
| `fs_util.rs:20` / `:28` | `mode_of`: real `st_mode & 0x1FF` vs synthesized `0444`/`0666` (+`0111` for dirs) |
| `fs_util.rs:159` / `:165` | `write_at`: `write_all_at` (pwrite) vs a hand-rolled `seek_write` loop |
| `fs_util.rs:192` / `:201` | `set_permissions`: `chmod` low 9 bits vs the read-only attribute only |
| `fs_util.rs:215` / `:226` | `ensure_user_writable`: OR in `0o200` vs clear read-only |

**Answers to the unowned questions:**

- **`/`-separated `.lvi` paths joined on Windows — safe.** `scan_folder` always emits `/`
  (`fs_util.rs:76`) and Win32 accepts `/` as a separator; Rust's `Path` treats both `/` and `\` as
  separators on Windows, so `ensure_parent`'s `path.parent()` (`fs_util.rs:117`) splits correctly on
  a mixed-separator path. No finding.
- **Case-insensitive collisions — the real hazard.** OPS-08. The module's disjointness argument
  reasons over distinct `rel` *strings*; NTFS collapses `Data/x.pak` and `data/x.pak` to one file.
- **Reserved device names / illegal characters — unguarded.** OPS-09.
- **`>260`-char paths — unresolved.** Rust std applies `maybe_verbatim` (the `\\?\` prefix) only to
  paths it can make absolute. `derive_target_path` (`downsync.rs:243–253`) returns a bare stem, i.e.
  a **relative** target root, whenever `--target-path` is omitted — the configuration most likely to
  hit the limit. Experiment #3.
- **`u16` POSIX mask round-trip on NTFS — documented, with a security caveat.** `docs/format-spec.md`
  §563–580 states the Windows mapping and calls the degradation "expected/accepted upstream
  behavior". It is, but the consequence is not written down anywhere a user will see it: with
  `retain_permissions` defaulting to **true**, a `0600` asset downsynced onto Windows becomes
  readable by every local user, and a Windows-authored version re-downsynced onto Linux marks every
  writable file `0666`. Filed as `OPS-DOC-05`, not as a code finding — the format leaves no room to
  do better.
- **`seek_write` on a preallocated file — behaves like `pwrite` for this use.** Each writer opens its
  own handle (`apply.rs:328`), so the file-pointer side effect cannot interleave, and Rust std opens
  with `FILE_SHARE_READ|WRITE|DELETE` so concurrent handles are legal. One asymmetry the code should
  note: the Unix branch retries `ErrorKind::Interrupted` (std's `write_all_at` contract) and the
  Windows loop does not (`fs_util.rs:172` propagates it). Practically unreachable on Windows files;
  listed under Lower-priority.

**Exposure.** R8 owns the CI-lane asymmetry; the code-behaviour consequence is that of the seven
integration test files, six are `#![cfg(unix)]` — including every one that touches the apply path
end-to-end. On Windows, the only automated coverage of downsync is `apply.rs`'s in-module unit tests
(which use a mock store and a `tempdir`, so they do exercise `create_file_sized` + `seek_write`) and
`crates/longtail-store`'s own tests. Nothing exercises permissions, deletes, resume, or the CLI.

---

## Findings

### `OPS-01` — resume-under-default-config is untested, and the documented roadmap breaks it
**P1** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/downsync.rs:174-176` and `:218-220`; `crates/longtail/tests/smoke.rs:42`
- **What:** Resume-after-cancel is correct today (see Named deliverable 1) and rests on two
  invariants, I1 (cache index deleted before mutation, rewritten only on success) and I2 (the target
  scan's completeness test is a content hash). Neither is recorded in the four keeper docs. The only
  resume test disables `cache_target_index`, which is the library and CLI default, so I1 has no
  coverage. `docs/rust-port.md` §Roadmap proposes an "mtime/size-short-circuiting target scan",
  which is a direct violation of I2.
- **Failure scenario:** (a) Today: someone moves the cache-index write earlier for "crash
  resilience", or makes `delete_local` non-fatal; `smoke.rs` still passes because it sets
  `cache_target_index = false`; every interrupted production download then resumes against a cache
  claiming the target is already the new version, writes nothing, and exits 0 with holes in the
  game files. (b) After the roadmap item: a scan that skips a file whose size already equals
  `desired.asset_sizes[i]` skips *exactly* the torn files, because step 5b preallocated them to that
  size.
- **Evidence:** `apply.rs:155` (`create_file_sized(root, rel, size)` before any block arrives);
  `version.rs:63-65` (the scan reads every byte); `downsync.rs:113-129` (cache file short-circuits
  the scan entirely); `smoke.rs:42` (`o.cache_target_index = false;`);
  `support/longtail-testkit/src/tree_manifest.rs:110` (compare does check `blake3_hex`, so the test
  shape is right — only its options are wrong).
- **Recommendation:** (1) Add a second resume test with the **default** options, asserting the cache
  file is absent after the cancelled run and that the resumed tree matches byte-for-byte. (2) Write
  I1 and I2 into `docs/rust-port.md` next to the roadmap item, phrased as a precondition on it.
  (3) If the short-circuit lands, key it on a marker file written *before* mutation and removed after
  success — never on the desired size.
- **Tradeoff / risk:** None for the test. The doc change constrains a planned optimization, which is
  the point.
- **Effort:** S (test + doc)
- **Regression test to add:** `crates/longtail/tests/smoke.rs` — cancel mid-apply with default
  options; assert `!target.join(".longtail.index.cache.lvi").exists()`; resume; compare manifests.
  Add a hostile variant that re-plants a cache index claiming completion and asserts the resumed run
  still produces the right tree only when the cache is genuinely stale-safe.

### `OPS-02` — a read-only asset cannot be rewritten: EACCES on the second downsync
**P1** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/apply.rs:155` → `crates/longtail/src/fs_util.rs:134-139`
- **What:** Step 5b opens every write-plan file with `.write(true).create(true).truncate(true)`. On
  an existing file whose mode lacks `0o200` this returns `EACCES` (owner bits are checked, ownership
  is not). `retain_permissions` defaults to **true** (`options.rs:97`, `main.rs:605`), so step 7
  faithfully chmods assets to their recorded mode — including `0444`. The next downsync that must
  rewrite such an asset fails. `ensure_user_writable` exists (`fs_util.rs:215`) but is called only
  from `remove_asset` (`:249`, `:262`).
- **Failure scenario:** Ship v1 containing `data/config.ini` at mode `0444`. Downsync v1: file
  created, then chmod `0444`. Ship v2 with a changed `config.ini`. Downsync v2 → step 5b →
  `create {…}/config.ini: Permission denied`, exit 1, no partial progress, no actionable message.
  Same class for a `0555` directory that must receive a new file (`create_file_sized`'s `O_CREAT`
  needs write on the parent). Fully reachable with CLI defaults on Linux and macOS.
- **Evidence:** C's *legacy* write path does exactly the missing step —
  `support/longtail-sys/longtail/src/longtail.c:5315-5345` reads the permissions and, when
  `!(permissions & Longtail_StorageAPI_UserWriteAccess)`, calls
  `SetPermissions(path, permissions | UserWriteAccess)` **before** `OpenWriteFile` at `:5349`,
  restoring the recorded mode at `:5675`. `Longtail_ChangeVersion2` (`longtail.c:8720-8911`) has no
  equivalent — `grep GetPermissions` over `8000..8920` is empty — so this is C parity for the modern
  path, but golongtail offers `--use-legacy-write` as the escape and the port rejects it outright
  (`downsync.rs:31-33`, `LongtailError::LegacyWriteUnsupported`). The port therefore has strictly
  fewer options than the tool it replaces.
- **Recommendation:** Call `ensure_user_writable` immediately before `create_file_sized` in step 5b
  (and before the zero-asset create in 5a). When `retain_permissions` is false, capture the original
  mode and restore it after the drain, so the flag's "don't touch permissions" contract holds. This
  is a fix over C, not a divergence from the format — no bytes change.
- **Tradeoff / risk:** One extra `stat` per write-plan file (`ensure_user_writable` already
  short-circuits when the bit is set). Under `retain_permissions = false` the restore step is new
  code on a path that currently does nothing, so it needs its own test.
- **Effort:** M
- **Regression test to add:** `crates/longtail/tests/downsync_e2e.rs` (unix): downsync v1, `chmod 444`
  an asset that v2 modifies, downsync v2, assert success and the v2 manifest — plus the mirror case
  with `retain_permissions = false` asserting the mode is unchanged.

### `OPS-03` — an empty `--source-paths` file wipes the entire store
**P0** · `security` · CONFIRMED
- **Where:** `crates/longtail/src/prune.rs:188-232`; `crates/longtail-cli/src/main.rs:569-579`
- **What:** `prune-store`'s keep-set is built solely by iterating `source_version_index_paths`
  (`prune.rs:189-206`). `read_lines_file` trims and drops empty lines and returns `Ok(vec![])` for an
  empty or whitespace-only file. With no sources, the loop body never runs, `keep` stays empty, the
  dry-run early return is skipped, and `store.prune_blocks(&[])` is called with an empty keep set.
  Nothing checks `keep.is_empty()`; there is no `--force`, no prompt, and no count sanity check. The
  module doc is candid: "No confirmation prompts — the safety surface is `--dry-run`" (`prune.rs:1-3`).
- **Failure scenario:** The canonical CI shape is
  `aws s3 ls … | sed … > versions.txt && longtail prune-store --storage-uri s3://bucket/store --source-paths versions.txt`.
  If the listing command fails, matches nothing, is redirected before the producer runs, or writes a
  file of blank lines, `versions.txt` is empty. `prune-store` then overwrites the store index with an
  empty one and deletes every `.lsb` object. Every shipped version becomes unrecoverable; the blocks
  are gone from S3. Silent, total, irreversible, exit 0.
- **Evidence:** `prune.rs:188` (`let mut keep: HashSet<u64> = HashSet::new();`), `:190-192` (empty
  paths `continue`), `:209` (dry-run gate is the only early exit), `:232`
  (`store.prune_blocks(&keep_vec)`). `main.rs:569-579` (no empty-result error). No test covers the
  empty-keep-set case — `commands_spec.rs:1015-1069` only covers a *non-empty* keep set.
- **Recommendation:** Refuse to proceed when the resolved keep-set is empty unless an explicit
  `--allow-empty-keep-set` (or `--force`) is given, and refuse when `read_lines_file` yields zero
  entries for a `--source-paths`/`--target-paths` argument. Additionally consider a "would delete
  N of M blocks" ratio guard requiring confirmation above some threshold — but the empty check alone
  removes the catastrophic case and is three lines.
- **Tradeoff / risk:** Diverges from golongtail (which is equally unguarded), so a pipeline that
  legitimately prunes to nothing would break — that pipeline does not exist, and the flag covers it.
  Tag: this is a *behaviour* divergence on a destructive command, not a byte-compat one.
- **Effort:** S
- **Regression test to add:** `crates/longtail-cli/tests/commands_spec.rs` — `prune-store` with an
  empty list file must fail, and `count_ext(&chunks, "lsb")` must be unchanged.

### `OPS-04` — the master store index is overwritten non-atomically, with no cancel handler
**P1** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/prune.rs:332-334` → `crates/longtail/src/fs_util.rs:455-461`
- **What:** `prune_store_index` writes the pruned index back over `--store-index-path` with
  `write_to_uri`, which for a local path or `file://` is `fs::write` — `O_TRUNC` then write, no temp
  file, no rename, no fsync. Meanwhile `run_prune_store_index` (`main.rs:840-861`) installs no
  ctrl-c handler, so SIGINT terminates the process immediately (see OPS-05).
- **Failure scenario:** An operator runs `prune-store-index` against a filesystem-backed store, sees
  it take longer than expected on a multi-hundred-MB `.lsi`, and presses ctrl-c. The process dies
  between `O_TRUNC` and the last write. `store.lsi` is now a truncated prefix; every `downsync`,
  `get`, and `validate-version` against that store fails to parse it. Recovery requires knowing that
  `init-remote-store` rebuilds the index by scanning every block — an expensive operation and
  undiscoverable from the error message. Same window on a power loss (no fsync).
- **Evidence:** `fs_util.rs:460` (`fs::write(path, bytes)`); contrast the store layer, which *does*
  write fsblob objects via temp-file-plus-rename (`crates/longtail-store/src/blob/fs.rs:225-246`) —
  so the atomic primitive exists and this path bypasses it. `main.rs:557` dispatches
  `PruneStoreIndex` with no `install_cancel_handler`. S3 targets are unaffected (a single
  `PutObject` is atomic).
- **Recommendation:** Route `write_to_uri`'s local branch through the same temp-plus-rename the
  fsblob store uses, and `sync_all` the temp before renaming. That also fixes the `.lvi`,
  get-config, and cache-index writes in one change (`upsync.rs:166`, `put.rs:194`,
  `clonestore.rs:191`, `downsync.rs:219`). Cross-reference R3 for whether the lockless S3 shard path
  has an equivalent window.
- **Tradeoff / risk:** `COMPAT-RISK` is low but real: a rename changes the inode, which matters if
  anything holds the `.lsi` open or hardlinks it. No existing gate would catch a regression here —
  `crates/longtail-store/tests/sync_fixtures.rs` covers shard content, not write atomicity — which is
  itself part of this finding.
- **Effort:** M
- **Regression test to add:** a unit test asserting no partially-written file is observable — write a
  large index through `write_to_uri` while a reader polls, or simply assert the temp file appears and
  disappears. Also add ctrl-c handling per OPS-05.

### `OPS-05` — no ctrl-c handling on `put`, `clone-store`, `prune-store*`, or `cp`
**P1** · `hardening` · CONFIRMED
- **Where:** `crates/longtail-cli/src/main.rs:525-543` (the handler) and its three call sites
  `:627`, `:664`, `:738`; `crates/longtail/src/clonestore.rs:107`
- **What:** `install_cancel_handler` is wired into `downsync`, `get`, and `upsync` only. Because
  `tokio::signal::ctrl_c()` is what displaces the default SIGINT disposition, every other subcommand
  — including `put`, `clone-store`, and all three `prune-store*` commands — is killed abruptly by
  the first ctrl-c. `clone_store` compounds it: `let cancel = CancellationToken::new();`
  (`clonestore.rs:107`) creates a token that nothing can ever fire, so the operation is
  structurally uncancellable even from the library API.
- **Failure scenario:** `clone-store` materializes and re-uploads N versions serially, each a full
  download plus upload; ctrl-c during version 3 of 10 kills the process mid-`write_content`, leaving
  the target store with blocks written but its index not yet flushed (`clonestore.rs:187`) and the
  local `--target-path` folder holding a half-materialized version-3 tree. For `prune-store` the same
  ctrl-c can land inside the block-delete loop (`crates/longtail-store/src/remote.rs:565-577`) or, for
  `prune-store-index`, inside the non-atomic index write (OPS-04).
- **Evidence:** `main.rs:753-788` (`run_put`: progress bar, no cancel), `:883-926` (`run_clone_store`:
  same), `:816-881` (all three prunes: neither), `:1022-1037` (`run_cp`). `CloneStoreOptions` has no
  `cancel` field at all.
- **Recommendation:** Install the handler in `run` for every subcommand and thread a
  `CancellationToken` into `CloneStoreOptions` and `PutOptions` (which already forwards to `upsync`,
  where a token is honoured). For the prune commands, at minimum install the handler so the first
  ctrl-c sets a flag checked before the destructive phase begins, and treat a cancel arriving *after*
  the index overwrite as "finish the deletes" rather than "abort".
- **Tradeoff / risk:** Making prune cancellable mid-delete is worse than not cancelling it, so the
  check must be coarse (before/after the destructive phase), not per-object.
- **Effort:** M
- **Regression test to add:** hard to test portably; at minimum assert `PutOptions`/
  `CloneStoreOptions` expose a `cancel` field and that `clone_store` returns `Cancelled` when a
  pre-cancelled token is supplied.

### `OPS-06` — nine subcommand aliases, the `version` subcommand, and eight global flags are missing
**P1** · `complexity` · CONFIRMED
- **Where:** `crates/longtail-cli/src/main.rs:27-44` (globals), `:46-82` (subcommands);
  oracle `target/review-evidence/14-golongtail-help.txt:11-26` and `:38-96`
- **What:** golongtail publishes an alias for nine subcommands — `validate`, `printVersionIndex`,
  `printStoreIndex`, `stats`, `dump`, `init`, `createVersionStoreIndex`, `cloneStore`, `pruneStore`
  (oracle `:38-70`) — plus a `version` subcommand (`:85`). `grep alias crates/longtail-cli/src/main.rs`
  returns nothing. golongtail also has eight globals the Rust CLI lacks:
  `--show-store-stats`, `--mem-trace`, `--mem-trace-detailed`, `--mem-trace-csv`,
  `--[no-]log-to-console`, `--log-file-path`, `--log-coloring`, `--log-console-timestamp`
  (oracle `:11-26`). `pack`/`unpack` are also absent but are documented as deferred
  (`docs/rust-port.md` §Deferred).
- **Failure scenario:** The CLI is described as "a drop-in replacement" (`main.rs:1-3`). Any existing
  pipeline step spelled `longtail stats --storage-uri … --version-index-path …`, `longtail validate …`,
  `longtail init --storage-uri …`, or any step that passes `--show-store-stats` or `--log-file-path`
  for its JSON logs, fails with clap's usage error and exit 2 — at the first invocation after the
  switchover, not gradually.
- **Evidence:** the oracle lines above; `commands_spec.rs` exercises only the canonical names, so no
  test would catch the gap. `docs/switchover-checklist.md:18-19` claims "Global flags are identical in
  name/shape", which is true only of the four that were kept.
- **Recommendation:** Add `#[command(visible_alias = "…")]` for all nine (free, no behaviour change),
  add a `version` subcommand that prints what `--version` prints, and decide explicitly per missing
  global: accept-and-ignore with a one-line warning (`--mem-trace*`, `--log-coloring`,
  `--log-console-timestamp`), implement (`--show-store-stats` — `DownsyncReport.store_stats` already
  carries the data; `--log-file-path` — a second `tracing` layer), or document the removal. Silence is
  the one option that produces a broken pipeline.
- **Tradeoff / risk:** Aliases and a `version` subcommand are pure additions. Accept-and-ignore
  flags risk masking a real behaviour expectation (`--show-store-stats` in particular) — hence the
  per-flag decision.
- **Effort:** S for aliases + `version`; M for the globals
- **Regression test to add:** `commands_spec.rs` — one test that runs every alias and every global
  flag through `--help`/a trivial invocation and asserts exit 0, generated from a table so a new
  subcommand cannot forget its alias.

### `OPS-07` — two option-less index readers strand `--s3-endpoint-resolver-uri` on eight subcommands
**P1** · `correctness` · CONFIRMED
- **Where:** `crates/longtail/src/inspect.rs:25-27` (`read_version_index_from_uri`) and `:88-90`
  (`read_store_index_from_uri`)
- **What:** These two `pub` free functions take **only a URI**, so they cannot receive an endpoint
  override and fall back to `default_s3()` (`inspect.rs:17-22`) unconditionally. This is *not* a case
  of options being hard-coded in the surrounding module: every option struct in `inspect.rs` carries
  `s3_options` and threads it correctly to the block store — `ValidateVersionOptions:36`→`:69`,
  `InitRemoteStoreOptions:130`→`:155`, `CreateVersionStoreIndexOptions:179`→`:213`/`:219`,
  `PrintVersionUsageOptions:237`→`:277`. The defect is confined to the two signatures, and it leaks
  to **eight call sites across three files**:

  | Call site | Subcommand | Flag reaches the store? |
  |---|---|---|
  | `main.rs:677` | `ls` | n/a — no store |
  | `main.rs:705` | `print-version` | n/a — no store |
  | `main.rs:997` | `dump-version-assets` | n/a — no store |
  | `main.rs:929` (store index) | `print-store` | n/a — no store |
  | `crates/longtail/src/cp.rs:60` | `cp` | **yes** (`cp.rs:91`, `:107`) |
  | `inspect.rs:55` | `validate-version` | **yes** (`:69`) |
  | `inspect.rs:205` | `create-version-store-index` | **yes** (`:213`, `:219`) |
  | `inspect.rs:269` | `print-version-usage` | **yes** (`:277`) |

  The bottom four are the sharp part: `cp`, `validate-version`, `create-version-store-index`, and
  `print-version-usage` **accept the flag, honour it for their block-store I/O, and ignore it for
  reading the index** — an inconsistency *within a single invocation*, which is far harder to
  diagnose in the field than a flag that never works at all.
- **Failure scenario:** A studio running MinIO or an S3-compatible gateway invokes
  `longtail validate-version --storage-uri s3://assets/store --version-index-path s3://assets/game.v7.lvi --s3-endpoint-resolver-uri http://minio.internal:9000`.
  The store connection goes to MinIO as asked; the `.lvi` read at `inspect.rs:55` goes to the public
  AWS endpoint for bucket `assets` and fails with a network or 403 error naming AWS — so the operator
  sees a command that is demonstrably talking to MinIO fail with an AWS error, and reasonably
  concludes the endpoint flag works and the *store* is broken. If a real AWS bucket of that name
  exists, it silently validates against the wrong index instead. The plain-`ls` case
  (`longtail ls --version-index-path s3://… --s3-endpoint-resolver-uri …`) fails outright, which is
  at least honest. These are the commands a CI pipeline uses for its verification steps, so the
  failure lands at inspection time, not download time.
- **Evidence:** `inspect.rs:25-27` and `:88-90` (`fs_util::read_from_uri(uri, &default_s3())`); the
  eight call sites tabulated above, each attributed to its enclosing function by inspection. Contrast
  `run_downsync` (`main.rs:619-622`), which wires the flag properly. Note that `main.rs` *does* set
  `opts.s3_options.endpoint_url` for `cp` (`:1034`), `validate-version` (`:693`),
  `create-version-store-index` (`:811`), and `print-version-usage` (`:985`) — the CLI is not at
  fault; the library signature is. `commands_spec.rs` has zero coverage of the flag on any
  subcommand.
- **Recommendation:** Give both functions an `&S3OptionsArg` parameter and thread it from all eight
  call sites. Do **not** rewire the four `main.rs` handlers as if the CLI were the defect — four of
  the eight sites are inside the library and would be untouched by a CLI-only fix. Where a flag
  genuinely cannot be honoured, the CLI should reject it rather than ignore it.
- **Tradeoff / risk:** Both functions are `pub` re-exports (`lib.rs:59-64`), so adding a parameter is
  a **breaking public-API change** — flag to **R5** (public API surface). An additive
  `_with_options` variant avoids the break at the cost of leaving the footgun signature in place;
  given there is no released baseline (`18-semver.txt`), taking the break now is cheaper than
  carrying two spellings.
- **Effort:** S
- **Regression test to add:** extend `crates/longtail-cli/tests/s3_interop.rs` (already MinIO-shaped)
  to run all eight affected subcommands against the MinIO endpoint with the flag, asserting success —
  and in particular `validate-version`, which exercises both halves (store *and* index) in one
  invocation and would have caught this.

### `OPS-08` — case-insensitive path aliasing breaks apply's range-disjointness premise
**P1** · `security` · PLAUSIBLE
- **Where:** `crates/longtail/src/apply.rs:8-18` (the invariant), `:146-158` (step 5b's `created`
  set), `:309-345` (`write_block_chunks`' per-file grouping)
- **What:** The module argues correctness from "true range disjointness", deriving it from
  strictly-increasing per-asset offsets, diff-set disjointness, and the first-wins chunk→block map.
  Every step of that argument keys on the asset's `rel` **string**. Step 5b dedups with
  `HashSet<String>` (`:146`) and `write_block_chunks` groups with `HashMap<&str, …>` (`:309`). On a
  case-insensitive filesystem two distinct index strings can name one file, and then two block tasks
  hold two handles onto the same inode and write at unrelated offsets.
- **Failure scenario:** A `.lvi` produced by a Linux upsync contains `Content/pak0.pak` (900 MB) and
  `content/pak0.pak` (12 MB) — legal on ext4, produced by any tooling that is inconsistent about
  case. Downsync onto NTFS: step 5b creates the file at 900 MB, then re-truncates it to 12 MB (the
  second `create_file_sized` runs `truncate(true)` + `set_len(12 MB)`). Concurrent block tasks then
  `pwrite` the 900 MB asset's chunks at offsets past EOF (extending the file) and the 12 MB asset's
  chunks over the same range. The result is one file of indeterminate content, `assets_written`
  reports 2, and the apply returns `Ok`. Without `--validate` nothing notices; the next downsync's
  content rescan sees one file where the index expects two, so the tree never converges.
- **Evidence:** `apply.rs:153` (`if created.insert(rel.clone())` — string identity),
  `fs_util.rs:137` (`truncate(true)` on the second create), `:328` (a fresh handle per (block, file)).
  No normalization or collision check exists in `apply.rs`, `fs_util.rs`, or
  `longtail-core/src/version_index.rs`. Marked PLAUSIBLE because I could not execute it on Windows
  or APFS; the code path is unambiguous but the filesystem behaviour is asserted from knowledge, not
  measured. Experiment #1.
- **Recommendation:** Validate the source version index once, before apply, for
  case-insensitive-duplicate asset paths, and fail with a named error listing the colliding paths.
  Do this on all platforms (a Linux-authored index that cannot be applied on Windows should fail on
  the machine that *produces* it, ideally in `upsync`). This is strictly a new check — no bytes
  change.
- **Tradeoff / risk:** `COMPAT-RISK`: it rejects indexes that C/golongtail accepts. Make it a hard
  error only on case-insensitive targets and a warning elsewhere, or gate it behind the same
  validation that already runs. No existing gate would catch a regression —
  `crates/longtail-cli/tests/commands_spec.rs` is `#![cfg(unix)]`.
- **Effort:** S
- **Regression test to add:** a unit test in `apply.rs`'s test module building a `VersionIndex` with
  `A.bin` and `a.bin` and asserting the pre-apply validation rejects it — runnable on Linux, which is
  the point.

### `OPS-09` — no validation of asset names against Windows device names, `:`, or trailing dot/space
**P1** · `security` · PLAUSIBLE
- **Where:** `crates/longtail/src/fs_util.rs:127-145` (`create_file_sized`), `:109-112`
  (`create_dir`), `:239-269` (`remove_asset`)
- **What:** Asset paths from a `.lvi` are joined onto the target root and opened with no name
  validation. Windows reserves `CON`, `PRN`, `AUX`, `NUL`, `COM1..9`, `LPT1..9` (with any extension),
  forbids `: * ? " < > |`, and strips trailing dots and spaces. A Linux-authored index can contain
  all of them.
- **Failure scenario:** An asset named `nul.txt` — or `nul` — is legal on ext4. On Windows,
  `create_file_sized` opens the NUL device: the open succeeds, `set_len` and every `seek_write`
  succeed, all bytes are discarded, and the apply returns `Ok`. Worse, an asset named
  `saves:autosave` opens an NTFS **alternate data stream** on `saves`: content lands in a stream
  that `dir` does not show and that is lost on any copy to a non-NTFS volume. In both cases
  `assets_written` counts the asset, exit code is 0, and the file the game needs does not exist.
  `--validate` would catch it (the rescan's asset count would differ), but `--validate` defaults off.
- **Evidence:** `fs_util.rs:133` (`root.join(rel_path)` then straight to `OpenOptions::open`); no
  name checks anywhere in the crate. PLAUSIBLE for the same reason as OPS-08 — the Win32 behaviour is
  asserted, not measured here. Experiment #2.
- **Recommendation:** Validate asset names once per index, before apply, on **all** platforms:
  reject reserved stems, the forbidden character set, and trailing dot/space. Producing such a name
  is almost always a bug in the content pipeline, so failing loudly at upsync time is the real fix;
  failing at downsync time is the safety net.
- **Tradeoff / risk:** `COMPAT-RISK`: rejects indexes C accepts. Existing stores may already contain
  such names, so this must be surveyable before it is enforced — hence "validate and report" before
  "validate and reject". No existing gate covers it.
- **Effort:** S (the check) + M (deciding enforcement)
- **Regression test to add:** a unit test over the validator with the full reserved-name table,
  runnable on Linux.

### `OPS-10` — no free-space preflight; ENOSPC lands after deletes-first
**P1** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/fs_util.rs:140-143` (`set_len`); `crates/longtail/src/apply.rs:89`
  (deletes) vs `:186-246` (writes)
- **What:** `set_len` is `ftruncate`, which extends the file sparsely and reserves no blocks, so the
  write plan is accepted no matter how little free space exists. Nothing sums `desired.asset_sizes`
  over the write plan and compares it with the filesystem's free space. Deletes run first, so by the
  time the disk fills, the previous version's removed assets are already gone.
- **Failure scenario:** A launcher updates a 60 GB install on a volume with 8 GB free where the
  update needs 12 GB net. Deletes remove the v1-only assets. Step 5b preallocates every write-plan
  file — all succeed, because the files are sparse. Blocks stream in and `pwrite` starts failing
  `ENOSPC` partway through. The apply returns `Err`, the CLI prints `error: io error: write_at: No
  space left on device`, exit 1. The target is now neither v1 nor v2, the user's disk is full, and
  the only recovery is to free space and re-run — which will content-rescan the whole 60 GB first.
- **Evidence:** `fs_util.rs:141` (`file.set_len(final_size)`); `apply.rs:144-158` runs before any
  fetch; `apply.rs:89` runs before that. `grep` for `available_space`/`free` over
  `crates/longtail/src` finds nothing. This is faithful to C (`OpenWriteFile(path, m_FileSize)`,
  `longtail_concurrentchunkwrite.c:108`), so it is parity — but the Tauri app is the first consumer
  that can do something about it.
- **Recommendation:** Before step 5a, sum `desired.asset_sizes[i]` over `write_asset_indexes` and
  compare against available space on the target volume; fail early with a typed
  `LongtailError::InsufficientSpace { needed, available }` so the launcher can show "needs 12 GB,
  8 GB free" instead of a raw `ENOSPC` mid-download. Do the check *before* the deletes, so a failed
  precondition leaves the previous version intact.
- **Tradeoff / risk:** The estimate is an upper bound (it ignores the space reclaimed by deletes and
  by shrinking assets), so a strict check would refuse some updates that would in fact fit. Compute
  `Σ new − Σ removed` and add a margin, and make the check advisory-but-typed rather than absolute.
  Needs a dependency for free space (`fs4` is already in the tree per `docs/rust-port.md`).
- **Effort:** M
- **Regression test to add:** hard to test without a small loopback filesystem; at minimum unit-test
  the space-estimate function against a synthetic diff.

### `OPS-11` — the target-index cache is written non-atomically and unsynced
**P2** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/downsync.rs:218-220` → `crates/longtail/src/fs_util.rs:455-461`
- **What:** The success-path cache write is `fs::write` — truncate, write, no fsync, no rename. A
  corrupt cache is deliberately a hard error, which is the right call
  (`commands_spec.rs:374-402` pins it) but makes an unsynced write consequential.
- **Failure scenario:** A downsync finishes; the cache write returns; the machine loses power before
  the page cache is flushed. On the next boot `.longtail.index.cache.lvi` exists with a truncated or
  zero-length body. The next `downsync`/`get` reads it as the target index
  (`downsync.rs:113-114` → `read_version_index_local`) and fails to parse — exit 1, every retry
  identical, and the fix (delete a dot-file inside the game folder) is not in the error message.
  For a Tauri launcher this is an unrecoverable-looking update loop.
- **Evidence:** `fs_util.rs:460`; `downsync.rs:271-274` (parse failure propagates);
  `commands_spec.rs:398-401` asserts the hard failure.
- **Recommendation:** Two independent fixes, both cheap: (1) write the cache via temp-plus-rename
  with an `sync_all` on the temp (shared with OPS-04); (2) treat an unparseable *cache* index — as
  distinct from an explicit `--target-index-path` — as "no cache", delete it, and fall back to the
  scan. (2) alone converts a hard failure into a slow success and is the behaviour a launcher needs.
- **Tradeoff / risk:** (2) softens a deliberate design choice, so keep the hard error for the
  explicit `--target-index-path` (a user-supplied file must not be silently ignored) and update the
  test at `commands_spec.rs:374` to assert the new split.
- **Effort:** S
- **Regression test to add:** replace `downsync_corrupt_target_index_cache` with two tests: corrupt
  *cache* → succeeds and rewrites the cache; corrupt explicit `--target-index-path` → fails.

### `OPS-12` — the chunk-size cross-check is a `debug_assert`, and the payload slice is unchecked
**P2** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/apply.rs:317` and `:342`
- **What:** `debug_assert_eq!(bsz, w.chunk_size)` compares the size the *block index* reports for a
  chunk with the size the *version index* reports. It is compiled out of release. The subsequent
  write uses the block's size (`bsz`) at the version's offset (`w.asset_offset`), and slices
  `block.payload[block_off..block_off + len]` with no bounds check. `StoredBlock::from_bytes`
  explicitly does not validate the payload length against the declared chunk sizes
  (`crates/longtail-core/src/block.rs:129-137`: "the tail is the payload, so oversize buffers are not
  rejected").
- **Failure scenario:** A block whose index declares chunk `X` at 300 bytes while the version index
  records 200. Release build: 300 bytes are written at an offset spaced for 200, clobbering the next
  chunk's region and pushing the file past `asset_sizes[ai]`; `written` overcounts; no error. If the
  declared sizes instead exceed the actual payload, the slice panics inside `spawn_blocking` —
  survivable today (`flatten_apply_task`, `apply.rs:353-363`, converts the `JoinError` into
  `io error: apply block task: task panicked: …`, which tells the operator nothing about the corrupt
  block) but fatal under `panic = "abort"`.
- **Evidence:** `apply.rs:317` (`debug_assert_eq!`), `:342` (`&block.payload[block_off..block_off + len]`),
  `block.rs:129-137`. Whether a corrupt block can reach here at all depends on block-hash
  verification in the store read path — cross-reference **R3** (store read verification) and **R2**
  (block decode validation); this finding is about apply's own robustness either way.
- **Recommendation:** Promote both to checked errors: return
  `StoreError::BadFormat` when `bsz != w.chunk_size` (the `ok_or_else` two lines above already
  models the shape), and bounds-check `block_off + len <= block.payload.len()` before slicing.
- **Tradeoff / risk:** None — both are impossible on well-formed input, so the only behaviour change
  is on input that currently corrupts or panics.
- **Effort:** S
- **Regression test to add:** in `apply.rs`'s test module, hand the `MockStore` a block whose index
  declares a larger chunk than the payload holds, and assert a typed `BadFormat` error rather than a
  join error.

### `OPS-13` — the stated disjointness invariant omits its own premise
**P2** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/apply.rs:8-18`
- **What:** The module doc lists three grounds for range disjointness: increasing per-asset offsets,
  diff-set disjointness, and the first-wins chunk→block map. It omits a fourth, load-bearing premise:
  **no two entries of `desired` share an asset path.** Nothing validates it — not
  `VersionIndex::from_bytes`, not `change_version2`, not the diff (which keys on `path_hashes`, so
  two identical paths produce two identical hashes and both survive into the write plan).
- **Failure scenario:** A `.lvi` — corrupt, hand-crafted, or produced by a buggy third-party writer —
  lists `data/x.pak` twice with different chunk lists. Both indexes land in `write_asset_indexes`
  (`apply.rs:95-96`); step 5b's `created` set means only one `create_file_sized` runs, so the file
  ends at whichever size came first; both assets' chunk plans go into `block_writes` with the same
  `rel` and overlapping `asset_offset` ranges, and two block tasks write conflicting bytes at the
  same offsets. Output is nondeterministic — which directly contradicts the
  `permuted_completion_order_yields_byte_identical_trees` guarantee the module advertises.
- **Evidence:** `apply.rs:95-129` (no uniqueness check while building the plan), `:146-158` (dedup by
  path, which *hides* the duplicate rather than rejecting it), `:309` (`HashMap<&str, …>` merges the
  two plans into one file's run list, where the sort at `:327` then produces duplicate offsets that
  the merge loop at `:330-345` was never designed to see).
- **Recommendation:** State the premise in the module doc, and enforce it once per apply: build a
  `HashSet<&str>` over the write-plan paths (the code already builds `created` — make its `insert`
  returning `false` a `BadFormat` error instead of a silent skip). Fold in the case-insensitive check
  from OPS-08 at the same site.
- **Tradeoff / risk:** `COMPAT-RISK`: rejects an index C would attempt. C is nondeterministic on the
  same input, so this is a strict improvement, but it is a behaviour change on malformed input. No
  existing gate covers duplicate paths.
- **Effort:** S
- **Regression test to add:** as OPS-08, a unit test with a duplicate-path `VersionIndex` asserting a
  typed error.

### `OPS-14` — `clone-store --create-version-local-store-index` can overwrite the `.lvi` it just wrote
**P1** · `correctness` · CONFIRMED
- **Where:** `crates/longtail/src/clonestore.rs:191` and `:194-198`
- **What:** Line 191 writes the target version index to `target_lvi`. Line 195 derives the `.lsi`
  path as `target_lvi.replace(".lvi", ".lsi")`. `str::replace` replaces **all** occurrences and
  returns the input unchanged when there is no match — so when the target path does not contain
  `.lvi`, `lsi_path == target_lvi`, and line 197 writes the store index over the version index.
- **Failure scenario:** A `--target-paths` list file (read verbatim by `read_lines_file`,
  `main.rs:885`) contains `s3://backup/versions/game-v7.index` — any naming convention that is not
  `.lvi`. With `--create-version-local-store-index`, clone-store reports success, `cloned` is
  incremented, and the object at that URI is a `.lsi`. Any subsequent `get`/`downsync` naming it as a
  source fails to parse. The version index is unrecoverable except by re-running clone-store without
  the flag. A second, milder case: a path such as `s3://b/archive.lvi.d/v1.lvi` has both
  occurrences replaced, producing `s3://b/archive.lsi.d/v1.lsi`.
- **Evidence:** `clonestore.rs:195` (`target_lvi.replace(".lvi", ".lsi")`); `:191` and `:197` write
  through the same `write_to_uri` with no exists-check, and `write_to_uri`'s local branch is a
  truncating `fs::write` (`fs_util.rs:460`) while its S3 branch is an unconditional `PutObject`
  (`crates/longtail-store/src/blob/s3.rs:398-411`). `commands_spec.rs:1160-1213` covers clone-store
  but never passes `--create-version-local-store-index`.
- **Recommendation:** Use `strip_suffix(".lvi")` and error out when the target path does not end in
  `.lvi` (or when the derived `.lsi` path equals the `.lvi` path). Assert
  `lsi_path != target_lvi` before writing.
- **Tradeoff / risk:** golongtail's derivation could not be checked (no Go source in the tree), so
  this may be parity — but overwriting the artefact you just produced is not behaviour worth
  preserving. Listed as an Open question.
- **Effort:** S
- **Regression test to add:** `commands_spec.rs` — clone-store with `--create-version-local-store-index`
  and a target path not ending in `.lvi` must fail, and the `.lvi` must still parse.

### `OPS-15` — prune reports success it did not achieve, and the three commands disagree on output
**P2** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/prune.rs:424-433`; `crates/longtail-cli/src/main.rs:831-837`,
  `:854-859`, `:874-880`
- **What:** Two separate problems on the destructive commands. (a) `prune_store_blocks` counts a
  deletion only when it succeeds and discards the error:
  `if let Ok(mut obj) = … && obj.delete().await.is_ok() { deleted += 1 }`. Every failure is invisible.
  (b) Output is inconsistent across the three siblings: `prune-store` prints its dry-run result to
  **stdout** unconditionally but its real result to **stderr** only under `--show-stats`;
  `prune-store-index` prints `Pruned N blocks out of M` to stdout unconditionally **including in
  dry-run**, where nothing was pruned; `prune-store-blocks` prints three stdout lines and correctly
  suppresses `Deleted` in dry-run.
- **Failure scenario:** (a) A pipeline runs `prune-store-blocks` against a bucket where the IAM role
  lacks `s3:DeleteObject`. Every delete 403s. The command prints `Found 4210 blocks to prune` /
  `Deleted 0 blocks` and exits **0**. Storage cost never drops and no alarm fires; if the operator's
  check is on exit status (the normal case), the failure is undetectable. (b) An operator scripts
  `prune-store --dry-run … | tee audit.log` to review before committing, then drops `--dry-run`;
  `audit.log` is now empty for the real run and the operator concludes it did nothing. And
  `prune-store-index --dry-run` printing "Pruned N blocks" is an outright false statement.
- **Evidence:** `prune.rs:427-431`; `main.rs:832-836` (dry-run → `println!`, real → `eprintln!`
  behind `cli.show_stats`), `:854-859` (no dry-run branch at all), `:877-879` (correct).
- **Recommendation:** Collect delete failures in `prune_store_blocks`, return them (a
  `failed_deletes` count on `PruneStoreBlocksResult`), and make a non-zero count a non-zero exit.
  Normalise all three commands: results on stdout, unconditional, with the dry-run distinction in
  the wording ("would delete N").
- **Tradeoff / risk:** Changing stdout wording breaks anything parsing it — but no test pins the
  format (see Hardening backlog), so nothing in-repo constrains it, and the current wording is wrong.
- **Effort:** S
- **Regression test to add:** `commands_spec.rs` — assert `prune-store-index --dry-run` output does
  not claim to have pruned, and that all three commands emit their result on stdout without
  `--show-stats`.

### `OPS-16` — exit codes collapse to 0/1/130, and 130 is not always reachable
**P2** · `idiom` · CONFIRMED
- **Where:** `crates/longtail-cli/src/main.rs:502-518`
- **What:** The top-level match has two error arms: `Cancelled` → 130, everything else →
  `ExitCode::FAILURE` (1). `LongtailError` has fourteen variants (`crates/longtail/src/error.rs:20-84`)
  spanning "bad argument", "store not authorized", "network", "not found", "validation mismatch", and
  "corrupt format", all indistinguishable to a caller. Separately, the commands with no cancel handler
  (OPS-05) die *by signal*, so a Rust or Python caller reading `ExitStatus::code()` gets `None`, not
  `Some(130)`. And if any error occurred before a ctrl-c, apply prefers the error
  (`apply.rs:190`, `first_err.get_or_insert`), so a cancelled-but-also-failing run exits 1.
- **Failure scenario:** The CI pipeline wants "retry on transient failure, fail the build on bad
  input". With one error code it must parse `stderr`, which is unpinned English. A transient S3 5xx
  and a typo in `--storage-uri` are the same exit code, so either every failure is retried (wasting
  a build slot and delaying the real signal) or none is.
- **Evidence:** `main.rs:512-517`; `error.rs:20-84`. `commands_spec.rs` asserts only
  `status.success()` / `!status.success()` — the specific value 1 is never checked, and 130 has no
  test at all.
- **Recommendation:** Map to a small, documented set: 0 success, 1 unexpected/internal, 2 clap
  (already), 3 invalid argument or config, 4 store not-found/validation (content problem — do not
  retry), 5 network/auth (retryable), 130 cancelled. Document it in `readme.md`. Note the divergence
  from golongtail explicitly rather than silently.
- **Tradeoff / risk:** golongtail returns 1 for everything, so richer codes are a divergence — but a
  divergence a pipeline can only benefit from, since it previously had no information. Anything
  branching on "exit == 1" would change behaviour, so it belongs in the switchover notes.
- **Effort:** S
- **Regression test to add:** a table-driven `commands_spec.rs` test asserting the exact code per
  error class, plus a SIGINT test (spawn, signal, assert 130) so the 130 path stops being untested.

### `OPS-17` — `assets_written` counts planned files, not written ones
**P2** · `correctness` · CONFIRMED
- **Where:** `crates/longtail/src/apply.rs:140` and `:156`
- **What:** `stats.assets_written` is incremented in step 5a (empty files) and step 5b (every
  pre-created write-plan file) — both strictly before any block is fetched. It is reported as
  "assets written" (`main.rs:1216`) and surfaced on `DownsyncReport.assets_written`, which the Tauri
  app will read.
- **Failure scenario:** Not corruption — a false report. The number is correct for a successful
  apply, so the defect only shows where the count matters most: any future partial-success or
  progress-reporting use reads "4 assets written" when zero bytes have landed. A launcher showing
  "wrote 12,438 files" the instant preallocation finishes, before the download starts, is a visible
  bug.
- **Evidence:** `apply.rs:156` sits inside the `created.insert(...)` branch of the pre-create loop,
  which the module doc itself describes as running "strictly BEFORE the concurrent loop" (`:15-18`).
- **Recommendation:** Either rename the field to `assets_planned` and add a separate
  `assets_completed`, or move the increment to a per-file completion count. The first is cheaper and
  more honest.
- **Tradeoff / risk:** `DownsyncReport` is public API; renaming is breaking. Adding a field is not.
- **Effort:** S
- **Regression test to add:** in `apply.rs`'s test module, fail one block and assert the reported
  written-asset count reflects completion, not planning.

### `OPS-18` — three flags are parsed and never read, and the negatable pairs have no conflict check
**P2** · `idiom` · CONFIRMED
- **Where:** `crates/longtail-cli/src/main.rs:108-111`, `:122-129`, `:158-161`, `:171-178`,
  `:401-404`; consumers `:605`, `:613-614`, `:646`, `:650-651`, `:899`
- **What:** golongtail spells these as kingpin `--[no-]x` booleans; the port renders each as two
  independent clap flags and reads only the `no_` half. So `--retain-permissions`, `--scan-target`,
  and `--cache-target-index` are accepted and have no effect, and `--retain-permissions
  --no-retain-permissions` silently resolves to "no" with no `conflicts_with` diagnostic.
- **Failure scenario:** Behaviourally inert today, because all three default to true — but the flags
  read as an override that does not exist. An operator debugging a permissions problem adds
  `--retain-permissions` to force the behaviour on, sees no change, and concludes permissions are
  broken. And if a future change flips a default, the positive flag will silently fail to restore it.
  A config-generator emitting both halves gets the negative silently.
- **Evidence:** `main.rs:605` (`opts.retain_permissions = !a.no_retain_permissions;` — `a.retain_permissions`
  is never referenced), `:613-614` (same shape). `options.rs:97-100` confirms the defaults.
  `commands_spec.rs` never passes any of the three positives.
- **Recommendation:** Use clap's `#[arg(long, overrides_with = "no_x")]` pattern (or
  `ArgAction::SetTrue` on both with a resolved accessor) so the last flag on the line wins, matching
  kingpin, and add `conflicts_with` only if last-wins is not the golongtail behaviour. Delete the
  unread fields either way so the compiler enforces it.
- **Tradeoff / risk:** Last-wins vs conflict-error is a parity question the oracle cannot settle
  (Open questions). Last-wins is the safer guess for a `--[no-]` CLI.
- **Effort:** S
- **Regression test to add:** `commands_spec.rs` — assert `--no-retain-permissions --retain-permissions`
  produces the documented resolution, and that the positive alone is honoured.

### `OPS-19` — the path filter is 13.7% covered and a leading `**` excludes everything
**P2** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/path_filter.rs:23-49`;
  `target/review-evidence/15-coverage/summary.txt` (`path_filter.rs` — 13.71% region, 25.00% line,
  the worst figure in the slice by a wide margin)
- **What:** `split_regexes` splits on `**` and compiles each piece. A leading `**` yields the empty
  piece `&s[0..0]`, and `Regex::new("")` succeeds and matches every input. Combined with
  exclude-wins-over-include (`:89-98`), `--exclude-filter-regex "**foo"` excludes the entire tree.
  There is no CLI-level test of either filter flag.
- **Failure scenario:** A pipeline writes `--exclude-filter-regex "**/temp/**"` — the intuitive
  spelling for someone thinking in glob syntax, and the `**` separator makes it look right. The
  leading `**` compiles the empty regex, every asset is excluded from the target scan, the target
  index comes back **empty**, and `create_version_diff(empty, source)` marks every asset as added —
  so downsync re-downloads and rewrites the entire version on every run, and no asset is ever
  deleted. Silent, expensive, and looks like a caching bug.
- **Evidence:** `path_filter.rs:36` (`let piece = &s[start..i - 1];` → `""` when `start == 0` and
  `i == 1`), `:51-55` (`Regex::new("")` is not rejected), `:99-101` (empty include list means
  include-all, so the failure is asymmetric between the two flags). Coverage artifact as cited. The
  flag-sweep found zero `commands_spec.rs` tests for `--include-filter-regex` or
  `--exclude-filter-regex`.
- **Recommendation:** Reject an empty piece in `compile` with a message naming the likely cause
  ("empty regex between `**` separators"), and add a table-driven unit test over `split_regexes`
  (leading/trailing/doubled separators, `\*` escaping, multi-byte input) plus one CLI test per flag.
- **Tradeoff / risk:** `COMPAT-RISK`: Go's `regexp.MustCompile("")` also matches everything, so
  rejecting the empty piece diverges from golongtail on input that is almost certainly a typo. If
  parity must hold, keep the behaviour and add the tests — but then the doc must say that a leading
  `**` excludes everything.
- **Effort:** S
- **Regression test to add:** as described; the unit tests alone would lift this file out of last
  place on coverage.

### `OPS-20` — the delete retry loop has no backoff
**P3** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/apply.rs:426-462`
- **What:** `delete_assets` makes exactly ten passes over the remaining removals with no sleep
  between them. Its documented purpose — "lets a dir be removed after its children succeed" — is
  order-based, so ten immediate passes are correct for that. But the retry shape is also the only
  thing standing between the apply and a transient Windows sharing violation, and ten passes complete
  in microseconds.
- **Failure scenario:** On Windows, an antivirus or the search indexer holds a handle on an asset
  being removed. `remove_file` fails; the ten passes all execute inside the scanner's window; the
  last pass returns a hard error (`apply.rs:448-453`) and the whole downsync fails. A single 50 ms
  sleep between passes would ride it out. Note the same class applies to `create_file_sized`, which
  has no retry at all — a sharing violation there fails immediately.
- **Evidence:** `apply.rs:427` (`while retry > 0 && …`) with no `sleep`; `docs/rust-port.md:121`
  discusses sharing violations in the *lock-file* context, showing the risk is on the author's radar
  elsewhere.
- **Recommendation:** Add a short escalating sleep between passes (this function is sync and runs
  before the concurrent phase, so blocking is fine), and give `create_file_sized`'s caller in step 5b
  the same treatment on Windows.
- **Tradeoff / risk:** Adds up to a second of latency to a run that has undeletable assets. Negligible
  against the alternative of a failed download.
- **Effort:** S
- **Regression test to add:** none practical without a Windows host; assert the backoff exists via a
  unit test with an injected clock if the retry logic is extracted.

---

## Lower-priority observations

- **Path traversal — R6's finding, and yes, the guard belongs here.** `root.join(rel_path)` appears
  unguarded at `fs_util.rs:104, 110, 116, 133, 150, 191, 240`, with no `is_absolute` or
  `Component::ParentDir` check anywhere in `crates/longtail/src` or
  `longtail-core/src/version_index.rs` (verified by grep). The right home is a single
  `fn resolve(root, rel) -> Result<PathBuf>` in `fs_util.rs` that all seven sites call — not a check
  at each call site, and not in `longtail-core` (which is I/O-free and has no notion of a root).
  Filing as PLAUSIBLE per the mission; **R6 owns adjudication**. Worth noting from my side that
  deletes run *first* (`apply.rs:89`), so an escaping path in `current` deletes outside the root
  before anything else happens, and `current` can come from an attacker-controlled
  `--target-index-path` as well as from the remote `.lvi`.
- `main.rs:509` prints "partial **download** left in place" for a cancelled `upsync` too — the arm is
  shared by every subcommand.
- `write_at`'s Windows branch does not retry `ErrorKind::Interrupted` while the Unix branch does
  (via std's `write_all_at`) — `fs_util.rs:172`. Unreachable on Windows files in practice.
- `crates/longtail-cli/src/progress.rs:136` and `:190` — `.expect("progress mutex poisoned")`. Both
  are in the non-TTY arm only; the realistic poisoner is `eprintln!` panicking on a closed stderr
  (`:172`, `:194`), i.e. a CI job whose log pipe went away. The panic then goes to that same dead
  stderr, so the only signal is exit 101. `unwrap_or_else(|e| e.into_inner())` would degrade
  gracefully.
- `enable_file_mapping` is a documented no-op on six subcommands but the no-op is visible only in
  rustdoc (`options.rs:59`) — a CLI user gets no hint.
- `upsync` accepts `--use-legacy-write`, which golongtail's `upsync` does not have (oracle
  `:344-410`); it errors at `upsync.rs:41`. Harmless, but it is an *extra* flag in a parity CLI.
- `create-version-store-index --version-local-store-index-path` is required in Rust (`main.rs:317-318`)
  and optional in golongtail (absent from the oracle's usage line at `:522`).
- `put --target-path s3://bucket` (no key) splits at the scheme's own slash
  (`put.rs:83`, `rfind(['/', '\\'])`), yielding `storage_uri = "s3://store"` — a different bucket.
  Degenerate input; cited as cmd_put.go:64-70 parity.
- `prune-store-index`'s CLI output computes `r.old_block_count - r.new_block_count` on `u32`
  (`main.rs:857`). Safe today because `StoreIndex::prune` only removes, but `saturating_sub` costs
  nothing.
- `--include-filter-regex` on downsync filters the *target scan*, not the write plan, so an excluded
  asset is seen as absent and therefore re-downloaded every run. That is golongtail's semantics
  (oracle `:152` "for assets in --target-path") but it is surprising enough to deserve a sentence in
  the help text.
- `byte_count_decimal` is applied to *counts* at `main.rs:950`, `:954`, `:1185` ("Block Count: 1234
  (1.2 k)"). Cited as stats.go parity; leaving alone.
- `delete_uri` (`fs_util.rs:420-452`) and `CloneStoreOptions::source_zip_paths` are both dead — the
  zip fallback they exist for is documented as deferred, but the CLI still accepts
  `--source-zip-paths` (`main.rs:393-394`) and silently ignores it.

## Comments & documentation issues

### `OPS-DOC-01` — golongtail's prune CAUTION is dropped and the prune flags have no help text
**P2** · CONFIRMED · `crates/longtail-cli/src/main.rs:66-71`, `:333-340`, `:353-360`, `:373-374`
- All three golongtail prune commands carry "CAUTION! Running uploads to a store that is being
  pruned may cause loss of the uploaded data" in their help (oracle `:562-564`, `:610`, `:665-667`).
  The Rust `--help` reduces this to one neutral line each. Worse, `--dry-run`,
  `--write-version-local-store-index`, `--validate-versions`, and `--skip-invalid-versions` have no
  doc comments, so clap renders them with **no help text at all**, against golongtail's explicit
  descriptions (oracle `:597-607`) — including the one that matters most:
  `--skip-invalid-versions` means "disregard its blocks", i.e. *delete that version's data*
  (`prune.rs:96-99`).
- **Recommendation:** Copy golongtail's CAUTION into the three `///` command docs and give every
  prune flag a doc comment, with `--skip-invalid-versions` spelling out that skipping makes the
  version's blocks prune-eligible. Effort: S.

### `OPS-DOC-02` — the roadmap proposes an optimization that breaks an unwritten invariant
**P1** · CONFIRMED · `docs/rust-port.md` §Roadmap ("Incremental-scan redesign")
- "The fix is a streaming and/or mtime/size-short-circuiting target scan (as golongtail does)" is
  stated with no mention of the resumability invariant it interacts with. A torn file has exactly the
  desired size, so size is not weak evidence of completeness — it is inverted evidence. Add the two
  invariants from Named deliverable 1 as an explicit precondition on the item. Co-signed with **R4**.
  Effort: S.

### `OPS-DOC-03` — "Logging is `tracing`-based" is not true of the facade
**P2** · CONFIRMED · `CLAUDE.md` §Runtime configuration; `crates/longtail-cli/src/main.rs:475-477`
- `CLAUDE.md` states logging is tracing-based. `rg 'tracing::|info!|debug!|warn!|error!|trace!|span!|instrument'`
  over `crates/longtail/src/` returns **nothing**, and `07b-machete.txt` (exit 1 = findings)
  independently reports `tracing` as an **unused dependency of `crates/longtail`**. The only emitters
  in the default workspace are four sites in `crates/longtail-store/src/cache.rs`. `main.rs:475-477`
  goes further and claims library logs include "retries" — no retry logging exists anywhere.
- **Recommendation:** Either instrument the facade (the phase boundaries in `downsync.rs:97-220` are
  the obvious spans, and would make `--log-level=info` useful for support) or drop the dependency and
  correct both claims. Do not leave the dependency declared and unused — it makes the claim look
  satisfied. Effort: S either way.

### `OPS-DOC-04` — a declared divergence from cited C is not in the divergences section
**P2** · CONFIRMED · `crates/longtail/src/apply.rs:407-410`
- The comment says "Files that persist after the last pass are a hard error; dirs that persist are
  left (Go tolerates)". C does the opposite for dirs — `longtail.c:7857-7867` hard-errors when a
  directory survives the retries. The Rust behaviour is deliberate and honestly commented, but the
  justification rests on "(Go tolerates)", which is unverifiable from this repo (no golongtail source
  vendored, and the surrounding citations name Go files by line elsewhere). Per the contract, an
  undocumented divergence is a finding: it is absent from `docs/rust-port.md` §"Deliberate
  divergences from C/Go".
- **Recommendation:** Add it to that section with the golongtail file:line, or vendor/pin the Go
  snippet the claim rests on. Every other C citation in `apply.rs` is line-exact against the
  vendored source (checked: `:1`, `:79`, `:83`, `:86`, `:91`, `:131`, `:144`, `:160`, `:266`, `:275`,
  `:286`, `:389` — the last is off by nine lines, pointing at the enclosing loop rather than the
  `PutUnique` call), so this is the one weak link in an otherwise very checkable set. Effort: S.

### `OPS-DOC-05` — the largest CLI-compat gap is undocumented, and the Windows permission consequence is unstated
**P2** · CONFIRMED · `docs/rust-port.md` §Dropped/§Deferred; `docs/format-spec.md:563-580`
- §Deferred names `pack`/`unpack` and the clone-store zip fallback, but not the nine missing
  subcommand aliases, the missing `version` subcommand, or the eight missing global flags (OPS-06) —
  which together are the biggest drop-in-compatibility gap in the slice.
  `docs/switchover-checklist.md:18-19` actively asserts the opposite — "Global flags are identical in
  name/shape: `--worker-count`, `--remote-worker-count`, `--log-level`, `--show-stats`, and
  per-command `--s3-endpoint-resolver-uri`" — of which the first half is true only of the four flags
  that were kept, and the second half is falsified outright by OPS-07 (the per-command flag is
  stranded on eight subcommands, four of which honour it for the store but not the index).
  Separately,
  `format-spec.md` §563-580 correctly documents the Windows permission mapping as accepted upstream
  behaviour, but nowhere is the *consequence* recorded: with `retain_permissions` defaulting to true,
  a `0600` asset becomes locally readable on Windows, and a Windows-authored version marks every
  writable file `0666` when downsynced onto Linux.
- **Recommendation:** Add the CLI-surface gaps to §Dropped/§Deferred (they are decisions, not
  oversights, once written down), and add two sentences to `format-spec.md` §Windows mapping on the
  security consequence of the round trip. Effort: S.

## Hardening backlog

Ranked by value per unit of effort.

1. **Resume test under default options** (OPS-01) — the single highest-value test in this slice, and
   the gate for the planned incremental-scan work. Assert the cache index is absent after a cancel
   and that the resumed tree matches byte-for-byte.
2. **Empty-keep-set refusal + test** (OPS-03) — three lines of code, removes the only
   whole-store-destruction path.
3. **Pre-apply version-index validation** (OPS-08, OPS-09, OPS-13) — one function, three findings:
   duplicate paths, case-insensitive duplicates, Windows-illegal names. All testable on Linux, which
   is where the six `#![cfg(unix)]` test files live.
4. **Promote `debug_assert_eq!` and bounds-check the payload slice** (OPS-12) — mechanical, and turns
   a silent release-build corruption into a typed error.
5. **Read-only-asset rewrite test** (OPS-02) — a five-line `chmod` in `downsync_e2e.rs` covers a
   failure that will otherwise reach production.
6. **Windows CI coverage for the apply path** — at minimum, un-`cfg(unix)` `downsync_e2e.rs` and
   `commands_spec.rs` with `mask_mode = true` on the manifest comparison (`TreeManifest::compare`
   already supports it, `tree_manifest.rs:89`) and permissions assertions gated. R8 owns the lane;
   the code change is here.
7. **`path_filter.rs` unit tests** (OPS-19) — a table-driven test over `split_regexes` moves the
   worst-covered file in the slice (13.71%) to respectable in an hour.
8. **Exit-code table + a SIGINT test** (OPS-16) — 130 currently has zero coverage on either path.
9. **Golden-output tests for the read-only commands** — `ls`, `print-version`, `print-store`,
   `print-version-usage`, `dump-version-assets` are all consumed by pipelines and none has a
   byte-for-byte assertion; every stdout check in `commands_spec.rs` is a `contains(...)` on one or
   two labels, so column widths, line order, and numeric formats can all drift silently.
10. **Proptest for `ls_entries`/`details_string`** (`main.rs:1051`, `:1079`) — pure functions over a
    `VersionIndex`, trivially proptestable, currently covered only by substring assertions.
11. **A fuzz target over `get`'s config parsing** (`get.rs:33-84`) — the one place the CLI parses
    untrusted structured input outside the on-disk formats. `cargo-fuzz` is installed with no targets
    (per the MANIFEST).

## Verified good

Things I traced and found correct — don't redo these.

- **Apply's phase order matches C exactly**: mkdir (`apply.rs:79`, longtail.c:8763) → preflight
  (`:83`, :8780) → deletes-first (`:89`, :8787/:7758) → plan build (`:91`, :8587) → zero assets
  (`:131`, :8292) → pre-create+truncate (`:144`, concurrentchunkwrite.c:108) → concurrent positional
  writes (`:160`, :8347) → permissions last (`:266`, :8900), with added dirs excluded (`:275`,
  :7995). All eight citations verified against the vendored C. The 6→7 boundary is enforced by the
  `JoinSet` drain (`:248-252`), not by convention.
- **Concurrent writes are range-disjoint for well-formed input**, on the three grounds the module
  states plus the unstated fourth (OPS-13): per-asset offsets strictly increase (`:127`), the
  added/content-modified sets are disjoint by construction of the diff merge, and each chunk
  occurrence maps to exactly one block via first-wins (`:401`). The permuted-completion-order test
  (`:805-867`) is a genuine proof-by-construction, and the first-touch assertion inside every mock
  fetch (`:545-559`, test at `:881-911`) pins the 5b-before-6 ordering machine-checkably. This is the
  best-tested part of the slice.
- **First-touch truncation happens once, serially, before any concurrency** — not lazily, not per
  block, no lock needed (`apply.rs:144-158`; workers reopen without truncation via `open_for_write`,
  `fs_util.rs:149-155`).
- **`write_at`'s partial-write loop is correct**: it advances both the slice and the offset
  (`fs_util.rs:171`) and cannot spin on a zero-byte success (the `WriteZero` guard at `:173-178`).
- **File↔directory type changes at the same path are safe** because dir paths carry a trailing `/`
  and therefore a different path hash, making the change a remove-plus-add — and deletes run first.
- **`--dry-run` is complete across all three prune commands.** Every mutation is gated:
  `prune.rs:104-106` (the `.lsi` rewrite), `:209-215` (the early return, before the ReadWrite store
  is even opened), `:332` (the index write-back), `:425` (the delete loop). The gather pass uses
  `AccessType::ReadOnly`, so it does not create lock files. I looked specifically for a mutation that
  survives dry-run and found none.
- **`prune-store` writes the pruned index before deleting blocks**, so a crash between them leaves
  harmless orphans rather than dangling index entries — enforced in the store layer, cited at
  `prune.rs:1-3`. (A worker flagged a lockless-S3 stale-shard hazard that would weaken this;
  it lives in `crates/longtail-store/src/sync.rs` and is **R3's**, cross-referenced not duplicated.)
- **`get`'s config handling is defensive and matches its comments**: unknown keys ignored, only
  `storage-uri` + `source-path` required, all configs must agree on `storage-uri` (`get.rs:53-61`),
  `.lsi` paths dropped wholesale unless every config supplies one (`:87-89`), and the
  accepted-but-ignored `--version-local-store-index-path` is explained in place (`:94-95`) rather
  than left mysterious.
- **`put`'s path defaulting** (`put.rs:99-140`) matches its cmd_put.go citations, and the
  `--no-version-local-store-index` + explicit-path conflict is a real error rather than a silent
  precedence rule.
- **`split_regexes`' byte-index arithmetic is safe** despite slicing a `&str` by byte offsets
  (`path_filter.rs:36`): both `start` and `i - 1` always land on an ASCII `*` or `0`, and UTF-8
  continuation bytes can never equal `b'*'`. (The empty-piece problem is OPS-19; the slicing is fine.)
- **Progress on a non-TTY does not spam.** `CliProgress::new` checks `is_terminal()`
  (`progress.rs:80`) and the `Plain` arm emits at most one line per decile plus one per phase — no
  carriage returns, CI-log-safe. `finish(false)` calls `bar.abandon()` (`:113`) so a cancelled or
  failed run leaves the bar frozen at its honest position with a newline, and the `error:`/`cancelled`
  line prints cleanly below. The only half-drawn case is the second-ctrl-c `process::exit(130)`
  (`main.rs:538`).
- **`--log-level` vs `RUST_LOG` precedence is correct and documented** (`main.rs:478-487`: `RUST_LOG`
  wins, `--log-level` is the fallback, matching the flag's own doc at `:34-36`).
- **`downsync_corrupt_target_index_cache`** (`commands_spec.rs:374-402`) is a well-aimed test: it
  proves the default-on cache is actually read and that a malformed one fails cleanly.

## Experiments requested

| # | Hypothesis | Exact command | What would change the finding |
|---|---|---|---|
| 1 | On a case-insensitive filesystem, a version index containing two assets whose paths differ only in case causes overlapping concurrent writes and a wrong tree (OPS-08). | On Windows (or macOS APFS case-insensitive): build a fixture store from a source tree containing `Data/x.bin` (300 KB, distinct bytes) and `data/y.bin`, then hand-edit the `.lvi` so the second asset's path is `data/x.bin`; run `cargo run -p longtail-cli -- downsync --storage-uri <store> --source-path <lvi> --target-path out --validate` and `dir /r out` (to reveal streams). | If downsync fails with a typed error, or `--validate` catches it, the finding drops to P3 (a diagnosis-quality issue). If it exits 0 with one file of mixed content, it is CONFIRMED at P1. |
| 2 | A Linux-authored `.lvi` containing an asset named `nul.txt`, `con`, or `saves:auto` silently writes to a device or an NTFS alternate data stream on Windows, and downsync exits 0 (OPS-09). | On Windows: same fixture approach with those three names; `cargo run -p longtail-cli -- downsync --storage-uri <store> --source-path <lvi> --target-path out`; then `dir out`, `dir /r out`, and `type out\nul.txt`. | If the open fails and the run errors out, the finding becomes P3 (improve the message). If it exits 0 with the file missing or the bytes in a stream, CONFIRMED at P1. |
| 3 | With `--target-path` omitted, the derived relative target root (`downsync.rs:243-253`) prevents Rust std from applying the `\\?\` prefix, so assets whose full path exceeds 260 chars fail on Windows even with long paths enabled. | On Windows with LongPathsEnabled=1: build a fixture whose deepest asset path is ~200 chars, `cd` into a directory ~120 chars deep, and run `cargo run -p longtail-cli -- downsync --storage-uri <store> --source-path <lvi>` (no `--target-path`), then repeat with an absolute `--target-path`. | If both succeed, no finding. If only the absolute form succeeds, file a P2: canonicalize the target root to an absolute path before apply. |
| 4 | `prune-store` with an empty `--source-paths` file deletes every block (OPS-03). | `printf '\n \n' > /tmp/empty.txt`; copy `fixtures/stores/default/store` to `/tmp/s`; `cargo run -p longtail-cli -- prune-store --storage-uri /tmp/s --source-paths /tmp/empty.txt`; then `find /tmp/s -name '*.lsb' | wc -l`. | A zero count confirms P0 empirically (it is already CONFIRMED by reading). A refusal would mean I misread a guard — recheck `prune.rs:188-232`. |
| 5 | A retained-permissions `0444` asset cannot be rewritten by a second downsync (OPS-02). | Downsync `zoo.lvi` into `out`; `chmod 444 out/<asset modified by the v2 fixture>`; downsync the v2 `.lvi` into the same `out`; observe the error. | If it succeeds, something un-protects the file that I did not find — recheck `apply.rs:144-158`. If it fails EACCES, CONFIRMED empirically. |

## Open questions for the maintainer

1. **Are the bool-flag defaults right?** The oracle cannot settle them (kingpin renders `--[no-]x`
   without a default). `options.rs:97-100` chooses `true` for `retain_permissions`, `scan_target`,
   and `cache_target_index` on the strength of a `cmd_downsync.go:120-135` citation. The
   `cache_target_index` default in particular is consequential: every `downsync`/`get` writes a
   hidden `.longtail.index.cache.lvi` into the user's target folder, and it is what makes OPS-11 a
   support ticket. Please confirm against the golongtail source.
2. **Does golongtail's `--[no-]x` resolve last-wins or as a conflict?** Determines the fix shape for
   OPS-18.
3. **Does golongtail's clone-store derive the `.lsi` path with a suffix replace or a full
   `strings.Replace`?** Determines whether OPS-14 is a port bug or inherited.
4. **Is `pack`/`unpack` genuinely out of scope for the switchover, or does a pipeline step use it?**
   `docs/rust-port.md` says deferred and `commands_spec.rs:1339-1349` holds `#[ignore]`d
   placeholders, so the intent is clear — but the missing **aliases** and `version` subcommand
   (OPS-06) look like oversights rather than decisions, and I'd like that confirmed before treating
   them as such.
5. **Is `--show-store-stats` needed by CI?** `DownsyncReport.store_stats` already carries the data, so
   implementing it is cheap — but only worth doing if something consumes it.
6. **Is `crates/longtail`'s `tracing` dependency intended to be used, or removed?** (OPS-DOC-03.)

## Files read

- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/apply.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/downsync.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/fs_util.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/get.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/put.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/prune.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/path_filter.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/clonestore.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/inspect.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/cp.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/version.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/lib.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/src/options.rs` (defaults only)
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-cli/src/main.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-cli/src/progress.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail/tests/smoke.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-cli/tests/commands_spec.rs` (targeted sections)
- `/home/chris/work/longtail-rs/cm/rust-port/support/longtail-testkit/src/tree_manifest.rs`
- `/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-core/src/block.rs:114-150` (secondary axis)
- `/home/chris/work/longtail-rs/cm/rust-port/support/longtail-sys/longtail/src/longtail.c` (secondary axis: 5300-5360, 5660-5690, 7758-7912, 8280-8300, 8755-8800)
- `/home/chris/work/longtail-rs/cm/rust-port/support/longtail-sys/longtail/lib/concurrentchunkwrite/longtail_concurrentchunkwrite.c:80-140` (secondary axis)
- `/home/chris/work/longtail-rs/cm/rust-port/docs/rust-port.md`, `CLAUDE.md`, `docs/format-spec.md` (grepped sections)
- `/home/chris/work/longtail-rs/cm/rust-port/target/review-evidence/`: `MANIFEST.md`, `REVIEWER-CONTRACT.md`, `14-golongtail-help.txt`, `15-coverage/summary.txt`, `12-loc.txt`, `07b-machete.txt`
