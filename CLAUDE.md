# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository purpose

A pure-Rust implementation of [longtail](https://github.com/DanEngelbrecht/longtail) and the
parts of [golongtail](https://github.com/DanEngelbrecht/golongtail) we use. It is byte-compatible
with the C library's on-disk formats (`.lvi` version indexes, `.lsi` store indexes, `.lsb` stored
blocks) and with existing S3 and local-filesystem stores, and it ships a golongtail-compatible
CLI. The production use case is a `get`/`downsync` (download) path driven from a Tauri app, plus a
CI/CD pipeline that calls the CLI.

Read first for background: **`docs/rust-port.md`** (why and how the port was done — architecture,
deliberate divergences, upstream findings, safety, roadmap) and **`docs/format-spec.md`** (the
authoritative on-disk format spec). Compatibility with real stores is the paramount constraint.

## Workspace layout

This is a **virtual cargo workspace** (root `Cargo.toml` has `[workspace]`/`[workspace.package]`,
no `[package]`):

```
crates/
  longtail-core/    # SYNC, no tokio: on-disk format codecs, FileInfos scan/sort, HPCDC chunker
                    #   (+ FastCDC bench-only), hash (blake3/blake2s/meow-parse), compression
                    #   registry, store-index algebra, version build/diff.
  longtail-store/   # tokio-native: blob stores (fs/mem/S3), the RemoteBlockStore actor,
                    #   Cache/Compress block-store decorators, optimistic store-index sync.
  longtail/         # facade: downsync/upsync ops, ChangeVersion2 apply, error tree, progress.
  longtail-cli/     # clap binary `longtail-rs` — the golongtail CLI replacement.
support/
  longtail-sys/     # LEGACY. Raw bindgen bindings over the vendored C sources.
  longtail-ffi/     # LEGACY. Safe-ish wrappers over the C library.
                    #   sys + ffi exist ONLY as the reference oracle for differential
                    #   regression testing — nothing on a build path depends on them.
  longtail-testkit/ # corpus gen, fixture manifest, differential helpers. The pure part is always
                    #   available; C-backed helpers are behind the `differential` feature.
  longtail-bench/   # criterion micro-benches + the e2e harness. publish = false.
xtask/              # fixture gen/verify tooling.
fixtures/           # committed golden fixtures + manifest.json (see fixtures/README.md).
docs/               # rust-port.md, format-spec.md.
test-data/          # mkdata scripts for CI and the differential tests.
```

`crates/*`, `support/longtail-testkit`, `support/longtail-bench`, and `xtask` are pure Rust and
form `default-members`, so a plain `cargo build`/`cargo test` needs no network access and never
touches the native library. `support/longtail-sys` and `support/longtail-ffi` are workspace
members but **not** default members (they build the C library) — reach them with
`-p longtail-sys` / `-p longtail-ffi`, or `--workspace` to include everything.

`support/longtail-sys/build.rs` defaults to the `vendored` feature: it compiles the C sources from
the git submodule under `support/longtail-sys/longtail/` with `cc`, then runs `bindgen` over its
headers. So the submodule must be checked out and a C toolchain present — that is what the
differential CI jobs need, and the only thing that needs it. `--no-default-features` switches to a
pinned prebuilt download instead (`UPSTREAM_VERSION`, the per-OS SHA256 constants, and
`support/longtail-sys/scripts/get-hashes-for-upstream.sh`); nothing here builds it that way. To
move to a different upstream C version, bump the submodule.

## Common commands

- Build (pure crates only, no network): `cargo build`
- Build everything, including the native legacy pair: `cargo build --workspace`
- Tests: `cargo test`. Full: `cargo test --workspace`. The default run depends only on
  the committed `fixtures/` (no `mkdata` needed).
- Differential tests (regression against the C library): `cargo test -p longtail-testkit --features
  differential`. This needs the submodule + a C toolchain and, for the three-way e2e, the pinned
  golongtail binary — run `test-data/mkdata.sh` (or `.ps1`) and `cargo run -p xtask --
  fetch-golongtail` first.
- Verify fixtures: `cargo run -p xtask -- verify-fixtures`.
- Lint: `cargo +nightly clippy --workspace --all-targets` (CI also runs the
  `--features longtail-core/fastcdc` variant).
- Format: `cargo +nightly fmt --all` (max width 100, see `rustfmt.toml`); CI enforces
  `--check` on nightly.
- CLI: `cargo run -p longtail-cli -- <subcommand>` — `get`, `downsync`, `upsync`, `put`, `ls`,
  `print-version`, `validate-version`, `prune-store{,-index,-blocks}`, `clone-store`, `cp`, and the
  other store-maintenance commands. The built binary is `longtail-rs`.

### Benchmarks

Benches never run in CI — the numbers are machine-dependent — so CI only proves they still
compile. To measure, build the release binaries first, then run them; the e2e harness spawns the
three implementations itself and needs the pinned golongtail binary and a C toolchain.

```sh
cargo run -p xtask -- fetch-golongtail                 # pinned golongtail, once
cargo build --release -p longtail-cli
cargo build --release -p longtail-bench --features differential --bin e2e --bin ffi-driver
cargo build --release -p longtail-bench --features fastcdc --bin dedup

cargo bench -p longtail-bench --features differential,fastcdc   # micro-benches
target/release/e2e     # end-to-end matrix; LONGTAIL_BENCH_DATA_SIZE_MB, ITERS,
                       # COLD_WORKERS and MAIN_WORKERS size the run
target/release/dedup   # HPCDC vs FastCDC dedup ratios, reusing the e2e dataset
```

Keep a run's numbers with the machine spec that produced them; a figure without its hardware
cannot be compared against anything later. Bench reports are working documents, not repo content —
the durable conclusions belong in `docs/rust-port.md` §Performance.

## CI

`rust.yaml` gates every PR: the test job on Linux + Windows (build, test, `verify-fixtures`,
the `--no-default-features` feature matrix, and rustdoc with `-D warnings`), nightly clippy/fmt,
and miri over `longtail-core`. Windows runs its own clippy, because `cfg(windows)` code is
compiled by nothing else.

The heavier suites carry their own triggers so they run when they can tell you something:
`differential.yaml` (regression against the retained C implementation, Ubuntu + Windows) and
`s3-minio.yaml` (blob sync and the mixed Rust/Go writer interop, against a minio container) run
on a schedule and on PRs that touch what they guard — manifests, lockfile, or the code underneath
them. `audit.yaml` runs `rustsec/audit-check`. `fixture-freshness.yaml` is schedule-only.
`release-readiness.yaml` builds and tests the shipped `[profile.release]` on both platforms; it is
off the PR gate because release-mode test compilation is slow, and opts in per PR via the
`release-tests` label.

Both differential jobs need the `longtail-sys` submodule checked out and a C toolchain; every
other job sets `submodules: false` and must keep working without one.

## Runtime configuration

- The `downsync`/`upsync` operations take a caller-supplied worker/thread count (the successor to
  the old `LONGTAIL_WORKER_COUNT`): it sizes the per-operation rayon pool for CPU work and bounds
  the store's `Semaphore` for remote block I/O.
- The public API is plain `async fn` on the caller's ambient tokio runtime (the library never
  builds its own runtime); `*_blocking` convenience wrappers exist for the CLI and simple callers.
- AWS credentials are supplied as a provider/`Client` (never a snapshot), so the SDK's lazy
  credentials cache refreshes mid-operation on long transfers.
- Logging is `tracing`-based; `RUST_LOG` drives an `EnvFilter`.

## Safety

Every default-member library and binary target is `#![forbid(unsafe_code)]`. The small set of
justified `unsafe` (test `umask` pins, the testkit differential shim, the bench e2e measurement
binary) is inventoried in `docs/rust-port.md` (§Safety posture).

## Conventions

- `longtail-core` is I/O-free and tokio-free; local-disk and async concerns live in
  `longtail-store` and the `longtail` facade.
- On-disk formats use unaligned byte-cursor reads/writes only — never `&[u64]` casts (packed
  `u64` arrays may land on 4-byte boundaries; see `docs/format-spec.md`).
- Keep code comments about the code and its design; cite the upstream C/Go source (commit-pinned
  when a comment carries a line number) where it clarifies a decision.
