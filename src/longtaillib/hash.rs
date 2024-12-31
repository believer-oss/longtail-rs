#[rustfmt::skip]
// Hash API
// pub fn Longtail_GetHashAPISize() -> u64;
// pub fn Longtail_MakeHashAPI( mem: *mut ::std::os::raw::c_void, dispose_func: Longtail_DisposeFunc, get_identifier_func: Longtail_Hash_GetIdentifierFunc, begin_context_func: Longtail_Hash_BeginContextFunc, hash_func: Longtail_Hash_HashFunc, end_context_func: Longtail_Hash_EndContextFunc, hash_buffer_func: Longtail_Hash_HashBufferFunc,) -> *mut Longtail_HashAPI;
// pub fn Longtail_Hash_GetIdentifier(hash_api: *mut Longtail_HashAPI) -> u32;
// pub fn Longtail_Hash_BeginContext( hash_api: *mut Longtail_HashAPI, out_context: *mut Longtail_HashAPI_HContext,) -> ::std::os::raw::c_int;
// pub fn Longtail_Hash_Hash( hash_api: *mut Longtail_HashAPI, context: Longtail_HashAPI_HContext, length: u32, data: *const ::std::os::raw::c_void,);
// pub fn Longtail_Hash_EndContext( hash_api: *mut Longtail_HashAPI, context: Longtail_HashAPI_HContext,) -> u64;
// pub fn Longtail_Hash_HashBuffer( hash_api: *mut Longtail_HashAPI, length: u32, data: *const ::std::os::raw::c_void, out_hash: *mut u64,) -> ::std::os::raw::c_int;
// // Registry
// pub fn Longtail_GetHashRegistrySize() -> u64;
// pub fn Longtail_GetHashRegistry_GetHashAPI( hash_registry: *mut Longtail_HashRegistryAPI, hash_type: u32, out_compression_api: *mut *mut Longtail_HashAPI,) -> ::std::os::raw::c_int;
// pub fn Longtail_CreateFullHashRegistry() -> *mut Longtail_HashRegistryAPI;
// pub fn Longtail_CreateDefaultHashRegistry( hash_type_count: u32, hash_types: *const u32, hash_apis: *mut *const Longtail_HashAPI,) -> *mut Longtail_HashRegistryAPI;
// pub fn Longtail_MakeHashRegistryAPI( mem: *mut ::std::os::raw::c_void, dispose_func: Longtail_DisposeFunc, get_hash_api_func: Longtail_HashRegistry_GetHashAPIFunc,) -> *mut Longtail_HashRegistryAPI;
// // Blake2
// pub fn Longtail_CreateBlake2HashAPI() -> *mut Longtail_HashAPI;
// pub fn Longtail_GetBlake2HashType() -> u32;
// // Blake3
// pub fn Longtail_CreateBlake3HashAPI() -> *mut Longtail_HashAPI;
// pub fn Longtail_GetBlake3HashType() -> u32;
// pub fn Longtail_CreateBlake3HashRegistry() -> *mut Longtail_HashRegistryAPI;
// // MeowHash
// pub fn Longtail_CreateMeowHashAPI() -> *mut Longtail_HashAPI;
// pub fn Longtail_GetMeowHashType() -> u32;
//
// struct Longtail_HashAPI
// {
//     struct Longtail_API m_API;
//     Longtail_Hash_GetIdentifierFunc GetIdentifier;
//     Longtail_Hash_BeginContextFunc BeginContext;
//     Longtail_Hash_HashFunc Hash;
//     Longtail_Hash_EndContextFunc EndContext;
//     Longtail_Hash_HashBufferFunc HashBuffer;
// };
//
// struct Longtail_HashRegistryAPI
// {
//     struct Longtail_API m_API;
//     Longtail_HashRegistry_GetHashAPIFunc GetHashAPI;
// };

use std::ops::{Deref, DerefMut};

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

/// The Hash API provides functions for hashing data.
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

// TODO: Remove strum dependency
/// The HashType enum represents the different types of hash that can be used.
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
