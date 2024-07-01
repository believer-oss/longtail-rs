use std::{collections::HashMap, sync::Mutex};

use thiserror::Error;
use tracing::{debug, warn};

use crate::{BlobClient, BlobObject, BlobStore};

#[derive(Debug)]
pub struct MemBlob {
    pub generation: i32,
    pub path: String,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct MemBlobStore {
    pub blobs: Mutex<HashMap<String, MemBlob>>,
    pub prefix: String,
    pub supports_locking: bool,
}

impl MemBlobStore {
    pub fn new(prefix: &str, supports_locking: bool) -> Self {
        MemBlobStore {
            blobs: Mutex::new(HashMap::new()),
            prefix: prefix.to_string(),
            supports_locking,
        }
    }
}

impl BlobStore for MemBlobStore {
    fn new_client(
        &mut self,
    ) -> Result<impl BlobClient<'_>, Box<(dyn std::error::Error + 'static)>> {
        Ok(MemBlobClient { store: self })
    }

    fn get_string() -> String {
        "memstore".to_string()
    }
}

#[derive(Debug, Copy, Clone)]
pub struct MemBlobClient<'a> {
    pub store: &'a MemBlobStore,
}

impl<'a> BlobClient<'a> for MemBlobClient<'a> {
    fn new_object(
        &'a mut self,
        path: String,
        // ) -> Result<MemBlobObject<'a>, Box<dyn std::error::Error + 'static>> {
    ) -> Result<impl BlobObject, Box<(dyn std::error::Error + 'static)>> {
        Ok(MemBlobObject {
            client: self,
            path,
            locked_generation: None,
        })
    }

    fn get_objects(
        &'a self,
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

    fn supports_locking(&'a self) -> bool {
        self.store.supports_locking
    }

    fn get_string(&'a self) -> String {
        MemBlobStore::get_string()
    }

    fn close(&'a self) {}
}

#[derive(Error, Debug)]
pub enum MemBlobError {
    #[error("Blob '{0}' not found")]
    BlobNotFound(String),

    #[error("Lock generation mismatch '{0}' != '{1}'")]
    LockGenerationMismatch(i32, i32),
}

pub struct MemBlobObject<'a> {
    pub client: &'a mut MemBlobClient<'a>,
    pub path: String,
    pub locked_generation: Option<i32>,
}

impl BlobObject for MemBlobObject<'_> {
    fn exists(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let blobs = self.client.store.blobs.lock().unwrap();
        Ok(blobs.contains_key(&self.path))
    }
    fn lock_write_version(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let blobs = self.client.store.blobs.lock().unwrap();
        let blob = blobs.get(&self.path).ok_or_else(|| {
            warn!("lock_write_version: blob not found");
            MemBlobError::BlobNotFound("blob not found".to_string())
        })?;
        self.locked_generation = Some(blob.generation);
        Ok(())
    }
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> Result<usize, Box<dyn std::error::Error>> {
        let blobs = self.client.store.blobs.lock().unwrap();
        blobs.get(&self.path).map_or_else(
            move || {
                warn!("lock_write_version: blob not found");
                Err(MemBlobError::BlobNotFound("blob not found".to_string()).into())
            },
            |blob| {
                debug!("read: blob: {:?}", blob);
                buf.copy_from_slice(&blob.data);
                debug!("read: buf: {:?}", buf);
                Ok(blob.data.len())
            },
        )
    }
    fn write(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
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
    fn delete(&mut self) -> Result<(), Box<dyn std::error::Error>> {
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
        let mut store = MemBlobStore::new("test", true);
        let mut client = store.new_client().unwrap();
        let mut obj = client.new_object("test".to_string()).unwrap();
        obj.write(b"testit").unwrap();
        let mut buf = [0u8; 6];
        assert_eq!(obj.read(&mut buf).unwrap(), 6);
        debug!("buf: {:?}", buf);
        assert_eq!(&buf, b"testit");
        obj.delete().unwrap();
        assert!(!obj.exists().unwrap());
    }
}
