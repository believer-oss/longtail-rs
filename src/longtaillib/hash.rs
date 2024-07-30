use std::ops::{
    Deref,
    DerefMut,
};

use crate::*;

// Redefining these consts here because enum values need to be const, and the
// longtail headers are exporting the underlying defines as functions.
// Another approach was attempted where we could copy the existing defines into
// header_contents blocks in build.rs, but that is blocked by:
// https://github.com/rust-lang/rust-bindgen/pull/2369
const LONGTAIL_MEOW_HASH_TYPE: usize =
    (('m' as usize) << 24) + (('e' as usize) << 16) + (('o' as usize) << 8) + ('w' as usize);
const LONGTAIL_BLAKE2_HASH_TYPE: usize =
    (('b' as usize) << 24) + (('l' as usize) << 16) + (('k' as usize) << 8) + ('2' as usize);
const LONGTAIL_BLAKE3_HASH_TYPE: usize =
    (('b' as usize) << 24) + (('l' as usize) << 16) + (('k' as usize) << 8) + ('3' as usize);

#[repr(C)]
#[derive(Debug)]
pub struct HashAPI {
    pub hash_api: *mut Longtail_HashAPI,
    _pin: std::marker::PhantomPinned,
}

impl Drop for HashAPI {
    fn drop(&mut self) {
        // unsafe { Longtail_DisposeAPI(&mut (*self.hash_api).m_API as *mut
        // Longtail_API) };
    }
}

impl Deref for HashAPI {
    type Target = *mut Longtail_HashAPI;
    fn deref(&self) -> &Self::Target {
        &self.hash_api
    }
}

impl DerefMut for HashAPI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.hash_api
    }
}

impl HashAPI {
    pub fn new(hash_api: *mut Longtail_HashAPI) -> HashAPI {
        HashAPI {
            hash_api,
            _pin: std::marker::PhantomPinned,
        }
    }
}

#[derive(EnumString, FromRepr, Debug, PartialEq, Copy, Clone)]
#[repr(usize)]
pub enum HashType {
    #[strum(serialize = "meow")]
    Meow = LONGTAIL_MEOW_HASH_TYPE,
    #[strum(serialize = "blake2")]
    Blake2 = LONGTAIL_BLAKE2_HASH_TYPE,
    #[strum(serialize = "blake3")]
    Blake3 = LONGTAIL_BLAKE3_HASH_TYPE,
}

#[repr(C)]
pub struct HashRegistry {
    pub hash_registry: *mut Longtail_HashRegistryAPI,
    _pin: std::marker::PhantomPinned,
}

impl Drop for HashRegistry {
    fn drop(&mut self) {
        println!("Disposing hash registry");
        unsafe { Longtail_DisposeAPI(&mut (*self.hash_registry).m_API as *mut Longtail_API) };
    }
}

impl Default for HashRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HashRegistry {
    pub fn new() -> HashRegistry {
        let hash_registry = unsafe { Longtail_CreateFullHashRegistry() };
        HashRegistry {
            hash_registry,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn get_hash_api(&self, hash_type: HashType) -> Result<HashAPI, i32> {
        let mut hash_api_c = std::ptr::null_mut::<Longtail_HashAPI>();
        let result = unsafe {
            Longtail_GetHashRegistry_GetHashAPI(
                self.hash_registry,
                hash_type as u32,
                &mut hash_api_c,
            )
        };
        if result != 0 {
            return Err(result);
        }
        let hash_api = HashAPI::new(hash_api_c);
        Ok(hash_api)
    }
}
