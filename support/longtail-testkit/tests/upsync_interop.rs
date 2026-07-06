//! Stage 7 interop gate ⑥ (both directions) over an fs store — the "fs
//! chaos-style variant" of the mixed-writer interop:
//!
//! - Rust `upsync` (in-process facade) → spawned pinned golongtail `downsync`
//!   → tree identical to source.
//! - golongtail `upsync` → Rust `downsync` (in-process facade) → tree identical.
//!
//! Runs when `xtask fetch-golongtail` has cached the binary; skips cleanly
//! otherwise. Gate ⑧ (concurrent mixed writers to one minio store) is a separate
//! env-gated / manual run (see `rust-port-7-results.md`).

#![cfg(all(feature = "differential", unix))]

use std::path::Path;
use std::process::Command;

use longtail::{DownsyncOptions, UpsyncOptions, downsync_blocking, upsync_blocking};
use longtail_testkit::paths::golongtail_binary;
use longtail_testkit::tree_manifest::TreeManifest;

fn pin_umask() {
    // SAFETY: single libc call; the subprocess inherits the fixture-gen umask.
    unsafe {
        libc::umask(0o022);
    }
}

fn make_src(dir: &Path) {
    let write = |rel: &str, content: &str| {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    };
    write("readme.txt", &"interop-readme-".repeat(50));
    write("data/blob.bin", &"interop-blob-payload-".repeat(200));
    write("data/nested/deep.txt", &"deep-".repeat(80));
    write("dup.txt", &"interop-blob-payload-".repeat(200)); // duplicate content
}

fn capture(p: &Path) -> TreeManifest {
    TreeManifest::capture(p).unwrap()
}

/// Rust upsync → golongtail downsync → identical tree.
#[test]
fn gate6_rust_upsync_golongtail_downsync() {
    let Some(go) = golongtail_binary() else {
        eprintln!("skipping gate6 (rust→go): golongtail binary not cached");
        return;
    };
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_src(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");

    let opts = UpsyncOptions::new(
        src.to_str().unwrap(),
        store.to_str().unwrap(),
        lvi.to_str().unwrap(),
    );
    upsync_blocking(opts).expect("rust upsync");

    let out = tmp.path().join("out");
    let status = Command::new(&go)
        .args([
            "downsync",
            "--storage-uri",
            store.to_str().unwrap(),
            "--source-path",
            lvi.to_str().unwrap(),
            "--target-path",
            out.to_str().unwrap(),
            "--no-cache-target-index",
        ])
        .status()
        .expect("spawn golongtail");
    assert!(
        status.success(),
        "golongtail downsync of Rust-written store failed"
    );

    capture(&out)
        .compare(&capture(&src), false)
        .expect("gate6 rust→go: tree mismatch");
}

/// golongtail upsync → Rust downsync → identical tree.
#[test]
fn gate6_golongtail_upsync_rust_downsync() {
    let Some(go) = golongtail_binary() else {
        eprintln!("skipping gate6 (go→rust): golongtail binary not cached");
        return;
    };
    pin_umask();
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    make_src(&src);
    let store = tmp.path().join("store");
    let lvi = tmp.path().join("v.lvi");

    let status = Command::new(&go)
        .args([
            "upsync",
            "--storage-uri",
            store.to_str().unwrap(),
            "--source-path",
            src.to_str().unwrap(),
            "--target-path",
            lvi.to_str().unwrap(),
        ])
        .status()
        .expect("spawn golongtail");
    assert!(status.success(), "golongtail upsync failed");

    let out = tmp.path().join("out");
    let mut d = DownsyncOptions::new(
        vec![lvi.to_str().unwrap().to_string()],
        store.to_str().unwrap(),
        out.to_str().unwrap(),
    );
    d.cache_target_index = false;
    downsync_blocking(d).expect("rust downsync of go-written store");

    capture(&out)
        .compare(&capture(&src), false)
        .expect("gate6 go→rust: tree mismatch");
}
