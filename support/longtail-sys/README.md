# longtail-sys (LEGACY)

Raw `bindgen`-generated FFI bindings to the prebuilt [longtail](https://github.com/DanEngelbrecht/longtail)
C library. `build.rs` downloads the pinned native library for the target platform (see
`UPSTREAM_VERSION` and the per-OS SHA256 constants), extracts headers from the `longtail/` git
submodule, and runs `bindgen`.

**Retained for one purpose only: differential regression testing** of the pure-Rust workspace
against the C implementation. Only `longtail-ffi` (also legacy) depends on it; nothing in the
default build does.

It is **not a default workspace member** — a plain `cargo build`/`cargo test` never touches it or
the native library. Reach it explicitly with `-p longtail-sys`, or via `--workspace`.

Scheduled for deletion after the pure-Rust code has run in production for one release cycle. When
updating the upstream C library, refresh the SHA256 constants in `build.rs` with
`scripts/get-hashes-for-upstream.sh`.
