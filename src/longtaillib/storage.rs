use crate::*;
use std::{
    ffi::c_char,
    ops::{Deref, DerefMut},
};

#[repr(C)]
pub struct CFullPath {
    pub full_path: *const c_char,
}

impl Drop for CFullPath {
    fn drop(&mut self) {
        unsafe { Longtail_Free((self.full_path as *mut c_char) as *mut std::ffi::c_void) };
    }
}

impl Deref for CFullPath {
    type Target = *const c_char;
    fn deref(&self) -> &Self::Target {
        &self.full_path
    }
}

impl DerefMut for CFullPath {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.full_path
    }
}

impl CFullPath {
    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new(
        storage_api: *mut Longtail_StorageAPI,
        root_path: &str,
        path: &str,
    ) -> CFullPath {
        let c_root_path = std::ffi::CString::new(root_path).unwrap();
        let c_path = std::ffi::CString::new(path).unwrap();
        let c_full_path = unsafe {
            Longtail_Storage_ConcatPath(storage_api, c_root_path.as_ptr(), c_path.as_ptr())
        };
        CFullPath {
            full_path: c_full_path,
        }
    }
}

#[repr(C)]
pub struct COpenFile {
    pub storage_api: *mut Longtail_StorageAPI,
    pub open_file: *mut Longtail_StorageAPI_HOpenFile,
}

impl Drop for COpenFile {
    fn drop(&mut self) {
        unsafe { Longtail_Storage_CloseFile(self.storage_api, *self.open_file) };
    }
}

impl Deref for COpenFile {
    type Target = *mut Longtail_StorageAPI_HOpenFile;
    fn deref(&self) -> &Self::Target {
        &self.open_file
    }
}

impl DerefMut for COpenFile {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.open_file
    }
}

impl COpenFile {
    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new(
        storage_api: *mut Longtail_StorageAPI,
        c_full_path: *const c_char,
        block_size: u64,
    ) -> Result<COpenFile, i32> {
        let open_file = std::ptr::null_mut::<Longtail_StorageAPI_HOpenFile>();
        let result = unsafe {
            Longtail_Storage_OpenWriteFile(storage_api, c_full_path, block_size, open_file)
        };
        if result != 0 {
            return Err(result);
        }
        Ok(COpenFile {
            storage_api,
            open_file,
        })
    }

    pub fn read(&self, offset: u64, size: u64) -> Result<Vec<u8>, i32> {
        let mut buffer = vec![0u8; size as usize];
        let result = unsafe {
            Longtail_Storage_Read(
                self.storage_api,
                *self.open_file,
                offset,
                size,
                buffer.as_mut_ptr() as *mut std::ffi::c_void,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(buffer)
    }
}

#[repr(C)]
pub struct StorageAPI {
    pub storage_api: *mut Longtail_StorageAPI,
    _pin: std::marker::PhantomPinned,
}

impl Drop for StorageAPI {
    fn drop(&mut self) {
        unsafe { Longtail_DisposeAPI(&mut (*self.storage_api).m_API as *mut Longtail_API) };
    }
}

impl Deref for StorageAPI {
    type Target = *mut Longtail_StorageAPI;
    fn deref(&self) -> &Self::Target {
        &self.storage_api
    }
}

impl DerefMut for StorageAPI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.storage_api
    }
}

impl StorageAPI {
    pub fn new_fs() -> StorageAPI {
        let storage_api = unsafe { Longtail_CreateFSStorageAPI() };
        StorageAPI {
            storage_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_inmem() -> StorageAPI {
        let storage_api = unsafe { Longtail_CreateInMemStorageAPI() };
        StorageAPI {
            storage_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn read_from_storage(&self, root_path: &str, path: &str) -> Result<Vec<u8>, i32> {
        let c_full_path = unsafe { CFullPath::new(self.storage_api, root_path, path) };
        let c_open_file = unsafe { COpenFile::new(self.storage_api, *c_full_path, 0)? };
        let mut file_size = 0;
        let result = unsafe {
            Longtail_Storage_GetSize(self.storage_api, **c_open_file, &mut file_size as *mut u64)
        };
        if result != 0 {
            return Err(result);
        }
        let buffer = c_open_file.read(0, file_size)?;
        Ok(buffer)
    }

    pub fn write_to_storage(&self, root_path: &str, path: &str, buffer: &[u8]) -> Result<(), i32> {
        let c_full_path = unsafe { CFullPath::new(self.storage_api, root_path, path) };
        let result = unsafe { EnsureParentPathExists(self.storage_api, *c_full_path) };
        if result != 0 {
            return Err(result);
        }
        let block_size = buffer.len() as u64;

        let c_open_file = unsafe { COpenFile::new(self.storage_api, *c_full_path, 0)? };
        if block_size > 0 {
            let result = unsafe {
                Longtail_Storage_Write(
                    self.storage_api,
                    **c_open_file,
                    0,
                    block_size,
                    buffer.as_ptr() as *const std::ffi::c_void,
                )
            };
            if result != 0 {
                return Err(result);
            }
        }
        Ok(())
    }

    pub fn open_read_file(&self, path: &str) -> Result<COpenFile, i32> {
        let c_path = std::ffi::CString::new(path).unwrap();
        let c_open_file = std::ptr::null_mut::<Longtail_StorageAPI_HOpenFile>();
        let result = unsafe {
            Longtail_Storage_OpenReadFile(self.storage_api, c_path.as_ptr(), c_open_file)
        };
        if result != 0 {
            return Err(result);
        }
        Ok(COpenFile {
            storage_api: self.storage_api,
            open_file: c_open_file,
        })
    }

    // TODO: Not sure this belongs here, because we have COpenFile, but modeling golongtail for
    // now.
    pub fn get_size(&self, f: COpenFile) -> Result<u64, i32> {
        let mut size = 0;
        let result = unsafe { Longtail_Storage_GetSize(self.storage_api, *f.open_file, &mut size) };
        if result != 0 {
            return Err(result);
        }
        Ok(size)
    }

    pub fn read(&self, f: COpenFile, offset: u64, size: u64) -> Result<Vec<u8>, i32> {
        let mut buffer = vec![0u8; size as usize];
        let result = unsafe {
            Longtail_Storage_Read(
                self.storage_api,
                *f.open_file,
                offset,
                size,
                buffer.as_mut_ptr() as *mut std::ffi::c_void,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(buffer)
    }

    // TODO: This shouldn't be needed, since RAII on COpenFile should handle this.
    pub fn close_file(&self, f: COpenFile) {
        unsafe { Longtail_Storage_CloseFile(self.storage_api, *f.open_file) };
    }

    pub fn start_find(&self, path: &str) -> Result<*mut Longtail_StorageAPI_Iterator, i32> {
        let c_path = std::ffi::CString::new(path).unwrap();
        let mut iterator = std::ptr::null_mut::<Longtail_StorageAPI_Iterator>();
        let result =
            unsafe { Longtail_Storage_StartFind(self.storage_api, c_path.as_ptr(), &mut iterator) };
        if result != 0 {
            return Err(result);
        }
        Ok(iterator)
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn find_next(
        &self,
        iterator: *mut Longtail_StorageAPI_HIterator,
    ) -> Result<(), i32> {
        let result = unsafe { Longtail_Storage_FindNext(self.storage_api, *iterator) };
        if result != 0 {
            return Err(result);
        }
        Ok(())
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn close_find(&self, iterator: *mut Longtail_StorageAPI_HIterator) {
        unsafe { Longtail_Storage_CloseFind(self.storage_api, *iterator) };
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn get_entry_properties(
        &self,
        iterator: *mut Longtail_StorageAPI_HIterator,
    ) -> Result<Longtail_StorageAPI_EntryProperties, i32> {
        let mut properties = Longtail_StorageAPI_EntryProperties {
            m_Name: std::ptr::null(),
            m_IsDir: 0,
            m_Size: 0,
            m_Permissions: 0,
        };
        let result = unsafe {
            Longtail_Storage_GetEntryProperties(
                self.storage_api,
                *iterator,
                &mut properties as *mut Longtail_StorageAPI_EntryProperties,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(properties)
    }
}

#[repr(C)]
pub struct BlockstoreAPI {
    pub blockstore_api: *mut Longtail_BlockStoreAPI,
    _pin: std::marker::PhantomPinned,
}

impl Drop for BlockstoreAPI {
    fn drop(&mut self) {
        unsafe { Longtail_DisposeAPI(&mut (*self.blockstore_api).m_API as *mut Longtail_API) };
    }
}

impl Deref for BlockstoreAPI {
    type Target = *mut Longtail_BlockStoreAPI;
    fn deref(&self) -> &Self::Target {
        &self.blockstore_api
    }
}

impl DerefMut for BlockstoreAPI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.blockstore_api
    }
}

impl BlockstoreAPI {
    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_fs(
        jobs: *mut Longtail_JobAPI,
        storage_api: *mut Longtail_StorageAPI,
        contentPath: &str,
        block_extension: Option<&str>,
        enable_file_mapping: bool,
    ) -> BlockstoreAPI {
        let c_content_path = std::ffi::CString::new(contentPath).unwrap();
        let c_block_extension = if let Some(block_extension) = block_extension {
            std::ffi::CString::new(block_extension).unwrap()
        } else {
            std::ffi::CString::new("").unwrap()
        };
        let blockstore_api = unsafe {
            Longtail_CreateFSBlockStoreAPI(
                jobs,
                storage_api,
                c_content_path.as_ptr(),
                c_block_extension.as_ptr(),
                enable_file_mapping as i32,
            )
        };
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_cached(
        jobs: *mut Longtail_JobAPI,
        cache_blockstore: *mut Longtail_BlockStoreAPI,
        persistent_blockstore: *mut Longtail_BlockStoreAPI,
    ) -> BlockstoreAPI {
        let blockstore_api =
            Longtail_CreateCacheBlockStoreAPI(jobs, cache_blockstore, persistent_blockstore);
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_compressed(
        backing_blockstore: *mut Longtail_BlockStoreAPI,
        compression_api: *mut Longtail_CompressionRegistryAPI,
    ) -> BlockstoreAPI {
        let blockstore_api =
            Longtail_CreateCompressBlockStoreAPI(backing_blockstore, compression_api);
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_share(backing_blockstore: *mut Longtail_BlockStoreAPI) -> BlockstoreAPI {
        let blockstore_api = Longtail_CreateShareBlockStoreAPI(backing_blockstore);
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_lru(
        backing_blockstore: *mut Longtail_BlockStoreAPI,
        max_cache_size: u32,
    ) -> BlockstoreAPI {
        let blockstore_api = Longtail_CreateLRUBlockStoreAPI(backing_blockstore, max_cache_size);
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_archive(
        storage_api: *mut Longtail_StorageAPI,
        archive_path: &str,
        archive_index: *mut Longtail_ArchiveIndex,
        enable_write: bool,
        enable_file_mapping: bool,
    ) -> BlockstoreAPI {
        let c_archive_path = std::ffi::CString::new(archive_path).unwrap();
        let blockstore_api = Longtail_CreateArchiveBlockStore(
            storage_api,
            c_archive_path.as_ptr(),
            archive_index,
            enable_write as i32,
            enable_file_mapping as i32,
        );
        BlockstoreAPI {
            blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_block_store(
        hash_api: *mut Longtail_HashAPI,
        job_api: *mut Longtail_JobAPI,
        block_store: *mut Longtail_BlockStoreAPI,
        store_index: *mut Longtail_StoreIndex,
        version_index: *mut Longtail_VersionIndex,
    ) -> StorageAPI {
        let blockstore_api = Longtail_CreateBlockStoreStorageAPI(
            hash_api,
            job_api,
            block_store,
            store_index,
            version_index,
        );
        StorageAPI {
            storage_api: blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }
}
