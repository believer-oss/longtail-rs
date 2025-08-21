use tracing::{debug, info};

use crate::{
    error::LongtailError, BikeshedJobAPI, ChunkerAPI, FileInfos, HashAPI, HashRegistry, HashType,
    Longtail_FileInfos, Longtail_GetFilesRecursively2, Longtail_ProgressAPI, PathFilterAPIProxy,
    ProgressAPI, ProgressAPIProxy, StorageAPI, VersionIndex,
};
use std::{io::Read, ptr::null_mut};

// TODO: This implementation is a direct port from golang and needs to be
// rewritten to be idiomatic Rust.
#[repr(C)]
#[derive(Debug)]
pub struct FolderScanner {
    file_infos: FileInfos,
    elapsed: std::time::Duration,
    error: *const std::os::raw::c_char,
}

// TODO: Only implementing GetFilesRecursively2 for now?
// TODO: Async?
/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
pub fn get_files_recursively(
    storage_api: &StorageAPI,
    job_api: &BikeshedJobAPI,
    path_filter: &PathFilterAPIProxy,
    root_path: &str,
) -> Result<FileInfos, i32> {
    let c_root_path = std::ffi::CString::new(root_path).expect("root_path contains null bytes");
    let mut file_infos = std::ptr::null_mut::<Longtail_FileInfos>();
    let result = unsafe {
        Longtail_GetFilesRecursively2(
            storage_api.storage_api,
            job_api.job_api,
            path_filter.as_ptr(),
            // (*path_filter).api.as_mut().unwrap(),
            // null_mut(),
            null_mut(),
            null_mut(),
            c_root_path.as_ptr(),
            &mut file_infos as *mut *mut Longtail_FileInfos,
        )
    };
    if result != 0 {
        return Err(result);
    }
    Ok(FileInfos(file_infos))
}

impl FolderScanner {
    pub fn get_file_infos(&self) -> &FileInfos {
        &self.file_infos
    }
    pub fn get_elapsed(&self) -> std::time::Duration {
        self.elapsed
    }
    pub fn get_error(&self) -> *const std::os::raw::c_char {
        self.error
    }

    pub fn scan(
        root_path: &str,
        path_filter: &PathFilterAPIProxy,
        fs: &StorageAPI,
        jobs: &BikeshedJobAPI,
    ) -> Result<FolderScanner, i32> {
        let start = std::time::Instant::now();
        let file_infos = get_files_recursively(fs, jobs, path_filter, root_path)?;
        let elapsed = start.elapsed();
        Ok(FolderScanner {
            file_infos,
            elapsed,
            error: std::ptr::null(),
        })
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct VersionIndexReader {
    pub version_index: VersionIndex,
    pub hash_api: HashAPI,
    // pub elapsed: std::time::Duration,
    // pub error: *const std::os::raw::c_char,
}

impl VersionIndexReader {
    // #[allow(clippy::too_many_arguments)]
    // pub fn read(
    //     source_folder_path: &str,
    //     source_index_path: &str,
    //     target_chunk_size: u32,
    //     compression_type: u32,
    //     hash_id: u32,
    //     path_filter: *mut PathFilterAPIProxy,
    //     fs: &StorageAPI,
    //     jobs: *mut Longtail_JobAPI,
    //     hash_registry: &HashRegistry,
    //     enable_file_mappping: bool,
    //     scanner: &FolderScanner,
    // ) -> VersionIndexReader {
    //     let start = std::time::Instant::now();
    //     let version_index = Self::get_folder_index(
    //         source_folder_path,
    //         source_index_path,
    //         target_chunk_size,
    //         compression_type,
    //         hash_id,
    //         path_filter,
    //         fs,
    //         jobs,
    //         hash_registry,
    //         enable_file_mappping,
    //         scanner,
    //     );
    //     let elapsed = start.elapsed();
    //     VersionIndexReader {
    //         version_index,
    //         hash_api: std::ptr::null_mut(),
    //         elapsed,
    //         error: std::ptr::null(),
    //     }
    // }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    #[allow(clippy::too_many_arguments)]
    pub fn get_folder_index(
        source_folder_path: &str,
        source_index_path: &str,
        target_chunk_size: u32,
        compression_type: u32,
        hash_id: u32,
        _path_filter: &PathFilterAPIProxy,
        storage_api: &StorageAPI,
        job_api: &BikeshedJobAPI,
        hash_registry: &HashRegistry,
        enable_file_mappping: bool,
        scanner: &FolderScanner,
    ) -> Result<VersionIndexReader, LongtailError> {
        info!("source_index_path: {}", source_index_path);
        if source_index_path.is_empty() {
            struct ProgressHandler {}
            impl ProgressAPI for ProgressHandler {
                fn on_progress(&self, _total_count: u32, _done_count: u32) {
                    debug!("GetFolderIndex Progress: {}/{}", _done_count, _total_count);
                }
            }
            let file_infos = scanner.get_file_infos();
            info!("file_infos: {:?}", file_infos);
            let compression_types = (*file_infos).get_compression_types_for_files(compression_type);
            let hash = hash_registry
                .get_hash_api(
                    HashType::from_repr(hash_id as usize).expect("could not find hash type"),
                )
                .expect("registry does not contain hash type");
            let chunker = ChunkerAPI::new();
            let progress = ProgressAPIProxy::new(Box::new(ProgressHandler {}));
            let source_folder_path = std::ffi::CString::new(source_folder_path)
                .expect("source_folder_path contains null bytes");
            let mut vindex = std::ptr::null_mut::<crate::Longtail_VersionIndex>();
            let result = unsafe {
                crate::Longtail_CreateVersionIndex(
                    storage_api.storage_api,
                    *hash,
                    chunker.get_chunker_api(),
                    job_api.job_api,
                    &progress as *const ProgressAPIProxy as *mut Longtail_ProgressAPI,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    source_folder_path.as_ptr(),
                    file_infos.0,
                    compression_types.as_ptr(),
                    target_chunk_size,
                    enable_file_mappping as i32,
                    &mut vindex,
                )
            };
            if result != 0 {
                return Err(result.into());
            }
            Ok(VersionIndexReader {
                version_index: VersionIndex::new_from_lt(vindex),
                hash_api: hash,
            })
        } else {
            let mut f = std::fs::File::open(source_index_path)?;
            let metadata = f.metadata()?;
            let mut buffer = vec![0u8; metadata.len() as usize];
            f.read_exact(&mut buffer)?;
            let version_index = VersionIndex::new_from_buffer(&mut buffer)?;
            let hash_api = hash_registry.get_hash_api(
                HashType::from_repr(hash_id as usize).expect("Could not find hash type"),
            )?;
            Ok(VersionIndexReader {
                version_index,
                hash_api,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BikeshedJobAPI, HashRegistry, HashType, PathFilterAPI, PathFilterAPIProxy, StorageAPI,
    };
    #[derive(Debug)]
    struct TestPathFilter {}
    impl PathFilterAPI for TestPathFilter {
        #[unsafe(no_mangle)]
        fn include(
            &self,
            _root_path: &str,
            _asset_path: &str,
            _asset_name: &str,
            _is_dir: bool,
            _size: u64,
            _permissions: u16,
        ) -> bool {
            true
        }
    }
    #[test]
    // #[ignore]
    fn test_folder_scanner() {
        let _guard = crate::init_logging().unwrap();
        let jobs = BikeshedJobAPI::new(1, 1);
        let fs = StorageAPI::new_fs();
        let pf = TestPathFilter {};
        let path_filter = PathFilterAPIProxy::new_proxy_ptr(Box::new(pf));
        let path_filter_ref = unsafe { path_filter.as_ref().expect("Cannot deref path filter") };
        let scanner =
            FolderScanner::scan("test-data/small/storage", path_filter_ref, &fs, &jobs).unwrap();
        let file_infos = scanner.get_file_infos();
        assert_eq!(file_infos.get_file_count(), 7);
        for (path, size, permissions) in file_infos.iter() {
            println!("{path} {size} {permissions:o}");
        }
    }
    #[test]
    fn test_version_index_reader() {
        let _guard = crate::init_logging().unwrap();
        let jobs = BikeshedJobAPI::new(1, 1);
        let hash_registry = HashRegistry::new();
        let fs = StorageAPI::new_fs();
        let pf = TestPathFilter {};
        let path_filter = PathFilterAPIProxy::new_proxy_ptr(Box::new(pf));
        let path_filter_ref = unsafe { path_filter.as_ref().expect("Cannot deref path filter") };
        let scanner = FolderScanner::scan("test-data/small", path_filter_ref, &fs, &jobs).unwrap();
        let source_folder_path = "test-data/small";
        let source_index_path = "";
        let target_chunk_size = 64 * 1024;
        let compression_type = 0;
        let hash_id = HashType::Blake3 as u32;
        let enable_file_mappping = false;
        let version_index_reader = VersionIndexReader::get_folder_index(
            source_folder_path,
            source_index_path,
            target_chunk_size,
            compression_type,
            hash_id,
            path_filter_ref,
            &fs,
            &jobs,
            &hash_registry,
            enable_file_mappping,
            &scanner,
        )
        .unwrap();
        let version_index = version_index_reader.version_index;
        assert_eq!(version_index.get_asset_count(), 16);
    }
}
