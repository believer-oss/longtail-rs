# longtail-ffi (LEGACY)

Safe-ish Rust wrappers over the longtail C library, driven through `longtail-sys`. This was the
original production code path before the pure-Rust rewrite (`crates/longtail-core`,
`longtail-store`, `longtail`, `longtail-cli`).

**It is retained for one purpose only: differential regression testing.** The `differential`
test lane drives the same operations through both this C wrapper and the pure-Rust crates and
asserts byte-identical results, so the C implementation remains the reference oracle. Nothing in
the default build depends on it.

It is **not a default workspace member** — building it requires the prebuilt native library that
`longtail-sys`'s `build.rs` downloads. Reach it explicitly with `-p longtail-ffi`, or via the
differential feature (`cargo test -p longtail-testkit --features differential`).

Scheduled for deletion after the pure-Rust code has run in production for one release cycle.
