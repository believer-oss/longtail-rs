use crate::*;
use std::{
    ffi::c_char,
    ops::{Deref, DerefMut},
};

#[rustfmt::skip]
/// Storage API
// pub fn Longtail_GetStorageAPISize() -> u64;
// pub fn Longtail_MakeStorageAPI( mem: *mut ::std::os::raw::c_void, dispose_func: Longtail_DisposeFunc, open_read_file_func: Longtail_Storage_OpenReadFileFunc, get_size_func: Longtail_Storage_GetSizeFunc, read_func: Longtail_Storage_ReadFunc, open_write_file_func: Longtail_Storage_OpenWriteFileFunc, write_func: Longtail_Storage_WriteFunc, set_size_func: Longtail_Storage_SetSizeFunc, set_permissions_func: Longtail_Storage_SetPermissionsFunc, get_permissions_func: Longtail_Storage_GetPermissionsFunc, close_file_func: Longtail_Storage_CloseFileFunc, create_dir_func: Longtail_Storage_CreateDirFunc, rename_file_func: Longtail_Storage_RenameFileFunc, concat_path_func: Longtail_Storage_ConcatPathFunc, is_dir_func: Longtail_Storage_IsDirFunc, is_file_func: Longtail_Storage_IsFileFunc, remove_dir_func: Longtail_Storage_RemoveDirFunc, remove_file_func: Longtail_Storage_RemoveFileFunc, start_find_func: Longtail_Storage_StartFindFunc, find_next_func: Longtail_Storage_FindNextFunc, close_find_func: Longtail_Storage_CloseFindFunc, get_entry_properties_func: Longtail_Storage_GetEntryPropertiesFunc, lock_file_func: Longtail_Storage_LockFileFunc, unlock_file_func: Longtail_Storage_UnlockFileFunc, get_parent_path_func: Longtail_Storage_GetParentPathFunc, map_file_func: Longtail_Storage_MapFileFunc, unmap_file_func: Longtail_Storage_UnmapFileFunc, open_append_file_func: Longtail_Storage_OpenAppendFileFunc,) -> *mut Longtail_StorageAPI;
// pub fn Longtail_Storage_OpenReadFile( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char, out_open_file: *mut Longtail_StorageAPI_HOpenFile,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_GetSize( storage_api: *mut Longtail_StorageAPI, f: Longtail_StorageAPI_HOpenFile, out_size: *mut u64,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_Read( storage_api: *mut Longtail_StorageAPI, f: Longtail_StorageAPI_HOpenFile, offset: u64, length: u64, output: *mut ::std::os::raw::c_void,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_OpenWriteFile( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char, initial_size: u64, out_open_file: *mut Longtail_StorageAPI_HOpenFile,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_Write( storage_api: *mut Longtail_StorageAPI, f: Longtail_StorageAPI_HOpenFile, offset: u64, length: u64, input: *const ::std::os::raw::c_void,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_SetSize( storage_api: *mut Longtail_StorageAPI, f: Longtail_StorageAPI_HOpenFile, length: u64,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_SetPermissions( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char, permissions: u16,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_GetPermissions( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char, out_permissions: *mut u16,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_CloseFile( storage_api: *mut Longtail_StorageAPI, f: Longtail_StorageAPI_HOpenFile,);
// pub fn Longtail_Storage_CreateDir( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_RenameFile( storage_api: *mut Longtail_StorageAPI, source_path: *const ::std::os::raw::c_char, target_path: *const ::std::os::raw::c_char,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_ConcatPath( storage_api: *mut Longtail_StorageAPI, root_path: *const ::std::os::raw::c_char, sub_path: *const ::std::os::raw::c_char,) -> *mut ::std::os::raw::c_char;
// pub fn Longtail_Storage_IsDir( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_IsFile( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_RemoveDir( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_RemoveFile( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_StartFind( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char, out_iterator: *mut Longtail_StorageAPI_HIterator,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_FindNext( storage_api: *mut Longtail_StorageAPI, iterator: Longtail_StorageAPI_HIterator,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_CloseFind( storage_api: *mut Longtail_StorageAPI, iterator: Longtail_StorageAPI_HIterator,);
// pub fn Longtail_Storage_GetEntryProperties( storage_api: *mut Longtail_StorageAPI, iterator: Longtail_StorageAPI_HIterator, out_properties: *mut Longtail_StorageAPI_EntryProperties,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_LockFile( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char, out_lock_file: *mut Longtail_StorageAPI_HLockFile,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_UnlockFile( storage_api: *mut Longtail_StorageAPI, lock_file: Longtail_StorageAPI_HLockFile,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_GetParentPath( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char,) -> *mut ::std::os::raw::c_char;
// pub fn Longtail_Storage_MapFile( storage_api: *mut Longtail_StorageAPI, f: Longtail_StorageAPI_HOpenFile, offset: u64, length: u64, out_file_map: *mut Longtail_StorageAPI_HFileMap, out_data_ptr: *mut *const ::std::os::raw::c_void,) -> ::std::os::raw::c_int;
// pub fn Longtail_Storage_UnmapFile( storage_api: *mut Longtail_StorageAPI, m: Longtail_StorageAPI_HFileMap,);
// pub fn Longtail_Storage_OpenAppendfile( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char, out_open_file: *mut Longtail_StorageAPI_HOpenFile,);
// pub fn Longtail_CreateBlockStoreStorageAPI( hash_api: *mut Longtail_HashAPI, job_api: *mut Longtail_JobAPI, block_store: *mut Longtail_BlockStoreAPI, store_index: *mut Longtail_StoreIndex, version_index: *mut Longtail_VersionIndex,) -> *mut Longtail_StorageAPI;
// pub fn Longtail_CreateFSStorageAPI() -> *mut Longtail_StorageAPI;
// pub fn Longtail_CreateInMemStorageAPI() -> *mut Longtail_StorageAPI;
//
// struct Longtail_StorageAPI_EntryProperties
// {
//     const char* m_Name;
//     uint64_t m_Size;
//     uint16_t m_Permissions;
//     int m_IsDir;
// };
//
// struct Longtail_StorageAPI
// {
//     struct Longtail_API m_API;
//     Longtail_Storage_OpenReadFileFunc OpenReadFile;
//     Longtail_Storage_GetSizeFunc GetSize;
//     Longtail_Storage_ReadFunc Read;
//     Longtail_Storage_OpenWriteFileFunc OpenWriteFile;
//     Longtail_Storage_WriteFunc Write;
//     Longtail_Storage_SetSizeFunc SetSize;
//     Longtail_Storage_SetPermissionsFunc SetPermissions;
//     Longtail_Storage_GetPermissionsFunc GetPermissions;
//     Longtail_Storage_CloseFileFunc CloseFile;
//     Longtail_Storage_CreateDirFunc CreateDir;
//     Longtail_Storage_RenameFileFunc RenameFile;
//     Longtail_Storage_ConcatPathFunc ConcatPath;
//     Longtail_Storage_IsDirFunc IsDir;
//     Longtail_Storage_IsFileFunc IsFile;
//     Longtail_Storage_RemoveDirFunc RemoveDir;
//     Longtail_Storage_RemoveFileFunc RemoveFile;
//     Longtail_Storage_StartFindFunc StartFind;
//     Longtail_Storage_FindNextFunc FindNext;
//     Longtail_Storage_CloseFindFunc CloseFind;
//     Longtail_Storage_GetEntryPropertiesFunc GetEntryProperties;
//     Longtail_Storage_LockFileFunc LockFile;
//     Longtail_Storage_UnlockFileFunc UnlockFile;
//     Longtail_Storage_GetParentPathFunc GetParentPath;
//     Longtail_Storage_MapFileFunc MapFile;
//     Longtail_Storage_UnmapFileFunc UnMapFile;
//     Longtail_Storage_OpenAppendFileFunc OpenAppendFile;
// };


/// Represents a full path in the storage API.
#[repr(C)]
#[derive(Debug, Clone)]
pub(crate) struct CFullPath {
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
    pub(crate) unsafe fn new(
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

/// Represents an open file handle in the storage API.
#[repr(C)]
#[derive(Debug, Clone)]
pub(crate) struct COpenFile {
    pub storage_api: *mut Longtail_StorageAPI,
    pub open_file: Longtail_StorageAPI_HOpenFile,
}

impl Drop for COpenFile {
    fn drop(&mut self) {
        unsafe { Longtail_Storage_CloseFile(self.storage_api, self.open_file) };
        let open_file = unsafe { Box::from_raw(self.open_file) };
        drop(open_file)
    }
}

impl Deref for COpenFile {
    type Target = *mut Longtail_StorageAPI_OpenFile;
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
    pub(crate) unsafe fn new_for_write(
        storage_api: *mut Longtail_StorageAPI,
        c_full_path: *const c_char,
        block_size: u64,
    ) -> Result<COpenFile, i32> {
        let open_file_h = Box::into_raw(Box::new(0 as Longtail_StorageAPI_HOpenFile));
        let result = unsafe {
            Longtail_Storage_OpenWriteFile(
                storage_api,
                c_full_path,
                block_size,
                open_file_h as *mut Longtail_StorageAPI_HOpenFile,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(COpenFile {
            storage_api,
            open_file: *open_file_h,
        })
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub(crate) unsafe fn new_for_read(
        storage_api: *mut Longtail_StorageAPI,
        c_full_path: *const c_char,
    ) -> Result<COpenFile, i32> {
        let open_file_h = Box::into_raw(Box::new(0 as Longtail_StorageAPI_HOpenFile));
        let result = unsafe {
            Longtail_Storage_OpenReadFile(
                storage_api,
                c_full_path,
                open_file_h as *mut Longtail_StorageAPI_HOpenFile,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(COpenFile {
            storage_api,
            open_file: *open_file_h,
        })
    }

    pub(crate) fn read(&self, offset: u64, size: u64) -> Result<Vec<u8>, i32> {
        let mut buffer = vec![0u8; size as usize];
        let result = unsafe {
            Longtail_Storage_Read(
                self.storage_api,
                self.open_file,
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

/// The longtail storage API abstracts file and directory operations.
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

// TODO: Do we want to expose the full implmentation here? If so, we need to
// wrap the rest of the C pointers in Rust types. If not, restrict visibility to
// the public API.
#[allow(dead_code)]
impl StorageAPI {
    pub fn new(
        hash_api: &HashAPI,
        job_api: &BikeshedJobAPI,
        block_store: &BlockstoreAPI,
        store_index: &StoreIndex,
        version_index: &VersionIndex,
    ) -> StorageAPI {
        let blockstore_api = unsafe {
            Longtail_CreateBlockStoreStorageAPI(
                **hash_api,
                **job_api,
                **block_store,
                **store_index,
                **version_index,
            )
        };
        StorageAPI {
            storage_api: blockstore_api,
            _pin: std::marker::PhantomPinned,
        }
    }

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
        let c_open_file = unsafe { COpenFile::new_for_read(self.storage_api, *c_full_path)? };
        let mut file_size = 0;
        let result = unsafe {
            Longtail_Storage_GetSize(self.storage_api, *c_open_file, &mut file_size as *mut u64)
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

        let c_open_file =
            unsafe { COpenFile::new_for_write(self.storage_api, *c_full_path, block_size)? };
        if block_size > 0 {
            let result = unsafe {
                Longtail_Storage_Write(
                    self.storage_api,
                    *c_open_file,
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

    pub fn read_version_index(&self, path: &str) -> Result<VersionIndex, i32> {
        let c_path = std::ffi::CString::new(path).unwrap();
        let mut version_index = std::ptr::null_mut::<Longtail_VersionIndex>();
        let result = unsafe {
            Longtail_ReadVersionIndex(
                self.storage_api,
                c_path.as_ptr(),
                &mut version_index as *mut *mut Longtail_VersionIndex,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(VersionIndex::new_from_lt(version_index))
    }

    pub fn file_exists(&self, path: &str) -> bool {
        let c_path = std::ffi::CString::new(path).unwrap();
        unsafe { Longtail_Storage_IsFile(self.storage_api, c_path.as_ptr()) == 1 }
    }

    pub fn delete_file(&self, path: &str) -> Result<(), i32> {
        let c_path = std::ffi::CString::new(path).unwrap();
        let result = unsafe { Longtail_Storage_RemoveFile(self.storage_api, c_path.as_ptr()) };
        if result != 0 {
            return Err(result);
        }
        Ok(())
    }

    // TODO: Not sure this belongs here, because we have COpenFile, but modeling
    // golongtail for now.
    pub(crate) fn get_size(&self, f: COpenFile) -> Result<u64, i32> {
        let mut size = 0;
        let result = unsafe { Longtail_Storage_GetSize(self.storage_api, f.open_file, &mut size) };
        if result != 0 {
            return Err(result);
        }
        Ok(size)
    }

    pub(crate) fn read(&self, f: COpenFile, offset: u64, size: u64) -> Result<Vec<u8>, i32> {
        let mut buffer = vec![0u8; size as usize];
        let result = unsafe {
            Longtail_Storage_Read(
                self.storage_api,
                f.open_file,
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
    pub(crate) fn close_file(&self, f: COpenFile) {
        unsafe { Longtail_Storage_CloseFile(self.storage_api, f.open_file) };
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

    pub fn write_version_index(&self, version_index: &VersionIndex, path: &str) -> Result<(), i32> {
        let c_path = std::ffi::CString::new(path).unwrap();
        let result = unsafe {
            Longtail_WriteVersionIndex(
                self.storage_api,
                version_index.version_index,
                c_path.as_ptr(),
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_in_mem_storage() {
        let _guard = crate::init_logging().unwrap();
        let storage_api = crate::StorageAPI::new_inmem();
        let buffer = vec![1; 1024 * 1024];
        let result = storage_api.write_to_storage("folder", "file", buffer.as_slice());
        if result.is_err() {
            panic!("Failed to write to storage {:?}", result)
        }
        let read_buffer = storage_api.read_from_storage("folder", "file").unwrap();
        assert_eq!(buffer, read_buffer);
    }
}
