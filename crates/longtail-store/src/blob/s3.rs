//! S3 blob store — a port of `s3Store.go` on `aws-sdk-s3`.
//!
//! **Credentials are held as a provider/client, never a snapshot:**
//! [`S3Options`] accepts a caller-built
//! [`aws_sdk_s3::Client`], an [`aws_config::SdkConfig`], or a
//! [`SharedCredentialsProvider`] (+ region/endpoint fallbacks). The SDK's lazy
//! credentials cache then refreshes mid-operation on long transfers — proven by
//! the fake-expiry test.
//!
//! `supports_locking()` is hardwired `false` (s3Store.go:106); the store-index
//! sync uses the lockless shard/merge-on-read flavor here.
//!
//! Divergence from Go, documented: `get_objects` **paginates** the
//! `ListObjectsV2` result (Go reads only the first page, s3Store.go:92-103) — a
//! correctness fix for stores with > 1000 objects (Init rebuild).

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::StalledStreamProtectionConfig;

use super::{BlobClient, BlobObject, BlobProperties, BlobStore};
use crate::error::StoreError;

/// How to obtain an S3 client. Highest precedence first: an explicit `client`,
/// then a `sdk_config`, then piecewise `credentials_provider`/`region`, each
/// overlaid with `endpoint_url` / `transfer_acceleration` / `force_path_style`.
#[derive(Clone, Default)]
pub struct S3Options {
    /// A fully-built client (holds its own provider). Used verbatim.
    pub client: Option<Client>,
    /// A caller-loaded SDK config (holds its provider).
    pub sdk_config: Option<aws_config::SdkConfig>,
    /// A credentials provider to inject (never a static snapshot — the SDK's
    /// cache refreshes from it).
    pub credentials_provider: Option<SharedCredentialsProvider>,
    /// Region override (else the default region chain / `us-east-1`).
    pub region: Option<String>,
    /// Custom endpoint (S3-compatible stores, minio).
    pub endpoint_url: Option<String>,
    /// S3 Transfer Acceleration.
    pub transfer_acceleration: bool,
    /// Force path-style addressing (required by minio and most S3-compatibles).
    pub force_path_style: bool,
}

impl std::fmt::Debug for S3Options {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Options")
            .field("client", &self.client.is_some())
            .field("sdk_config", &self.sdk_config.is_some())
            .field("credentials_provider", &self.credentials_provider.is_some())
            .field("region", &self.region)
            .field("endpoint_url", &self.endpoint_url)
            .field("transfer_acceleration", &self.transfer_acceleration)
            .field("force_path_style", &self.force_path_style)
            .finish()
    }
}

/// An S3-backed blob store.
#[derive(Debug, Clone)]
pub struct S3BlobStore {
    bucket: String,
    /// Object-key prefix, ending in `/` when non-empty (matches Go).
    prefix: String,
    options: S3Options,
}

impl S3BlobStore {
    /// `NewS3BlobStore` — `prefix` is normalized to end in `/` when non-empty.
    pub fn new(bucket: impl Into<String>, prefix: &str, options: S3Options) -> S3BlobStore {
        let prefix = if prefix.is_empty() {
            String::new()
        } else {
            let trimmed = prefix.trim_start_matches('/');
            if trimmed.is_empty() {
                String::new()
            } else if trimmed.ends_with('/') {
                trimmed.to_string()
            } else {
                format!("{trimmed}/")
            }
        };
        S3BlobStore {
            bucket: bucket.into(),
            prefix,
            options,
        }
    }

    /// Parse an `s3://bucket/prefix` URI. Uses the default option set.
    pub fn from_uri(uri: &str) -> Result<S3BlobStore, StoreError> {
        Self::from_uri_with_options(uri, S3Options::default())
    }

    /// Parse an `s3://bucket/prefix` URI with caller-supplied options.
    pub fn from_uri_with_options(uri: &str, options: S3Options) -> Result<S3BlobStore, StoreError> {
        let rest = uri
            .strip_prefix("s3://")
            .ok_or_else(|| StoreError::InvalidUri {
                uri: uri.to_string(),
                reason: "expected s3:// scheme".into(),
            })?;
        let (bucket, prefix) = match rest.split_once('/') {
            Some((b, p)) => (b, p),
            None => (rest, ""),
        };
        if bucket.is_empty() {
            return Err(StoreError::InvalidUri {
                uri: uri.to_string(),
                reason: "empty bucket".into(),
            });
        }
        Ok(Self::new(bucket, prefix, options))
    }

    async fn build_client(&self) -> Client {
        if let Some(client) = &self.options.client {
            return client.clone();
        }
        let sdk_config = match &self.options.sdk_config {
            Some(cfg) => cfg.clone(),
            None => {
                let mut loader = aws_config::defaults(BehaviorVersion::latest())
                    .stalled_stream_protection(StalledStreamProtectionConfig::disabled());
                if let Some(provider) = &self.options.credentials_provider {
                    loader = loader.credentials_provider(provider.clone());
                }
                if let Some(region) = &self.options.region {
                    loader = loader.region(aws_config::Region::new(region.clone()));
                }
                if let Some(endpoint) = &self.options.endpoint_url {
                    loader = loader.endpoint_url(endpoint.clone());
                }
                loader.load().await
            }
        };
        let mut builder = aws_sdk_s3::config::Builder::from(&sdk_config);
        if let Some(provider) = &self.options.credentials_provider {
            builder = builder.credentials_provider(provider.clone());
        }
        if let Some(region) = &self.options.region {
            builder = builder.region(aws_config::Region::new(region.clone()));
        }
        if let Some(endpoint) = &self.options.endpoint_url {
            builder = builder.endpoint_url(endpoint.clone());
        }
        if self.options.force_path_style {
            builder = builder.force_path_style(true);
        }
        if self.options.transfer_acceleration {
            builder = builder.accelerate(true);
        }
        builder = builder.stalled_stream_protection(StalledStreamProtectionConfig::disabled());
        Client::from_conf(builder.build())
    }
}

#[async_trait]
impl BlobStore for S3BlobStore {
    async fn new_client(&self) -> Result<Box<dyn BlobClient>, StoreError> {
        let client = self.build_client().await;
        Ok(Box::new(S3BlobClient {
            client,
            bucket: self.bucket.clone(),
            prefix: self.prefix.clone(),
        }))
    }

    fn name(&self) -> String {
        format!("s3://{}/{}", self.bucket, self.prefix)
    }
}

#[derive(Clone)]
struct S3BlobClient {
    client: Client,
    bucket: String,
    prefix: String,
}

#[async_trait]
impl BlobClient for S3BlobClient {
    async fn new_object(&self, path: &str) -> Result<Box<dyn BlobObject>, StoreError> {
        Ok(Box::new(S3BlobObject {
            client: self.client.clone(),
            bucket: self.bucket.clone(),
            prefix: self.prefix.clone(),
            key: format!("{}{}", self.prefix, path),
        }))
    }

    async fn get_objects(&self, path_prefix: &str) -> Result<Vec<BlobProperties>, StoreError> {
        let full_prefix = format!("{}{}", self.prefix, path_prefix);
        let mut out = Vec::new();
        let mut continuation: Option<String> = None;
        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&full_prefix);
            if let Some(token) = &continuation {
                req = req.continuation_token(token);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| StoreError::Backend(format!("list_objects_v2: {e}")))?;
            for object in resp.contents() {
                let key = object.key().unwrap_or_default();
                let name = key.strip_prefix(&self.prefix).unwrap_or(key).to_string();
                out.push(BlobProperties {
                    size: object.size().unwrap_or_default() as u64,
                    name,
                });
            }
            if resp.is_truncated().unwrap_or(false) {
                continuation = resp.next_continuation_token().map(|s| s.to_string());
                if continuation.is_none() {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(out)
    }

    fn supports_locking(&self) -> bool {
        false
    }

    fn name(&self) -> String {
        format!("s3://{}/{}", self.bucket, self.prefix)
    }
}

struct S3BlobObject {
    client: Client,
    bucket: String,
    prefix: String,
    key: String,
}

#[async_trait]
impl BlobObject for S3BlobObject {
    async fn exists(&self) -> Result<bool, StoreError> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                if let Some(svc) = e.as_service_error()
                    && svc.is_not_found()
                {
                    return Ok(false);
                }
                Err(StoreError::Backend(format!(
                    "head_object {}: {e}",
                    self.key
                )))
            }
        }
    }

    async fn lock_write_version(&mut self) -> Result<bool, StoreError> {
        // S3 has no locking (s3Store.go:141).
        Ok(false)
    }

    async fn read(&self) -> Result<Vec<u8>, StoreError> {
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                if let Some(svc) = e.as_service_error()
                    && svc.is_no_such_key()
                {
                    return Err(StoreError::NotFound(self.key.clone()));
                }
                return Err(StoreError::Backend(format!("get_object {}: {e}", self.key)));
            }
        };
        let data = resp
            .body
            .collect()
            .await
            .map_err(|e| StoreError::Backend(format!("read body {}: {e}", self.key)))?;
        Ok(data.into_bytes().to_vec())
    }

    async fn write(&mut self, data: &[u8]) -> Result<bool, StoreError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .body(data.to_vec().into())
            .send()
            .await
            .map_err(|e| StoreError::Backend(format!("put_object {}: {e}", self.key)))?;
        Ok(true)
    }

    async fn delete(&mut self) -> Result<(), StoreError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
            .map_err(|e| StoreError::Backend(format!("delete_object {}: {e}", self.key)))?;
        Ok(())
    }

    fn name(&self) -> String {
        let key = self.key.strip_prefix('/').unwrap_or(&self.key);
        let _ = &self.prefix;
        format!("s3://{}/{}", self.bucket, key)
    }
}
