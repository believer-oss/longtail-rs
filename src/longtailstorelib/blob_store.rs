#[allow(unused_imports)]
use crate::UNCPrefix;

pub trait BlobObject {
    fn exists(&self) -> Result<bool, Box<dyn std::error::Error>>;
    fn lock_write_version(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn read(&self, buf: &mut [u8]) -> Result<usize, Box<dyn std::error::Error>>;
    fn write(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>>;
    fn delete(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn get_string(&self) -> String;
}

#[repr(C)]
pub struct BlobProperties {
    pub size: usize,
    pub name: String,
}

pub struct S3Options {
    pub opts: (),
}

pub trait BlobClient<'a>: Copy + Clone {
    fn new_object(
        &'a mut self,
        path: String,
    ) -> Result<impl BlobObject, Box<dyn std::error::Error>>;
    fn get_objects(
        &'a self,
        path_prefix: String,
    ) -> Result<Vec<BlobProperties>, Box<dyn std::error::Error>>;
    fn supports_locking(&'a self) -> bool;
    fn get_string(&'a self) -> String;
    fn close(&'a self);
}

pub trait BlobStore {
    fn new_client(&mut self) -> Result<impl BlobClient, Box<dyn std::error::Error>>;
    fn get_string() -> String;
}

pub fn create_blob_store(_uri: String, _opts: Option<S3Options>) -> crate::MemBlobStore {
    // match uri {
    //     s if s.starts_with("fsblob://") => {
    //         // NewFSBlobStore(uri[len("fsblob://"):], false)
    //         todo!()
    //     }
    //     s if s.starts_with(UNCPrefix) => {
    //         // NewFSBlobStore(uri, false)
    //         todo!()
    //     }
    //     s if s.starts_with("s3://") => {
    //         // NewS3BlobStore(blobStoreURL, opts...)
    //         todo!()
    //     }
    //     _ => {
    //         // NewFSBlobStore(uri, false)
    //         todo!()
    //     }
    // };
    crate::MemBlobStore::new("test", true)
}
