use std::io::Write;

use aws_sdk_s3::{config::StalledStreamProtectionConfig, Client as S3Client};

use crate::{BlobClient, BlobObject, BlobStore};

#[derive(Debug, Clone, Default)]
pub struct S3Options {
    endpoint_resolver_uri: Option<String>,
    s3_transfer_accel: Option<bool>,
}

impl S3Options {
    pub fn new(endpoint_resolver_uri: Option<String>, s3_transfer_accel: Option<bool>) -> Self {
        Self {
            endpoint_resolver_uri,
            s3_transfer_accel,
        }
    }
}

#[derive(Debug, Clone)]
pub struct S3BlobStore {
    bucket_name: String,
    prefix: String,
    options: Option<S3Options>,
}

impl S3BlobStore {
    pub fn new(bucket_name: &str, prefix: &str, options: Option<S3Options>) -> Self {
        S3BlobStore {
            bucket_name: bucket_name.to_string(),
            prefix: prefix.to_string(),
            options,
        }
    }
}

// FIXME: Disabling stalled stream protection is a workaround until we plumb in
// better handling. We currently get parallel downloads by virtue of the
// longtail JobAPI, which is set as the workers argument to
// commands.rs:downsync. Ideally I think we would handle the
// ThroughputBelowMinimum error by scaling down the concurrency of the S3
// client, but that does not appear to be possible with the JobAPI at runtime.
// We would also need to handle retrying the download, since I don't
// believe there is any retry logic in longtail, but more research is needed. A
// better architecture would be to run our own event loop for the S3 client and
// only pass messages from the longtail JobAPI threads. This would allow us to
// handle the download concurrency and retries ourselves.
//
// In the mean time, we should disable the stalled stream protection to let the
// download complete.
impl BlobStore for S3BlobStore {
    fn new_client<'a>(&self) -> Result<Box<dyn BlobClient + 'a>, Box<dyn std::error::Error>> {
        let region_provider = aws_config::meta::region::RegionProviderChain::default_provider()
            .or_else(aws_config::Region::new("us-east-1"));
        let mut shared_config = aws_config::defaults(aws_config::BehaviorVersion::v2025_01_17())
            .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
            .region(region_provider);

        if let Some(options) = &self.options {
            if let Some(uri) = &options.endpoint_resolver_uri {
                shared_config = shared_config.endpoint_url(uri.clone());
            }
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let sdk_config = rt.block_on(async { shared_config.load().await });
        let mut service_config = aws_sdk_s3::config::Builder::from(&sdk_config);

        if let Some(options) = &self.options {
            if let Some(accel) = options.s3_transfer_accel {
                tracing::debug!("Setting s3 transfer acceleration: {}", accel);
                service_config = service_config.accelerate(accel);
            } else {
                tracing::debug!("Not using s3 transfer acceleration");
            }
        }

        let config = service_config.build();
        let s3_client = S3Client::from_conf(config);
        Ok(Box::new(S3BlobClient {
            s3_client,
            store: self.clone(),
        }))
    }

    fn get_string(&self) -> String {
        format!("s3://{0}/{1}", self.bucket_name, self.prefix)
    }
}

#[derive(Debug, Clone)]
struct S3BlobClient {
    s3_client: S3Client,
    store: S3BlobStore,
}

impl BlobClient for S3BlobClient {
    // New objects are always rooted unter the storages prefix, but allow the prefix
    // to be stripped if it's passed in.
    fn new_object(
        &self,
        object_key: String,
    ) -> Result<Box<dyn BlobObject + '_>, Box<dyn std::error::Error>> {
        if object_key.starts_with(&self.store.prefix) {
            Ok(Box::new(S3BlobObject {
                client: self,
                object_key,
            }))
        } else {
            let prefix = self.store.prefix.clone();
            let prefix = prefix.strip_suffix('/').unwrap_or(&prefix);
            let object_key = object_key.strip_prefix('/').unwrap_or(&object_key);
            let object_key = format!("{prefix}/{object_key}");
            Ok(Box::new(S3BlobObject {
                client: self,
                object_key,
            }))
        }
    }

    fn get_objects(
        &self,
        path_prefix: String,
    ) -> Result<Vec<crate::BlobProperties>, Box<dyn std::error::Error>> {
        let bucket_name = self.store.bucket_name.clone();
        let path_prefix = format!("{}{}", self.store.prefix.clone(), path_prefix);
        tracing::debug!("Listing objects: [{}] [{}]", bucket_name, path_prefix);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let inner = rt.block_on(
            self.s3_client
                .list_objects_v2()
                .bucket(bucket_name)
                .prefix(path_prefix)
                .send(),
        )?;
        let mut ret = Vec::<crate::BlobProperties>::new();
        for object in inner.contents() {
            let key = object.key.clone().unwrap_or_default();
            let size = object.size.unwrap_or_default();
            ret.push(crate::BlobProperties {
                size: size as usize,
                name: key,
            });
        }
        Ok(ret)
    }

    fn supports_locking(&self) -> bool {
        false
    }

    fn get_string(&self) -> String {
        self.store.get_string()
    }

    fn close(&self) {}
}

#[derive(Debug)]
struct S3BlobObject<'a> {
    client: &'a S3BlobClient,
    object_key: String,
}

impl BlobObject for S3BlobObject<'_> {
    fn exists(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let bucket_name = self.client.store.bucket_name.clone();
        let object_key = self.object_key.clone();
        tracing::debug!(
            "Checking object exists: [{}] [{}] [{}]",
            bucket_name,
            self.client.store.prefix,
            object_key
        );

        let inner = rt.block_on(
            self.client
                .s3_client
                .head_object()
                .bucket(bucket_name)
                .key(object_key)
                .send(),
        );
        if let Err(e) = inner {
            match e.as_service_error() {
                Some(err) => {
                    if err.is_not_found() {
                        return Ok(false);
                    }
                }
                None => {
                    return Err(Box::new(e));
                }
            }
        }
        Ok(true)
    }
    fn lock_write_version(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(false)
    }
    fn read(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        tracing::debug!(
            "Reading object from s3: [{}] [{}]",
            self.client.store.bucket_name,
            self.object_key
        );
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let mut inner = rt
            .block_on(
                self.client
                    .s3_client
                    .get_object()
                    .bucket(self.client.store.bucket_name.clone())
                    .key(self.object_key.clone())
                    .send(),
            )
            .inspect_err(|e| tracing::debug!("Error reading!:{:?}", e))?;
        let mut buf = Vec::<u8>::new();
        while let Some(bytes) = rt.block_on(inner.body.try_next())? {
            buf.write_all(&bytes)?;
        }
        Ok(buf)
    }
    fn write(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let _inner = rt.block_on(
            self.client
                .s3_client
                .put_object()
                .bucket(self.client.store.bucket_name.clone())
                .key(self.object_key.clone())
                .body(data.to_vec().into())
                .send(),
        )?;
        Ok(())
    }
    fn delete(&self) -> Result<(), Box<dyn std::error::Error>> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        let _inner = rt.block_on(
            self.client
                .s3_client
                .delete_object()
                .bucket(self.client.store.bucket_name.clone())
                .key(self.object_key.clone())
                .send(),
        )?;
        Ok(())
    }
    fn get_string(&self) -> String {
        let bucket = self.client.store.bucket_name.clone();
        // Key already includes the prefix
        let key = self.object_key.clone();
        let key = key.strip_prefix('/').unwrap_or(&key);
        format!("s3://{bucket}/{key}")
    }
}

#[cfg(test)]
mod tests {
    use longtail_sys::Longtail_StoredBlock;
    use tracing::info;

    use super::*;
    use crate::{
        create_block_store_for_uri, AccessType, AsyncGetStoredBlockAPI,
        AsyncGetStoredBlockAPIProxy, BikeshedJobAPI, BlobStore, StoredBlock,
    };

    static BUCKET: &str = "build-artifacts20230504001207614000000001";
    static PREFIX: &str = "cmtest";

    #[test]
    #[ignore]
    fn test_blob_store_strings() {
        let store = S3BlobStore::new("bucket", "prefix", None);
        let client = store.new_client().unwrap();
        assert_eq!(client.get_string(), "s3://bucket/prefix");
        let object = client.new_object("object".to_string()).unwrap();
        assert_eq!(object.get_string(), "s3://bucket/prefix/object");
    }

    #[test]
    #[ignore]
    fn test_blob_object_exists() {
        let store = S3BlobStore::new(BUCKET, PREFIX, None);
        let client = store.new_client().unwrap();
        let object = client
            .new_object("game-win64-test.json".to_string())
            .unwrap();
        assert!(object.exists().unwrap());
        let object = client.new_object("not-exist".to_string()).unwrap();
        assert!(!object.exists().unwrap());
    }

    #[test]
    #[ignore]
    fn test_blob_object_read_write() {
        let store = S3BlobStore::new(BUCKET, PREFIX, None);
        let client = store.new_client().unwrap();
        let object = client.new_object("testfile".to_string()).unwrap();
        let data = b"hello world";
        object.write(data).unwrap();
        let buf = object.read().unwrap();
        assert_eq!(&buf, data);
    }

    #[test]
    #[ignore]
    fn test_blob_object_delete() {
        let store = S3BlobStore::new(BUCKET, PREFIX, None);
        let client = store.new_client().unwrap();
        let object = client.new_object("testfile".to_string()).unwrap();
        let data = b"hello world";
        object.write(data).unwrap();
        assert!(object.exists().unwrap());
        object.delete().unwrap();
        assert!(!object.exists().unwrap());
    }

    #[test]
    #[ignore]
    fn test_s3blob_store_uri() {
        let _guard = crate::init_logging().unwrap();
        info!("test_s3blob_store_uri");
        #[derive(Debug)]
        struct TestGetStoredBlockCompletion {}
        impl AsyncGetStoredBlockAPI for TestGetStoredBlockCompletion {
            fn on_complete(&self, stored_block: *mut Longtail_StoredBlock, err: i32) {
                let stored_block = StoredBlock::new_from_lt(stored_block);
                info!("Stored block: {:?}", stored_block);
                info!("Error: {:?}", err);
            }
        }
        let mut async_complete_api =
            AsyncGetStoredBlockAPIProxy::new(Box::new(TestGetStoredBlockCompletion {}));
        info!("async_complete_api: {:?}", async_complete_api);

        let uri = format!("s3://{BUCKET}/{PREFIX}/store");
        let store = create_block_store_for_uri(
            &uri,
            None,
            Some(&BikeshedJobAPI::new(1, 1)),
            1,
            AccessType::ReadOnly,
            true,
            None,
        )
        .unwrap();
        let hash = 0xd1006a10ce6543f6;
        let result =
            crate::Blockstore::get_stored_block(&store, hash, &mut async_complete_api as *mut _);
        assert!(result.is_ok());
    }
}
