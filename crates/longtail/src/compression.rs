//! CLI-name → compression-ID mapping (golongtail's `compressionTypeMap`,
//! longtailutils.go:458-472), including the `zstd_low`/`zstd_high` → `zstd_max`
//! alias quirk. Used by the byte-gate (upsync-equivalent uniform tag) and
//! by upsync.

use longtail_core::compress::{BROTLI_FAMILY, LZ4_ID, ZSTD_FAMILY};

/// No-compression tag (`NoCompressionType`, longtailutils.go:487).
pub const NO_COMPRESSION: u32 = 0;

/// Resolve a golongtail compression-algorithm name to its ID. `None` for an
/// unknown name (`GetCompressionType` errors upstream).
pub fn compression_type_for_name(name: &str) -> Option<u32> {
    Some(match name {
        "none" => NO_COMPRESSION,
        "brotli" => BROTLI_FAMILY | u32::from(b'1'), // generic default
        "brotli_min" => BROTLI_FAMILY | u32::from(b'0'),
        "brotli_max" => BROTLI_FAMILY | u32::from(b'2'),
        "brotli_text" => BROTLI_FAMILY | u32::from(b'b'), // text default
        "brotli_text_min" => BROTLI_FAMILY | u32::from(b'a'),
        "brotli_text_max" => BROTLI_FAMILY | u32::from(b'c'),
        "lz4" => LZ4_ID,
        "zstd" => ZSTD_FAMILY | u32::from(b'2'), // default
        "zstd_min" => ZSTD_FAMILY | u32::from(b'1'),
        "zstd_max" => ZSTD_FAMILY | u32::from(b'3'),
        "zstd_high" => ZSTD_FAMILY | u32::from(b'3'), // alias → max (quirk)
        "zstd_low" => ZSTD_FAMILY | u32::from(b'3'),  // alias → max (quirk)
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_ids() {
        assert_eq!(compression_type_for_name("none"), Some(0));
        assert_eq!(compression_type_for_name("zstd"), Some(0x7a74_6432));
        assert_eq!(compression_type_for_name("zstd_min"), Some(0x7a74_6431));
        assert_eq!(compression_type_for_name("zstd_max"), Some(0x7a74_6433));
        // Alias quirk: both map to max.
        assert_eq!(compression_type_for_name("zstd_high"), Some(0x7a74_6433));
        assert_eq!(compression_type_for_name("zstd_low"), Some(0x7a74_6433));
        assert_eq!(compression_type_for_name("lz4"), Some(0x6c7a_3432));
        assert_eq!(compression_type_for_name("brotli"), Some(0x6274_6c31));
        assert_eq!(compression_type_for_name("brotli_text"), Some(0x6274_6c62));
        assert_eq!(compression_type_for_name("nope"), None);
    }
}
