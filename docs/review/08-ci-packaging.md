# 08 · CI, packaging & test-strategy review
- **Reviewed at:** `456274d` · **Lead model:** fable · **Workers:** 2 × fable (a third launch was refused twice by the concurrent-agent cap; I covered its ground — the nine-manifest matrix — by direct reading)
- **Slice:** `.github/workflows/**`, all nine `Cargo.toml` + `Cargo.lock`, `.cargo/config.toml`, `rustfmt.toml`, `renovate.json`, `.gitignore`, `.gitmodules`, `xtask/src/main.rs`, `fixtures/**`, `test-data/mkdata.{sh,ps1}` · **Confidence:** covered well (the cold native-build cost, initially filed as experiment #1, has since been measured — see CI-02)

## Findings index

| ID | P | Dimension | One line | file:line | Verdict |
|---|---|---|---|---|---|
| CI-01 | P1 | hardening | The Windows PR lane green-lights a platform the suite barely runs: no facade/CLI integration tests (`#![cfg(unix)]`), no clippy, no bench compile-check — lane half of ALG-01/R7 | `rust.yaml:42-54` | CONFIRMED |
| CI-02 | P2 | hardening | The `formatting` PR gate depends on network + C toolchain + libclang per PR (submodule fetch, cc, bindgen via `--workspace`); cost measured at ~6 s cold penalty (`EXP-01`), so the issue is determinism and a false readme claim, not speed | `rust.yaml:129,135` | CONFIRMED (EXP-01) |
| CI-03 | P1 | hardening | The S3 lanes cannot fail by not running: env-gated tests record PASS while skipping, and the workflow has no anti-skip guard — the production S3 backend (24.49% region) is one env-var typo from permanent zero coverage with green dashboards | `s3-minio.yaml:35-39`, `s3_spec.rs:160-163` | CONFIRMED |
| CI-04 | P2 | hardening | None of the four PR-gating jobs sets `timeout-minutes`; a miri or proptest hang bills the 6-hour default and blocks all merges | `rust.yaml:22,42,113,147` | CONFIRMED |
| CI-05 | P2 | hardening | Every lane rides floating toolchains (`nightly` unpinned; pure lanes don't even name one); no `rust-toolchain.toml`, no `rust-version` anywhere — one bad nightly bricks clippy/fmt/miri repo-wide | `rust.yaml:121,157` | CONFIRMED |
| CI-06 | P1 | hardening | CI never builds or tests `--release`, there is no `[profile.release]`, and the switchover checklist has the operator build the shipped binary locally — the tested profile (debug-assertions on, OPS-12) is not the shipped profile | root `Cargo.toml`, `switchover-checklist.md:13` | CONFIRMED |
| CI-07 | P2 | security | `mkdata.{sh,ps1}` download and execute golongtail with no checksum and no fail-fast, duplicating xtask's SHA256-pinned fetcher; the `.ps1` also skips the `store.lsi.sync` cleanup | `mkdata.sh:9`, `mkdata.ps1:1` | CONFIRMED |
| CI-08 | P2 | hardening | fixture-freshness gets the submodule only as a `build.rs` side effect, and a vendored C-compile failure is swallowed into `cargo:warning` — the freshness gate fails as inscrutable linker spew, with no timeout | `fixture-freshness.yaml:17`, `build.rs:414-420` | CONFIRMED |
| CI-09 | P2 | hardening | No CI cargo invocation passes `--locked`: the lanes may silently re-resolve while audit/deny vouch for the committed `Cargo.lock` | all four workflows (grep: zero hits) | CONFIRMED |
| CI-10 | P3 | hardening | audit.yaml's path filter watches `audit.yml` (file is `audit.yaml`) and a nonexistent `**/audit.toml`; no `pull_request` trigger, so a vulnerable dep merges and is caught up to 24 h later | `audit.yaml:6,11` | CONFIRMED |
| CI-11 | P3 | idiom | Publish/manifest hygiene: `publish = false` only on `longtail-bench`; no `license` on any crate; `longtail-ffi` at 0.2.0 vs workspace 0.1.0; `longtail-sys` edition 2021; unversioned path deps; no `[workspace.dependencies]`, `[workspace.lints]`, or MSRV | nine `Cargo.toml`s | CONFIRMED |
| CI-12 | P3 | hardening | Renovate is configured but its output is not consumed: bare `config:recommended`, 10 stale `renovate/*` branches including `crate-tokio-vulnerability` | `renovate.json`, `git branch -r` | CONFIRMED |
| CI-13 | P2 | hardening | The pinned-golongtail channel is Linux-only (URL, SHA, and lookup path all hardcode `linux-x64`), so gates ⑥/⑧ and the three-way's third leg are structurally impossible on Windows — and their skip is green | `xtask/main.rs:38-41`, `paths.rs:55` | CONFIRMED |
| CI-14 | P2 | hardening | The four public `*_blocking` wrappers — the documented entry point for simple callers — are called by no test and not by the CLI; `lib.rs` sits at 17.86% region | `longtail/src/lib.rs:103-145` | CONFIRMED |
| CI-15 | P3 | hardening | `cargo doc` runs nowhere in CI; four real rustdoc defects exist, one caused by the CLI's `[[bin]] name = "longtail"` colliding with the lib crate's doc output | `longtail-cli/Cargo.toml:7-9`, `08-doc.txt` | CONFIRMED |
| CI-16 | P3 | hardening | `uri.rs` scheme dispatch — the CLI's front door — is 56.83% region: the gs/abfs rejections, unknown-scheme arm, and Windows drive-letter path are untested | `uri.rs:185-225` | CONFIRMED |

Doc findings `CI-DOC-01` … `CI-DOC-04` are indexed in their own section. The lane-structure
deliverable, oracle cost inputs, coverage policy, and the release half of the upgrade statement
are named-deliverable sections before the findings.

## Scope

Read in full: `rust.yaml`, `audit.yaml`, `fixture-freshness.yaml`, `s3-minio.yaml`, all nine
`Cargo.toml`, `.cargo/config.toml`, `rustfmt.toml`, `renovate.json`, `.gitignore`, `.gitmodules`,
`xtask/src/main.rs` (836 lines), `test-data/mkdata.{sh,ps1}`, `fixtures/README.md`,
`support/longtail-sys/build.rs`, `support/longtail-testkit/src/paths.rs`. Read in part:
`fixtures/manifest.json` (head + generator block), `Cargo.lock` (header, targeted greps),
`docs/switchover-checklist.md` (release-relevant lines), `docs/rust-port.md` (eight-gate table),
`readme.md` (compat/licensing lines), `crates/longtail/src/lib.rs:100-148`,
`crates/longtail-store/src/uri.rs:185-225`, `crates/longtail-cli/src/main.rs:495-510`,
`crates/longtail-cli/src/progress.rs` (terminal check), test-file heads for gating attributes.
Excluded: fixture binary payloads (verified via `13-fixtures.txt`, not re-hashed); source files
owned by R1–R7 except along the declared test-strategy axis.

Secondary axis declared: test gating and coverage attribution required reading test-file heads
and a handful of source lines in files owned by R3/R5/R7; findings there are filed only as
lane/coverage findings, with cross-references, not as code findings.

## Verification performed

- Evidence consulted: `MANIFEST.md` (incl. both provenance caveats), `00-scope.txt` (no tags),
  `03-test.txt` (217 run / 217 pass / 2 skip; s3_spec PASS at 0.004s), `05-tree.txt`
  (20-name duplicate-version list), `06b-audit.txt` (clean, 411 deps), `07-unused.txt` (udeps
  cannot run), `07b-machete.txt` (unused deps), `08-doc.txt` (4 defects), `10-release.txt`
  (release builds, 21.2 MB binary), `12-loc.txt`, `13-fixtures.txt` (112 files / 29.09 MiB
  verified; 114 tracked), `15-coverage/summary.txt`, `16-deny.txt` (no `deny.toml`; licenses
  FAILED on defaults), `18-semver.txt` (no baseline).
- Both workers' claims were re-verified line-by-line; two worker errors were caught and are
  corrected here: (i) worker (c) claimed `verify-fixtures` runs only in the scheduled freshness
  workflow — wrong, it gates every PR in both pure lanes (`rust.yaml:34`, `:54`); (ii) my seeded
  brief overstated "no `cargo bench --no-run` per-PR" — pure-linux runs it twice per PR
  (`rust.yaml:39-40`); the gap is Windows-only.
- Seeded-lead corrections: "no `description` on any crate" is **wrong** — `longtail-sys` and
  `longtail-ffi` both carry (good) LEGACY descriptions; everything else in that lead verified.
  The OPS-19 path is `crates/longtail/src/path_filter.rs`, not `longtail-cli`.
- Could NOT verify: whether the SHA-pinned `believer-oss/setup-rust-toolchain` fork enables
  rust-cache internally (its source is not in this repo — experiment #3); whether
  `bitnami/minio:latest` still resolves to a maintained image (needs registry access / CI run
  history).
- Post-review update (2026-08-05): experiment #1 was **run by the orchestrator**
  (`EXP-01-cold-clippy.txt`) — mechanism confirmed, cost hypothesis refuted (cold 19 s vs warm
  13 s). CI-02, its index row, Deliverable 1, and the experiments table were updated; nothing
  else was re-analyzed.

## Deliverable 1 — cost inputs for R2's oracle decision, and where the differential lane belongs

R2 already decided retention (Option B, dated exit — `02-algorithms-and-oracle.md`). These are
the cost numbers that decision assumed, now measured or bounded:

**CI minutes.** The differential lanes are capped at 30 min (linux, `rust.yaml:70`) + 40 min
(windows, `rust.yaml:97`), weekly — a **ceiling of ~70 runner-minutes/week**, zero per-PR. The
evidence pack contains no actual run durations (the pack was generated warm and locally), so the
honest statement is "bounded by timeout, actuals unknown"; at GitHub's public-repo pricing this
is negligible, and even on paid minutes it is under 2% of a modest 100-PR/month pure-lane spend.
The per-PR exposure to the C library today is **not** in the differential lanes at all — it is
CI-02: the `formatting` gate builds `longtail-sys` on every PR — measured at only a ~6 s cold
penalty (`EXP-01-cold-clippy.txt`), but carrying a network + C-toolchain + libclang dependency
into a required check.

**Native-build fragility (the actual retention cost).** Five concrete debts, all verified:
1. The submodule arrives only as a `build.rs` side effect (`build.rs:71-78` shells
   `git submodule update --init`) — every consuming lane silently depends on
   `github.com/DanEngelbrecht/longtail` being reachable at build time (CI-08).
2. A vendored C-compile failure is downgraded to `cargo:warning` (`build.rs:414-420`) and
   resurfaces as undefined-reference linker spew far from the cause (CI-08) — note the asymmetry:
   the AVX2/AVX512 sub-builds (`build.rs:384`, `:403`) and the prebuilt path (`:94`, `:101`)
   all panic loudly.
3. The oracle version skew ALG-10: submodule at `v0.3.3-101-g96241fe` vs `UPSTREAM_VERSION =
   "v0.4.3"` (`build.rs:13`) — with `default = ["vendored"]` (`longtail-sys/Cargo.toml`), CI
   always exercises the *older* one.
4. `mkdata.{sh,ps1}` feed the lane an unverified binary with no fail-fast (CI-07).
5. Windows differential is structurally degraded: no golongtail leg at all (CI-13).

**Maintenance burden.** The legacy pair contributes measurable manifest debt: `longtail-ffi`
pins year-old dep versions (`aws-sdk-s3 1.69` vs the store's `1.120`, `tokio 1.43` vs `1.49`),
which is a plausible driver for part of the 20-name duplicate-version list in `05-tree.txt`
(e.g. `http 0.2.12`/`1.5.0`, `itertools` ×3) — each duplicate is compile time and renovate
churn. `07b-machete.txt` adds unused-dep noise. Deleting the pair (R2 step 7) reclaims all of
this; none of it is large enough to override R2's reasoning that deletion permanently freezes
`fixtures/`.

**Where the differential lane belongs:** exactly where it is — **scheduled weekly + manual
dispatch, never per-PR** — with three adjustments: (a) move the `--workspace` clippy pass out of
the PR `formatting` job and into this lane (CI-02), making the differential lane the *only* place
the C library is ever compiled; (b) add it to the release-readiness dispatch bundle (Deliverable
4) so a release candidate always gets a fresh differential run rather than "some Monday's"; (c)
after R2's decoupling steps 3–5, this lane shrinks to a pure-Rust + pinned-golongtail interop
lane (gates ⑤⑥⑦ + freshness) and the native fragility list above disappears with it.

## Deliverable 2 — the target CI lane structure

### The five installed tools

| Tool | Where | Runtime (evidence) | Failure mode if it flakes / why this placement |
|---|---|---|---|
| `cargo-deny` | **PR gate** for `check bans licenses sources` (deterministic against `Cargo.lock`), using **R6's `deny.toml`** verbatim; `check advisories` stays **scheduled** (daily, alongside/replacing audit.yaml) | ~1 s (`MANIFEST.md` row 16) | Splitting matters: bans/licenses/sources cannot flake (pure lockfile function); advisories change under your feet — a new RUSTSEC landing mid-review must not redden an unrelated PR. Today `16-deny.txt` fails licenses only because there is no config at all. |
| `cargo-fuzz` | **PR gate**: seed-corpus replay only (`cargo fuzz run <t> <corpus> -runs=0`) over **R1's five targets (FMT-007) and R2's three (02 §fuzz)** — deterministic, seconds. **Scheduled** (nightly or weekly): bounded discovery, `-max_total_time=60 -rss_limit_mb=2048` per target (~8 min for all eight) | replay: seconds; discovery: ~8 min | Discovery finding a *new* crash on an unrelated PR is the classic fuzz-gate flake — so discovery never gates; each new reproducer is committed to `fuzz/corpus/` and becomes part of the deterministic PR replay. Needs nightly; the `fuzz/` crate must stay out of `default-members` (R1's note). R2 asked for target 1 per-PR on both OSes: its replay satisfies that deterministically. |
| `cargo-llvm-cov` | **Scheduled weekly**, publishing `--summary-only`; enforcement via a committed per-crate region floor checked in that job (see Deliverable 3). Never a PR gate | ~1 min (`MANIFEST.md` rows 15a-c: 58 s + 1 s) | Coverage deltas jitter with async timing and nightly/LLVM version bumps; a hard PR gate on a percentage gets gamed or ignored. A floor breach in the weekly job files an issue instead of blocking merges. |
| `cargo-semver-checks` | **Blocked** until a baseline exists — `18-semver.txt`: no tags (`00-scope.txt`), nothing published, `main`'s layout incompatible. After the first release tag (**R5 owns the baseline mechanics**): run **on release + weekly**, not per-PR while the API is pre-1.0 and deliberately moving | n/a today | Pre-baseline it can only error; post-baseline, per-PR gating would turn every intentional API change into ceremony. On-release is where an accidental break actually matters. |
| `cargo-bloat` | **Local / manual dispatch only.** Record results into `docs/bench-<date>.md` like the benches | 33 s + 1 s (`MANIFEST.md` rows 17a/b) | Its numbers move with every rustc release; there is no size budget stated anywhere to gate against. If the Tauri app ever gets one, revisit as a scheduled report. |

Two adjacent facts the deliverable must carry: **`cargo doc` runs nowhere in CI** while
`08-doc.txt` shows four real defects (CI-15) — add `RUSTDOCFLAGS="-D warnings" cargo doc
--no-deps` to the `formatting` job (3 s per `MANIFEST.md` row 08) once the `[[bin]]` collision is
fixed with `doc = false`; and **`cargo-udeps` cannot run on this workspace at all**
(`07-unused.txt`: rejects `resolver = "3"`) — `cargo machete` is the substitute and already has
actionable output (`07b-machete.txt`), suitable for the same scheduled lane as coverage.

### Target lane table

| Lane | Cadence | Contents (delta from today in bold) |
|---|---|---|
| pure-linux | PR | build, test, verify-fixtures, bench --no-run ×2 — **all `--locked`, `timeout-minutes`, fuzz-corpus replay** |
| pure-windows | PR | **parity with pure-linux** (bench --no-run added; the `cfg(unix)` removals are R2/R7's code half — until they land, this lane must assert its expected test count, see CI-01) |
| formatting | PR | fmt + clippy over **default members only** (drop `--workspace`), **+ `cargo doc --no-deps -D warnings`**, **pinned nightly** |
| miri | PR | unchanged, **+ timeout, pinned nightly** |
| deny-static | PR (new) | `cargo deny check bans licenses sources --locked` with R6's config |
| advisories | daily | audit.yaml (path filter fixed, CI-10) or `cargo deny check advisories`; keep the issue-filing behavior |
| s3-minio | weekly + dispatch | unchanged **+ anti-skip guard (CI-03), pinned minio image digest** |
| differential | weekly + dispatch | unchanged **+ absorbs `--workspace` clippy; mkdata hardened (CI-07); fatal C-compile errors (CI-08)** |
| fixture-freshness | weekly + dispatch | unchanged **+ explicit `submodules: true` checkout, timeout** |
| quality-report | weekly (new) | llvm-cov summary + floor check, machete, fuzz discovery, (post-tag) semver-checks |
| release-readiness | dispatch (new) | Deliverable 4: full weekly set on the RC SHA + `--release` build & release-profile gates |

**What a green PR means today:** unix-only dev-profile logic, the fixtures/byte gates on Linux,
`longtail-core` under miri, floating-nightly clippy/fmt — with S3, golongtail interop, Windows
e2e, the C differential, doc health, and the shipped release profile all deferred to "some
Monday" or to nothing. **What it should mean at ship:** all of the above plus — the same test
set passes on Windows (or the delta is asserted, not silent); the lockfile the lanes ran is the
lockfile audit vouched for; the fuzz corpus replays clean; deny's static checks pass; and no lane
can go green by silently skipping its reason to exist.

## Deliverable 3 — test strategy: coverage policy, the pure lane's authority, fixtures

**Coverage enforcement: ratcheted floors in the weekly job, not a PR gate.** The measured
baseline is TOTAL 77.09% region / 78.46% line (`15-coverage/summary.txt`) — and crucially, that
number *is the pure lane's view*: the S3 lane skipped (worker (c) confirmed all three
network-S3 tests passed as no-ops in `03-test.txt`), differential tests don't compile in, so
what the coverage report shows is exactly what a green PR guarantees. Recommend: commit
per-crate region floors a point below current (`longtail-core` and `-store` are strong; the
facade and CLI floors go at their current values), have the weekly quality-report lane fail →
file an issue on breach, and raise floors manually at maintenance windows. A per-PR percentage
gate is the alternative I recommend against: async-timing jitter and llvm/nightly bumps make it
flake, and the failure mode (contributors padding trivial tests) is worse than the disease.

**Uncovered regions on production paths** (all verified against `15-coverage/summary.txt`; the
first two are already filed by wave 1 and cross-referenced, not re-filed):
- `crates/longtail-store/src/blob/s3.rs` — **24.49% region**, the lowest in the crate. The
  covered fraction is the fake-HTTP credential test + a constructor; every real
  read/write/delete/pagination path needs minio and runs weekly at best, behind CI-03's silent
  skip. This is the Tauri download path's backend.
- `crates/longtail/src/path_filter.rs` — **13.71%** (OPS-19; R7 owns the code finding). Zero
  tests of matching semantics anywhere; `commands_spec.rs` never passes a filter flag.
- `support/longtail-testkit/src/fixture_manifest.rs` — **8.20%**. Disproportionate: this is the
  code that decides whether committed fixtures count as valid, it stands behind the per-PR
  `verify-fixtures` gate (`rust.yaml:34`, `:54`), and ALG-05 already shows its failure path has
  no test proving it can fire. The gate that guards everything is itself the least-guarded code.
- `xtask/src/main.rs` — **0.00%**. Expected for a manually-run tool, but the same caveat:
  `verify_fixtures()` (`main.rs:735-752`) is a per-PR gate with no negative test.
- `crates/longtail/src/lib.rs` — **17.86%**: the four `*_blocking` wrappers (CI-14).
- `crates/longtail-store/src/uri.rs` — **56.83%**: scheme-dispatch arms (CI-16).
- `crates/longtail-cli/src/progress.rs` — **48.43%**: tests pipe output, so `is_terminal()`
  (`progress.rs:80`) is always false and the indicatif arm never runs. Low risk; noted.

**Can the pure lane alone gate a release? No.** It cannot see: the S3 backend (above), gates
⑤⑥⑧ (golongtail interop — weekly, and never on Windows, CI-13), the C differential (weekly),
any Windows end-to-end behavior (CI-01), the release profile (CI-06), or doc health (CI-15). The
pure lane is an excellent *merge* gate for `longtail-core`/`-store` logic and byte-format
fidelity on Linux; a *release* additionally requires the release-readiness dispatch bundle
(Deliverable 4). This is also the honest framing of `docs/rust-port.md`'s eight-gate table —
ALG-DOC-03 already flags the cadence overstatement.

**Fixture strategy: sound; keep it.** 112 manifest-verified files, 29.09 MiB (`13-fixtures.txt`;
114 tracked incl. README + manifest). Committing 30 MB of goldens to git is the right call at
this size — no LFS, no CI caching needed, clone cost trivial, and the per-PR `verify-fixtures`
hash check makes rot loud. The two structural weaknesses are (i) freshness runs weekly and
*only* via the differential-featured xtask (`fixture-freshness.yaml:23-24`), so fixture
regeneration is exactly the capability whose retention R2's memo protects — my cost inputs above
support that memo; and (ii) the freshness lane's own fragility (CI-08). One gap R1 flagged
(FMT-008: no golongtail-produced empty index) is a fixture-set matter; the *mechanism* needs no
change to accommodate it.

## Deliverable 4 — release & rollback (the release half of the joint on-disk upgrade statement)

R1's half (`01-format-codecs.md` §"On-disk upgrade / rollback"): version words match C exactly,
rollback is byte-safe both directions, future bumps fail loud and typed, `.lsb` is unversioned.
R3's half (`03-store-concurrency.md` Deliverable 4): shard names, `.gen` sidecars, locks, and
CAS order are mechanism-identical to golongtail `49a20e1`; Rust→Go is strictly safer than
Go→Rust (STORE-06); the one real hazard is Windows mixed writers (STORE-07). My half is the
release mechanics that sit on top:

**Current state: there is no release machinery at all.** No git tags (`00-scope.txt`), no
CHANGELOG, no release workflow, no `[profile.release]`, and `docs/switchover-checklist.md:13`
opens with the *operator* building `cargo build --release -p longtail-cli` on their own machine
— the shipped binary's provenance is a dev workstation, its profile is one CI has never
compiled, and (OPS-12) its `debug_assert`s are compiled out relative to everything CI tested
(CI-06).

**Release story to put in place before switchover:**
1. **Tag the switchover commit** (`v0.1.0` or similar). The tag is simultaneously: the
   semver-checks baseline R5 needs (`18-semver.txt`), the rollback reference for the *code*, and
   the anchor for a CHANGELOG (currently absent — start it at this tag; nothing earlier needs
   reconstructing).
2. **A `release-readiness` dispatch workflow** run on the RC SHA: the differential, s3-minio,
   and fixture-freshness lanes (fresh, not "whenever Monday last ran"), plus a `--release
   --locked` build for linux + windows, plus the byte gates and smoke suite executed **in the
   release profile** (`cargo test --release -p longtail --test lvi_byte_gate --test
   upsync_byte_gate --test smoke`) so the profile that ships is the profile that passed.
   Artifacts uploaded with SHA256s — the same provenance discipline the repo already applies to
   golongtail (`fixtures/manifest.json` generator block) applied to itself.
3. **Declare `[profile.release]` deliberately** rather than inheriting defaults. In particular
   the panic strategy interacts with STORE-12 (no `catch_unwind`, rayon panics abort): the
   default `unwind` vs an explicit `abort` is a real behavioral choice for a Tauri-embedded
   library, and today nobody has made it.
4. **Rollback is golongtail v0.4.5**, and it is already a good rollback artifact: pinned,
   SHA256-verified, fetchable in one command (`xtask fetch-golongtail`, `main.rs:37-41`). Per R1
   + R3, a store touched by the Rust CLI is fully readable by it; residue is cosmetic
   (`.tmp.<pid>` orphans, `._lck` files). The two caveats to write into the runbook are theirs:
   don't run mixed Rust+Go *fs-store writers* on Windows during a staged rollout (STORE-07), and
   a Go writer can leave a torn `store.lsi` that the Rust reader hard-fails on (STORE-06).
   Target-side residue (`.longtail.index.cache.lvi`) is R7's domain (OPS-11).
5. **On-disk upgrade discipline going forward:** any release that changes written bytes must
   regenerate `fixtures/` via `gen-fixtures` (which is why the oracle retention matters) and
   ride through the freshness + differential lanes on the RC SHA — the release-readiness bundle
   makes that automatic rather than remembered.

## Findings

### `CI-01` — the Windows PR lane green-lights a platform the suite barely runs
**P1** · `hardening` · CONFIRMED
- **Where:** `.github/workflows/rust.yaml:42-54`
- **What:** `pure-windows` runs `cargo build`, `cargo test`, `verify-fixtures` — and that is
  all. Because every integration test in `crates/longtail/tests/` and
  `crates/longtail-cli/tests/` is `#![cfg(unix)]` (ALG-01, R7 §Windows), `cargo test` on Windows
  compiles them to nothing: no `.lvi` byte gate, no upsync gate, no downsync e2e, no CLI spec
  (38 tests), no deadlock regression. The lane also omits the two `cargo bench --no-run` steps
  pure-linux has (`rust.yaml:39-40`) — the only difference between the two jobs — so bench
  compile breakage on Windows is invisible too. Windows coverage of the download path is
  `longtail-core` unit tests plus `apply.rs` in-module tests (R7's inventory).
- **Failure scenario:** a Windows-only regression in the facade (`fs_util.rs`'s four `cfg`
  forks, the `seek_write` loop, permissions synthesis) merges green, ships in the Tauri build,
  and is first observed by a player.
- **Evidence:** `rust.yaml:42-54` read directly; worker (c)'s gating inventory
  (`lvi_byte_gate.rs:7`, `commands_spec.rs:10`, etc.) re-verified per file; R7 Named
  deliverable 3 ("Nothing exercises permissions, deletes, resume, or the CLI" on Windows).
- **Recommendation:** the code half — de-`cfg(unix)`-ing the test files — belongs to R2/R7 and
  is the real fix. The **lane** half, which I own: (a) add the two bench `--no-run` steps to
  pure-windows for parity; (b) until the `cfg` removals land, make the asymmetry loud instead
  of silent — run tests via `cargo nextest run` and assert the executed-test count against a
  committed per-OS expectation (nextest prints `217 tests run`; a one-line
  `grep`-and-compare step suffices), so the day the unix gates are removed the expectation file
  is the reviewable diff, and until then the Windows number is visibly ~60 lower, not invisibly.
- **Tradeoff / risk:** a count assertion needs updating when tests are added — that is the
  point; it converts silent shrinkage into a diff.
- **Effort:** S (lane half)
- **Regression test to add:** the per-lane expected-count assertion itself.

### `CI-02` — the `formatting` PR gate depends on network, a C toolchain, and libclang — for ~6 seconds of C build
**P2** · `hardening` · CONFIRMED (mechanism and cost both measured: `EXP-01-cold-clippy.txt`)
- **Where:** `.github/workflows/rust.yaml:129`, `:135`
- **What:** `cargo +nightly clippy --workspace --all-targets` (run twice: plain and
  `--features longtail-core/fastcdc`) includes `longtail-sys` and `longtail-ffi`, which are
  deliberately excluded from `default-members` so that "a plain `cargo build` needs no network"
  (root `Cargo.toml:4-5`). **Mechanism confirmed by experiment #1** (fresh clone, submodules
  uninitialised, empty `CARGO_TARGET_DIR`, the exact PR command): the job shells
  `git submodule update --init` against `DanEngelbrecht/longtail` (`build.rs:71-78`), compiles
  the entire vendored C library (704 `.o` objects plus `liblongtail-cc{,-avx2,-avx512}.a`), and
  runs bindgen (`out/bindings.rs` — libclang required). The checkout at `rust.yaml:117` has no
  `submodules:` key and no cache step exists in any workflow.
  **Cost correction, stated plainly:** the cold run totalled **19 s** against 13 s warm — the
  entire cold penalty is ~6 s on the experiment host (fast link, many cores; a GitHub runner
  will be slower, but the order of magnitude is seconds, not minutes). The
  "warm cache conceals a build measured in minutes" framing originated in `MANIFEST.md`'s
  provenance caveat; my initial draft repeated it, and the orchestrator's experiment disproved
  it. The corrected number replaces the cost argument — it does not rescue it.
- **Failure scenario:** determinism, not cost. A required PR check depends on (a) GitHub
  reachability of a third party's repo at build time, (b) a host C compiler, (c) libclang on
  the runner image; any of the three failing or drifting reddens every open PR for a job whose
  purpose (fmt + lint of the Rust workspace) needs none of them. It also makes `readme.md`'s
  "no C library is built or linked" claim false on the CI path — the doc finding is R2's
  (ALG-DOC-02); the experiment now supplies its proof.
- **Evidence:** `EXP-01-cold-clippy.txt` (exit 0 in 19 s; submodule populated during the run;
  35 MB of C build output; `bindings.rs` produced). `MANIFEST.md`'s caveat is superseded for
  the cost claim. Note the C build succeeded cleanly there (**0** `cargo:warning` lines), so
  `build.rs`'s error-swallow was *not* exercised: whether a **broken** C build passes this
  check-only gate silently (clippy never links) is experiment #5 now, not a demonstrated fact —
  the swallow itself remains CONFIRMED from source (`build.rs:414-420`, see CI-08).
- **Recommendation:** unchanged — run the PR clippy over the seven default members (the exact
  package list `01-clippy-pure.json` used, which is clean) and move the `--workspace` clippy
  into the differential lane, which already owns the native build. The payoff is a
  deterministic merge pipeline and a keeper-doc claim made true again, not minutes saved. Still
  the decoupling step the R2 memo wants for free.
- **Why still P2 with the cost argument gone:** the availability leg is the same class as
  CI-05 (floating nightly) — an external, uncontrolled dependency that can brick every open PR
  at once — and the remedy is a one-line workflow change; "nice to have" (P3) undersells a
  required-check availability risk with an S-sized fix.
- **Tradeoff / risk:** clippy regressions inside `longtail-sys`/`-ffi` would surface weekly
  instead of per-PR — acceptable for crates whose code is frozen pending deletion.
- **Effort:** S
- **Regression test to add:** n/a (workflow change).

### `CI-03` — the S3 lanes cannot fail by not running
**P1** · `hardening` · CONFIRMED
- **Where:** `.github/workflows/s3-minio.yaml:35-39`, `:62`, `:108`;
  `crates/longtail-store/tests/s3_spec.rs:160-163`, `:188-191`;
  `crates/longtail-cli/tests/s3_interop.rs:72-78`
- **What:** the env-gated S3 tests early-`return` — recording **PASS** — when
  `LONGTAIL_TEST_S3_ENDPOINT` is unset or the golongtail binary is uncached, and the workflow
  contains no step verifying the tests actually executed. The "Wait for minio" steps
  (`s3-minio.yaml:48-56`, `:96-104`) prove minio is up, not that the env-var names in the
  workflow still match the names the tests read.
- **Failure scenario:** any drift — an env-var rename in test code, a job-level `env:` typo, a
  nextest filter change — turns the weekly lane permanently green-while-empty. R3's evidence
  shows exactly what that looks like: `03-test.txt:280-281` records `s3_spec` PASS at 0.004 s,
  and `blob/s3.rs` sits at 24.49% region — the production S3 backend's only network exercise is
  this one skippable lane.
- **Evidence:** all six skip sites quoted by worker (a)/(c) and re-read by me; workflow env
  block read directly.
- **Recommendation:** introduce `LONGTAIL_TEST_S3_REQUIRED=1`, set only by this workflow: when
  present, the `else` branches `panic!` instead of `return`. Three-line change per test file,
  zero effect on every other lane. (An output-grep step — fail if stdout contains `skipping` —
  works with `--nocapture` but is the weaker fix; the env contract survives refactors.)
- **Tradeoff / risk:** none meaningful; a genuinely broken minio service now fails the lane,
  which is the desired behavior.
- **Effort:** S
- **Regression test to add:** run the lane once with the endpoint var deliberately unset and
  `REQUIRED` set; it must fail.

### `CI-04` — no timeouts on any PR gate; miri is an unbounded hang
**P2** · `hardening` · CONFIRMED
- **Where:** `.github/workflows/rust.yaml:22`, `:42`, `:113`, `:147` (jobs); `timeout-minutes`
  exists only at `:70` and `:97` (the differential jobs)
- **What:** pure-linux, pure-windows, formatting, and miri inherit GitHub's 360-minute default.
  Local timings (`MANIFEST.md`: nextest 48 s, miri 57 s, clippy warm 13 s) say healthy runs are
  minutes; a hang — a deadlocked tokio test, a proptest case explosion under miri's ~50×
  slowdown, a wedged native download in CI-02's path — bills six hours and, with required
  checks, blocks the merge queue for the duration.
- **Failure scenario:** one wedged miri run stalls all merges for an afternoon and burns 6 h of
  runner; on a paid plan that is real money for zero signal.
- **Evidence:** grep across `.github/workflows/` — exactly two `timeout-minutes` keys, both
  differential (worker (a), re-verified).
- **Recommendation:** `timeout-minutes: 15` on pure lanes and formatting, `20-30` on miri
  (headroom over 57 s local is generous), `15` on fixture-freshness and the s3 jobs while
  touching them.
- **Tradeoff / risk:** a too-tight bound flakes on a slow runner day; the numbers above are
  15-30× observed.
- **Effort:** S
- **Regression test to add:** n/a.

### `CI-05` — floating toolchains gate PRs; no toolchain or MSRV pin anywhere
**P2** · `hardening` · CONFIRMED
- **Where:** `rust.yaml:121` and `:157` (`toolchain: nightly`, undated); pure lanes pass no
  `toolchain:` at all (whatever the SHA-pinned setup-action fork defaults to); no
  `rust-toolchain.toml` in the repo; no `rust-version` in any of the nine manifests
- **What:** three required checks (formatting's clippy `-D warnings`, fmt, miri) run on
  whatever nightly exists at job start. Nightly clippy grows lints and rustfmt occasionally
  changes output; either event turns every open PR red simultaneously through no fault of the
  code. Meanwhile no MSRV is declared for a workspace on edition 2024 + `resolver = "3"`
  (already new enough that `cargo-udeps` cannot parse it, `07-unused.txt`).
- **Failure scenario:** an overnight nightly adds a default-deny lint that fires in
  `longtail-store`; the merge queue is bricked until someone hotfixes either the code or the
  workflow, under time pressure, on every branch at once.
- **Evidence:** workflow lines read directly; `rustfmt.toml` contains only `max_width = 100`
  (nothing in it actually requires nightly fmt).
- **Recommendation:** pin `nightly-YYYY-MM-DD` in the two workflow inputs (or a
  `rust-toolchain.toml` if local/CI parity is wanted — note that pins *every* local build, so
  the workflow-input pin is the lighter touch); bump it deliberately, e.g. via a monthly
  renovate rule once CI-12 is addressed. Declare `rust-version` at the workspace level and
  inherit it.
- **Tradeoff / risk:** pinned nightlies age; the monthly bump is the maintenance cost, and it
  is bounded and scheduled instead of ambient.
- **Effort:** S
- **Regression test to add:** n/a.

### `CI-06` — the shipped profile is never built or tested by CI
**P1** · `hardening` · CONFIRMED
- **Where:** no `--release` in any workflow (grep: zero hits); no `[profile.release]` in the
  root `Cargo.toml`; `docs/switchover-checklist.md:13` (`cargo build --release -p longtail-cli`
  on the operator's machine)
- **What:** every test CI runs is dev-profile, where `debug_assertions` are on; the binary that
  ships is release-profile, where OPS-12's `debug_assert_eq!` chunk-size cross-check is
  compiled out and optimization behavior differs. The release profile itself is all defaults
  (no LTO/codegen/panic decisions ever made). `10-release.txt` proves the release build works
  today (41 s, 21.2 MB binary) — on this machine, once, locally.
- **Failure scenario:** a release-only misbehavior (an optimizer-sensitive path, a
  `debug_assert`-guarded invariant that silently stops being checked, per OPS-12 corrupting
  adjacent ranges instead of panicking) ships in the very binary the switchover checklist has
  the operator hand-build, with no CI provenance and no release-profile test run ever executed.
- **Evidence:** grep of workflows; root `Cargo.toml` read in full; checklist line read.
- **Recommendation:** Deliverable 4 items 2-3: a release-readiness dispatch lane that builds
  `--release --locked` on both OSes and runs the byte gates + smoke under `--release`; declare
  `[profile.release]` explicitly (the panic-strategy choice interacts with STORE-12 and
  belongs to the maintainer). Cheap interim: add `cargo test --release -p longtail` to the
  weekly differential lane.
- **Tradeoff / risk:** release-mode test compilation adds minutes to a weekly/dispatch lane —
  nothing per-PR.
- **Effort:** M
- **Regression test to add:** the release-profile byte-gate run itself (experiment #2 is its
  first execution).

### `CI-07` — mkdata executes an unverified download; the two scripts have drifted
**P2** · `security` · CONFIRMED
- **Where:** `test-data/mkdata.sh:3-12`, `:40-41`; `test-data/mkdata.ps1:1`
- **What:** both scripts download the golongtail v0.4.5 binary (wget/curl/`Invoke-WebRequest`)
  and execute it with **no checksum**, while the same repository already owns a SHA256-pinned
  fetcher for the same artifact (`xtask/main.rs:40`, `:100-103` — verified: it hashes both
  fresh downloads and cached copies). Neither script fails fast (`set -e` absent; no
  `$ErrorActionPreference`), and the `.ps1` omits the `store.lsi.sync` cleanup the `.sh` does
  at line 41 — so the Windows-generated test data differs in shape from the Linux data by one
  stray lock file.
- **Failure scenario:** (security) a compromised or replaced release asset executes on the
  weekly differential runners unverified — the exact scenario xtask's pinning exists to stop;
  (correctness) a failed download exits 0 and the lane fails later inside `longtail-ffi` unit
  tests as bare `unwrap` panics on missing files (`version_index.rs:437`), misattributing the
  cause; (drift) the `.sync` file sits in `test-data/small/storage/`, which
  `FolderScanner::scan` tests walk (`folderscanner.rs:243`), so the two OS lanes exercise
  subtly different trees.
- **Evidence:** both scripts read in full; xtask verification path read; consumers located via
  grep (only `longtail-ffi` unit tests use `test-data/`).
- **Recommendation:** have mkdata stop downloading: call `cargo run -p xtask --
  fetch-golongtail` (Linux) — one verified fetcher, one pinned hash — and for Windows extend
  xtask with the win32 URL+SHA pair (which CI-13 needs anyway). Add `set -euo pipefail` /
  `$ErrorActionPreference = "Stop"`, and add the `.sync` cleanup to the `.ps1`.
- **Tradeoff / risk:** none; the pinned hash already exists for Linux and adding the Windows
  pair is a constant.
- **Effort:** S
- **Regression test to add:** none practical beyond the scripts failing fast; the xtask path
  is already self-verifying.

### `CI-08` — fixture-freshness stands on a swallowed C-compile and a side-effect submodule
**P2** · `hardening` · CONFIRMED
- **Where:** `.github/workflows/fixture-freshness.yaml:17`, `:23-24`;
  `support/longtail-sys/build.rs:71-78`, `:414-420`
- **What:** the freshness job checks out without submodules and immediately runs
  `cargo run -p xtask --features differential -- …`, whose dependency chain (xtask →
  testkit/differential → ffi → sys, `default = ["vendored"]`) reaches `build.rs`, which (a)
  fetches the submodule itself as a side effect and (b) on C-compile failure prints
  `cargo:warning=Failed to compile` and **continues** into bindgen — the job then dies at link
  time with undefined-symbol spew whose real cause is two build layers away (deduced from the
  missing `rustc-link-lib` directive on the failure path, not yet demonstrated — experiment #5;
  EXP-01's C build succeeded cleanly so it could not exercise this). (Contrast: the
  AVX2/AVX512 sub-builds at `:384`/`:403` and the prebuilt path at `:94`/`:101` all panic.)
  A wholly *missing* submodule is loud — `add_c_files` panics at `build.rs:491` — it is
  specifically the compile-failure case that is swallowed. Note also that the first step runs
  `fetch-golongtail` **with** `--features differential`, forcing the full native build for a
  subcommand that needs none of it (the pure `cargo run -p xtask -- fetch-golongtail` in
  `rust.yaml:83` proves it works featureless). No `timeout-minutes`.
- **Failure scenario:** a runner-image gcc bump makes one vendored C file error; the weekly
  freshness gate — the only automated guardian of fixture regenerability — fails with linker
  noise, gets triaged as "CI flake", and fixture drift detection quietly lapses for weeks.
- **Evidence:** workflow + build.rs read in full; chain confirmed through the four manifests'
  feature graphs.
- **Recommendation:** three one-liners: `submodules: true` on the checkout (removing the
  side-effect dependency); make `try_compile` failure fatal in `build.rs` (this is the legacy
  oracle — a hard stop is correct; R2's memo depends on this lane being trustworthy); drop
  `--features differential` from the fetch step. Add a timeout.
- **Tradeoff / risk:** none; the swallow has no legitimate consumer (nothing builds
  `longtail-sys` intending to tolerate a failed C build).
- **Effort:** S
- **Regression test to add:** manual dispatch of the lane after the change is sufficient.

### `CI-09` — no `--locked` anywhere: the lanes and the auditors can disagree about the lockfile
**P2** · `hardening` · CONFIRMED
- **Where:** all cargo invocations in all four workflows (grep for `--locked`/`--frozen`: zero
  hits); `Cargo.lock` (v4, committed; `06b-audit.txt`: 411 dependencies scanned)
- **What:** `cargo build`/`test` without `--locked` will silently re-resolve and rewrite the
  lockfile whenever a manifest edit left it stale. audit.yaml and the future deny gate vouch
  for the *committed* lock; the build lanes execute whatever resolution happens at run time.
- **Failure scenario:** a PR bumps a version requirement without regenerating the lock; CI
  resolves a newer transitive dep at build time, tests pass against it, audit continues judging
  the stale committed lock — the tested tree and the vouched-for tree diverge, silently.
- **Evidence:** grep across workflows; lockfile header read.
- **Recommendation:** `--locked` on every CI cargo invocation (including `cargo run -p xtask`
  steps). Renovate's lockfile PRs (CI-12) keep this friction-free.
- **Tradeoff / risk:** PRs with stale locks now fail fast with an explicit message — desired.
- **Effort:** S
- **Regression test to add:** n/a.

### `CI-10` — audit.yaml watches files that don't exist and never sees a PR
**P3** · `hardening` · CONFIRMED
- **Where:** `.github/workflows/audit.yaml:2-16`
- **What:** the push-path filter names `.github/workflows/audit.yml` — the file is `audit.yaml`
  — so workflow edits never self-trigger; it also watches `**/audit.toml`, which exists nowhere
  in the repo (a `deny.toml`, when R6's lands, would also not match). There is no
  `pull_request` trigger at all; only path-filtered `push` (uncapped to any branch), the daily
  cron, and dispatch.
- **Failure scenario:** a PR adding a RUSTSEC-affected dependency merges green; the daily cron
  catches it up to 24 h later on `main`, after the fact.
- **Evidence:** the full `on:` block read directly (quoted by worker (a), re-verified).
- **Recommendation:** fix the filename in the filter; either add
  `pull_request: paths: ["**/Cargo.toml", "**/Cargo.lock"]` or — better — let the deny-static
  PR gate (Deliverable 2) own PR-time dependency checking and keep audit.yaml purely as the
  daily advisory alerter, whose `issues: write` reporting behavior is worth keeping.
- **Tradeoff / risk:** none.
- **Effort:** S
- **Regression test to add:** n/a.

### `CI-11` — publish/manifest hygiene: nothing stops an accidental `cargo publish`, and the metadata says nothing
**P3** · `idiom` · CONFIRMED
- **Where:** the nine manifests; specifically `longtail-bench/Cargo.toml` (`publish = false` —
  the only one), `longtail-ffi/Cargo.toml:3` (`version = "0.2.0"`), `longtail-sys/Cargo.toml:4`
  (`edition = "2021"`), root `Cargo.toml` (no `[workspace.dependencies]`, no
  `[workspace.lints]`)
- **What:** verified matrix: `publish = false` is missing on `longtail-sys`, `longtail-ffi`,
  `longtail-testkit`, `xtask`, and all four `crates/*`; no crate has `license`, `repository`,
  `readme`, or `keywords` (`longtail-sys`/`-ffi` do carry good LEGACY `description`s — the
  seeded lead was wrong there); intra-workspace deps are unversioned `path` deps throughout;
  `longtail-ffi` sits at 0.2.0 against the workspace's 0.1.0 and pins year-old dep versions
  (`tokio 1.43`, `aws-sdk-s3 1.69` vs the store's `1.49`/`1.120`), feeding the 20-name
  duplicate list in `05-tree.txt`; `07b-machete.txt` lists unused deps in four crates (xtask's
  `serde`/`serde_json`/`sha2`/`hex` confirmed by grep — its uses all route through
  `longtail_testkit::fixture_manifest`); there is no `[workspace.lints]`, so bare local
  `cargo clippy` is laxer than CI's `-D warnings`.
- **Failure scenario:** low-drama but real: a well-meaning `cargo publish -p longtail-core`
  (the one crate with no path deps) is currently stopped only by crates.io's missing-license
  rejection; version-req skew across nine manifests makes renovate churn and duplicate builds;
  the missing lints table means contributors discover `-D warnings` failures only in CI.
- **Evidence:** all nine manifests read in full; `05-tree.txt` duplicate list;
  `07b-machete.txt`.
- **Recommendation:** `publish = false` on all nine (until the maintainer answers the
  publishing question below); hoist shared dep versions into `[workspace.dependencies]`; add
  `[workspace.lints]` mirroring CI; align `longtail-ffi` to `version.workspace = true` (or
  leave it — it's scheduled for deletion; do not spend more than the one line); remove the
  machete-flagged deps. License fields wait on the licensing answer (Open questions).
- **Tradeoff / risk:** none of substance; all mechanical.
- **Effort:** S-M
- **Regression test to add:** n/a (deny-static with R6's config will hold the line thereafter).

### `CI-12` — renovate produces PRs nobody consumes
**P3** · `hardening` · CONFIRMED
- **Where:** `renovate.json` (four lines: bare `config:recommended`); `git branch -r`: 10
  `origin/renovate/*` branches, including `renovate/crate-tokio-vulnerability`
- **What:** the update pipeline exists but its output accumulates: ten open update branches,
  one of them a *vulnerability* remediation branch (evidently superseded — `06b-audit.txt` is
  clean — but never closed). A bot whose security PRs sit unmerged trains the team to ignore
  it, which is worse than no bot.
- **Failure scenario:** the next `crate-*-vulnerability` branch that actually matters sits in
  the same pile with the same habit applied to it.
- **Evidence:** `renovate.json` read; branch list from read-only git.
- **Recommendation:** decide the bot's contract: either (a) group lockfile-only updates into a
  weekly `lockFileMaintenance` PR with automerge-on-green (the pure lane + deny-static is a
  sound automerge bar), keeping only semver-major and vulnerability PRs for humans; or (b)
  remove `renovate.json` and rely on the daily advisories lane plus deliberate bumps. Close
  the ten stale branches either way.
- **Tradeoff / risk:** automerge requires trusting the PR gates — after CI-03/CI-09 land, they
  are trustworthy for this purpose.
- **Effort:** S
- **Regression test to add:** n/a.

### `CI-13` — the pinned-golongtail channel is Linux-only, so Windows can never run the interop gates
**P2** · `hardening` · CONFIRMED
- **Where:** `xtask/src/main.rs:38-41` (URL, SHA256, and filename all hardcode
  `longtail-linux-x64`); `support/longtail-testkit/src/paths.rs:55` (lookup:
  `target/golongtail/longtail-linux-x64`); `rust.yaml:93-108` (differential-windows has no
  fetch step)
- **What:** `fetch-golongtail` downloads the Linux binary unconditionally; the testkit's
  `golongtail_binary()` looks only for that filename. Consequently on Windows: gate ⑥
  (`upsync_interop.rs:48-49`, `:93-94`) and gate ⑧ (`s3_interop.rs:77-78`) skip, and the
  three-way e2e silently degrades to two-way (`downsync_three_way.rs:170` `golongtail_binary()?`,
  `:246` `if let Some(res)` — it logs `RAN`/skipped at `:255-257` but stays green). ALG-13
  filed the silent-green mechanism for gate ⑥; this finding is the *channel* fact that makes
  the skip permanent on one OS: there is no verified Windows binary to find.
- **Failure scenario:** a Windows-only interop break (say, path-separator handling in an
  upsynced `.lvi` consumed by golongtail on Windows) is untestable in CI by construction, on
  the OS the Tauri app ships to.
- **Evidence:** all lines above read directly; `mkdata.ps1:1` downloads
  `longtail-win32-x64.exe` (unverified, to a *different* location `test-data/longtail.exe`
  that `golongtail_binary()` never checks).
- **Recommendation:** add the win32 URL + SHA256 pair to xtask (choose per
  `std::env::consts::OS`), point `paths.rs` at the OS-appropriate filename, and add the fetch
  step to differential-windows. This also completes CI-07's consolidation.
- **Tradeoff / risk:** golongtail's Windows binary must actually pass the interop suite —
  running it the first time may surface real findings (that is the point).
- **Effort:** S-M
- **Regression test to add:** gate ⑥ executing (not skipping) on the Windows differential
  lane; the CI-01 count assertion catches regressions thereafter.

### `CI-14` — the public `*_blocking` API is tested by nothing and used by nothing in-repo
**P2** · `hardening` · CONFIRMED
- **Where:** `crates/longtail/src/lib.rs:103-145`; coverage row `lib.rs` 17.86% region
  (`15-coverage/summary.txt`)
- **What:** the four blocking wrappers (`downsync_blocking`, `get_blocking`,
  `upsync_blocking`, `put_blocking`) are the documented entry point for non-async callers —
  but the CLI builds its own runtime and calls `block_on(run(&cli))`
  (`longtail-cli/src/main.rs:495-502`; zero `_blocking` references in `longtail-cli/src`), and
  no test calls them either. Each wrapper builds a *new* multi-thread runtime per call; that
  behavior (and its error mapping to `InvalidArgument`) has never executed under test. If the
  Tauri app is the intended consumer, its entry point ships coverage-free; if nothing consumes
  them, they are dead public API.
- **Failure scenario:** a caller invokes `get_blocking` from inside an async context — the
  classic `block_on`-in-runtime panic — or the per-call runtime construction misbehaves under
  repeated calls; first execution of this code path is in the customer's process.
- **Evidence:** `lib.rs:100-148` read; CLI call site read; coverage row quoted.
- **Recommendation:** one smoke test per wrapper against a fixture store (they can reuse the
  existing `smoke.rs` scaffolding), plus a doc line stating they must not be called from async
  context. Separately fix the CLAUDE.md claim (CI-DOC-03).
- **Tradeoff / risk:** none.
- **Effort:** S
- **Regression test to add:** the four smoke tests.

### `CI-15` — `cargo doc` runs nowhere; the CLI bin name guarantees a doc collision
**P3** · `hardening` · CONFIRMED
- **Where:** no `cargo doc` in any workflow (grep: zero hits); `crates/longtail-cli/Cargo.toml:7-9`
  (`[[bin]] name = "longtail"`); `08-doc.txt` (exit 101 with four defects, per `MANIFEST.md`'s
  corrections section)
- **What:** the four pre-identified rustdoc defects (two feature-gated broken links, one
  private-item link, and the `longtail` bin-vs-lib output collision, cargo#6313) exist because
  nothing exercises docs. The collision is structural: the CLI's binary must be named
  `longtail` for golongtail compatibility, so any combined doc build will collide until the bin
  is excluded.
- **Failure scenario:** doc rot compounds unchecked; any future `cargo doc --workspace` (e.g.
  docs hosting for the Tauri team) fails or silently overwrites the facade's docs with the
  CLI's.
- **Evidence:** grep; manifests; `MANIFEST.md` defect list.
- **Recommendation:** `doc = false` under the `[[bin]]` table; fix the three link defects
  (R9/R5 own the prose); then add `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` (default
  members) to the `formatting` job — 3 s (`MANIFEST.md` row 08).
- **Tradeoff / risk:** none; the bin has no doc-worthy public items.
- **Effort:** S
- **Regression test to add:** the CI doc step is the regression test.

### `CI-16` — the store-URI front door is half-tested
**P3** · `hardening` · CONFIRMED
- **Where:** `crates/longtail-store/src/uri.rs:185-225`; coverage row 56.83% region
- **What:** the scheme-dispatch arms — `gs://` rejection (`:186-190`), `abfs`/`abfss` rejection
  (`:191-195`), unknown-scheme `InvalidUri` (`:222-225`), and the Windows drive-letter
  disambiguation — are uncovered; CLI tests pass only plain paths (worker (c): no
  `s3://`/`file://` occurrences in `commands_spec.rs`), and `blobstore_spec.rs` exercises the
  blob-level constructor, not this block-level one.
- **Failure scenario:** the first user to typo a scheme (`s3:/bucket`, `gs://…` from a GCS
  habit) exercises untested error paths; on Windows, `C:\store` vs scheme parsing is decided by
  code no test has run.
- **Evidence:** source lines and coverage row read; worker (c)'s grep re-verified.
- **Recommendation:** a table-driven unit test over ~10 URI shapes (accepted × rejected ×
  drive-letter) directly in `uri.rs`.
- **Tradeoff / risk:** none. **Effort:** S
- **Regression test to add:** the table test.

## Lower-priority observations

- `bitnami/minio:latest` floats on both s3-minio jobs (`s3-minio.yaml:27`, `:75`); Bitnami's
  2025 Docker Hub catalog restructuring makes an unpinned `latest` a lane-breakage risk
  independent of this repo's code — pin a digest (PLAUSIBLE on the ecosystem event; the
  floating tag itself is CONFIRMED).
- `.gitmodules` still names the submodule `longtail-sys/longtail` (pre-`support/` move); path
  is correct, so this is cosmetic — but any `git submodule` command addressing it *by name*
  must use the stale name. One-line fix via `git mv`-style re-add or hand-edit + sync.
- `.gitignore` has no credential patterns (`.env`, `*.pem`); nothing in the workflows writes
  credentials to disk today (minio creds are inline literals for a throwaway service), so this
  is preventive hygiene only.
- `mkdata.sh:3` uses `uname -o`, which historically errors on macOS (falls through to the
  `wget` branch, which macOS lacks by default) — the Darwin branch may be unreachable on the
  platform it targets. CI never runs it on macOS; local-dev papercut.
- `mkdata.sh`'s bare `wget` (no `-O`) saves `longtail-linux-x64.1` if the file already exists,
  and the subsequent `mv` then installs the *old* download.
- The pure lanes run `cargo build` then `cargo test` (`rust.yaml:32-33`); the build step is
  redundant work but buys failure attribution — fine as is.
- `progress.rs` at 48.43%: the indicatif arm needs a TTY and tests pipe output
  (`progress.rs:80`); acceptable, noted for the coverage floor discussion.
- `.cargo/config.toml` pins `+crt-static` for `x86_64-pc-windows-msvc` only — correct and
  deliberate for the Tauri link; note it does not apply to aarch64-windows if that ever ships.
- The `formatting` job's comment (`rust.yaml:110-112`) explains the nightly pin rationale well
  — the *comment* quality here is good; the gap is the missing date pin (CI-05).
- `xtask`'s `download()` (`main.rs:115-134`) shells `curl`/`wget` to avoid a TLS dep in the
  pure lane — sound choice, and its failure handling is correct (checked statuses, `bail!`).

## Comments & documentation issues

### `CI-DOC-01` — fixtures/README.md's byte-exactness claim is false for `.lsi`, and its regeneration claim omits the real prerequisite
**P2** · `hardening` · CONFIRMED
- **Where:** `fixtures/README.md:9-11` ("Regenerating it requires the pinned CLI and must
  reproduce the recorded checksums exactly (`xtask gen-fixtures`); a mismatch is a
  compatibility regression, not a fixture that needs updating")
- **What:** two inaccuracies in the sentence most likely to be obeyed literally. (i) golongtail
  emits store-index block order non-deterministically — the freshness workflow's own header
  comment says so (`fixture-freshness.yaml:4-6`) and `diff_fixtures` compares `.lsi`
  semantically for exactly this reason (`xtask/main.rs:798-813`) — so a regeneration that
  differs in `.lsi` bytes is *expected*, not a regression, and `manifest.json`'s `.lsi` hashes
  legitimately change on regen. (ii) regeneration requires `--features differential` and the C
  chunker for boundary tables (`main.rs:163-170` bails without it), not merely "the pinned
  CLI". A maintainer following the README verbatim would misdiagnose healthy regen output as a
  compat break, or fail at the first bail.
- **Recommendation:** state the `.lsi` semantic-comparison rule and the differential-feature
  requirement in the README; it is a keeper-adjacent file (`CLAUDE.md` points at it).
- **Effort:** S

### `CI-DOC-02` — CLAUDE.md's differential-lane instructions send Windows developers down a dead path
**P3** · `hardening` · CONFIRMED
- **Where:** `CLAUDE.md` §Common commands: "run `test-data/mkdata.sh` (or `.ps1`) and
  `cargo run -p xtask -- fetch-golongtail` first"
- **What:** on Windows, `fetch-golongtail` downloads a Linux ELF (CI-13) that verifies against
  the Linux SHA and can never spawn; `golongtail_binary()` then returns `Some(path-to-ELF)` and
  the three-way's spawn fails, or — if the fetch is skipped — everything silently degrades.
  Either way the instruction as written does not produce a working Windows differential run,
  and differential-windows in CI (`rust.yaml:93-108`) pointedly does *not* follow it.
- **Recommendation:** fix alongside CI-13; until then, a one-line caveat in CLAUDE.md.
- **Effort:** S

### `CI-DOC-03` — CLAUDE.md says the `*_blocking` wrappers exist "for the CLI"; the CLI doesn't use them
**P3** · `idiom` · CONFIRMED
- **Where:** `CLAUDE.md` §Runtime configuration ("`*_blocking` convenience wrappers exist for
  the CLI and simple callers"); `crates/longtail-cli/src/main.rs:495-502`
- **What:** the CLI builds its own runtime and never references `_blocking` (grep: zero hits in
  `longtail-cli/src`). A keeper-doc claim that is checkably false; it also disguises CI-14's
  coverage hole ("the CLI tests cover them" is a natural, wrong inference).
- **Recommendation:** "…exist for simple non-async callers (the CLI builds its own runtime)".
- **Effort:** S

### `CI-DOC-04` — `paths.rs`'s doc comment names the wrong cached filename
**P3** · `idiom` · CONFIRMED
- **Where:** `support/longtail-testkit/src/paths.rs:50-52` ("cached by `xtask fetch-golongtail`
  (`<workspace>/target/golongtail/longtail`)") vs the code at `:55` (`longtail-linux-x64`)
- **What:** the comment's path is not the path; anyone pre-placing a binary per the comment
  gets a silent skip (the function's `None` path). Trivial, but this function is the single
  switch behind three silent-green gates (CI-13), so its documentation should be exact.
- **Recommendation:** fix the string; CI-13's OS-dispatch change rewrites this line anyway.
- **Effort:** S

## Hardening backlog

Ranked; items 1-4 are the ones I would not ship without.

1. **Anti-skip guard on every env/binary-gated test** (CI-03, CI-13): `LONGTAIL_TEST_S3_REQUIRED`
   panic-instead-of-return, set by the s3 workflow; same pattern for the golongtail-gated tests
   in the differential lane. Converts four silent-green gates into real ones. (S)
2. **Per-lane expected-test-count assertion** (CI-01): nextest count vs committed expectation,
   per OS. The single cheapest control against coverage evaporating via `cfg`/feature gates —
   it would have caught ALG-01 the day it happened. (S)
3. **Timeouts + pinned nightly + `--locked`** (CI-04/05/09): three mechanical workflow edits
   that remove the three largest whole-repo availability risks. (S)
4. **Negative test for the fixture gate** (with ALG-05, which owns the code finding): corrupt
   one byte of a scratch copy, assert `Manifest::verify` reports it and `verify-fixtures` exits
   non-zero — the per-PR gate has never been shown able to fail. (S)
5. Release-profile byte-gate run, weekly + release-readiness (CI-06). (M)
6. Fuzz wiring per Deliverable 2: R1's five targets + R2's three; PR replay deterministic,
   discovery scheduled. (M — the targets are R1/R2's; the lane is mine)
7. The four `*_blocking` smoke tests (CI-14) and the `uri.rs` table test (CI-16). (S)
8. mkdata → xtask consolidation with the win32 hash pair (CI-07 + CI-13). (S-M)
9. `deny.toml` (R6's) + deny-static PR gate + `publish = false` across the workspace (CI-11). (S)
10. Coverage floors file + weekly check (Deliverable 3). (S)

## Verified good

- **`verify-fixtures` gates every PR on both OSes** (`rust.yaml:34`, `:54`) and the manifest
  design is right: per-file SHA256 plus generator provenance (URL, OS, binary SHA) in
  `fixtures/manifest.json:2-7`. Worker (c)'s contrary claim was wrong; corrected here.
- **xtask's binary supply chain is exemplary where it exists**: pinned URL + SHA256, verifies
  fresh downloads *and* cached copies, deletes on mismatch (`main.rs:81-110`). CI-07/CI-13 are
  about extending this discipline, not fixing it.
- **`diff-fixtures`' comparison semantics are exactly right**: byte-exact everywhere except
  `.lsi`, which is compared as sorted (hash, tag, chunk_hashes, chunk_sizes) tuples so a
  tag/size regression cannot hide behind reordering (`main.rs:798-813`).
- **Both third-party actions are SHA-pinned** (`believer-oss/setup-rust-toolchain@ff4c7a2…`,
  `rustsec/audit-check@3ae0128…`); only GitHub's own `actions/checkout@v4` rides a tag.
- **The differential lanes' cadence design is correct** (weekly + dispatch, `if:` guarded at
  `rust.yaml:68`/`:95`, with clear comments explaining why) — the seeded concern was where the
  lane belongs; the answer is: where it already is.
- **The lane comments in rust.yaml are unusually good** — the miri block (`:137-146`) and the
  differential block (`:56-65`) each state what runs, why, and what is deliberately excluded.
- **`concurrency` groups with `cancel-in-progress`** on rust.yaml and s3-minio.yaml prevent
  redundant runs on force-pushes.
- **`01-clippy-pure.json` is clean** (zero warnings over the seven default members), fmt is
  clean (`04-fmt.txt`), audit is clean (`06b-audit.txt`), and the committed lock has no git
  sources (grep: zero) — the pure lane's static health is genuinely good.
- **`.gitignore` correctly excludes the unverified mkdata binaries** (`test-data/longtail{,.exe}`)
  and generated test data, so they can never be committed by accident.
- **The `s3` feature plumbing** (`longtail-store` → `longtail` → `longtail-cli`, default-on,
  `--no-default-features` clean per `11-featurematrix.txt`) is coherent, and the
  `aws-sdk-s3 default-features = false` rationale comments in both `longtail-store` and
  `longtail-ffi` manifests are precise about the RUSTSEC motivation, including the note that
  the legacy crate must match "or it re-introduces the advisory" — manifest comments done well.

## Experiments requested

| # | Hypothesis | Exact command | What result would change |
|---|---|---|---|
| 1 | **RUN** (`EXP-01-cold-clippy.txt`) — hypothesis was: the cold path costs multiple minutes and requires network + libclang | fresh clone at `456274d`, no submodule init, empty `CARGO_TARGET_DIR`, the exact `formatting` command | **Resolved and applied to CI-02:** mechanism CONFIRMED (submodule fetch over the network, 704-object vendored C build, bindgen → libclang), cost REFUTED (cold 19 s vs warm 13 s, ~6 s penalty). CI-02 re-argued on determinism + doc honesty; priority held at P2, reasons in the finding |
| 2 | The release profile passes the byte gates today (CI-06 baseline) | `cargo test --release --locked -p longtail --test lvi_byte_gate --test upsync_byte_gate --test smoke` | a failure escalates CI-06 to P0 territory (shipped profile broken *now*); a pass converts CI-06 to pure lane-work |
| 3 | The SHA-pinned `believer-oss/setup-rust-toolchain` fork enables rust-cache internally (affects the CI-04 timeout headroom and lane-runtime expectations; EXP-01 has since shown the CI-02 cold path is cheap regardless) | fetch `https://github.com/believer-oss/setup-rust-toolchain/blob/ff4c7a2d9523e22eab355f13c7732a4ea3e7a9b1/action.yml` and check for a `cache` input defaulting true | caching present → dependency compile time amortizes across PRs and the suggested `timeout-minutes` values have extra headroom; absent → size timeouts to full cold dependency builds. No longer decisive for CI-02, whose remedy rests on determinism |
| 4 | The weekly s3-minio lane has been silently green or failing on the floating image (CI-03 / minio observation) | `gh run list --workflow s3-minio.yaml --limit 12 --json conclusion,createdAt` + open one run's logs and grep for `skipping` | `skipping` in a green run proves CI-03's scenario has already happened; pull failures confirm the image-pinning observation |
| 5 | A **broken** vendored C build passes the check-only clippy gate silently (clippy never links, and `build.rs:414-420` swallows the compile error into `cargo:warning`), while `fixture-freshness`'s `cargo run` path fails at link time with misattributed errors — EXP-01 could not test this because its C build succeeded cleanly (0 warnings) | in a scratch clone with the submodule initialised, introduce a syntax error into `support/longtail-sys/longtail/src/longtail.c`, then (a) `cargo +nightly clippy --workspace --all-targets`; (b) `cargo run -p xtask --features differential -- fetch-golongtail` | (a) exiting 0 proves the formatting gate cannot see a dead oracle build — strengthens CI-02's determinism argument and CI-08's make-`try_compile`-fatal remedy; (a) failing would weaken the CI-08 misattribution scenario for the clippy path (the source-level swallow stands either way) |

## Open questions for the maintainer

1. **Licensing (escalated, not decided).** The repo has no LICENSE file, no `license` fields,
   and `readme.md` is silent — while the workspace derives from and (in the legacy pair)
   vendors/links MIT-licensed upstream C (`support/longtail-sys/longtail/LICENSE.txt`), ships a
   CLI named `longtail`, and will be distributed inside the Tauri app. What is the intended
   license and distribution posture? This is a general observation, not legal advice — the MIT
   attribution obligations for shipped binaries and the naming question are worth confirming
   with outside counsel before the switchover release.
2. **Is crates.io publication ever intended** for any of the four `crates/*`? Decides between
   blanket `publish = false` (my default recommendation) and full metadata + R5's semver
   discipline.
3. **Who owns renovate?** If nobody will merge its PRs, should it be removed rather than left
   accumulating vulnerability branches (CI-12)?
4. **What is the MSRV / toolchain policy** for the Tauri build environment? Needed to pin
   `rust-version` and the CI nightly (CI-05) meaningfully.
5. **Where should release binaries be built** — the release-readiness lane (Deliverable 4) or
   the operator flow in `switchover-checklist.md:13`? If the latter is deliberate (e.g. signing
   constraints), the checklist should at least require the release-profile gate run (experiment
   #2) on the same SHA first.
6. **May the differential-windows lane fetch and spawn the golongtail win32 binary** (CI-13)?
   If Windows interop is consciously out of scope, that decision should be written into
   `docs/rust-port.md` §Dropped-and-deferred rather than enforced by a hardcoded Linux URL.

## Files read

Workflows: `.github/workflows/rust.yaml`, `audit.yaml`, `fixture-freshness.yaml`,
`s3-minio.yaml` (all in full).
Manifests & config: root `Cargo.toml`, `crates/longtail-core/Cargo.toml`,
`crates/longtail-store/Cargo.toml`, `crates/longtail/Cargo.toml`,
`crates/longtail-cli/Cargo.toml`, `support/longtail-sys/Cargo.toml`,
`support/longtail-ffi/Cargo.toml`, `support/longtail-testkit/Cargo.toml`,
`support/longtail-bench/Cargo.toml`, `xtask/Cargo.toml`, `.cargo/config.toml`, `rustfmt.toml`,
`renovate.json`, `.gitignore`, `.gitmodules`, `Cargo.lock` (header + targeted).
Build/test infra: `xtask/src/main.rs` (full), `support/longtail-sys/build.rs` (full),
`test-data/mkdata.sh`, `test-data/mkdata.ps1`, `support/longtail-testkit/src/paths.rs`,
`fixtures/README.md`, `fixtures/manifest.json` (head).
Targeted source reads (test-strategy axis): `crates/longtail/src/lib.rs:100-148`,
`crates/longtail-store/src/uri.rs:185-225`, `crates/longtail-cli/src/main.rs:495-510`,
`crates/longtail-cli/src/progress.rs` (grep + line 80),
`support/longtail-testkit/tests/downsync_three_way.rs:168-257` (excerpts), test-file heads for
`#![cfg(unix)]` / feature gating.
Docs: `docs/switchover-checklist.md` (targeted), `docs/rust-port.md:80-100`, `readme.md`
(targeted), `CLAUDE.md`.
Wave-1 reviews: findings indexes of `01`/`02`/`03`/`07` + R1 §upgrade/rollback + FMT-007,
R2 §fuzz + §recommendation + ALG-01, R3 Deliverable 4, R7 Named deliverable 3.
Evidence pack: `MANIFEST.md`, `00-scope.txt`, `01-clippy-pure.json` (size/warning check),
`01b-clippy-ws.txt` (via MANIFEST caveat), `03-test.txt` (targeted), `05-tree.txt` (dup scan),
`06b-audit.txt`, `07-unused.txt`, `07b-machete.txt` (via MANIFEST), `08-doc.txt` (via MANIFEST),
`10-release.txt` (tail), `12-loc.txt`, `13-fixtures.txt`, `15-coverage/summary.txt` (targeted
rows + TOTAL), `16-deny.txt` (head), `18-semver.txt`.
