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
use aws_sdk_s3::error::{DisplayErrorContext, ProvideErrorMetadata, SdkError};
use aws_sdk_s3::primitives::ByteStream;
use bytes::Bytes;

use longtail_core::{StoreIndex, StoreIndexReader};

use super::{BlobClient, BlobObject, BlobProperties, BlobStore};
use crate::error::StoreError;

/// S3/STS error codes that mean "the store rejected our credentials". Matched
/// case-sensitively against `ProvideErrorMetadata::code`.
fn is_auth_code(code: Option<&str>) -> bool {
    matches!(
        code,
        Some(
            "AccessDenied"
                | "AccessDeniedException"
                | "InvalidAccessKeyId"
                | "SignatureDoesNotMatch"
                | "ExpiredToken"
                | "InvalidToken"
                | "TokenRefreshRequired"
                | "AuthorizationHeaderMalformed"
                | "InvalidSecurity"
                | "AccountProblem"
        )
    )
}

/// Classify an `aws-sdk-s3` `SdkError` into the appropriate [`StoreError`],
/// preserving the full SDK cause chain via [`DisplayErrorContext`] (the bare
/// `Display` is terse — e.g. `"service error"` / `"dispatch failure"` — and
/// drops the S3 error code). `op` labels the operation (e.g. `"get_object …"`).
///
/// A credentials rejection (S3 auth error code) → [`StoreError::NotAuthorized`];
/// a transport/timeout failure (dispatch/timeout/response) →
/// [`StoreError::Network`]; anything else → [`StoreError::Backend`]. This lets a
/// consumer branch on the failure class without string-matching.
fn map_sdk_err<E, R>(op: impl std::fmt::Display, e: SdkError<E, R>) -> StoreError
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
    R: std::fmt::Debug + Send + Sync + 'static,
{
    let detail = format!("{op}: {}", DisplayErrorContext(&e));
    match &e {
        SdkError::ServiceError(ctx) if is_auth_code(ctx.err().code()) => {
            StoreError::NotAuthorized(detail)
        }
        SdkError::TimeoutError(_) | SdkError::DispatchFailure(_) | SdkError::ResponseError(_) => {
            StoreError::Network(detail)
        }
        _ => StoreError::Backend(detail),
    }
}

/// How to obtain an S3 client. Highest precedence first: an explicit `client`,
/// then a `sdk_config`, then piecewise `credentials_provider`/`region`, each
/// overlaid with `endpoint_url` / `transfer_acceleration` / `force_path_style`.
#[derive(Clone)]
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
    /// S3 Transfer Acceleration. **Defaults to `false`** — a deliberate
    /// divergence from the legacy FFI `get_with_cache` path, which hardcoded it
    /// on. `false` is the safer default (acceleration needs the bucket opted in
    /// and adds cost); callers that want the old throughput set it explicitly.
    pub transfer_acceleration: bool,
    /// Force path-style addressing (required by minio and most S3-compatibles).
    pub force_path_style: bool,
    /// SDK stalled-stream protection. **Defaults to `true`.** When on, a GET
    /// body stream that delivers ~no data for the SDK grace period (5 s) errors
    /// out (→ [`StoreError::Network`]) so the read-retry ladder can recover it,
    /// rather than hanging indefinitely. This was historically disabled to dodge
    /// a pre-GA SDK bug that false-tripped on slow *consumers* (smithy-rs#3485,
    /// fixed April 2024, well before our pinned `aws-smithy-runtime`); the escape
    /// hatch remains so a caller can turn it back off without a code change.
    pub stalled_stream_protection: bool,
    /// Ceiling on a single object read; see [`super::DEFAULT_MAX_BLOB_BYTES`].
    /// The object's own `Content-Length` decides nothing — it is supplied by the
    /// same store the bytes come from.
    pub max_read_bytes: u64,
}

impl Default for S3Options {
    fn default() -> S3Options {
        S3Options {
            max_read_bytes: super::DEFAULT_MAX_BLOB_BYTES,
            client: None,
            sdk_config: None,
            credentials_provider: None,
            region: None,
            endpoint_url: None,
            transfer_acceleration: false,
            force_path_style: false,
            stalled_stream_protection: true,
        }
    }
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
            .field("stalled_stream_protection", &self.stalled_stream_protection)
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
                let mut loader = aws_config::defaults(BehaviorVersion::latest());
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
        // Authoritative regardless of any inherited sdk_config setting: on by
        // default (a stalled upstream errors → retry ladder), off only when the
        // caller opts out via `S3Options`.
        let ssp = if self.options.stalled_stream_protection {
            StalledStreamProtectionConfig::enabled().build()
        } else {
            StalledStreamProtectionConfig::disabled()
        };
        builder = builder.stalled_stream_protection(ssp);
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
            max_read_bytes: self.options.max_read_bytes,
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
    max_read_bytes: u64,
}

#[async_trait]
impl BlobClient for S3BlobClient {
    async fn new_object(&self, path: &str) -> Result<Box<dyn BlobObject>, StoreError> {
        Ok(Box::new(S3BlobObject {
            client: self.client.clone(),
            bucket: self.bucket.clone(),
            prefix: self.prefix.clone(),
            key: format!("{}{}", self.prefix, path),
            max_read_bytes: self.max_read_bytes,
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
                .map_err(|e| map_sdk_err("list_objects_v2", e))?;
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
                    // The S3 contract says `IsTruncated=true` always carries a
                    // continuation token, so this only happens against a
                    // non-conformant S3-compatible endpoint — which a custom
                    // endpoint URL makes reachable. Breaking here would return
                    // `Ok` with a short list, and a short list of `store_*.lsi`
                    // shards is a narrowed store index: blocks that exist become
                    // invisible, surfacing later as "chunk not in the store
                    // index" on a download, or as blocks deleted by prune.
                    return Err(StoreError::Backend(format!(
                        "list_objects_v2 for `{full_prefix}` reported truncation with no \
                         continuation token after {} objects; refusing to return a partial \
                         listing",
                        out.len()
                    )));
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
    max_read_bytes: u64,
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
                Err(map_sdk_err(format_args!("head_object {}", self.key), e))
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
                return Err(map_sdk_err(format_args!("get_object {}", self.key), e));
            }
        };
        // Refuse before collecting: `collect()` buffers the whole body in one
        // allocation, so an oversized object has to be rejected on the declared
        // length rather than after it is already resident. The length is only a
        // hint — it comes from the same store — so the collected result is
        // re-checked below.
        if let Some(len) = resp.content_length()
            && len as u64 > self.max_read_bytes
        {
            return Err(StoreError::Backend(format!(
                "{} declares {len} bytes, over the {}-byte read ceiling",
                self.key, self.max_read_bytes
            )));
        }
        let data = resp
            .body
            .collect()
            .await
            // A body-stream read failure is a transport interruption mid-download
            // (not an SdkError, so `map_sdk_err` doesn't apply) — classify as
            // Network and keep the full cause via DisplayErrorContext.
            .map_err(|e| {
                StoreError::Network(format!(
                    "read body {}: {}",
                    self.key,
                    DisplayErrorContext(&e)
                ))
            })?;
        // `AggregatedBytes::to_vec` collects the segments in a single copy;
        // `into_bytes().to_vec()` would concatenate to a `Bytes` first, then copy
        // again — one buffer-sized allocation saved per shard/block read.
        let out = data.to_vec();
        if out.len() as u64 > self.max_read_bytes {
            return Err(StoreError::Backend(format!(
                "{} delivered {} bytes, over the {}-byte read ceiling",
                self.key,
                out.len(),
                self.max_read_bytes
            )));
        }
        Ok(out)
    }

    /// Streamed: `Content-Length` is the bound and the body is fed to the decoder
    /// chunk by chunk, so a gigabyte-scale store index never exists twice. No
    /// read ceiling — see the trait method.
    async fn read_store_index(&self) -> Result<StoreIndex, StoreError> {
        let resp = match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                if let Some(svc) = e.as_service_error()
                    && svc.is_no_such_key()
                {
                    return Err(StoreError::NotFound(self.key.clone()));
                }
                return Err(map_sdk_err(format_args!("get_object {}", self.key), e));
            }
        };
        // The length the decoder is held to. A body that then delivers more or
        // fewer bytes than this fails in the decoder rather than being trusted.
        let len = resp.content_length().unwrap_or_default().max(0) as u64;
        if len == 0 {
            return Err(StoreError::NotFound(self.key.clone()));
        }
        let mut reader = StoreIndexReader::new(len);
        let mut body = resp.body;
        while let Some(chunk) = body.try_next().await.map_err(|e| {
            StoreError::Network(format!(
                "read body {}: {}",
                self.key,
                DisplayErrorContext(&e)
            ))
        })? {
            reader.feed(&chunk)?;
        }
        Ok(reader.finish()?)
    }

    async fn write(&mut self, data: Bytes) -> Result<bool, StoreError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&self.key)
            // `ByteStream::from(Bytes)` takes ownership with no copy (the old
            // `&[u8]` signature forced a `to_vec()` of the whole body here — a
            // full extra copy of the serialized store index on every flush).
            .body(ByteStream::from(data))
            .send()
            .await
            .map_err(|e| map_sdk_err(format_args!("put_object {}", self.key), e))?;
        Ok(true)
    }

    async fn delete(&mut self) -> Result<(), StoreError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
            .map_err(|e| map_sdk_err(format_args!("delete_object {}", self.key), e))?;
        Ok(())
    }

    fn name(&self) -> String {
        let key = self.key.strip_prefix('/').unwrap_or(&self.key);
        let _ = &self.prefix;
        format!("s3://{}/{}", self.bucket, key)
    }
}
