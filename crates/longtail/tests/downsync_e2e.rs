//! Pure-Rust facade downsync end-to-end against committed fixture stores,
//! compared to the committed tree manifests. Linux-only; skipped under miri.

#![cfg(unix)]

use std::path::PathBuf;

use longtail::{DownsyncOptions, downsync};
use longtail_testkit::paths::fixtures_dir;
use longtail_testkit::tree_manifest::TreeManifest;

fn pin_umask() {
    unsafe {
        libc::umask(0o022);
    }
}

fn manifest(name: &str) -> TreeManifest {
    let p = fixtures_dir().join("manifests").join(name);
    TreeManifest::from_json(&std::fs::read_to_string(&p).unwrap()).unwrap()
}

async fn run_downsync(source_lvi: PathBuf, store: PathBuf, target: PathBuf) {
    let mut opts = DownsyncOptions::new(
        vec![source_lvi.to_string_lossy().into_owned()],
        store.to_string_lossy().into_owned(),
        target.to_string_lossy().into_owned(),
    );
    opts.cache_target_index = false;
    downsync(opts).await.expect("downsync");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fresh_downsync_zoo_matches_manifest() {
    pin_umask();
    let fx = fixtures_dir();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    run_downsync(
        fx.join("stores/default/zoo.lvi"),
        fx.join("stores/default/store"),
        target.clone(),
    )
    .await;
    let got = TreeManifest::capture(&target).unwrap();
    got.compare(&manifest("zoo.json"), false)
        .expect("zoo tree matches manifest");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_v1_then_v2() {
    pin_umask();
    let fx = fixtures_dir();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    // v1 into empty target, then v2 over it (resume/upgrade).
    run_downsync(
        fx.join("stores/default/chain-v1.lvi"),
        fx.join("stores/default/store"),
        target.clone(),
    )
    .await;
    TreeManifest::capture(&target)
        .unwrap()
        .compare(&manifest("chain-v1.json"), false)
        .expect("v1 tree");
    run_downsync(
        fx.join("stores/default/chain-v2.lvi"),
        fx.join("stores/default/store"),
        target.clone(),
    )
    .await;
    TreeManifest::capture(&target)
        .unwrap()
        .compare(&manifest("chain-v2.json"), false)
        .expect("v2 tree after resume");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sharded_store_merge_on_read() {
    pin_umask();
    let fx = fixtures_dir();
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("out");
    run_downsync(
        fx.join("stores/sharded/version.lvi"),
        fx.join("stores/sharded"),
        target.clone(),
    )
    .await;
    TreeManifest::capture(&target)
        .unwrap()
        .compare(&manifest("sharded-union.json"), false)
        .expect("sharded union tree");
}
