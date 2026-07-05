//! In-memory blob store — a faithful port of `memblobstore.go`.
//!
//! Used as a test double and to exercise the sync module's shard/merge-on-read
//! path without minio (constructed with `supports_locking = false`). The
//! generation CAS matches `memBlobObject` byte-for-byte
//! (`TestGenerationWrite`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::{BlobClient, BlobObject, BlobProperties, BlobStore};
use crate::error::StoreError;

#[derive(Debug, Clone)]
struct MemBlob {
    generation: i64,
    data: Vec<u8>,
}

#[derive(Debug, Default)]
struct MemState {
    blobs: HashMap<String, MemBlob>,
}

/// A shareable in-memory blob store. Clones share the same backing map (like
/// clients of one Go `memBlobStore`).
#[derive(Debug, Clone)]
pub struct MemBlobStore {
    state: Arc<Mutex<MemState>>,
    supports_locking: bool,
}

impl MemBlobStore {
    /// `NewMemBlobStore(prefix, supports_locking)`. The prefix is accepted for
    /// parity but — exactly like Go — is not applied to object paths.
    pub fn new(_prefix: &str, supports_locking: bool) -> MemBlobStore {
        MemBlobStore {
            state: Arc::new(Mutex::new(MemState::default())),
            supports_locking,
        }
    }
}

#[async_trait]
impl BlobStore for MemBlobStore {
    async fn new_client(&self) -> Result<Box<dyn BlobClient>, StoreError> {
        Ok(Box::new(MemBlobClient {
            state: self.state.clone(),
            supports_locking: self.supports_locking,
        }))
    }

    fn name(&self) -> String {
        "memstore".to_string()
    }
}

struct MemBlobClient {
    state: Arc<Mutex<MemState>>,
    supports_locking: bool,
}

#[async_trait]
impl BlobClient for MemBlobClient {
    async fn new_object(&self, path: &str) -> Result<Box<dyn BlobObject>, StoreError> {
        Ok(Box::new(MemBlobObject {
            state: self.state.clone(),
            path: path.to_string(),
            locked_generation: None,
        }))
    }

    async fn get_objects(&self, prefix: &str) -> Result<Vec<BlobProperties>, StoreError> {
        let state = self.state.lock().unwrap();
        let mut out = Vec::new();
        for (key, blob) in state.blobs.iter() {
            if key.starts_with(prefix) {
                out.push(BlobProperties {
                    name: key.clone(),
                    size: blob.data.len() as u64,
                });
            }
        }
        Ok(out)
    }

    fn supports_locking(&self) -> bool {
        self.supports_locking
    }

    fn name(&self) -> String {
        "memstore".to_string()
    }
}

struct MemBlobObject {
    state: Arc<Mutex<MemState>>,
    path: String,
    /// `None` = never locked; `Some(-1)` = locked while absent; `Some(g)` =
    /// locked at generation `g` (memblobstore.go `lockedGeneration`).
    locked_generation: Option<i64>,
}

#[async_trait]
impl BlobObject for MemBlobObject {
    async fn exists(&self) -> Result<bool, StoreError> {
        let state = self.state.lock().unwrap();
        Ok(state.blobs.contains_key(&self.path))
    }

    async fn lock_write_version(&mut self) -> Result<bool, StoreError> {
        let state = self.state.lock().unwrap();
        match state.blobs.get(&self.path) {
            Some(blob) => {
                self.locked_generation = Some(blob.generation);
                Ok(true)
            }
            None => {
                self.locked_generation = Some(-1);
                Ok(false)
            }
        }
    }

    async fn read(&self) -> Result<Vec<u8>, StoreError> {
        let state = self.state.lock().unwrap();
        match state.blobs.get(&self.path) {
            Some(blob) => Ok(blob.data.clone()),
            None => Err(StoreError::NotFound(self.path.clone())),
        }
    }

    async fn write(&mut self, data: &[u8]) -> Result<bool, StoreError> {
        let mut state = self.state.lock().unwrap();
        let exists = state.blobs.contains_key(&self.path);

        if let Some(locked) = self.locked_generation {
            if exists {
                let current = state.blobs.get(&self.path).unwrap().generation;
                if current != locked {
                    return Ok(false);
                }
            } else if locked != -1 {
                return Ok(false);
            }
        }

        if !exists {
            state.blobs.insert(
                self.path.clone(),
                MemBlob {
                    generation: 0,
                    data: data.to_vec(),
                },
            );
        } else {
            let blob = state.blobs.get_mut(&self.path).unwrap();
            blob.data = data.to_vec();
            blob.generation += 1;
        }
        Ok(true)
    }

    async fn delete(&mut self) -> Result<(), StoreError> {
        let mut state = self.state.lock().unwrap();
        if let Some(locked) = self.locked_generation {
            match state.blobs.get(&self.path) {
                None => return Ok(()),
                Some(blob) => {
                    if blob.generation != locked {
                        return Err(StoreError::GenerationMismatch(self.path.clone()));
                    }
                }
            }
        }
        state.blobs.remove(&self.path);
        Ok(())
    }

    fn name(&self) -> String {
        format!("memstore/{}", self.path)
    }
}
