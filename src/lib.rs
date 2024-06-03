#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

pub fn logcontext(log_context: Longtail_LogContext) {
    let file = unsafe { std::ffi::CStr::from_ptr(log_context.file) };
    let function = unsafe { std::ffi::CStr::from_ptr(log_context.function) };
    println!("LogContext: {:?}", log_context);
    println!("LogContext.File: {:?}", file);
    println!("LogContext.Function: {:?}", function);
    for n in 1..=log_context.field_count as isize {
        let field = unsafe { *log_context.fields.offset(n - 1) };
        let name = unsafe { std::ffi::CStr::from_ptr(field.name) };
        let value = unsafe { std::ffi::CStr::from_ptr(field.value) };
        println!("Field {:?}: {:?}", name, value);
    }
}

pub fn setup_logging() {
    unsafe {
        Longtail_SetLogLevel(0);
        Longtail_SetLog(Some(log_callback), std::ptr::null_mut());
    }
    println!("Log Level: {0}", unsafe { Longtail_GetLogLevel() });
}

unsafe extern "C" fn log_callback(
    context: *mut Longtail_LogContext,
    log: *const std::os::raw::c_char,
) {
    let log = unsafe { std::ffi::CStr::from_ptr(log) };
    let context = unsafe { *context };
    logcontext(context);
    println!("Log: {}", log.to_str().unwrap());
}

#[repr(C)]
pub struct VersionIndex {
    pub version_index: *mut Longtail_VersionIndex,
    _pin: std::marker::PhantomPinned,
}

impl Drop for VersionIndex {
    fn drop(&mut self) {
        let version_index_ptr = self.version_index as *mut _ as *mut std::ffi::c_void;
        println!("Freeing VersionIndex: {:?}", version_index_ptr);
        unsafe { Longtail_Free(version_index_ptr) }
    }
}

impl std::fmt::Debug for VersionIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VersionIndex")
            .field("Raw pointer", &self.version_index)
            .field("m_Version", &self.get_version())
            .field("m_HashIdentifier", &self.get_hash_identifier())
            .field("m_TargetChunkSize", &self.get_target_chunk_size())
            .field("m_AssetCount", &self.get_asset_count())
            .field("m_ChunkCount", &self.get_chunk_count())
            .field("m_AssetChunkCounts", &self.get_asset_chunk_counts())
            .field("m_PathHashes", &self.get_path_hashes())
            .field("m_ContentHashes", &self.get_asset_hashes())
            .field("m_AssetSizes", &self.get_asset_sizes())
            .field("m_AssetChunkCounts", &self.get_asset_chunk_counts())
            .field(
                "m_AssetChunkIndexStarts",
                &self.get_asset_chunk_index_starts(),
            )
            .field("m_AssetChunkIndexes", &self.get_asset_chunk_indexes())
            // TODO(cm): Not sure why, but this one errors out
            // unsafe precondition(s) violated: slice::from_raw_parts requires the pointer to be aligned and non-null, and the total size of the slice not to exceed `isize::MAX`
            // .field("m_ChunkHashes", &self.get_chunk_hashes())
            .field("m_ChunkSizes", &self.get_chunk_sizes())
            .field("m_ChunkTags", &self.get_chunk_tags())
            .field("m_NameOffsets", &self.get_name_offsets())
            .field("m_NameDataSize", &self.get_name_data_size())
            .field("m_Permissions", &self.get_permissions())
            .field("m_NameData", &self.get_name_data())
            .finish()
    }
}

impl VersionIndex {
    pub fn read_version_index_from_buffer(buffer: &mut [u8]) -> Result<VersionIndex, i32> {
        let buffer_size = buffer.len();
        let mut version_index = std::ptr::null_mut::<Longtail_VersionIndex>();
        println!("Buffer address: {:p}", buffer.as_ptr());
        let result = unsafe {
            Longtail_ReadVersionIndexFromBuffer(
                buffer.as_ptr().cast(),
                buffer_size,
                &mut version_index,
            )
        };
        println!("Result: {}", result);
        println!("Version Index: {:?}", version_index);
        if result != 0 {
            return Err(result);
        }
        // Ok(unsafe { *version_index })
        Ok(VersionIndex {
            version_index,
            _pin: std::marker::PhantomPinned,
        })
    }

    pub fn get_version(&self) -> u32 {
        unsafe { *(*self.version_index).m_Version }
    }
    pub fn get_hash_identifier(&self) -> u32 {
        unsafe { *(*self.version_index).m_HashIdentifier }
    }
    pub fn get_target_chunk_size(&self) -> u32 {
        unsafe { *(*self.version_index).m_TargetChunkSize }
    }
    pub fn get_asset_count(&self) -> u32 {
        unsafe { *(*self.version_index).m_AssetCount }
    }
    pub fn get_asset_path(&self, index: u32) -> String {
        let offset = unsafe { *(*self.version_index).m_NameOffsets.offset(index as isize) };
        let size = unsafe {
            *(*self.version_index)
                .m_NameOffsets
                .offset(index as isize + 1)
        } - offset;
        let name_data: &[u8] = unsafe {
            std::slice::from_raw_parts(
                (*self.version_index).m_NameData as *const u8,
                size.try_into().unwrap(),
            )
        };
        let name = std::str::from_utf8(name_data).unwrap();
        name.to_string()
    }
    pub fn get_asset_hashes(&self) -> Vec<u64> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let hashes =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_ContentHashes, count) };
        hashes.to_vec()
    }
    pub fn get_asset_size(&self, index: u32) -> u64 {
        unsafe { *(*self.version_index).m_AssetSizes.offset(index as isize) }
    }
    pub fn get_asset_permissions(&self, index: u32) -> u16 {
        unsafe { *(*self.version_index).m_Permissions.offset(index as isize) }
    }
    pub fn get_asset_chunk_counts(&self) -> Vec<u32> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let chunk_counts =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_AssetChunkCounts, count) };
        chunk_counts.to_vec()
    }
    pub fn get_asset_chunk_index_starts(&self) -> Vec<u32> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let starts = unsafe {
            std::slice::from_raw_parts((*self.version_index).m_AssetChunkIndexStarts, count)
        };
        starts.to_vec()
    }
    pub fn get_asset_chunk_indexes(&self) -> Vec<u32> {
        let count = unsafe { *(*self.version_index).m_AssetChunkIndexCount } as usize;
        let indexes =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_AssetChunkIndexes, count) };
        indexes.to_vec()
    }
    pub fn get_chunk_count(&self) -> u32 {
        unsafe { *(*self.version_index).m_ChunkCount }
    }
    pub fn get_chunk_hashes(&self) -> Vec<u64> {
        let count = unsafe { *(*self.version_index).m_ChunkCount } as usize;
        let hashes =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_ChunkHashes, count) };
        hashes.to_vec()
    }
    pub fn get_chunk_sizes(&self) -> Vec<u32> {
        let count = unsafe { *(*self.version_index).m_ChunkCount } as usize;
        let sizes =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_ChunkSizes, count) };
        sizes.to_vec()
    }
    pub fn get_asset_sizes(&self) -> Vec<u64> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let sizes =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_AssetSizes, count) };
        sizes.to_vec()
    }
    pub fn get_chunk_tags(&self) -> Vec<u32> {
        let count = unsafe { *(*self.version_index).m_ChunkCount } as usize;
        let tags = unsafe { std::slice::from_raw_parts((*self.version_index).m_ChunkTags, count) };
        tags.to_vec()
    }
    pub fn get_asset_chunk_index_count(&self) -> u32 {
        unsafe { *(*self.version_index).m_AssetChunkIndexCount }
    }
    pub fn get_path_hashes(&self) -> Vec<u64> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let hashes =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_PathHashes, count) };
        hashes.to_vec()
    }
    pub fn get_name_offsets(&self) -> Vec<u32> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let offsets =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_NameOffsets, count) };
        offsets.to_vec()
    }
    pub fn get_name_data_size(&self) -> u32 {
        unsafe { (*self.version_index).m_NameDataSize }
    }
    pub fn get_permissions(&self) -> Vec<u16> {
        let count = unsafe { *(*self.version_index).m_AssetCount } as usize;
        let permissions =
            unsafe { std::slice::from_raw_parts((*self.version_index).m_Permissions, count) };
        permissions.to_vec()
    }
    pub fn get_name_data(&self) -> String {
        let size = self.get_name_data_size() as usize;
        let name_data: &[u8] = unsafe {
            std::slice::from_raw_parts((*self.version_index).m_NameData as *const u8, size)
        };
        let name = std::str::from_utf8(name_data).unwrap();
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_read_version_index_from_buffer() {
        // let mut buffer = [0u8; 1024];
        // let result = read_version_index_from_buffer(&mut buffer);
        // assert_eq!(result, Err(9)); // Empty buffer

        setup_logging();

        let mut f = std::fs::File::open("test-data/storage/testdir/store.lsi").unwrap();
        let metadata = f.metadata().unwrap();
        let mut buffer = vec![0u8; metadata.len() as usize];
        println!("Reading {} bytes", metadata.len());
        f.read_exact(&mut buffer).unwrap();
        println!("Bytes read: {:?}", buffer);
        let result = VersionIndex::read_version_index_from_buffer(&mut buffer);
        // assert_eq!(
        //     result,
        //     Ok(Longtail_VersionIndex {
        //         m_Version: todo!(),
        //         m_HashIdentifier: todo!(),
        //         m_TargetChunkSize: todo!(),
        //         m_AssetCount: todo!(),
        //         m_ChunkCount: todo!(),
        //         m_AssetChunkIndexCount: todo!(),
        //         m_PathHashes: todo!(),
        //         m_ContentHashes: todo!(),
        //         m_AssetSizes: todo!(),
        //         m_AssetChunkCounts: todo!(),
        //         m_AssetChunkIndexStarts: todo!(),
        //         m_AssetChunkIndexes: todo!(),
        //         m_ChunkHashes: todo!(),
        //         m_ChunkSizes: todo!(),
        //         m_ChunkTags: todo!(),
        //         m_NameOffsets: todo!(),
        //         m_NameDataSize: todo!(),
        //         m_Permissions: todo!(),
        //         m_NameData: todo!()
        //     })
        // );
        println!("Result: {:?}", result);
        assert_eq!(1, 0);
    }
}
