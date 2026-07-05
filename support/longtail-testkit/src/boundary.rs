//! Chunk-boundary golden table types (always available; generation lives in the
//! `differential` module).
//!
//! A boundary table records, for a given (input, target chunk size, chunker
//! entry point), the ordered chunk boundaries and their longtail chunk hashes.
//! These tables double as hash goldens for the Stage 3 algorithm port.
//!
//! The HPCDC chunker has TWO entry points that seed the rolling hash
//! differently and can therefore produce DIFFERENT boundaries when `min > 48`
//! (`docs/format-spec.md` §6). Tables are labeled `streaming` (canonical —
//! golongtail's default, `--enable-file-mapping=false`) or `buffer`.

use serde::{Deserialize, Serialize};

/// Which chunker entry point produced a table.
pub const PATH_STREAMING: &str = "streaming";
pub const PATH_BUFFER: &str = "buffer";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkEntry {
    pub offset: u64,
    pub size: u32,
    /// 16-digit lowercase hex of the 64-bit longtail chunk hash.
    pub hash_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundaryTable {
    pub input_id: String,
    pub input_sha256: String,
    pub target_chunk_size: u32,
    /// `"streaming"` or `"buffer"` (see [`PATH_STREAMING`]/[`PATH_BUFFER`]).
    pub chunker_path: String,
    /// Hash algorithm the chunk hashes were computed with (always `"blake3"`
    /// here, matching golongtail's default and the committed store fixtures).
    pub hash_algorithm: String,
    pub chunks: Vec<ChunkEntry>,
}

impl BoundaryTable {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("serialize boundary table")
    }

    pub fn from_json(s: &str) -> serde_json::Result<BoundaryTable> {
        serde_json::from_str(s)
    }
}

/// Derive the HPCDC `(min, avg, max)` chunker parameters from a target chunk
/// size, exactly as `DynamicChunking` does (`docs/format-spec.md` §6):
/// `min = max(48, target/8)`, `avg = max(48, target/2)`,
/// `max = max(48, target*2)`.
pub fn chunker_params_for_target(target: u32) -> (u32, u32, u32) {
    const WINDOW: u32 = 48;
    let min = std::cmp::max(WINDOW, target / 8);
    let avg = std::cmp::max(WINDOW, target / 2);
    let max = std::cmp::max(WINDOW, target * 2);
    (min, avg, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_match_spec_defaults() {
        assert_eq!(chunker_params_for_target(32768), (4096, 16384, 65536));
        assert_eq!(chunker_params_for_target(1024), (128, 512, 2048));
        assert_eq!(chunker_params_for_target(131072), (16384, 65536, 262144));
        assert_eq!(
            chunker_params_for_target(1048576),
            (131072, 524288, 2097152)
        );
    }
}
