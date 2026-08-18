# longtail-sys (LEGACY)

Raw `bindgen`-generated FFI bindings to the [longtail](https://github.com/DanEngelbrecht/longtail)
C library. By default (`vendored`, on) `build.rs` compiles the C sources straight out of the
`longtail/` git submodule with `cc` and runs `bindgen` over its headers — so the submodule must be
checked out, and a C toolchain is required. With `--no-default-features` it instead downloads a
pinned prebuilt library (`UPSTREAM_VERSION` and the per-OS SHA256 constants); nothing in this
repository builds it that way.

**Retained for one purpose only: differential regression testing** of the pure-Rust workspace
against the C implementation. Only `longtail-ffi` (also legacy) depends on it; nothing in the
default build does.

It is **not a default workspace member** — a plain `cargo build`/`cargo test` never touches it or
the native library. Reach it explicitly with `-p longtail-sys`, or via `--workspace`.

To move to a different upstream C version, bump the submodule; the SHA256 constants and
`scripts/get-hashes-for-upstream.sh` apply only to the non-default prebuilt path.
