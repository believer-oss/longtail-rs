use crate::{
    HashAPI, Longtail_CreateVersionDiff, Longtail_GetRequiredChunkHashes, Longtail_VersionDiff,
    VersionIndex,
};
use std::ops::{Deref, DerefMut};

#[repr(C)]
#[derive(Debug, Clone)]
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
