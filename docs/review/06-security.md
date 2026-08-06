# 06 · Security & hardening review

- **Reviewed at:** `456274d` · **Lead model:** opus · **Workers:** 3 × fable
- **Slice:** the trust boundary — everything a `.lvi`/`.lsi`/`.lsb`/S3 key/CLI flag can make the port
  do · **Confidence:** covered well on the write path and the codec/transport boundary; covered
  thinly on Windows-specific path semantics (no Windows host available, and the per-PR Windows lane
  runs 3 of 8 gates per ALG-01)

## Findings index

| ID | P | Dimension | One line | file:line | Verdict |
|---|---|---|---|---|---|
| SEC-01 | P0 | security | A `.lvi` asset path is `join`ed to the target root with **zero** validation; an absolute or `../` name escapes `--target-path` and gets created, truncated, written, chmod'd — and read, on the upsync side | `fs_util.rs:104,110,116,133,150,191,240` ← `version_index.rs:110` | CONFIRMED |
| SEC-02 | P0 | security | `downsync` caches the *source* `.lvi` inside the target dir and reads it back as the *current* index next run; deletes-first then removes the escaped paths → arbitrary file **deletion** on run 2 | `downsync.rs:219,113` → `apply.rs:89,439` → `fs_util.rs:240` | CONFIRMED |
| SEC-03 | P1 | security | The block hash covers only the chunk-hash array, so the `block_hash` check at `remote.rs:362` cannot detect substituted `chunk_sizes`, `tag`, or payload; nothing ever re-hashes chunk bytes | `pack.rs:24-30`, `remote.rs:362`, `apply.rs:215-232` | CONFIRMED |
| SEC-04 | P1 | memory | Blob reads are unbounded at the transport layer, so the 512 MiB prefetch budget (sized from *declared* chunk sizes) bounds nothing an attacker controls | `blob/fs.rs:323-325`, `blob/s3.rs:378-395`, `remote.rs:519` | CONFIRMED |
| SEC-05 | P1 | hardening | `cp` sizes a `Vec` from an untrusted `u32` asset chunk count → 34 GiB `with_capacity` → **abort**; and `cp` and `apply` each bounds-check exactly the walk the other leaves bare | `cp.rs:82-83`; `cp.rs:147-152` vs `apply.rs:342` | CONFIRMED |
| SEC-06 | P1 | hardening | Panic-as-DoS is asymmetric and undocumented: the same malformed input is an `Err` on the `spawn_blocking` writer and a **process abort** on the rayon codec pool | `apply.rs:221-230,353-363` vs `store/compress.rs:38-48` | CONFIRMED |
| SEC-07 | P2 | security | `get` treats a get-config's `storage-uri`/`source-path` as trusted URIs and *silently ignores* the `s3-endpoint-resolver-uri` key `put` writes — an undocumented, security-positive-but-breaking divergence | `get.rs:43-83`, `put.rs:168-174` | CONFIRMED (Rust side) / PLAUSIBLE (divergence) |
| SEC-08 | P2 | security | `--s3-endpoint-resolver-uri` redirects every signed S3 request, accepts plain `http://`, and logs nothing | `main.rs:620-621`, `blob/s3.rs:209-224` | CONFIRMED |
| SEC-09 | P2 | hardening | `--no-stalled-stream-protection` removes the **only** stall guard; there is no operation, read, or connect timeout anywhere in the production tree | `main.rs:624-625`, `blob/s3.rs:234-239` | CONFIRMED |
| SEC-10 | P2 | security | The apply/upsync side never `lstat`s: a pre-existing symlink under `--target-path` is silently followed by create/truncate/chmod/remove; only the *scan* side is symlink-aware | `fs_util.rs:78-84` vs `:134,151,195,250,263` | CONFIRMED |
| SEC-11 | P2 | hardening | No `deny.toml`; `licenses FAILED`; all 9 workspace packages have no `license` field and the repo has no `LICENSE` file | `16-deny.txt:11214`, `Cargo.toml:11-13` | CONFIRMED |
| SEC-12 | P2 | hardening | The audit workflow cannot gate a PR (no `pull_request` trigger) and its self-path filter names `audit.yml` while the file is `audit.yaml` | `.github/workflows/audit.yaml:2-16` | CONFIRMED |
| SEC-13 | P3 | hardening | `longtail-sys/build.rs` skips SHA256 verification whenever the zip is already on disk | `support/longtail-sys/build.rs:88-91` | CONFIRMED |
| SEC-14 | P3 | hardening | Vendor-default minio credentials are inline in a workflow (locate + advise below; values not reproduced) | `.github/workflows/s3-minio.yaml` (4 blocks) | CONFIRMED |
| SEC-15 | P3 | hardening | Cache eviction deletes *every* file under `<cache>/chunks/` regardless of extension, with no ownership marker on the directory | `cache.rs:262-283,247` | CONFIRMED |

Appendix A classifies all 19 production `unwrap`/`expect` sites (the mission's "12" undercounts —
see the appendix for the exact command and the delta). Doc findings are `SEC-DOC-01…04`.

## Scope

**Read in full:** `crates/longtail/src/{fs_util,apply,downsync,get,put,cp,clonestore,path_filter}.rs`;
`crates/longtail-core/src/{version_index,cursor,block,compress,pack}.rs` and the parse half of
`store_index.rs`; `crates/longtail-store/src/{uri,compress,cache,blob/fs,blob/s3}.rs`;
`.github/workflows/{audit,s3-minio}.yaml`; `support/longtail-sys/build.rs`.

**Skimmed (secondary axis only):** `crates/longtail-store/src/{remote,sync}.rs` (untrusted-input
reachability of the fetch/prune paths — R3 owns them, cross-referenced not re-filed);
`crates/longtail/src/{upsync,inspect,prune,version}.rs` (index-walk bounds only — R7 owns the CLI
surface); `crates/longtail-core/tests/{malformed,codec_malformed}.rs` (to establish what the
hardening backlog does *not* already cover); `support/longtail-bench/src/bin/e2e.rs:78-120` (the
`unsafe` inventory deliverable).

**Excluded:** the chunker, hash, and diff algorithms (R2); the store actor's concurrency (R3); CLI
argument surface vs golongtail (R7); benches and fixtures except as evidence.

## Verification performed

Evidence-pack artifacts consulted: `MANIFEST.md`, `06-audit.json` / `06b-audit.txt`, `16-deny.txt`,
`05-tree.txt`, `07b-machete.txt`, `14-golongtail-help.txt`, `17-bloat.txt`. Per the contract I ran no
`cargo`; every count below is either from the pack or from a read-only `rg` whose exact command is
quoted.

Independently verified worker claims, including two I had to **correct**:

- Worker (b) and my own reading agreed there is no traversal guard: `rg 'is_absolute|Component::|canonicalize|\.components\(\)'` over `crates/*/src` returns **zero** hits. Confirmed by hand.
- Worker (b) reported the fs-blob listing → `prefix.join(name)` → `remove_file` chain as "unguarded". **Corrected:** `FsBlobClient::get_objects` derives names via `path.strip_prefix(root)` over a real directory walk (`blob/fs.rs:128`), so a listing name can never contain `..`; and the S3 listing path never touches a local root. Filed under "Verified good", not as a finding.
- I initially assumed the remote fetch does not check the returned block's self-declared hash. **Corrected by reading `remote.rs:362`** — it does. SEC-03 is re-scoped accordingly and is stronger for it: the check exists but the hash construction cannot support what it appears to promise.
- My first draft of SEC-05 claimed the unguarded `.lsi` walks in `cp.rs:118-127` / `inspect.rs:287-291` were reachable. **Worker (a) disproved it and I verified the disproof** by reading `store_index.rs:570-660`: `get_existing_content` terminates in `StoreIndex::from_block_indexes` (`:658`), so every consumer downstream of it receives a canonical index. SEC-05 was rewritten around the asymmetry that *is* real (`cp.rs:147-152` guards the payload slice that `apply.rs:342` does not).
- Worker (a) also found the reachability path R1's FMT-001 does not cite: `diff.rs:129-133`, called from `downsync.rs:169` **before the store is contacted**. Verified by reading both. Recorded in Appendix A.2 as an extension of FMT-001, not as a new finding.
- Worker (c)'s "zero advisories" is confirmed verbatim in `06-audit.json:3` (`"found":false,"count":0`) and `16-deny.txt:28388` (`advisories ok`).
- Worker (c)'s crypto-stack claim verified: `grep -c 'ring v' 05-tree.txt` → **0**; `aws-lc-rs v1.17.3` appears at `05-tree.txt:646,653` under `rustls` ← `aws-smithy-http-client`. `ring` reaches the graph only via `reqwest` ← `longtail-sys` (build-dep, non-default member). **The shipped default build carries one crypto provider, not two** — this corrects the mission's premise.

**Could not verify:** anything needing execution. SEC-01's escape, SEC-05's abort, and SEC-06's
asymmetry are traced by reading; each has an experiment request with an exact command. Windows
semantics (drive-letter and UNC `join` replacement, device names) are reasoned from `std`'s
documented `Path::join` contract, not observed — see Experiments 1 and 3. R7's OPS-09 covers the
Windows *naming* half and is PLAUSIBLE for the same reason.

## Findings

### `SEC-01` — a `.lvi` asset path is joined to the target root with no validation of any kind

**P0** · `security` · CONFIRMED · `COMPAT-RISK`

- **Where:** `crates/longtail/src/fs_util.rs:104, 110, 116, 133, 150, 191, 240` — seven
  `root.join(rel_path)` calls. Source of `rel_path`: `crates/longtail-core/src/version_index.rs:110`.

- **What:** the untrusted input is the `.lvi` `m_NameData` blob and `m_NameOffsets`
  (`version_index.rs:57-63`), kept verbatim from the file. `VersionIndex::path` (`:110-114`) applies
  exactly two checks — the offset is in range and the bytes are UTF-8 — and returns an arbitrary
  string. `apply.rs:57` strips a trailing `/` and nothing else. Then:

  | effect | chain |
  |---|---|
  | mkdir | `apply.rs:137` → `fs_util.rs:110-111` `create_dir_all(root.join(rel))` |
  | create + **truncate** | `apply.rs:139,155` → `fs_util.rs:132-139` `OpenOptions::write().create(true).truncate(true)` |
  | write attacker bytes | `apply.rs:328,342` → `fs_util.rs:150,158` `open_for_write` + `write_all_at` |
  | chmod | `apply.rs:270,278` → `fs_util.rs:191-199` |
  | **delete** | `apply.rs:441` → `fs_util.rs:240,250,263` (see SEC-02 for reachability) |
  | **read + upload** | `upsync.rs:266-268` → `fs_util.rs:104` `File::open(root.join(rel))` |

  `Path::join` is documented to *replace* the receiver when the argument is absolute
  ("If `path` is absolute, it replaces the current path"). `..` components are not normalized by
  `join` either — they are resolved by the kernel at syscall time against the real directory.
  `rg` over `crates/longtail-core/src crates/longtail/src crates/longtail-store/src
  crates/longtail-cli/src` for `is_absolute|Component::|canonicalize|\.components\(\)|ParentDir`
  returns **zero hits**. The `RegexPathFilter` is applied only on the *scan* side (`fs_util.rs:85`);
  `change_version2` takes no filter, so every asset in the `.lvi` is materialized unfiltered.

- **Failure scenario:** the Tauri app runs `downsync` against a store whose `.lvi` an attacker can
  write (a compromised S3 bucket, a bucket with an over-broad write policy, a MITM on a
  non-TLS-pinned endpoint, or a CI job that publishes to a shared store). The `.lvi` contains one
  extra asset named `../.bashrc` (or, on the CI runner, `/home/runner/.ssh/authorized_keys`; or
  absolute `C:\Users\x\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\x.cmd`).
  `create_file_sized` calls `ensure_parent` → `create_dir_all` on the escaped parent, then opens the
  file `create(true).truncate(true)`, `write_at`s the attacker's chunk bytes, and step 7 chmods it to
  the `.lvi`'s declared mode. **Arbitrary file write followed by arbitrary mode set = code execution
  as the desktop user or the CI runner.** The mirror case: `upsync --source-index-path <hostile
  .lvi>` names `../../.aws/credentials`, `open_asset` reads it, and the content is packed into a
  block and **uploaded to the attacker's store**.

  Severity bound (verified independently of R1): `fs_util.rs:197` masks the mode with
  `PERM_MASK = 0x1FF` = `0o777` before `from_mode`, and `Permissions` is a `u16` that does carry
  `0o4000`/`0o2000`/`0o1000` off the wire (`version_index.rs:59-60`). So setuid/setgid/sticky
  **cannot** reach `chmod`. R1's report is correct. This bounds the attack to "write + make
  executable as the invoking user", not "drop a setuid root binary".

- **Evidence:** the seven join sites read in full; `version_index.rs:88-114`; `apply.rs:55-58,
  98-158, 266-280, 411-464`; `upsync.rs:263-270`. Worker (b)'s exhaustive sweep of every
  path-construction site in the four production crates found no other consumer of `.lvi` names and
  no guard anywhere; I re-ran its greps. **No test in the repo exercises a traversal path** —
  `rg '\.\./|absolute|traversal' crates/*/tests/*.rs support/longtail-testkit/tests/*.rs` finds only
  fixture-path constructions.

- **Recommendation — the adjudication R1 and R7 deferred to me. The guard belongs at the
  filesystem boundary, in `fs_util.rs`, not at parse time.** Three reasons:

  1. **Parse-time rejection breaks byte-compat.** `VersionIndex` is documented "round-trip fidelity
     beats normalization" (`version_index.rs:18-24`) and `to_bytes` must reproduce a wild file
     byte-for-byte (gate ①, `crates/longtail/tests/lvi_byte_gate.rs`). Rejecting a path in
     `from_bytes` also breaks `ls`, `print-version`, `validate-version`, and `prune-store`, none of
     which touch the filesystem — a hostile `.lvi` should still be *inspectable*.
  2. **The dangerous operation is the syscall, not the parse.** Six of the seven sinks are in one
     150-line file with one shape. A guard there is provably complete by construction; a guard in
     the codec has to be re-proved for every future consumer.
  3. **It is the only place that knows the root.** Containment is a two-argument property.

  Concretely: add one private function to `fs_util.rs` and route all seven joins through it.

  ```rust
  /// Join `rel_path` under `root`, rejecting anything that could escape.
  /// `rel_path` is untrusted (`.lvi` name blob); `root` is operator-supplied.
  fn safe_join(root: &Path, rel_path: &str) -> Result<PathBuf, LongtailError> {
      let p = Path::new(rel_path);
      if p.is_absolute() || p.has_root() {                       // "/x", "C:\x", "\\?\", "\\srv\s"
          return Err(LongtailError::UnsafeAssetPath { path: rel_path.into() });
      }
      for c in p.components() {
          match c {
              Component::Normal(_) | Component::CurDir => {}
              Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                  return Err(LongtailError::UnsafeAssetPath { path: rel_path.into() });
              }
          }
      }
      Ok(root.join(p))
  }
  ```

  Notes that matter: (a) iterate `Component`s rather than string-matching `".."`, so `a/../../b` and
  `..\\b` are both caught, and on Windows `Component::Prefix` catches `C:`, `\\?\`, and UNC in one
  arm; (b) `has_root()` catches a leading `\` on Windows, which `is_absolute()` alone does not;
  (c) reject, do not sanitize — silently rewriting `../x` to `x` would produce a tree that differs
  from what C produces, which is a compat break in the other direction; (d) **this must also gate the
  delete path** (`remove_asset`, `fs_util.rs:240`) — see SEC-02; (e) R7's **OPS-09** (Windows device
  names, `:` ADS, trailing dot-space) is the *same guard's* second clause — I adjudicate it into
  `safe_join` as an additional `Component::Normal` inspection rather than a separate mechanism, so
  there is exactly one place that decides what a materializable asset name is.

  Also: state the trust boundary in `docs/rust-port.md` (see SEC-DOC-02). A guard nobody documents
  gets removed by the next optimization pass.

- **Tradeoff / risk:** `COMPAT-RISK` — this changes which `.lvi` files are *materializable*.
  Legitimate longtail versions cannot contain absolute or `..` paths (they are produced by
  `scan_folder`, `fs_util.rs:73-76`, which builds names from real directory entries under the root),
  so no real store is affected. **The gate that would catch a mistake here is
  `crates/longtail/tests/downsync_e2e.rs` plus the fixture corpus (`13-fixtures.txt` passes today);
  the three-way differential (`downsync_three_way.rs`) is weekly, not per-PR.** No existing test
  covers the rejection direction — that is the regression test below.
- **Effort:** S for the guard; M including the tests and the doc.
- **Regression test to add:** a unit test in `fs_util.rs` over the table `["/etc/passwd", "../x",
  "a/../../x", "C:\\x", "\\\\?\\C:\\x", "\\\\srv\\share\\x", "a/./b"]` asserting the first six are
  `Err` and the last is `Ok(root/a/b)`; plus an integration test in `crates/longtail/tests/` that
  builds a `.lvi` naming `../escaped.txt`, runs `downsync` into a `tempdir` subdirectory, and asserts
  (i) the call errors and (ii) `tempdir/escaped.txt` does not exist. Then R1's `vi_walk` fuzz target
  gains a real invariant to assert (see the fuzz review below).

---

### `SEC-02` — the cached target index turns SEC-01's write primitive into a delete primitive

**P0** · `security` · CONFIRMED

- **Where:** `crates/longtail/src/downsync.rs:219` (write), `:70,74,113` (read back),
  `crates/longtail/src/apply.rs:89` (deletes run first), `:439`, `crates/longtail/src/fs_util.rs:240,250,263`.
- **What:** with `--cache-target-index` (on by default per `14-golongtail-help.txt:226`), a
  successful downsync writes **the source version index** into the target folder as
  `.longtail.index.cache.lvi` (`downsync.rs:219` — note it caches `source_version`, not a rescan).
  On the next run that file is read back as `target_index` (`:74,113-114`), i.e. as `current` in
  `change_version2`. `create_version_diff(&target_index, &source_version)` then puts every asset
  present in the cached index but absent from the new source into
  `diff.source_removed_asset_indexes`, and `delete_assets` (`apply.rs:411-464`, called at `:89`
  **before any write**) resolves each through `current.path(ai)` → `remove_asset` →
  `root.join(rel)` → `ensure_user_writable` → `fs::remove_file`.
- **Failure scenario:** run 1 serves a `.lvi` containing `../../../home/user/Documents/x.docx`
  (SEC-01 writes it, and caches the index). Run 2 serves a `.lvi` **without** that entry. The diff
  marks it removed; `delete_assets` chmods it `+w` and unlinks it — at an arbitrary absolute path,
  before a single byte is fetched, and the operation then continues and reports success. This is the
  arm the mission asked me to trace: **yes, a delete can land outside `--target-path`, and it does
  so on the deletes-first phase.** Note the attacker does not even need run 1 to succeed at writing
  anything: the cache index is written from `source_version.to_bytes()` unconditionally at `:219`,
  so the only requirement is that run 1 completes.
- **Evidence:** `downsync.rs:60-78, 174-176, 217-220`; `apply.rs:79-89, 411-464`; `fs_util.rs:235-269`.
  `remove_asset` guards only *type* (`is_file`/`is_dir`), never containment. The dir arm calls
  `fs::remove_dir` (empty-only), which bounds that half; the file arm is unbounded.
- **Recommendation:** the `safe_join` from SEC-01 must gate `remove_asset` too — that is the single
  change that closes this. As defence in depth, `downsync` should refuse to *accept* a cache index
  it would not itself have written: after reading `.longtail.index.cache.lvi`, reject the whole file
  if any asset path fails `safe_join` (rather than skipping entries, which would silently under-delete).
  Cross-reference OPS-01 (resume-after-cancel rests on this same cache file) and OPS-11 (it is
  written non-atomically) — the three findings share a file and should be fixed together.
- **Tradeoff / risk:** none for real stores; a legitimate cached index always round-trips a scan.
  No existing test covers it — `smoke.rs:42` disables `cache_target_index` (per OPS-01).
- **Effort:** S (rides SEC-01's guard).
- **Regression test to add:** two-run integration test — downsync a `.lvi` with an escaping asset,
  then downsync a `.lvi` without it, asserting the out-of-root file still exists and the run errors.

---

### `SEC-03` — the block hash cannot detect a substituted payload, and nothing re-hashes chunk bytes

**P1** · `security` · CONFIRMED

- **Where:** `crates/longtail-core/src/pack.rs:24-30` (what the block hash covers),
  `crates/longtail-store/src/remote.rs:362` (the check that exists),
  `crates/longtail/src/apply.rs:215-232` (the consumer that trusts it).
- **What:** `block_hash` is `HashBuffer` over the block's **chunk-hash array only** — `pack.rs:24-30`
  serializes `chunk_hashes` as LE `u64`s and hashes that. It does not cover `chunk_sizes`, `tag`, or
  one byte of the payload. `fetch_stored_block` does check `block.block_index.block_hash !=
  block_hash` (`remote.rs:362`) and rejects a mismatch, and `CacheBlockStore` repeats the check
  (`cache.rs:97`) — but both only prove that the object's *self-declared* hash equals the requested
  one. An attacker with write access to the store keeps `chunk_hashes` byte-identical (so both checks
  pass) and replaces `tag`, `chunk_sizes`, and the entire payload. Nothing downstream re-hashes the
  decoded chunk bytes against their chunk hashes — I grepped `remote.rs`, `cache.rs`,
  `store/compress.rs`, and `apply.rs` for any `hasher.hash`/`block_hash(` call on a fetched block and
  found none.
- **Failure scenario:** an attacker who can write one `.lsb` object substitutes the contents of a
  game asset or an executable in the shipped version. Every layer reports success. `--validate`
  (`downsync.rs:317-384`) does not help: it re-scans the target and compares per-asset content hashes
  against **the source `.lvi`'s** `content_hashes` — an attacker-supplied number — so it proves the
  download matches the attacker's declaration, not the publisher's. There is no publisher signature
  anywhere in the format.
- **Evidence:** `pack.rs:20-30` and its own doc ("the block's chunk-hash array … in packing order");
  `remote.rs:353-372`; `cache.rs:88-123`; `apply.rs:294-348`; `downsync.rs:344-382`. This is
  **inherited from C**, not a port defect — under the byte-compat mandate it is not fixable in the
  format. It is a *documentation and expectation* defect.
- **Recommendation:** (1) Write the trust boundary down (SEC-DOC-02): "a longtail store is trusted
  for content integrity; content-addressing here is a deduplication key, not an authentication tag."
  This directly contradicts the intuition a reader takes from `readme.md`'s "content-addressed"
  framing, and it is the premise that makes SEC-01 a P0 rather than a curiosity. (2) Offer an
  **opt-in** `--verify-chunks` on `downsync`/`get` that re-hashes each decoded chunk against its
  chunk hash inside `write_block_chunks` (the bytes are already in memory and the hasher is already
  resolved at `downsync.rs:108`); it is off by default so byte-compat and throughput are untouched.
  That converts a store compromise from "silent substitution" into a hard error for callers who want
  it — notably the Tauri app.
- **Tradeoff / risk:** the opt-in costs one hash pass over the payload (blake3 ≈ GB/s, so single-digit
  percent). Default-off means no `COMPAT-RISK`.
- **Effort:** S for the doc; M for the opt-in flag.
- **Regression test to add:** a store fixture whose `.lsb` payload is mutated while `chunk_hashes` is
  left intact; assert the plain download succeeds (documenting today's behaviour) and that
  `--verify-chunks` errors.

---

### `SEC-04` — blob reads are unbounded, so the prefetch budget bounds nothing an attacker controls

**P1** · `memory` · CONFIRMED

- **Where:** `crates/longtail-store/src/blob/fs.rs:323-325` (`read_to_end` into an unbounded `Vec`),
  `crates/longtail-store/src/blob/s3.rs:378-395` (`resp.body.collect()`, unbounded),
  `crates/longtail-store/src/remote.rs:519` (the budget estimate).
- **What:** this is the resource-exhaustion class the mission assigned me, and it sits *below*
  ALG-02. `preflight_get` sizes each prefetch permit from `size_by_hash` — the sum of the block's
  `chunk_sizes` **as declared in the store index** (`remote.rs:503,519`) — and clamps it to
  `DEFAULT_MAX_PREFETCH_BYTES` = 512 MiB (`remote.rs:70`). Two independent gaps:
  1. The declared size and the object's actual size are unrelated. A store index declaring a 4 KiB
     block whose `.lsb` object is 40 GiB acquires 4 KiB of budget and then reads 40 GiB into a `Vec`.
  2. The budget gates *background prefetch only* — the module doc says so explicitly
     (`remote.rs:17`) — so demand fetches are bounded only by the worker semaphore
     (`min(NumCPU, 8)` for s3, uncapped `NumCPU` for fs, `uri.rs:72-84`). N concurrent unbounded
     reads.
- **Failure scenario:** a hostile or corrupted store serves one oversized `.lsb`. On the Tauri
  desktop path the process RSS goes to the object size × in-flight workers and the app is OOM-killed
  with no error surface. No timeout limits how long the read takes either (SEC-09).
- **Evidence:** `blob/fs.rs:310-330`; `blob/s3.rs:359-396`; `remote.rs:494-532`; `uri.rs:114-125`
  (the budget's own doc calls it "test-oriented … it bounds memory held by unconsumed prefetches",
  which is precisely the scope limit). Cross-reference **ALG-02** for the *decompression* half — I
  am not re-filing it. Adjudicating ALG-02's PLAUSIBLE arms: **lz4 and zstd are bounded on output**
  (`lz4_flex::block::decompress` and `zstd::bulk::decompress` both take the capacity as a hard
  ceiling and error past it), so for those two the exposure is the 4 GiB `with_capacity` alone;
  **brotli is unbounded on output** (`compress.rs:239-244` — `BrotliDecompress` grows `out` as a
  `Write` sink with no ceiling, and the `decoded.len()` check at `:314` runs after). ALG-02's
  CONFIRMED/PLAUSIBLE split is exactly right; brotli is the one that needs the streaming-with-limit
  fix, the other two need only a declared-size cap.
- **Recommendation:** one cap, applied in three places. Add a `max_block_bytes` to `BlockStoreOpts`
  (default e.g. 256 MiB — two orders of magnitude above the 8 MiB default `target_block_size` at
  `upsync.rs`): (a) `BlobObject::read` grows a `Vec` with `take(limit + 1)` and errors past it;
  (b) `decode_block_payload` rejects `uncompressed_size > limit` **before** allocating; (c) brotli
  decompresses through a limiting `Write` adapter so the bomb is caught at the ceiling, not after.
- **Tradeoff / risk:** a store legitimately containing a block larger than the cap would stop
  working. Blocks are produced by `create_store_index` with the 10% overshoot rule
  (`pack.rs:79-80`), so a real block never exceeds `target_block_size * 1.1` plus one oversize chunk;
  a 256 MiB default is ~30× headroom. Make it configurable so the escape hatch exists.
- **Effort:** M
- **Regression test to add:** a `MemBlobStore` fixture serving an over-cap object; assert
  `StoreError`, not an allocation. Pair with the `lsb_decode` fuzz target proposed below under
  `-rss_limit_mb`.

---

### `SEC-05` — `cp` allocates from an untrusted `u32`, and the two block consumers guard opposite halves

**P1** · `hardening` · CONFIRMED

- **Where:** `crates/longtail/src/cp.rs:80-88`; `crates/longtail/src/cp.rs:145-153` vs
  `crates/longtail/src/apply.rs:342`.
- **What:** two defects, plus an adjudication of R1's FMT-002.

  **(a) Allocation from a raw count — CONFIRMED, and mine.** `cp.rs:81-83`:
  ```rust
  let count = vi.asset_chunk_counts[asset] as usize;
  let mut chunk_hashes: Vec<u64> = Vec::with_capacity(count);
  ```
  `asset_chunk_counts` is a `u32` array read verbatim from the `.lvi` (`version_index.rs:46,192`).
  `from_bytes` never checks it against `asset_chunk_index_count` — its only semantic checks are
  `ACI >= C` and the total-size compare (`version_index.rs:169,181`). So a **few-hundred-byte** `.lvi`
  with `asset_chunk_counts[0] = 0xFFFF_FFFF` requests `Vec::<u64>::with_capacity(4_294_967_295)` =
  34 GiB. Allocation failure in Rust runs the alloc error handler, which **aborts** — not a catchable
  panic, no error path, no message, before a single byte of network I/O.

  **(b) The guard asymmetry runs in *both* directions, and only one direction matters.**

  | walk | `apply.rs` | `cp.rs` | actually reachable? |
  |---|---|---|---|
  | `.lsi` block→chunk range | **guarded** (`:373-376`, `checked_add` + `e <= len`, skip) | bare (`:118-127`) | **No.** Both are fed the output of `get_existing_content`, which ends in `StoreIndex::from_block_indexes` (`store_index.rs:658`) and is therefore canonical by construction — offsets rebuilt cumulatively. `apply.rs`'s guard is dead defence; `cp.rs`'s absence is harmless. |
  | `.lsb` payload slice | **bare** (`:342`) | **guarded** (`:147-152`, `if e > payload.len() { return Err(..) }`) | **Yes** — this is R1's **FMT-002**. |

  So each file bounds-checks precisely the walk the other leaves bare, and the one that is reachable
  is the one `apply.rs` — the production download path — leaves open. **`cp.rs:147-152` is the
  ready-made patch for FMT-002**, four lines away in the same crate.

- **Failure scenario:** (a) `longtail cp --version-index-path s3://…/v.lvi …` against a hostile index
  aborts the process. (b) is FMT-002's, bounded by SEC-06 to an error rather than a crash.
- **Evidence:** `cp.rs:59-158` read in full; `version_index.rs:147-219`; `apply.rs:294-348, 368-405`;
  `store_index.rs:570-660` read in full to establish the canonicality proof — the `.lsi` half of my
  own first draft was wrong and worker (a) caught it.
- **Recommendation:** (a) size the `Vec` from the validated slice length, never from the header field
  — or, durably, R1's **FMT-001** (validate `starts[a] + counts[a] <= ACI` and
  `asset_chunk_indexes[j] < C` once at parse), which also closes the seven reachable `.lvi` walks in
  Appendix A.2. Because `VersionIndex`'s fields are `pub` and a hand-built struct bypasses
  `from_bytes`, the belt-and-braces version is one shared accessor,
  `fn asset_chunks(&self, a: usize) -> Result<impl Iterator<Item = (u64, u32)>, FormatError>`,
  replacing all seven copies of the loop. (b) Lift `cp.rs:147-152`'s check into `write_block_chunks`
  before `apply.rs:342`, and delete `apply.rs`'s dead `.lsi` guard or keep it and add the same to
  `cp.rs` — but say which, because right now the inconsistency reads as an oversight in whichever
  file you happen to open.
- **Tradeoff / risk:** none — a well-formed index is unaffected. `COMPAT-RISK` only if FMT-001's
  parse-time rejection lands too; the accessor and the payload check change nothing about which
  bytes are accepted, only when the error is raised.
- **Effort:** S
- **Regression test to add:** in `crates/longtail-core/tests/malformed.rs`, a `.lvi` with
  `asset_chunk_counts[0] = u32::MAX` and a small `ACI`; assert `Err` from the accessor and — once
  FMT-001 lands — from `from_bytes`. The abort itself needs a subprocess test
  (`assert_cmd` + expect signal), which is why it is Experiment 2.

---

### `SEC-06` — panic-as-DoS is asymmetric: the same input is an error on one pool and an abort on the other

**P1** · `hardening` · CONFIRMED

- **Where:** `crates/longtail/src/apply.rs:221-230` and `:350-363` (caught) vs
  `crates/longtail-store/src/compress.rs:38-48` (not caught).
- **What:** malformed-input panics do not have one failure mode.
  - Panics inside `write_block_chunks` — including R1's **FMT-002** slice at `apply.rs:342` — run
    under `tokio::task::spawn_blocking` (`apply.rs:221`). Tokio catches the unwind and returns a
    `JoinError`, which `flatten_apply_task` (`:353-363`) converts into a `LongtailError`. **This
    materially bounds FMT-002: it degrades to an error, not a crash.** The comment at `:350-352`
    already says so, and it is correct.
  - Panics inside the codec run on the **rayon** pool via `on_pool` (`store/compress.rs:38-48`).
    Rayon's default panic handler aborts the process; the `rx.await.expect("rayon codec task dropped
    its result")` at `:47` is therefore unreachable in practice — the abort wins first. R3's
    **STORE-12** covers the missing handler; my point is the *asymmetry* and that it is undocumented.
  - Panics on the caller's async task — `apply.rs:110` (FMT-001), `cp.rs:85`, `upsync.rs:226` — unwind
    through `downsync()` into the CLI's `block_on` and terminate the process; in the Tauri app they
    poison whatever task hosts the command.
- **Failure scenario:** two customers report "the app closes with no error". One is FMT-001 (unwind
  to the top), one is ALG-02's 4 GiB `with_capacity` on the codec pool (abort). They need different
  fixes and produce indistinguishable symptoms, and only the second is reachable by STORE-12's
  `panic_handler`.
- **Evidence:** `apply.rs:213-245, 350-363`; `store/compress.rs:37-48`; `compress.rs:291`
  ("Never panics on malformed input — every failure is a typed `CompressError`"), which is true of
  the *logic* and false of the *allocation* (ALG-02). No `[profile.release] panic = "abort"` exists
  in any `Cargo.toml` (grepped), so the unwinding cases really do unwind.
- **Recommendation:** (1) take STORE-12's `panic_handler` on the rayon pool
  (`crates/longtail/src/version.rs` `build_pool`) so a codec panic becomes an error on every path;
  (2) document the two failure modes in `docs/rust-port.md` next to the safety posture — "a
  malformed input reaches the user as an error on the write path and as a process abort on the codec
  path" is exactly the kind of claim a production-readiness statement needs to be true about;
  (3) once (1) lands, `store/compress.rs:47`'s `expect` becomes genuinely reachable and should
  return `StoreError::WorkerGone` instead.
- **Tradeoff / risk:** a `panic_handler` that swallows a panic can mask a real bug; log at `error!`
  with the payload so it is loud.
- **Effort:** S
- **Regression test to add:** a test that installs a deliberately-panicking `Compressor` and asserts
  `get_stored_block` returns `Err`, not a dead process (needs the subprocess harness of Experiment 2).

---

### `SEC-07` — the get-config is a trust boundary with no documented contract, and one key is silently dropped

**P2** · `security` · CONFIRMED (Rust behaviour) / PLAUSIBLE (the divergence claim)

- **Where:** `crates/longtail/src/get.rs:43-83`; `crates/longtail/src/put.rs:168-174`.
- **What:** `get` reads a JSON document from an arbitrary URI (`get.rs:34`, including `s3://`) and
  takes two fields from it as *URIs it will then dereference*: `storage-uri` (`:43-52`) becomes the
  block store, `source-path` (`:64-74`) becomes the `.lvi` to read, and
  `version-local-store-index-path` (`:77-83`) becomes a store-index override. There is no scheme
  allowlist, no relation required to the get-config's own location, and no logging of what was
  resolved. Separately, `put` writes an `s3-endpoint-resolver-uri` key into the config
  (`put.rs:168-174`) which `get` **never reads** — `get.rs:2-4` states "unknown keys ignored;
  required keys are `storage-uri` + `source-path` only."
- **Failure scenario:** two, in opposite directions. (a) A get-config the operator does not fully
  control (fetched from a shared bucket, or a CI artifact) redirects `storage-uri` at a different
  store; combined with SEC-01 that is the full write primitive from a single JSON file. (b) The
  functional break: a minio/S3-compatible user runs `put --s3-endpoint-resolver-uri http://minio:9000`,
  the endpoint lands in the config, and the Rust `get` ignores it and tries real AWS. golongtail has
  the flag on `get` too (`14-golongtail-help.txt:200-202`), and a key written by `put` that nothing
  reads would be pointless upstream — so the Go `get` almost certainly consumes it. **This is a
  divergence that is not in `docs/rust-port.md` §"Deliberate divergences".**
- **Evidence:** `get.rs` read in full; `put.rs:165-194`; `14-golongtail-help.txt:170-232`;
  `rg 'endpoint|get-config' docs/rust-port.md docs/format-spec.md readme.md` → one hit, in
  `readme.md:24`, describing what a get-config is. Cross-reference **OPS-07** (R7 found the same
  option stranding on the *CLI* side for 8 subcommands).
- **Recommendation:** keep the security-positive behaviour, but make it **loud and documented**
  rather than silent: (a) `tracing::warn!` when a get-config carries an `s3-endpoint-resolver-uri`
  that is being ignored, naming the `--s3-endpoint-resolver-uri` flag as the replacement — a silent
  drop is the worst of both worlds; (b) record it in §"Deliberate divergences" with the security
  reason (a config-supplied endpoint is remote-controlled request redirection; a flag-supplied one is
  operator-controlled — see SEC-08); (c) log the resolved `storage-uri` and `source-path` at `info!`
  so a CI log shows where the bytes came from.
- **Tradeoff / risk:** if the Go `get` really does honour the key, minio users' existing get-configs
  break — which is a compat regression, so the warn message matters more than the doc. Experiment 4
  settles it.
- **Effort:** S
- **Regression test to add:** a `commands_spec.rs` case asserting a get-config containing the key
  produces the warning and does not change the endpoint.

---

### `SEC-08` — `--s3-endpoint-resolver-uri` silently redirects every signed request and accepts plain HTTP

**P2** · `security` · CONFIRMED

- **Where:** `crates/longtail-cli/src/main.rs:620-621` (and the 20 other subcommand copies listed at
  `main.rs:89,143,190,202,215,225,278,304,314,326,348,368,384,386,428,440,452,462`) →
  `crates/longtail-store/src/blob/s3.rs:209-211, 222-224`.
- **What:** the flag maps straight to the SDK's `endpoint_url`. Every subsequent `get_object`,
  `put_object`, `list_objects_v2`, and `delete_object` goes to that host, signed with the ambient
  credentials. `http://` is accepted with no warning; nothing is logged about which endpoint was
  used.
- **Failure scenario:** the exfiltration direction is the sharp one — `upsync --storage-uri
  s3://bucket/... --s3-endpoint-resolver-uri http://attacker.example` uploads the entire source tree
  to the attacker, over cleartext, and reports success. The credential direction is bounded but not
  nil: SigV4 signs the `Host` header, so the `Authorization` value is not replayable against real S3,
  but the attacker still receives the access-key ID and the full request in the clear on `http://`.
  This is operator-supplied, not remote-supplied, so it is a footgun rather than a hole — but it is a
  footgun with 21 copies and no guard rail.
- **Evidence:** `main.rs:618-626` and the `S3Options` plumbing; `blob/s3.rs:195-241`. Note also R3's
  **STORE-13**: when a caller supplies a pre-built `Client`, `build_client` returns it at `:196-198`
  and the endpoint/path-style/accelerate overrides are discarded — so the flag silently does nothing
  for an embedder like the Tauri app. Two different silent behaviours from one option.
- **Recommendation:** `tracing::warn!` once at client construction when `endpoint_url` is set,
  echoing the host; escalate to a second warning when the scheme is not `https`. Cheap, and it makes
  a hijacked CI config visible in the job log. Do not block `http://` — minio-over-localhost is the
  intended use (`s3-minio.yaml:36`).
- **Tradeoff / risk:** none; warnings only.
- **Effort:** S
- **Regression test to add:** none needed beyond asserting the warning fires in `commands_spec.rs`.

---

### `SEC-09` — the stalled-stream opt-out removes the only stall guard, and there is no timeout anywhere

**P2** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-cli/src/main.rs:624-625`; `crates/longtail-store/src/blob/s3.rs:98-105, 234-239`.
- **What:** `rg 'timeout' crates/longtail-store/src crates/longtail/src crates/longtail-cli/src`
  returns **no production timeout** — every hit is a doc comment, an error-classification arm, or a
  `tokio::time::timeout` inside `#[cfg(test)]` code. The whole availability story rests on the SDK's
  `StalledStreamProtectionConfig` (`blob/s3.rs:234-239`), whose own doc at `:98-105` describes it as
  the mechanism that turns a stalled body into a `Network` error so the retry ladder can recover.
  `--no-stalled-stream-protection` turns it off.
- **Failure scenario:** a hostile or degraded endpoint accepts the `GET` and then drips one byte per
  minute. With the flag set, `resp.body.collect()` at `blob/s3.rs:380` never returns; the worker
  permit is held (`remote.rs:339`), and with `min(NumCPU,8)` such streams the download wedges
  permanently. The CLI has no watchdog; the Tauri app's progress bar simply stops. Cancellation does
  not help — it is polled between blocks (`apply.rs:189`, R3's STORE-14), and a block that never
  completes is never re-polled.
- **Evidence:** the grep above; `blob/s3.rs:359-396`; `remote.rs:333-351`; `apply.rs:186-260`.
  The default is correct (`S3Options::default()` sets it `true`, `blob/s3.rs:118`) and the code
  comment explaining *why* the historical FFI disabled it (smithy-rs#3485, fixed 2024) is exactly the
  kind of citation the conventions ask for.
- **Recommendation:** (a) document on the flag's help text that it removes the only stall detection
  and that no other timeout exists — right now the help string reads as a compatibility knob;
  (b) `tracing::warn!` when it is set; (c) consider an SDK-level `operation_attempt_timeout` as a
  floor so the opt-out degrades rather than disarms. (c) is optional; (a) and (b) are not.
- **Tradeoff / risk:** a timeout floor set too low breaks large-block transfers on slow links —
  which is why it should be a configured value, not a hardcoded one.
- **Effort:** S for (a)+(b), M for (c).
- **Regression test to add:** none practical without a fault-injecting endpoint; note it as an
  accepted gap.

---

### `SEC-10` — the write path never `lstat`s, so a pre-existing symlink under the target is followed

**P2** · `security` · CONFIRMED

- **Where:** guarded on the scan side at `crates/longtail/src/fs_util.rs:78-84`; unguarded on the
  write side at `:134, 151, 195, 204, 218, 245, 250, 259, 263`.
- **What:** `scan_dir` uses `fs::symlink_metadata` and skips anything that is neither a regular file
  nor a directory (`:78-84`), so upsync never follows a symlink out of the source tree — good. The
  apply side has no counterpart: `OpenOptions::open`, `fs::set_permissions`, `fs::metadata`,
  `path.is_file()`, `path.is_dir()`, `fs::remove_file` all follow symlinks, and `create_dir_all`
  happily traverses one. Worker (b)'s sweep confirms `symlink_metadata` at `fs_util.rs:78` is the
  **only** symlink-aware call in the entire production tree; there is no `read_link`, no
  `is_symlink`, no `O_NOFOLLOW`.
- **Failure scenario:** two. (a) **Pre-existing:** `--target-path` is a directory a user or a prior
  process populated, containing `data -> /etc`. A `.lvi` naming `data/hosts` writes through the link
  — and `safe_join` from SEC-01 does **not** catch this, because the path is component-wise
  innocent. (b) **TOCTOU:** `create_file_sized` (step 5b) opens and truncates every write-plan file
  *before* the concurrent loop (`apply.rs:144-158`), and `open_for_write` reopens it per block
  (`:328`). A local attacker who can write into the target directory replaces the file with a
  symlink between those two opens and the positional writes land elsewhere. This is a low-privilege
  local escalation, and the deletes-first ordering widens the window.
- **Evidence:** `fs_util.rs` read in full; `apply.rs:131-158, 294-348`; worker (b)'s call-site
  inventory, re-verified by `rg 'symlink|read_link|is_symlink|nofollow' crates/*/src`.
- **Recommendation:** on the apply path, `symlink_metadata` the target before create/chmod/remove and
  refuse when it is a symlink — that is one call in `create_file_sized`, `set_permissions`, and
  `remove_asset`, and it matches what `scan_dir` already does on the other side, so the two halves
  finally agree. The TOCTOU arm needs `O_NOFOLLOW` on the reopen (`open_for_write`) to close
  properly; on Windows the equivalent is `FILE_FLAG_OPEN_REPARSE_POINT`, which `std` does not expose
  — record that as an accepted platform gap rather than pretending it is covered.
- **Tradeoff / risk:** `COMPAT-RISK`, mild — C longtail follows symlinks here, so a store that
  legitimately relies on a symlinked target subdirectory would start failing. I judge that
  vanishingly unlikely for a game-asset sync, but it is a behaviour change and belongs behind the
  same release note as SEC-01. **No existing test covers it.**
- **Effort:** M
- **Regression test to add:** create `target/link -> /tmp/outside`, downsync a `.lvi` naming
  `link/x`, assert the write is refused and `/tmp/outside/x` does not exist. Unix-only — and note
  ALG-01: a `#![cfg(unix)]` test does not run in the Windows per-PR lane.

---

### `SEC-11` — no `deny.toml`, `licenses FAILED`, and the repository has no license at all

**P2** · `hardening` · CONFIRMED

- **Where:** `target/review-evidence/16-deny.txt:3` (no config found), `:11214, :11228, :11242,
  :11267, :11281, :11297, :11312, :11331, :26820` (all 9 workspace packages `error[unlicensed]`),
  `:28388` (`advisories ok, bans ok, licenses FAILED, sources ok`); `Cargo.toml:11-13`.
- **What:** `cargo deny` ran against its **default** config, whose license allowlist is empty, so all
  370 third-party crates are "not explicitly allowed" — that part is a configuration artifact, not a
  problem. The real signal is the 9 `error[unlicensed]` + 9 `warning[no-license-field]` entries: none
  of `longtail`, `longtail-core`, `longtail-store`, `longtail-cli`, `longtail-testkit`,
  `longtail-bench`, `longtail-sys`, `longtail-ffi`, `xtask` declares a `license`, `[workspace.package]`
  declares only `version` and `edition` (`Cargo.toml:11-13`), and `find` over the repo turns up no
  `LICENSE`, `COPYING`, `NOTICE`, or `SECURITY.md`. The only license file in the tree belongs to the
  vendored C submodule (`support/longtail-sys/longtail/LICENSE.txt`, MIT, © 2019 Dan Engelbrecht).
- **Failure scenario:** the switchover ships a binary and a library with no license grant. Any
  downstream consumer — including the Tauri app if it is a separate repo — has no permission to use
  the code, and `cargo publish` is impossible. Separately, `bans` currently passes, so nothing would
  stop a future PR from adding a GPL dependency.
- **Legal note:** **this is not legal advice.** The port is a reimplementation of an MIT-licensed C
  project and vendors that project as a submodule for the differential lane. Confirm the intended
  license with the repo owner and with outside counsel before any external distribution; I have
  deliberately not picked one.

- **Recommendation — the proposed `deny.toml` (named deliverable).** The license section depends on
  the question above being answered first; everything else can land today.

  ```toml
  # deny.toml — cargo-deny 0.20.x
  [graph]
  # Only the platforms we ship. Prunes UEFI/wasm-only duplicates (r-efi, some getrandom).
  targets = [
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
  ]
  all-features = false          # `s3` is default-on; `fastcdc` is bench-only.
  exclude = ["longtail-sys", "longtail-ffi"]   # legacy oracle, scheduled for deletion

  [advisories]
  version = 2
  yanked = "deny"
  ignore = []                   # keep empty; every entry needs a dated comment + owner

  [licenses]
  version = 2
  # BLOCKED ON: the repository's own license decision (see above). Until the 9
  # workspace packages carry a `license` field, `private.ignore` is what keeps
  # this section from failing on our own crates — it is a placeholder, not the fix.
  allow = [
    "MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause", "BSD-3-Clause", "ISC", "0BSD", "Zlib",
    "Unicode-3.0", "Unlicense", "CC0-1.0", "MIT-0", "BSL-1.0",
    "CDLA-Permissive-2.0", "bzip2-1.0.6",
  ]
  confidence-threshold = 0.93
  # LGPL-2.1-or-later appears only as an OR-alternative in
  # "MIT OR Apache-2.0 OR LGPL-2.1-or-later" (2 crates); the allowlist above
  # already satisfies those expressions, so no copyleft obligation is taken on.
  [licenses.private]
  ignore = true                 # REMOVE once the workspace declares a license.

  [bans]
  multiple-versions = "warn"    # see the duplicate policy below
  wildcards = "deny"            # no `version = "*"` deps
  deny = []
  skip = [
    # Known, benign, upstream-driven splits. Re-audit when the AWS SDK bumps.
    { crate = "block-buffer" }, { crate = "const-oid" }, { crate = "crypto-common" },
    { crate = "digest" },       { crate = "hmac" },      { crate = "sha1" },
    { crate = "sha2" },         { crate = "cpufeatures" },   # RustCrypto 0.10 vs 0.11
    { crate = "http" }, { crate = "http-body" },             # smithy 0.2 vs hyper 1.x
    { crate = "syn" },  { crate = "getrandom" }, { crate = "rand_core" },
    { crate = "hashbrown" }, { crate = "windows-sys" },
    { crate = "constant_time_eq" }, { crate = "shlex" }, { crate = "r-efi" },
  ]

  [sources]
  unknown-registry = "deny"
  unknown-git = "deny"
  allow-registry = ["https://github.com/rust-lang/crates.io-index"]
  allow-git = []                # nothing is a git dep; the C library is a
                                # submodule + a SHA256-pinned release download,
                                # neither of which cargo-deny sees (see SEC-13).
  ```

  **What gates a PR vs warns.** Gate (fail the build): `advisories` (a known CVE in the shipped tree
  is a stop-ship), `sources` (an unexpected registry or git dep is a supply-chain event), `licenses`
  once the repo's own license is decided, and `bans.wildcards`. Warn only:
  `bans.multiple-versions` — all 18 duplicates `16-deny.txt` reports are upstream-driven (the
  RustCrypto 0.10/0.11 migration, `http` 0.2 vs 1.x inside the AWS SDK, `syn` 2 vs 3), none is
  actionable by this repo, and gating on them would block every dependency bump. Revisit if the count
  grows past ~20 or if a *security-relevant* crate (a crypto or TLS crate) appears twice — today
  none does, because `ring` is confined to the legacy `longtail-sys` lane and `aws-lc-rs` is the sole
  provider in the shipped tree.

- **Tradeoff / risk:** `targets` pruning hides advisories that only affect an untargeted platform;
  acceptable, and it is what makes the duplicate list actionable. `exclude`-ing the legacy pair is
  right only while they remain non-default-members.
- **Effort:** S for the file; the license decision is not an engineering task.
- **Regression test to add:** a `cargo deny check` step in `rust.yaml`'s PR-gating job (see SEC-12).

---

### `SEC-12` — the audit workflow cannot gate a PR and its path filter watches a file that does not exist

**P2** · `hardening` · CONFIRMED

- **Where:** `.github/workflows/audit.yaml:2-16`.
- **What:** three defects in fifteen lines. (a) The triggers are `push` (path-filtered), `schedule`,
  and `workflow_dispatch` — there is **no `pull_request` trigger**, so the job can never be a
  required check on a PR. (b) The self-reference in the path filter is `".github/workflows/audit.yml"`
  (`:6`) while the file is `audit.yaml`, so editing the workflow does not trigger it via that entry.
  (c) `"**/audit.toml"` (`:11`) matches nothing — no `audit.toml` exists anywhere in the repo. The
  action itself is pinned by SHA (`:30`), which is correct and worth keeping.
- **Failure scenario:** a PR adds a dependency with a known RUSTSEC advisory. Nothing runs on the PR.
  It merges; the daily cron then opens an issue (or, on `push` to a non-default branch, produces a
  check-run nobody reads). The switchover ships with the advisory. Today the tree is clean
  (`06-audit.json:3` — 0 vulnerabilities, 0 warnings over 411 deps), so this is latent, not live.
- **Evidence:** the workflow read in full; `06-audit.json:3`; `16-deny.txt:28388`; `rust.yaml:7-10`
  names the gating set as "the pure lane, clippy/fmt, and miri" — audit is not in it.
- **Recommendation:** add `pull_request:` to the triggers, fix the path to `audit.yaml`, drop the
  dead `audit.toml` entry (or add the file if an ignore list is wanted), and add a
  `cargo deny check advisories bans sources licenses` step to the **PR-gating** job in `rust.yaml`
  once SEC-11's `deny.toml` exists. `cargo-deny` is a superset of `cargo-audit` for this purpose and
  runs in ~1 s (`MANIFEST.md` row 16).
- **Tradeoff / risk:** gating on advisories means a new RUSTSEC entry can red-line unrelated PRs.
  That is the intended behaviour; the `ignore` list with dated comments is the escape hatch.
- **Effort:** S
- **Regression test to add:** n/a (CI config).

---

### `SEC-13` — the prebuilt C library's SHA256 is not verified when the archive is already cached

**P3** · `hardening` · CONFIRMED

- **Where:** `support/longtail-sys/build.rs:86-91`.
- **What:** `try_download` computes and compares the SHA256 only on the download branch (`:98-102`,
  `panic!("SHA256 mismatch")` — correct). The `if file.exists() { dst }` early return at `:88-90`
  skips both the verification *and* the extraction for an archive already on disk. `UPSTREAM_VERSION`
  is pinned (`:13`), the URL is HTTPS to `github.com` (`:11-12`), and the submodule is pinned to a
  commit — `.gitmodules` carries no `branch =`, so `git submodule update --init` (`:71-78`) checks
  out the recorded gitlink (`96241fe`, `v0.3.3-101-g96241fe`). Worth noting alongside R2's **ALG-10**:
  the header submodule is at v0.3.3-101 while the prebuilt binary is v0.4.3.
- **Failure scenario:** anything that can write into the build cache (a prior malicious crate's
  `build.rs`, a poisoned CI cache restore, a shared build agent) substitutes the zip and it is
  extracted and linked without a hash check. Confined to the **legacy differential lane** —
  `longtail-sys` is not a default member (`Cargo.toml:6-9`) — which is why this is P3 and not higher.
- **Evidence:** `build.rs:60-110` read in full; `.gitmodules`; `Cargo.toml:6-9`; worker (c)'s report,
  verified line by line.
- **Recommendation:** move the hash check outside the `exists` branch — hash whatever is on disk,
  re-download on mismatch. Five lines. If the legacy pair is deleted after one release cycle as
  `CLAUDE.md` says, this disappears with it; do the five lines anyway, because "we're deleting it
  soon" has a poor track record.
- **Effort:** S
- **Regression test to add:** none (build script).

---

### `SEC-14` — vendor-default minio credentials inline in the S3 workflow

**P3** · `hardening` · CONFIRMED

- **Where:** `.github/workflows/s3-minio.yaml` — four blocks: the two service-container `env:` maps
  and the two job-level `env:` maps (job 1 and the interop job). I am not reproducing the values.
- **What:** these are the **published vendor defaults** for the `bitnami/minio` image, bound to an
  ephemeral service container reachable only on `localhost:9000` inside the runner, in a workflow
  that runs on `workflow_dispatch` and a weekly cron. **No rotation is required and nothing is
  leaked** — there is no real credential here.
- **Failure scenario:** the risk is process, not exposure: a secret scanner flags them and burns
  triage time, or the pattern gets copied into a workflow that points at a real endpoint. The second
  is the one that matters — the same file already sets `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`
  at job level, which is exactly the shape that becomes dangerous if the endpoint ever changes.
- **Recommendation:** leave the values (moving them to `secrets` would make the workflow harder to
  run and no safer), but add a one-line comment above each block stating they are the image's
  documented defaults for a localhost-only container and must never be pointed at a real endpoint;
  and add the file to whatever secret-scanning allowlist the org uses. Advise the maintainer rather
  than change it — the workflow is R8's file.
- **Effort:** S

---

### `SEC-15` — cache eviction deletes every file under `chunks/`, whatever it is

**P3** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-store/src/cache.rs:262-283` (`collect_cache_files`) and `:247`
  (`remove_file`).
- **What:** `collect_cache_files` recurses `<cache_root>/chunks` and pushes **every regular file** —
  the `.lrb` extension the doc comment at `:217-218` promises ("Sums the size of every `.lrb` block
  file … Only files under `chunks/` are considered") is never actually tested for. `evict_cache_dir`
  then deletes least-recently-used entries until the budget is met. The cache root is
  `--cache-path`, an operator-supplied directory that the code neither creates exclusively nor marks
  as owned.
- **Failure scenario:** `--cache-path` is pointed at a directory that already contains a `chunks/`
  subtree — a shared cache root, a typo, or a path that a previous tool used — and a `--cache-size`
  run silently deletes files that are not ours. The blast radius is bounded to one subdirectory name,
  which is why this is P3 rather than a peer of OPS-03, but the class is the same: a destructive
  sweep with no ownership check.
- **Evidence:** `cache.rs:212-283` read in full; the doc at `:217-218` states an invariant the code
  does not enforce; the three tests at `:302-350` only ever create `.lrb` files, so the gap is
  untested. Cross-reference **STORE-10** (the sweep only runs in `close()`, which the error path
  never reaches) — the two together mean eviction is both under-triggered and over-broad.
- **Recommendation:** filter on `extension() == Some("lrb")` in `collect_cache_files` — one line,
  and it makes the existing doc comment true. Optionally drop a `.longtail-cache` marker file in the
  cache root on first use and refuse to evict a root without one.
- **Effort:** S
- **Regression test to add:** extend `evict_removes_least_recently_used_until_under_cap` with a
  non-`.lrb` file under `chunks/` and assert it survives.

## Lower-priority observations

- `crates/longtail-core/src/store_index.rs:189-227` — the 28-line doc block describing
  `MergeStoreIndex` semantics is attached to the private `reserve_capacity`, because the
  `/// Reserve capacity …` lines were appended to the same comment; `pub fn merge` at `:229` has no
  doc at all. R1 owns the file — flagging, not filing.
- `crates/longtail/src/clonestore.rs:195` — `target_lvi.replace(".lvi", ".lsi")` is a global replace;
  R7 filed it as **OPS-14**. From a security angle it is also a write-target-derivation from an
  operator string with no validation, and `safe_join`-style thinking does not apply because the
  target is a URI. Agreeing with OPS-14's fix (`strip_suffix`), not re-filing.
- `crates/longtail/src/path_filter.rs` — the `regex` crate has linear-time matching and a default
  10 MB compiled-size limit, so the filter regexes are not a ReDoS or memory vector even though they
  come from the CLI. `split_regexes`' `&s[start..i - 1]` (`:36`) only ever slices at ASCII `*`
  positions, so it cannot panic on a UTF-8 boundary. Verified good.
- `crates/longtail-store/src/blob/fs.rs:229-234` — the temp-file name mixes PID with a
  nanosecond-XOR-counter (`fastrand_like`, `:250-259`). Not cryptographic, but the file is created in
  a store-owned directory with `File::create` (which does not `O_EXCL`), so a predictable name is a
  local-symlink-attack surface in a world-writable store directory. Very low likelihood; noting for
  completeness rather than filing.
- **R1's FMT-012 can be upgraded from PLAUSIBLE to CONFIRMED** — Appendix A.3 gives the construction
  (`block_size` and `block_use` are `wrapping_add` sums at `store_index.rs:588,590`, so a `.lsi`
  whose chunk sizes sum past `u32::MAX` makes the quotient exceed `u32` before the cast). Handing it
  to R1 rather than re-filing.
- `Cargo.toml` has no `[profile.release] panic = "abort"`, so the unwinding cases in SEC-06 genuinely
  unwind. Worth keeping that way.
- `07b-machete.txt` reports `tracing` as an unused dependency of the `longtail` facade — which is
  consistent with SEC-07/SEC-08's finding that the facade logs nothing about resolved URIs or
  endpoints. Adding the logging those findings ask for would also make the dependency used.

## Comments & documentation issues

### `SEC-DOC-01` — `CLAUDE.md`'s `unsafe` claim is false; `docs/rust-port.md`'s is correct

**P2** · `idiom` · CONFIRMED

- **Where:** `CLAUDE.md:101`; contrast `docs/rust-port.md:208-223`.
- **What:** this is the named deliverable, so here is the full inventory, established by
  `rg 'forbid\(unsafe_code\)|unsafe ' crates/ support/longtail-bench/ xtask/`:

  | target | kind | `forbid` | `unsafe` |
  |---|---|---|---|
  | `longtail-core` | lib | `src/lib.rs:43` | none |
  | `longtail-store` | lib | `src/lib.rs:19` | none |
  | `longtail` | lib | `src/lib.rs:32` | none |
  | `longtail-cli` | **bin** | `src/main.rs:5` | none |
  | `xtask` | **bin** | `src/main.rs:16` | none |
  | `longtail-testkit` | lib | `src/lib.rs:8` | none |
  | `longtail-bench` | lib | `src/lib.rs:20` | none |
  | **`longtail-bench` `src/bin/e2e.rs`** | **bin** | **absent** | **5 blocks: `:89, :91, :110, :113, :114`** (`libc::wait4`, `MaybeUninit::assume_init`, `libc::kill`) |

  `CLAUDE.md:101` asserts "Every default-member library **and binary target** is
  `#![forbid(unsafe_code)]`." `support/longtail-bench` **is** a default member (`Cargo.toml:6-9`) and
  `e2e.rs` **is** one of its binary targets, so the claim is false as written. A crate-level
  `#![forbid]` in `src/lib.rs` does not reach `src/bin/*.rs` — each bin is its own crate root.

  `docs/rust-port.md:208-223` is **correct and precise**: it enumerates the same 5 `libc` blocks and
  explicitly says "a binary target that the library's `forbid` does not cover" (`:219-220`), and its
  count of five matches mine exactly. The `memmap2` note at `:222-223` is also accurate — there is no
  `memmap2` dependency anywhere.
- **Recommendation:** change `CLAUDE.md:101` to "Every default-member library target, plus the
  `longtail-cli` and `xtask` binaries, is `#![forbid(unsafe_code)]`; the bench `e2e` binary is the one
  exception (5 `libc` blocks, inventoried in `docs/rust-port.md` §Safety posture)." Alternatively add
  `#![forbid(unsafe_code)]` to `e2e.rs` and `#[allow(unsafe_code)]` on the five blocks with the
  justification inline — that makes the claim true by construction and survives future edits, which
  a prose assertion does not. R9 should check the doc wording against this table.
- **Effort:** S

### `SEC-DOC-02` — no keeper doc states the trust boundary

**P1** · `security` · CONFIRMED

- **Where:** absent. `rg 'threat|untrusted|hostile|malicious|trusted' docs/rust-port.md
  docs/format-spec.md readme.md CLAUDE.md` returns **zero** hits.
- **What:** the port's entire input surface — `.lvi`, `.lsi`, `.lsb`, get-config JSON, S3 object keys
  — is remote, and four of my five P0/P1 findings exist because no one wrote down what is trusted.
  `readme.md`'s "content-addressed" framing actively suggests an integrity property the format does
  not provide (SEC-03). A production-readiness statement that does not name its trust boundary cannot
  be checked.
- **Recommendation:** one section in `docs/rust-port.md`, roughly: *"Everything read from a store is
  attacker-influenced if the store or transport is. The store is trusted for content integrity —
  block hashes cover the chunk-hash array only, and nothing re-hashes payload bytes (SEC-03). The
  port defends the **filesystem boundary**: asset paths from a `.lvi` are validated for containment
  before any syscall (SEC-01), and no other property of a `.lvi`/`.lsi`/`.lsb` is assumed. Operators
  must control who can write to their store."* Four sentences, and it makes SEC-01's guard something
  a future optimization pass will not quietly delete.
- **Effort:** S

### `SEC-DOC-03` — `decode_block_payload`'s "never panics" is true of the logic, not the allocation

**P3** · `hardening` · CONFIRMED

- **Where:** `crates/longtail-core/src/compress.rs:291`.
- **What:** "Never panics on malformed input — every failure is a typed `CompressError`." Every
  *branch* in the function is fallible, so the claim is true of the control flow; it is not true of
  `Brotli::decompress`'s `Vec::with_capacity(uncompressed_size)` at `:239`, which on a 4 GiB
  declaration aborts (ALG-02). R2 owns the fix; the doc line should say what it actually promises —
  e.g. "every *decode* failure is a typed `CompressError`; allocation is bounded by
  `max_block_bytes`" once SEC-04's cap lands.
- **Effort:** S

### `SEC-DOC-04` — the get-config key set is documented only in a source comment

**P2** · `complexity` · CONFIRMED

- **Where:** `crates/longtail/src/get.rs:2-4`.
- **What:** the three keys `get` reads, the one it ignores, and the "all configs must agree on
  `storage-uri`" rule exist only in a module comment. The get-config is a *format* — it is written by
  `put`, consumed by `get`, and shared with golongtail — and `docs/format-spec.md` does not describe
  it. See SEC-07 for the security consequence of the undocumented ignored key.
- **Recommendation:** a short §10 in `docs/format-spec.md`: the key set, which are required, which
  are ignored and why, and that every value is a URI that will be dereferenced.
- **Effort:** S

## Review of R1's and R2's proposed fuzz targets, from a security angle

The question is whether each target asserts the invariant an **attacker** attacks, as opposed to the
one a **refactor** breaks. Both are worth having; they are not the same.

- **`vi_parse` / `si_parse` / `lsb_parse` (FMT-007).** Their headline invariant is the byte fixpoint
  `to_bytes() == input`. That is a *compat* invariant — it protects against a refactor, not an
  attacker. An attacker's goal is the opposite: to get a buffer **accepted** whose downstream use is
  dangerous, and a fixpoint-preserving hostile `.lvi` is trivially constructible (SEC-01's escaping
  path round-trips perfectly). Keep the fixpoint assertion; do not count these three toward
  trust-boundary coverage.
- **`vi_walk` (FMT-007).** This is the right shape — it crosses from parse into consumption and,
  as R1 says, reproduces FMT-001 directly. **Two additions make it the security target:** (a) call
  `path()` for every asset and assert `safe_join(root, path)` either errors or yields a path with
  `root` as a prefix — that is precisely the invariant SEC-01 shows is currently violable, and
  without it the target cannot catch a traversal; (b) call `get_required_chunk_hashes`
  (`diff.rs:129-133`) as well as `validate_store` — per Appendix A.2 that is the site on the actual
  download path, and it panics before the store is even contacted, so it is both the cheapest
  reproducer and the one that matters.
- **`si_algebra` (FMT-007).** `merge(a,b).to_bytes() == merge_consuming` is a compat invariant (the
  S3 shard name depends on it — correctly identified). The *security* invariant for a `.lsi` is
  different and, pleasingly, already holds: whatever offsets a parsed index carries,
  `get_existing_store_index` rebuilds a canonical one (`store_index.rs:658`), which is what makes
  every downstream `.lsi` walk safe. **Assert that** — for an arbitrary parsed `.lsi`, the result of
  `get_existing_store_index` satisfies `block_chunks_offsets[i] + block_chunk_counts[i] <= C` for all
  `i` — because it is the property the whole store layer's safety rests on and nothing tests it
  today.
- **Missing entirely: a decode target.** No proposed target calls `decode_block_payload`, so ALG-02's
  bomb — the highest-value memory finding in the two reviews — is unreachable by all five. Add
  `lsb_decode`: `fuzz_target!(|i: (u32, Vec<u8>)| { let _ = decode_block_payload(i.0, &i.1); })`,
  run under `-rss_limit_mb=2048`. R1 is right that `-rss_limit_mb` is what proves the allocation
  bound holds; the target that would exercise it does not exist yet. This is the single highest-value
  addition to the plan.
- **R2's `chunker_range_equivalence` (02, "What differential fuzzing can and cannot replace").**
  Excellent target, but be clear about what it is: the chunker consumes **local files the operator
  owns**, not store bytes. It is a correctness fuzz that substitutes for missing oracle coverage
  (ALG-11), not a trust-boundary fuzz. Worth saying so in the plan so nobody counts it toward
  attacker-surface coverage.
- **Operationally**, R1's CI shape is right and I would keep it exactly: a scheduled 60 s/target run
  plus a `-runs=0` corpus replay on every PR. The replay is what keeps a fixed crash fixed and costs
  seconds. Commit every reproducer under `fuzz/corpus/<target>/`.

## Hardening backlog

Ranked by value per unit of effort.

1. **`safe_join` + its unit table + the two-run delete integration test** (SEC-01, SEC-02). One
   function, seven call sites, closes both P0s.
2. **`lsb_decode` fuzz target under `-rss_limit_mb`** — the only proposed target that reaches
   ALG-02/SEC-04.
3. **`vi_walk` extended with the containment assertion** (SEC-01, SEC-05, FMT-001) — turns the
   guard into a property.
4. **A bounded-read cap in `BlobObject::read` + `decode_block_payload`** (SEC-04), with a
   `MemBlobStore` over-cap test.
5. **The shared `VersionIndex::asset_chunks` accessor** (SEC-05, FMT-001) — replaces **seven** copies
   of an unguarded loop (Appendix A.2) and makes the `with_capacity` safe by construction. Pair it
   with lifting `cp.rs:147-152`'s payload check into `apply.rs:342` (FMT-002).
6. **Rayon `panic_handler`** (SEC-06 / STORE-12) — makes the two panic paths agree.
7. **`symlink_metadata` before create/chmod/remove on the apply path** (SEC-10), unix test; record
   the Windows reparse-point gap as accepted rather than covered.
8. **`deny.toml` + `pull_request` on the audit workflow** (SEC-11, SEC-12).
9. **A `proptest` over `safe_join`**: for arbitrary `String` inputs, `Ok(p)` implies
   `p.starts_with(root)`. Cheap, and it is the exact statement the security claim needs.
10. **miri**: nothing here needs it — the four production crates are `forbid(unsafe_code)` and
    `09-miri.txt` already passes 96 tests. No loom need either; R3 covered the concurrency.

## Verified good

Things I checked that are sound, so the next session does not redo them.

- **The index codecs cannot over-allocate from a header count.** `Reader::take` (`cursor.rs:47-58`)
  validates `pos + n <= data.len()` before slicing, and every `*_vec` collects from that exact
  slice — so a `.lvi`/`.lsi`/`.lsb` parse allocates at most the input size, no matter what the
  counts say. This is the single most important thing the format layer gets right, and it is why
  SEC-05's `with_capacity` (which reads the field *after* parse) is the exception rather than the
  rule. Cross-check: `malformed.rs:207-235` already tests exactly this for both index types.
- **`.lsi` parse is exact-length** (`store_index.rs:106-117` rejects both short and long), so a
  store index's counts are pinned to the object size.
- **Every consumer downstream of `get_existing_content` receives a canonical store index.**
  `get_existing_store_index` rebuilds through `StoreIndex::from_block_indexes` (`store_index.rs:658`)
  with cumulatively-recomputed offsets, and every `.lsi` chunk-range read inside it is guarded
  (`:580-583`, `:454-457`, `:491-496`, `:166-174`). So a wild `.lsi` on the store cannot propagate a
  bad offset into `apply`, `cp`, or `inspect`. This is the strongest structural property in the
  store layer and it is worth not losing: any future path that hands a *parsed* `StoreIndex`
  straight to a consumer, rather than a rebuilt one, re-opens all of it.
- **The remote fetch checks the returned block's declared hash** against the requested one
  (`remote.rs:362`), and the cache repeats it (`cache.rs:97`). Sound as far as it goes — SEC-03
  explains how far that is.
- **Block and cache keys are hash-derived, never listing-derived**: `sync::block_path`
  (`sync.rs:62-66`), `sync::shard_key` (sha256), `cache_block_path` (`cache.rs:70-74`). No attacker
  string ever reaches those joins.
- **The fs-blob listing cannot produce an escaping key** — `get_objects_sync` builds names via
  `path.strip_prefix(root)` over a real directory walk (`blob/fs.rs:128`), so `prefix.join(name)` at
  `:75` stays under the prefix. This corrects worker (b)'s "unguarded" characterisation of
  `sync.rs:272/361` and `prune.rs:427`.
- **`prune_store_blocks` deletes only through the same client that listed** (`prune.rs:408,427`),
  and `parse_block_hash` (`:381-389`) requires a hex-parseable `0x…` name, so a non-block file in the
  store is never deleted. (OPS-03 remains the real prune danger; it is R7's and unaffected by this.)
- **The permission write-back masks correctly** — `fs_util.rs:197`, `& 0x1FF`. setuid/setgid/sticky
  from a hostile `.lvi` cannot reach `chmod`. R1's report confirmed independently.
- **`scan_folder` does not follow symlinks out of the source** (`fs_util.rs:78-84`, `symlink_metadata`
  + skip-non-regular). The write side is the gap (SEC-10), not this one.
- **Credentials are never snapshotted, never logged, and never placed in a URI.** `S3Options` holds a
  provider or a `Client` (`blob/s3.rs:79-106`), its hand-written `Debug` prints only
  `credentials_provider: bool` (`:123-136`), and error messages carry the S3 error code and key but
  no credential material (`:58-73`). `rg` over the production tree found no `access_key`/`secret`/
  `token` string outside tests. This is done well.
- **Supply chain is clean today**: 0 vulnerabilities and 0 warnings over 411 deps
  (`06-audit.json:3`), `advisories ok` / `bans ok` / `sources ok` (`16-deny.txt:28388`). One crypto
  provider in the shipped build (`aws-lc-rs`; `ring` reaches only the legacy `longtail-sys` lane) —
  the mission's "two crypto stacks" premise does not hold for the default-member tree.
- **The path-filter regexes are not a DoS vector** (see Lower-priority observations).

## Experiments requested

| # | hypothesis | exact command | what would change the finding |
|---|---|---|---|
| 1 | A `.lvi` naming `../escaped.txt` makes `downsync` write outside `--target-path` (SEC-01) | Build a `.lvi` with one asset `../escaped.txt` over a one-chunk store fixture, then `mkdir -p /tmp/t/inner && cargo run -p longtail-cli -- downsync --source-path /tmp/t/v.lvi --storage-uri /tmp/t/store --target-path /tmp/t/inner && ls -l /tmp/t/escaped.txt` | If `escaped.txt` does **not** appear, some layer I did not find rejects it → SEC-01 drops to PLAUSIBLE and I need the rejecting line. |
| 2 | `cp` against a `.lvi` with `asset_chunk_counts[0] = 0xFFFFFFFF` aborts on allocation rather than erroring (SEC-05a) | Craft the `.lvi` (≈200 bytes), then `cargo run -p longtail-cli -- cp --version-index-path /tmp/t/bad.lvi --storage-uri /tmp/t/store --source-path a --target-path /tmp/out; echo "exit=$?"` | `exit=134` (SIGABRT) or an OOM-kill confirms the abort. A clean non-zero exit with a `LongtailError` means the allocator returned and the finding becomes a `Vec` growth cost, not an abort → P2. |
| 3 | On Windows, a `.lvi` path of `C:\Windows\Temp\x` or `\\server\share\x` escapes via `Path::join` prefix replacement (SEC-01, Windows arm) | On a Windows runner: `cargo test -p longtail --test downsync_e2e -- --ignored windows_absolute_asset_path` after adding that case; or a 5-line `Path::new(root).join("C:\\Windows\\Temp\\x")` print | If `join` does **not** replace for these forms, the Windows arm narrows to `..` only and OPS-09's device-name clause becomes the dominant Windows risk. |
| 4 | golongtail's `cmd_get.go` reads `s3-endpoint-resolver-uri` from the get-config JSON (SEC-07) | `git -C <golongtail checkout> grep -n 's3-endpoint-resolver-uri' -- cmd_get.go longtailutils/` at the v0.4.5 tag | If Go also ignores it, SEC-07's divergence half evaporates and only the `put`-writes-a-dead-key cleanup remains → P3. |
| 5 | A `.lsb` whose brotli frame declares `uncompressed_size = 0xFFFFFFFF` aborts the process (ALG-02 / SEC-04) | `cargo run -p longtail-cli -- downsync …` against a store with that block; `echo "exit=$?"` | Confirms which of "abort" vs "typed error" the user actually sees, which determines whether SEC-04's cap must precede the allocation or may follow it. |

## Open questions for the maintainer

1. **What license does this repository ship under?** SEC-11 is blocked on it, and so is the
   `[licenses]` section of the proposed `deny.toml`. Not legal advice — please confirm with the repo
   owner and outside counsel before any external distribution.
2. **Who can write to the production store?** SEC-01's severity is P0 on the assumption that an
   attacker can influence a `.lvi`. If the store is write-restricted to a single signing CI identity
   and served over TLS to the Tauri app, the *likelihood* drops — but the guard is cheap and the
   consequence is code execution on end-user machines, so I would still land it before switchover.
3. **Does the Tauri app pass a pre-built `aws_sdk_s3::Client`?** If so, R3's STORE-13 means the
   endpoint, path-style, and stalled-stream settings are silently dropped for the app — worth knowing
   before SEC-09's default is relied on.
4. **Is `--source-index-path` on `upsync` used in production?** It is the read/exfiltration arm of
   SEC-01. If nothing uses it, removing it is a cheaper fix than guarding it.

## Files read

Absolute paths, all under `/home/chris/work/longtail-rs/cm/rust-port/`:

`crates/longtail/src/fs_util.rs`, `crates/longtail/src/apply.rs`, `crates/longtail/src/downsync.rs`,
`crates/longtail/src/get.rs`, `crates/longtail/src/put.rs`, `crates/longtail/src/cp.rs`,
`crates/longtail/src/clonestore.rs`, `crates/longtail/src/path_filter.rs`,
`crates/longtail/src/prune.rs` (:380-445), `crates/longtail/src/upsync.rs` (:85-285),
`crates/longtail-core/src/version_index.rs`, `crates/longtail-core/src/cursor.rs`,
`crates/longtail-core/src/block.rs`, `crates/longtail-core/src/compress.rs`,
`crates/longtail-core/src/pack.rs`, `crates/longtail-core/src/store_index.rs` (:1-250),
`crates/longtail-store/src/uri.rs`, `crates/longtail-store/src/compress.rs`,
`crates/longtail-store/src/cache.rs`, `crates/longtail-store/src/blob/fs.rs`,
`crates/longtail-store/src/blob/s3.rs`, `crates/longtail-store/src/remote.rs` (:333-395, :470-590),
`crates/longtail-store/src/sync.rs` (:360-465), `crates/longtail-cli/src/main.rs` (:470-500, flag
declarations, :618-665), `support/longtail-sys/build.rs` (:60-110),
`support/longtail-bench/src/bin/e2e.rs` (:78-120), `support/longtail-bench/src/lib.rs` (:1-30),
`.github/workflows/audit.yaml`, `.github/workflows/s3-minio.yaml` (:1-95), `Cargo.toml`,
`CLAUDE.md`, `docs/rust-port.md` (§Safety posture), `readme.md` (§CLI),
`target/review-evidence/MANIFEST.md`, `target/review-evidence/06-audit.json`,
`target/review-evidence/16-deny.txt` (license/bans sections),
`target/review-evidence/05-tree.txt` (crypto + duplicate sections),
`target/review-evidence/14-golongtail-help.txt` (`get`), and the `## Findings index` sections of
`docs/review/{01-format-codecs,02-algorithms-and-oracle,03-store-concurrency,07-operations-cli}.md`
(plus FMT-007 and ALG-02 in full, for the fuzz and decompression cross-references).

---

## Appendix A — every production `unwrap`/`expect`, classified

**Method.** `rg '\.unwrap\(\)|\.expect\(' crates/{longtail-core,longtail-store,longtail,longtail-cli}/src`,
minus `unwrap_or*`, minus everything at or below each file's first `#[cfg(test)]`. That yields
**19 sites**, not 12 — the mission's count matches the 11 sites *outside* `blob/mem.rs`; the 8 in the
in-memory blob store are production code (it is a `pub` store, reachable by any embedder) and belong
in the inventory.

"Untrusted input" here means a `.lvi`/`.lsi`/`.lsb` byte, an S3 object key, or a get-config value.

| # | site | classification | argument |
|---|---|---|---|
| 1 | `longtail-core/src/hash.rs:105` `Blake2sVar::new(8)` | **UNREACHABLE (proof)** | The argument is the literal `8`; `Blake2sVar::new` errors only for a length outside `1..=32`. No input reaches it. R2's **ALG-16** proposes making it structurally impossible — agreed. |
| 2 | `longtail-core/src/hash.rs:110` `finalize_variable` | **UNREACHABLE (proof)** | Errors only when the output buffer length ≠ the configured digest length; both are the same literal `8` from site 1. |
| 3 | `longtail-store/src/remote.rs:755` `index.as_ref().unwrap()` | **UNREACHABLE** | R3 proved the three sound; taken as given per my mission. **STORE-18** proposes encoding the invariant in the type. |
| 4 | `longtail-store/src/remote.rs:780` | **UNREACHABLE** | as above |
| 5 | `longtail-store/src/remote.rs:816` | **UNREACHABLE** | as above |
| 6 | `longtail-store/src/compress.rs:47` `rx.await.expect("rayon codec task dropped its result")` | **UNREACHABLE-IN-PRACTICE, masked** | The sender drops only if the rayon closure panics — and rayon's default panic handler **aborts the process first**, so this `expect` never runs (SEC-06, STORE-12). Once a `panic_handler` is installed it becomes genuinely reachable from a codec panic on attacker bytes and must return `StoreError::WorkerGone`. This is the one `expect` whose classification *changes* when another finding is fixed. |
| 7 | `longtail/src/apply.rs:205` `.expect("apply semaphore never closes")` | **UNREACHABLE (proof)** | The `Semaphore` is created at `:182` and owned by the local `Arc`; nothing calls `close()`. **STORE-20** covers the class (no semaphore in the workspace is ever closed). |
| 8 | `longtail/src/apply.rs:236` `report_lock.lock().unwrap()` | **NOT REACHABLE FROM UNTRUSTED INPUT** | Poisoning requires a panic while the guard is held — i.e. inside the embedder's `ProgressSink::report`. Attacker bytes do not reach it. If it does fire, the panic is inside a `JoinSet` task and `flatten_apply_task` (`:353`) converts it to an error, so the blast radius is one failed apply. |
| 9 | `longtail/src/upsync.rs:270` `cached.as_mut().unwrap().1` | **UNREACHABLE (proof)** | `cached = Some(...)` is assigned on the immediately preceding line (`:269`); this is a borrow-checker workaround, not a runtime assumption. Could be an `if let` + early bind. |
| 10–17 | `longtail-store/src/blob/mem.rs:77, 110, 115, 129, 137, 142, 160, 168` | **NOT REACHABLE FROM UNTRUSTED INPUT** | Six are `state.lock().unwrap()` (poisoning only); `:142` and `:160` are `blobs.get(&self.path).unwrap()` guarded by an `exists`/`contains_key` check earlier in the same lock scope. `MemBlobStore` never parses attacker bytes — it stores and returns them. |
| 18–19 | `longtail-cli/src/progress.rs:136, 190` `.expect("progress mutex poisoned")` | **NOT REACHABLE FROM UNTRUSTED INPUT** | Poisoning only, from a panic in the terminal-rendering closure. CLI-only. |

**Net:** zero of the 19 is reachable from untrusted input today; one (#6) becomes reachable the
moment SEC-06/STORE-12's `panic_handler` lands, and should be converted to a typed error in the same
change. The `unwrap`/`expect` inventory is **not** where this port's panic risk lives.

### A.2 — reachability of the panics that are *not* `unwrap`s

This is where the risk actually is: unchecked slice indexing on fields read from a file.

| site | classification | argument |
|---|---|---|
**The single root cause.** `VersionIndex::from_bytes` validates array *lengths* but never the
per-asset chunk-map *values*: neither `asset_chunk_index_starts[a] + asset_chunk_counts[a] <= ACI`
nor `asset_chunk_indexes[i] < C` is checked anywhere in the workspace. That one gap makes **seven**
call sites reachable. R1's FMT-001 names it; the table below is its full blast radius, which is the
part R1's single citation understates.

| site | classification | argument |
|---|---|---|
| **`diff.rs:129-133`** `to.asset_chunk_indexes[start + k]`, `to.chunk_hashes[cidx]` | **REACHABLE — and this is the one that matters** | `get_required_chunk_hashes` is called at `downsync.rs:169`, **before `downsync.rs:170` contacts the store**. A hostile `.lvi` alone panics the primary download path with no network access and no other precondition. This is the shortest path to FMT-001 and it is not the site R1 cites. |
| `apply.rs:110-112` | **REACHABLE** | Same fields; panics on the caller's async task → unwinds out of `downsync()`. |
| `build.rs:198-206` (`extract_record`) | **REACHABLE** | Only with 2+ `--source-path` (`downsync.rs:265` → `merge_version_index`). |
| `validate.rs:52-57` | **REACHABLE** | `validate-version`, and `prune-store` via `prune.rs:87`. The site R1 cites. |
| `cp.rs:85-87` | **REACHABLE** | `cp` on a hostile index. |
| `upsync.rs:222-228` | **REACHABLE via `--source-index-path` only** | A scanned index is well-formed by construction. |
| `inspect.rs:302-311` | **REACHABLE** | `print-version-usage`. Read-only, so DoS only. |
| `apply.rs:342` `block.payload[block_off..block_off + len]` (R1's **FMT-002**) | **REACHABLE, but bounded to an error** | `block_off`/`len` accumulate from the `.lsb`'s own `chunk_sizes`; the payload length is never cross-checked (**ALG-03**), and for `tag == 0` the payload is returned verbatim (`compress.rs:293-295`). But it runs inside `spawn_blocking` (`apply.rs:221`), so tokio catches the unwind and `flatten_apply_task` (`:353-363`) returns a `LongtailError`. **Error, not crash** — this materially bounds FMT-002 and should be recorded there. `cp.rs:147-152` is the ready-made patch (SEC-05). |
| `cp.rs:118-127`, `inspect.rs:287-291`, `upsync.rs:252-257` (`.lsi` walks) | **UNREACHABLE (proof)** | These consume the output of `get_existing_content`, which terminates in `StoreIndex::from_block_indexes` (`store_index.rs:658`) — offsets rebuilt cumulatively, so the index is canonical whatever the on-store `.lsi` said. `upsync`'s `missing` is built locally by `create_missing_content` and never parsed. My first draft had these as reachable; worker (a) supplied the disproof and I verified it by reading `store_index.rs:570-660`. |
| `store_index.rs:621-630` `for idx in offset..(offset + count)` (no local check) | **UNREACHABLE (proof)** | `b` is drawn from `potentials`, and the only `potentials.push` (`:602`) sits inside the branch already validated at `:578-583` for the same `b`. The bare `offset + count` cannot overflow for the same reason. |
| `apply.rs:304` `block_index.chunk_hashes[i]` | **UNREACHABLE (proof)** | `i` iterates `chunk_sizes`, and `BlockIndex::read_prefix` (`block.rs:67-68`) reads both vectors with the same `chunk_count`. (A hand-built `BlockIndex` could differ — the fields are `pub` — but no production path builds one except via `read_prefix`.) |
| `apply.rs:317` `debug_assert_eq!(bsz, w.chunk_size)` | **REACHABLE in debug builds only** | `bsz` is the `.lsb`'s size for the chunk, `w.chunk_size` the `.lvi`'s; nothing forces agreement. Compiled out of the shipped release binary. R7's **OPS-12** owns it. |
| `compress.rs:239` `Vec::with_capacity(uncompressed_size)` | **REACHABLE → abort** | ALG-02. Not a panic — an allocation failure, which aborts. See SEC-04, SEC-06, Experiment 5. |
| `cp.rs:82-83` `Vec::with_capacity(count)` | **REACHABLE → abort** | SEC-05(a). Same mechanism, reachable from a ~200-byte `.lvi`. |

Everything else that indexes a parsed structure is guarded, and I checked each: `cursor.rs:47-58`
gates every read; `store_index.rs:105-117`, `version_index.rs:181-187`, `block.rs:59-65` gate the
array lengths; `version_index.rs:95-100` and `file_infos.rs:103-108` gate the name blob;
`store_index.rs:166-174, 454-457, 491-496, 580-583` gate every `.lsi` chunk range; the `HashMap`
`Index` uses in `diff.rs`, `build.rs`, and `downsync.rs:370-375` all key off maps filled from the
same source in the same function.

### A.3 — the integer casts

`rg -o ' as (u8|u16|u32|u64|usize|i32|i64|u128|isize)\b'` over the four production `src/` trees
returns **232**; excluding `#[cfg(test)]` bodies leaves **219**, of which 2 are float→int, so
**217 integer→integer casts** are in scope (worker (a)'s refinement, spot-checked against my own
grep). The mission's 234 is within counting noise. The count is not the interesting number; the
classification is, and only one group matters:

- **Narrowing from an untrusted on-disk field.** The `u32 → usize` casts of `asset_chunk_counts`,
  `asset_chunk_index_starts`, `block_chunk_counts`, `block_chunks_offsets`, and `chunk_sizes` are
  **widening on 64-bit** (`u32 → usize` cannot lose data) — so on every platform CI builds, none of
  them truncates. Their danger is not truncation but the *unchecked use of the widened value* as an
  index or a capacity, which is A.2 above and SEC-05. The genuinely narrowing casts are
  `len() as u32` on the **write** side (R1's **FMT-010**, **FMT-011**) and `remote.rs:519` /
  `blob/s3.rs:299` (R3's **STORE-19**) — all already filed, none reachable from untrusted input on
  the read path.
- **Genuinely lossy, on-disk-sourced, unchecked — exactly two.** `store_index.rs:597`
  (`((block_use as u64 * 100) / block_size as u64) as u32`) is R1's **FMT-012**, which R1 marked
  PLAUSIBLE; here is the construction that makes it CONFIRMED. `block_size` and `block_use` are
  `wrapping_add` sums of on-disk `chunk_sizes` (`:588, :590`), so a `.lsi` whose chunk sizes sum past
  `u32::MAX` can leave `block_size` wrapped to `1` while `block_use` stays near `4e9`; the quotient
  is then ~`4e11` and the `as u32` truncates it. Effect: attacker-chosen block *selection* inside
  `get_existing_store_index` — a correctness and cost issue, not memory safety. `compress.rs:303`
  (`uncompressed_size as usize`) is value-preserving but unbounded as an allocation size — ALG-02 /
  SEC-04.
- **Write-side narrowing worth one line.** `compress.rs:340-341` writes
  `raw.len() as u32` / `compressed.len() as u32` into the frame header. A block payload past 4 GiB
  wraps silently and produces a corrupt-but-parseable `.lsb`. Not attacker-reachable (block sizes are
  bounded by `create_store_index`'s fill loop), but it is the one `to_bytes`-side cast that can emit
  bad bytes rather than merely a wrong count, and the `u32::try_from(..).map_err(SizeOverflow)`
  pattern at `store_index.rs:176` shows the codebase already knows the right shape. Sibling of
  R1's FMT-010/FMT-011.
- **32-bit targets.** `u64 → usize` *would* truncate, and `FormatError::SizeOverflow`'s guards
  (`cursor.rs:15-23`) exist for exactly that case — R1's **FMT-014** correctly notes CI never builds
  such a target. I add only the security framing: if a 32-bit build is ever shipped, the entire
  `checked_*` layer becomes load-bearing against attacker counts overnight, and it has no test. Either
  add a 32-bit `cargo check` to CI or state in `docs/rust-port.md` that 32-bit targets are
  unsupported. I found no other cast whose truncation is reachable from a `.lvi`/`.lsi`/`.lsb` field
  on a 64-bit target.
