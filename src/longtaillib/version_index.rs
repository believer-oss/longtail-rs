use crate::{
    hash::HashType, BikeshedJobAPI, ChunkerAPI, FileInfos, HashAPI, Longtail_CreateVersionIndex,
    Longtail_Free, Longtail_ProgressAPI, Longtail_ReadVersionIndexFromBuffer,
    Longtail_VersionIndex, ProgressAPIProxy, StorageAPI,
};
use std::{
    ops::{Deref, DerefMut},
    ptr::null_mut,
};

#[rustfmt::skip]
// Version Index API
// pub fn Longtail_VersionIndex_GetVersion(version_index: *const Longtail_VersionIndex) -> u32;
// pub fn Longtail_VersionIndex_GetHashAPI(version_index: *const Longtail_VersionIndex) -> u32;
// pub fn Longtail_VersionIndex_GetAssetCount(version_index: *const Longtail_VersionIndex) -> u32;
// pub fn Longtail_VersionIndex_GetChunkCount(version_index: *const Longtail_VersionIndex) -> u32;
// pub fn Longtail_VersionIndex_GetChunkHashes( version_index: *const Longtail_VersionIndex,) -> *const TLongtail_Hash;
// pub fn Longtail_VersionIndex_GetChunkSizes( version_index: *const Longtail_VersionIndex,) -> *const u32;
// pub fn Longtail_VersionIndex_GetChunkTags( version_index: *const Longtail_VersionIndex,) -> *const u32;
// pub fn Longtail_BuildVersionIndex( mem: *mut ::std::os::raw::c_void, mem_size: usize, file_infos: *const Longtail_FileInfos, path_hashes: *const TLongtail_Hash, content_hashes: *const TLongtail_Hash, asset_chunk_index_starts: *const u32, asset_chunk_counts: *const u32, asset_chunk_index_count: u32, asset_chunk_indexes: *const u32, chunk_count: u32, chunk_sizes: *const u32, chunk_hashes: *const TLongtail_Hash, optional_chunk_tags: *const u32, hash_api_identifier: u32, target_chunk_size: u32, out_version_index: *mut *mut Longtail_VersionIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_CreateVersionIndex( storage_api: *mut Longtail_StorageAPI, hash_api: *mut Longtail_HashAPI, chunker_api: *mut Longtail_ChunkerAPI, job_api: *mut Longtail_JobAPI, progress_api: *mut Longtail_ProgressAPI, optional_cancel_api: *mut Longtail_CancelAPI, optional_cancel_token: Longtail_CancelAPI_HCancelToken, root_path: *const ::std::os::raw::c_char, file_infos: *const Longtail_FileInfos, optional_asset_tags: *const u32, target_chunk_size: u32, enable_file_map: ::std::os::raw::c_int, out_version_index: *mut *mut Longtail_VersionIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_MergeVersionIndex( base_version_index: *const Longtail_VersionIndex, overlay_version_index: *const Longtail_VersionIndex, out_version_index: *mut *mut Longtail_VersionIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_WriteVersionIndexToBuffer( version_index: *const Longtail_VersionIndex, out_buffer: *mut *mut ::std::os::raw::c_void, out_size: *mut usize,) -> ::std::os::raw::c_int;
// pub fn Longtail_ReadVersionIndexFromBuffer( buffer: *const ::std::os::raw::c_void, size: usize, out_version_index: *mut *mut Longtail_VersionIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_WriteVersionIndex( storage_api: *mut Longtail_StorageAPI, version_index: *mut Longtail_VersionIndex, path: *const ::std::os::raw::c_char,) -> ::std::os::raw::c_int;
// pub fn Longtail_ReadVersionIndex( storage_api: *mut Longtail_StorageAPI, path: *const ::std::os::raw::c_char, out_version_index: *mut *mut Longtail_VersionIndex,) -> ::std::os::raw::c_int;
// pub fn Longtail_GetVersionIndexSize( asset_count: u32, chunk_count: u32, asset_chunk_index_count: u32, path_data_size: u32,) -> usize;
// pub fn Longtail_GetRequiredChunkHashes( version_index: *const Longtail_VersionIndex, version_diff: *const Longtail_VersionDiff, out_chunk_count: *mut u32, out_chunk_hashes: *mut TLongtail_Hash,) -> ::std::os::raw::c_int;
//
// struct Longtail_VersionIndex
// {
//     uint32_t* m_Version;
//     uint32_t* m_HashIdentifier;
//     uint32_t* m_TargetChunkSize;
//     uint32_t* m_AssetCount;
//     uint32_t* m_ChunkCount;
//     uint32_t* m_AssetChunkIndexCount;
//     TLongtail_Hash* m_PathHashes;       // []
//     TLongtail_Hash* m_ContentHashes;    // []
//     uint64_t* m_AssetSizes;             // []
//     uint32_t* m_AssetChunkCounts;       // []
//     // uint64_t* m_CreationDates;       // []
//     // uint64_t* m_ModificationDates;   // []
//     uint32_t* m_AssetChunkIndexStarts;  // []
//     uint32_t* m_AssetChunkIndexes;      // []
//     TLongtail_Hash* m_ChunkHashes;      // []
//
//     uint32_t* m_ChunkSizes;             // []
//     uint32_t* m_ChunkTags;              // []
//
//     uint32_t* m_NameOffsets;            // []
//     uint32_t m_NameDataSize;
//     uint16_t* m_Permissions;            // []
//     char* m_NameData;
// };

/// A version index in the Longtail API consists of pointers to the internal version identifier,
/// hash type, path data, and all of the chunks needed to reconstruct this version from the store.
#[repr(C)]
pub struct VersionIndex {
    pub version_index: *mut Longtail_VersionIndex,
    _pin: std::marker::PhantomPinned,
}

impl Deref for VersionIndex {
    type Target = *mut Longtail_VersionIndex;
    fn deref(&self) -> &Self::Target {
        &self.version_index
    }
}

impl DerefMut for VersionIndex {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.version_index
    }
}

// This would be better as field_with, but it's not stable yet.
// https://doc.rust-lang.org/std/fmt/struct.DebugStruct.html#method.field_with
fn display_x<T>(i: usize, v: &[T], continuation: bool) -> String
where
    T: std::fmt::Debug + std::string::ToString,
{
    let end = if continuation { ", ...]" } else { "]" };
    String::from("[")
        + &v.iter()
            .take(i)
            .map(|s| s.to_string())
            .collect::<Vec<String>>()
            .join(", ")
        + end
}

impl Drop for VersionIndex {
    fn drop(&mut self) {
        let version_index_ptr = self.version_index as *mut _ as *mut std::ffi::c_void;
        unsafe { Longtail_Free(version_index_ptr) }
    }
}

impl std::fmt::Debug for VersionIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m_version = self.get_version();
        let m_hash_identifier = self.get_hash_identifier();
        let m_target_chunk_size = self.get_target_chunk_size();
        let m_asset_count = self.get_asset_count();
        let m_chunk_count = self.get_chunk_count();
        let m_asset_chunk_index_count = self.get_asset_chunk_index_count();
        // let m_path_hashes = self.get_path_hashes();
        // let m_content_hashes = self.get_asset_hashes();
        // let m_asset_sizes = self.get_asset_sizes();
        // let m_asset_chunk_counts = self.get_asset_chunk_counts();
        // let m_asset_chunk_index_starts = self.get_asset_chunk_index_starts();
        // let m_asset_chunk_indexes = self.get_asset_chunk_indexes();
        // let m_chunk_hashes = self.get_chunk_hashes();
        // let m_chunk_sizes = self.get_chunk_sizes();
        // let m_chunk_tags = self.get_chunk_tags();
        // let m_name_offsets = self.get_name_offsets();
        let m_name_data_size = self.get_name_data_size();
        // let m_permissions = self.get_permissions();
        // let m_name_data = self.get_name_data();

        let (asset_to_show, asset_cont) = if m_asset_count > 5 {
            (5_usize, true)
        } else {
            (m_asset_count as usize, false)
        };

        let m_path_hashes = display_x(asset_to_show, &self.get_path_hashes(), asset_cont);
        let m_content_hashes = display_x(asset_to_show, &self.get_asset_hashes(), asset_cont);
        let m_asset_sizes = display_x(asset_to_show, &self.get_asset_sizes(), asset_cont);
        let m_asset_chunk_counts =
            display_x(asset_to_show, &self.get_asset_chunk_counts(), asset_cont);
        let m_name_offsets = display_x(asset_to_show, &self.get_name_offsets(), asset_cont);
        let m_permissions = display_x(asset_to_show, &self.get_permissions(), asset_cont);
        let m_name_data = display_x(asset_to_show, &self.get_name_data(), asset_cont);
        let m_asset_chunk_index_starts = display_x(
            asset_to_show,
            &self.get_asset_chunk_index_starts(),
            asset_cont,
        );

        let (chunk_index_to_show, chunk_index_cont) = if m_asset_chunk_index_count > 5 {
            (5_usize, true)
        } else {
            (m_asset_chunk_index_count as usize, false)
        };
        let m_asset_chunk_indexes = display_x(
            chunk_index_to_show,
            &self.get_asset_chunk_indexes(),
            chunk_index_cont,
        );

        let (chunk_to_show, chunk_cont) = if m_chunk_count > 5 {
            (5_usize, true)
        } else {
            (m_chunk_count as usize, false)
        };
        // Fixed? get_chunk_hashes() works now, but do other accessors need to be fixed?
        // This accessor triggers UB in a test file, but it seems like it could be any
        // of the slice::from_raw_parts calls.
        // unsafe precondition(s) violated: slice::from_raw_parts requires the pointer
        // to be aligned and non-null, and the total size of the slice not to
        // exceed `isize::MAX`
        //
        let m_chunk_hashes = display_x(chunk_to_show, &self.get_chunk_hashes(), chunk_cont);
        let m_chunk_sizes = display_x(chunk_to_show, &self.get_chunk_sizes(), chunk_cont);
        let m_chunk_tags = display_x(chunk_to_show, &self.get_chunk_tags(), chunk_cont);

        f.debug_struct("VersionIndex")
            .field("m_Version", &m_version)
            .field("m_HashIdentifier", &m_hash_identifier)
            .field(
                "m_HashIdentifier",
                &HashType::from_repr(m_hash_identifier as usize).unwrap(),
            )
            .field("m_TargetChunkSize", &m_target_chunk_size)
            .field("m_AssetCount", &m_asset_count)
            .field("m_ChunkCount", &m_chunk_count)
            .field("m_AssetChunkIndexCount", &m_asset_chunk_index_count)
            .field("m_PathHashes", &m_path_hashes)
            .field("m_ContentHashes", &m_content_hashes)
            .field("m_AssetSizes", &m_asset_sizes)
            .field("m_AssetChunkCounts", &m_asset_chunk_counts)
            .field("m_AssetChunkIndexStarts", &m_asset_chunk_index_starts)
            .field("m_AssetChunkIndexes", &m_asset_chunk_indexes)
            .field("m_ChunkHashes", &m_chunk_hashes)
            .field("m_ChunkSizes", &m_chunk_sizes)
            .field("m_ChunkTags", &m_chunk_tags)
            .field("m_NameOffsets", &m_name_offsets)
            .field("m_NameDataSize", &m_name_data_size)
            .field("m_Permissions", &m_permissions)
            .field("m_NameData", &m_name_data)
            .finish()
    }
}

impl VersionIndex {
    /// Create a new `VersionIndex` from a `FileInfos` struct, by chunking each
    /// file and hashing the resulting chunks.
    #[allow(clippy::too_many_arguments)]
    pub fn new_from_fileinfos(
        storage_api: &StorageAPI,
        hash_api: &HashAPI,
        chunker_api: &ChunkerAPI,
        job_api: &BikeshedJobAPI,
        progress_api: &ProgressAPIProxy,
        path: &str,
        validate_file_infos: FileInfos,
        max_chunk_size: u32,
        enable_file_mapping: bool,
    ) -> Result<VersionIndex, i32> {
        let path = std::ffi::CString::new(path).unwrap();
        let mut version_index = std::ptr::null_mut::<Longtail_VersionIndex>();
        let result = unsafe {
            Longtail_CreateVersionIndex(
                **storage_api,
                **hash_api,
                **chunker_api,
                **job_api,
                progress_api as *const _ as *mut Longtail_ProgressAPI,
                null_mut(),
                null_mut(),
                path.as_ptr(),
                validate_file_infos.0,
                null_mut(),
                max_chunk_size,
                enable_file_mapping as i32,
                &mut version_index,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(VersionIndex {
            version_index,
            _pin: std::marker::PhantomPinned,
        })
    }

    /// Deserialize a `VersionIndex` from a buffer.
    pub fn new_from_buffer(buffer: &mut [u8]) -> Result<VersionIndex, i32> {
        let buffer_size = buffer.len();
        let mut version_index = std::ptr::null_mut::<Longtail_VersionIndex>();
        let result = unsafe {
            Longtail_ReadVersionIndexFromBuffer(
                buffer.as_ptr().cast(),
                buffer_size,
                &mut version_index,
            )
        };
        if result != 0 {
            return Err(result);
        }
        // Ok(unsafe { *version_index })
        Ok(VersionIndex {
            version_index,
            _pin: std::marker::PhantomPinned,
        })
    }

    pub(crate) fn new_from_lt(version_index: *mut Longtail_VersionIndex) -> VersionIndex {
        VersionIndex {
            version_index,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// Get the internal version identifier for this version index
    pub fn get_version(&self) -> u32 {
        unsafe { *(*self.version_index).m_Version }
    }
    /// Get the hash identifier for this version index
    pub fn get_hash_identifier(&self) -> u32 {
        unsafe { *(*self.version_index).m_HashIdentifier }
    }
    /// Get the target chunk size for this version index
    pub fn get_target_chunk_size(&self) -> u32 {
        unsafe { *(*self.version_index).m_TargetChunkSize }
    }
    /// Get the number of assets in this version index
    pub fn get_asset_count(&self) -> u32 {
        unsafe { *(*self.version_index).m_AssetCount }
    }
    /// Get the paths to the files in this version index
    pub fn get_asset_path(&self, index: u32) -> String {
        let offset = unsafe { *(*self.version_index).m_NameOffsets.offset(index as isize) };
        let name_data_start = unsafe { (*self.version_index).m_NameData.add(offset as usize) };
        let c_str = unsafe { std::ffi::CStr::from_ptr(name_data_start) };
        String::from_utf8_lossy(c_str.to_bytes()).to_string()
    }
    /// Get the hashes of the assets in this version index
    pub fn get_asset_hashes(&self) -> Vec<u64> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let hashes =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_ContentHashes, count) };
        hashes.to_vec()
    }
    /// Get the asset sizes of the assets in this version index
    pub fn get_asset_size(&self, index: u32) -> u64 {
        unsafe { *(*self.version_index).m_AssetSizes.offset(index as isize) }
    }
    /// Get the permissions of the assets in this version index
    pub fn get_asset_permissions(&self, index: u32) -> u16 {
        unsafe { *(*self.version_index).m_Permissions.offset(index as isize) }
    }
    /// Get the number of chunks in this version index
    pub fn get_asset_chunk_counts(&self) -> Vec<u32> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let chunk_counts =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_AssetChunkCounts, count) };
        chunk_counts.to_vec()
    }
    /// Get the start indexes of the asset chunks in this version index
    pub fn get_asset_chunk_index_starts(&self) -> Vec<u32> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let starts = unsafe {
            std::slice::from_raw_parts((*self.version_index).m_AssetChunkIndexStarts, count)
        };
        starts.to_vec()
    }
    /// Get the indexes of the asset chunks in this version index
    pub fn get_asset_chunk_indexes(&self) -> Vec<u32> {
        let count = unsafe { *(*self.version_index).m_AssetChunkIndexCount } as usize;
        let indexes =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_AssetChunkIndexes, count) };
        indexes.to_vec()
    }
    /// Get the number of chunks in this version index
    pub fn get_chunk_count(&self) -> u32 {
        unsafe { *(*self.version_index).m_ChunkCount }
    }
    /// Get the hashes of the chunks in this version index
    pub fn get_chunk_hashes(&self) -> Vec<u64> {
        let count = unsafe { *(*self.version_index).m_ChunkCount } as isize;

        // This prevents unaligned access to the chunk hashes.
        let unaligned = unsafe { (*self.version_index).m_ChunkHashes } as *const u64;
        let mut hashes = Vec::with_capacity(count.try_into().unwrap());
        for i in 0..count {
            let hash = unsafe { std::ptr::read_unaligned(unaligned.offset(i)) };
            hashes.push(hash);
        }
        hashes
    }
    /// Get the sizes of the chunks in this version index
    pub fn get_chunk_sizes(&self) -> Vec<u32> {
        let count = unsafe { *(*self.version_index).m_ChunkCount } as usize;
        let sizes =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_ChunkSizes, count) };
        sizes.to_vec()
    }
    /// Get the sizes of the assets in this version index
    pub fn get_asset_sizes(&self) -> Vec<u64> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let sizes =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_AssetSizes, count) };
        sizes.to_vec()
    }
    /// Get the tags of the chunks in this version
    pub fn get_chunk_tags(&self) -> Vec<u32> {
        let count = unsafe { *(*self.version_index).m_ChunkCount } as usize;
        let tags = unsafe { std::slice::from_raw_parts((*self.version_index).m_ChunkTags, count) };
        tags.to_vec()
    }
    /// Get the number of asset chunk indexes in this version index
    pub fn get_asset_chunk_index_count(&self) -> u32 {
        unsafe { *(*self.version_index).m_AssetChunkIndexCount }
    }
    /// Get the hashes of the asset paths in this version index
    pub fn get_path_hashes(&self) -> Vec<u64> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let hashes =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_PathHashes, count) };
        hashes.to_vec()
    }
    /// Get the offsets of the asset names in this version index
    pub fn get_name_offsets(&self) -> Vec<u32> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let offsets =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_NameOffsets, count) };
        offsets.to_vec()
    }
    /// Get the sizes of the asset names in this version index
    pub fn get_name_data_size(&self) -> u32 {
        unsafe { (*self.version_index).m_NameDataSize }
    }
    /// Get the permissions of the assets in this version index
    pub fn get_permissions(&self) -> Vec<u16> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let permissions =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_Permissions, count) };
        permissions.to_vec()
    }
    /// Get the paths of the assets in this version index
    pub fn get_name_data(&self) -> Vec<String> {
        let size = self.get_name_data_size() as usize;
        let name_data: &[u8] = unsafe {
            std::slice::from_raw_parts((*self.version_index).m_NameData as *const u8, size)
        };
        name_data
            .split(|&c| c == 0)
            .map(|s| String::from_utf8(s.to_vec()).unwrap())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_read_version_index_from_buffer() {
        let _guard = crate::init_logging().unwrap();

        let mut f = std::fs::File::open("test-data/small/target-path/testdir.lvi").unwrap();
        let metadata = f.metadata().unwrap();
        let mut buffer = vec![0u8; metadata.len() as usize];
        println!("Reading {} bytes", metadata.len());
        f.read_exact(&mut buffer).unwrap();
        println!("Bytes read: {:?}", buffer);
        let result = VersionIndex::new_from_buffer(&mut buffer);
        println!("Result: {:?}", result);
        match result {
            Ok(version_index) => {
                assert_eq!(version_index.get_version(), 2);
                assert_eq!(version_index.get_hash_identifier(), 1651272499);
                assert_eq!(version_index.get_target_chunk_size(), 32768);
                assert_eq!(version_index.get_asset_count(), 1);
                assert_eq!(version_index.get_chunk_count(), 1);
                assert_eq!(version_index.get_asset_chunk_index_count(), 1);
                assert_eq!(version_index.get_path_hashes(), [17453309618399787745]);
                assert_eq!(version_index.get_asset_hashes(), [6929027089488294080]);
                assert_eq!(version_index.get_asset_sizes(), [4]);
                assert_eq!(version_index.get_asset_chunk_counts(), [1]);
                assert_eq!(version_index.get_asset_chunk_index_starts(), [0]);
                assert_eq!(version_index.get_asset_chunk_indexes(), [0]);
                assert_eq!(version_index.get_chunk_hashes(), [18030661544962586696]);
                assert_eq!(version_index.get_chunk_sizes(), [4]);
                assert_eq!(version_index.get_chunk_tags(), [2054448178]);
                assert_eq!(version_index.get_name_offsets(), [0]);
                assert_eq!(version_index.get_name_data_size(), 9);
                #[cfg(target_family = "unix")]
                assert_eq!(version_index.get_permissions(), [420]);
                #[cfg(target_family = "windows")]
                assert_eq!(version_index.get_permissions(), [438]);
                assert_eq!(version_index.get_name_data(), [String::from("testfile")]);
            }
            Err(e) => {
                panic!("Error reading version index from buffer: {:?}", e);
            }
        }
    }
}
