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
  longtail-cli/     # clap binary — the golongtail CLI replacement.
support/
  longtail-sys/     # LEGACY. Raw bindgen bindings to the prebuilt C library.
  longtail-ffi/     # LEGACY. Safe-ish wrappers over the C library.
                    #   sys + ffi are retained ONLY as the reference oracle for differential
                    #   regression testing; scheduled for deletion after one release cycle.
  longtail-testkit/ # corpus gen, fixture manifest, differential helpers. The pure part is always
                    #   available; C-backed helpers are behind the `differential` feature.
  longtail-bench/   # criterion micro-benches + the e2e harness. publish = false.
xtask/              # fixture gen/verify tooling.
fixtures/           # committed golden fixtures + manifest.json (see fixtures/README.md).
docs/               # rust-port.md, format-spec.md, bench-<date>.md, switchover-checklist.md.
test-data/          # CI + differential-lane mkdata scripts.
```

`crates/*`, `support/longtail-testkit`, `support/longtail-bench`, and `xtask` are pure Rust and
form `default-members`, so a plain `cargo build`/`cargo test` needs no network access and never
touches the native library. `support/longtail-sys` and `support/longtail-ffi` are workspace
members but **not** default members (they need a prebuilt native-lib download) — reach them with
`-p longtail-sys` / `-p longtail-ffi`, or `--workspace` to include everything.

`support/longtail-sys/build.rs` downloads a pinned prebuilt native library for the target platform
(see `UPSTREAM_VERSION` and the per-OS SHA256 constants) and runs `bindgen`; a git submodule under
`support/longtail-sys/longtail/` supplies headers. This matters only to the legacy differential
lane. To bump the upstream C library, refresh the SHA256 constants with
`support/longtail-sys/scripts/get-hashes-for-upstream.sh`.

## Common commands

- Build (pure crates only, no network): `cargo build`
- Build everything, including the native legacy pair: `cargo build --workspace`
- Tests (pure lane): `cargo test`. Full: `cargo test --workspace`. The pure lane depends only on
  the committed `fixtures/` (no `mkdata` needed).
- Differential lane (regression against the C library): `cargo test -p longtail-testkit --features
  differential`. This needs the prebuilt native lib and, for the three-way e2e, the pinned
  golongtail binary — run `test-data/mkdata.sh` (or `.ps1`) and `cargo run -p xtask --
  fetch-golongtail` first.
- Verify fixtures: `cargo run -p xtask -- verify-fixtures`.
- Lint: `cargo +nightly clippy --workspace --all-targets` (CI also runs the
  `--features longtail-core/fastcdc` variant).
- Format: `cargo +nightly fmt --all` (max width 100, see `rustfmt.toml`); CI enforces
  `--check` on nightly.
- CLI: `cargo run -p longtail-cli -- <subcommand>` — `get`, `downsync`, `upsync`, `put`, `ls`,
  `print-version`, `validate-version`, `prune-store{,-index,-blocks}`, `clone-store`, `cp`, and the
  other store-maintenance commands.

## CI

`.github/workflows/rust.yaml`: the **pure lane** (Ubuntu + Windows), **clippy/fmt** (nightly), and
**miri** (over `longtail-core`) gate every PR. The **differential lanes** (Ubuntu + Windows,
regression against the retained C implementation) run on a **weekly schedule and manual dispatch
only** — they cost a native-lib build per run and only guard against the C library, which no
longer changes per-PR. `.github/workflows/fixture-freshness.yaml` and
`.github/workflows/s3-minio.yaml` (the S3/minio blob-sync and mixed-writer interop jobs) are
scheduled. `.github/workflows/audit.yaml` runs `rustsec/audit-check` daily and on Cargo.{toml,lock}
changes.

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
