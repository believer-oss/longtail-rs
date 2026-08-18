//! Corpus generation, fixture manifests, tree-manifest capture, and
//! differential-test helpers shared across the pure-Rust longtail port's test
//! suites.
//!
//! Everything except the [`differential`] module builds without the native
//! library (no `differential` feature), so a default test run needs no C
//! toolchain and no prebuilt-lib download.
#![forbid(unsafe_code)]

pub mod boundary;
pub mod corpus;
pub mod data;
pub mod fixture_manifest;
pub mod paths;
pub mod tree_manifest;

#[cfg(feature = "differential")]
pub mod differential;
