# 05 · API surface, idioms & complexity review
- **Reviewed at:** `456274d` · **Lead model:** fable · **Workers:** 4 × fable
- **Slice:** the `pub` surface of `longtail`/`longtail-core`, the diagnostics contract across the
  four error enums, complexity/duplication in the facade + CLI, and the Tauri embedding contract.
- **Confidence:** covered well (facade read in full; `longtail-core` surface enumerated by worker,
  verdict-bearing rows re-verified by lead; `longtail-store` touched only along the declared
  error/runtime axes).

## Findings index

| ID | P | Dimension | One line | file:line | Verdict |
|---|---|---|---|---|---|
| API-01 | P1 | hardening | Zero `#[non_exhaustive]` in the workspace; options/error/report types freeze hard at the first tag, and cfg-gated `pub s3_options` fields make feature unification a downstream build break | `options.rs:19,77`, `error.rs:20` | CONFIRMED |
| API-02 | P1 | idiom | Take the OPS-07 break now: `read_{version,store}_index_from_uri` are the only two URI-taking pub fns without S3 options, and their optionlessness also strands 3 internal callers | `inspect.rs:25,88` | CONFIRMED |
| API-03 | P2 | hardening | Crates are publishable-by-default yet unpublishable (path deps carry no `version`); no tag exists, so semver-checks is inert — wiring spec below | `crates/longtail/Cargo.toml:12-13` | CONFIRMED |
| API-04 | P2 | idiom | Unnameable public surface: the four `DEFAULT_*` upsync consts back every constructor but can't be named by callers; `S3OptionsArg` is the type of 9 pub fields yet private | `upsync.rs:31-36`, `fs_util.rs:330` | CONFIRMED |
| API-05 | P2 | hardening | `CloneStoreOptions::source_zip_paths` (and CLI `--source-zip-paths`) is accepted and silently ignored — the zip fallback it promises is unimplemented | `clonestore.rs:36`, `main.rs:893-894` | CONFIRMED |
| API-06 | P2 | hardening | `put` writes `s3-endpoint-resolver-uri` into the get-config JSON; `get` never reads it back — a put→get round-trip silently drops the custom endpoint | `put.rs:168-174`, `get.rs:42-83` | CONFIRMED |
| API-07 | P2 | complexity | CLI S3/progress/cancel wiring is copy-pasted 12-13×; the newest S3 knob (stalled-stream) reached only 4 of 12 S3 subcommands — `put` and `clone-store`, the biggest transfers, can't opt out | `main.rs:619-626` vs `:730-737` | CONFIRMED |
| API-08 | P2 | complexity | The re-upload pipeline is duplicated between `upsync` and the `clone_store` loop; the copies diverge exactly where two filed bugs live (OPS-14, the never-cancelled token) | `upsync.rs:116-176`, `clonestore.rs:151-198` | CONFIRMED |
| API-09 | P3 | complexity | 11 verbatim cfg-gated `S3OptionsArg` bindings + 3 identical `default_s3()` fns across the op modules | `downsync.rs:92-95` et al. | CONFIRMED |
| API-10 | P3 | complexity | `BlockStoreOpts` literal ×10; the 1-thread rayon pool is built 5 ways with 2 different error strings; `validate_version` inlines the builder its own file wraps 20 lines later | `inspect.rs:56-61` vs `:78-85` | CONFIRMED |
| API-11 | P1 | hardening | The error-class contract the facade advertises has no API and no test; block-get flattening (STORE-04) breaks it in practice, and task panics surface as `Io` | `lib.rs:82-85`, `apply.rs:358` | CONFIRMED |
| API-12 | P2 | hardening | The library emits zero telemetry outside 4 cache-eviction events; `tracing` is an unused dep of `longtail`; the two silent fallbacks that most need a signal have none | `07b-machete.txt`, `downsync.rs:303`, `sync.rs:456` | CONFIRMED |
| API-13 | P2 | hardening | `downsync`/`upsync` block their polling tokio thread through `pool.install` for the whole Indexing/Validating phase — undocumented; freezes a current-thread runtime solid | `downsync.rs:117-126`, `version.rs:53` | CONFIRMED |
| API-14 | P2 | hardening | Progress callbacks fire on rayon workers (scan) and on tokio workers under a held `std::sync::Mutex` (apply); the threading contract is undocumented and a slow sink serializes the apply loop | `version.rs:69`, `apply.rs:236-243` | CONFIRMED |
| API-15 | P2 | hardening | Every rayon pool is built without a `panic_handler`, so a detached codec-task panic aborts the embedding GUI process (facade-side enabler of STORE-12) | `version.rs:170`, `store/compress.rs:44` | CONFIRMED |
| API-16 | P2 | hardening | Embedder cancellation is graceful-only with unbounded latency; the CLI's force-stop is `process::exit` — the embedder equivalent (task abort) silently skips flush/close/eviction | `lib.rs:71-81`, `apply.rs:248-260` | CONFIRMED |
| API-17 | P3 | idiom | Two parallel stringly-typed phase vocabularies (`on_phase` strings vs `PhaseTiming` names), both undocumented; a GUI must match magic strings that nothing stabilizes | `downsync.rs:104-214`, `options.rs:118` | CONFIRMED |
| API-18 | P3 | idiom | 16 user-facing io-error contexts format paths with `{:?}` — doubled backslashes + quotes in every error a Windows user reports | `apply.rs:81`, `fs_util.rs:105` … | CONFIRMED |
| API-19 | P3 | hardening | `Instant::now() - Duration::from_secs(1)` panics when the process starts <1 s after boot; runs at the start of every scan | `version.rs:141` | CONFIRMED |
| API-20 | P3 | hardening | `discriminator_from_avg` has no `#[allow(clippy::suboptimal_flops)]`; an applied `--fix` fuses the FMA and moves every chunk boundary · `COMPAT-RISK` | `core/chunker.rs:242-245` | CONFIRMED |

Doc findings `API-DOC-01` … `API-DOC-06` are indexed in their own section.

## Scope

- **Read in full:** `crates/longtail/src/{lib,error,options,progress,inspect,downsync,version,
  apply,get,cp,put,compression,hash_util}.rs`; `crates/longtail-core/src/error.rs`;
  `crates/longtail-store/src/error.rs`; `crates/longtail/Cargo.toml`.
- **Read in part (cited lines verified):** `clonestore.rs` (26-271 minus tests), `upsync.rs`
  (1-190), `path_filter.rs` (1-80), `fs_util.rs` (all error-context and URI-dispatch sites),
  `prune.rs` (78-95 + options spans), `longtail-cli/src/main.rs` (84-260, 460-800, 883-971 spans),
  `longtail-core/src/{store_index.rs:185-231, chunker.rs:230-250}`,
  `longtail-store/src/{blob/s3.rs:1-120, cache.rs:150-200, sync.rs:75-100 & 440-462,
  compress.rs:30-60}`, `docs/rust-port.md:20-75,189`.
- **Excluded:** algorithm internals (R2), actor/concurrency internals (R3), formats (R1), CLI flag
  parity (R7) — read only where my four deliverables required, cross-referenced not re-filed.

## Verification performed

- Evidence pack: `MANIFEST.md`, `00-scope.txt` (no tags), `02-clippy-pedantic.txt` (via worker d,
  every kept instance re-read at source by lead), `07b-machete.txt`, `08-doc.txt`,
  `12-loc.txt`, `18-semver.txt`.
- Four workers: (a) pub-surface enumeration, (b) complexity measurement, (c) op-module duplication,
  (d) pedantic triage. Every claim that became a finding was re-verified by reading the cited
  lines; the surface tables below are worker-enumerated with lead spot-verification of every
  verdict-bearing row (all sampled rows were accurate).
- **Could not verify:** golongtail's `cmd_get.go` handling of the `s3-endpoint-resolver-uri` config
  key (source not vendored) — API-06 stands on the in-repo put→get asymmetry alone. Rendered-rustdoc
  confirmation of API-DOC-01 is listed under Experiments.

## Findings

### `API-01` — Nothing is `#[non_exhaustive]`; the surface freezes at the first tag
**P1** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/options.rs:19-79,178-205,211-240`, `crates/longtail/src/error.rs:20`,
  `crates/longtail-core/src/error.rs:16`, plus every options/result struct in `inspect.rs`,
  `prune.rs`, `cp.rs`, `put.rs`, `clonestore.rs`. `grep -rn non_exhaustive crates/` → 0 hits.
- **What:** All 13 options structs have all-`pub` fields; `LongtailError` (14 variants), `StoreError`
  (16), and the six core error enums are exhaustively matchable. `DownsyncOptions.s3_options`
  (`options.rs:77`) and its 8 siblings are `#[cfg(feature = "s3")]`-gated pub fields.
- **Failure scenario:** (1) After the launcher pins the crate, *any* added option field, report
  field, or error variant is semver-major — HEAD itself just added such a field
  (`S3Options::stalled_stream_protection`, commit `456274d`). (2) Feature unification: a downstream
  crate that literal-constructs `DownsyncOptions` without `s3` stops compiling the day any other
  crate in its graph enables `s3` — cfg-gated pub fields break the features-are-additive rule.
- **Evidence:** the type docs already mandate the safe pattern ("Construct with
  [`DownsyncOptions::new`] … then set the optional fields", `options.rs:16-17`), so
  `#[non_exhaustive]` costs the documented workflow nothing.
- **Recommendation:** before the first tag, add `#[non_exhaustive]` to: every options struct with a
  `new()` (13), every report/stats struct, `LongtailError`, and the six `longtail-core` error
  enums (and note to R3: `StoreError`). Do **not** seal the codec structs (`VersionIndex` etc.) —
  their parallel-array shape is frozen by the C formats and `downsync.rs:277-293` literal-constructs
  one cross-crate.
- **Tradeoff / risk:** downstream loses struct-literal construction and exhaustive error matches —
  exactly the point. Error-handling code needs a wildcard arm.
- **Effort:** S
- **Regression test to add:** none (compile-time); the semver-checks gate in API-03 enforces it
  staying on.

### `API-02` — Adjudication: take the OPS-07 signature break now
**P1** · `idiom` · CONFIRMED
- **Where:** `crates/longtail/src/inspect.rs:25-28` and `:88-91`, re-exported `lib.rs:59-64`.
- **What:** Both readers hardcode `default_s3()`. The sweep for other "takes a URI but not the
  options that URI needs" signatures found **no others** — every other entry point carries
  `s3_options` in its options struct. But three of those entry points then call the option-less
  reader and strand their own options: `validate_version` (`inspect.rs:55`),
  `create_version_store_index` (`:205`), `print_version_usage_stats` (`:269`) — plus `cp.rs:60`.
  8 call sites total (those four + `main.rs:677,705,929,997`).
- **Failure scenario:** OPS-07's — a minio/custom-endpoint user's index read goes to AWS.
- **Evidence:** `18-semver.txt`: no baseline exists; nothing published; `git tag -l` empty
  (`00-scope.txt`). The break is free today and semver-major the day after the first tag.
- **Recommendation:** **break now.** Prefer a small options struct over a bare parameter so the
  signature never breaks again: `read_version_index_from_uri(uri, &ReadUriOptions)` where
  `ReadUriOptions` is `#[non_exhaustive] + Default` with a cfg-gated `s3_options` field (the same
  pattern every sibling uses; avoids exposing the `S3OptionsArg` alias in a bare param position).
  Thread `opts.s3_options` through the three inspect-internal callers and `cp.rs:60` in the same
  change — that is the actual OPS-07 fix.
- **Tradeoff / risk:** none today; 8 mechanical call-site edits.
- **Effort:** S
- **Regression test to add:** a `file://`-vs-custom-endpoint test asserting the index read uses the
  supplied options (an fsblob:// URI with options carrying a poisoned endpoint must still work;
  an s3:// URI must fail against the poisoned endpoint, not resolve to AWS).

### `API-03` — Publish metadata is incoherent; semver-checks has nothing to bite on
**P2** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/Cargo.toml:12-13` (`longtail-core`/`longtail-store` path deps, no
  `version`), same pattern in `crates/longtail-store/Cargo.toml:19`; no `publish = false` on any
  `crates/*` member (only `support/longtail-bench` has it); `version.workspace = true` → 0.1.0.
- **What:** The three library crates default to publishable but `cargo publish` would fail on the
  version-less path deps; meanwhile no git tag exists, so `cargo-semver-checks` has no baseline
  (`18-semver.txt`) and the surface is drifting unguarded.
- **Failure scenario:** post-switchover, an innocent field addition ships to the pinned launcher as
  a breaking change nobody detected — there is no gate that would fire.
- **Recommendation (the wiring spec):**
  1. Decide distribution (Open question #1). Either way, at the switchover commit create the
     baseline: `git tag v0.1.0` (git-dep consumption) — and if crates.io is intended, add
     `version = "0.1.0"` to both path deps first.
  2. Add to the per-PR pure lane (after the existing test job, no network needed):
     `cargo semver-checks check-release -p longtail-core -p longtail --baseline-rev $(git describe --tags --abbrev=0 --match 'v*')`.
     Pin the tool (evidence pack ran 0.50.0). The baseline must be a tag on this branch's lineage —
     `main` has the incompatible FFI-era layout and can never serve.
  3. On each release, tag; the `describe` recipe then ratchets automatically.
- **Tradeoff / risk:** semver-checks adds ~a compile of the baseline per PR; acceptable for two
  small pure crates.
- **Effort:** S
- **Regression test to add:** the CI step itself; Experiments #2 validates the recipe.

### `API-04` — Public surface callers can't name; names callers can't reach
**P2** · `idiom` · CONFIRMED
- **Where:** `crates/longtail/src/upsync.rs:31-36`; `crates/longtail/src/fs_util.rs:330-332`;
  `crates/longtail/src/hash_util.rs:26`.
- **What:** (1) `DEFAULT_TARGET_BLOCK_SIZE`/`_MAX_CHUNKS_PER_BLOCK`/`_TARGET_CHUNK_SIZE`/
  `_MIN_BLOCK_USAGE_PERCENT` are `pub` in the private `upsync` mod and not re-exported — they are
  the documented defaults of three constructors (`options.rs:255-258`, `put.rs:61-64`,
  `clonestore.rs:75-77`) yet a caller who changes one field cannot reset it by name. Two of the
  four also lack doc comments. (2) `S3OptionsArg` is the declared type of 9 `pub` fields
  (`cp.rs:35`, `prune.rs:125,253,350`, `inspect.rs:36,130,179,237`, `clonestore.rs:51-53`) but the
  alias lives in private `fs_util` — rustdoc renders a private path. (3) `hash_identifier_for_name`
  is pub-unreachable while its siblings `SyncHasher`/`make_hasher` are exported (`lib.rs:58`).
- **Failure scenario:** a Tauri caller tuning `target_block_size` hardcodes `8 * 1024 * 1024`; when
  a future release changes the default, the caller silently diverges from CLI behavior.
- **Recommendation:** re-export the four consts (e.g. from `options`); either export the alias
  under a real name or declare those 9 fields as `longtail_store::S3Options` directly (they are
  that type under the only feature combination where they exist); make
  `hash_identifier_for_name` `pub(crate)` or export it — not the current halfway.
- **Tradeoff / risk:** none.
- **Effort:** S
- **Regression test to add:** none; the missing_docs ratchet (API-DOC-05) catches the undocumented
  consts.

### `API-05` — `source_zip_paths` is accepted and silently ignored
**P2** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/clonestore.rs:36` (field), `:70` (initialized, never read —
  `grep -rn source_zip_paths crates/longtail/src` has no other hit); CLI wires it at
  `main.rs:893-894` from `--source-zip-paths` (`main.rs:394`).
- **What:** The zip fallback is a documented dropped feature (`docs/rust-port.md:189`) — but the
  API keeps the knob and swallows it. `fs_util::delete_uri` (`fs_util.rs:421`,
  `#[allow(dead_code)]`) is the orphaned other half.
- **Failure scenario:** a CI pipeline migrated from golongtail passes its zip-fallback list and
  believes old versions whose `.lvi` is gone are still cloneable; the day a source `.lvi` is
  missing, `clone_store` hard-errors at `clonestore.rs:143` with no hint the configured fallback
  was never in play.
- **Recommendation:** dropped features should reject, not ignore: return
  `LongtailError::InvalidArgument("source_zip_paths: zip fallback is not supported in the
  pure-Rust port")` when non-empty (mirroring the `use_legacy_write` → typed-error precedent,
  `error.rs:61-62`), and make the CLI flag error the same way. Remove the field at the next break
  window; fix or delete the `delete_uri` doc (API-DOC-04).
- **Tradeoff / risk:** a pipeline passing the flag today starts failing loudly — that is the point;
  gate the CLI message with the golongtail-parity wording R7 tracks.
- **Effort:** S
- **Regression test to add:** CLI test: `clone-store --source-zip-paths f` exits non-zero with the
  unsupported message.

### `API-06` — put→get round-trip drops the S3 endpoint from the get-config
**P2** · `hardening` · CONFIRMED
- **Where:** writer: `crates/longtail/src/put.rs:168-174` (inserts `s3-endpoint-resolver-uri`);
  reader: `crates/longtail/src/get.rs:42-83` reads only `storage-uri`, `source-path`,
  `version-local-store-index-path`.
- **What:** `put` deliberately persists the endpoint into the get-config JSON (mirroring
  cmd_put.go:115-146 per its comment); `get` — in the same workspace — never consumes the key, and
  its module doc declares "unknown keys ignored" (`get.rs:2-3`).
- **Failure scenario:** CI runs `put` against minio/R2 with `--s3-endpoint-resolver-uri`; the
  launcher later runs `get` on the produced config with no explicit endpoint flag. The storage URI
  resolves against the AWS default endpoint: `NotFound`/auth failure — or, worst case, a same-named
  bucket on real AWS.
- **Evidence:** both files read in full; the asymmetry needs no golongtail oracle.
- **Recommendation:** parse the key in `get` into `s3_options.endpoint_url` (explicit caller
  options should win over the config value; document the precedence). If golongtail's `get`
  ignores the key too, say so in a comment — but our `put` writing it makes our `get` reading it
  self-consistency, not divergence.
- **Tradeoff / risk:** behavior change for configs that carry a stale endpoint; precedence rule
  covers it.
- **Effort:** S
- **Regression test to add:** unit test in `get.rs`: config with `s3-endpoint-resolver-uri` →
  assert the composed `DownsyncOptions.s3_options.endpoint_url`.

### `API-07` — CLI wiring duplication already cost a knob its coverage
**P2** · `complexity` · CONFIRMED
- **Where:** `crates/longtail-cli/src/main.rs` — endpoint wiring ×13 (`619-622, 656-659, 691-694,
  730-733, 772-775, 793-796, 809-812, 827-830, 850-853, 870-873, 908-916, 983-986, 1032-1035`);
  stalled-stream wiring ×4 only (`623-626, 660-663, 695-698, 734-737`); progress wiring ×5; cancel
  wiring ×3; `run_get` (`639-674`) is `run_downsync` (`598-637`) with two lines changed.
- **What:** Per-subcommand option plumbing is copy-pasted. Current control-flow shape: 17
  `run_*` fns, each a linear field-copy block + 2-4 cfg-gated `if` blocks.
- **Failure scenario (already happened):** HEAD's `--no-stalled-stream-protection` reached only
  `downsync`/`get`/`validate-version`/`upsync` (`main.rs:94,148,207,230`); `put` and `clone-store`
  — the two heaviest S3 transfer commands — cannot opt out of the very SDK behavior the flag
  exists to disable. Every future S3 knob repeats this 12-site lottery.
- **Recommendation:** one `#[derive(Args)] struct S3Args { s3_endpoint_resolver_uri, no_stalled_stream_protection }`
  flattened (`#[command(flatten)]`) into each S3-taking args struct, plus one
  `fn apply(&self, &mut S3Options)` — collapses 17 blocks into one function. Same pattern for the
  progress+cancel pair. Whether `put`/`clone-store` *should* get the flag is R7's call (their
  OPS-06/18 own flag parity); the refactor makes the answer one line either way.
- **Tradeoff / risk:** pure CLI refactor; no library or byte-level behavior change.
- **Effort:** M
- **Regression test to add:** a clap-level test iterating S3-taking subcommands and asserting each
  accepts the shared S3 flags.

### `API-08` — The upsync/clone-store duplication is where the bugs live
**P2** · `complexity` · CONFIRMED
- **Where:** `crates/longtail/src/upsync.rs:116-188` vs `crates/longtail/src/clonestore.rs:151-198`.
- **What:** hasher → open ReadWrite store → `get_existing_content` → `create_missing_content` →
  conditional `write_content` → flush/close → write `.lvi` → optional `.lsi` merge+write: ~30
  lines duplicated around the (properly shared) `write_content`. The copies diverge in exactly the
  spots that produced filed findings: clonestore derives the `.lsi` path via
  `target_lvi.replace(".lvi", ".lsi")` (`clonestore.rs:195` — **OPS-14**, replaces every
  occurrence anywhere in the URI) where upsync takes an explicit URI (`upsync.rs:169-176`);
  clonestore feeds a locally-created, never-cancelled token (`clonestore.rs:107` — **STORE-15**)
  where upsync honors `opts.cancel`; clonestore discards `store.stats()` and phase timings that
  upsync reports.
- **Failure scenario:** the divergences *are* the scenario — duplication hid OPS-14 and the
  uncancellable half from review of the "same" pipeline.
- **Recommendation:** extract one `async fn upload_version(store, &VersionIndex, params) ->
  UploadStats` consumed by both; clonestore's remaining deltas (dual S3 configs, per-version loop)
  stay local. Fold the OPS-14 fix into the extraction (explicit `.lsi` URI derivation helper with a
  suffix-only replace). This is orchestration code, not the byte-producing algebra — `write_content`
  and the codecs are untouched, so the upsync byte gate still proves output identity.
- **Tradeoff / risk:** `COMPAT-RISK`-adjacent only via ordering of writes; gate:
  `crates/longtail/tests/upsync_byte_gate.rs` (per-PR) + `sync_fixtures.rs`.
- **Effort:** M
- **Regression test to add:** clone-store `.lsi`-derivation unit test (`a.lvi.lvi`, no-`.lvi` cases
  — doubles as the OPS-14 regression).

### `API-09` — The cfg-gated S3 binding is pasted 11× (+3 `default_s3` clones)
**P3** · `complexity` · CONFIRMED
- **Where:** `downsync.rs:92-95`, `upsync.rs:51-54`, `clonestore.rs:98-105` (×2), `put.rs:190-193`,
  `get.rs:24-27`, `cp.rs:90-93`, `prune.rs:158-161,289-292,397-400`, `inspect.rs:218-221`;
  `default_s3()` triplicated verbatim at `cp.rs:16-22`, `prune.rs:21-27`, `inspect.rs:16-22`.
- **What:** every op opens with the same 4-line `#[cfg]` pair; `inspect.rs:219` already drifts
  (moves instead of clones).
- **Failure scenario:** cost, not breakage: every new op re-pastes; a future edit to one arm (e.g.
  defaulting a field) misses ten siblings silently, cfg-split so the miss only surfaces on the
  non-default feature build.
- **Recommendation:** in `fs_util` (where the alias lives): `pub(crate) fn s3_arg(#[cfg(feature =
  "s3")] o: &S3Options) -> S3OptionsArg` — or simpler, a single `pub(crate) fn default_s3()` plus
  one macro-free helper taking the options struct's field. One definition, 14 call sites.
- **Tradeoff / risk:** none.
- **Effort:** S
- **Regression test to add:** none — `11-featurematrix.txt`'s `--no-default-features` lane already
  compiles both arms.

### `API-10` — Store-opening boilerplate: 10 literals, 5 pool idioms, 2 error strings
**P3** · `complexity` · CONFIRMED
- **Where:** `BlockStoreOpts` literals: `downsync.rs:146-163`, `upsync.rs:119-127`,
  `clonestore.rs:151-163,226-238`, `cp.rs:95-108`, `prune.rs:174-186,218-230`,
  `inspect.rs:62-70,148-156,206-214,270-278`. Pool construction: `prune.rs:29-36` (`fn pool`),
  `inspect.rs:78-85` (`fn single_thread_pool`), `inspect.rs:56-61` (**inline in
  `validate_version`, ignoring the helper defined 20 lines below**), `cp.rs:99-104` (inline),
  `clonestore.rs:232` (`build_pool(1)`). Error strings: `"rayon pool: {e}"` ×4 vs
  `"failed to build rayon pool: {e}"` (`version.rs:173`).
- **What:** the same read-only single-thread store setup exists in five spellings.
- **Failure scenario:** cost: `prune_store`'s two literals (`prune.rs:176-185` vs `219-229`) differ
  only in `access_type` — a future field added to one and not the other compiles fine and behaves
  differently between the gather and the destructive phase of the *same command*.
- **Recommendation:** delete `prune::pool` and `inspect::single_thread_pool` in favor of
  `version::build_pool(1)`; add a facade-internal
  `open_store(uri, access, remote_workers, cache, pool, s3) -> Arc<dyn BlockStore>` used by the
  8 simple sites (downsync's budget-carrying variant stays bespoke).
- **Tradeoff / risk:** none; pure plumbing.
- **Effort:** S-M
- **Regression test to add:** none needed beyond existing op tests.

### `API-11` — The advertised error-class contract has no API and no test
**P1** · `hardening` · CONFIRMED — justification: the Tauri retry/re-auth UX is specified against a
guarantee that is currently false on the main download path.
- **Where:** the promise: `crates/longtail/src/lib.rs:82-85` ("a facade-only consumer can match on
  … `NotAuthorized` / `Network` / `NotFound`"). The break: **STORE-04**
  (`remote.rs:490,626-632`) — cross-referenced, not re-filed. The facade-side gaps:
  `apply.rs:353-363` maps a task panic to `LongtailError::Io("apply block task")`;
  no test anywhere asserts an S3 auth rejection surfaces as
  `LongtailError::Store(StoreError::NotAuthorized)` through a full `downsync`.
- **What:** classification exists only as prose. A consumer must hand-write a match over 14 + 16
  variants, guess retryability, and (today) still gets `Backend("… AccessDenied …")` on the one
  path that matters.
- **Failure scenario:** launcher shows "network problem — retrying" for expired credentials
  (STORE-04's flattening) and "disk error" for a code-bug panic (`Io` mapping); user follows the
  wrong remedy in both cases.
- **Recommendation:** the contract in §Diagnostics contract below: add
  `#[non_exhaustive] pub enum ErrorClass { Cancelled, NotFound, Unauthorized, Transient,
  InvalidInput, Corrupt, Internal }` + `LongtailError::class(&self)` implementing that table, in
  the same change that fixes STORE-04 (which is what makes the mapping truthful). Route panics to
  `Internal`, not `Io`.
- **Tradeoff / risk:** the class enum becomes API — hence `#[non_exhaustive]` from day one.
- **Effort:** M (facade side; STORE-04 fix is R3's).
- **Regression test to add:** the round-trip test in Experiments #3 (minio, revoked key →
  `class() == Unauthorized`); a unit test that a panicking mock-store task yields `Internal`.

### `API-12` — A GUI-embedded library that cannot say what it is doing
**P2** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/Cargo.toml:23` declares `tracing`; `grep` finds **zero** uses in
  `crates/longtail/src` (`07b-machete.txt` flags it unused). The only library events in the
  workspace are 4 in `longtail-store/src/cache.rs:168-253`. Silent sites that most need a signal:
  `downsync.rs:297-311` (`load_store_index_override` — `.ok()?` swallows read/merge failures and
  falls back to a full store scan, an orders-of-magnitude slowdown with zero indication);
  `sync.rs:456` (`Err(_) => return Ok(rebuilt)` — the comment admits "Go logs", Rust doesn't);
  the read-retry ladder (`sync.rs:79-92`) retries through ≈3.85 s of sleeps invisibly (STORE-17's
  latency has no witness).
- **Failure scenario:** launcher support ticket: "download stuck at 0%". It is in a silent retry
  ladder or a silent full store rescan; neither the user, the log, nor the developer can tell
  which.
- **Recommendation:** either emit or drop the dep — emit: one `info_span!` per op (uri, worker
  counts), `warn!` at the three sites above + prune's swallowed deletes (STORE-02's remedy names
  this too), `debug!` per retry rung with attempt count. That is ~10 events, not an observability
  project. Then delete the machete suppression need and correct API-DOC-02's doc claims.
- **Tradeoff / risk:** events in hot paths — all named sites are cold (fallback/retry/error).
- **Effort:** S-M
- **Regression test to add:** a `tracing-subscriber` capture test asserting the override-fallback
  path emits a `warn` naming the failed `.lsi` URI.

### `API-13` — The async ops block their polling thread for whole phases
**P2** · `hardening` · CONFIRMED
- **Where:** `downsync.rs:115-126` (scan) and `:202-214` (validate) call
  `create_version_index_from_folder`, which runs `pool.install(par_iter…)` **inline**
  (`version.rs:53-74`); `upsync.rs:92-111` likewise. `pool.install` parks the calling thread until
  the parallel scan completes.
- **What:** `downsync().await` holds one tokio worker hostage for the full "Indexing version"
  phase — minutes on a 100 GB install rescan. The runtime-model docs (`lib.rs:6-15`,
  `docs/rust-port.md:40-41`, "plain `async fn` … on the caller's ambient runtime") never disclose
  it. "CPU work never goes through `spawn_blocking`" (rust-port.md:41) is about the *work* (rayon)
  — the *coordinator* thread is the problem.
- **Failure scenario:** an embedder on a current-thread runtime (or `tauri::async_runtime` with the
  op awaited among UI-serving tasks) freezes every other future on that worker for the phase
  duration; progress keeps flowing (rayon-thread callbacks) which makes the stall look like an app
  bug, not a documented property.
- **Recommendation:** wrap the two `create_version_index_from_folder` call sites (and upsync's) in
  `tokio::task::spawn_blocking` — the CPU work still runs on rayon, so the documented design
  holds; only the parked coordinator moves off the runtime. At minimum, document the requirement
  ("multi-thread runtime; the future occupies its thread during Indexing/Validating").
- **Tradeoff / risk:** `spawn_blocking` closure needs owned/`'static` captures — mechanical clones
  of the small inputs; no behavior change (the function is already sync and deterministic).
- **Effort:** S-M
- **Regression test to add:** a `#[tokio::test(flavor = "current_thread")]` that runs a small
  `downsync` concurrently with a heartbeat task and asserts the heartbeat keeps ticking during the
  scan phase.

### `API-14` — Progress-callback threading is a contract nobody wrote down
**P2** · `hardening` · CONFIRMED
- **Where:** scan: `version.rs:66-70` — `on_scan` fires on rayon workers inside `pool.install`;
  apply: `apply.rs:234-243` — `progress.report` fires on tokio workers **while holding
  `report_lock`** (`std::sync::Mutex`); phase changes fire on the op's own task. The only guidance
  is "must be cheap and non-blocking" (`progress.rs:29-30`).
- **What:** the embedder's `on_progress` runs on three different thread kinds. On a rayon thread
  there is no tokio reactor: a sink that calls `tokio::spawn`/`Handle::current()` panics, and that
  panic propagates through `pool.install` and unwinds the whole op. In apply, a sink that blocks
  (full bounded channel, synchronous IPC to the webview) serializes *all* block-completion
  reporting behind the mutex and parks a runtime worker.
- **Failure scenario:** a Tauri sink forwards progress via `tokio::sync::mpsc::Sender::blocking_send`
  → deadlock candidate on a starved runtime; or uses `tokio::spawn` → panic mid-scan surfacing as
  a mystery `JoinError`/unwind, not a progress bug.
- **Recommendation:** document the provenance per phase on `ProgressSink` ("may be called from
  rayon worker threads and tokio worker threads; must not block, must not assume a tokio context";
  recommend `try_send` + coalescing in sinks). Consider `catch_unwind` around the sink call in the
  two forwarders so a sink bug degrades to lost updates rather than an aborted op — that is a
  5-line hardening, not a re-architecture.
- **Tradeoff / risk:** `catch_unwind` hides sink bugs; pair it with a (API-12) `warn!`.
- **Effort:** S
- **Regression test to add:** a test sink that panics once — op completes with a warning, target
  identical.

### `API-15` — No rayon `panic_handler` anywhere: a codec panic kills the GUI
**P2** · `hardening` · CONFIRMED
- **Where:** `version.rs:164-174` (`build_pool` — no `.panic_handler`), same for
  `inspect.rs:78-85`, `prune.rs:29-36`, `cp.rs:99-104`, and any caller-supplied pool
  (`options.rs:68` documents no requirement). Detached use: `store/compress.rs:38-48` —
  `pool.spawn` + oneshot (`on_pool`).
- **What:** rayon's contract: a panic in a *detached* `spawn` goes to the pool's panic handler, and
  **aborts the process** when none is set. `par_iter`/`install` panics propagate to the caller
  (contained-ish); the codec bridge is the detached kind. This is the facade-side enabler of
  **STORE-12** (cross-referenced, not re-filed): R3 located the missing `catch_unwind` at the
  codec; the pools the facade builds are where the process-abort default gets switched off.
- **Failure scenario:** one malformed compressed block panics the decoder on a rayon worker → the
  entire launcher process aborts mid-download, no dialog, no flush, no eviction.
- **Recommendation:** `build_pool` (and the 1-thread builders, once API-10 unifies them) sets
  `.panic_handler(|p| tracing::error!(…))`; `on_pool` then surfaces the dropped oneshot as
  `CompressError`/`StoreError` instead of `.expect` (`compress.rs:47`) — that half is STORE-12's
  fix. Document that caller-supplied pools must set a panic handler.
- **Tradeoff / risk:** a swallowed panic must still fail the op — the oneshot-drop path already
  does that once `expect` is replaced.
- **Effort:** S (facade), M (with STORE-12's codec side).
- **Regression test to add:** mock codec that panics → `downsync` returns `Err`, process alive.

### `API-16` — The embedder has no safe equivalent of the second Ctrl-C
**P2** · `hardening` · CONFIRMED
- **Where:** the contract comment `lib.rs:71-81`; graceful path `apply.rs:186-260`; the CLI's
  force-stop `main.rs:538` (`std::process::exit(130)` — R3's finding, cross-referenced).
- **What:** token cancellation is block-granular and "cannot abort an already-in-flight fetch"
  (`lib.rs:79-80`). Worst-case latency to return `Cancelled` is one in-flight block get through the
  full 6-rung retry ladder (≈3.85 s of sleeps, STORE-17) plus SDK timeouts — tens of seconds with a
  bad network. The CLI's escape hatch is process exit; an embedder's only analog is
  `JoinHandle::abort()` / dropping the future, whose costs are undocumented: the apply `JoinSet`
  drops → in-flight tasks cancel at their next await; `store.flush()/close()` never run → cache
  write-backs may be lost and **LRU eviction never runs** (STORE-10 — the byte budget is enforced
  only in `close()`), so the on-disk cache exceeds its limit until the next *successful* run;
  STORE-09's permit/map-entry leak becomes reachable (bounded, since stores are per-op); an aborted
  upsync can leave uploaded-but-unindexed blocks (wild blocks — the FMT-003 prune interaction).
  The *target tree* stays safe either way: files are pre-truncated and resumability rests on the
  next run's rescan.
- **Failure scenario:** launcher implements "Cancel" as task-abort (the natural tokio idiom
  because token-cancel latency is unacceptable in UI); a user who cancels daily accumulates an
  unbounded block cache and occasionally loses warm-cache write-backs — nothing documents that this
  was the trade.
- **Recommendation:** document the two-lane contract on `downsync`/`get` (token = graceful,
  bounded-loss, latency up to one block-fetch + ladder; abort = immediate, skips flush/eviction,
  target still resumable). Cheap latency improvement: check the token before each retry-ladder
  sleep (that touchpoint is STORE-14/STORE-17 territory — cross-referenced). Consider running
  eviction at *open* as well as close so abort-heavy usage self-heals (a one-call change in
  `CacheBlockStore` composition — flag to R3).
- **Tradeoff / risk:** eviction-at-open adds startup I/O proportional to cache size; gate on
  "budget exceeded".
- **Effort:** S (docs) + S (ladder token check, R3's file).
- **Regression test to add:** abort a `downsync` mid-apply (drop the future at a gated block),
  re-run to completion, assert the target validates — pins the "abort is resumable" half of the
  contract.

### `API-17` — Phase names are an unversioned string protocol
**P3** · `idiom` · CONFIRMED
- **Where:** `on_phase` strings: `"Reading version index"`, `"Indexing version"`,
  `"Reading store index"` (`downsync.rs:104,112,137`), `"Updating version"` (`apply.rs:177`),
  `"Validating version"` (`downsync.rs:203`), `"Indexing version"`/`"Writing content"`
  (`upsync.rs:91,149`), `"Cloning version {i}/{n}"` (`clonestore.rs:148`). `PhaseTiming.phase`
  strings: `read_source_index`, `build_target_index`, `open_store`, `diff_and_retarget`, `apply`,
  `flush`, `validate` (`downsync.rs:109-214`) — a second, disjoint vocabulary for the same
  phases.
- **What:** a GUI that segments its bar per phase must match undocumented literals; a rename (or
  the clonestore interpolated variant) breaks it silently. Nothing tests the strings.
- **Failure scenario:** a future PR rewords `"Updating version"` → the launcher's download-bar
  branch stops matching; CI green, UI regressed.
- **Recommendation:** export the phase names as consts (or a `#[non_exhaustive]` enum with a
  `Display`) used by both `on_phase` and `PhaseTiming`; document the sequence per op. Keep the
  human wording as the enum's `Display` so the CLI output is unchanged.
- **Tradeoff / risk:** enum + interpolated clone-store phase needs a payload variant or keeps a
  string escape hatch.
- **Effort:** S
- **Regression test to add:** assert the emitted phase sequence for a small downsync equals the
  documented list.

### `API-18` — Sixteen user-facing errors Debug-format their paths
**P3** · `idiom` · CONFIRMED
- **Where:** `apply.rs:81`, `downsync.rs:272`, `fs_util.rs:58,63,79,105,111,119,139,142,154,199,
  205,209,266,458,460,466` — all `format!("… {path:?}")` into `LongtailError::Io.context`.
- **What:** `Path`'s `Debug` quotes and escapes: on Windows — the production platform — every I/O
  error a user reports reads `open "C:\\Users\\name\\game\\data.bin"`.
- **Failure scenario:** cosmetic but permanent: support screenshots and launcher error dialogs
  carry doubled backslashes; non-UTF-8 paths render as escape soup.
- **Recommendation:** `path.display()` at all 16+2 sites (mechanical; clippy's
  `unnecessary_debug_formatting` pedantic lint enumerates them).
- **Tradeoff / risk:** none (`display()` is lossy for non-UTF-8 — irrelevant for messages).
- **Effort:** S
- **Regression test to add:** none (would be churn); one-time sweep + the pedantic lint in the
  workspace-lints set (API-DOC-05) keeps it fixed.

### `API-19` — Scan-start `Instant` arithmetic can panic at boot
**P3** · `hardening` · CONFIRMED
- **Where:** `version.rs:141` — `Mutex::new(Instant::now() - Duration::from_secs(1))` in
  `scan_progress_forwarder`, constructed at the start of every downsync/upsync scan.
- **What:** `Instant - Duration` panics on underflow; on Linux and Windows `Instant` is
  time-since-boot.
- **Failure scenario:** an auto-start launcher (login item / CI VM boot script) invoking a
  downsync within the first second after boot panics before any work, in library code.
- **Recommendation:** `Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(Instant::now)`
  — under the `Ok(mut t) if t.elapsed() >= 100ms` throttle (`version.rs:145`) the fallback still
  forwards the first sample within 100 ms, preserving intent.
- **Tradeoff / risk:** none.
- **Effort:** S
- **Regression test to add:** not directly testable; the `checked_sub` makes the panic
  structurally impossible.

### `API-20` — Pin the FP shape of the chunker discriminator against `clippy --fix`
**P3** · `hardening` · CONFIRMED · `COMPAT-RISK`
- **Where:** `crates/longtail-core/src/chunker.rs:242-245` (`discriminator_from_avg`).
- **What:** the doc comment (`:236-241`) states the operator shape and constants are reproduced
  bit-for-bit from `HPCDCDiscriminatorFromAvg`, but no `#[allow(clippy::suboptimal_flops)]` pins
  it. Pedantic clippy (`02-clippy-pedantic.txt`) suggests `mul_add`, which fuses to a single
  rounding — a 1-ulp discriminator change moves HPCDC boundaries and silently diverges every chunk
  hash from C-produced indexes.
- **Failure scenario:** a well-meaning `cargo clippy --fix` (the pedantic output prints the
  invitation per target) applies the fusion; the divergence *would* be caught — gates:
  `support/longtail-testkit/tests/chunker_golden.rs` (per-PR) and `fixtures/` — but pinning at the
  source makes the intent survive refactors and keeps the failure from ever reaching CI.
- **Recommendation:** targeted `#[allow(clippy::suboptimal_flops)]` with a one-line "bit-for-bit
  C reproduction — do not fuse" justification.
- **Tradeoff / risk:** none.
- **Effort:** S
- **Regression test to add:** exists (chunker goldens); the allow is belt to their suspenders.

## Deliverable 1 — public-surface disposition

No git tags, nothing published (`00-scope.txt`, `18-semver.txt`): **this table is the proposed
v0.1.0 commitment.** "seal" = add `#[non_exhaustive]`. Semver notes assume the API-01/02/03
recommendations land before the first tag.

### `crates/longtail-core`

All 13 modules are `pub mod` with selective root re-exports; no unreachable pub items
(worker-verified; spot-checked by lead).

| Item (kind) | Where | Disposition | Semver / notes |
|---|---|---|---|
| `BlockIndex`, `StoredBlock` (structs, pub fields) | `block.rs:17,120` | keep | codec structs: fields frozen by the C format; leave exhaustive |
| `VersionIndex` (14 pub fields) | `version_index.rs:36` | keep | as above; literal-constructed cross-crate (`downsync.rs:277`) — add a `VersionIndex::empty(hash_id, tcs)` ctor and migrate that literal (S) |
| `StoreIndex` (7 pub fields) | `store_index.rs:31` | keep | as above; fix the `merge` doc misattachment (API-DOC-01) |
| `FileEntry`, `FileInfos` | `file_infos.rs:15,29` | keep | scan-input types; R1's FMT-009 covers accessor arms |
| `Permissions` (newtype + 10 assoc consts) | `perms.rs:12` | keep; fix or remove `contains` per **FMT-006**; document the 9 bit consts | `POSIX_MASK` unused → FMT-016 |
| `VersionDiff`, `ChunkSpan`, `ChunkHash` | `diff.rs:19`, `chunker.rs:145,219` | keep | plain data |
| `HpcdcChunker` | `chunker.rs:228` | keep | private fields + ctors — already well-shaped |
| `FastCdcChunker` | `chunker.rs:171` | keep, feature-gated | fix the default-features doc link (API-DOC-05); bench-only, consider `#[doc(cfg(feature = "fastcdc"))]` when stable |
| `SeedMode` (enum) | `chunker.rs:132` | seal | chunker input mode may grow |
| `Chunker` (trait) | `chunker.rs:154` | keep open, document "external impls unsupported for store compat" | or seal — Open question #2 |
| `Compressor` (trait) | `compress.rs:128` | **seal** (private supertrait) | concrete codecs are private and the registry (`compressor_for`) is closed — an external impl is unregistrable dead weight; sealing later is major |
| `Hash` (trait) | `hash.rs:55` | keep open | `create_version_index_from_folder` is deliberately generic over it; document compat caveat |
| `Blake3`, `Blake2s` (ZSTs) | `hash.rs:65,81` | keep | |
| `FormatError` (10), `ChunkerError` (3), `CompressError` (6), `HashError` (2), `MergeVersionError` (1), `ValidateError` (2) | per worker table | **seal all six** (API-01) | adding a variant is otherwise major; R1's FMT findings will add variants |
| fns: `chunk_asset`, `assemble_version_index`, `create_version_index`, `merge_version_index`, `create_version_diff`, `get_required_chunk_hashes`, `compressor_for`, `block_hash`, `create_store_index`, `create_missing_content`, `validate_store` | `build.rs`/`diff.rs`/`compress.rs`/`pack.rs`/`validate.rs` | keep | ALG-14 (panic on mismatched slices) is the one signature-adjacent wart — R2's |
| module-scoped fns: `encode/decode_block_payload`, `decode_stored_block`, `blake3_hash`, `blake2s_hash`, `hasher`, `discriminator_from_avg` | `compress.rs`, `hash.rs`, `chunker.rs` | keep module-scoped | deliberately not flattened to root; fine |
| consts: hash IDs, codec families, `WINDOW_SIZE`, `MAX_AVG`, two `VERSION`s + root aliases | various | keep | format constants — frozen |

### `crates/longtail`

Root re-exports plus 5 `pub mod`s (`compression`, `error`, `options`, `path_filter`, `progress`).

| Item (kind) | Where | Disposition | Semver / notes |
|---|---|---|---|
| `downsync`, `get`, `upsync`, `put`, `cp`, `clone_store`, `prune_store{,_index,_blocks}`, `validate_version`, `init_remote_store`, `create_version_store_index`, `print_version_usage_stats`, `store_index_stats` (fns) | op modules | keep | signatures take options structs — stable once structs are sealed |
| `read_version_index_from_uri`, `read_store_index_from_uri` | `inspect.rs:25,88` | **break now** per API-02 (add options param) | the only pre-tag breaking change proposed |
| `downsync_blocking`, `get_blocking`, `upsync_blocking`, `put_blocking` | `lib.rs:103-145` | keep; add `Handle::try_current()` guard (observation O-8) | |
| `create_version_index_from_folder` | `version.rs:25` | keep | generic `H: Hash` + `&rayon::ThreadPool` + `&CancellationToken` params couple to rayon/tokio-util majors — accepted, note in docs |
| `make_hasher`, `SyncHasher` | `hash_util.rs:11,15` | keep | |
| `hash_identifier_for_name` | `hash_util.rs:26` | demote `pub(crate)` or export (API-04) | currently unreachable pub |
| `compression_type_for_name`, `compression::NO_COMPRESSION` | `compression.rs:9,13` | keep | the alias-quirk table is compat-load-bearing; tested |
| `DownsyncOptions`, `GetOptions`, `UpsyncOptions` | `options.rs:19,178,211` | **seal**; keep `new()` pattern | cfg-gated `s3_options` field hazard dies with sealing (API-01) |
| `PutOptions`, `CloneStoreOptions`, `CpOptions`, `PruneStore{,Index,Blocks}Options`, `ValidateVersionOptions`, `InitRemoteStoreOptions`, `CreateVersionStoreIndexOptions`, `PrintVersionUsageOptions` | per-module | **seal**; document the undocumented `new()`s | `CloneStoreOptions::source_zip_paths` → **remove/reject** (API-05) |
| `DownsyncReport`, `UpsyncReport`, `DownsyncStoreStats`, `PhaseTiming`, `PruneStore*Result`, `StoreIndexStats`, `VersionUsageStats` | `options.rs`, `prune.rs`, `inspect.rs` | **seal** | serde derives make the JSON shape a second compat surface for the launcher — document it; `UpsyncReport.blocks_written == blocks_missing` redundancy (O-7): keep both for golongtail parity, document |
| `upsync::DEFAULT_*` (4 consts) | `upsync.rs:31-36` | **export** (API-04) + document the 2 undocumented | |
| `LongtailError` (14 variants) | `error.rs:20` | **seal**; add `class()` (API-11); route panics to `Internal` | |
| `ProgressSink` (trait), `Progress`, `NullProgress` | `progress.rs` | keep open (users implement it); document threading (API-14) | |
| `RegexPathFilter` | `path_filter.rs:15` | keep | OPS-19 owns its coverage gap |
| re-export `CancellationToken` | `lib.rs:81` | keep | couples to tokio-util 0.7 majors — documented rationale is sound |
| re-export `StoreError` | `lib.rs:85` | keep; sealing is R3's file | |
| re-export `S3Options` (cfg s3) | `lib.rs:92` | keep; **seal** (store crate) | its pub fields are `aws_sdk_s3::Client` / `SdkConfig` / `SharedCredentialsProvider` (`s3.rs:81-86`): every AWS SDK major becomes a `longtail` major. Unavoidable while callers inject providers; document it as the crate's loudest semver coupling |
| `DownsyncOptions::max_prefetch_bytes` | `options.rs:74` | keep `#[doc(hidden)]` | test knob, correctly hidden |
| all of `fs_util`'s pub items | `fs_util.rs` | demote to `pub(crate)` (private mod already hides them); delete dead `delete_uri` or implement its caller | API-DOC-04 |

**Naming inconsistencies to resolve at the same break window** (worker-c verified, lead-sampled):
`target_path` means target *folder* (Downsync/Get/CloneStore), target **`.lvi` URI** (Upsync), and
destination *file* (Cp); `source_path` means folder (Upsync/Put), in-version asset (Cp), and
version-index URI (`CreateVersionStoreIndexOptions` — its doc already has to shout "NOT a source
folder", `inspect.rs:173`); `validate`/`validate_versions` vs clone-store's inverted
`skip_validate` (whose default *enables* what the others' default disables). Renames are cheap only
now; if golongtail flag parity forbids renaming, document each divergence on the field.

## Deliverable 3 — the diagnostics contract

Classification as it stands (against the four enums: `longtail-core/src/error.rs`,
`longtail-store/src/error.rs`, `longtail/src/error.rs`, CLI mapping `main.rs:502-518`):

| Class | Today | Verdict |
|---|---|---|
| Cancelled | `LongtailError::Cancelled`; CLI exit 130 with resume hint (`main.rs:508-511`) | ✅ sound |
| Not found | `StoreError::NotFound` for s3/fsblob URIs — but a missing **local** `.lvi` surfaces as `Io{kind=NotFound}` (`downsync.rs:272`): the class depends on the URI scheme | ⚠️ unify in `class()` |
| Unauthorized | `StoreError::NotAuthorized` well-defined (`s3.rs:30-47` code list) for index/list ops — **flattened to `Backend` on every block get** (STORE-04) | ❌ until STORE-04 lands |
| Transient/retryable | `StoreError::Network` (dispatch/timeout/response, `s3.rs:70-72`); same flattening on block gets; internal retry ladder already consumes most transients | ⚠️ |
| Invalid input | `InvalidArgument`, `InvalidGetConfig`, `UnsupportedUri{uri,reason}`, `InvalidUri` | ✅ |
| Corrupt data | `FormatError`/`CompressError`/`BadFormat` chains preserved via `#[from]` | ✅ |
| Internal bug | task panic → `Io("apply block task")` (`apply.rs:358`) | ❌ miscategorized |

Message quality: contexts consistently name the object (`chunk {hash} required by {rel}`
`apply.rs:115`; `NotFound(key)`; op-labeled SDK chains via `DisplayErrorContext`) — good. Two
warts: Debug-formatted paths (API-18) and the category-only top-level `Display` — which is
*deliberate* and well-documented (`error.rs:14-18`), with `full_chain()` provided and used by the
CLI. Leak check: **negative** — `S3Options` derives only `Clone` (no `Debug`, `s3.rs:80`), so
credentials cannot be Debug-printed; `map_sdk_err` details carry op + SDK error chain (bucket/key/
endpoint may appear — remedy-relevant, not secret; verified no credential material in the mapped
variants).

**The contract to adopt** (API-11): `LongtailError::class() -> ErrorClass` per the table's target
column, truthful only after STORE-04; `Cancelled` guaranteed to mean "target resumable"; every
`Io` carries op + path (display-formatted); panics become `Internal`. The launcher then branches on
7 classes instead of 30 variants, and the round-trip test (Experiments #3) pins the one class that
has silently regressed once already.

## Deliverable 4 — the Tauri embedding contract

What the embedder gets today, verified line-by-line; gaps are the findings cited.

1. **Runtime**: plain `async fn` on the caller's runtime; never builds one (`lib.rs:6-15`) ✅ — but
   the future **occupies its polling thread** during Indexing/Validating (API-13). Requirement to
   document: multi-thread runtime, or wrap the op in `tokio::spawn`. `*_blocking` from an async
   context panics deep in tokio (O-8 proposes a guard).
2. **Progress**: rate-limited, monotone (`apply.rs:234-243` lock), dual-dimension. Fires from rayon
   *and* tokio threads, no reactor guaranteed, mutex held during apply emission (API-14). Phase
   names are unstable strings (API-17).
3. **Panics**: apply block tasks are contained (`flatten_apply_task`, `apply.rs:350-363` — never
   aborted mid-write, surfaced as `Err`) ✅; scan panics unwind the op's future (caller sees
   `JoinError` if spawned) ⚠️; detached codec panics **abort the process** (API-15 / STORE-12) ❌.
4. **Cancellation**: token = graceful pause primitive, target + `.lrb` cache stay valid, resume is
   delta-only (`lib.rs:71-81`) ✅ — latency unbounded (in-flight fetch + retry ladder). The CLI's
   force-stop is `process::exit(130)`; the embedder's analog is task abort, which skips
   flush/close/eviction (API-16). Both lanes and their costs must be in the rustdoc, not just this
   review.
5. **Errors**: structured tree + `full_chain()`; class contract per Deliverable 3 (API-11).
6. **Credentials**: provider/`Client` injection with mid-operation refresh (`s3.rs:1-8`) ✅; no
   Debug leak ✅.
7. **Observability**: none (API-12). A GUI cannot show "retrying block 2,113 (attempt 3)" because
   the library never says it.

Recommended embedder guidance to ship in `lib.rs` docs once the above land: spawn the op
(`tokio::spawn`), keep the `JoinHandle` + token; wire Cancel-button → token, window-close →
token + bounded wait + abort; supply a `panic_handler` pool; treat `class()` as the UX switch.

## Lower-priority observations

- **O-1** `fs_util.rs` triplicates the URI-scheme dispatch ladder across
  `read_from_uri`/`write_to_uri`/`delete_uri` (`fs_util.rs:285,358,421`) and duplicates the s3
  parent/basename split (`:339-347` vs `:400-409`) — fold into one `parse_uri` when touched.
- **O-2** Phase timing implemented twice: `PhaseTimer` struct (`downsync.rs:386-406`) vs `lap`
  closure (`upsync.rs:72-80`); put/clonestore/cp/prune report no phases at all.
- **O-3** `change_version2` (219 lines, 10 params, `apply.rs:65-283`): correct as written (R2/R3),
  numbered-comment seams already extraction-shaped; the reap loop `:193-197` and drain loop
  `:248-252` are near-twins. A params struct + two extractions when next touched — nothing more.
- **O-4** `prune.rs:88` clones `existing.block_hashes` where a move suffices — one Vec copy per
  retained version in the CI prune job (clippy `redundant_clone`, lead-verified).
- **O-5** `get.rs:99` computes `opts.target_path.clone().unwrap_or_default()` then overwrites it at
  `:101` — dead value.
- **O-6** Duplicate-chunk block resolution: `cp.rs:123-125` uses first-wins (`or_insert`, matching
  C's `PutUnique` as `apply.rs:390-405` does); `inspect.rs:291` uses last-wins (`insert`) — the two
  copies of the same walk disagree, so `print-version-usage` numbers may differ from golongtail on
  multi-covered chunks. PLAUSIBLE (Go behavior unverified); align on `or_insert`.
- **O-7** `UpsyncReport.blocks_written`/`blocks_missing` are the same value (`upsync.rs:182-183`);
  documented, keep for parity.
- **O-8** `*_blocking` wrappers panic inside tokio contexts; a
  `tokio::runtime::Handle::try_current().is_ok()` → `InvalidArgument` guard makes misuse a typed
  error (`lib.rs:103-145`).
- **O-9** Empty-input handling diverges: downsync/get error on all-empty sources
  (`downsync.rs:42-46`, `get.rs:18-22`), clone-store returns `Ok(0)` (`clonestore.rs:121-126`),
  prune proceeds to an empty keep-set — the destructive end of this spectrum is **OPS-03** (P0,
  cross-referenced).
- **O-10** `try_read_version_index` maps *any* `Io` error to `Ok(None)` (`clonestore.rs:213`) — a
  permission-denied target `.lvi` reads as "not yet cloned" and triggers a full re-clone instead of
  surfacing.
- **O-11** `HashError::UnknownHashId { id: 0 }` for an unknown *name* (`hash_util.rs:31`) encodes a
  fake id; a name-carrying variant would be honest (needs a core enum addition — after sealing).

## Comments & documentation issues

### `API-DOC-01` — `StoreIndex::merge`'s load-bearing doc is attached to a private fn
**P2** · CONFIRMED — `crates/longtail-core/src/store_index.rs:189-219`: the 28-line semantics block
("byte-for-byte on the success path … the S3 shard name is the sha256 of these bytes") runs with no
break into `reserve_capacity`'s doc lines and attaches to the **private** `fn reserve_capacity`
(`:220`); `pub fn merge` (`:229`) renders undocumented. The crate's most compat-critical contract is
invisible in rustdoc, and no lint fires (doc on private item is legal). Fix: split the blocks
(1-line blank). Regression: the missing_docs ratchet (API-DOC-05) then flags any recurrence.

### `API-DOC-02` — Keeper-doc claim "Logging is `tracing`-based" is unearned
**P2** · CONFIRMED — `CLAUDE.md` (§Runtime configuration) and `main.rs:475-477` ("library logs
(e.g. cache-eviction summaries, **retries**) surface") vs reality: 4 events total, none for
retries (`grep` over `longtail-store`/`longtail`; `07b-machete.txt` flags `tracing` unused in
`longtail`). Fix the docs when API-12 lands — or soften them now; a keeper doc promising logs that
don't exist sends a debugging operator to a dead end.

### `API-DOC-03` — Runtime-model docs omit the two facts an embedder needs most
**P3** · CONFIRMED — `lib.rs:6-15` and `docs/rust-port.md:40-41,58-70` describe the pools and the
token but not (a) the polling-thread occupancy during scan/validate (API-13), (b) progress-callback
thread provenance (API-14), (c) abort-vs-token costs (API-16). These belong in the `downsync`
rustdoc — the review is not a substitute.

### `API-DOC-04` — `delete_uri` doc describes a caller that doesn't exist
**P3** · CONFIRMED — `fs_util.rs:417-421`: "Used by clone-store's zip fallback cleanup" on an
`#[allow(dead_code)]` fn; the zip fallback is unimplemented (API-05). Delete the fn or fix the
comment.

### `API-DOC-05` — rustdoc is broken, ungated, and the ratchet is cheap: adopt it
**P2** · CONFIRMED — `08-doc.txt` (exit 101, complete artifact): four real defects — the
`FastCdcChunker` link breaks the default-feature doc build, `merge_consuming` links a private item,
`longtail-testkit`'s `differential` link, and the `longtail` bin/lib doc filename collision
(cargo#6313 — rename the CLI's binary target, e.g. `[[bin]] name = "longtail-cli"`… or keep the
binary name for golongtail parity and doc-exclude the bin). Nothing in CI runs `cargo doc` (R8).
**Assessment (mission item): adopt `[workspace.lints]`** — the workspace has none today (root
`Cargo.toml` has only `[workspace]`/`[workspace.package]`). Proposal: `[workspace.lints.rust]
missing_docs = "warn"` + `[lints] workspace = true` in the two library crates; module coverage is
already 100%, and the full item-level backlog is small and enumerated (API-DOC-06), so escalate to
`deny` in `longtail-core`/`longtail` once cleared. Add `RUSTDOCFLAGS="-D warnings" cargo doc
--no-deps` to the pure lane. Include `clippy::unnecessary_debug_formatting = "warn"` (API-18) and
`clippy::suboptimal_flops` stays *off* workspace-wide (API-20 handles the one site with a targeted
allow).

### `API-DOC-06` — Undocumented pub-item inventory (the ratchet's worklist)
**P3** · CONFIRMED — the 9 `Permissions` bit consts (`perms.rs:16-24`, one shared `//` comment);
2 of 4 `DEFAULT_*` consts (`upsync.rs:32-33`); `GetOptions` fields `options.rs:189-199`; the
`new()` ctors in `inspect.rs:40,134,183,241`, `prune.rs:129,257,354`, `cp.rs:39`,
`clonestore.rs:57`, `put.rs:51`. Roughly 20 items — one sitting.

## Hardening backlog

1. **Seal the surface** (API-01) + **semver-checks CI wiring** (API-03) — the two changes that make
   every later mistake catchable.
2. **`ErrorClass` + minio auth round-trip test** (API-11, with STORE-04) — the launcher's UX
   contract.
3. **Rayon `panic_handler` + panicking-codec test** (API-15, with STORE-12) — process-abort off.
4. **`spawn_blocking` around the scan + current-thread-runtime heartbeat test** (API-13).
5. **Abort-mid-apply resumability test** (API-16) — pins the undocumented half of cancellation.
6. **`missing_docs` ratchet + rustdoc CI gate** (API-DOC-05/06).
7. **Zip-fallback rejection + CLI test** (API-05); **get-config endpoint key test** (API-06).
8. Small pins: `checked_sub` (API-19), `suboptimal_flops` allow (API-20), `.display()` sweep
   (API-18), sink-panic containment test (API-14).

## Verified good

- `S3Options` cannot Debug-leak credentials (no `Debug` derive, `s3.rs:80`); `map_sdk_err`
  preserves the SDK chain without secret material and splits auth/network/backend correctly
  (`s3.rs:30-77`).
- `LongtailError`'s category-Display + `full_chain()` design is coherent, documented, and correctly
  consumed by the CLI (`error.rs:12-18,95-105`, `main.rs:512-517`); Cancelled → exit 130 with a
  resume hint.
- Apply's panic/abort discipline: tasks never aborted mid-write, panics flattened to errors,
  first-error-wins with prompt reaping, monotone progress under lock — all tested in-module
  (`apply.rs:186-263,350-363,805-933`).
- `RateLimited` + `scan_progress_forwarder` throttling is non-blocking on rayon (`try_lock`,
  `version.rs:144`) and always delivers first/terminal samples.
- `downsync` (202 lines) is long but flat, phase-commented, single-options-param — no refactor
  needed; `get`'s config parsing errors name the config path and offending key throughout
  (`get.rs:35-73`).
- `compression_type_for_name` reproduces the golongtail alias quirk and is tested against raw IDs
  (`compression.rs:26-27,36-49`).
- The `CancellationToken` and `StoreError` re-export rationales (`lib.rs:71-92`) are exactly right
  for a facade-only consumer.
- `PutOptions.s3_endpoint_resolver_uri` is *not* dead — it feeds the get-config JSON
  (`put.rs:168-174`); checked before almost mis-filing it (led to API-06 instead).

## Experiments requested

| # | Hypothesis | Exact command | What result would change |
|---|---|---|---|
| 1 | Adding `#[non_exhaustive]` to the 13 options structs + 7 error enums compiles the workspace unchanged (no cross-crate literals besides `downsync.rs:277`) | apply the attribute patch; `cargo check --workspace --all-targets` | compile errors would enumerate literal-construction sites API-01's Effort:S missed |
| 2 | The API-03 CI recipe works offline against a branch-lineage tag | `git tag review-baseline && cargo semver-checks check-release -p longtail-core -p longtail --baseline-rev review-baseline` | a tool failure (feature resolution, workspace layout) would change the wiring spec |
| 3 | An S3 auth rejection surfaces as `LongtailError::Store(StoreError::NotAuthorized)` through a full `downsync` | minio lane: valid store, then revoke the key; run `downsync`; inspect the variant | today it should come back `Backend(_)` (STORE-04); after the fix this becomes API-11's regression test |

## Open questions for the maintainer

1. Distribution intent: crates.io, or git-dep pinned by the launcher? (Decides API-03's shape:
   `version` on path deps vs `publish = false` + tags.)
2. Should `Chunker`/`Hash` be sealed alongside `Compressor`, or is external extension (custom hash
   for private stores) an intended use? Byte-compat argues sealed; the generic scan API argues open.
3. Is the accepted-no-op pattern (`enable_file_mapping`) intended for the *library* API, or only
   for CLI flag parity? API-05 shows the silent-ignore failure mode; a policy would settle
   `source_zip_paths` and future drops.
4. Does the launcher already match on phase strings, and which ones? (Locks API-17's naming before
   the enum ships.)
5. Is the get-config JSON schema (including `s3-endpoint-resolver-uri`) meant to be consumed by the
   launcher, or CLI-only? (Raises/lowers API-06's priority.)

## Files read

`crates/longtail/src/lib.rs` · `crates/longtail/src/error.rs` · `crates/longtail/src/options.rs` ·
`crates/longtail/src/progress.rs` · `crates/longtail/src/inspect.rs` ·
`crates/longtail/src/downsync.rs` · `crates/longtail/src/version.rs` ·
`crates/longtail/src/apply.rs` · `crates/longtail/src/get.rs` · `crates/longtail/src/cp.rs` ·
`crates/longtail/src/put.rs` · `crates/longtail/src/compression.rs` ·
`crates/longtail/src/hash_util.rs` · `crates/longtail/src/clonestore.rs` (26-271) ·
`crates/longtail/src/upsync.rs` (1-190) · `crates/longtail/src/path_filter.rs` (1-80) ·
`crates/longtail/src/fs_util.rs` (cited spans) · `crates/longtail/src/prune.rs` (cited spans) ·
`crates/longtail/Cargo.toml` · `crates/longtail-core/src/error.rs` ·
`crates/longtail-core/src/store_index.rs` (185-231) · `crates/longtail-core/src/chunker.rs`
(230-250) · `crates/longtail-core/src/upsync-adjacent consts via grep` ·
`crates/longtail-store/src/error.rs` · `crates/longtail-store/src/blob/s3.rs` (1-120) ·
`crates/longtail-store/src/cache.rs` (150-200) · `crates/longtail-store/src/sync.rs` (75-100,
440-462) · `crates/longtail-store/src/compress.rs` (30-60) · `crates/longtail-cli/src/main.rs`
(84-260, 460-800, 883-971) · `docs/rust-port.md` (20-75, 189) · evidence pack: `MANIFEST.md`,
`00-scope.txt`, `07b-machete.txt`, `08-doc.txt`, `12-loc.txt`, `18-semver.txt`,
`02-clippy-pedantic.txt` (worker-triaged, kept instances lead-verified) · findings indexes of
`docs/review/{01,02,03,07}-*.md`.
