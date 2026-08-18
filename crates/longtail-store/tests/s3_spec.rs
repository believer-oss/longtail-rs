//! S3-backed blob/sync behavioral tests + the credential-refresh proof.
//!
//! - [`fake_expiry_credentials_refresh`] runs **always** (no network): it wires
//!   a custom, always-expired credentials provider into an S3 client whose HTTP
//!   layer is `aws-smithy-http-client`'s `StaticReplayClient`, and asserts the
//!   provider is re-consulted after expiry across requests *without rebuilding
//!   the client* — the launcher's mid-operation refresh requirement, made
//!   testable (the mid-operation credential-refresh requirement).
//!   `StaticReplayClient` keeps the full orchestrator + SigV4 signing on-path
//!   (preferred over operation-level mocks that can short-circuit before
//!   identity resolution).
//! - The remaining tests are **env-gated** on `LONGTAIL_TEST_S3_ENDPOINT`
//!   (minio); they skip cleanly (not fail) when it is absent.

#![cfg(feature = "s3")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use bytes::Bytes;

use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_credential_types::provider::future::ProvideCredentials as ProvideCredentialsFuture;
use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
use aws_sdk_s3::config::Region;
use aws_smithy_http_client::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;

use longtail_store::blob::{BlobStore, S3BlobStore, S3Options};

/// A credentials provider that hands out already-expired credentials and counts
/// how many times it is consulted.
#[derive(Debug)]
struct CountingProvider {
    count: Arc<AtomicUsize>,
}

impl ProvideCredentials for CountingProvider {
    fn provide_credentials<'a>(&'a self) -> ProvideCredentialsFuture<'a>
    where
        Self: 'a,
    {
        self.count.fetch_add(1, Ordering::SeqCst);
        // Expiry an hour in the past → the SDK's lazy cache must refresh on
        // every request.
        let creds = Credentials::new(
            "AKIDTEST",
            "SECRETTEST",
            Some("SESSIONTOKEN".to_string()),
            Some(SystemTime::now() - Duration::from_secs(3600)),
            "counting-provider",
        );
        ProvideCredentialsFuture::ready(Ok(creds))
    }
}

#[tokio::test]
async fn fake_expiry_credentials_refresh() {
    let count = Arc::new(AtomicUsize::new(0));
    let provider = SharedCredentialsProvider::new(CountingProvider {
        count: count.clone(),
    });

    let replay = StaticReplayClient::new(vec![
        ReplayEvent::new(
            http::Request::builder()
                .uri("http://s3.local/bucket/prefix/a")
                .body(SdkBody::empty())
                .unwrap(),
            http::Response::builder()
                .status(200)
                .header("content-length", "5")
                .body(SdkBody::from("hello"))
                .unwrap(),
        ),
        ReplayEvent::new(
            http::Request::builder()
                .uri("http://s3.local/bucket/prefix/b")
                .body(SdkBody::empty())
                .unwrap(),
            http::Response::builder()
                .status(200)
                .header("content-length", "5")
                .body(SdkBody::from("world"))
                .unwrap(),
        ),
    ]);

    let conf = aws_sdk_s3::config::Builder::new()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(provider)
        .http_client(replay.clone())
        .endpoint_url("http://s3.local")
        .force_path_style(true)
        .build();
    let client = aws_sdk_s3::Client::from_conf(conf);

    // Inject the fully-built client (holds the provider, never a snapshot).
    let store = S3BlobStore::new(
        "bucket",
        "prefix",
        S3Options {
            client: Some(client),
            ..Default::default()
        },
    );
    let blob_client = store.new_client().await.unwrap();

    // Two independent reads, no client rebuild between them.
    let o1 = blob_client.new_object("a").await.unwrap();
    let d1 = o1.read().await.expect("read a");
    assert_eq!(d1, b"hello");

    let o2 = blob_client.new_object("b").await.unwrap();
    let d2 = o2.read().await.expect("read b");
    assert_eq!(d2, b"world");

    let consulted = count.load(Ordering::SeqCst);
    assert!(
        consulted >= 2,
        "provider should be re-consulted per request after expiry (got {consulted})"
    );
}

// --- env-gated minio tests ------------------------------------------------

fn minio_options() -> Option<(String, S3Options)> {
    let endpoint = std::env::var("LONGTAIL_TEST_S3_ENDPOINT").ok()?;
    let bucket =
        std::env::var("LONGTAIL_TEST_S3_BUCKET").unwrap_or_else(|_| "longtail-test".into());
    let access =
        std::env::var("LONGTAIL_TEST_S3_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".into());
    let secret =
        std::env::var("LONGTAIL_TEST_S3_SECRET_KEY").unwrap_or_else(|_| "minioadmin".into());
    let creds = Credentials::new(access, secret, None, None, "minio-test");
    let opts = S3Options {
        credentials_provider: Some(SharedCredentialsProvider::new(creds)),
        region: Some("us-east-1".into()),
        endpoint_url: Some(endpoint),
        force_path_style: true,
        ..Default::default()
    };
    Some((bucket, opts))
}

/// A unique prefix per run so repeated runs against a shared minio don't collide.
fn unique_prefix(tag: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("longtail-s3test/{tag}-{nanos}")
}

#[tokio::test]
async fn s3_blob_round_trip() {
    let Some((bucket, opts)) = minio_options() else {
        eprintln!("skipping s3_blob_round_trip: LONGTAIL_TEST_S3_ENDPOINT not set");
        return;
    };
    let prefix = unique_prefix("blob");
    let store = S3BlobStore::new(&bucket, &prefix, opts);
    let client = store.new_client().await.unwrap();

    let mut obj = client.new_object("hello.txt").await.unwrap();
    assert!(!obj.exists().await.unwrap());
    assert!(obj.write(Bytes::from_static(b"hello s3")).await.unwrap());
    assert!(obj.exists().await.unwrap());
    assert_eq!(obj.read().await.unwrap(), b"hello s3");

    let objs = client.get_objects("").await.unwrap();
    assert!(objs.iter().any(|o| o.name == "hello.txt"));

    obj.delete().await.unwrap();
    assert!(!obj.exists().await.unwrap());
}

/// Source: remotestore_test.go::TestS3StoreIndexSync — lockless shard merge
/// under concurrent writers (S3 never locks).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn s3_store_index_sync() {
    use longtail_core::{BlockIndex, StoreIndex};
    use longtail_store::{add_to_remote_store_index, read_merged_store_index};

    let Some((bucket, opts)) = minio_options() else {
        eprintln!("skipping s3_store_index_sync: LONGTAIL_TEST_S3_ENDPOINT not set");
        return;
    };
    let prefix = unique_prefix("sync");
    let store: Arc<dyn BlobStore> = Arc::new(S3BlobStore::new(&bucket, &prefix, opts));

    let worker_count: u8 = 8;
    let blocks_per_worker: u8 = 4;
    let mut handles = Vec::new();
    for n in 0..worker_count {
        let store = store.clone();
        let seed_base = blocks_per_worker * n;
        handles.push(tokio::spawn(async move {
            let client = store.new_client().await.unwrap();
            let mut blocks = Vec::new();
            for i in 0..blocks_per_worker {
                let seed = (seed_base + i) as u64;
                blocks.push(BlockIndex {
                    block_hash: (seed << 16) + 21412151,
                    hash_identifier: 997,
                    tag: 2,
                    chunk_hashes: vec![(seed << 8) + 1, (seed << 8) + 2],
                    chunk_sizes: vec![10, 20],
                });
            }
            let add = StoreIndex::from_block_indexes(&blocks).unwrap();
            add_to_remote_store_index(&*client, &add).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let client = store.new_client().await.unwrap();
    // Consolidate shards (lockless).
    add_to_remote_store_index(&*client, &StoreIndex::empty(0))
        .await
        .unwrap();
    let index = read_merged_store_index(&*client).await.unwrap();
    let expected = worker_count as usize * blocks_per_worker as usize;
    assert_eq!(index.block_hashes.len(), expected);
    let unique: std::collections::HashSet<u64> = index.block_hashes.iter().copied().collect();
    assert_eq!(unique.len(), expected);
}
