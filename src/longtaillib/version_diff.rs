use crate::{
    HashAPI, Longtail_CreateVersionDiff, Longtail_GetRequiredChunkHashes, Longtail_VersionDiff,
    VersionIndex,
};
use std::ops::{Deref, DerefMut};

#[repr(C)]
#[derive(Clone)]
pub struct VersionDiff {
    pub version_diff: *mut Longtail_VersionDiff,
    _pin: std::marker::PhantomPinned,
}

// impl Drop for VersionDiff {
//     fn drop(&mut self) {
//         // unsafe { Longtail_DisposeAPI(&mut (*self.version_diff).m_API as *mut Longtail_API) };
//     }
// }
impl Deref for VersionDiff {
    type Target = *mut Longtail_VersionDiff;
    fn deref(&self) -> &Self::Target {
        &self.version_diff
    }
}

impl DerefMut for VersionDiff {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.version_diff
    }
}

impl std::fmt::Debug for VersionDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VersionDiff")
            .field("version_diff", &self.version_diff)
            .field("m_SourceRemovedCount", &unsafe {
                *(*self.version_diff).m_SourceRemovedCount
            })
            .field("m_TargetAddedCount", &unsafe {
                *(*self.version_diff).m_TargetAddedCount
            })
            .field("m_ModifiedContentCount", &unsafe {
                *(*self.version_diff).m_ModifiedContentCount
            })
            .field("m_ModifiedPermissionsCount", &unsafe {
                *(*self.version_diff).m_ModifiedPermissionsCount
            })
            .field("m_SourceRemovedAssetIndexes", &unsafe {
                (*self.version_diff).m_SourceRemovedAssetIndexes
            })
            .field("m_TargetAddedAssetIndexes", &unsafe {
                (*self.version_diff).m_TargetAddedAssetIndexes
            })
            .field("m_SourceContentModifiedAssetIndexes", &unsafe {
                (*self.version_diff).m_SourceContentModifiedAssetIndexes
            })
            .field("m_TargetContentModifiedAssetIndexes", &unsafe {
                (*self.version_diff).m_TargetContentModifiedAssetIndexes
            })
            .field("m_SourcePermissionsModifiedAssetIndexes", &unsafe {
                (*self.version_diff).m_SourcePermissionsModifiedAssetIndexes
            })
            .field("m_TargetPermissionsModifiedAssetIndexes", &unsafe {
                (*self.version_diff).m_TargetPermissionsModifiedAssetIndexes
            })
            .finish()
    }
}

impl VersionDiff {
    pub fn diff(
        hash: &HashAPI,
        source_version_index: &VersionIndex,
        target_version_index: &VersionIndex,
    ) -> Result<VersionDiff, i32> {
        let mut version_diff = std::ptr::null_mut();
        let result = unsafe {
            Longtail_CreateVersionDiff(
                **hash,
                **source_version_index,
                **target_version_index,
                &mut version_diff,
            )
        };
        if result != 0 {
            return Err(result);
        }
        Ok(VersionDiff {
            version_diff,
            _pin: std::marker::PhantomPinned,
        })
    }

    pub fn get_required_chunk_hashes(
        &self,
        source_version_index: &VersionIndex,
    ) -> Result<Vec<u64>, i32> {
        let max_chunk_count = source_version_index.get_chunk_count() as usize;
        let mut chunk_hashes = Vec::with_capacity(max_chunk_count);
        let mut chunk_count = 0;
        let result = unsafe {
            Longtail_GetRequiredChunkHashes(
                source_version_index.version_index,
                self.version_diff,
                &mut chunk_count,
                chunk_hashes.as_mut_ptr(),
            )
        };
        if result != 0 {
            return Err(result);
        }
        unsafe { chunk_hashes.set_len(chunk_count as usize) }
        Ok(chunk_hashes)
    }
}

#[cfg(test)]
mod test {
    use crate::{HashRegistry, HashType};

    use super::*;
    use std::io::Read;

    #[test]
    fn test_version_diff() {
        let _guard = crate::init_logging().unwrap();
        let source_version_index = create_test_version_index("test-data/target-path/testdir.lvi");
        let target_version_index = create_test_version_index("test-data/target-path/testdir2.lvi");
        let hash = source_version_index.get_hash_identifier();
        let from_repr = HashType::from_repr(hash as usize);
        let hash_reg = HashRegistry::new();
        let hash_api = hash_reg
            .get_hash_api(from_repr.unwrap())
            .expect("Failed to create hash api");
        let version_diff =
            VersionDiff::diff(&hash_api, &source_version_index, &target_version_index)
                .expect("Failed to create version diff");
        let required_chunk_hashes = version_diff
            .get_required_chunk_hashes(&source_version_index)
            .expect("Failed to get required chunk hashes");
        assert_eq!(
            required_chunk_hashes.len(),
            source_version_index.get_chunk_count() as usize
        );
    }

    fn create_test_version_index(file: &str) -> VersionIndex {
        let mut f = std::fs::File::open(file).unwrap();
        let metadata = f.metadata().unwrap();
        let mut buffer = vec![0u8; metadata.len() as usize];
        f.read_exact(&mut buffer).unwrap();
        VersionIndex::read_version_index_from_buffer(&mut buffer).unwrap()
    }
}
