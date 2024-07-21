use dyn_clone::DynClone;
use http::Uri;

#[allow(unused_imports)]
use crate::UNC_PREFIX;
use crate::{FsBlobStore, S3BlobStore, S3Options};

// pub enum BlobType {
//     Fs(FsBlobStore),
//     S3(S3BlobStore),
//     Mem(MemBlobStore),
// }
//
// impl BlobStore for BlobType {
//     fn new_client(&self) -> Result<Box<Pin<BlobClient>>, Box<dyn std::error::Error>> {
//         match self {
//             BlobType::Fs(fs_blob_store) => fs_blob_store.new_client(),
//             BlobType::S3(s3_blob_store) => s3_blob_store.new_client(),
//             BlobType::Mem(mem_blob_store) => mem_blob_store.new_client(),
//         }
//     }
//     fn get_string(&self) -> String {
//         match self {
//             BlobType::Fs(fs_blob_store) => fs_blob_store.get_string(),
//             BlobType::S3(s3_blob_store) => s3_blob_store.get_string(),
//             BlobType::Mem(mem_blob_store) => mem_blob_store.get_string(),
//         }
//     }
// }

pub fn create_blob_store_for_uri(uri: &str, opts: Option<S3Options>) -> Box<dyn BlobStore> {
    match uri {
        s if s.starts_with("fsblob://") => {
            let fs_blob_store = FsBlobStore::new(&uri[9..], true);
            Box::new(fs_blob_store)
        }
        s if s.starts_with("s3://") => {
            let uri = uri.parse::<Uri>().unwrap();
            let bucket_name = uri.host().unwrap().to_string();
            let mut prefix = uri.path().to_string();
            prefix = prefix.trim_start_matches('/').to_string();
            let s3_blob_store = S3BlobStore::new(&bucket_name, &prefix, opts);
            Box::new(s3_blob_store)
        }
        s if s.starts_with("file://") => {
            let blob_store = FsBlobStore::new(&uri[7..], true);
            Box::new(blob_store)
        }
        _ => {
            let blob_store = FsBlobStore::new(uri, true);
            Box::new(blob_store)
        }
    }
}

pub trait BlobObject {
    fn exists(&self) -> Result<bool, Box<dyn std::error::Error>>;
    fn lock_write_version(&mut self) -> Result<bool, Box<dyn std::error::Error>>;
    fn read(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>>;
    fn write(&self, data: &[u8]) -> Result<(), Box<dyn std::error::Error>>;
    fn delete(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn get_string(&self) -> String;
}

#[repr(C)]
#[derive(Debug)]
pub struct BlobProperties {
    pub size: usize,
    pub name: String,
}

pub trait BlobClient: DynClone + std::fmt::Debug {
    fn new_object(
        &self,
        path: String,
    ) -> Result<Box<dyn BlobObject + '_>, Box<dyn std::error::Error>>;
    fn get_objects(
        &self,
        path_prefix: String,
    ) -> Result<Vec<BlobProperties>, Box<dyn std::error::Error>>;
    fn supports_locking(&self) -> bool;
    fn get_string(&self) -> String;
    fn close(&self);
}

pub trait BlobStore: DynClone {
    fn new_client<'a>(
        &self,
    ) -> Result<Box<dyn BlobClient + 'a>, Box<dyn std::error::Error + 'static>>;
    fn get_string(&self) -> String;
}

fn split_uri(uri: &str) -> (&str, &str) {
    if let Some((parent, name)) = uri.rsplit_once('/') {
        (parent, name)
    } else {
        ("", uri)
    }
}

// TODO: implement read_blob_with_retry
pub fn read_blob<'a>(
    client: &(dyn BlobClient + 'a),
    key: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let object = client.new_object(key.to_owned())?;
    let buf = object.read()?;
    Ok(buf)
}

pub fn read_from_uri(
    uri: &str,
    opts: Option<S3Options>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let (parent, name) = split_uri(uri);
    tracing::debug!("Reading from URI: [{parent}] [{name}]");
    let store = create_blob_store_for_uri(parent, opts);
    let client = store.new_client()?;
    let object = client.new_object(name.to_owned())?;
    let buf = object.read()?;
    Ok(buf)
}
// TODO: implement write_to_uri and delete_from_uri
