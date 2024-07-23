use std::{
    collections::HashMap,
    result::Result,
    sync::{Arc, Mutex},
};

use thiserror::Error;
use tracing::{debug, warn};

use crate::{BlobClient, BlobObject, BlobStore};

#[derive(Debug)]
pub struct MemBlob {
    pub generation: i32,
    pub path: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct MemBlobStore {
    pub blobs: Arc<Mutex<HashMap<String, MemBlob>>>,
    pub prefix: String,
    pub supports_locking: bool,
}

impl MemBlobStore {
    pub fn new(prefix: &str, supports_locking: bool) -> Self {
        MemBlobStore {
            blobs: Arc::new(Mutex::new(HashMap::new())),
            prefix: prefix.to_string(),
            supports_locking,
        }
    }
}

impl BlobStore for MemBlobStore {
    fn new_client<'a>(
        &self,
    ) -> Result<Box<dyn BlobClient + 'a>, Box<(dyn std::error::Error + 'static)>> {
        Ok(Box::new(MemBlobClient {
            store: self.clone(),
        }))
    }

    fn get_string(&self) -> String {
        "memstore".to_string()
    }
}

#[derive(Debug, Clone)]
pub struct MemBlobClient {
    pub store: MemBlobStore,
}

impl BlobClient for MemBlobClient {
    fn new_object(
        &self,
        path: String,
        // ) -> Result<MemBlobObject, Box<dyn std::error::Error + 'static>> {
    ) -> Result<Box<dyn BlobObject + '_>, Box<(dyn std::error::Error + 'static)>> {
        Ok(Box::new(MemBlobObject {
            client: self.clone(),
            path,
            locked_generation: None,
        }))
    }

    fn get_objects(
        &self,
        path_prefix: String,
    ) -> Result<Vec<crate::BlobProperties>, Box<dyn std::error::Error>> {
        let blobs = self.store.blobs.lock().unwrap();
        let mut ret = Vec::<crate::BlobProperties>::new();
        for (k, v) in blobs.iter() {
            if k.starts_with(&path_prefix) {
                ret.push(crate::BlobProperties {
                    size: v.data.len(),
                    name: k.clone(),
                });
            }
        }
        Ok(ret)
    }

    fn supports_locking(&self) -> bool {
        self.store.supports_locking
    }

    fn get_string(&self) -> String {
        self.store.get_string()
    }

    fn close(&self) {}
}

#[derive(Error, Debug)]
pub enum MemBlobError {
    #[error("Blob '{0}' not found")]
    BlobNotFound(String),

    #[error("Lock generation mismatch '{0}' != '{1}'")]
    LockGenerationMismatch(i32, i32),
}

pub struct MemBlobObject {
    pub client: MemBlobClient,
    pub path: String,
    pub locked_generation: Option<i32>,
}

impl BlobObject for MemBlobObject {
    fn exists(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let blobs = self.client.store.blobs.lock().unwrap();
        Ok(blobs.contains_key(&self.path))
    }
    fn lock_write_version(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let blobs = self.client.store.blobs.lock().unwrap();
        let blob = blobs.get(&self.path).ok_or_else(|| {
            warn!("lock_write_version: blob not found");
            MemBlobError::BlobNotFound("blob not found".to_string())
        })?;
        self.locked_generation = Some(blob.generation);
        Ok(true)
    }
    fn read(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let blobs = self.client.store.blobs.lock().unwrap();
        blobs.get(&self.path).map_or_else(
            move || Err(MemBlobError::BlobNotFound("blob not found".to_string()).into()),
            |blob| Ok(blob.data.clone()),
        )
    }
    fn write(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let mut blobs = self.client.store.blobs.lock().unwrap();
        if let Some(blob) = blobs.get_mut(&self.path) {
            if let Some(locked_generation) = self.locked_generation {
                if locked_generation != blob.generation || locked_generation == -1 {
                    return Err(MemBlobError::LockGenerationMismatch(
                        locked_generation,
                        blob.generation,
                    )
                    .into());
                }
            }
            blob.data = data.to_vec();
            blob.generation += 1;
        } else {
            let blob = MemBlob {
                generation: 0,
                path: self.path.clone(),
                data: data.to_vec(),
            };
            debug!("write: inserting blob: {:?}", blob);
            blobs.insert(self.path.clone(), blob);
        }

        Ok(())
    }
    fn delete(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut blobs = self.client.store.blobs.lock().unwrap();
        if self.locked_generation.is_some() {
            match blobs.contains_key(&self.path) {
                true => {
                    if let Some(blob) = blobs.get(&self.path) {
                        if let Some(locked_generation) = self.locked_generation {
                            if locked_generation != blob.generation {
                                return Err(MemBlobError::LockGenerationMismatch(
                                    locked_generation,
                                    blob.generation,
                                )
                                .into());
                            }
                        }
                    }
                }
                false => {
                    warn!("delete: blob not found");
                    return Err(MemBlobError::BlobNotFound("blob not found".to_string()).into());
                }
            }
        }
        blobs.remove(&self.path);
        Ok(())
    }

    fn get_string(&self) -> String {
        format!("{}/{}", self.client.get_string(), self.path)
    }
}

#[cfg(test)]
mod tests {
    use crate::init_logging;

    use super::*;
    #[test]
    fn test_mem_blob_store() {
        let _guard = init_logging().unwrap();
        // let store = std::sync::Arc::new(Mutex::new(MemBlobStore::new("test", true)));
        let store: MemBlobStore = MemBlobStore::new("test", true);
        {
            // let mut lock = store.lock().unwrap();
            // let mut client = lock.new_client().unwrap();
            let client = store.new_client().unwrap();
            let s = String::from("test");
            {
                let obj = client.new_object(s).unwrap();
                obj.write(b"testit").unwrap();
                let buf = obj.read().unwrap();
                assert_eq!(&buf, b"testit");
                obj.delete().unwrap();
                assert!(!obj.exists().unwrap());
            }
            // drop(obj);
            // drop(store);
            //
        }
    }
}
