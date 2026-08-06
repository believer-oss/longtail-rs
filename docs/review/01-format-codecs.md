# 01 · Format-codecs review
- **Reviewed at:** `456274d` · **Lead model:** opus · **Workers:** 3 × fable
- **Slice:** the untrusted-input parser surface — `.lvi`/`.lsi`/`.lsb` codecs, the byte cursor,
  `FileInfos`, `Permissions`, `validate_store`, and conformance to `docs/format-spec.md`
  §§cross-cutting/1–3/7/9 · **Confidence:** covered well (parse paths traced line-by-line; the
  consumer reachability arguments cross the slice boundary and are labelled as such)

## Findings index

| ID | P | Dimension | One line | file:line | Verdict |
|---|---|---|---|---|---|
| FMT-001 | P1 | hardening | `.lvi` asset→chunk map is never validated at parse; `validate_store` then indexes it raw → panic on a parseable file | `version_index.rs:169`, `validate.rs:56` | CONFIRMED |
| FMT-002 | P1 | hardening | `.lsb` payload length is never cross-checked against Σ`chunk_sizes`; the apply path slices past the end → panic | `block.rs:132`, `apply.rs:342` | CONFIRMED |
| FMT-003 | P1 | security | Wild blocks are *silently skipped* by `get_existing_store_index`/`prune`, so `prune-store` deletes live `.lsb` files | `store_index.rs:580`, `:522` | CONFIRMED |
| FMT-004 | P2 | hardening | `to_bytes` on a struct with mismatched array lengths emits a parseable, silently-shifted `.lvi` | `version_index.rs:224` | CONFIRMED |
| FMT-005 | P2 | complexity | Asset paths must be UTF-8 to be materialized — an undocumented divergence; the format stores raw bytes | `version_index.rs:110` | CONFIRMED |
| FMT-006 | P2 | idiom | `Permissions::contains` documents "any bits set", implements "all bits set"; zero call sites | `perms.rs:41-45` | CONFIRMED |
| FMT-007 | P2 | hardening | Zero fuzz targets over the only attacker-controlled input surface in the workspace | (absent) | CONFIRMED |
| FMT-008 | P2 | hardening | No golongtail-produced empty-index fixture: §9's first edge case is verified only against our own writer | `fixtures/` | CONFIRMED |
| FMT-009 | P3 | hardening | `FileInfos`' accessor error arms are dead/untested while `VersionIndex`' twins are tested | `file_infos.rs:104`, `:112`, `:120` | CONFIRMED |
| FMT-010 | P3 | hardening | `len() as u32` count casts in every `to_bytes`; the neighbouring offset uses `try_from` | `store_index.rs:436` vs `:432` | CONFIRMED |
| FMT-011 | P3 | hardening | `path_data.len() as u32` truncates silently past a 4 GiB name blob | `file_infos.rs:67` | CONFIRMED |
| FMT-012 | P3 | hardening | Usage-percent `as u32` can truncate once the u32 size sums wrap | `store_index.rs:597` | PLAUSIBLE |
| FMT-013 | P3 | hardening | The cursor's own truncation guard is unreachable from every caller and has no unit test | `cursor.rs:49-54` | CONFIRMED |
| FMT-014 | P3 | hardening | `FormatError::SizeOverflow` is unreachable on 64-bit; the 56 `checked_*` guards are only load-bearing on a target CI never builds | `cursor.rs:15-23` | CONFIRMED |
| FMT-015 | P3 | complexity | `validate_store` swallows a bad name blob and reports it as a size mismatch | `validate.rs:48` | CONFIRMED |
| FMT-016 | P3 | idiom | `Permissions::POSIX_MASK` has no call site; the one place that must mask re-declares it | `perms.rs:27` | CONFIRMED |

Doc findings (`FMT-DOC-01` … `FMT-DOC-09`) are indexed in their own section.

## Scope

**Read in full:** `crates/longtail-core/src/{cursor,version_index,store_index,block,perms,file_infos,error,validate,lib}.rs`;
`crates/longtail-core/tests/{malformed,roundtrip_proptest,store_algebra}.rs`; `docs/format-spec.md`;
`docs/rust-port.md`; `fixtures/README.md`; `.github/workflows/fixture-freshness.yaml` (header).

**Skimmed / read in part along a declared secondary axis (reachability of a parse-layer gap; filed
under my axis only, cross-referenced to the owner):** `crates/longtail/src/{apply,prune,upsync,cp,inspect}.rs`,
`crates/longtail/src/fs_util.rs` (permission mask + `asset_path` only),
`crates/longtail-store/src/{sync,remote}.rs` and `blob/fs.rs` (§2/§3 naming rules only),
`crates/longtail-core/src/{diff,compress}.rs` (call sites only),
`crates/longtail-core/tests/{merge,file_infos,codec_malformed}.rs`, `crates/longtail-cli/src/main.rs` (prune flags).

**Excluded:** `chunker.rs`, `hash.rs`, `build.rs`, `pack.rs`, `compress.rs` internals (other slices);
`docs/format-spec.md` §§4–6, §8, §10 except where §3 depends on them.

## Verification performed

Evidence-pack artifacts consulted: `MANIFEST.md`, `12-loc.txt`, `13-fixtures.txt`,
`02-clippy-pedantic.txt` (cast lints on my files), `15-coverage/summary.txt` and
`15-coverage/lcov.info` (per-line zero-hit extraction for all nine files), `03-test.txt`,
`09-miri.txt`.

I re-derived every arithmetic claim myself rather than trusting a worker: the three worker reports
were treated as leads and each row that entered this document was checked against the source. In
particular I recomputed the maximum values of `data_size`/`fixed_size` by hand (FMT-014), the
array-offset alignment arithmetic for all three formats (FMT-DOC-05), and dumped every committed
fixture's header with `od` to get real `A`/`C`/`ACI`/`B` values (FMT-008, FMT-DOC-05).

**Could not verify:** anything requiring a build. No `cargo` was run (per contract), so FMT-001's
panic, FMT-002's slice panic, and the fuzz-target designs are traced-by-reading, not executed —
each has an experiment request with an exact command. `cargo-fuzz` has no targets to collect
(`MANIFEST.md` line 45), so the fuzz section is a design, not a result. The differential lane
(`*_differential.rs`) is absent from the pack by design, so claims about C's behaviour rest on the
source citations already in the code plus `docs/format-spec.md`.

## Findings

### `FMT-001` — the `.lvi` asset→chunk index map is never validated, and `validate_store` indexes it raw

**P1** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-core/src/version_index.rs:169-187` (the only two value checks) and
  `crates/longtail-core/src/validate.rs:52-59`
- **What:** `VersionIndex::from_bytes` performs exactly two semantic checks — `ACI >= C`
  (`version_index.rs:169`) and the total-size compare (`:181`). It never checks that
  `asset_chunk_index_starts[a] + asset_chunk_counts[a] <= ACI`, nor that each
  `asset_chunk_indexes[j] < C`. `validate_store` then walks that map with plain indexing:

  ```rust
  let cidx = version_index.asset_chunk_indexes[start + k] as usize;   // validate.rs:56
  summed = summed.wrapping_add(version_index.chunk_sizes[cidx] as u64); // validate.rs:57
  ```

  Both indexes are attacker-controlled values from the file. The same pattern is repeated by five
  consumers outside my slice (`diff.rs:132`, `apply.rs:110`, `upsync.rs:226`, `cp.rs:85`,
  `inspect.rs:310`), so the parse layer is the single place a fix pays for all of them.
- **Failure scenario:** a `.lvi` that parses cleanly with `A=1, C=1, ACI=1` and
  `asset_chunk_index_starts = [0xFFFF_FFFF]` panics with "index out of bounds" at `validate.rs:56`.
  A `.lvi` with `asset_chunk_indexes = [7]` and `C=1` panics at `validate.rs:57`. Reachable from
  `validate-version`, `prune-store{,-index}`, `clone-store`, `ls`, `cp`, `upsync`, and — via
  `apply.rs:110` — the production Tauri download path, whose `desired` index is bytes fetched from
  S3. On a 32-bit target the same input is worse than a panic: `start + k` wraps (both operands are
  parsed `u32`s), yielding a small in-bounds index, so `validate_store` returns a **wrong verdict**
  silently.
- **Evidence:** `version_index.rs:169-187` is the complete set of value checks (read in full).
  `error.rs:74-85` already defines `ChunkRangeOutOfBounds` for exactly this class in `StoreIndex`
  — the asymmetry is the tell: the store index guards its offset/count pairs
  (`store_index.rs:166-174`), the version index does not. Coverage confirms no test reaches this:
  `validate.rs` shows 100.00% region/line coverage
  (`15-coverage/summary.txt`) because every test feeds it a *consistent* index, and worker (c)'s
  audit of all six test files found no test that constructs an out-of-range
  `asset_chunk_indexes`/`asset_chunk_index_starts`. The mutation proptest
  (`malformed.rs:288-305`) cannot catch it: it calls `from_bytes` and discards the result
  (`let _ = ...`), never touching an accessor or `validate_store`.
- **Recommendation:** validate the map once in `from_bytes`, after the size check — one pass over
  `ACI` u32s plus one pass over `A`:
  `if asset_chunk_indexes.iter().any(|&i| i as usize >= c) { return Err(...) }` and
  `start.checked_add(count) <= aci` per asset. Reuse `ChunkRangeOutOfBounds` or add an
  `InvalidAssetChunkIndex` variant. Cost is O(A + ACI) integer compares on a buffer already
  fully in memory — noise next to the parse itself (codecs run at 4–26 GiB/s per `lib.rs:29`).
  If a strict parse is judged too aggressive, the fallback is a checked accessor
  (`VersionIndex::asset_chunks(a) -> Result<&[u32], FormatError>`) that all six consumers must
  use — more churn, same protection.
- **Tradeoff / risk:** `COMPAT-RISK` — this rejects on read a file C accepts (C reads out of
  bounds instead, so there is no defined C behaviour to preserve, but a *wild-but-harmless* file
  would newly fail). Every committed fixture is internally consistent (`13-fixtures.txt` passes
  and all 16 fixture `.lvi`s parse today), so the gate that would catch a mistake is
  `fixtures/` + `crates/longtail/tests/lvi_byte_gate.rs`; both would keep passing.
- **Effort:** S
- **Regression test to add:** two hand-built `VersionIndex` byte buffers (start out of range;
  index out of range) asserting the specific `Err`, plus a `validate_store` call on each proving no
  panic. Add them to `tests/malformed.rs` next to the existing accessor tests.

### `FMT-002` — a `.lsb` payload is never checked against Σ`chunk_sizes`; the apply path slices past its end

**P1** · `hardening` · CONFIRMED · `COMPAT-RISK`

- **Where:** `crates/longtail-core/src/block.rs:129-139` (`StoredBlock::from_bytes`); consumer
  `crates/longtail/src/apply.rs:303-306` and `:342`
- **What:** `block.rs`'s module doc states the position honestly — "**No version field** — parse
  validation is the truncation check only" (`block.rs:5-6`). The truncation check covers the
  block-index region only; `payload = data[consumed..]` is accepted at any length, including
  shorter than the chunk sizes it claims. `docs/format-spec.md:241-246` states the invariant
  (`m_Tag == 0` → payload length is `Σ m_ChunkSizes`; `m_Tag != 0` → `uncompressed_size ==
  Σ m_ChunkSizes`) and **nothing in the production path checks either**. `compress.rs:313-319`
  validates the decoded length against the *frame's own* declared size, not against the block
  index.
- **Failure scenario:** a `.lsb` with one chunk of declared size 4096 and a 16-byte payload
  reaches `write_block_chunks`, which builds `(offset, size)` pairs from `chunk_sizes`
  (`apply.rs:303-306`) and then slices `&block.payload[block_off..block_off + len]`
  (`apply.rs:342`) → panic inside the `spawn_blocking` task. The panic is converted to an error by
  `flatten_apply_task` (`apply.rs:352-364`), so the blast radius is a failed download rather than a
  crashed process — but a panic in a library on remote input is still the wrong failure mode, and
  the diagnostic ("task panicked") tells the operator nothing about the corrupt block.
- **Evidence:** `block.rs:132-139` read in full; `apply.rs:342` read; `compress.rs:292-321` read —
  no Σ-`chunk_sizes` comparison exists anywhere in `crates/longtail*/src` (grep for a sum over
  `chunk_sizes` returns only progress accounting: `remote.rs:787`, `inspect.rs:109`,
  `upsync.rs:236`). `docs/rust-port.md:92` lists "size sums" as gate ④ — that is a *test-only*
  gate over fixtures, not a runtime check.
- **Recommendation:** in `BlockIndex::read_prefix`, compute `sum: u64 = chunk_sizes.iter().map(u64::from).sum()`
  (cannot overflow: `n ≤ u32::MAX` × `u32::MAX` < 2⁶⁴) and have `StoredBlock::from_bytes` reject
  only a payload that is **shorter** than required for `tag == 0`
  (`payload.len() < sum` → `Err(Truncated)`). Use `>=`, not `==`: C derives
  `m_BlockChunksDataSize` from the file length and ignores a longer tail, so an equality check
  could reject a block a real store contains. For `tag != 0` the same check belongs where the
  decoded buffer meets the block index (`compress::decode_stored_block`, R2's file — cross-ref).
- **Tradeoff / risk:** `COMPAT-RISK`, mitigated by choosing `>=`. Gate:
  `support/longtail-testkit/tests/*_golden.rs` (per-PR) parses all 32 committed `.lsb`s, and
  `crates/longtail-store/tests/sync_fixtures.rs` exercises the block path; both would catch a
  wrong-direction check immediately.
- **Effort:** S
- **Regression test to add:** in `tests/malformed.rs`, a `StoredBlock` byte buffer whose payload is
  one byte short of Σ`chunk_sizes` → `Err`; one byte long → `Ok` (documents the asymmetry
  deliberately).

### `FMT-003` — silently skipping wild blocks turns `prune-store` into a data-loss operation

**P1** · `security` · CONFIRMED

- **Where:** `crates/longtail-core/src/store_index.rs:580-583` (`get_existing_store_index`) and
  `:520-524` (`prune`); destructive consumer `crates/longtail/src/prune.rs:199-233`
- **What:** three `StoreIndex` methods handle a block whose `(offset, count)` runs off the chunk
  arrays by **skipping it** (`get_existing_store_index:582`, `block_payload_sizes:495`,
  `prune:522`), while a fourth errors (`push_block:167-174` → `ChunkRangeOutOfBounds`) and a fifth
  returns `None` (`block_index_at:455`). The skip is documented as a no-panic measure
  ("C would read OOB", `store_index.rs:520-521`) but its consequence for a *destructive* caller is
  not.
- **Failure scenario:** `prune-store` builds its keep-set from
  `get_existing_content(chunks, 0)` (`prune.rs:200`), i.e. `get_existing_store_index`. A store
  index in which one block's chunk range is corrupt → that block is skipped → its hash never
  enters `keep` → `store.prune_blocks(&keep_vec)` (`prune.rs:232`) **deletes the `.lsb` from the
  store**, permanently, for a block that is still referenced by the version. `--validate-versions`
  defaults to `false` (`crates/longtail-cli/src/main.rs:338`), so nothing intervenes on the default
  path; with `--validate-versions --skip-invalid-versions` the outcome is worse — the version
  contributes *no* blocks and all of its data becomes prune-eligible. `prune-store-index`
  (`prune.rs:328`) writes the shrunken index straight out, making the loss durable in the index
  too.
- **Evidence:** `store_index.rs:513-528` and `:562-659` read in full; `prune.rs:60-110` and
  `:157-240` read; `main.rs:330-341` read for the default. Coverage proves the skip is untested:
  `store_index.rs` line 523 (the `prune` skip) is zero-hit in `15-coverage/lcov.info`, as are lines
  488/492 (`block_payload_sizes`) and 450/456 (`block_index_at`). The only wild-index tests
  (`store_algebra.rs:108`, `:153`) assert the *skip* — i.e. they lock in the unsafe policy for the
  destructive caller.
- **Recommendation:** make the policy uniform and loud at the layer that can afford it. Cheapest
  correct version: `StoreIndex::prune` returns `Result<StoreIndex, FormatError>` and propagates
  `push_block`'s error, and `get_existing_store_index` gains a sibling that reports the skipped
  count so `prune.rs` can refuse to run destructively against a store index that is not
  self-consistent. A one-line alternative that fixes the worst case without an API change: have
  `prune_store`/`prune_store_index` call `StoreIndex::is_canonical`-style validation up front and
  abort. (`is_canonical` is currently private, `store_index.rs:295`.)
- **Tradeoff / risk:** no compat risk — the input is one C cannot handle either (it reads out of
  bounds), so there is no C behaviour to preserve. Risk of the change is an operator seeing a hard
  error where a prune previously "worked"; that is the point. No existing test would catch a
  regression here (line 523 is uncovered), which is itself part of the finding.
- **Effort:** M
- **Regression test to add:** `prune` over a store index with one corrupt block, asserting an
  `Err` (or a non-zero skipped count) rather than a quietly smaller index; plus a `prune-store`
  CLI test asserting a non-zero exit instead of a deletion.

### `FMT-004` — `to_bytes` can emit a parseable, silently-shifted `.lvi` from an inconsistent struct

**P2** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-core/src/version_index.rs:224-253`
- **What:** all thirteen `VersionIndex` fields are `pub` and no constructor or validator enforces
  the length invariants the doc states at `:30-34`. `to_bytes` derives the header counts from three
  specific arrays (`path_hashes.len()`, `chunk_hashes.len()`, `asset_chunk_indexes.len()`,
  `:235-237`) and then writes every array at its own length. It is infallible, so a mismatch is
  silent.
- **Failure scenario:** a caller (the Tauri app, or `downsync.rs:283`-style struct assembly) builds
  `A = path_hashes.len() = 2` but leaves `content_hashes` with one element. `to_bytes` writes a
  buffer 8 bytes shorter than `fixed_size(2, …)`. If `name_data` is ≥ 8 bytes — always true for
  real content — the buffer still satisfies `from_bytes`' size check, so it parses **successfully**
  with every array from `asset_sizes` onward shifted 8 bytes earlier and `name_data` 8 bytes
  shorter. The result is a self-consistent `.lvi` describing different content, which round-trips
  byte-identically forever after. No error, no gate.
- **Evidence:** `version_index.rs:224-253` and `:147-219` read in full; the corruption follows from
  the arithmetic in `fixed_size` (`:124-142`), which sums per-array strides from the *declared*
  counts while `to_bytes` emits *actual* lengths.
- **Recommendation:** add a private `debug_assert`-plus-`Result` pair: either make `to_bytes`
  return `Result<Vec<u8>, FormatError>` (breaking, but the honest signature), or add
  `pub fn check_invariants(&self) -> Result<(), FormatError>` and call it from `to_bytes` under
  `debug_assert!`, plus at the two write sites (`upsync`, `put`). The same argument applies to
  `BlockIndex` (`block.rs:106` writes `chunk_hashes.len()` while `chunk_sizes` may differ) —
  `from_block_indexes` already validates exactly that pair at `store_index.rs:424-431`, so the
  check exists and is simply not applied on the serialize path.
- **Tradeoff / risk:** `COMPAT-RISK` only in the sense that a currently-silent bug becomes an
  error. `crates/longtail/tests/{lvi_byte_gate,upsync_byte_gate}.rs` gate the real writers, so a
  `debug_assert` addition is safe.
- **Effort:** S
- **Regression test to add:** a `VersionIndex` with `content_hashes` one element short and an
  8+ byte `name_data`; assert `to_bytes` errors (or that `from_bytes(to_bytes(x))` is not silently
  `Ok` with different content).

### `FMT-005` — asset paths must be valid UTF-8 to be materialized; undocumented divergence

**P2** · `complexity` · CONFIRMED

- **Where:** `crates/longtail-core/src/version_index.rs:108-114`; consumer
  `crates/longtail/src/apply.rs:55-58`
- **What:** the format stores raw bytes: `m_NameData` is a NUL-terminated byte blob
  (`format-spec.md:60`), C `strcpy`s whatever the OS gave it, and POSIX filenames are arbitrary
  non-NUL bytes. The port's accessor pair is correctly split — `path_bytes` returns raw bytes,
  `path` decodes UTF-8 — but the download path uses the decoding one:
  `asset_path` (`apply.rs:56`) calls `vi.path(...)?`, so a single non-UTF-8 asset name fails the
  whole `change_version2` with `InvalidUtf8`.
- **Failure scenario:** a `.lvi` produced by golongtail from a Linux tree containing a latin-1 or
  Shift-JIS filename (routine in older game-asset pipelines) downsyncs fine with golongtail and
  fails outright with this port. That is a store C can read and this cannot — the paramount
  constraint inverted.
- **Evidence:** `version_index.rs:110-114` and `apply.rs:55-58` read. Not documented: grep for
  `UTF-8|utf8` across `readme.md`, `CLAUDE.md`, `docs/rust-port.md`, `docs/format-spec.md` returns
  **zero hits**, so this is a behavioural divergence absent from
  `docs/rust-port.md` §"Deliberate divergences". No fixture covers it: the corpus' only exotic name
  is `names/héllo-wörld-日本語.txt` (`support/longtail-testkit/src/corpus.rs:296`), which is valid
  UTF-8. The `InvalidUtf8` arm is exercised only by a hand-built struct (`malformed.rs:263`).
- **Recommendation:** decide and write it down. Either (a) accept the restriction and document it
  in `docs/rust-port.md` §"Deliberate divergences" with the failure mode, or (b) make the apply
  path byte-faithful on unix (`OsStr::from_bytes(vi.path_bytes(i)?)`) and keep the lossy `String`
  path for display only. (b) is the compat-preserving option; the path-safety checks it feeds are
  R6's (`fs_util.rs`) — **R6 owns the hostile-path question; my slice's answer is that rejecting a
  path at parse time is wrong for this format because the bytes are legal, so validation belongs
  at the point of materialization, not in the codec.**
- **Tradeoff / risk:** option (b) widens what the port will write to disk and interacts directly
  with path traversal — do not land it without R6's check in place. Windows has no byte-path
  equivalent, so (b) is unix-only and must fall back to a typed error on Windows.
- **Effort:** M
- **Regression test to add:** a fixture (or hand-built `.lvi`) with a `\xff`-containing asset name;
  assert the chosen behaviour explicitly rather than by omission.

### `FMT-006` — `Permissions::contains` documents "any", implements "all"

**P2** · `idiom` · CONFIRMED

- **Where:** `crates/longtail-core/src/perms.rs:41-45`
- **What:** the doc-comment reads "True if any of the given bits are set"; the body is
  `(self.0 & bits) == bits`, i.e. *all* of the given bits. For a single-bit argument the two agree,
  which is why nothing has bitten yet.
- **Failure scenario:** a future caller writes `p.contains(USER_WRITE | GROUP_WRITE)` to mean "is
  this writable at all" — the intended Windows read-only mapping question (`format-spec.md:570`,
  `fs_util.rs:208`) — and gets `false` for `0o644`. Silent wrong permission decision.
- **Evidence:** `perms.rs:41-45` read. The function has **zero call sites** in the workspace and
  `FNDA:0` in `15-coverage/lcov.info` (one of the four uncovered functions behind perms.rs'
  74.00%/61.29% coverage row).
- **Recommendation:** fix the doc to say "all", and add `contains_any` if the other predicate is
  wanted. Cheaper still: delete both `contains` and `from_bits` (also zero call sites, also
  zero-hit) — the tuple constructor `Permissions(bits)` is what every caller and test actually
  uses.
- **Tradeoff / risk:** none; no on-disk effect.
- **Effort:** S
- **Regression test to add:** `assert!(!Permissions(0o644).contains(0o222))` and
  `assert!(Permissions(0o644).contains(0o200))` — two lines in the existing `perms.rs` test module.

### `FMT-007` — zero fuzz targets over the workspace's only attacker-controlled input

**P2** · `hardening` · CONFIRMED

- **Where:** absent — no `fuzz/` directory exists anywhere in the repo (`find` over the tree,
  excluding `target/`); `MANIFEST.md:45` records `cargo-fuzz` installed with nothing to collect.
- **What:** the four codecs are the port's trust boundary — every byte comes from S3 or a local
  cache — and they are covered only by (a) exhaustive prefix truncation over three tiny
  hand-built samples, (b) three one-byte-mutation proptests that assert nothing but absence of
  panic, and (c) round-trip fixpoints over structurally-valid strategies. FMT-001 and FMT-002 are
  both instances a fuzzer finds in seconds.
- **Failure scenario:** the class the current tests cannot reach — internally inconsistent but
  well-sized buffers — is exactly where both P1s live.
- **Evidence:** worker (c)'s full inventory of the six test files, verified against my own reading
  of `malformed.rs` (327 lines) and `roundtrip_proptest.rs` (254 lines): the mutation proptests
  (`malformed.rs:303-304`, `:313-314`, `:324-325`) discard their results with `let _ =`, so they
  test `from_bytes` and nothing downstream.
- **Recommendation:** five targets, in a `fuzz/` crate excluded from the virtual workspace
  (`cargo fuzz init` adds an empty `[workspace]` to `fuzz/Cargo.toml`; keep `publish = false` and
  do not add it to `default-members`). Corpora seed for free from `fixtures/`:

  | target | input | seed corpus | invariant |
  |---|---|---|---|
  | `vi_parse` | `&[u8]` → `VersionIndex::from_bytes` | `fixtures/**/*.lvi` (16 files) | never panics; on `Ok`, `to_bytes() == input` (byte fixpoint — holds for any accepted buffer because every header count is re-derived from the parsed array lengths) |
  | `si_parse` | `&[u8]` → `StoreIndex::from_bytes` | `fixtures/**/*.lsi` (29) | same, plus `Err` for any oversize buffer |
  | `lsb_parse` | `&[u8]` → `StoredBlock::from_bytes` | `fixtures/**/chunks/**/*.lsb` (32) | same fixpoint; after FMT-002, `Err` for a short payload |
  | `vi_walk` | `&[u8]` → parse, then `path_bytes`/`path`/`is_dir` for every asset **and** `validate_store` against a fixture `.lsi` | `fixtures/**/*.lvi` | never panics — **this target reproduces FMT-001 directly** |
  | `si_algebra` | `(&[u8], &[u8])` via `arbitrary` → parse both, then `merge`, `merge_consuming`, `prune`, `get_existing_store_index`, `block_payload_sizes` | pairs from `fixtures/**/*.lsi` | never panics; `merge(a,b).to_bytes() == a.clone().merge_consuming(&b).to_bytes()` (the S3 shard name depends on it) |

  CI: a single scheduled job, `cargo +nightly fuzz run <t> -- -max_total_time=60 -rss_limit_mb=2048`
  per target (5 min total), plus `cargo +nightly fuzz run <t> <seed-corpus> -runs=0` on every PR as
  a regression replay — that second form is cheap and is what keeps a fixed crash fixed.
- **Tradeoff / risk:** needs nightly; the `fuzz/` crate must stay out of `default-members` or a
  plain `cargo build` starts requiring nightly. `-rss_limit_mb` matters: an absurd header count is
  rejected before allocation today (FMT-014), and the limit is what proves it stays that way.
- **Effort:** M
- **Regression test to add:** the crash corpus itself; commit each reproducer under
  `fuzz/corpus/<target>/`.

### `FMT-008` — no golongtail-produced empty index; §9's first edge case is verified only against our own writer

**P2** · `hardening` · CONFIRMED

- **Where:** `fixtures/` (all 16 `.lvi`, all 29 `.lsi`); rule at `docs/format-spec.md:611-616`
- **What:** §9's first bullet — a zero-count index still carries its full fixed header, and readers
  must not treat a zero count as an absent header — is the kind of rule an implementation gets
  wrong silently. Every committed fixture has non-zero counts: the smallest `.lvi` is
  `A=2, C=2, ACI=2` (`get-configs/version.lvi`, `stores/meow/zoo.lvi`); the smallest `.lsi` is
  `B=1, C=2`. The `A=0`/`B=0`/`C=0` case is exercised only by our own proptest strategies
  (`roundtrip_proptest.rs:27`, `:88`, `:133` all start at 0) and unit tests, i.e. against the port's
  own writer.
- **Failure scenario:** if golongtail wrote an empty index differently (a 0-byte file, or a
  version-only header), the port would round-trip its own bytes happily and fail on the real thing
  — and no gate would notice, because gate ① only checks what is committed.
- **Evidence:** `od -An -tu4 -N24` over every `fixtures/**/*.lvi` and `-N16` over every
  `fixtures/**/*.lsi` (headers dumped in this review). `13-fixtures.txt` confirms the manifest is
  complete and verified, so the absence is real, not a listing artifact.
- **Recommendation:** add one fixture cell: upsync an empty directory with the pinned golongtail
  v0.4.5 and commit the resulting `.lvi`/`.lsi`. If golongtail refuses to create one, record *that*
  in `format-spec.md` §9 — "unreachable via the CLI" is a useful fact and closes the question.
- **Tradeoff / risk:** none; one more fixture cell (bytes: tens).
- **Effort:** S (see experiment #2)
- **Regression test to add:** the round-trip gate picks it up automatically once committed.

### `FMT-009` — `FileInfos`' accessor error arms are dead while `VersionIndex`' twins are tested

**P3** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-core/src/file_infos.rs:103-113` and `:117-121`
- **What:** `FileInfos::path_bytes`/`path` are line-for-line twins of
  `VersionIndex::path_bytes`/`path`, with the same three error arms. The `VersionIndex` arms each
  have an exact-variant test (`malformed.rs:237`, `:251`, `:263`); the `FileInfos` ones have none.
- **Failure scenario:** `FileInfos` is `pub` with `pub` fields, so a caller can hand-build one with
  an out-of-range `path_start_offsets` entry; the arm that must catch it has never executed. Low
  reachability (the production builder `from_scanned_entries` cannot produce one), which is why
  this is P3 rather than P2.
- **Evidence:** `15-coverage/lcov.info` zero-hit lines for `file_infos.rs`: 104, 105, 106, 107
  (`NameOffsetOutOfBounds`) and 112 (`UnterminatedName`); the file's one missed *function* is the
  `map_err(|_| InvalidUtf8 …)` closure at `:120`, which is structurally dead because the blob is
  built from `String`s.
- **Recommendation:** three tests mirroring `malformed.rs:237-271`, or — better — collapse the
  duplication: both types hold "blob + offsets" and could share one free function
  `name_at(blob: &[u8], offsets: &[u32], i: usize)`, halving the surface and making one test set
  cover both.
- **Tradeoff / risk:** the shared-helper refactor touches two public accessors; behaviour must stay
  identical (both are already byte-identical in logic).
- **Effort:** S
- **Regression test to add:** as above.

### `FMT-010` — `len() as u32` header counts sit next to a `try_from` doing the same job

**P3** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-core/src/store_index.rs:436` (`n as u32`) versus `:432-433`
  (`u32::try_from(...)`); same pattern at `block.rs:106`, `version_index.rs:235-237`,
  `store_index.rs:145-146`, and the `chunk_count()`/`asset_count()`/`block_count()` accessors
  (`block.rs:30`, `version_index.rs:69,74,79`, `store_index.rs:63,68`).
- **What:** within six lines, `from_block_indexes` guards the *offset* with `u32::try_from` (→
  `SizeOverflow`) and truncates the *count* with `as u32`. Both derive from in-memory `Vec`
  lengths; the inconsistency is arbitrary.
- **Failure scenario:** a hand-built `BlockIndex` with more than `u32::MAX` chunks writes a wrapped
  count into the header, producing a file that parses as a different (much smaller) block. Requires
  ≥32 GiB of hashes in RAM, so this is a latent-consistency finding, not a live bug — but the
  asymmetry is exactly the sort of thing that survives a refactor into somewhere reachable.
- **Evidence:** `02-clippy-pedantic.txt` flags each site by name
  (`casting usize to u32 may truncate the value…` at `block.rs:106:15`, `file_infos.rs:67:37`, …) —
  157 `cast_possible_truncation` hits workspace-wide.
- **Recommendation:** use `u32::try_from(...).map_err(|_| FormatError::SizeOverflow)?` wherever the
  function already returns `Result` (`from_block_indexes`), and keep `as u32` only in the
  infallible accessors, with a one-line comment stating why it cannot overflow for a parsed index
  (the count came from a `u32`). Do not sprinkle `#[allow]`.
- **Tradeoff / risk:** none on disk.
- **Effort:** S
- **Regression test to add:** none warranted; this is a consistency fix.

### `FMT-011` — `path_data.len() as u32` truncates silently past a 4 GiB name blob

**P3** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-core/src/file_infos.rs:67`, with `:87` and `:92` as the same class
- **What:** the name-blob offset is the one `as u32` in my slice whose input is not itself bounded
  by a parsed `u32`: it is the running length of a locally built blob. Past 4 GiB it wraps, and
  every subsequent offset points into the wrong string.
- **Failure scenario:** a scan of ~10 million assets with ~450-byte average paths crosses 4 GiB of
  `path_data`. The resulting `.lvi` is well-formed and parses cleanly, with garbage paths for
  everything after the wrap. That is corruption without an error — worse than a refusal.
- **Evidence:** `file_infos.rs:52-83` read in full; flagged by
  `02-clippy-pedantic.txt` at `file_infos.rs:67:37`. Note the format itself caps
  the blob at `u32::MAX` (`m_NameOffsets` is `u32`), so the correct behaviour is a typed error, not
  a wider type.
- **Recommendation:** `u32::try_from(path_data.len())` and make `from_scanned_entries` return
  `Result<FileInfos, FormatError>` (`SizeOverflow`). It has exactly the callers you would expect
  (the scan path); the signature change is contained.
- **Tradeoff / risk:** a fallible constructor ripples into the upsync scan; a `debug_assert!` plus
  a documented limit is the cheap 80% if that ripple is unwelcome now.
- **Effort:** S
- **Regression test to add:** not practical at 4 GiB; assert the `Result` shape with a unit test on
  a synthetic offsets vector instead, or accept the `debug_assert`.

### `FMT-012` — usage-percent `as u32` can truncate once the u32 size sums wrap

**P3** · `hardening` · PLAUSIBLE

- **Where:** `crates/longtail-core/src/store_index.rs:584-598`
- **What:** `block_use`/`block_size` are `u32` accumulators using `wrapping_add` (`:588`, `:590`) —
  deliberate, to mirror C's `uint32_t` sums. The percentage is then computed in `u64` and cast back:
  `((block_use as u64 * 100) / block_size as u64) as u32` (`:597`). Once the sums wrap,
  `block_use > block_size` becomes possible and the quotient can exceed `u32::MAX`, so the final
  cast truncates.
- **Failure scenario:** an adversarial `.lsi` whose per-block chunk sizes sum to just over 2³²
  yields a small `block_size` and a large `block_use`; the truncated percentage changes which
  blocks `get_existing_store_index` selects, so a download plan differs from C's. Real block sizes
  are ~8 MiB (`format-spec.md:444`), so this needs a hostile index — hence PLAUSIBLE: I traced the
  arithmetic but cannot show a natural input.
- **Evidence:** `store_index.rs:584-598` read; the `block_size == 0` arm at `:594` is zero-hit in
  `15-coverage/lcov.info` (line 595), confirming the wrap regime is untested.
- **Recommendation:** accumulate in `u64` and compare in `u64`, or clamp the percentage with
  `.min(100)`. **Compat note:** widening the accumulator diverges from C's wrapping `uint32_t`
  *on inputs where C's own result is meaningless*; if bit-exact wrap parity is wanted, keep `u32`
  and clamp only the final cast.
- **Tradeoff / risk:** `COMPAT-RISK` in the adversarial regime only; the differential lane
  (`support/longtail-testkit/tests/*_differential.rs`, **weekly**) is the only gate that would
  notice, and it does not generate wrapping size sums today.
- **Effort:** S
- **Regression test to add:** a store index with two chunks of size `0x8000_0000` and a query
  covering one, asserting the selected block set.

### `FMT-013` — the cursor's truncation guard is unreachable from every caller and untested

**P3** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-core/src/cursor.rs:47-58`
- **What:** every codec computes its total size from the header counts and compares it against
  `data.len()` *before* reading any array (`version_index.rs:181-187`, `store_index.rs:105-117`,
  `block.rs:59-65`) — this mirrors C's up-front compare and is the compat-relevant check. The
  cursor's own `take` bound is therefore pure defence in depth, and it never fires.
- **Failure scenario:** none today — this is a coverage/maintenance finding. Its value is negative
  evidence: the guard is what makes a *future* codec that forgets the up-front check fail safely,
  and nothing proves it works.
- **Evidence:** `15-coverage/lcov.info` shows `cursor.rs` lines 50-53 (the whole
  `Err(FormatError::Truncated { … })` return) zero-hit, and that is with `malformed.rs:88-131`
  iterating **every** prefix length of three formats. `cursor.rs` has no `#[cfg(test)] mod tests`
  of its own; its 93.21% region coverage is entirely incidental.
- **Recommendation:** add a small `#[cfg(test)] mod tests` inside `cursor.rs` (the module is
  `pub(crate)`, so an integration test cannot reach it) exercising `take` past the end, each
  `*_vec` with `n` larger than the buffer, and `remaining()` at `pos == len`. Six assertions.
- **Tradeoff / risk:** none.
- **Effort:** S
- **Regression test to add:** as above; this *is* the test.

### `FMT-014` — `SizeOverflow` is unreachable on 64-bit, so 56 guards are only load-bearing on an untested target

**P3** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-core/src/cursor.rs:12-23` and every caller
  (`version_index.rs:129-141`, `store_index.rs:72-81`, `block.rs:35-40`)
- **What:** the guards are correct and I am **not** recommending their removal — the point is to
  record why they look dead so nobody removes them. With all counts at `u32::MAX`, the largest
  size any of the three `*_size` functions can compute is: `.lvi` `24 + 58 × (2³²−1) ≈ 2.5 × 10¹¹`;
  `.lsi` `16 + 32 × (2³²−1) ≈ 1.4 × 10¹¹`; `.lsb` `20 + 12 × (2³²−1) ≈ 5.2 × 10¹⁰`. All are ~8
  orders of magnitude below `usize::MAX` on 64-bit, so no `checked_mul`/`checked_add` in a parse
  path can ever return `Err` there. On 32-bit they all can, and they are the only thing standing
  between an absurd header and a wrapped allocation size.
- **Failure scenario:** none on the shipped targets. The risk is the inverse: CI builds only
  x86_64 Linux + Windows (`.github/workflows/rust.yaml`), so the 32-bit behaviour these guards
  exist for is never exercised, and the one 32-bit-specific hazard I found (`start + k` in
  `validate.rs:56`, FMT-001) is invisible there.
- **Evidence:** arithmetic above, computed from `HEADER_SIZE` and the strides I read in each
  `*_size`. Corroborating coverage: `version_index.rs` has **100.00% line** but **87.54% region**
  (42/337 missed) and `block.rs` 100.00% line / 92.86% region — the missing regions are the `?`
  error edges on the checked arithmetic. `malformed.rs:214-215` says so in a comment
  ("`Truncated` on 64-bit, or `SizeOverflow` on 32-bit"), and asserts only `is_err()`. No
  `SizeOverflow` assertion exists anywhere in the suite.
- **Recommendation:** add one cheap CI step —
  `cargo check -p longtail-core --target i686-unknown-linux-gnu` plus, if a runner is easy,
  `cargo test -p longtail-core --target i686-unknown-linux-gnu` — or state in
  `docs/rust-port.md` that 64-bit targets are the supported set and the guards are belt-and-braces.
  Either resolution is fine; silence is not, because it leaves 56 sites looking like dead code.
- **Tradeoff / risk:** a 32-bit lane costs a target install and would likely surface FMT-001's
  wrap path immediately (a feature, not a bug).
- **Effort:** S
- **Regression test to add:** `#[cfg(target_pointer_width = "32")]` assertions that a `u32::MAX`
  header yields `SizeOverflow`, guarded so the 64-bit lane skips them.

### `FMT-015` — `validate_store` reports a bad name blob as a size mismatch

**P3** · `complexity` · CONFIRMED

- **Where:** `crates/longtail-core/src/validate.rs:48`
- **What:** `let is_dir = version_index.is_dir(a).unwrap_or(false);` swallows all three accessor
  errors (offset out of range, missing NUL, index out of bounds). A directory whose name blob is
  malformed is then treated as a file, its `Σ chunk_sizes` (0) is compared against its
  `asset_sizes` (0) — which happens to pass — but a *file* with a malformed name is silently
  reclassified and, if the chunk map is inconsistent, contributes a `SizeMismatch`.
- **Failure scenario:** a corrupt `.lvi` reports "store does not match version: 1 asset(s) with
  size mismatch" when the real defect is an unterminated name. The operator chases the store, not
  the index. `prune.rs:87` and `:94` turn that message into a hard error or a skipped version, so
  the misdiagnosis has consequences.
- **Evidence:** `validate.rs:31-73` read in full; the three error arms it discards are
  `version_index.rs:93/96/104`.
- **Recommendation:** propagate it — `ValidateError` gains a `MalformedIndex(FormatError)` variant,
  or `validate_store` returns `Result<(), ValidateError>` where `ValidateError` wraps
  `FormatError`. C has nothing to match here (it cannot fail this way), so there is no compat
  constraint.
- **Tradeoff / risk:** `ValidateError` is public and re-exported (`lib.rs:75`); adding a variant is
  a minor breaking change for exhaustive matchers.
- **Effort:** S
- **Regression test to add:** `validate_store` over a version index with an unterminated name →
  assert the new variant, not `SizeMismatch`.

### `FMT-016` — `POSIX_MASK` has no call site; the one place that must mask re-declares it

**P3** · `idiom` · CONFIRMED

- **Where:** `crates/longtail-core/src/perms.rs:26-27`; duplicate at
  `crates/longtail/src/fs_util.rs:17`
- **What:** `Permissions::POSIX_MASK = 0o0777` is `pub` and unused; `fs_util.rs` declares its own
  `const PERM_MASK: u32 = 0x1FF` and uses it at both the scan (`:23`) and the `chmod` (`:197`).
- **Failure scenario:** the masking at `fs_util.rs:197` is load-bearing for security, not tidiness:
  `Permissions` deliberately preserves all 16 bits (`perms.rs:8-10`), and bits 9–11 of a POSIX mode
  are sticky/setgid/**setuid**. A hostile `.lvi` with `permissions = 0o4755` would create a setuid
  executable if that mask were ever dropped. Today it is present and correct
  (`0x1FF == 0o777`, verified), but the constant that documents *why* lives in another crate and is
  never referenced, so a future edit has nothing tying it to §7.
- **Evidence:** `perms.rs:16-27`, `fs_util.rs:17-23`, `:186-199` read; grep shows `POSIX_MASK`
  appears exactly once in the workspace (its definition). Coverage cannot show a `const`, but the
  four uncovered `perms.rs` functions (`from_bits`, `contains`, both `From` impls) confirm the type's
  API is mostly unused.
- **Recommendation:** have `fs_util` use `Permissions::POSIX_MASK` (widening at the call site) and
  delete the local duplicate, so the mask, its spec citation, and its security rationale sit
  together. **R6 owns `fs_util.rs`; my slice's answer is that the mask is correct today and the
  fix is a consolidation, not a bug fix.**
- **Tradeoff / risk:** none.
- **Effort:** S
- **Regression test to add:** an apply-path test asserting that a `0o4755` permission in a version
  index materializes as `0o755` on disk. Worth having regardless of the refactor — it pins the
  security property.

## Lower-priority observations

- `store_index.rs:621` — `for idx in offset..(offset + count)` is a plain `+` on two parsed values,
  safe only because the same triple passed `checked_add` plus a bounds test 40 lines earlier
  (`:580-582`) in a different loop. Currently correct (I verified every path into that loop goes
  through `potentials.push` at `:602`); fragile under refactor. Reuse `block_index_at` or re-derive
  `end` locally.
- `store_index.rs:526` — `out.hash_identifier = self.hash_identifier;` is redundant;
  `StoreIndex::empty(self.hash_identifier)` at `:515` already set it.
- Merge's remote-side internal-duplicate skip is untested in both implementations
  (`store_index.rs:279` and `:378` zero-hit); only the local side has a test (`merge.rs:91`).
- `get_existing_store_index` is never called with `min_block_usage_percent > 100`
  (`store_index.rs:637` zero-hit); the C-cited `<= 100` gate's false edge — which returns an empty
  index — is unverified against C.
- `block_index_at`'s two `None` returns (`store_index.rs:450`, `:456`) and
  `is_canonical`'s length-mismatch `false` (`:302`) and `from_block_indexes`' parallel-length guard
  (`:425-430`) are all zero-hit. `si_strategy` (`roundtrip_proptest.rs:88-97`) always generates
  length-consistent arrays, so even the "arbitrary" proptest cannot reach them.
- `merge_consuming`'s conflicting-identifier error is asserted as `is_err()` only
  (`store_algebra.rs:235-236`), while `merge`'s is variant-exact (`merge.rs:67`). Same for the
  truncation loops (`malformed.rs:97`, `:114`, `:126`) and the huge-count tests (`:216`, `:228`).
- `error.rs` and `lib.rs` have **no row at all** in `15-coverage/summary.txt` and no `SF:` record in
  `lcov.info` — both are declaration-only, so this is expected, not a gap. Recorded so the next
  session doesn't hunt for it.
- `merge_consuming`'s fast path skips `push_block`'s per-block bounds check on `self`. I verified
  this is sound: `is_canonical` (`:295-312`) requires exactly-cumulative offsets ending at
  `chunk_hashes.len()`, which implies every block's range is in bounds, and the `u32::try_from`
  offset guard is satisfied because the offsets were parsed as `u32`. No divergence from `merge`.

## Comments & documentation issues

### `FMT-DOC-01` — the merge byte-identity contract is attached to a private helper

**P2** · CONFIRMED · **Where:** `crates/longtail-core/src/store_index.rs:189-227`

A 31-line doc block documenting `merge`'s C-source-cited semantics ("**byte-for-byte on the success
path** … the S3 store-index shard name is the sha256 of these bytes, so byte-identity is
load-bearing") runs from `:189` to `:219` and is immediately followed — with **no blank line and no
item between** — by three more `///` lines describing `reserve_capacity` (`:217-219`), whose
signature is at `:220`. The whole block therefore documents the **private** `reserve_capacity`, and
`pub fn merge` at `:229` has no doc-comment at all. Consequence: the most compat-critical
explanation in the crate is invisible in rustdoc, and the public method looks undocumented.
`cargo doc` cannot catch this (`08-doc.txt` lists four unrelated defects). **Fix:** move the
`reserve_capacity` sentences below the `merge` block and put each on its own item; a blank line is
not enough. **Effort:** S.

### `FMT-DOC-02` — "malformed input never panics" is false in the same crate

**P2** · CONFIRMED · **Where:** `crates/longtail-core/src/error.rs:4` and `src/lib.rs:40-42`

`error.rs:4` states "Malformed input must always surface as an `Err` — never a panic and never a
silent wrap"; `lib.rs:40` states "**Malformed input never panics.**" FMT-001 disproves both:
`validate_store` — same crate, taking exactly the structs `from_bytes` produces — panics on a
parseable `.lvi`. The `lib.rs` sentence is then qualified by a following clause about size
computations, which is where the true claim lives; the heading sentence overpromises. **Fix:** after
FMT-001 lands, both claims become true and should stay verbatim. Until then, scope them to the
parse layer explicitly. **Effort:** S.

### `FMT-DOC-03` — `format-spec.md` §2 names an fs lock file the port deliberately does not implement

**P2** · CONFIRMED · **Where:** `docs/format-spec.md:202-205`

§2 "Naming & locking" presents `<store_root>/store.lsi.sync` as *the* fs lock, citing
`longtail_fsblockstore.c:1443`. The port implements a different, golongtail-compatible scheme —
flock on `store.lsi._lck` plus a `store.lsi.gen` generation sidecar — and
`crates/longtail-store/src/sync.rs:17-21` explicitly flags the spec as misattributing "C's
FSBlockStore lock, a different component". Since golongtail produced every fixture and is what the
CI/CD pipeline runs, the Go scheme is the compat-relevant one and the spec should lead with it,
noting `store.lsi.sync` as C-fsblockstore-only. Interop consequence worth stating in the spec: a C
`fsblockstore` writer and this port sharing one filesystem store would **not** mutually exclude.
(**R3 owns `sync.rs`**; the spec text is §2, mine.) **Effort:** S.

### `FMT-DOC-04` — store-index block order is non-deterministic; that fact lives only in a CI comment

**P2** · CONFIRMED · **Where:** `docs/format-spec.md:178-190` (the gap);
`.github/workflows/fixture-freshness.yaml:5-6`; `crates/longtail-core/src/store_index.rs:21-23`

The workflow says it compares store indexes "semantically (equal sorted block-hash set) because
golongtail emits their block order non-deterministically; everything else is byte-exact". That is a
format-level invariant — **two `.lsi` files describing the same content need not be byte-equal, so
byte comparison is not a valid equality test for `.lsi`** — and it is stated only in a CI comment
and a Rust doc-comment. §2 currently reads as if the array order were canonical. Confirmed as
requested: it belongs in `format-spec.md` §2, alongside the consequence for `store_<sha256>.lsi`
shard naming (different orderings of the same content produce different shard names, which is why
merge-on-read is required rather than merely convenient). **Effort:** S.

### `FMT-DOC-05` — §9's misalignment bullet points at the wrong counts

**P3** · CONFIRMED · **Where:** `docs/format-spec.md:638-644`

§9 says a `u64` array "following an odd number of preceding `u32`s (or an odd-length `u16` array as
in `m_Permissions`)" may be misaligned, and that "the fixture corpus commits odd-asset-count
cases". Both halves are imprecise. Deriving from the header sizes and strides:

- `.lvi`: `path_hashes`/`content_hashes`/`asset_sizes` start at byte 24 and are **always** 8-aligned.
  `chunk_hashes` starts at `24 + 32·A + 4·ACI`, so it is 4-aligned-only iff **ACI is odd** — `A`'s
  parity is irrelevant. `m_Permissions` is the last array before the byte blob, so its length never
  misaligns anything.
- `.lsi`: both `u64` arrays start at 16 and `16 + 8·B` — **never** misaligned, for any counts.
- `.lsb`: `chunk_hashes` starts at byte 20 (`8 + 4 + 4 + 4`), so it is **always** 4-aligned and
  never 8-aligned — every `.lsb` is a misalignment case.

The corpus does cover it, but for the other reason: odd `ACI` appears in
`stores/chunk-1024/zoo.lvi` (9447), `stores/default/chain-v1.lvi` (7), `chain-v3.lvi` (9),
`stores/default/zoo.lvi` (2061) and `stores/sharded/version.lvi` (9) — verified by dumping the
headers with `od`. **Fix:** replace "odd asset count" with "odd `m_AssetChunkIndexCount` (`.lvi`)
and every `.lsb`", and note that `.lsi` is always aligned. This matters because it tells a future
porter which fixture actually protects the invariant. **Effort:** S.

### `FMT-DOC-06` — §3 documents C's temp-file shape as if it were part of the format

**P3** · CONFIRMED · **Where:** `docs/format-spec.md:277-285`

"§3 fs-store write path & integrity behaviors" states blocks are written to
`<final block path>.<16-hex-unique-id>` and that "stray `.lsb.<16hex>` files in a store are
abandoned temp writes". The port writes `<path>.tmp.<pid>.<hex>` instead
(`crates/longtail-store/src/blob/fs.rs:229-233`), so a reader of the spec will look for the wrong
strays. The rename-into-place *semantics* are preserved and both shapes are excluded by the
`.lsb`/`.lsi` suffix filters, so nothing is broken — but the section should be marked C-only /
informational, or list both shapes. (**R3 owns `blob/fs.rs`.**) **Effort:** S.

### `FMT-DOC-07` — the `VersionIndex` invariant doc promises more than `from_bytes` delivers

**P2** · CONFIRMED · **Where:** `crates/longtail-core/src/version_index.rs:30-34`

"All per-asset arrays have length `asset_count` … `VersionIndex::from_bytes` guarantees these
invariants; hand-built structs must uphold them for `to_bytes` to produce a well-formed buffer."
The guarantee is about array *lengths* only. A reader reasonably concludes the parsed index is
internally consistent — which is precisely the assumption the six raw-indexing consumers make, and
precisely what FMT-001 shows is false. **Fix:** state explicitly that the *values* in
`asset_chunk_index_starts`/`asset_chunk_indexes` are unvalidated (or, after FMT-001, that they are
validated and how). Also drop the "hand-built structs must uphold them" line's implication that
`to_bytes` fails loudly when they don't — it doesn't (FMT-004). **Effort:** S.

### `FMT-DOC-08` — `rust-port.md`'s strictness claim is incomplete in a way that matters

**P2** · CONFIRMED · **Where:** `docs/rust-port.md:127-128`

"**Strictness beyond C on malformed input.** Readers reject `AssetChunkIndexCount < ChunkCount` and
trailing bytes, and return typed errors where C silently wraps 32-bit arithmetic." True as far as it
goes, and it reads as a summary of the port's malformed-input posture. Given FMT-001 and FMT-002,
the honest version names the two things readers do *not* check (the asset→chunk map; the `.lsb`
payload length) — or the sentence becomes fully true once those land. This is the entry a future
maintainer will trust when deciding whether a parsed index is safe to index into. **Effort:** S.

### `FMT-DOC-09` — `.lsb` has no version field, and the spec does not draw the consequence

**P3** · CONFIRMED · **Where:** `docs/format-spec.md:216`, `:649-653`;
`crates/longtail-core/src/block.rs:5-6`

The spec states the fact ("**No version field** — the block index header has no version u32 at
all") but not what follows: `.lsb` is the one format with **no forward-compatibility signal**. A
future upstream change to the block-index header would not be rejected — it would be parsed as a
different chunk count and most likely surface as `Truncated`, or, for an unlucky layout, as a
plausible-looking block. This belongs in §9's "Reader version strictness" bullet, which currently
says version equality is checked for "every format's version field" without noting that one format
has none. **Effort:** S.

## Hardening backlog

Ranked by (protection gained) / (effort):

1. **Validate the `.lvi` asset→chunk map in `from_bytes`** (FMT-001). One O(A+ACI) pass removes a
   panic class from six call sites including the production download path.
2. **Reject a short `.lsb` payload** (FMT-002). One `u64` sum in `read_prefix`.
3. **Fuzz targets `vi_walk` and `si_algebra` first** (FMT-007) — they cover the inconsistent-but-
   well-sized class that no current test reaches, and `vi_walk` is a live reproducer for FMT-001.
4. **Make the wild-block skip loud on destructive paths** (FMT-003).
5. **`cursor.rs` unit tests** (FMT-013) — six assertions, converts incidental coverage into intent.
6. **Variant-exact assertions** where tests currently accept any `Err`: `malformed.rs:97/114/126/216/228`,
   `store_algebra.rs:235`, and the `Err/Err` arms of the two merge proptests
   (`roundtrip_proptest.rs:203`, `:215`). A parser that starts returning the *wrong* typed error
   would pass every one of them today.
7. **`FileInfos` accessor error tests** (FMT-009), ideally via a shared `name_at` helper.
8. **Proptest strategies that generate inconsistent lengths** — `si_strategy`
   (`roundtrip_proptest.rs:88`) always produces parallel arrays of matching length, which is why
   `is_canonical`'s length branch, `block_payload_sizes`' `.get` miss, and
   `from_block_indexes`' guard are all unreachable in the suite. One extra strategy reaches four
   uncovered arms.
9. **32-bit `cargo check` (or test) lane** (FMT-014), or an explicit 64-bit-only statement.
10. **Miri already covers the codecs** — `09-miri.txt` records 96 tests across 8 binaries passing.
    Keep the proptest case caps that make that tractable (`config()` at `malformed.rs:10`,
    `roundtrip_proptest.rs:14`); do not raise them without checking miri's runtime.
11. **An empty-index fixture from golongtail** (FMT-008) and a **non-UTF-8-name fixture** (FMT-005).

## Verified good

Do not re-audit these; I traced them:

- **Unaligned-read discipline holds absolutely.** Zero `&[u64]`/`&[u32]` casts, `align_to`,
  `transmute`, `from_raw_parts`, `bytemuck`, or `zerocopy` anywhere in the nine files. Every scalar
  goes through `from_le_bytes`/`to_le_bytes`, and the vector readers use
  `slice::as_chunks::<N>()` (`cursor.rs:79`, `:86`, `:93`) — a safe std API returning `&[[u8; N]]`,
  byte-wise and alignment-free. `#![forbid(unsafe_code)]` at `lib.rs:43`. The cross-cutting rule at
  `format-spec.md:14-16` is satisfied; only §9's *description* of which fixtures cover it is off
  (FMT-DOC-05).
- **Every read is bounds-checked exactly once, in one place.** All five reader methods funnel
  through `Reader::take` (`cursor.rs:47-58`): `checked_add(pos, n)` then `end > data.len()`. No
  unchecked read exists. The `b[0]..b[7]` indexing inside `u32`/`u64` is on slices `take` proved to
  be exactly 4/8 bytes long.
- **No `unwrap`/`expect` in any of the nine files.** The only relatives are
  `unwrap_or(HEADER_SIZE)` capacity hints (`version_index.rs:230`, `store_index.rs:141`,
  `block.rs:97`, `:146`), `unwrap_or_else` (`store_index.rs:658`) and `unwrap_or(false)`
  (`validate.rs:48`, see FMT-015) — none can panic.
- **No `u64`-parsed-to-`usize` cast exists.** Every disk-sourced length/offset/count is a `u32`,
  and `u32 as usize` is exact on both 32- and 64-bit. The parsed `u64`s (hashes, asset sizes) are
  only ever compared or used as map keys inside my slice.
- **Allocation is bounded by the buffer, not by the header.** `u16_vec`/`u32_vec`/`u64_vec` call
  `take(n × stride)` *before* collecting, so a `u32::MAX` count cannot drive an allocation — it
  errors first. This closes the absurd-count DoS class structurally, not by luck.
- **Version constants are correct and byte-verified.** `.lvi` `0x0000_0002`
  (`version_index.rs:11`), `.lsi` `0x0100_0000` (`store_index.rs:14`); both match
  `format-spec.md:38/173` and the C constants, and I confirmed them against every committed
  fixture header (`od`): all 16 `.lvi`s carry `2`, all 29 `.lsi`s carry `16777216`.
- **Field order and strides match the spec exactly**, for all three formats, including the
  easily-transposed `.lvi` tail (`name_offsets` then `permissions` then the blob) — read/write
  order at `version_index.rs:189-201`/`:238-251` versus `format-spec.md:49-60`.
- **Round-trip byte fixpoint is structurally sound**, which is what makes the fuzz invariant in
  FMT-007 valid: every header count is re-derived from the parsed array length, the version is
  re-emitted as the accepted constant, and the tail (`name_data`/payload) is preserved verbatim, so
  `to_bytes(from_bytes(b)) == b` for every accepted `b`.
- **`merge_consuming` is equivalent to `merge`** on the fast path — verified by hand (see the last
  bullet of "Lower-priority observations"), and gated by two proptests
  (`roundtrip_proptest.rs:200`, `:212`) comparing serialized bytes.
- **§7 permission handling is correct end to end.** All nine bit constants match
  `format-spec.md:542-550`; preserving all 16 bits on parse is right (C `memcpy`s them, and
  round-trip byte-identity depends on it); and the write-back masks to `0o777`
  (`fs_util.rs:197`), so the setuid/setgid/sticky bits of a hostile index cannot reach `chmod`.
  FMT-016 is a consolidation, not a hole.
- **§3 block path and shard naming are byte-correct.** `block_path`
  (`crates/longtail-store/src/sync.rs:62-66`) yields `chunks/<top-4-hex>/0x<16-lowercase-hex>.lsb`
  — `[2..6]` of `0x{h:016x}` is exactly the top four nibbles — and the shard key is
  `store_{sha256:x}.lsi` over the exact serialized bytes (`sync.rs:106-109`), matching
  `format-spec.md:266` and `:198-201`. The requested-hash echo check at `remote.rs:362` implements
  §3's path-derivation integrity rule.
- **`validate_store`'s error precedence matches C** (`EINVAL` over `ENOENT`,
  `validate.rs:64-71` versus `format-spec.md`/`longtail.c:9487-9500`), and the ordering is tested
  (`build_diff.rs:213-256`).

## On-disk upgrade / rollback — my half (format version fields)

R3 and R8 own the store-layout and CI halves; this is the codec half.

- **What we write.** `.lvi` always `0x0000_0002` (`version_index.rs:11`, written unconditionally at
  `:232`); `.lsi` always `0x0100_0000` (`store_index.rs:14`, `:143`). Both equal the pinned C
  constants (`Longtail_CurrentVersionIndexVersion` / `…StoreIndexVersion`,
  `format-spec.md:138-139`) and every committed fixture. `.lsb` has **no** version field.
- **Forward (golongtail reads our output).** Yes, by construction: identical version words plus
  identical layout, gated per-PR by `fixtures/` round-trip, the two byte gates
  (`crates/longtail/tests/{lvi_byte_gate,upsync_byte_gate}.rs`), and the golden suites; gated
  weekly by the differential lane and the minio interop job.
- **Backward (we read golongtail's output).** Yes, same reasoning. No legacy read path exists:
  `.lvi` v0.0.1 is rejected, exactly as C's reader rejects it (`format-spec.md:142-147`).
- **Rollback.** Safe in both directions for the current constants — a store written by either
  implementation is readable by the other, and there is no on-write upgrade step that would make a
  store un-readable by an older binary.
- **The failure mode on a future upstream bump.** `.lvi`/`.lsi` fail **loud and typed**
  (`UnsupportedVersion { found, expected }`) — the correct behaviour. Two gaps worth recording:
  (a) no test asserts the CLI's exit code or message for that case, so "loud" is untested at the
  process boundary; (b) `.lsb` has no version field at all, so a future block-header change is
  **undetectable** — the truncation check is the only signal and a sufficiently unlucky layout
  would parse as a valid block with wrong chunk counts (FMT-DOC-09). Adding the Σ`chunk_sizes`
  check from FMT-002 also buys a weak integrity signal here, which is a second reason to do it.

## Format-coverage matrix — spec field × exercising fixture

Every `.lvi`/`.lsi` header below was read directly with `od`; `13-fixtures.txt` confirms the
manifest verifies.

| Spec field / rule | Exercised by | Gap |
|---|---|---|
| `.lvi` version, hash id, target chunk size | all 16 `.lvi` (versions all `2`; hash ids blake3 `0x626c6b33`, blake2 `0x626c6b32`, meow `0x6d656f77`) | — |
| `A` (asset count) | 2 … 1137 (`get-configs` … `default/zoo`) | **`A = 0` absent** (FMT-008) |
| `C`, `ACI`, `ACI > C` | `C` 2…5712; `ACI` 2…9447; `ACI > C` in every zoo cell (e.g. 1269/1270) | `ACI == C` covered (chain cells); `C = 0` absent |
| odd `ACI` (the real misalignment case) | `chunk-1024` (9447), `chain-v1` (7), `chain-v3` (9), `default/zoo` (2061), `sharded` (9) | — (but §9 cites the wrong count: FMT-DOC-05) |
| `m_ChunkTags` non-zero | every `comp-*` cell | tag `0` covered by `comp-none` |
| `m_Permissions` low 9 bits | `perms/mode-{644,755,444}` in the zoo | **bits 9–15 set: no fixture** — proptest only (`roundtrip_proptest.rs:46`) |
| directory-as-asset (`size 0`, trailing `/`, `chunk_count 0`) | `empty-dir`, `deep/`, all chain cells | — |
| zero-chunk asset | `empty-file` (zoo), `roundtrip_proptest.rs:237` | — |
| long / UTF-8 asset names | `names/<255-char>.txt`, `names/héllo-wörld-日本語.txt` | **non-UTF-8 name: no fixture** (FMT-005) |
| asset size > 4 GiB | none — largest corpus file is 9 MiB (`multi-block`, `corpus.rs:204`) | **`asset_sizes` high 32 bits are always zero in every fixture**; proptest covers the round-trip only |
| `.lsi` version / `B` / `C` | all 29 `.lsi`; `B` 1…6, `C` 2…5712 | **`B = 0` absent** |
| per-block chunk grouping, cumulative offsets | every `.lsi` | wild offset/count: unit tests only |
| sha256 shard naming + merge-on-read | `stores/sharded/` (2 shards, `B=1 C=2` and `B=1 C=7`) | **no two shards with identical content** (identical bytes collide to one name by design — idempotence untested by fixture) |
| `.lsb` header, chunk arrays, always-4-aligned `u64` | 32 `.lsb` across the comp/hash cells | — |
| `.lsb` payload: tag 0 raw / tag ≠ 0 framed | `comp-none` vs `comp-{lz4,zstd_min,zstd_max,brotli,brotli_text}` | **payload length ≠ Σ chunk_sizes: no fixture, no runtime check** (FMT-002) |
| chunker boundary grid | `boundaries/*.{streaming,buffer}.json` at 5 target sizes | good — the strongest area |

Coverage cross-check for my slice (`15-coverage/summary.txt`, verbatim): `block.rs` 92.86% region /
100.00% line; `cursor.rs` 93.21% / 95.45%; `file_infos.rs` 94.78% / 91.43%; `perms.rs` 74.00% /
61.29% (4 of 8 functions never executed); `store_index.rs` 94.06% / 95.64%; `validate.rs` 100.00% /
100.00%; `version_index.rs` 87.54% / 100.00%. `error.rs` and `lib.rs` have no rows (declaration-only).
Every zero-hit line in these files is accounted for above — either as a finding or as a
lower-priority observation. `validate.rs` at 100% while harbouring FMT-001 is the sharpest lesson in
this slice: line coverage measures which lines ran, not which inputs were tried.

## Experiments requested

| # | Hypothesis | Exact command | What would change the finding |
|---|---|---|---|
| 1 | FMT-001 is a real panic, not a mis-read | Add `crates/longtail-core/tests/fmt001_repro.rs` building a `VersionIndex` with `asset_chunk_index_starts = vec![u32::MAX]`, `asset_chunk_counts = vec![1]`, serializing, re-parsing, then calling `validate_store(&StoreIndex::empty(0x626c6b33), &parsed)`; run `cargo test -p longtail-core --test fmt001_repro -- --nocapture` | A clean `Err` instead of a panic would demote FMT-001 to P3 (documentation only). A panic confirms P1. |
| 2 | golongtail can produce an empty index, and its bytes match our writer | `mkdir -p /tmp/empty && cargo run -p xtask -- fetch-golongtail && ./target/golongtail/longtail upsync --source-path /tmp/empty --target-path /tmp/e.lvi --storage-uri /tmp/estore` then `od -An -tu4 -N24 /tmp/e.lvi` | A 24-byte header with `A=C=ACI=0` confirms §9 and gives FMT-008 its fixture. A refusal or a different shape is itself the answer to record. |
| 3 | The `checked_*` guards are load-bearing only on 32-bit | `rustup target add i686-unknown-linux-gnu && cargo test -p longtail-core --target i686-unknown-linux-gnu` | Any failure (most likely `validate.rs:56`'s `start + k` overflow in debug) upgrades FMT-014 from documentation to a real portability bug. All-pass keeps it P3. |
| 4 | Fuzzing finds FMT-001/FMT-002 in under a minute each | `cargo fuzz init` (then exclude `fuzz/` from the workspace), add the five targets from FMT-007, seed corpora from `fixtures/`, and run `cargo +nightly fuzz run vi_walk fuzz/corpus/vi_walk -- -max_total_time=60` and `… lsb_parse … -max_total_time=60` | A crash in `vi_walk` within 60 s is the strongest possible evidence for FMT-001 and sets the CI budget. No crash in 10 minutes would mean my reachability argument is wrong somewhere. |
| 5 | A non-UTF-8 asset name survives golongtail and breaks the port | `printf 'x' > $(printf '/tmp/nu/\xff.txt')` (after `mkdir /tmp/nu`), upsync with the pinned golongtail, then `cargo run -p longtail-cli -- downsync --source-path … --target-path /tmp/out …` | A successful golongtail upsync plus a Rust `InvalidUtf8` failure confirms FMT-005 as a compat break and argues for P1. A golongtail refusal makes it a documentation-only item. |
| 6 | FMT-002's panic is reachable end to end | Craft a `.lsb` in a scratch fs store whose payload is one byte short of Σ`chunk_sizes`, then `cargo run -p longtail-cli -- downsync` against it | A "task panicked" error confirms the chain through `apply.rs:342`. A clean typed error would mean a check exists somewhere I did not find, and FMT-002 drops to P3. |

## Open questions for the maintainer

1. **Is a strict parse acceptable?** FMT-001's cheap fix rejects a `.lvi` C would accept (and then
   read out of bounds). The alternative — checked accessors — spreads the change across six files
   in three crates. Which way do you want the trust boundary drawn?
2. **Do your stores contain non-UTF-8 asset paths?** FMT-005's severity is entirely a data
   question. If the answer is "never, the pipeline enforces ASCII", it becomes a one-line
   documentation fix.
3. **Is 32-bit a target you care about?** (FMT-014.) If not, saying so in `docs/rust-port.md` is
   worth more than a CI lane.
4. **Should `prune`'s skip-versus-error policy be settled crate-wide?** Five `StoreIndex` methods
   currently do three different things with a wild block (error / skip / `None`). A single stated
   policy would be easier to defend than five local decisions — and one of them deletes data
   (FMT-003).
5. **Who owns `Permissions` going forward?** Four of its eight functions have zero call sites and
   zero coverage; `POSIX_MASK` is unused while the one place that needs it re-declares the value.
   Trim the type, or wire it up?

## Files read

**In full:**
`/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-core/src/cursor.rs`,
`/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-core/src/version_index.rs`,
`/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-core/src/store_index.rs`,
`/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-core/src/block.rs`,
`/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-core/src/perms.rs`,
`/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-core/src/file_infos.rs`,
`/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-core/src/error.rs`,
`/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-core/src/validate.rs`,
`/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-core/src/lib.rs`,
`/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-core/tests/malformed.rs`,
`/home/chris/work/longtail-rs/cm/rust-port/crates/longtail-core/tests/roundtrip_proptest.rs`,
`/home/chris/work/longtail-rs/cm/rust-port/docs/format-spec.md`,
`/home/chris/work/longtail-rs/cm/rust-port/docs/rust-port.md`,
`/home/chris/work/longtail-rs/cm/rust-port/fixtures/README.md`.

**In part (secondary axis — reachability and §2/§3/§7 conformance only):**
`crates/longtail-core/tests/store_algebra.rs`, `crates/longtail-core/tests/merge.rs` (test
inventory), `crates/longtail-core/src/compress.rs` (`decode_block_payload`),
`crates/longtail/src/apply.rs` (`asset_path`, the asset-chunk loop, `write_block_chunks`),
`crates/longtail/src/prune.rs` (`keep_for_version`, `prune_store`, `prune_store_index`),
`crates/longtail/src/fs_util.rs` (`PERM_MASK`, `set_permissions`),
`crates/longtail-store/src/sync.rs` (module doc, `block_path`, `shard_key`),
`crates/longtail-store/src/blob/fs.rs` (`atomic_write`),
`crates/longtail-store/src/remote.rs:362`,
`crates/longtail-cli/src/main.rs:330-365`,
`support/longtail-testkit/src/corpus.rs` (`generate_zoo`),
`.github/workflows/fixture-freshness.yaml`,
`fixtures/manifest.json` (generator block), and the binary headers of all 16 `.lvi`, 29 `.lsi`, and
one `.lsb` under `fixtures/`.

**Evidence pack:** `MANIFEST.md`, `12-loc.txt`, `13-fixtures.txt`, `02-clippy-pedantic.txt`,
`15-coverage/summary.txt`, `15-coverage/lcov.info`, `03-test.txt`, `09-miri.txt`.
