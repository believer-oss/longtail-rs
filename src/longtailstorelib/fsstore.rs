use std::ops::Deref;

#[cfg(target_os = "windows")]
use std::os::windows::fs::MetadataExt;

#[cfg(not(target_os = "windows"))]
use std::os::unix::fs::MetadataExt;

use crate::{normalize_file_system_path, BlobClient, BlobObject, BlobStore};
use fs4::fs_std::FileExt;

#[derive(Debug, Clone)]
pub struct FsBlobStore {
    pub enable_locking: bool,
    pub prefix: String,
}

impl FsBlobStore {
    pub fn new(prefix: &str, enable_locking: bool) -> Self {
        FsBlobStore {
            enable_locking,
            prefix: prefix.to_string(),
        }
    }
}

impl BlobStore for FsBlobStore {
    fn new_client<'a>(
        &self,
    ) -> Result<Box<dyn BlobClient + 'a>, Box<dyn std::error::Error + 'static>> {
        Ok(Box::new(FsBlobClient {
            store: self.clone(),
        }))
    }
    fn get_string(&self) -> String {
        "fsblob://".to_string()
    }
}

#[derive(Debug)]
pub struct FsExclusiveLockGuard {
    file: std::fs::File,
    lock_path: String,
}

impl Drop for FsExclusiveLockGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
        std::fs::remove_file(&self.lock_path).expect("error removing lock file")
    }
}

impl Deref for FsExclusiveLockGuard {
    type Target = std::fs::File;
    fn deref(&self) -> &Self::Target {
        &self.file
    }
}

#[derive(Debug, Clone)]
pub struct FsBlobClient {
    pub store: FsBlobStore,
}

impl BlobClient for FsBlobClient {
    fn new_object(
        &self,
        path: String,
    ) -> Result<Box<dyn crate::BlobObject + '_>, Box<dyn std::error::Error>> {
        let path = format!("{}{}{}", self.store.prefix, std::path::MAIN_SEPARATOR, path);
        let path = normalize_file_system_path(path);
        Ok(Box::new(FsBlobObject {
            client: self.clone(),
            path,
            metageneration: -1,
        }))
    }

    fn get_objects(
        &self,
        path_prefix: String,
    ) -> Result<Vec<crate::BlobProperties>, Box<dyn std::error::Error>> {
        let search_path = &self.store.prefix;
        let mut objects = Vec::<crate::BlobProperties>::new();
        let meta = std::fs::metadata(search_path)?;
        if !meta.is_dir() {
            #[cfg(target_os = "windows")]
            let size = meta
                .file_size()
                .try_into()
                .expect("could not find file size");

            #[cfg(not(target_os = "windows"))]
            let size = meta.size().try_into().expect("could not find file size");

            objects.push(crate::BlobProperties {
                size,
                name: normalize_file_system_path(search_path.to_string()),
            });

            return Ok(objects);
        }
        for entry in std::fs::read_dir(search_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            if path.to_string_lossy().ends_with("._lck") {
                continue;
            }
            let path = path
                .strip_prefix(search_path)
                .expect("search_path is not a prefix of path");
            // TODO: Windows strings may fail here...
            let leaf_path = normalize_file_system_path(
                path.to_str()
                    .expect("could not convert path to utf-8")
                    .to_string(),
            );
            if leaf_path.len() < path_prefix.len() {
                continue;
            }
            if leaf_path.starts_with(&path_prefix) {
                objects.push(crate::BlobProperties {
                    size: entry.metadata()?.len() as usize,
                    name: leaf_path,
                });
            }
        }
        Ok(objects)
    }

    fn supports_locking(&self) -> bool {
        self.store.enable_locking
    }

    fn get_string(&self) -> String {
        self.store.get_string()
    }

    fn close(&self) {}
}

#[derive(Debug)]
pub struct FsBlobObject {
    pub client: FsBlobClient,
    pub path: String,
    pub metageneration: i64,
}

impl FsBlobObject {
    fn get_meta_generation(&self) -> Result<i64, Box<dyn std::error::Error>> {
        let metapath = format!("{}.gen", self.path);
        match std::fs::read(metapath) {
            Ok(data) => {
                let meta_generation =
                    i64::from_le_bytes(data.try_into().expect("invalid meta generation"));
                Ok(meta_generation)
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(0)
                } else {
                    Err(e.into())
                }
            }
        }
    }

    fn set_meta_generation(&self, meta_generation: i64) -> Result<(), Box<dyn std::error::Error>> {
        let metapath = format!("{}.gen", self.path);
        let data = meta_generation.to_le_bytes();
        std::fs::write(metapath, data)?;
        Ok(())
    }

    fn delete_generation(&self) -> Result<(), Box<dyn std::error::Error>> {
        let metapath = format!("{}.gen", self.path);
        match std::fs::remove_file(metapath) {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    fn lock_file(&self) -> Result<FsExclusiveLockGuard, Box<dyn std::error::Error>> {
        let lock_path = format!("{}.lck", self.path);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&lock_path)?;
        file.lock_exclusive()?;
        Ok(FsExclusiveLockGuard { file, lock_path })
    }
}

impl BlobObject for FsBlobObject {
    fn exists(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let metadata = std::fs::metadata(&self.path);
        tracing::info!("FsBlobObject::exists({}) -> {:?}", self.path, metadata);
        match metadata {
            Ok(_) => Ok(true),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(false)
                } else {
                    Err(e.into())
                }
            }
        }
    }

    fn lock_write_version(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        if !self.client.store.enable_locking {
            return Err("locking is not supported".into());
        }

        let _filelock = self.lock_file()?;

        match self.exists() {
            Ok(true) => {
                self.metageneration = self.get_meta_generation()?;
            }
            Ok(false) => {
                self.metageneration = 0;
            }
            Err(e) => {
                return Err(e);
            }
        }
        Ok(true)
    }

    fn read(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let _filelock = if self.client.store.enable_locking {
            Some(self.lock_file()?)
        } else {
            None
        };

        let data = std::fs::read(&self.path)?;
        Ok(data)
    }

    fn write(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let _filelock = if self.client.store.enable_locking {
            Some(self.lock_file()?)
        } else {
            None
        };

        let _ = std::fs::create_dir_all(
            std::path::Path::new(&self.path)
                .parent()
                .expect("could not get parent of path"),
        );

        if self.client.store.enable_locking && self.metageneration != -1 {
            let current_meta_generation = self.get_meta_generation()?;
            if current_meta_generation != self.metageneration {
                return Err("meta generation mismatch".into());
            }
        }

        std::fs::write(&self.path, data)?;

        if self.client.store.enable_locking && self.metageneration != -1 {
            self.set_meta_generation(self.metageneration + 1)?;
        }

        Ok(())
    }

    fn delete(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.client.store.enable_locking {
            let _filelock = self.lock_file()?;
            if self.metageneration != -1 {
                let current_meta_generation = self.get_meta_generation()?;
                if current_meta_generation != self.metageneration {
                    return Err("meta generation mismatch".into());
                }
            }
        }

        std::fs::remove_file(&self.path)?;

        if self.client.store.enable_locking {
            self.delete_generation()?;
        }
        Ok(())
    }

    fn get_string(&self) -> String {
        format!("{}{}", self.client.get_string(), self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlobStore;

    #[test]
    fn test_fs_blob_store() {
        let _guard = crate::init_logging().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_dir_path = temp_dir.path().to_str().unwrap();
        let store = FsBlobStore::new(temp_dir_path, false);
        assert_eq!(store.get_string(), "fsblob://");
        let client = store.new_client().unwrap();
        assert_eq!(client.get_string(), "fsblob://");
        let objects = client.get_objects("".to_string()).unwrap();
        assert_eq!(objects.len(), 0);
    }
    #[test]
    fn test_fs_blob_object() {
        let _guard = crate::init_logging().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_dir_path = temp_dir.path().to_str().unwrap();
        let store = FsBlobStore::new(temp_dir_path, false);
        let client = store.new_client().unwrap();
        let object = client.new_object("test.txt".to_string()).unwrap();
        #[cfg(target_os = "windows")]
        assert_eq!(
            object.get_string(),
            format!("fsblob://{}/test.txt", temp_dir_path.replace("\\", "/"))
        );
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            object.get_string(),
            format!("fsblob://{}/test.txt", temp_dir_path)
        );
        assert!(!object.exists().unwrap());
        assert!(object.read().is_err());
        assert!(object.write("hello".as_bytes()).is_ok());
        assert!(object.exists().unwrap());
        assert_eq!(object.read().unwrap(), "hello".as_bytes());
        assert!(object.write("world".as_bytes()).is_ok());
        assert_eq!(object.read().unwrap(), "world".as_bytes());
        assert!(object.delete().is_ok());
        assert!(!object.exists().unwrap());
    }
    #[test]
    fn test_fs_blob_locking() {
        let _guard = crate::init_logging().unwrap();
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_dir_path = temp_dir.path().to_str().unwrap();
        let store = FsBlobStore::new(temp_dir_path, true);
        let client = store.new_client().unwrap();
        let mut object = client.new_object("test.txt".to_string()).unwrap();
        assert!(!object.exists().unwrap());
        assert!(object.lock_write_version().unwrap());
        assert!(!object.exists().unwrap());
        assert!(object.write("hello".as_bytes()).is_ok());
    }
}
