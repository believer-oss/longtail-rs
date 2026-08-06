//! Interop gate ⑧ — Rust and spawned golongtail **concurrently** upsync
//! disjoint content to one minio store; the converged store index must contain
//! every block, and each implementation must then downsync the *other's*
//! version. Env-gated (skips cleanly without a minio endpoint) and needs the
//! pinned golongtail binary cached (`xtask fetch-golongtail`).
//!
//! **Path-style caveat (proven in the manual smoke test):** golongtail never
//! sets `UsePathStyle`, so its AWS SDK addresses buckets virtual-host style
//! (`<bucket>.<host>`). Stock minio does not serve that; run minio with
//! `MINIO_DOMAIN=<host>` and set `LONGTAIL_TEST_S3_ENDPOINT` to a host that
//! resolves virtual-host names to the minio address (e.g.
//! `http://127.0.0.1.nip.io:PORT`).

// Unix-only: this drives a spawned golongtail binary, and the pinned build
// xtask fetches is Linux-only. The lane that runs these is Linux anyway.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use longtail_testkit::paths::golongtail_binary;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_longtail")
}

struct Minio {
    endpoint: String,
    bucket: String,
    access: String,
    secret: String,
    region: String,
}

fn minio_env() -> Option<Minio> {
    let Ok(endpoint) = std::env::var("LONGTAIL_TEST_S3_ENDPOINT") else {
        // See `s3_spec.rs`: skipping is the default, but where these are meant
        // to run, LONGTAIL_TEST_S3_REQUIRED turns a skip into a failure.
        assert!(
            std::env::var_os("LONGTAIL_TEST_S3_REQUIRED").is_none(),
            "LONGTAIL_TEST_S3_REQUIRED is set but LONGTAIL_TEST_S3_ENDPOINT is not — \
             the mixed-writer test would have skipped and reported success"
        );
        return None;
    };
    Some(Minio {
        endpoint,
        bucket: std::env::var("LONGTAIL_TEST_S3_BUCKET")
            .unwrap_or_else(|_| "longtail-interop".into()),
        access: std::env::var("LONGTAIL_TEST_S3_ACCESS_KEY")
            .unwrap_or_else(|_| "minioadmin".into()),
        secret: std::env::var("LONGTAIL_TEST_S3_SECRET_KEY")
            .unwrap_or_else(|_| "minioadmin".into()),
        region: std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".into()),
    })
}

fn write_tree(dir: &Path, files: &[(&str, &str)]) {
    for (rel, content) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }
}

fn unique_prefix() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("interop-test/{nanos}")
}

fn aws_env(cmd: &mut Command, m: &Minio) {
    cmd.env("AWS_ACCESS_KEY_ID", &m.access)
        .env("AWS_SECRET_ACCESS_KEY", &m.secret)
        .env("AWS_REGION", &m.region);
}

/// Concurrent Rust + golongtail upsync of disjoint content → cross-downsync.
#[test]
fn gate8_mixed_writer_minio() {
    let Some(m) = minio_env() else {
        eprintln!("skipping gate8: LONGTAIL_TEST_S3_ENDPOINT not set");
        return;
    };
    let Some(go) = golongtail_binary() else {
        eprintln!("skipping gate8: golongtail binary not cached");
        return;
    };

    let tmp = tempfile::tempdir().unwrap();
    let rust_src = tmp.path().join("rust-src");
    let go_src = tmp.path().join("go-src");
    write_tree(
        &rust_src,
        &[
            ("rust-file.txt", &"rust-unique-".repeat(400)),
            ("rdir/r.txt", &"rust-nested-".repeat(250)),
        ],
    );
    write_tree(
        &go_src,
        &[
            ("go-file.txt", &"go-unique-".repeat(400)),
            ("gdir/g.txt", &"go-nested-".repeat(250)),
        ],
    );

    let prefix = unique_prefix();
    let store = format!("s3://{}/{}/store", m.bucket, prefix);
    let rust_lvi = format!("s3://{}/{}/rust.lvi", m.bucket, prefix);
    let go_lvi = format!("s3://{}/{}/go.lvi", m.bucket, prefix);

    // Concurrent upsync (disjoint content) to the same store.
    let mut rust_up = Command::new(bin());
    rust_up.args([
        "upsync",
        "--storage-uri",
        &store,
        "--source-path",
        rust_src.to_str().unwrap(),
        "--target-path",
        &rust_lvi,
        "--s3-endpoint-resolver-uri",
        &m.endpoint,
    ]);
    aws_env(&mut rust_up, &m);
    let mut go_up = Command::new(&go);
    go_up.args([
        "upsync",
        "--storage-uri",
        &store,
        "--source-path",
        go_src.to_str().unwrap(),
        "--target-path",
        &go_lvi,
        "--s3-endpoint-resolver-uri",
        &m.endpoint,
    ]);
    aws_env(&mut go_up, &m);

    let rh = rust_up.spawn().expect("spawn rust upsync");
    let gh = go_up.spawn().expect("spawn go upsync");
    let ro = rh.wait_with_output().unwrap();
    let goo = gh.wait_with_output().unwrap();
    assert!(
        ro.status.success(),
        "rust upsync failed: {}",
        String::from_utf8_lossy(&ro.stderr)
    );
    assert!(
        goo.status.success(),
        "go upsync failed: {}",
        String::from_utf8_lossy(&goo.stderr)
    );

    // Cross-downsync: each reads the OTHER's version from the converged store.
    let r_reads_g = tmp.path().join("r-reads-g");
    let g_reads_r = tmp.path().join("g-reads-r");

    let mut rd = Command::new(bin());
    rd.args([
        "downsync",
        "--storage-uri",
        &store,
        "--source-path",
        &go_lvi,
        "--target-path",
        r_reads_g.to_str().unwrap(),
        "--s3-endpoint-resolver-uri",
        &m.endpoint,
        "--no-cache-target-index",
    ]);
    aws_env(&mut rd, &m);
    assert!(
        rd.status().unwrap().success(),
        "rust downsync of go version failed"
    );

    let mut gd = Command::new(&go);
    gd.args([
        "downsync",
        "--storage-uri",
        &store,
        "--source-path",
        &rust_lvi,
        "--target-path",
        g_reads_r.to_str().unwrap(),
        "--s3-endpoint-resolver-uri",
        &m.endpoint,
        "--no-cache-target-index",
    ]);
    aws_env(&mut gd, &m);
    assert!(
        gd.status().unwrap().success(),
        "go downsync of rust version failed"
    );

    assert!(
        trees_equal(&r_reads_g, &go_src),
        "Rust must reproduce golongtail's tree"
    );
    assert!(
        trees_equal(&g_reads_r, &rust_src),
        "golongtail must reproduce Rust's tree"
    );
}

/// Shallow content compare of two trees (relative file paths + bytes).
fn trees_equal(a: &Path, b: &Path) -> bool {
    let fa = collect(a);
    let fb = collect(b);
    if fa.len() != fb.len() {
        return false;
    }
    for (rel, bytes) in &fa {
        match fb.iter().find(|(r, _)| r == rel) {
            Some((_, other)) if other == bytes => {}
            _ => return false,
        }
    }
    true
}

fn collect(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, out);
            } else if p.is_file() {
                let rel = p.strip_prefix(root).unwrap().to_path_buf();
                out.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
}
