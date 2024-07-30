use std::ops::{Deref, DerefMut};

use crate::*;

// Redefining these consts here because enum values need to be const, and the
// longtail headers are exporting the underlying defines as functions.
// Another approach was attempted where we could copy the existing defines into
// header_contents blocks in build.rs, but that is blocked by:
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

pub const LONGTAIL_NO_COMPRESSION_TYPE: u32 = 0;

#[derive(EnumString, FromRepr, Debug, PartialEq)]
#[repr(usize)]
pub enum CompressionType {
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

#[repr(C)]
pub struct CompressionRegistry {
    pub compression_registry: *mut Longtail_CompressionRegistryAPI,
    _pin: std::marker::PhantomPinned,
}

impl Drop for CompressionRegistry {
    fn drop(&mut self) {
        unsafe {
            Longtail_DisposeAPI(&mut (*self.compression_registry).m_API as *mut Longtail_API)
        };
    }
}

impl Default for CompressionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for CompressionRegistry {
    type Target = *mut Longtail_CompressionRegistryAPI;
    fn deref(&self) -> &Self::Target {
        &self.compression_registry
    }
}

impl DerefMut for CompressionRegistry {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.compression_registry
    }
}

impl CompressionRegistry {
    pub fn new() -> CompressionRegistry {
        let compression_registry = unsafe { Longtail_CreateFullCompressionRegistry() };
        CompressionRegistry {
            compression_registry,
            _pin: std::marker::PhantomPinned,
        }
    }
    pub fn get_compression_api(
        &self,
        compression_type: CompressionType,
    ) -> Result<*mut Longtail_CompressionAPI, i32> {
        let mut compression_api = std::ptr::null_mut::<Longtail_CompressionAPI>();
        // TODO: Not sure what settings_id is, so stubbing it in for now.
        let settings_id = std::ptr::null_mut::<u32>();
        let result = unsafe {
            Longtail_GetCompressionRegistry_GetCompressionAPI(
                self.compression_registry,
                compression_type as u32,
                &mut compression_api,
                settings_id,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(compression_api)
    }

    pub fn create_full_compression_registry() -> CompressionRegistry {
        CompressionRegistry {
            compression_registry: unsafe { Longtail_CreateFullCompressionRegistry() },
            _pin: std::marker::PhantomPinned,
        }
    }
}
