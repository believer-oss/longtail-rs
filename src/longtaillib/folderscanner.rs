use tracing::info;

use crate::{
    BikeshedJobAPI, ChunkerAPI, HashAPI, HashRegistry, HashType, Longtail_FileInfos,
    Longtail_GetFilesRecursively2, Longtail_ProgressAPI, PathFilterAPIProxy, ProgressAPI,
    ProgressAPIProxy, StorageAPI, VersionIndex,
};
use std::{io::Read, ptr::null_mut};

#[derive(Debug)]
pub struct FileInfos(pub *mut Longtail_FileInfos);

impl FileInfos {
    pub fn get_file_count(&self) -> u32 {
        unsafe { (*self.0).m_Count }
    }
    fn get_path_data_size(&self) -> u32 {
        unsafe { (*self.0).m_PathDataSize }
    }
    fn get_sizes_ptr(&self) -> *const u64 {
        unsafe { (*self.0).m_Sizes }
    }
    fn get_permissions_ptr(&self) -> *const u16 {
        unsafe { (*self.0).m_Permissions }
    }
    fn get_path_data_ptr(&self) -> *const u8 {
        unsafe { (*self.0).m_PathData as *const _ }
    }

    fn get_path_start_offsets(&self, index: u32) -> u32 {
        // The index should be less than the file count
        assert!(index < self.get_file_count());
        let index = isize::try_from(index).expect("Failed to convert index to isize");
        unsafe { *(*self.0).m_PathStartOffsets.offset(index) }
    }
    pub fn get_file_path(&self, index: u32) -> String {
        let offset = self.get_path_start_offsets(index);

        // The offset should be less than the path data size
        assert!(offset < self.get_path_data_size());
        let offset = usize::try_from(offset).expect("Failed to convert offset to usize");
        unsafe {
            let data = self.get_path_data_ptr().add(offset);
            std::ffi::CStr::from_ptr(data as *const _)
                .to_string_lossy()
                .into_owned()
        }
    }
    pub fn get_file_size(&self, index: u32) -> u64 {
        // The index should be less than the file count
        assert!(index < self.get_file_count());
        let index = isize::try_from(index).expect("Failed to convert index to isize");
        unsafe { *self.get_sizes_ptr().offset(index) }
    }
    pub fn get_file_permissions(&self, index: u32) -> u16 {
        // The index should be less than the file count
        assert!(index < self.get_file_count());
        let index = isize::try_from(index).expect("Failed to convert index to isize");
        unsafe { *self.get_permissions_ptr().offset(index) }
    }
    pub fn iter(&self) -> FileInfosIterator {
        FileInfosIterator {
            file_infos: self,
            index: 0,
        }
    }
    pub fn get_compression_types_for_files(&self, compression_type: u32) -> *const u32 {
        let len = self
            .get_file_count()
            .try_into()
            .expect("Failed to convert usize to u32");
        vec![compression_type; len].as_ptr()
    }
}

pub struct FileInfosIterator<'a> {
    file_infos: &'a FileInfos,
    index: u32,
}
type FileInfosItem = (String, u64, u16);

impl Iterator for FileInfosIterator<'_> {
    type Item = FileInfosItem;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.file_infos.get_file_count() {
            return None;
        }
        let path = (*self.file_infos).get_file_path(self.index);
        let size = (*self.file_infos).get_file_size(self.index);
        let permissions = (*self.file_infos).get_file_permissions(self.index);
        self.index += 1;
        Some((path, size, permissions))
    }
}

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
    let c_root_path = std::ffi::CString::new(root_path).unwrap();
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
    ) -> FolderScanner {
        let start = std::time::Instant::now();
        let file_infos = get_files_recursively(fs, jobs, path_filter, root_path).unwrap();
        let elapsed = start.elapsed();
        FolderScanner {
            file_infos,
            elapsed,
            error: std::ptr::null(),
        }
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
    ) -> Result<VersionIndexReader, i32> {
        if source_index_path.is_empty() {
            struct ProgressHandler {}
            impl ProgressAPI for ProgressHandler {
                fn on_progress(&self, _total_count: u32, _done_count: u32) {
                    println!("GetFolderIndex Progress: {}/{}", _done_count, _total_count);
                }
            }
            let file_infos = scanner.get_file_infos();
            info!("file_infos: {:?}", file_infos);
            let compression_types = (*file_infos).get_compression_types_for_files(compression_type);
            let hash = hash_registry
                .get_hash_api(
                    HashType::from_repr(hash_id as usize).expect("Could not find hash type"),
                )
                .unwrap();
            let chunker = ChunkerAPI::new();
            let progress = ProgressAPIProxy::new(Box::new(ProgressHandler {}));
            let source_folder_path = std::ffi::CString::new(source_folder_path).unwrap();
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
                    compression_types,
                    target_chunk_size,
                    enable_file_mappping as i32,
                    &mut vindex,
                )
            };
            if result != 0 {
                return Err(result);
            }
            Ok(VersionIndexReader {
                version_index: VersionIndex::from_longtail_versionindex(vindex),
                hash_api: hash,
            })
        } else {
            let mut f = std::fs::File::open(source_index_path).unwrap();
            let metadata = f.metadata().unwrap();
            let mut buffer = vec![0u8; metadata.len() as usize];
            f.read_exact(&mut buffer).unwrap();
            let result = VersionIndex::new_from_buffer(&mut buffer);
            let hash_api = hash_registry
                .get_hash_api(
                    HashType::from_repr(hash_id as usize).expect("Could not find hash type"),
                )
                .unwrap();
            Ok(VersionIndexReader {
                version_index: result.unwrap(),
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
        #[no_mangle]
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
        let pf = Box::new(TestPathFilter {});
        let path_filter = PathFilterAPIProxy::new_proxy_ptr(pf);
        let path_filter = unsafe { path_filter.as_ref().expect("Cannot deref path filter") };
        let root_path = "test-data/small/storage";
        let scanner = FolderScanner::scan(root_path, path_filter, &fs, &jobs);
        let file_infos = scanner.get_file_infos();
        assert_eq!(file_infos.get_file_count(), 7);
        for (path, size, permissions) in file_infos.iter() {
            println!("{} {} {:o}", path, size, permissions);
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
        let path_filter = unsafe { path_filter.as_ref().expect("Cannot deref path filter") };
        let root_path = "test-data/small";
        let scanner = FolderScanner::scan(root_path, path_filter, &fs, &jobs);
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
            path_filter,
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
