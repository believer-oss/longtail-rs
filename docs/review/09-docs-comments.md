# 09 · Documentation & comments review

- **Reviewed at:** `456274d` · **Lead model:** opus · **Workers:** 4 × fable
- **Slice:** cross-document consistency, the target shape of the four keeper docs, disposition of
  every non-keeper doc, the upstream-citation policy, and comment quality across the four crates.
- **Confidence:** covered well for `readme.md`, `CLAUDE.md`, `docs/rust-port.md`, the cross-document
  link graph, CLI/env-var/feature coverage, the citation census, and the comment metrics.
  **Covered thinly for `docs/format-spec.md`'s interior**: I read all 670 lines and verified its
  provenance block, §10, the `archive` claim, the `.lrb` gap and 9 of its 101 upstream citations, but
  the worker assigned to check its ~40 numeric constants and ~30 Rust-path references one by one did
  not return. That verification gap is the one deliverable that came in thinner than planned — see
  Open questions #5. Nothing in this document depends on it.

**ID convention.** Every finding in this review is a documentation finding. `DOCS-NN` under
`## Findings` = the four keeper docs, cross-document consistency, and doc infrastructure.
`DOCS-DOC-NN` under `## Comments & documentation issues` = in-source `//` / `///` / `//!` comments.
The index is ordered by priority, and the finding bodies follow it in the same order, so `DOCS-21`
(added after R4's input landed) sits among the P1s rather than at the end.

**Not re-derived here.** R1/R2/R3/R7 each filed `*-DOC-*` findings inside their own slices; I read all
34 and do not restate them. Where a defect of theirs *propagates* to a document they do not own, I
file the propagation and cross-reference the original. R6 and R4 landed after I began; two of their
results are folded in and attributed — **R6's `unsafe` inventory is authoritative over mine**
(DOCS-05), and R4's roadmap analysis is the substance of DOCS-21.

## Findings index

| ID | P | Dimension | One line | file:line | Verdict |
|---|---|---|---|---|---|
| DOCS-01 | P1 | complexity | 46 of 53 CLI flags have no keeper-doc coverage; 32 live only in a doc slated for deletion | `docs/switchover-checklist.md:23-44` | CONFIRMED |
| DOCS-02 | P1 | hardening | The switchover runbook — the operator's source of truth — is wrong in four independent ways | `docs/switchover-checklist.md:9,18,25-26,44` | CONFIRMED |
| DOCS-03 | P1 | hardening | `format-spec.md` cites a machine-local tree and points validation at a dead constant | `docs/format-spec.md:3-10` | CONFIRMED |
| DOCS-04 | P1 | hardening | Nothing runs `cargo doc`; 7 live rustdoc defects, and the pack's list of 4 under-reports | `.github/workflows/` (absence) | CONFIRMED |
| DOCS-21 | P1 | complexity | `rust-port.md`'s flagship roadmap item is half-shipped, measured in a non-shipping config, and proposes an unimplementable fix | `docs/rust-port.md:196-200` | CONFIRMED |
| DOCS-05 | P2 | hardening | The two keepers contradict each other on `forbid(unsafe_code)`: `CLAUDE.md` states a false universal, `rust-port.md` states it correctly | `CLAUDE.md:101` vs `docs/rust-port.md:208-223` | CONFIRMED |
| DOCS-06 | P2 | hardening | The false "downloads a prebuilt library" build story is in three more files nobody owns | `support/longtail-sys/README.md:3-6,16-17`, `support/longtail-ffi/README.md:12-13` | CONFIRMED |
| DOCS-07 | P2 | complexity | Two keepers tell the verification story with two unreconciled taxonomies; §10.3 claims a check that cannot run | `docs/format-spec.md:655-671` vs `docs/rust-port.md:84-96` | CONFIRMED |
| DOCS-08 | P2 | hardening | `put-path-memory.md` contradicts its own Resolution section and pins a stale self-SHA | `docs/put-path-memory.md:8-9,11,213-216` | CONFIRMED |
| DOCS-09 | P2 | complexity | The newest and most decision-relevant measurements are unreachable from every keeper | `docs/rust-port.md:7-8,109` | CONFIRMED |
| DOCS-10 | P2 | hardening | An `archive` cargo feature is named in 3 docs and 1 test and declared in no `Cargo.toml` | `docs/rust-port.md:189` | CONFIRMED |
| DOCS-11 | P2 | security | The four `LONGTAIL_TEST_S3_*` vars gating the S3 lane are documented in no markdown, and silently default | `crates/longtail-store/tests/s3_spec.rs:131-137` | CONFIRMED |
| DOCS-12 | P2 | hardening | `.lrb` is a compat-bearing on-disk layout absent from the authoritative format spec | `docs/format-spec.md:212-285` (gap) | CONFIRMED |
| DOCS-13 | P2 | complexity | Switchover-runbook rollback repeats a safety claim R3 proved false | `docs/switchover-checklist.md:203-206` | CONFIRMED |
| DOCS-14 | P3 | complexity | `CLAUDE.md`'s `docs/` listing omits two entries | `CLAUDE.md:42` | CONFIRMED |
| DOCS-15 | P3 | idiom | The `s3` and `vendored` cargo features are documented nowhere | `crates/longtail-store/Cargo.toml:7-16` | CONFIRMED |
| DOCS-16 | P3 | complexity | `rust-port.md` gives two different HPCDC ceilings with swapped causes | `docs/rust-port.md:138`, `:169` | CONFIRMED |
| DOCS-17 | P3 | complexity | `CLAUDE.md` recommends a lint command needing a C toolchain 20 lines after saying none is needed | `CLAUDE.md:69` vs `:46-48` | CONFIRMED |
| DOCS-18 | P3 | idiom | Root readme is lowercase while all three subdirectory READMEs are uppercase, and code hardcodes the uppercase form | `readme.md` vs `fixtures/README.md` | CONFIRMED |
| DOCS-19 | P3 | complexity | `rust-port.md:105` forward-references a section that does not exist | `docs/rust-port.md:105` | CONFIRMED |
| DOCS-20 | P3 | complexity | The July bench doc's verdict heading still says FAIL 158 lines after the doc supersedes it | `docs/bench-2026-07-05.md:328` | CONFIRMED |

## Scope

**Read in full:** `readme.md` · `CLAUDE.md` · `docs/rust-port.md` · `docs/format-spec.md` ·
`docs/put-path-memory.md` · `docs/switchover-checklist.md` · `docs/bench-2026-08-03.md` ·
`fixtures/README.md` · `support/longtail-sys/README.md` · `support/longtail-ffi/README.md` ·
the `## Comments & documentation issues` section of all four wave-1 review documents.

**Read in part:** `docs/bench-2026-07-05.md` (heading tree + §7 verdicts + §9 addendum) ·
`crates/longtail-cli/src/main.rs` (clap definitions, `:22-400`) ·
`crates/longtail/src/options.rs` · `crates/longtail-core/src/perms.rs` ·
`crates/longtail-core/src/{hash,chunker,error,compress}.rs` (module docs + cited constants) ·
`crates/longtail-store/src/cache.rs` (module doc + block-path helper) ·
`crates/longtail-store/src/{remote,sync}.rs` (module docs) ·
`.github/workflows/rust.yaml` (`formatting`, `miri` jobs) · `support/longtail-sys/build.rs`
(feature selection + `UPSTREAM_VERSION`) · `support/longtail-bench/{Cargo.toml,src/bin/e2e.rs}` ·
`support/longtail-testkit/tests/hash_recompute_golden.rs`, `chunker_differential.rs` ·
`support/longtail-sys/longtail/` (submodule, for citation spot-checks).

**Excluded:** the substance of every claim inside another reviewer's slice. Where I read their code
it was only to decide whether a *document* states it correctly. `docs/review/*` are inputs, not
targets.

## Verification performed

**Evidence-pack artifacts consulted:** `MANIFEST.md` (§"Pre-identified rustdoc defects", §"Exit-code
corrections") · `08-doc.txt`, `08b-doc-warnings.txt`, `08c-doc-fastcdc.txt` (the rustdoc inventory —
`08b` is the complete one; see DOCS-04) · `17-bloat.txt:278` (`zstd_sys` 321.1 KiB / 2.4%) ·
`12-loc.txt` (crate sizes) · `14-golongtail-help.txt` (flag oracle, via R7) ·
`00-scope.txt`, `13-fixtures.txt`.

**Computed by me** (not in the pack; commands given so they are reproducible):

| Measure | Value | Command |
|---|---|---|
| Distinct CLI long flags | 53 | derived from `#[arg(` + field name in `crates/longtail-cli/src/main.rs` |
| Flags with **no** keeper-doc coverage | **46 / 53** | flag list × grep over all non-review `.md` |
| Flags documented **only** in `switchover-checklist.md` | **32** | same |
| Flags documented **nowhere** | **10** | same |
| Line-numbered upstream citations, repo-wide | **341** on 329 lines, across **47** files | `rg -o '[A-Za-z_0-9]+\.(c\|h\|go):[0-9]+(-[0-9]+)?'` excluding `target/ .git/ docs/review/` and the submodule |
| — of which C/H | 221 (17 distinct files, all resolvable in the submodule) | same, `.c\|.h` only |
| — of which Go | **120** (22 distinct files, **0** resolvable — no Go source in-repo) | same, `.go` only |
| Inline commit pins | 14 occurrences, 3 distinct (`@96241fe`×9, `@49a20e1`×4, `@a459c3b`×1) | `rg -o '@[0-9a-f]{7,40}'`, same exclusions |
| Citations on a line that already names an upstream symbol | 132 (39%) | scripted, see DOCS-03 |
| `longtail-sys`/`longtail-ffi` references | 80 hits across 41 files (10 in the four keepers) | `rg -c`, same exclusions |
| `//!` lines, four crates | 564 across 47 files — **100% of files have one** | `grep -c '^//!'` per file |
| Public items needing a doc for `deny(missing_docs)` | **216** (core 39, store 30, longtail 147, cli 0) | worker sweep, method in DOCS-DOC-05; my narrower cross-check gave 193, same shape |

**Citation spot-check (9 of 101 in `format-spec.md`), against the in-repo submodule at `96241fe`:**
`longtail.c:2552` (`Longtail_GetVersionIndexDataSize`) ✓ · `:2606` (`InitVersionIndexFromData`) ✓ ·
`:2633` (version reject) ✓ exact · `:8913` ✓ · `:8979` ✓ · `:7307` ✓ exact
(`m_Tag = &store_index->m_BlockTags[block_chunk_index_offset]`) · `:9145` ✓ exact
(`[block_index]`) · `hpcdcchunker.c:12` (`ChunkerWindowSize 48u`) ✓ · `:126-129` (discriminator) ✓.
**All nine land within ±2 lines of the claim.** The C citations are *currently accurate* — the defect
in DOCS-03 is that nothing preserves that, and that the doc names a tree outside the repo as its base.

**Could not verify:** the 120 Go citations (`@49a20e1`) — golongtail source is not vendored
(`find . -name '*.go'` outside `target/` returns nothing); the pinned v0.4.5 binary the repo fetches
is not source. Anything requiring `cargo` (banned). The cold-CI cost of `--workspace` clippy
(R8's, and the MANIFEST warns the warm 13 s figure is not it).

---

## Findings

### `DOCS-01` — 46 of 53 CLI flags have no keeper-doc coverage; 32 exist only in a doc slated for deletion

**P1** · `complexity` · CONFIRMED

- **Where:** `docs/switchover-checklist.md:23-44` (the flag-mapping table); flag definitions at
  `crates/longtail-cli/src/main.rs:29-41` (globals) and `:85-400` (per-subcommand).
- **What:** The CLI declares **53** distinct long flags. Across every non-review `.md` in the repo:
  - **7** appear in a keeper (`--cache-path`, `--source-path`, `--storage-uri`, `--target-path` in
    `readme.md`; `--cache-size-limit`, `--dry-run` in `docs/rust-port.md`; `--enable-file-mapping` in
    `docs/format-spec.md`);
  - **32** appear *only* in `docs/switchover-checklist.md` — including every `upsync`/`put` tuning
    knob (`--target-chunk-size`, `--target-block-size`, `--max-chunks-per-block`,
    `--min-block-usage-percent`, `--compression-algorithm`, `--hash-algorithm`), every `prune-*`
    flag, `--s3-endpoint-resolver-uri`, `--log-level`, `--show-stats`, and `--use-legacy-write`;
  - **4** appear only in a bench doc;
  - **10** appear in **no document at all**: `--no-stalled-stream-protection`, `--retain-permissions`,
    `--no-retain-permissions`, `--scan-target`, `--no-scan-target`, `--cache-target-index`,
    `--include-filter-regex`, `--target-index-path`, `--source-s3-endpoint-resolver-uri`,
    `--target-s3-endpoint-resolver-uri`.
  Two of those ten are asymmetries that read as oversights, not decisions: `--exclude-filter-regex`
  is in the table but `--include-filter-regex` is not; `--no-cache-target-index` is in the table but
  `--cache-target-index` is not.
- **Failure scenario:** The brief for this review states that only `readme.md`,
  `docs/format-spec.md`, `docs/rust-port.md` and `CLAUDE.md` survive. Executing that plan deletes the
  only written description of 36 of the CLI's 53 flags. The CI/CD pipeline is one of the two named
  consumers; the first person to ask "what does `--min-block-usage-percent` default to and does it
  match golongtail?" after the deletion has to read `main.rs`. `--no-stalled-stream-protection` is
  the newest flag on the branch (`456274d`, the HEAD commit) and changes S3 read behaviour under a
  stalled connection on four subcommands — exactly the flag an operator reaches for during an
  incident, and it is written down nowhere.
- **Evidence:** the flag/doc cross-product in Verification; `crates/longtail-cli/src/main.rs:94,148,
  207,230` (`no_stalled_stream_protection` on `downsync`/`get`/`validate-version`/`upsync`).
- **Recommendation:** Do not solve this with a document. Flags belong in `--help`, which is
  executable and cannot drift: give every `#[arg]` a doc comment (R7's `OPS-DOC-01` already found the
  `prune-*` flags have none), and add a test that asserts every flag has non-empty help text. Then
  `docs/rust-port.md` needs only the **deltas** from golongtail — see the new §"CLI compatibility with
  golongtail" in the target outline, and the disposition of `switchover-checklist.md`.
- **Tradeoff / risk:** Help text is not a flag-by-flag *mapping* to golongtail; the mapping table's
  value is that it asserts equivalence. That assertion belongs in the new rust-port.md section, and it
  needs R7's OPS-06 corrections before it is true.
- **Effort:** M (53 doc comments + one table-driven test + one new doc section)
- **Regression test to add:** `crates/longtail-cli/tests/commands_spec.rs` — walk the clap `Command`
  tree and assert every argument has a non-empty `help`. A new flag then cannot land undocumented.

### `DOCS-02` — the switchover runbook, the operator's source of truth, is wrong in four independent ways

**P1** · `hardening` · CONFIRMED

- **Where:** `docs/switchover-checklist.md:9`, `:18-19`, `:25-26`, `:44`, `:196`.
- **What:** This document is the artifact a human follows to flip production. Four of its claims are
  false as of `456274d`:
  1. `:18-19` — *"Global flags are identical in name/shape"* and `:25-26` *"The Rust flag names are
     the golongtail v0.4.5 names verbatim; only the binary changes."* R7's `OPS-06` (CONFIRMED)
     establishes nine missing subcommand aliases, a missing `version` subcommand, and eight missing
     globals against the `14-golongtail-help.txt` oracle. A pipeline step spelled `longtail stats …`
     or passing `--log-file-path` exits 2.
  2. `:44`, `:196` — *"the `archive` feature, not yet implemented"* / *"until the archive feature
     lands"*. There is no `archive` feature (DOCS-10). The sentence tells an operator to wait for
     something that is not on any switch.
  3. The table omits **all ten** flags from DOCS-01, including `--no-stalled-stream-protection`,
     which post-dates the document by a month.
  4. `:203-206` repeats a safety claim R3 disproved — filed separately as DOCS-13.
- **Failure scenario:** The document's own §Sign-off table (`:211-223`) is empty, so it has not been
  executed. It will be, by a human, at switchover. Steps 1-9 are individually sound; the *mapping*
  table is what a pipeline author edits their YAML from, and following it produces `exit 2` on any
  step that used an alias.
- **Evidence:** `docs/review/07-operations-cli.md` `OPS-06`; `target/review-evidence/14-golongtail-help.txt:11-26,38-96`;
  `grep -rn 'alias' crates/longtail-cli/src/main.rs` → nothing; the feature scan in DOCS-10.
- **Recommendation:** Do not repair this document in place — the mapping table cannot be kept true by
  hand (DOCS-01 shows it already drifted by ten flags in one month). Land R7's OPS-06 aliases first,
  then move the *divergences* (`:46-67`) into `docs/rust-port.md` §"CLI compatibility with golongtail"
  and let `--help` carry the flags. What remains is the staging runbook + sign-off table, which is a
  one-shot artifact — see Disposition.
- **Tradeoff / risk:** Splitting the document means the operator follows two things at switchover.
  Mitigate by keeping the runbook as the entry point and having it link the rust-port.md section.
- **Effort:** M
- **Regression test to add:** R7's OPS-06 test (every alias + global through `--help`, exit 0) is the
  gate that makes claim 1 true and keeps it true.

### `DOCS-03` — `format-spec.md` cites a machine-local tree and points citation validation at a dead constant

**P1** · `hardening` · CONFIRMED

- **Where:** `docs/format-spec.md:3-10`.
- **What:** The provenance block reads: *"Verified line-by-line against upstream C source at
  `~/github/longtail` (commit `96241fe`, …) and `~/github/golongtail` (commit `49a20e1`, …) … this
  repo pins a specific upstream release via `UPSTREAM_VERSION` in `support/longtail-sys/build.rs` —
  check that version's tag/commit matches before trusting citations blindly."* Three problems, in
  ascending order of consequence:
  1. `~/github/longtail` is a path in the author's home directory. It happens to exist on this
     machine; it exists nowhere else. Meanwhile the repo **does** carry the exact pinned tree:
     `git submodule status` reports
     `96241fe2fe6a92602efce57b0c1d3a0964d4a90e support/longtail-sys/longtail (v0.3.3-101-g96241fe)`.
     The spec names an unreachable base while an authoritative in-repo one sits unmentioned.
  2. The self-check instruction is broken. `UPSTREAM_VERSION = "v0.4.3"`
     (`support/longtail-sys/build.rs:13`) sits on the download path, which
     `default = ["vendored"]` (`support/longtail-sys/Cargo.toml:22`) makes dead code — R2's
     `ALG-DOC-01`, and R2's `ALG-10` records the version skew. A reader following the instruction
     compares the submodule's `v0.3.3-101` against a `v0.4.3` that is never fetched, gets a mismatch,
     and has no way to tell whether the citations are stale.
  3. **120 of the 341** line-numbered citations repo-wide are Go (`@49a20e1`), and golongtail source
     is not vendored at all. They are unverifiable by anyone without the author's clone.
- **Failure scenario:** A maintainer in a year opens the spec to answer "does the C reader reject a
  trailing byte?", follows `longtail.c:2663`, finds unrelated code because upstream moved, and cannot
  tell whether the *claim* is stale or only the *line number*. Because the spec is the authoritative
  reference for the paramount constraint, that ambiguity is expensive: the safe response is to
  re-derive the whole section. This is not hypothetical decay — three of the four keepers were last
  touched on Jul 6 while 21 commits landed through Aug 4, and nothing signalled it.
- **Evidence:** `git submodule status`; `support/longtail-sys/Cargo.toml:21-23`;
  `support/longtail-sys/build.rs:13,508-515`; the citation census in Verification. **My nine
  spot-checks all resolved exactly** — the citations are accurate *today*; the defect is that nothing
  keeps them so and the stated base is wrong.
- **Recommendation:** See §"Upstream citation policy" below for the full proposal and its price. The
  minimum here: replace `~/github/longtail` with `support/longtail-sys/longtail` (in-repo, pinned,
  greppable), delete the `UPSTREAM_VERSION` instruction, and state plainly that golongtail citations
  have no in-repo base.
- **Tradeoff / risk:** If R2's Option A (delete the legacy crates) is chosen, the submodule goes with
  them and the C base becomes unreachable too. That is an argument for making the citation base an
  explicit, separately-decided artifact — see the policy section.
- **Effort:** S for the header; M for the policy migration.
- **Regression test to add:** an `xtask check-citations` that resolves every
  `<file>.c:<line>` in `docs/` and `crates/` against the submodule and fails on a miss. See the
  policy section for why the *symbol* form is the better target and this the fallback.

### `DOCS-04` — nothing runs `cargo doc`; there are seven live rustdoc defects, and the pack's list of four under-reports

**P1** · `hardening` · CONFIRMED

- **Where:** `.github/workflows/` (absence);
  `crates/longtail-core/src/chunker.rs:152`, `crates/longtail-core/src/store_index.rs:321`,
  `crates/longtail-store/src/remote.rs:9`, `crates/longtail-store/src/sync.rs:326`, `:368`,
  `support/longtail-testkit/src/lib.rs:5`, `support/longtail-bench/src/bin/merge_mem.rs:5`,
  `crates/longtail-cli/Cargo.toml:6-8`.
- **What:** `rg 'cargo doc|rustdoc|RUSTDOCFLAGS' .github/ xtask/ test-data/ Cargo.toml crates/*/Cargo.toml`
  returns nothing. The MANIFEST lists four defects from `08-doc.txt`; that run used
  `RUSTDOCFLAGS=-D warnings`, so `longtail-core` and `longtail-testkit` **errored out and
  `longtail-store` and `longtail-bench` were never documented at all**. The complete inventory is in
  `08b-doc-warnings.txt` and is **seven** broken/private links plus the output collision:

  | # | Defect | Site |
  |---|---|---|
  | 1 | unresolved link `FastCdcChunker` (feature-gated) | `longtail-core/src/chunker.rs:152` |
  | 2 | public doc → private `Self::is_canonical` | `longtail-core/src/store_index.rs:321` |
  | 3 | unresolved link `differential` (feature-gated) | `longtail-testkit/src/lib.rs:5` |
  | 4 | unresolved link `StoreIndex::merge` | `longtail-bench/src/bin/merge_mem.rs:5` |
  | 5 | unresolved link `BlobClient` | `longtail-store/src/remote.rs:9` |
  | 6 | public doc → private `read_store_store_index_with_items` | `longtail-store/src/sync.rs:326` |
  | 7 | public doc → private `try_overwrite` | `longtail-store/src/sync.rs:368` |
  | — | output filename collision `target/doc/longtail/index.html` | bin `longtail` vs lib `longtail` |

  `08c-doc-fastcdc.txt` confirms #1 is purely feature-gating: under `--features fastcdc` only #2
  remains.
- **Failure scenario:** Three of the four defects that matter are in the *facade and store* crates — the
  API a Tauri consumer reads. `remote.rs:9`'s dead `BlobClient` link is in the module doc of the
  actor that R3 spent ten findings on; `sync.rs:326`/`:368` are the two public store-index entry
  points, and their docs point at helpers a consumer cannot see. A published doc build of this
  workspace fails outright under `-D warnings` and silently drops half the crates without it. Because
  no job runs it, defect count only goes up.
- **Evidence:** `target/review-evidence/08b-doc-warnings.txt:3,14,23,32,45,77,86,95`;
  `08-doc.txt:219-252` (the truncated run); `08c-doc-fastcdc.txt`; `MANIFEST.md:70-82`.
- **Recommendation:** Add a `docs` job to `rust.yaml` running
  `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features` (all-features is what
  makes #1 and #3 resolvable). Fix the collision with `doc = false` on
  `crates/longtail-cli/Cargo.toml`'s `[[bin]]` — **do not rename the binary**: `name = "longtail"` is
  the golongtail drop-in contract and renaming it breaks every pipeline step.
- **Tradeoff / risk:** `--all-features` pulls `differential`, which needs the C toolchain + submodule
  (R8 prices this). If that is unacceptable per-PR, run the default-feature build in CI and accept #1
  and #3 as `#[cfg_attr(not(feature = …), doc = "…")]`-guarded, or drop the two links to plain code
  spans. The collision fix and defects #2, #4-#7 are feature-independent and cost nothing.
- **Effort:** S
- **Regression test to add:** the CI job itself is the test.

### `DOCS-21` — `rust-port.md`'s flagship roadmap item is half-shipped, measured in a configuration that does not ship, and proposes an unimplementable fix

**P1** · `complexity` · CONFIRMED · *substance from R4; folded in here because the defect is in a keeper doc*

- **Where:** `docs/rust-port.md:196-200` (§Roadmap, first bullet) and `:108` (the §Performance
  sentence that forward-references it).
- **What:** The bullet reads: *"**Incremental-scan redesign (the biggest win).** Incremental
  downloads are dominated by the target scan: at 1 GiB, building the target index takes ≈ 1434 ms
  versus ≈ 37 ms to apply — the scan is ~97% of the wall. It is worth ≈ 330 ms (~70%) of the 384 MiB
  incremental cell (≈ 1.4 s at 1 GiB) and ≈ 250 MiB of peak RSS. The fix is a streaming and/or
  mtime/size-short-circuiting target scan (as golongtail does)."* Three independent problems, per R4:
  1. **Half of it shipped.** The "streaming" half landed in `c13a4d1` — `chunk_asset_streaming`
     (`crates/longtail/src/version.rs:65`, defined `:86-96`) reads one `max_hash_size` part at a time
     instead of the whole asset, and `docs/bench-2026-08-03.md:94-111` measures the result (512 MiB
     asset: peak 512 → 50 MB, "independent of asset size"). The roadmap still lists it as future work.
  2. **The numbers describe a configuration that is not the shipping one.** They were measured with
     `--no-cache-target-index`, so they characterise the cost of always rebuilding the target index —
     not what a default `downsync`/`get` does. Quoting them as the size of the remaining prize
     overstates it.
  3. **The proposed remedy cannot be built, and its attribution is unsupported.** An mtime/size
     short-circuit needs a per-asset timestamp to compare against, and `VersionIndex` has no
     timestamp field — `docs/format-spec.md:68-70` records this explicitly: the
     `m_CreationDates`/`m_ModificationDates` pointers in `struct Longtail_VersionIndex`
     (`longtail.h:1868-1869`) are **commented out** and "are NOT part of the v0.0.2 on-disk format".
     And "(as golongtail does)" is not true: neither the pinned C source nor golongtail at
     `49a20e1` contains any `mtime`/`ModTime` reference (verified by the orchestrator). The size half
     of the short-circuit is implementable; the mtime half is not, and the sentence presents them as
     one option.
- **Failure scenario:** This is the item labelled "**the biggest win**" in the roadmap of the
  document `readme.md` and `CLAUDE.md` both name as the place to start. Someone picking it up spends
  their first day rediscovering that half of it is already merged, then designs against a
  `VersionIndex` field that does not exist, then goes looking in golongtail for the prior art the
  parenthetical promises and finds none. The measured prize they were chasing was taken in a
  non-default configuration, so even the sizing that justified the work is wrong. Every one of those
  three dead ends is avoidable by editing five lines.
- **Evidence:** `docs/rust-port.md:108,196-200`; `crates/longtail/src/version.rs:65,86-96`
  (`chunk_asset_streaming`, and its comment "peak memory is a single part (~`max_hash_size`), not the
  whole asset"); `docs/bench-2026-08-03.md:94-111,179`; `docs/format-spec.md:68-70`;
  `git log --oneline main..HEAD` → `c13a4d1 perf(upsync): stream the scan chunker one part at a time`.
  The `mtime`/`ModTime` absence in both upstream trees is the orchestrator's verification, not mine —
  golongtail source is not in this repo (see DOCS-03).
- **Recommendation:** Retire the streaming half (it shipped; the fact belongs in §Performance beside
  the bench-doc link DOCS-09 asks for). Re-state the remaining item as the size-and-count
  short-circuit only, drop "(as golongtail does)", and re-measure the prize in the **default**
  configuration before quoting a number — or quote no number and say the sizing is stale. Carry R7's
  `OPS-DOC-02` warning in the same edit: that finding establishes the optimisation would break an
  unwritten invariant, which is a fourth reason the bullet as written is not a plan.
- **Tradeoff / risk:** None to the document. If the size-only short-circuit is later judged not worth
  it, the honest outcome is to move the item to §"Dropped and deferred" — which is a better state than
  a permanent flagship item nobody can start.
- **Effort:** S for the doc edit; the re-measurement is a bench run (R4's slice).
- **Regression test to add:** none. The structural fix is that a roadmap item should name the code it
  would change; `version.rs:86-96`'s existence would then have been obvious to the next editor.

### `DOCS-05` — the two keepers contradict each other on `forbid(unsafe_code)`, and the one a contributor reads first is the wrong one

**P2** · `hardening` · CONFIRMED

- **Where:** `CLAUDE.md:101` (false) vs `docs/rust-port.md:208-223` (correct);
  `support/longtail-bench/src/bin/merge_mem.rs`, `dedup.rs`, `ffi_driver.rs`, `e2e.rs`;
  `support/longtail-bench/Cargo.toml:74-99`.
- **What:** `CLAUDE.md:101` asserts, flatly and with no enumeration or exception:
  *"Every default-member library **and binary target** is `#![forbid(unsafe_code)]`."* **This is
  false** — R6's inventory (`docs/review/06-security.md`, the 8-row table) establishes that
  `support/longtail-bench/src/bin/e2e.rs` carries **five `unsafe` blocks** (`:89`, `:91`, `:110`,
  `:113`, `:114`) and **no `forbid`**.
  `docs/rust-port.md:208-223` describes the same fact **correctly**: it enumerates the seven targets
  that do carry the attribute — `longtail-core/src/lib.rs:43`, `longtail-store/src/lib.rs:19`,
  `longtail/src/lib.rs:32`, `longtail-cli/src/main.rs:5`, `xtask/src/main.rs:16`,
  `longtail-testkit/src/lib.rs:8`, `longtail-bench/src/lib.rs:20` — and then names `e2e.rs` as the
  exception, explaining precisely why (`:219-220`: "a binary target that the library's `forbid` does
  not cover"). **rust-port.md is the correct document; `CLAUDE.md` is the wrong one.** Prefer R6's
  inventory over any count here.
  One secondary observation of my own, which does not touch R6's inventory: `rust-port.md`'s *lead
  sentence* (`:210`) reuses `CLAUDE.md`'s loose "every default-member library and binary target"
  phrasing before the colon narrows it, and the three remaining `longtail-bench` bins are unmentioned
  by either doc. **`merge_mem.rs`** is the sharpest case — `support/longtail-bench/Cargo.toml:97-99`
  declares it with **no `required-features`**, so a plain `cargo build` builds it, and it has no
  `forbid`. It contains no `unsafe`, so it is not a counterexample to R6's inventory; it is a gap in
  the *guard*, which is what makes the cheap fix below worth doing.
- **Failure scenario:** `unsafe` is the one property in this workspace asserted absolutely and
  enforced by a compiler attribute rather than a test, so its whole value is that a reader can stop
  checking. `CLAUDE.md` is the document an agent or new contributor is pointed at first — it says so
  in its own first line — and on this question it tells them something untrue. A reader who takes it
  at face value never opens `rust-port.md` §Safety posture, and therefore never learns that the bench
  harness is exempt or why. Separately, a contributor adding a `libc` call to `merge_mem.rs` — a bin
  sitting directly beside one that already has five `unsafe` blocks — compiles clean with no attribute
  to stop them.
- **Evidence:** `docs/review/06-security.md` (authoritative);
  `rg -n 'forbid\(unsafe_code\)'` → the seven sites above;
  `rg -n 'unsafe' support/longtail-bench/src/bin/` → `e2e.rs:89,91,110,113,114` only;
  `grep '^#!\[' support/longtail-bench/src/bin/*.rs` → nothing;
  `support/longtail-bench/Cargo.toml:74-77,83-86,90-93,97-99`; `CLAUDE.md:99-103`;
  `docs/rust-port.md:208-223`.
- **Recommendation:** Two edits. (a) Make `docs/rust-port.md` §Safety posture the **single source**
  and reduce `CLAUDE.md:99-103` to a pointer at it — the two documents cannot contradict each other if
  only one states the fact, and this fixes the false claim by deleting it rather than by maintaining a
  second copy. (b) Add `#![forbid(unsafe_code)]` to `merge_mem.rs`, `dedup.rs` and `ffi_driver.rs`
  (none contains `unsafe`, so it is three lines and no behaviour change), which closes the guard gap
  and makes `rust-port.md:210`'s lead sentence exactly true as written with `e2e.rs` as its one
  documented exception.
- **Tradeoff / risk:** None for the three clean bins. A pointer instead of a restatement costs the
  `CLAUDE.md` reader one hop; that is strictly better than costing them a false belief.
- **Effort:** S
- **Regression test to add:** none needed — the attribute *is* the test. The value is in stating the
  fact in one place, not two.

### `DOCS-06` — the false "downloads a prebuilt native library" build story is in three more files nobody owns

**P2** · `hardening` · CONFIRMED

- **Where:** `support/longtail-sys/README.md:3-6`, `:16-17`; `support/longtail-ffi/README.md:12-13`;
  and, already filed by R2 as `ALG-DOC-01`, `CLAUDE.md:33,49,52-54,64-65`.
- **What:** R2 found `CLAUDE.md`'s description of `build.rs` downloading a pinned prebuilt library is
  dead code under `default = ["vendored"]`. The same description is repeated verbatim in two files
  outside every reviewer's slice:
  - `support/longtail-sys/README.md:3-6` — *"Raw `bindgen`-generated FFI bindings to the **prebuilt**
    … C library. `build.rs` **downloads** the pinned native library for the target platform (see
    `UPSTREAM_VERSION` and the per-OS SHA256 constants), **extracts headers** from the `longtail/`
    git submodule, and runs `bindgen`."* Under the default feature the submodule supplies the **full
    C sources**, which `cc` compiles (`build.rs:269-317`); no download occurs.
  - `support/longtail-sys/README.md:16-17` — *"When updating the upstream C library, refresh the
    SHA256 constants in `build.rs` with `scripts/get-hashes-for-upstream.sh`."* This is now the wrong
    upgrade procedure: bumping the submodule is.
  - `support/longtail-ffi/README.md:12-13` — *"building it requires the prebuilt native library that
    `longtail-sys`'s `build.rs` downloads."* It requires a host C toolchain and libclang.
  `docs/format-spec.md:8-9` carries the fourth instance, filed as DOCS-03.
- **Failure scenario:** These are the two files a maintainer reads immediately before deciding
  whether to delete the legacy crates (R2's Option A/B decision). They tell that maintainer the
  dependency is a network fetch of a pinned tarball — cheap to keep, trivially reproducible. The
  actual dependency is a git submodule reaching GitHub *at compile time from inside `build.rs`*, a C
  toolchain, and libclang, which is a materially different risk profile and is precisely the
  fragility R2's Option B analysis turns on.
- **Evidence:** `support/longtail-sys/Cargo.toml:21-23`; `support/longtail-sys/build.rs:508-515`
  (`if cfg!(feature = "vendored") { vendored() } else { upstream_dist() }`), `:269-317` (`vendored`,
  `cc` build), `:71-78` (`git submodule update --init`), `:13,18-33` (dead `UPSTREAM_VERSION`/SHA256);
  `rg -n 'no-default-features'` across the repo and `.github/` → zero hits, so the download arm is
  never selected.
- **Recommendation:** Fix all three files in one edit alongside R2's `CLAUDE.md` fix — they are the
  same sentence four times. If the crates are deleted per R2's exit date, the two support READMEs go
  with them and only `CLAUDE.md` and `format-spec.md` need the correction.
- **Tradeoff / risk:** None.
- **Effort:** S
- **Regression test to add:** none practical. The structural fix is DOCS-03's citation/provenance
  policy, which puts the pin in one place instead of five.

### `DOCS-07` — two keepers tell the verification story with unreconciled taxonomies, and `format-spec.md` §10.3 claims a check that cannot run

**P2** · `complexity` · CONFIRMED

- **Where:** `docs/format-spec.md:655-671` (§10, five checks) vs `docs/rust-port.md:84-96` (eight
  gates); `support/longtail-testkit/tests/hash_recompute_golden.rs:51-60,128-155`.
- **What:** Both keepers describe how compatibility is proved, and they do not agree on what the
  proof is:
  - `rust-port.md` gives **eight** numbered gates ①-⑧ and calls their conjunction "100% verified
    compatibility". `format-spec.md` §10 gives **five** "load-bearing checks".
  - The sets do not map. `format-spec.md` §10.2 (recompute block hashes → equal the stored hash *and*
    the encoded block path) has no counterpart among the eight. `rust-port.md` ⑤⑥⑦⑧ (downsync tree,
    upsync round-trip, `.lvi` byte-identity, concurrent shards) have no counterpart among the five.
  - **Neither list names a single test file.** R2's `02-algorithms-and-oracle.md:242-252` builds the
    mapping from gate → test → survives-deletion; that table exists in a review document and in
    neither keeper.
  - `format-spec.md:665` is **false**: *"Recompute path/content hashes from a `.lvi` → equal the
    stored values, **for each of blake3/blake2/meow**"*. Meow hashes cannot be recomputed in the pure
    port — `hash_recompute_golden.rs:60` says so in a comment (`// cannot recompute meow hashes in the
    pure port`), and the meow cell asserts `HashError::UnsupportedHash` instead
    (`:58-60`, `:107`, `:128`, `:187`). The spec claims equality where the suite asserts a typed
    error.
- **Failure scenario:** These two lists are what a reviewer, an auditor, or the person signing the
  switchover reads to decide the compat claim is met. Given two incompatible enumerations and no test
  names, the only way to know what is actually gated is to read `support/longtail-testkit/tests/`.
  Specifically, a reader of §10.3 believes meow-hashed stores are hash-verified end to end; they are
  parse-verified only, and `crates/longtail/tests/lvi_byte_gate.rs:3` confirms the byte gate skips the
  meow fixture ("15 of 16").
- **Evidence:** `docs/format-spec.md:655-671`; `docs/rust-port.md:84-96`;
  `support/longtail-testkit/tests/hash_recompute_golden.rs:7,58-60,107,128-155,187`;
  `crates/longtail-core/src/hash.rs:17-19` (meow "recognized, **verification unsupported**");
  `crates/longtail/tests/lvi_byte_gate.rs:3`. R2's `ALG-DOC-03` separately corrects gate ③'s "four
  hash kinds" and gate ②'s "every target size" — I do not restate those; this finding is that the two
  documents carry *different* taxonomies of the same thing.
- **Recommendation:** Make one of them authoritative and have the other link. `rust-port.md`'s gate
  table is the right home (it already has the eight rows); give it R2's cadence and OS columns plus a
  **test file** column, and reduce `format-spec.md` §10 to a pointer plus the spec-specific mapping
  "which section each gate proves". Fix §10.3 to say the meow cell asserts the typed error.
- **Tradeoff / risk:** A test-file column rots when tests move. It rots *loudly* — a missing path is
  greppable, unlike a prose claim.
- **Effort:** S
- **Regression test to add:** an `xtask` check that every test path named in the gate table exists.
  Cheap, and it is the only claim in the keepers that can be mechanically checked today.

### `DOCS-08` — `put-path-memory.md` contradicts its own Resolution section and pins a stale self-SHA

**P2** · `hardening` · CONFIRMED

- **Where:** `docs/put-path-memory.md:8-9`, `:11`, `:213-216` vs `:218-254`.
- **What:** The document's own header warning says *"**there are no upsync benchmarks in the repo yet
  — measuring alongside any fix work is a prerequisite for trusting these estimates.**"* (`:8-9`) and
  repeats it at `:213-216`: *"**Add upsync benchmarks to the repo alongside any of this work** … the
  download path already has this (`bench-2026-07-05.md`); the upload path does not."* Both are false:
  commit `30b16c4` ("bench(upsync): measure PUT-path flush memory (3-way, peak RSS)") added them, and
  the document's own §Resolution (`:218-254`, added the same day) reports the measurements and links
  `bench-2026-08-03.md`. The header warning that gates the whole document's credibility was never
  retracted.
  Separately, `:11` — *"Refs are against Rust port `@a459c3b`"* — pins the document to a commit
  **twelve commits before `HEAD`** (`a459c3b` is "chore(deps): bump indicatif 0.17->0.18"), while the
  file was last modified Aug 4. Every `path:line` in the body is therefore nominally against a tree
  that predates the very work the Resolution section describes as landed.
- **Failure scenario:** A reader opens this to decide whether the PUT path is production-ready for
  the signed-build workload. The first thing they read is "do not trust these numbers, there are no
  benchmarks". If they stop there — which the emphasis invites — they re-do work that is done and
  measured. If they read to `:254`, they get the opposite conclusion. The document argues with itself
  across 250 lines.
- **Evidence:** `git log --oneline main..HEAD` (`30b16c4`, `2cce0cd`, `f2edb58`, `c703b32`, `c13a4d1`,
  `8e28d81`, `78c2c46`, `4583371` all post-date `a459c3b`); `docs/put-path-memory.md:8-9,11,213-216,
  218-254`; `docs/bench-2026-08-03.md` exists and is linked from `:221`.
- **Recommendation:** Delete the document after folding — see Disposition. Two pieces are unique and
  worth keeping: the **two-file steady state** explanation (`:21-36`, why a real shard carries exactly
  two `store_*.lsi` and why a writer must not delete a file it did not merge) belongs in
  `docs/format-spec.md` §2; the **item-5 If-None-Match deferral with its honest caveat** (`:111-127`,
  content-addressed shard names mean the header would not have prevented the incident) belongs in
  `docs/rust-port.md` §"Dropped and deferred".
- **Tradeoff / risk:** The doc is a good record of the reasoning behind two days of work. That record
  is git history; the *conclusions* are what a future reader needs.
- **Effort:** S
- **Regression test to add:** n/a.

### `DOCS-09` — the newest and most decision-relevant measurements are unreachable from every keeper

**P2** · `complexity` · CONFIRMED

- **Where:** `docs/rust-port.md:7-8`, `:103-109`; `CLAUDE.md:42`.
- **What:** The full outbound-link graph of the repo's documentation:

  | from | to |
  |---|---|
  | `readme.md:71,73` | `docs/rust-port.md`, `docs/format-spec.md` |
  | `docs/rust-port.md:7,8,109` | `docs/format-spec.md`, `docs/bench-2026-07-05.md` |
  | `docs/format-spec.md` | **nothing** |
  | `CLAUDE.md:14,15,41,42` | `docs/rust-port.md`, `docs/format-spec.md`, `fixtures/README.md`, and `switchover-checklist.md` *only inside the ASCII directory tree* |
  | `docs/bench-2026-08-03.md:4` | `docs/put-path-memory.md` |
  | `docs/put-path-memory.md:159,221` | `docs/rust-port.md`, `docs/bench-2026-08-03.md` |
  | `docs/bench-2026-07-05.md:389` | `docs/rust-port.md` |
  | `docs/switchover-checklist.md`, both `support/*/README.md` | **nothing, and nothing links to them** |

  Consequences: `bench-2026-08-03.md` and `put-path-memory.md` form a closed two-node island —
  unreachable from `readme.md` or `rust-port.md` at any depth. `switchover-checklist.md` is reachable
  only by noticing a filename inside a fenced code block. The two support READMEs are orphans.
  `format-spec.md` — 670 lines, the authoritative reference — has zero outbound links, so a reader who
  enters there never finds `rust-port.md`.
  Meanwhile `rust-port.md` §Performance (`:103-109`) discusses only the download path and points at
  the July doc, though the last ten commits on the branch are PUT-path memory work whose results
  (flush peak 843 → 305 MiB, below golongtail; 512 MiB asset upsync peak 512 → 50 MB) are the
  strongest evidence in the repo that the upload path is production-ready.
- **Failure scenario:** The Fellowship OOM incident is the reason the PUT-path work exists. The next
  person asked "will the port survive the signing run?" starts at `readme.md`, follows the only two
  links, reads a §Performance section about downloads, and never sees the measurement that answers
  the question. `git log` is the only path to it.
- **Evidence:** the link extraction above (`rg -o '\[[^]]*\]\([^)]*\.md[^)]*\)|`[a-zA-Z0-9_./-]*\.md`'`
  over every doc); `docs/bench-2026-08-03.md:53-111`; `git log --oneline main..HEAD`.
- **Recommendation:** Two links close it: `docs/rust-port.md` §Performance gains an upload-path
  paragraph pointing at `bench-2026-08-03.md`, and `docs/format-spec.md`'s header gains a pointer to
  `rust-port.md`. Add `readme.md` §"Learn more" rows for whatever survives Disposition — a reader must
  be able to enumerate the documentation from the front door.
- **Tradeoff / risk:** None.
- **Effort:** S
- **Regression test to add:** an `xtask check-doc-links` that (a) fails on a link to a nonexistent
  file and (b) fails if any `.md` outside `docs/review/` is unreachable from `readme.md`. That
  second half is what would have caught this.

### `DOCS-10` — an `archive` cargo feature is named in three docs and one test and declared in no `Cargo.toml`

**P2** · `hardening` · CONFIRMED

- **Where:** `docs/rust-port.md:189`; `docs/switchover-checklist.md:44`, `:196`;
  `docs/format-spec.md:585`, `:608`; `crates/longtail-cli/tests/commands_spec.rs:8`, `:1335`,
  `:1339`, `:1346`.
- **What:** The complete set of cargo features declared anywhere in the workspace is:
  `s3` (`longtail-store/Cargo.toml:10`, `longtail/Cargo.toml:9`, `longtail-cli/Cargo.toml:12`),
  `fastcdc` (`longtail-core/Cargo.toml:30`, `longtail-bench/Cargo.toml:45`),
  `differential` (`longtail-testkit/Cargo.toml:32`, `longtail-bench/Cargo.toml:49`,
  `xtask/Cargo.toml:20`), and `vendored` (`longtail-sys/Cargo.toml:23`). There is no `archive`.
  Yet three documents describe `ArchiveIndex`/`pack`/`unpack` as living *behind* one — `rust-port.md:189`
  "behind an `archive` feature, droppable"; `format-spec.md:585` "Full spec deferred to the archive
  feature (feature-gated and droppable)"; `switchover-checklist.md:44` "the `archive` feature, not yet
  implemented" — and two `#[ignore]` attributes assert it as a reason
  (`commands_spec.rs:1339,1346`).
- **Failure scenario:** Three separate readers draw wrong conclusions. A contributor asked to "enable
  the archive feature" runs `cargo build --features archive` and gets
  `error: none of the selected packages contains these features`. A reviewer deciding whether
  `pack`/`unpack` are cheap to restore reads "feature-gated and droppable" and assumes gated code
  exists to un-gate; there is none. An operator reading `switchover-checklist.md:196` ("Keep
  `pack`/`unpack` on golongtail **until the archive feature lands**") waits on a switch that does not
  exist rather than treating it as unimplemented work needing a decision.
- **Evidence:** the `[features]` block of every `Cargo.toml` in the repo, enumerated above;
  `rg -n '\barchive\b' -g '*.toml'` → only `support/longtail-sys/build.rs`'s local `ZipArchive`
  variable, unrelated.
- **Recommendation:** Say "not implemented" in all three docs and delete the word "feature". Change
  the two `#[ignore]` reasons to match. If a feature gate is genuinely wanted later, declaring it is
  a one-line change — but declaring it now would make the docs true and the crate misleading in the
  other direction (an empty feature that gates nothing).
- **Tradeoff / risk:** None.
- **Effort:** S
- **Regression test to add:** an `xtask` check that every feature name in backticks in `docs/*.md`
  resolves in some `Cargo.toml` — the same lint would also catch DOCS-15's inverse.

### `DOCS-11` — the four `LONGTAIL_TEST_S3_*` vars gating the entire S3 lane are documented in no markdown, and three of them default silently

**P2** · `security` · CONFIRMED

- **Where:** `crates/longtail-store/tests/s3_spec.rs:131-137`;
  `crates/longtail-cli/tests/s3_interop.rs:34-43`; `.github/workflows/s3-minio.yaml:36-39,84-90`.
- **What:** The S3 test lane — the only coverage of the S3 blob store, the mixed-writer interop gate
  ⑧, and the credential-refresh path — is gated on `LONGTAIL_TEST_S3_ENDPOINT` and configured by
  `LONGTAIL_TEST_S3_BUCKET`, `LONGTAIL_TEST_S3_ACCESS_KEY`, `LONGTAIL_TEST_S3_SECRET_KEY`, plus
  `AWS_REGION`. Coverage:
  - `LONGTAIL_TEST_S3_ENDPOINT` is named in two module docs (`s3_spec.rs:12`, `s3_interop.rs:10`) and
    in the workflow. Good.
  - The other four are named **only** in the test source and in the workflow's `env:` block. Zero
    markdown mentions anywhere in the repo (`rg` over every non-review `.md`).
  - Three of them have hardcoded fallbacks (`s3_spec.rs:133-137`, `s3_interop.rs:37-41`) —
    bucket `longtail-test`, and the well-known minio development credential pair — and `AWS_REGION`
    falls back to `us-east-1` (`s3_interop.rs:43`). The fallbacks are `unwrap_or_else`, so **no
    warning is emitted when they engage**.
- **Failure scenario:** A developer, or a future CI job, wants to run the S3 lane against a real
  staging endpoint. They set `LONGTAIL_TEST_S3_ENDPOINT` — the only variable any document mentions —
  and run. The suite does not skip (the gate is satisfied), does not warn, and attempts every request
  against bucket `longtail-test` with the hardcoded minio development credentials. Against a real S3
  endpoint that is an authentication failure, which surfaces as a confusing test failure rather than
  "you forgot three variables"; against a misconfigured internal endpoint that happens to accept
  them, it is a write to the wrong bucket. `MANIFEST.md:46` records that `03-test.txt` does not cover
  S3 at all, so nothing in the evidence pack would have surfaced this.
- **Evidence:** `crates/longtail-store/tests/s3_spec.rs:131-137`;
  `crates/longtail-cli/tests/s3_interop.rs:34-43`; `.github/workflows/s3-minio.yaml:1-12,36-39,84-90`;
  `rg -l 'LONGTAIL_TEST_S3_(BUCKET|ACCESS_KEY|SECRET_KEY)' -g '*.md'` → nothing.
  I am not reproducing the credential values; they are hardcoded at the two cited line ranges and in
  the public workflow. They are the documented minio development defaults, not a real secret — but
  they should not silently become the effective credentials against a non-minio endpoint.
- **Recommendation:** Document all five where the lane is described (the `s3_spec.rs` /
  `s3_interop.rs` module docs already carry the endpoint variable, so extend those — that is the
  right altitude, not a `.md`), and make the fallbacks conditional on the endpoint looking like a
  local minio, or emit an `eprintln!` naming each variable that fell back. `CLAUDE.md` §Common
  commands should gain the one-line invocation for the lane, since it lists every other lane.
- **Tradeoff / risk:** Making fallbacks conditional could break the workflow, which sets all four
  explicitly — so it would not. A warning is zero-risk.
- **Effort:** S
- **Regression test to add:** none; the change is the warning.

### `DOCS-12` — `.lrb` is a compat-bearing on-disk layout absent from the authoritative format spec

**P2** · `hardening` · CONFIRMED

- **Where:** `docs/format-spec.md:212-285` (§3, the gap) and `:155-162`;
  `crates/longtail-store/src/cache.rs:4-14`, `:68-71`, `:214-218`.
- **What:** `format-spec.md` is "the authoritative on-disk format specification" and §3 documents the
  `.lsb` block path scheme in detail. It never mentions `.lrb`. But `.lrb` is a real on-disk layout
  the port must read **byte-compatibly with existing production data**:
  `cache.rs:4-9` — *"existing launcher caches were written by C's FSBlockStore as
  `chunks/<first-4-hex>/0x<hash16>.lrb` (golongtail passes an empty extension → C's default `.lrb`).
  This store uses the same block-path scheme with the `.lrb` extension … The stored bytes are
  byte-identical to the `.lsb` stored-block output — only the extension differs."*
  `cache.rs:11-14` adds a second compat decision the spec does not carry: the cache-dir `store.lsi`
  is treated as **advisory** and never read, deliberately diverging from C's authoritative cache
  index. `rust-port.md:146-153` mentions `.lrb` once, inside the LRU-eviction bullet, which is not
  where a format reader looks.
- **Failure scenario:** The Tauri launcher's cache directory on every end-user machine is in this
  format. Anyone writing a cache-cleanup tool, a support diagnostic, or a migration reads
  `format-spec.md` §3, implements the `.lsb` scheme, and finds nothing — or worse, writes `.lsb` into
  a cache directory that the port will then never read. The mtime semantics matter too: `cache.rs`
  stamps mtime on every access *including hits* so eviction is a true LRU clock, which means an
  external tool that touches files corrupts the eviction order.
- **Evidence:** `crates/longtail-store/src/cache.rs:4-14,68-71,99-110,214-218,260`;
  `docs/format-spec.md` — `rg -n 'lrb' docs/format-spec.md` → no matches;
  `support/longtail-testkit/tests/downsync_three_way.rs:432-445`
  (`assert_lrb_matches_lsb`, the byte-identity gate that makes this a format claim).
- **Recommendation:** Add a short §3.x to `format-spec.md` — path scheme, byte-identity to `.lsb`,
  the advisory `store.lsi`, and the mtime-as-access-clock contract. The content already exists in
  `cache.rs`'s module doc; the spec should own the format half and the module doc keep the
  implementation half. Cross-reference R3's `STORE-DOC-05` (the `evict_cache_dir` doc claims a `.lrb`
  filter the code does not have) — that discrepancy should be resolved before the spec text is
  written, so the spec does not enshrine the wrong one.
- **Tradeoff / risk:** `COMPAT-RISK` if the spec text and the code diverge — the caches are on
  end-user machines and cannot be migrated. The existing gate is
  `support/longtail-testkit/tests/downsync_three_way.rs:432` (`assert_lrb_matches_lsb`), which runs in
  the **weekly** differential lane only, and `crates/longtail-store/tests/decorators_integration.rs`
  (per-PR) for the populate/passthrough property.
- **Effort:** S
- **Regression test to add:** a per-PR test that asserts the cache block path for a known hash equals
  the `.lsb` path with the extension swapped — currently only the weekly three-way test asserts it.

### `DOCS-13` — the switchover rollback section propagates a safety claim R3 proved false

**P2** · `complexity` · CONFIRMED

- **Where:** `docs/switchover-checklist.md:203-206`; original at
  `crates/longtail-store/src/remote.rs:560-562` (R3's `STORE-DOC-02`).
- **What:** The runbook's Rollback section states: *"`prune-store` / `prune-store-index` are the only
  destructive commands. Always `--dry-run` first; a store index overwrite is done BEFORE any block
  delete, so a mid-run failure leaves harmless orphan blocks (recoverable via `init-remote-store`),
  **never a dangling index**."* R3 established (`STORE-DOC-02`, P1 CONFIRMED, and the underlying
  `STORE-01`) that the code comment making this claim is true for a **crash** and false for a **delete
  failure**: the delete-error path produces exactly the dangling entries the claim rules out.
- **Failure scenario:** R3's finding is about a code comment — its cost is "a reader stops looking".
  Here the same claim is operational instruction in the document a human follows while running a
  destructive command against a production store. An operator who sees `prune-store` report delete
  errors is told by this text that the index is fine and only orphans remain, and proceeds. The
  correct response is to re-run `init-remote-store` and re-validate. This is the highest-consequence
  place the claim appears and no reviewer owns the file.
- **Evidence:** `docs/switchover-checklist.md:203-206`; `docs/review/03-store-concurrency.md`
  `STORE-DOC-02` and `STORE-01`; `crates/longtail-store/src/remote.rs:560-577`;
  `crates/longtail-store/src/sync.rs:357-365`.
- **Recommendation:** Fix the code (R3's `STORE-01`) first; the runbook sentence then becomes true.
  If `STORE-01` is deferred, the runbook must say what the operator should do when a delete fails —
  that is the only part of this document that carries an action under failure.
- **Tradeoff / risk:** None; this is strictly narrowing an overclaim.
- **Effort:** S (follows `STORE-01`)
- **Regression test to add:** covered by `STORE-01`.

### `DOCS-14` — `CLAUDE.md`'s `docs/` listing omits two entries

**P3** · `complexity` · CONFIRMED

- **Where:** `CLAUDE.md:42`.
- **What:** The line reads `docs/  # rust-port.md, format-spec.md, bench-<date>.md, switchover-checklist.md.`
  Actual contents: `bench-2026-07-05.md`, `bench-2026-08-03.md`, `format-spec.md`,
  `put-path-memory.md`, `review/`, `rust-port.md`, `switchover-checklist.md`. The `bench-<date>.md`
  glob correctly covers both bench files; **`put-path-memory.md` and `review/` are omitted**.
- **Failure scenario:** `CLAUDE.md` is the operating manual for agents and new contributors, and this
  line is the only enumeration of `docs/` anywhere. Combined with DOCS-09 (nothing links
  `put-path-memory.md` from a keeper), the PUT-path analysis is invisible from both the link graph
  and the directory listing. An agent told "read the docs" reads three of five.
- **Evidence:** `ls docs/`; `CLAUDE.md:42`.
- **Recommendation:** Regenerate the line once Disposition is applied, so it lists exactly what
  survives. Better: replace the hand-maintained inventory with "see `readme.md` §Learn more" so there
  is one list, not two.
- **Tradeoff / risk:** None.
- **Effort:** S
- **Regression test to add:** folded into DOCS-09's `check-doc-links`.

### `DOCS-15` — the `s3` and `vendored` cargo features are documented nowhere

**P3** · `idiom` · CONFIRMED

- **Where:** `crates/longtail-store/Cargo.toml:7-16`, `crates/longtail/Cargo.toml:7-9`,
  `crates/longtail-cli/Cargo.toml:11-12`, `support/longtail-sys/Cargo.toml:22-23`.
- **What:** Of the four features that exist, `fastcdc` and `differential` are documented in
  `CLAUDE.md` and the bench docs. `s3` and `vendored` appear in **no** markdown file.
  - `s3` is `default = ["s3"]` on three crates and controls whether the entire AWS SDK is compiled.
    `11-featurematrix.txt` shows `--no-default-features` builds are a supported configuration — so a
    consumer who wants a smaller binary (the Tauri app is a plausible candidate) has a supported knob
    that no document mentions. The `Cargo.toml` comments describe it well
    (`longtail-store/Cargo.toml:8-9`); nothing at doc level does.
  - `vendored` is the feature whose default value makes `CLAUDE.md`'s and both support READMEs' build
    story false (DOCS-06, R2's `ALG-DOC-01`). It is the single highest-leverage undocumented flag in
    the repo, and its own crate's README does not name it.
- **Failure scenario:** For `vendored`, the consequence is DOCS-06 — four documents describe a build
  path that is only reachable by passing a flag none of them mention. For `s3`, a consumer asking
  "can I drop the AWS SDK?" has no way to learn the answer is yes.
- **Evidence:** the `[features]` blocks cited above; `rg -l '\bs3\b.*feature|feature.*\bs3\b' -g '*.md'`
  and the same for `vendored` → nothing; `target/review-evidence/11-featurematrix.txt`.
- **Recommendation:** One table in `CLAUDE.md` §Workspace layout: feature · crate · default? · what
  it turns on. Four rows.
- **Tradeoff / risk:** None.
- **Effort:** S
- **Regression test to add:** the inverse of DOCS-10's lint — every declared feature appears in the
  table. Both directions are the same `xtask` check.

### `DOCS-16` — `rust-port.md` gives two different HPCDC ceilings and attributes each to the wrong cause

**P3** · `complexity` · CONFIRMED

- **Where:** `docs/rust-port.md:138-140` and `:169-171`; correct values at
  `crates/longtail-core/src/chunker.rs:33-39`.
- **What:** The same fact is stated twice with two numbers and two swapped causes:
  - `:138` — *"For target `avg` above ≈ **9.31M** the C `(uint32_t)` cast of the discriminator is
    undefined (**the expression crosses its denominator pole**)"*
  - `:169` — *"Above `avg` ≈ **9.32M** the cast … is undefined — the expression **crosses its
    denominator pole / exceeds `u32` range**"*
  The code distinguishes two thresholds correctly (`chunker.rs:33-36`): the quotient leaves `u32`
  range at `avg ≈ 9_309_388` (≈9.31M) and the **denominator's pole** is at `avg ≈ 9_324_556`
  (≈9.32M). `MAX_AVG = 9_309_387` (`chunker.rs:39`) is the enforced bound, and
  `chunker_differential.rs:10` exhausts `avg ∈ [48, 9_309_387]`. So `:138`'s number is right and its
  cause is wrong; `:169`'s cause is complete but its number understates the rejected range by ~15,000
  and reads as if inputs between 9.31M and 9.32M are accepted. They are not.
- **Failure scenario:** Narrow but real: someone reconciling the port's rejection threshold against C
  — the exact task the differential ladder-9 test performs — has two candidate bounds from the keeper
  and must go to the source anyway. The `:169` entry is in §"Upstream findings", the section whose
  stated purpose is to be filed against upstream; a bug report with the wrong threshold is weaker.
- **Evidence:** `crates/longtail-core/src/chunker.rs:33-39`;
  `support/longtail-testkit/tests/chunker_differential.rs:9-11`;
  `docs/rust-port.md:138-140,169-171`. Cross-reference R2's `ALG-DOC-04`, which separately notes
  `format-spec.md` §6 omits the accepted-target range entirely — fixing both together gives one
  number in three places.
- **Recommendation:** Use the code's two-threshold wording in both bullets and cite
  `chunker.rs:33-39` as the source, or state only `MAX_AVG` and let the code comment carry the
  derivation.
- **Tradeoff / risk:** None.
- **Effort:** S
- **Regression test to add:** none; `chunker_differential.rs` ladder 9 already pins the bound.

### `DOCS-17` — `CLAUDE.md` recommends a lint command that needs a C toolchain, 20 lines after saying none is needed

**P3** · `complexity` · CONFIRMED

- **Where:** `CLAUDE.md:69-70` vs `:46-50`.
- **What:** `:46-48` establishes the workspace's central promise: *"a plain `cargo build`/`cargo test`
  needs no network access and never touches the native library."* `:69` then gives the standard lint
  command as `cargo +nightly clippy --workspace --all-targets` — and `--workspace` is precisely the
  flag `:50` says pulls in `longtail-sys`. Under `default = ["vendored"]` that means a submodule fetch
  from inside `build.rs`, a host C compiler, and libclang. `.github/workflows/rust.yaml:129` runs the
  same command in the per-PR `formatting` job. The section gives no hint of the difference.
- **Failure scenario:** A contributor on a machine without libclang follows §Common commands in
  order: `cargo build` works, `cargo test` works, `cargo clippy --workspace` fails in `build.rs` with
  a C-toolchain error, and nothing in the document explains why the third command has requirements
  the first two do not.
- **Evidence:** `CLAUDE.md:46-50,69-70`; `.github/workflows/rust.yaml:129`;
  `support/longtail-sys/build.rs:71-78,269-317,508-515`. `MANIFEST.md:84-103` documents why
  `01b-clippy-ws.txt`'s 13-second warm run is not evidence of the cold cost — R8 owns the CI-cost
  side; this is only the documentation inconsistency.
- **Recommendation:** Annotate the line with what it requires, and give the pure-lane equivalent
  (explicit `-p` list, as `01-clippy-pure.json` used) as the default. Whether CI should keep
  `--workspace` is R8's call.
- **Tradeoff / risk:** None for the document.
- **Effort:** S
- **Regression test to add:** n/a.

### `DOCS-18` — root readme is lowercase while all three subdirectory READMEs are uppercase, and code hardcodes the uppercase form

**P3** · `complexity` · CONFIRMED

- **Where:** `readme.md` (root); `fixtures/README.md`, `support/longtail-sys/README.md`,
  `support/longtail-ffi/README.md`; `support/longtail-testkit/src/fixture_manifest.rs:73`.
- **What:** I checked the seeded concern that a `README.md` reference would break on a case-sensitive
  host and **it does not hold**: there is no root `README.md`, and no document references one
  (`rg '(^|[^/a-zA-Z])README\.md'` over the repo returns exactly one hit, and it is a Rust string
  literal). Recording that as verified so it is not re-derived.
  What *is* inconsistent: the root file is `readme.md` while all three subdirectory READMEs are
  `README.md`, and `fixture_manifest.rs:73` hardcodes the uppercase spelling
  (`if rel == MANIFEST_NAME || rel == "README.md"`, the fixture-scan exclusion).
- **Failure scenario:** Low. The concrete one: a future fixture-manifest exclusion, or any tool that
  reuses that hardcoded literal, silently misses a lowercase `readme.md` if one is ever added under
  `fixtures/`. More generally, three-of-four plus a hardcoded string is a convention, and the root
  file is the exception.
- **Evidence:** `ls README.md` → no such file; `rg '(^|[^/a-zA-Z])README\.md'` → one hit,
  `support/longtail-testkit/src/fixture_manifest.rs:73`;
  `find . -name 'README.md' -not -path './target/*' -not -path './support/longtail-sys/longtail/*'` →
  the three subdirectory files.
- **Recommendation:** Rename the root to `README.md`. Justification, in order: three of the four
  existing files already use it; the code already hardcodes that spelling; and it is the spelling
  every tool that special-cases a readme (GitHub, crates.io, `cargo package`) looks for first. Update
  `CLAUDE.md` and `docs/` references in the same commit — there are currently none to the root file,
  so the rename is free.
- **Tradeoff / risk:** `git mv` on a case-insensitive filesystem needs the two-step rename; otherwise
  none.
- **Effort:** S
- **Regression test to add:** n/a.

### `DOCS-19` — `rust-port.md:105` forward-references a section that does not exist

**P3** · `complexity` · CONFIRMED

- **Where:** `docs/rust-port.md:105`.
- **What:** *"Cold and warm downsync are at golongtail parity (≈1.00× at 8 remote workers **after the
  download-path fixes below**)"*. There is no "download-path fixes" section below, or anywhere, in
  `rust-port.md`. The fixes are described in `docs/bench-2026-07-05.md:386-397` (the §9 addendum's
  preamble: "Fix 1 removes the preflight budget deadlock … Fix 2 makes the block apply concurrent").
  The sentence reads as if the document explains them.
- **Failure scenario:** Minor navigation cost — a reader scans the remaining 118 lines for a section
  that is not there. It compounds with DOCS-09: the pointer that *would* resolve it (`:109`) is in the
  next paragraph but is worded as a methodology reference, not as the location of the fixes.
- **Evidence:** `docs/rust-port.md:103-109`; heading tree of `rust-port.md` (11 `##`, no
  download-path section); `docs/bench-2026-07-05.md:386-397`.
- **Recommendation:** Point at `bench-2026-07-05.md` §9 by name, or fold a two-line summary of the
  two fixes into §Architecture where the prefetch budget is already described (`:49-52`).
- **Tradeoff / risk:** None.
- **Effort:** S
- **Regression test to add:** n/a.

### `DOCS-20` — the July bench doc's verdict heading still says FAIL, 158 lines after the doc supersedes it

**P3** · `complexity` · CONFIRMED

- **Where:** `docs/bench-2026-07-05.md:328` vs `:486-495`.
- **What:** §7 heading reads *"### Verdict 2 — E2E vs golongtail: **FAIL on fs-local** (slower and a
  hard scale ceiling)"* and its body calls the deadlock "Release-blocking for the launcher". §9.3
  (`:486-489`) then states: *"Verdict 2's 'FAIL on fs-local' (§7) is superseded for the cold/warm
  download path"*. The addendum is correct and honest; the heading is not updated, and headings are
  what a reader skims.
- **Failure scenario:** `docs/rust-port.md:109` sends readers to this document for "Full methodology
  and numbers". A reader who lands on §7 Verdicts — the section a reader jumps to — reads
  "release-blocking" about a condition fixed a month ago. The same document is the only source of
  download-path numbers, so it will keep being read.
- **Evidence:** `docs/bench-2026-07-05.md:313,328-343,375-397,482-495`;
  `docs/rust-port.md:105-109`.
- **Recommendation:** Retitle §7 Verdict 2 to carry "(superseded — see §9.3)" and leave the body as
  the historical record, which is the pattern §9's preamble already establishes ("Everything above
  (§1–§8) is the baseline record and is left untouched"). This is the one edit that document needs;
  see Disposition for why it otherwise stays.
- **Tradeoff / risk:** None.
- **Effort:** S
- **Regression test to add:** n/a.

---

## Deliverable (b) — target outlines for the four keepers

Heading tree · one-line charter per heading · source of the section's content. **No wording is
proposed** — each row says what the section is for and where its material comes from.
`[NEW]` marks a section that does not exist today.

### `readme.md` — target (~85 lines; today 73)

Charter: the sixty-second orientation for a stranger. It answers *what is this, does it work, how do
I run it once, where do I read next* — and nothing else.

| # | Heading | Charter | Source |
|---|---|---|---|
| `#` | longtail (pure Rust) | What it is and the single compat claim. | Current `:1-8`, minus the false "No C library is built or linked" sentence (R2 `ALG-DOC-02`). |
| `##` | Crates | Four rows, one line each. | Current `:9-18`; the `longtail-sys`/`longtail-ffi` paragraph (`:19-20`) disappears on R2's exit date. |
| `##` | Quick start | One CLI invocation and one library invocation, both verified to compile. | Current `:22-61` — verified correct against `crates/longtail/src/{lib.rs:55,65-68}`, `options.rs:29,83-87`, `downsync.rs:30`, and the clap definitions. No change. |
| `##` | Build requirements `[NEW]` | What `cargo build` actually needs: a C toolchain for `zstd-sys`, no network, no longtail C library. | `17-bloat.txt:278` (321.1 KiB / 2.4%); `crates/longtail-core/Cargo.toml:14-15` ("The only C in longtail-core"); `.github/workflows/rust.yaml:142`. Resolves R2 `ALG-DOC-02`. |
| `##` | Supported and not supported `[NEW]` | Schemes, platforms, and the four operations that return a typed error. | `crates/longtail-store/src/uri.rs:188` (`gs://`); `crates/longtail/src/error.rs:59-62` (`--use-legacy-write`); `crates/longtail-core/src/hash.rs:17-19` (meow write); `docs/rust-port.md:187-192` (pack/unpack); the feature table from DOCS-15. |
| `##` | Compatibility | The claim plus a pointer to its proof, not a restatement of it. | Current `:63-67` + a link to `rust-port.md` §How compatibility was verified. |
| `##` | Learn more | The complete doc map — every surviving document, one line each. | Current `:69-73` + `bench-2026-08-03.md` (DOCS-09) + whatever survives Disposition. |

### `CLAUDE.md` — target (~125 lines; today 112)

Charter: the operating manual for an agent or new contributor. Every statement must be actionable and
checkable; nothing here is a design record (that is `rust-port.md`'s job).

| # | Heading | Charter | Source |
|---|---|---|---|
| `#` | CLAUDE.md | Unchanged. | Current `:1-3`. |
| `##` | Repository purpose | What the repo is and what to read first. | Current `:5-16`. Verified correct. |
| `##` | Workspace layout | The tree, which members build by default, and how the legacy pair is actually built. | Current `:18-51` verified correct against root `Cargo.toml`; **replace** `:52-56` with the vendored-submodule story (`support/longtail-sys/Cargo.toml:21-23`, `build.rs:508-515,269-317`) per R2 `ALG-DOC-01`; regenerate the `docs/` line `:42` (DOCS-14); add the four-row feature table (DOCS-15). |
| `##` | Common commands | Every command a contributor runs, each annotated with what it requires. | Current `:58-75` — all verified to exist; annotate `:69` with the C-toolchain requirement (DOCS-17); add the S3-lane invocation with its five env vars (DOCS-11). |
| `##` | CI | Which workflow gates what, per-PR vs scheduled. | Current `:77-86` — verified accurate against all four workflow files; add the `docs` job once DOCS-04 lands. |
| `##` | Runtime configuration | The knobs a caller sets. | Current `:88-97`; fix "Logging is `tracing`-based" per R7 `OPS-DOC-03`. |
| `##` | Safety | A pointer, not a restatement. | Replace the current `:99-103` prose with a link to `rust-port.md` §Safety posture — the single-source fix for DOCS-05. |
| `##` | Conventions | The rules a contributor must not break. | Current `:105-112` + the citation policy below, which is a convention and belongs here. |

### `docs/rust-port.md` — target (~260 lines; today 223)

Charter: the design record. Why the port has the shape it has, every way it deliberately differs from
C/Go, and what is proved about it. It is the only document that carries *judgement*.

| # | Heading | Charter | Source |
|---|---|---|---|
| `#` | The pure-Rust longtail port | Framing + the citation base. | Current `:1-9`; the base must name the in-repo submodule (DOCS-03). |
| `##` | Motivation | Why replace rather than wrap. | Current `:11-25`. Unchanged. |
| `##` | What was ported, and where it lives | Crate-to-role table. | Current `:27-35`; the legacy row goes with the crates. |
| `##` | Architecture | The densest correct section in the keepers; leave it alone. | Current `:37-74`. Verified against `remote.rs`, `sync.rs`, `uri.rs`, `lib.rs`. |
| `##` | How compatibility was verified | One row per gate with **cadence · OS · implementing test file**. | Current `:76-101` + R2 `ALG-DOC-03`'s cadence/OS columns + the gate→test mapping at `docs/review/02-algorithms-and-oracle.md:242-252`. Becomes the single authority; `format-spec.md` §10 links here (DOCS-07). |
| `##` | Performance | One paragraph per path, each pointing at its dated bench doc. | Current `:103-109` (download) + a new upload paragraph from `bench-2026-08-03.md` §3/§4/§6 (DOCS-09); fix the dangling forward reference at `:105` (DOCS-19). |
| `##` | CLI compatibility with golongtail `[NEW]` | Every way the CLI differs from golongtail v0.4.5: missing aliases and globals, accepted-but-ignored flags, unimplemented commands, the minio virtual-host requirement. | `docs/switchover-checklist.md:46-67` (moved wholesale) + R7 `OPS-06`/`OPS-07` + `14-golongtail-help.txt` + `crates/longtail-cli/tests/s3_interop.rs:7-12` (path-style caveat). This is the section that lets `switchover-checklist.md` be retired (DOCS-01, DOCS-02). |
| `##` | Deliberate divergences from C/Go | Every intentional behavioural difference, each with its code site. | Current `:111-153` + R3 `STORE-DOC-01` (Windows mixed writers, `blob/fs.rs:17-21`) + R7 `OPS-DOC-04` + R2 `ALG-DOC-06` + the prefetch permit-estimate divergence (`remote.rs:41-44`) + read-only-disables-locking (`uri.rs:165-168`, coordinate with R3 `STORE-DOC-06`); fix the HPCDC ceiling (DOCS-16). |
| `##` | Upstream findings | Bugs to file against upstream. | Current `:155-176` + R2 `ALG-DOC-05` (`DiffHashes` reorder loop); fix the `:169` ceiling (DOCS-16). |
| `##` | Dropped and deferred | What is not here and why. | Current `:178-192`; `archive` is *not implemented*, not feature-gated (DOCS-10); add the If-None-Match deferral folded from `put-path-memory.md:111-127` (DOCS-08). |
| `##` | Roadmap | Only what is still open, each item naming the code it would change. | Current `:194-206`, **with the flagship item rewritten per DOCS-21**: retire the streaming half (shipped, `c13a4d1`, `crates/longtail/src/version.rs:65,86-96`), drop the unimplementable mtime half and the "(as golongtail does)" attribution, re-measure the prize in the default configuration; carry R7 `OPS-DOC-02`'s invariant warning. |
| `##` | Safety posture | The authoritative `unsafe`/`forbid` inventory — **the single source**; `CLAUDE.md` §Safety links here rather than restating (DOCS-05). | Current `:208-223`, which R6 confirms is correct as written; take the 8-row table from `docs/review/06-security.md` verbatim if it is more complete, and tighten the `:210` lead sentence once the three clean bench bins gain `forbid`. |
| `##` | Upstream citation policy `[NEW]` | How to cite C/Go so a citation is still checkable in a year. | The policy section below. Alternatively lives in `CLAUDE.md` §Conventions — it belongs in exactly one of the two. |

### `docs/format-spec.md` — target (~730 lines; today 670)

Charter: the authoritative on-disk format reference. Every claim is about *bytes*, cites the C or Go
that produces them, and is verifiable against a tree that exists in this repo.

| # | Heading | Charter | Source |
|---|---|---|---|
| `#` + provenance block | Longtail On-Disk Format Specification | Name the exact in-repo citation base and pin; state plainly that Go citations have none. | **Replace** `:3-10`: `support/longtail-sys/longtail` @ `96241fe` (`git submodule status`); delete the `UPSTREAM_VERSION` instruction (dead — DOCS-03); add an outbound link to `rust-port.md` (DOCS-09). |
| `##` | Cross-cutting rules | Invariants true of every format. | Current `:12-27`. |
| `##` | 1. VersionIndex (`.lvi`) | Header, arrays, name encoding, sort order, strictness. | Current `:29-153` + R1 `FMT-DOC-07`; move `:155-162` out to §11. |
| `##` | 2. StoreIndex (`.lsi`) | Header, arrays, naming, sharding, and the merge contract. | Current `:164-210` + R1 `FMT-DOC-03` (the fs lock file the port does not implement), `FMT-DOC-04` (block order is non-deterministic), `FMT-DOC-01` (merge byte-identity, currently attached to a private helper) + the two-file steady state and delete-only-what-you-merged rule from `docs/put-path-memory.md:21-36` (DOCS-08). |
| `##` | 3. StoredBlock (`.lsb`) | Layout, payload framing, path scheme. | Current `:212-285` + R1 `FMT-DOC-06` (temp-file shape is not format), `FMT-DOC-09` (no version field, and the consequence). |
| `###` | 3.x Local cache blocks (`.lrb`) `[NEW]` | The launcher-cache layout the port must read byte-compatibly. | `crates/longtail-store/src/cache.rs:4-14,68-71,214-218`; gate at `support/longtail-testkit/tests/downsync_three_way.rs:432-445` (DOCS-12). |
| `##` | 4. Compression IDs | ID table + registry dispatch rule + the non-gate statement. | Current `:287-314` + the encode-parameter tables lifted out of `crates/longtail-core/src/compress.rs:28-67` (DOCS-DOC-02). |
| `##` | 5. Hash IDs and hash-input definitions | IDs, digest→u64 mapping, what bytes each hash consumes. | Current `:316-344`. Verified correct. |
| `##` | 6. HPCDC Chunker | Constants, derivation, discriminator, boundary loop, the two entry points. | Current `:346-532` + R2 `ALG-DOC-04` (tail threshold is `params.min` not 48; the hardcoded `m_EnableFileMap = 0`; the accepted `avg` range, which is DOCS-16's number). |
| `##` | 7. Permission bits | Bit layout and both platform mappings. | Current `:534-581` + R7 `OPS-DOC-05` (the Windows write-back consequence for downsync). |
| `##` | 8. ArchiveIndex (`.la`) | Header shape only; explicitly *not implemented*. | Current `:583-608`; drop "the archive feature" (DOCS-10). |
| `##` | 9. Edge cases | The traps a reimplementer falls into. | Current `:610-653` + R1 `FMT-DOC-05` (the misalignment bullet points at the wrong counts). |
| `##` | 10. Golden-file test matrix | Which spec section each gate proves; the gate table itself lives in `rust-port.md`. | Current `:655-671`, reduced to a mapping + link (DOCS-07); fix the meow claim at `:665`. |
| `##` | 11. Caller-side artifacts (informational) `[NEW]` | Paths and JSON that are caller convention, not format — collected in one place so §1-§10 are purely about bytes. | Current `:155-162` promoted out of §1 + the get-config schema from `crates/longtail/src/put.rs:166-187` and `get.rs:42-70` + `docs/switchover-checklist.md:31` (the `put` path-defaulting rules). |

---

## Deliverable (c) — disposition of every non-keeper document

One line each. "Fold" always names the destination section from the outlines above.

| Document | Lines | Last touched | Disposition |
|---|---|---|---|
| `docs/bench-2026-07-05.md` | 495 | 2026-07-06 | **Keep with expiry** — the only record of download-path numbers and the only evidence behind `rust-port.md` §Performance; it is a dated snapshot, so retire it when a newer download-path bench supersedes it. One edit required first: retitle §7 Verdict 2 as superseded (DOCS-20). |
| `docs/bench-2026-08-03.md` | 193 | 2026-08-04 | **Keep with expiry** — same, for the upload path; it is the evidence that the Fellowship OOM vector is closed. Must be linked from `rust-port.md` §Performance or it stays unreachable (DOCS-09). |
| `docs/put-path-memory.md` | 254 | 2026-08-04 | **Delete after folding** — superseded by its own §Resolution and by the Aug 3 bench, and it argues with itself (DOCS-08). Fold `:21-36` (two-file steady state) → `format-spec.md` §2; fold `:111-127` (If-None-Match deferral + the content-addressed-shard caveat) → `rust-port.md` §Dropped and deferred. Everything else is git history. |
| `docs/switchover-checklist.md` | 223 | 2026-07-06 | **Keep with expiry, gated on its own §Sign-off table** — it is a one-shot runbook whose table (`:211-223`) is still empty, so it has a job to do; but it must first be **emptied of reference material**: flags → clap `--help` (DOCS-01), divergences `:46-67` → `rust-port.md` §CLI compatibility, `put` path-defaulting `:31` → `format-spec.md` §11. Fix `:18-19`, `:44`, `:196`, `:203-206` before anyone runs it (DOCS-02, DOCS-13). Delete when the sign-off table is complete. |
| `fixtures/README.md` | 40 | 2026-07-06 | **Keep — it is a de-facto fifth keeper, not a working document.** It is the local README for a committed data directory, is accurate as written, and is correctly linked from `CLAUDE.md:41`. Treating it as disposable would be a mistake; the only change it needs is if `xtask gen-fixtures` loses its C dependency under R2's decision, since `:9-11` promises regeneration is possible. |
| `support/longtail-sys/README.md` | 17 | 2026-07-06 | **Delete with its crate** on R2's exit date. Until then, fix `:3-6` and `:16-17` — the build story and the upgrade procedure are both wrong (DOCS-06). |
| `support/longtail-ffi/README.md` | 16 | 2026-07-06 | **Delete with its crate** on R2's exit date. Until then, fix `:12-13` (DOCS-06). |
| `docs/review/*.md` | 4 831 | — | Out of scope for disposition here; R10 merges them. They should not outlive the punchlist they produce. |

---

## Deliverable — upstream citation policy

**The problem, quantified.** 341 line-numbered citations to upstream C/Go, on 329 lines, across 47
files. Only 14 carry an inline commit pin — so **327 (96%) are pinned only by a sentence in a
document the reader may never open**. 221 are C/H against 17 distinct files, all resolvable in the
in-repo submodule at `96241fe`; **120 are Go against 22 distinct files, none of which exists
anywhere in this repository**. Nothing checks any of them. My nine spot-checks against the submodule
all resolved within ±2 lines, so the citations are accurate *today* — this is a proposal about
keeping them so, not a claim that they have already rotted.

**Three structural weaknesses**, in order of cost:

1. **The base is not in the repo.** `format-spec.md:5` names `~/github/longtail` and
   `~/github/golongtail`. The C tree is actually vendored at `support/longtail-sys/longtail` at
   exactly the cited commit; the Go tree is not vendored at all. So one third of the corpus is
   already unverifiable by anyone but the author, and the other two thirds *look* unverifiable
   because the doc points elsewhere (DOCS-03).
2. **Line numbers rot silently.** A moved function makes the citation point at unrelated code with
   no signal. The failure is silent and the recovery is expensive: a reader cannot distinguish "the
   line moved" from "the claim was wrong", so the safe response is to re-derive.
3. **The pin is ambiguous.** The repo has three upstream identities in play — submodule
   `v0.3.3-101-g96241fe`, the dead `UPSTREAM_VERSION = "v0.4.3"`, and fixtures from golongtail
   `v0.4.5` — and the citation convention names only the first two, inconsistently (R2 `ALG-10`).

**Proposed policy.**

- **Symbol first, line number never alone.** A citation names the upstream *symbol* —
  `Longtail_MergeStoreIndex` in `src/longtail.c`, `tryAddRemoteStoreIndex` in
  `remotestore/remotestore.go`. A line number may follow as a convenience but is never the only
  locator. `grep` finds a symbol after a refactor; it does not find a line.
- **One central pin table**, in `CLAUDE.md` §Conventions or `rust-port.md`, with three rows: the C
  library (repo, commit, and the in-repo path that carries it), golongtail source (repo, commit, and
  the honest statement that no in-repo copy exists), and the golongtail *binary* used for fixtures
  (`v0.4.5`, `fixtures/manifest.json`). Individual citations stop carrying `@sha` entirely.
- **Line numbers survive only where the claim is about a specific statement**, not a function —
  e.g. `longtail.c:7307`'s wrong-index read (`rust-port.md:131,160`) or the hardcoded
  `m_EnableFileMap = 0` (R2 `ALG-DOC-04`). Those keep symbol + line, because the line *is* the point.
- **Vendor the Go source or drop the Go line numbers.** There is no third option that leaves the 120
  Go citations checkable. Vendoring golongtail as a second submodule is the cheaper of the two if the
  citations are considered load-bearing; if not, the Go citations become symbol + file only.

**Migration price, measured.** Of the 341 citations, **132 (39%) already name an upstream symbol on
the same line** — those need only the line number deleted or demoted, which is mechanical.
**209 (61%) do not**, and each needs a human to open the cited line and name what is there. At a
conservative one minute each that is ~3.5 hours of work, concentrated in nine files:
`docs/format-spec.md` (101 citations), `crates/longtail-core/src/store_index.rs` (28),
`crates/longtail-store/src/sync.rs` (22), `crates/longtail/src/downsync.rs` (14),
`crates/longtail-core/src/compress.rs` (14), `crates/longtail/src/apply.rs` (13),
`crates/longtail-core/src/pack.rs` (13), `crates/longtail-store/src/remote.rs` (12),
`crates/longtail-core/src/build.rs` (12). Everything else is single digits.

**The check that makes it stick.** An `xtask check-citations` that resolves every
`<symbol>` cited against `<file>` in the pinned submodule and fails on a miss is ~100 lines and runs
in under a second — it is a `grep` per citation. Under the *current* line-number convention the
equivalent check is not possible at all: a line number is always "valid", it just points somewhere
else. That asymmetry is the strongest argument for the symbol form. Note the check has a
prerequisite: **45 of the 221 C citations use a shortened basename that matches no file in the tree**
(`hpcdcchunker.c` ×26 → `lib/hpcdcchunker/longtail_hpcdcchunker.c`, `compressblockstore.c` ×10,
`fsblockstore.c` ×6, `concurrentchunkwrite.c` ×3), so either the citations are normalized to real
paths or the checker carries an alias table.

**Interaction with R2's decision.** If the legacy crates are deleted (R2 Option A), the submodule
goes with them and the C base becomes as unreachable as the Go base. The citation base must therefore
be an explicitly-owned artifact, not a side effect of the FFI crates existing. If Option B is taken,
the submodule is already the right base and only the documents need to say so.

---

## Lower-priority observations

- `docs/format-spec.md` has **zero outbound links** in 670 lines. A reader who arrives there (it is
  the second link in `readme.md`) has no route to `rust-port.md`.
- `docs/rust-port.md:31` says the chunker is an "exact port" while `:138-140` documents a deliberate
  divergence in the same chunker. Both are true at different scopes; the adjacency is confusing.
- `docs/put-path-memory.md:149-151` claims `--enable-file-mapping` is "parsed and plumbed but never
  consulted by the library (dead code)". If still true after `c13a4d1`'s streaming scan chunker, it is
  a user-visible no-op flag that `format-spec.md:516-521` treats as meaningful — worth one line in
  `rust-port.md` §Deliberate divergences. R7 owns the CLI side; flagging the doc inconsistency only.
- `docs/switchover-checklist.md:64-67` (minio virtual-host addressing, `MINIO_DOMAIN`) is duplicated
  almost verbatim at `crates/longtail-cli/tests/s3_interop.rs:7-12`. The test comment is the better
  version (it says it was proven in a manual smoke test); the doc copy is the one that will drift.
- `CLAUDE.md:90-91` and `docs/rust-port.md:40-41` both describe `LONGTAIL_WORKER_COUNT` as "the old"
  variable. It is still read at `support/longtail-ffi/src/commands.rs:544`, so the framing is right,
  but both sentences become dangling when the legacy crates go.
- `support/longtail-bench/src/bin/e2e.rs:37-43` documents all nine `LONGTAIL_BENCH_*` variables in
  its module doc. This is the right pattern and the right altitude — cite it as the model when fixing
  DOCS-11.

## Comments & documentation issues

In-source comments only. Metrics from a worker sweep across `crates/{longtail-core,longtail-store,longtail,longtail-cli}/src`,
spot-verified by me at the sites cited.

### `DOCS-DOC-01` — the public API's option structs are 43 undocumented public fields behind a one-line module doc that mislabels the module

**P2** · `idiom` · CONFIRMED

- **Where:** `crates/longtail/src/options.rs:1`, `:118-136`, `:178-210`, `:211-241`.
- **What:** `options.rs` is 320 lines and its entire module doc is one line:
  *"Public options + report types for the **download-path** facade API."* The module also declares
  `UpsyncOptions` (`:211`) and `UpsyncReport` (`:278`) — the upload path — so the one sentence it has
  is wrong about its own contents. Of 88 public fields, **43 carry no `///`**: all ten of
  `DownsyncStoreStats` (`:127-136`), both of `PhaseTiming` (`:119-120`), fifteen of `GetOptions`
  (`:189-204`), fifteen of `UpsyncOptions` (`:222-239`), and `UpsyncReport.phases` (`:281`).
  `DownsyncOptions`' fields, by contrast, are fully documented — so the same-named field is
  documented on one struct and not on its two siblings.
- **Failure scenario:** This file *is* the public API of the facade — it is what the Tauri app
  imports. A consumer setting up `GetOptions` sees fifteen bare fields (`scan_target`,
  `cache_target_index`, `target_index_path`, `use_legacy_write`, …) whose meaning is only recoverable
  from `DownsyncOptions`' docs for the identically-named field, if they think to look. Two of them are
  traps: `use_legacy_write` returns a typed error if set (`crates/longtail/src/error.rs:59-62`), and
  `enable_file_mapping` may be a no-op (see Lower-priority). Neither says so at the field.
- **Evidence:** `crates/longtail/src/options.rs:1,19,118-136,160,178-210,211-241,278-281`;
  field-doc census scripted over the file.
- **Recommendation:** Document the 43 fields, and make the module doc name both paths. `GetOptions`
  and `UpsyncOptions` fields that mirror `DownsyncOptions` can carry a one-line reference rather than
  duplicated prose.
- **Tradeoff / risk:** None.
- **Effort:** M
- **Regression test to add:** `#![warn(missing_docs)]` on `crates/longtail` — see `DOCS-DOC-05`.

### `DOCS-DOC-02` — `compress.rs`'s 72-line module doc carries a format table that belongs in the spec

**P3** · `complexity` · CONFIRMED

- **Where:** `crates/longtail-core/src/compress.rs:1-72` (72 `//!` lines over a 452-line file);
  `crates/longtail-store/src/remote.rs:1-52`; `crates/longtail-core/src/lib.rs:1-42`.
- **What:** Three module docs are long enough that they compete with the code below rather than
  introduce it:
  - `compress.rs` — 72 lines. `:28-67` is an "Encode levels / params — cited from the C source"
    inventory including a zstd table (`:33-39`) and a brotli table (`:57-64`). Those values duplicate
    the constants in the codec impls below *and* the ID inventory in `format-spec.md` §4, which the
    module doc itself cites at `:2` and `:50`. The one piece that is not derivable from either — the
    `zstd_low` shadowed-macro quirk at `:41-50` — is genuinely worth having, and is also already in
    `format-spec.md:297`.
  - `remote.rs` — 52 lines. `:28-44`'s mechanics are restated as inline comments at the
    implementation sites (`:457-462`, `:498-509`, `:615`), and `:45-52` duplicates the drain helper's
    doc at `:615`. R3's `STORE-DOC-10` separately found the module's liveness invariant restated in
    five places; this is the same pattern one level up.
  - `lib.rs` (core) — 42 `//!` lines over an 84-line file, i.e. **half the file**. `:22-31` is a
    benchmark verdict ("PERF (owned structs)") already recorded in `bench-2026-07-05.md` §5 /
    Verdict 3 — a port decision, not a crate contract.
- **Failure scenario:** Two costs. A reader looking for the `zstd` level constants now has three
  places that can disagree (the module doc table, the impl, and `format-spec.md` §4) — and the module
  doc is the one no test touches. And a 72-line preamble is skipped, which defeats the point of the
  ~22 lines in it that are genuinely module-level.
- **Evidence:** `crates/longtail-core/src/compress.rs:2,11-13,28-67,41-50,69-72`;
  `crates/longtail-store/src/remote.rs:28-52,457-462,498-509,615`;
  `crates/longtail-core/src/lib.rs:22-31`; `docs/format-spec.md:287-314`.
- **Recommendation:** Move the encode-parameter tables to `format-spec.md` §4 (already in the target
  outline), the "user decision 2026-07-05" provenance and the owned-structs verdict to
  `rust-port.md`, and let the module docs keep the codec inventory, the dispatch rule, the non-gate
  statement, and the topology bullets.
- **Tradeoff / risk:** Moving format facts into the spec puts them further from the code. That is the
  correct direction here — the spec is the artifact under compat obligation, and the module doc can
  carry a one-line pointer.
- **Effort:** S
- **Regression test to add:** n/a.

### `DOCS-DOC-03` — nine comments narrate the port or cite crates scheduled for deletion

**P3** · `idiom` · CONFIRMED

- **Where:** `crates/longtail-store/src/blob/s3.rs:91-94`, `:101-104`;
  `crates/longtail-store/src/cache.rs:221-222`; `crates/longtail-store/src/blob/fs.rs:13`;
  `crates/longtail-store/src/remote.rs:499-502`, `:797-800`;
  `crates/longtail/src/downsync.rs:1-2`; `crates/longtail-core/src/compress.rs:11-13`;
  `crates/longtail-store/src/sync.rs:17-20`.
- **What:** The codebase is, overall, disciplined here — a sweep of the four crates found **zero**
  `TODO`/`FIXME`/`XXX`/`HACK` markers in `src/` (the one regex hit,
  `crates/longtail-cli/src/progress.rs:5`, is the literal `XXX/YYY` in a format description). Nine
  sites do narrate rather than explain, in two groups:
  - **Repo archaeology:** `s3.rs:101-104` ("This was historically disabled to dodge a pre-GA SDK
    bug"), `remote.rs:499-502` and `:797-800` ("the old path cloned the whole union store index" /
    "the old path materialized a full copy") — these describe a state of this repository that no
    longer exists, i.e. commits `78c2c46` and `f2edb58`. The *contract* each states (targeted query,
    no clone) is design and should stay; the "old path" framing is git history.
  - **Citations into crates scheduled for deletion:** `s3.rs:91-94` and `cache.rs:221-222` (the
    legacy FFI `get_with_cache` path), `fs.rs:13` (`ffi's fsstore.rs:49` panic),
    `downsync.rs:1-2` ("mirrors `cmd_downsync.go` + the ffi `commands.rs` map"). R2's `ALG-DOC-08`
    found the same pattern at `chunker.rs:143`; this is the rest of it. Each becomes an unresolvable
    reference the day the crates go.
  Separately, `sync.rs:17-20` is a comment that corrects a *different* comment ("⚠ The
  `fs_store_index_sync_with_locking` spec-stub doc-comment attributes the fs lock to `store.lsi.sync`;
  that is C's…"). The fix belongs at the mis-citing site, not in a warning about it — and R1's
  `FMT-DOC-03` covers the underlying spec claim.
- **Failure scenario:** Each `ffi/...` citation is a dangling pointer on a known date. A maintainer
  after the deletion reads "the deliberate divergence from the legacy FFI `get_with_cache` path" and
  cannot check what that path did — the justification for the current default becomes unverifiable
  exactly when it is most likely to be questioned.
- **Evidence:** the nine sites above, each read; `rg -n 'TODO|FIXME|XXX|HACK' crates/*/src` →
  one non-marker hit.
- **Recommendation:** For the archaeology group, keep the contract sentence and drop the "old path"
  clause. For the FFI-citation group, state the behaviour being diverged from rather than pointing at
  the file — e.g. record that acceleration was previously hardcoded on, without a path that will
  vanish. R2's `ALG-DOC-08` should be executed as one sweep with this.
- **Tradeoff / risk:** Some provenance is genuinely useful until the deletion happens. That argues
  for doing this sweep *as part of* the deletion commit, not before.
- **Effort:** S
- **Regression test to add:** after deletion, `rg 'longtail-ffi|longtail-sys' crates/` must return
  nothing — a one-line CI grep.

### `DOCS-DOC-04` — nine permission-bit constants are documented by a non-doc comment

**P3** · `idiom` · CONFIRMED

- **Where:** `crates/longtail-core/src/perms.rs:15-24`.
- **What:** `perms.rs` is 101 lines with a one-line module doc (`//! POSIX-style permission bits
  (docs/format-spec.md §7).` — which is the *right* kind of thin: it points at the authority). But
  its nine bit constants (`OTHER_EXECUTE` … `USER_READ`, `:16-24`) are introduced by a `//` comment at
  `:15` (`// §7 bit constants (octal, matching src/longtail.h:314-324).`) rather than `///`, so none
  of the nine appears documented in rustdoc while the neighbouring `POSIX_MASK` (`:27`) and all three
  `pub const fn` are. This is the single largest cluster of undocumented public constants in the
  workspace.
- **Failure scenario:** Small but exact: rendered docs for `Permissions` show nine bare `u16`
  constants with no indication that they are POSIX bit positions or that the values are octal. The
  information exists one line above in a form rustdoc discards.
- **Evidence:** `crates/longtail-core/src/perms.rs:1,12,15-27,31-43`.
- **Recommendation:** Promote `:15` to `///` on the first constant, or give the block a
  `/// # POSIX bit positions` doc-section. One character per line for the rest.
- **Tradeoff / risk:** None.
- **Effort:** S
- **Regression test to add:** covered by `DOCS-DOC-05`.

### `DOCS-DOC-05` — assessment: adopt `#![warn(missing_docs)]` on the two library crates, not a workspace-wide `deny`

**P2** · `hardening` · CONFIRMED (measurement) / recommendation

- **Where:** `crates/longtail-core/src/lib.rs:43`, `crates/longtail-store/src/lib.rs:19`,
  `crates/longtail/src/lib.rs:32`, `crates/longtail-cli/src/main.rs:5` — none carries any
  `missing_docs` attribute.
- **What (the measurement).** Module-doc coverage is already **100%**: all 47 files under the four
  crates' `src/` have a `//!` block (564 `//!` lines total). The gap is *item* docs. Exact counts for
  a clean `deny(missing_docs)` — **216 docs to author**:

  | crate | mod-scope items | struct fields | impl fns | impl consts | **total missing** |
  |---|---|---|---|---|---|
  | `longtail-core` | 13 | 16 | 1 | 9 | **39** |
  | `longtail-store` | 8 | 22 | 0 | 0 | **30** |
  | `longtail` | 8 | 129 | 10 | 0 | **147** |
  | `longtail-cli` | 0 | 0 | 0 | 0 | **0** |

  Character of the set, which is what makes the decision:
  - **167 of 216 are public struct fields**, and 129 of those are in `longtail` — concentrated in
    `options.rs` (43, `DOCS-DOC-01`), `prune.rs` (26), `inspect.rs` (17), `put.rs` (15),
    `clonestore.rs` (15), plus `longtail-store/src/block_store.rs` (20, the two stats structs).
  - **26 of the 29 mod-scope "items" are bare `pub mod x;` lines** whose module file already has a
    `//!` block — `missing_docs` does not fire on those, so the real mod-scope cost is **3**
    (`crates/longtail/src/fs_util.rs:332`, `upsync.rs:32-33`).
  - The 10 impl fns are all `pub fn new` constructors in `longtail`.
  - `longtail-cli` is a bin crate; the lint is moot there.
- **Assessment.** A workspace-wide `deny` is the wrong shape: 147 of the 216 land on one crate, and
  a hard deny would either block work until they are written or invite a blanket `#[allow]` that
  defeats it. But the *distribution* is exactly what a ratchet is for — `longtail-core` (39) and
  `longtail-store` (30) are a day's work between them, and those two are the crates a third party
  reads. `longtail` is the public API and has the worst coverage, which is the argument for fixing it
  rather than for skipping the lint.
- **Failure scenario for not adopting it:** the four `08b` rustdoc defects went unnoticed because
  nothing runs `cargo doc` (DOCS-04). Doc quality here is unmeasured, so it drifts in one direction.
  `DOCS-DOC-01` and `DOCS-DOC-04` are both instances of a public item shipping without a doc and
  nothing noticing.
- **Recommendation:** Adopt `#![warn(missing_docs)]` on `longtail-core` and `longtail-store` now
  (69 docs), and on `longtail` after `options.rs` is fixed (`DOCS-DOC-01`, 43 of its 147). Escalate
  to `deny` per crate as each reaches zero. Pair it with DOCS-04's `docs` CI job — the lint is
  worthless without a job that runs it. Do not lint `longtail-cli`.
- **Tradeoff / risk:** `warn` without a `-D warnings` job is decoration; `deny` before the backlog is
  cleared is a blocker. The per-crate ratchet is the only sequencing that is neither.
- **Effort:** M for core + store; L for `longtail`.
- **Regression test to add:** the DOCS-04 `docs` job with `-D warnings` makes the ratchet binding.

### `DOCS-DOC-06` — cross-reference: three in-source doc defects owned by other reviewers

Not restated here; listed so the merged punchlist does not lose them and so this document's
comment-quality picture is complete:

- `crates/longtail-core/src/store_index.rs:189-227` — the `Longtail_MergeStoreIndex` byte-identity
  contract is one contiguous `///` block that attaches to the **private** `fn reserve_capacity`
  (`:220`), leaving `pub fn merge` (`:229`) undocumented and the flagship contract invisible to
  rustdoc. **R1 `FMT-DOC-01`.** Independently confirmed by reading `:186-232`; it is also the
  mechanical cause of one line in `DOCS-DOC-05`'s count.
- `crates/longtail-core/src/error.rs:4` and `lib.rs:40-42` — "Malformed input never panics" /
  "must always surface as an `Err` — never a panic". **R1 `FMT-DOC-02`** disproves it. Verified the
  text at both sites.
- `crates/longtail-store/src/{remote.rs:560-562, cache.rs:213,260, sync.rs:457, blob/mod.rs:186-187,
  s3.rs:231-234}` — five comments that overclaim or misdescribe. **R3 `STORE-DOC-02,05,07,09,04`.**
  DOCS-13 files only the propagation of the first into the operator runbook.

## Hardening backlog

Ranked by ratio of drift prevented to effort.

1. **`docs` CI job** (`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace`) + `doc = false`
   on `crates/longtail-cli`'s `[[bin]]`. Closes DOCS-04 permanently and is the precondition for the
   `missing_docs` ratchet. **S.**
2. **`xtask check-doc-links`** — fail on a link to a nonexistent file, and fail if any `.md` outside
   `docs/review/` is unreachable from `readme.md`. Would have caught DOCS-09 and DOCS-14. **S.**
3. **Clap help-text test** — every argument in the `Command` tree has non-empty help. Makes DOCS-01
   unrepeatable and closes R7 `OPS-DOC-01`. **S.**
4. **`xtask check-features`** — every feature named in backticks in `docs/*.md` resolves in some
   `Cargo.toml`, and every declared feature appears in the `CLAUDE.md` table. Closes DOCS-10 and
   DOCS-15 in both directions. **S.**
5. **`xtask check-citations`** — resolve every cited upstream symbol against the pinned submodule.
   Requires the symbol-form migration and the basename normalization (see the policy section);
   impossible under the current line-number convention. **M**, after the policy lands.
6. **Gate-table path check** — every test file named in `rust-port.md`'s gate table exists. Makes
   DOCS-07's reconciliation self-maintaining. **S.**
7. **`#![warn(missing_docs)]`** on `longtail-core` then `longtail-store` then `longtail`, escalating
   to `deny` per crate. **M/L**, per `DOCS-DOC-05`.
8. **Post-deletion grep** — after the legacy crates go, `rg 'longtail-ffi|longtail-sys' crates/`
   must return nothing (`DOCS-DOC-03`). **S.**

## Verified good

So the next session does not redo these:

- **`readme.md`'s code samples are correct.** Every symbol, arity, field, and flag in `:26-61`
  resolves: `downsync` at `crates/longtail/src/lib.rs:55`, `DownsyncOptions` at `:65-68`,
  `DownsyncOptions::new(Vec<String>, impl Into<String>, impl Into<String>)` at `options.rs:83-87`,
  `pub cache_path: Option<PathBuf>` at `options.rs:29`, `pub async fn downsync(opts) -> Result<…>`
  at `downsync.rs:30`; the four CLI flags at `main.rs:86-104,138-150`.
- **`CLAUDE.md` §CI is accurate.** All four workflow files, their triggers, and their jobs check out
  against `.github/workflows/{rust,audit,fixture-freshness,s3-minio}.yaml`. No workflow is
  unmentioned. (One loose edge: `audit.yaml`'s `Cargo.{toml,lock}` trigger is `push`-only, so it is
  not a PR check — the wording is compatible but imprecise.)
- **`CLAUDE.md` §Common commands all resolve.** `differential` and `fastcdc` features declared;
  `xtask fetch-golongtail`/`verify-fixtures`/`gen-fixtures`/`diff-fixtures` all exist;
  `test-data/mkdata.{sh,ps1}` exist; `rustfmt.toml` is `max_width = 100`; `default-members` matches
  `:46-50` exactly.
- **`CLAUDE.md` §Runtime configuration is accurate** except the logging sentence (R7 `OPS-DOC-03`):
  `longtail-core` has no tokio dependency; no library builds a runtime; the four `*_blocking`
  wrappers exist at `crates/longtail/src/lib.rs:103-145`; credentials are provider/client, not
  snapshot.
- **No root-`README.md` casing defect exists.** The seeded concern does not hold — see DOCS-18.
- **`fixtures/README.md` is accurate and correctly linked.** Not a working document; treat as a
  keeper.
- **`format-spec.md`'s C citations resolve.** Nine spot-checks against the in-repo submodule at
  `96241fe` all landed within ±2 lines, several exact. The corpus is accurate today; DOCS-03 is about
  keeping it so, not about existing rot.
- **Module-doc coverage is 100%** — all 47 files under the four crates' `src/` carry a `//!` block.
- **Zero `TODO`/`FIXME`/`XXX`/`HACK` markers** in the four crates' `src/`.
- **`support/longtail-bench/src/bin/e2e.rs:37-43`** documents all nine `LONGTAIL_BENCH_*` variables
  at the right altitude. Use it as the model for DOCS-11.

## Experiments requested

| # | Hypothesis | Exact command | What would change the finding |
|---|---|---|---|
| 1 | Fixing the seven `08b` link defects plus `doc = false` on the CLI bin leaves a clean doc build under `-D warnings`, so DOCS-04's CI job is adoptable as-is. | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features 2>&1 \| tail -40` | A residual warning class I have not seen (e.g. `--all-features` surfacing new links in the `differential`-gated testkit module) would mean the job needs `--no-default-features` variants or a narrower package list, changing DOCS-04's recommendation from "add the job" to "add the job plus N further fixes". |
| 2 | The default-feature doc build (no `differential`, no `fastcdc`) can be made clean without `--all-features`, so the `docs` job needs no C toolchain. | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps -p longtail-core -p longtail-store -p longtail -p longtail-cli 2>&1 \| tail -40` | If defects #1 and #3 (the feature-gated links) are the only blockers, the job is free and DOCS-04's tradeoff paragraph drops entirely. If other defects appear only in this configuration, the job needs both variants. |
| 3 | `--enable-file-mapping` is still a parsed-but-unused no-op after `c13a4d1`, making `put-path-memory.md:149-151` correct and `format-spec.md:516-521` misleading about its significance. | `rg -n 'enable_file_mapping' crates/ --type rust` and read every hit past the clap definition and the options structs | If the flag now reaches a code path, the Lower-priority observation is void. If it does not, it becomes a `rust-port.md` §Deliberate divergences entry ("accepted and ignored") and R7 should carry the CLI half. |

## Open questions for the maintainer

1. **Is `docs/switchover-checklist.md` still going to be executed?** Its §Sign-off table is empty, so
   the staging gate has not run. If it will run, it is a live operational document and must be fixed
   before use (DOCS-02, DOCS-13). If the switchover has already happened by another route, it is
   deletable today and DOCS-01's urgency shifts entirely onto the clap help-text work.
2. **Which upstream trees are the citation base going to be, after R2's Option A/B decision?** The
   citation policy's shape depends on it: under Option B the submodule is already the right base and
   only the docs need updating; under Option A the C base disappears with the crates and the 221 C
   citations need the same treatment I propose for the 120 Go ones.
3. **Should `fixtures/README.md` be promoted to a named keeper?** It is accurate, linked, and
   describes committed data that outlives every working document. Treating it as disposable is the
   only part of the current keeper list I would change.
4. **Is `docs/` or `--help` the intended home for CLI flag reference?** DOCS-01's recommendation
   assumes `--help`. If the pipeline's authors want a single browsable table instead, that is a fifth
   keeper and the four-document plan needs revisiting.
5. **`docs/format-spec.md`'s interior needs one more verification pass that I did not complete.** The
   worker assigned to check its ~40 numeric constants (magic values, version IDs, hash/compression
   IDs, sizes, defaults) and ~30 Rust-path references against the code one by one did not return, and
   I did not redo that sweep by hand. What I *did* verify holds: the provenance block (DOCS-03), §10
   including the false meow claim (DOCS-07), the `archive` claim (DOCS-10), the `.lrb` gap (DOCS-12),
   the heading tree, and 9 of its 101 upstream citations — all nine exact. R1 independently checked
   §§1-3 and §9 and R2 checked §6, so the interior is not unexamined; what is missing is a systematic
   constant-by-constant diff of §§4-5 and §§7-8 against `crates/longtail-core/src/{compress,hash,perms}.rs`.
   Given that the spec is the authority for the paramount constraint, that pass is worth an hour
   before the doc is declared clean. No finding in this document depends on it.

## Files read

**Documents (full):** `/home/chris/work/longtail-rs/cm/rust-port/readme.md` ·
`/home/chris/work/longtail-rs/cm/rust-port/CLAUDE.md` ·
`/home/chris/work/longtail-rs/cm/rust-port/docs/rust-port.md` ·
`/home/chris/work/longtail-rs/cm/rust-port/docs/format-spec.md` ·
`/home/chris/work/longtail-rs/cm/rust-port/docs/put-path-memory.md` ·
`/home/chris/work/longtail-rs/cm/rust-port/docs/switchover-checklist.md` ·
`/home/chris/work/longtail-rs/cm/rust-port/docs/bench-2026-08-03.md` ·
`/home/chris/work/longtail-rs/cm/rust-port/fixtures/README.md` ·
`/home/chris/work/longtail-rs/cm/rust-port/support/longtail-sys/README.md` ·
`/home/chris/work/longtail-rs/cm/rust-port/support/longtail-ffi/README.md` ·
`/home/chris/work/longtail-rs/cm/rust-port/target/review-evidence/REVIEWER-CONTRACT.md` ·
`/home/chris/work/longtail-rs/cm/rust-port/target/review-evidence/MANIFEST.md`

**Documents (partial):** `/home/chris/work/longtail-rs/cm/rust-port/docs/bench-2026-07-05.md`
(headings, §7, §9) · the `## Comments & documentation issues` sections of
`/home/chris/work/longtail-rs/cm/rust-port/docs/review/{01-format-codecs,02-algorithms-and-oracle,03-store-concurrency,07-operations-cli}.md`,
plus `02`'s `ALG-DOC-03`/`ALG-DOC-04` and oracle table, `03`'s `STORE-DOC-02`, `07`'s `OPS-06`.

**Received as verified input, not read by me** (both landed after I began; attributed at the point of
use): R6's `unsafe`/`forbid` inventory in
`/home/chris/work/longtail-rs/cm/rust-port/docs/review/06-security.md` — **authoritative over my own
count in DOCS-05** — and R4's analysis of `rust-port.md` §Roadmap, which is the substance of DOCS-21,
including the orchestrator-verified absence of any `mtime`/`ModTime` reference in the pinned C source
and in golongtail at `49a20e1`.

**Source:** `crates/longtail-cli/src/main.rs` (`:22-400`, `:1039-1045`) ·
`crates/longtail-cli/Cargo.toml` · `crates/longtail/src/options.rs` ·
`crates/longtail-core/src/perms.rs` · `crates/longtail-core/src/hash.rs` ·
`crates/longtail-core/src/chunker.rs` (`:28-40`) · `crates/longtail-core/src/error.rs` (`:1-12`) ·
`crates/longtail-core/src/lib.rs` (`:36-46`) · `crates/longtail-core/src/store_index.rs`
(`:186-232`) · `crates/longtail-core/Cargo.toml` · `crates/longtail-store/src/cache.rs`
(`:1-25`, block-path helper, evict) · `crates/longtail-store/tests/s3_spec.rs` (`:1-13`, `:131-137`) ·
`crates/longtail-cli/tests/s3_interop.rs` (`:1-12`, `:34-43`) · `crates/longtail/src/version.rs`
(`:55-96`) · `crates/longtail/src/{get,put}.rs` (get-config schema) ·
`support/longtail-bench/Cargo.toml` · `support/longtail-bench/src/bin/e2e.rs` (`:1-60`, `:168-185`,
env sites) · `support/longtail-testkit/tests/hash_recompute_golden.rs` ·
`support/longtail-testkit/tests/chunker_differential.rs` (`:1-14`) ·
`support/longtail-sys/{Cargo.toml,build.rs}` · every `Cargo.toml` `[features]` block in the repo.

**Workflows:** `.github/workflows/{rust,audit,fixture-freshness,s3-minio}.yaml`.

**Upstream (submodule, for citation spot-checks):**
`support/longtail-sys/longtail/src/longtail.c` (`:2550-2556`, `:2604-2610`, `:2630-2638`,
`:7303-7311`, `:8911-8917`, `:8977-8983`, `:9143-9149`) ·
`support/longtail-sys/longtail/lib/hpcdcchunker/longtail_hpcdcchunker.c` (`:10-14`, `:124-131`).

**Evidence pack:** `MANIFEST.md` · `08-doc.txt` · `08b-doc-warnings.txt` · `08c-doc-fastcdc.txt` ·
`17-bloat.txt` · `12-loc.txt` · `13-fixtures.txt` · `00-scope.txt` · `11-featurematrix.txt`.
