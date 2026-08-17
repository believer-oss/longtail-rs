//! `BlobStore` semantics ported from golongtail's
//! `longtailstorelib/blobStore_test.go` and `longtailstorelib/fsstore_test.go`.
//!
//! mem/fs cases run everywhere (no native lib, no network). The two
//! `create_*_blob_store_from_uri` cases that merely check the store name are
//! pure construction (no network), so they run too; the S3-backed
//! *behavioral* cases live in `s3_spec.rs` under the env-gated job.
//!
//! GCS and Azure backends are intentionally omitted (out of scope for this port).

use bytes::Bytes;
use longtail_store::blob::{BlobStore, MemBlobStore};
use longtail_store::create_blob_store_for_uri;

// --- longtailstorelib/blobStore_test.go (mem/fs/uri) ---

/// Source: blobStore_test.go::TestCreateStoreAndClient — creating a mem blob
/// store and a client succeeds and the client closes cleanly.
#[tokio::test]
async fn create_store_and_client() {
    let store = MemBlobStore::new("the_path", true);
    let _client = store.new_client().await.expect("new_client");
    // Client "close" == drop.
}

/// Source: blobStore_test.go::TestListObjectsInEmptyStore — listing objects in
/// a freshly created (empty) store returns an empty set, not an error.
#[tokio::test]
async fn list_objects_in_empty_store() {
    let store = MemBlobStore::new("the_path", true);
    let client = store.new_client().await.unwrap();
    let objects = client.get_objects("").await.expect("get_objects");
    assert_eq!(objects.len(), 0);
    let obj = client.new_object("should-not-exist").await.unwrap();
    let err = obj.read().await.unwrap_err();
    assert!(err.is_not_found(), "expected NotFound, got {err:?}");
}

/// Source: blobStore_test.go::TestSingleObjectStore — write one object, read it
/// back with identical bytes; the object is reported as existing.
#[tokio::test]
async fn single_object_store() {
    let store = MemBlobStore::new("the_path", true);
    let client = store.new_client().await.unwrap();
    let mut obj = client.new_object("my-fine-object.txt").await.unwrap();
    assert!(!obj.exists().await.unwrap());
    let content = b"the content of the object";
    assert!(obj.write(Bytes::from_static(content)).await.unwrap());
    let data = obj.read().await.unwrap();
    assert_eq!(data, content);
    obj.delete().await.unwrap();
}

/// Source: blobStore_test.go::TestDeleteObject — after deleting an object it no
/// longer exists and is not returned by a subsequent list.
#[tokio::test]
async fn delete_object() {
    let store = MemBlobStore::new("the_path", true);
    let client = store.new_client().await.unwrap();
    let mut obj = client.new_object("my-fine-object.txt").await.unwrap();
    obj.write(Bytes::from_static(b"the content of the object"))
        .await
        .unwrap();
    obj.delete().await.unwrap();
    assert!(!obj.exists().await.unwrap());
}

/// Source: blobStore_test.go::TestListObjects — multiple written objects are all
/// returned by list, with correct names and sizes.
#[tokio::test]
async fn list_objects() {
    let store = MemBlobStore::new("the_path", true);
    let client = store.new_client().await.unwrap();
    for name in [
        "my-fine-object1.txt",
        "my-fine-object2.txt",
        "my-fine-object3.txt",
    ] {
        let mut obj = client.new_object(name).await.unwrap();
        obj.write(Bytes::copy_from_slice(name.as_bytes()))
            .await
            .unwrap();
    }
    let objects = client.get_objects("").await.unwrap();
    assert_eq!(objects.len(), 3);
    for o in objects {
        let obj = client.new_object(&o.name).await.unwrap();
        let data = obj.read().await.unwrap();
        assert_eq!(data, o.name.as_bytes());
        assert_eq!(o.size as usize, o.name.len());
    }
}

/// Source: blobStore_test.go::TestGenerationWrite — the optimistic-locking
/// write loop. Reproduces the full mem CAS sequence.
#[tokio::test]
async fn generation_write() {
    let store = MemBlobStore::new("the_path", true);
    let client = store.new_client().await.unwrap();
    let mut obj = client.new_object("my-fine-object.txt").await.unwrap();
    let c1 = b"the content of the object1";
    let c2 = b"the content of the object2";
    let c3 = b"the content of the object3";

    // Lock while absent → exists=false; first write wins; second (no re-lock) loses.
    assert!(!obj.lock_write_version().await.unwrap());
    assert!(obj.write(Bytes::from_static(c1)).await.unwrap());
    assert!(!obj.write(Bytes::from_static(c2)).await.unwrap());

    // Two handles lock the same key; only the first write wins.
    let mut obj2 = client.new_object("my-fine-object.txt").await.unwrap();
    assert!(obj.lock_write_version().await.unwrap());
    assert!(obj2.lock_write_version().await.unwrap());
    assert!(obj.write(Bytes::from_static(c2)).await.unwrap());
    assert!(!obj2.write(Bytes::from_static(c3)).await.unwrap());

    // Delete needs the current generation lock.
    assert!(obj.delete().await.is_err());
    obj.lock_write_version().await.unwrap();
    obj.delete().await.unwrap();
}

/// Source: blobStore_test.go::TestCreateFSBlobStoreFromURI — a `fsblob://` URI
/// resolves to a filesystem-backed blob store.
#[tokio::test]
async fn create_fs_blob_store_from_uri() {
    let store = create_blob_store_for_uri("fsblob://my-blob-store").unwrap();
    assert!(store.name().contains("my-blob-store"));
}

/// Source: blobStore_test.go::TestCreateS3BlobStoreFromURI — an `s3://` URI
/// resolves to an S3-backed blob store (bucket + prefix parsed). Pure
/// construction, no network.
#[cfg(feature = "s3")]
#[tokio::test]
async fn create_s3_blob_store_from_uri() {
    let store = create_blob_store_for_uri("s3://my-blob-store").unwrap();
    assert!(store.name().contains("my-blob-store"));
}

/// Source: blobStore_test.go::TestCreateFileBlobStoreFromURI — a `file://` URI
/// resolves to the local filesystem block store.
#[tokio::test]
async fn create_file_blob_store_from_uri() {
    let store = create_blob_store_for_uri("file://my-blob-store").unwrap();
    assert!(store.name().contains("my-blob-store"));
}

/// Source: blobStore_test.go::TestCreateFileBlobStoreFromPath — a bare path
/// (no scheme) resolves to the local filesystem block store.
#[tokio::test]
async fn create_file_blob_store_from_path() {
    let store = create_blob_store_for_uri("c:\\temp\\my-blob-store").unwrap();
    assert!(store.name().contains("my-blob-store"));
}

// --- longtailstorelib/fsstore_test.go ---

/// Source: fsstore_test.go::TestFSBlobStore — basic write against a real fs
/// blob store.
#[tokio::test]
async fn fs_blob_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = longtail_store::FsBlobStore::new(dir.path(), true);
    let client = store.new_client().await.unwrap();
    let mut obj = client.new_object("test.txt").await.unwrap();
    assert!(obj.write(Bytes::from_static(b"apa")).await.unwrap());
}

/// Source: fsstore_test.go::TestListObjectsInEmptyFSStore — empty fs store lists
/// empty, read of a missing object is NotFound, no stray files created.
#[tokio::test]
async fn list_objects_in_empty_fs_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = longtail_store::FsBlobStore::new(dir.path(), true);
    let client = store.new_client().await.unwrap();
    let objects = client.get_objects("").await.unwrap();
    assert_eq!(objects.len(), 0);
    let obj = client.new_object("should-not-exist").await.unwrap();
    assert!(obj.read().await.unwrap_err().is_not_found());
    // No stray files.
    assert_eq!(client.get_objects("").await.unwrap().len(), 0);
}

/// Source: fsstore_test.go::TestFSBlobStoreVersioning — the fs backend honors
/// the generation/metageneration optimistic lock through `.gen`/`._lck` files.
#[tokio::test]
async fn fs_blob_store_versioning() {
    let dir = tempfile::tempdir().unwrap();
    let store = longtail_store::FsBlobStore::new(dir.path(), true);
    let client = store.new_client().await.unwrap();
    let mut obj = client.new_object("test.txt").await.unwrap();
    let _ = obj.delete().await; // ignore (does not exist yet)

    assert!(!obj.lock_write_version().await.unwrap());
    assert!(obj.write(Bytes::from_static(b"apa")).await.unwrap());
    assert!(!obj.write(Bytes::from_static(b"skapa")).await.unwrap());
    assert!(obj.lock_write_version().await.unwrap());
    assert!(obj.write(Bytes::from_static(b"skapa")).await.unwrap());
    obj.read().await.unwrap();
    assert!(obj.delete().await.is_err());
    assert!(obj.lock_write_version().await.unwrap());
    obj.delete().await.unwrap();
}

/// Source: fsstore_test.go::TestFSBlobStoreVersioningStressTest — many
/// concurrent writers contend on one key via the optimistic lock; every write
/// lands (no lost updates), leaving the sorted union.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_blob_store_versioning_stress() {
    let dir = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(longtail_store::FsBlobStore::new(dir.path(), true));

    // 5 batches of 5 concurrent writers (mirrors the Go batching).
    let mut number = 1;
    for _ in 0..5 {
        let mut handles = Vec::new();
        for _ in 0..5 {
            let store = store.clone();
            let n = number;
            number += 1;
            handles.push(tokio::spawn(async move {
                write_a_number_with_retry(n, store).await
            }));
        }
        for h in handles {
            h.await.unwrap().expect("write_a_number_with_retry");
        }
    }

    let client = store.new_client().await.unwrap();
    let obj = client.new_object("test.txt").await.unwrap();
    let data = obj.read().await.unwrap();
    let text = String::from_utf8(data).unwrap();
    let lines: Vec<&str> = text.split('\n').collect();
    assert_eq!(lines.len(), 25);
    for (i, line) in lines.iter().enumerate() {
        assert_eq!(*line, format!("{:05}", i + 1));
    }
}

/// Port of `writeANumberWithRetry` (blobStore_test.go:133).
async fn write_a_number_with_retry(
    number: i32,
    store: std::sync::Arc<longtail_store::FsBlobStore>,
) -> Result<(), longtail_store::StoreError> {
    let client = store.new_client().await?;
    let mut object = client.new_object("test.txt").await?;
    loop {
        let exists = object.lock_write_version().await?;
        let mut slice: Vec<String> = Vec::new();
        if exists {
            let data = object.read().await?;
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            slice = String::from_utf8(data)
                .unwrap()
                .split('\n')
                .map(|s| s.to_string())
                .collect();
        }
        slice.push(format!("{number:05}"));
        slice.sort();
        let new_data = slice.join("\n");
        if object
            .write(Bytes::copy_from_slice(new_data.as_bytes()))
            .await?
        {
            return Ok(());
        }
    }
}

/// Source: fsstore_test.go::TestFSGetObjects — recursive listing returns every
/// written object; a nested prefix filters correctly.
#[tokio::test]
async fn fs_get_objects() {
    let dir = tempfile::tempdir().unwrap();
    let store = longtail_store::FsBlobStore::new(dir.path(), false);
    let client = store.new_client().await.unwrap();
    let files = [
        "first.txt",
        "second.txt",
        "third.txt",
        "fourth.txt",
        "nested/first_nested.txt",
        "nested/second_nested.txt",
    ];
    for name in files {
        let mut obj = client.new_object(name).await.unwrap();
        obj.write(Bytes::copy_from_slice(name.as_bytes()))
            .await
            .unwrap();
    }
    let blobs = client.get_objects("").await.unwrap();
    assert_eq!(blobs.len(), files.len());
    let nested = client.get_objects("nest").await.unwrap();
    assert_eq!(nested.len(), 2);
}
