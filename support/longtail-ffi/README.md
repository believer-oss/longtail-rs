# longtail-ffi (LEGACY)

Safe-ish Rust wrappers over the longtail C library, driven through `longtail-sys`. This was the
original production code path before the pure-Rust rewrite (`crates/longtail-core`,
`longtail-store`, `longtail`, `longtail-cli`).

**It is retained for one purpose only: differential regression testing.** The `differential`
tests drive the same operations through both this C wrapper and the pure-Rust crates and
assert byte-identical results, so the C implementation remains the reference oracle. Nothing in
the default build depends on it.

It is **not a default workspace member** — building it compiles the C library through
`longtail-sys`, which needs the git submodule checked out and a C toolchain. Reach it explicitly
with `-p longtail-ffi`, or via the differential feature
(`cargo test -p longtail-testkit --features differential`).

This is not a build dependency of anything shipped, so keeping it costs a CI job and a submodule.
It earns that by being the only independent check that the port still agrees with the C
implementation — while stores are written by both, that check is worth more than the crate costs.
