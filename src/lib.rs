#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use strum::{EnumString, FromRepr};

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

pub fn setup_logging(level: u32) {
    unsafe {
        Longtail_SetLogLevel(level as i32);
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

// Redefining these consts here because enum values need to be const, and the
// longtail headers are exporting the underlying defines as functions.
// Another approach was attempted where we could copy the existing defines into header_contents
// blocks in build.rs, but that is blocked by:
// https://github.com/rust-lang/rust-bindgen/pull/2369
const LONGTAIL_BROTLI_COMPRESSION_TYPE: usize =
    (('b' as usize) << 24) + (('t' as usize) << 16) + (('l' as usize) << 8);
const LONGTAIL_BROTLI_GENERIC_MIN_QUALITY_TYPE: usize =
    LONGTAIL_BROTLI_COMPRESSION_TYPE + ('0' as usize);
const LONGTAIL_BROTLI_GENERIC_DEFAULT_QUALITY_TYPE: usize =
    LONGTAIL_BROTLI_COMPRESSION_TYPE + ('1' as usize);
const LONGTAIL_BROTLI_GENERIC_MAX_QUALITY_TYPE: usize =
    LONGTAIL_BROTLI_COMPRESSION_TYPE + ('2' as usize);
const LONGTAIL_BROTLI_TEXT_MIN_QUALITY_TYPE: usize =
    LONGTAIL_BROTLI_COMPRESSION_TYPE + ('a' as usize);
const LONGTAIL_BROTLI_TEXT_DEFAULT_QUALITY_TYPE: usize =
    LONGTAIL_BROTLI_COMPRESSION_TYPE + ('b' as usize);
const LONGTAIL_BROTLI_TEXT_MAX_QUALITY_TYPE: usize =
    LONGTAIL_BROTLI_COMPRESSION_TYPE + ('c' as usize);

const LONGTAIL_LZ4_DEFAULT_COMPRESSION_TYPE: usize =
    (('l' as usize) << 24) + (('z' as usize) << 16) + (('4' as usize) << 8) + ('2' as usize);

const LONGTAIL_ZSTD_COMPRESSION_TYPE: usize =
    (('z' as usize) << 24) + (('t' as usize) << 16) + (('d' as usize) << 8);
const LONGTAIL_ZSTD_MIN_COMPRESSION_TYPE: usize = LONGTAIL_ZSTD_COMPRESSION_TYPE + ('1' as usize);
const LONGTAIL_ZSTD_DEFAULT_COMPRESSION_TYPE: usize =
    LONGTAIL_ZSTD_COMPRESSION_TYPE + ('2' as usize);
const LONGTAIL_ZSTD_MAX_COMPRESSION_TYPE: usize = LONGTAIL_ZSTD_COMPRESSION_TYPE + ('3' as usize);
const LONGTAIL_ZSTD_HIGH_COMPRESSION_TYPE: usize = LONGTAIL_ZSTD_COMPRESSION_TYPE + ('4' as usize);
const LONGTAIL_ZSTD_LOW_COMPRESSION_TYPE: usize = LONGTAIL_ZSTD_COMPRESSION_TYPE + ('5' as usize);

const LONGTAIL_MEOW_HASH_TYPE: usize =
    (('m' as usize) << 24) + (('e' as usize) << 16) + (('o' as usize) << 8) + ('w' as usize);
const LONGTAIL_BLAKE2_HASH_TYPE: usize =
    (('b' as usize) << 24) + (('l' as usize) << 16) + (('k' as usize) << 8) + ('2' as usize);
const LONGTAIL_BLAKE3_HASH_TYPE: usize =
    (('b' as usize) << 24) + (('l' as usize) << 16) + (('k' as usize) << 8) + ('3' as usize);

#[derive(EnumString, FromRepr, Debug, PartialEq)]
#[repr(usize)]
enum CompressionType {
    #[strum(serialize = "none")]
    None = 0,
    #[strum(serialize = "brotli")]
    Brotli = LONGTAIL_BROTLI_GENERIC_DEFAULT_QUALITY_TYPE,
    #[strum(serialize = "brotli_min")]
    BrotliMin = LONGTAIL_BROTLI_GENERIC_MIN_QUALITY_TYPE,
    #[strum(serialize = "brotli_max")]
    BrotliMax = LONGTAIL_BROTLI_GENERIC_MAX_QUALITY_TYPE,
    #[strum(serialize = "brotli_text")]
    BrotliText = LONGTAIL_BROTLI_TEXT_DEFAULT_QUALITY_TYPE,
    #[strum(serialize = "brotli_text_min")]
    BrotliTextMin = LONGTAIL_BROTLI_TEXT_MIN_QUALITY_TYPE,
    #[strum(serialize = "brotli_text_max")]
    BrotliTextMax = LONGTAIL_BROTLI_TEXT_MAX_QUALITY_TYPE,
    #[strum(serialize = "lz4")]
    Lz4 = LONGTAIL_LZ4_DEFAULT_COMPRESSION_TYPE,
    #[strum(serialize = "zstd")]
    Zstd = LONGTAIL_ZSTD_DEFAULT_COMPRESSION_TYPE,
    #[strum(serialize = "zstd_min")]
    ZstdMin = LONGTAIL_ZSTD_MIN_COMPRESSION_TYPE,
    #[strum(serialize = "zstd_max")]
    ZstdMax = LONGTAIL_ZSTD_MAX_COMPRESSION_TYPE,
    #[strum(serialize = "zstd_high")]
    ZstdHigh = LONGTAIL_ZSTD_HIGH_COMPRESSION_TYPE,
    #[strum(serialize = "zstd_low")]
    ZstdLow = LONGTAIL_ZSTD_LOW_COMPRESSION_TYPE,
}

#[derive(EnumString, FromRepr, Debug, PartialEq)]
#[repr(usize)]
enum HashType {
    #[strum(serialize = "meow")]
    Meow = LONGTAIL_MEOW_HASH_TYPE,
    #[strum(serialize = "blake2")]
    Blake2 = LONGTAIL_BLAKE2_HASH_TYPE,
    #[strum(serialize = "blake3")]
    Blake3 = LONGTAIL_BLAKE3_HASH_TYPE,
}

#[repr(C)]
pub struct VersionIndex {
    pub version_index: *mut Longtail_VersionIndex,
    _pin: std::marker::PhantomPinned,
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
        println!("Freeing VersionIndex: {:?}", version_index_ptr);
        unsafe { Longtail_Free(version_index_ptr) }
    }
}

impl std::fmt::Debug for VersionIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let m_Version = self.get_version();
        let m_hash_identifier = self.get_hash_identifier();
        let m_target_chunk_size = self.get_target_chunk_size();
        let m_asset_count = self.get_asset_count();
        let m_chunk_count = self.get_chunk_count();
        let m_asset_chunk_index_count = self.get_asset_chunk_index_count();
        let m_name_data_size = self.get_name_data_size();
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
        // This accessor triggers UB in a test file, but it seems like it could be any of
        // the slice::from_raw_parts calls.
        // unsafe precondition(s) violated: slice::from_raw_parts requires the pointer to be
        // aligned and non-null, and the total size of the slice not to exceed `isize::MAX`
        //
        let m_chunk_hashes = display_x(chunk_to_show, &self.get_chunk_hashes(), chunk_cont);
        let m_chunk_sizes = display_x(chunk_to_show, &self.get_chunk_sizes(), chunk_cont);
        let m_chunk_tags = display_x(chunk_to_show, &self.get_chunk_tags(), chunk_cont);

        f.debug_struct("VersionIndex")
            .field("m_Version", &m_Version)
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
    pub fn get_name_data(&self) -> Vec<String> {
        let size = self.get_name_data_size() as usize;
        let name_data: &[u8] = unsafe {
            std::slice::from_raw_parts((*self.version_index).m_NameData as *const u8, size)
        };
        name_data
            .split(|&c| c == 0)
            .map(|s| String::from_utf8(s.to_vec()).unwrap())
            .collect()
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

        setup_logging(LONGTAIL_LOG_LEVEL_DEBUG);

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
