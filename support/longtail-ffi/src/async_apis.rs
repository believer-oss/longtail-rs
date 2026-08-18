use std::ops::{Deref, DerefMut};

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, instrument, warn};

use crate::{
    Longtail_API, Longtail_AsyncFlushAPI, Longtail_AsyncGetExistingContentAPI,
    Longtail_AsyncGetStoredBlockAPI, Longtail_AsyncPreflightStartedAPI,
    Longtail_AsyncPruneBlocksAPI, Longtail_AsyncPutStoredBlockAPI, Longtail_StoreIndex,
    Longtail_StoredBlock,
};

// AsyncGetExistingContentAPI
// -------------------------------------------------------------------------------------------
// TODO: This needs to be a macro
pub trait AsyncGetExistingContentAPI: std::fmt::Debug + Send {
    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    unsafe fn on_complete(&mut self, store_index: *mut Longtail_StoreIndex, err: i32);
}

// Global registry to map unique IDs to completion handlers
static COMPLETION_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

lazy_static::lazy_static! {
    static ref COMPLETION_REGISTRY: StdMutex<HashMap<u64, Box<dyn AsyncGetExistingContentAPI>>> =
        StdMutex::new(HashMap::new());

    // Map context pointers (as usize) to IDs
    static ref CONTEXT_TO_ID_MAP: StdMutex<HashMap<usize, u64>> =
        StdMutex::new(HashMap::new());
}

#[allow(non_camel_case_types)]
#[derive(Debug)]
#[repr(C)]
pub struct AsyncGetExistingContentAPIProxy {
    pub api: *mut Longtail_AsyncGetExistingContentAPI,
}

// Make AsyncGetExistingContentAPIProxy Send
unsafe impl Send for AsyncGetExistingContentAPIProxy {}

#[repr(C)]
#[derive(Debug)]
pub struct AsyncGetExistingContentAPIInternal {
    pub api: Longtail_AsyncGetExistingContentAPI,
    _pin: std::marker::PhantomPinned,
}

// TODO: Unused, since we're relying on the dispose function to handle it?
impl Drop for AsyncGetExistingContentAPIInternal {
    fn drop(&mut self) {
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut
        // Longtail_API) };
    }
}

impl Deref for AsyncGetExistingContentAPIInternal {
    type Target = Longtail_AsyncGetExistingContentAPI;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl DerefMut for AsyncGetExistingContentAPIInternal {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.api
    }
}

impl AsyncGetExistingContentAPIProxy {
    pub fn new(
        async_get_existing_content_api: Box<dyn AsyncGetExistingContentAPI>,
    ) -> AsyncGetExistingContentAPIProxy {
        let internal = AsyncGetExistingContentAPIInternal {
            api: Longtail_AsyncGetExistingContentAPI {
                m_API: Longtail_API {
                    Dispose: Some(async_get_existing_content_api_dispose),
                },
                OnComplete: Some(async_get_existing_content_api_on_complete),
            },
            _pin: std::marker::PhantomPinned,
        };

        // Register the completion handler in the global registry using unique ID
        let completion_id = COMPLETION_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        // Use the function pointer as key since that's consistent
        let function_key = internal.api.OnComplete.unwrap() as usize;
        debug!(
            "AsyncGetExistingContentAPIProxy::new: registering completion id={} for function: {:#x}",
            completion_id, function_key
        );

        // Register the completion handler in the global registry
        if let Ok(mut registry) = COMPLETION_REGISTRY.lock() {
            registry.insert(completion_id, async_get_existing_content_api);
        }
        if let Ok(mut context_map) = CONTEXT_TO_ID_MAP.lock() {
            context_map.insert(function_key, completion_id);
        }

        // Create internal struct on heap and return proxy with pointer to it
        let internal_ptr = Box::into_raw(Box::new(internal));
        AsyncGetExistingContentAPIProxy {
            api: unsafe { &mut (*internal_ptr).api as *mut Longtail_AsyncGetExistingContentAPI },
        }
    }

    /// Extracts the function key from an API pointer if it matches our callback
    unsafe fn get_function_key_if_ours(
        async_api: *mut Longtail_AsyncGetExistingContentAPI,
    ) -> Option<usize> {
        unsafe { async_api.as_ref() }?
            .OnComplete
            .and_then(|func_ptr| {
                let is_our_func = func_ptr as *const ()
                    == async_get_existing_content_api_on_complete as *const ();
                if is_our_func {
                    Some(func_ptr as usize)
                } else {
                    None
                }
            })
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    pub unsafe fn new_from_api(
        async_api: *mut Longtail_AsyncGetExistingContentAPI,
    ) -> AsyncGetExistingContentAPIProxy {
        // Check if this API pointer is already in our registry
        if let Some(function_key) = unsafe { Self::get_function_key_if_ours(async_api) } {
            // This is one of our Rust proxies, check if it's in the registry
            if let Ok(context_map) = CONTEXT_TO_ID_MAP.lock()
                && context_map.contains_key(&function_key)
            {
                warn!(
                    "AsyncGetExistingContentAPIProxy::new_from_api: API pointer {:p} is already in registry - potential double-wrap detected!",
                    async_api
                );
            }
        }

        debug!("AsyncGetExistingContentAPIProxy::new_from_api: wrapping external C API");
        AsyncGetExistingContentAPIProxy { api: async_api }
    }
}

impl AsyncGetExistingContentAPI for AsyncGetExistingContentAPIProxy {
    #[instrument]
    unsafe fn on_complete(&mut self, store_index: *mut Longtail_StoreIndex, err: i32) {
        if !store_index.is_null() {
            let oncomplete = unsafe {
                (*self.api).OnComplete.expect(
                    "AsyncGetExistingContentAPIProxy::on_complete: oncomplete function missing",
                )
            };
            debug!(
                "AsyncGetExistingContentAPIProxy::on_complete: oncomplete: {:?}",
                oncomplete
            );
            unsafe { oncomplete(self.api, store_index, err) };
        }
    }
}

/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
pub unsafe extern "C" fn async_get_existing_content_api_on_complete(
    context: *mut Longtail_AsyncGetExistingContentAPI,
    store_index: *mut Longtail_StoreIndex,
    err: i32,
) {
    // Check if this is one of our Rust proxies using the helper function
    if let Some(function_key) = unsafe {
        if !context.is_null() {
            AsyncGetExistingContentAPIProxy::get_function_key_if_ours(context)
        } else {
            None
        }
    } {
        debug!(
            "Looking up completion handler for function: {:#x}",
            function_key
        );

        // First find the completion ID for this function pointer
        let completion_id = if let Ok(context_map) = CONTEXT_TO_ID_MAP.lock() {
            context_map.get(&function_key).copied()
        } else {
            None
        };

        if let Some(id) = completion_id {
            debug!(
                "Found completion ID {} for function {:#x}",
                id, function_key
            );
            if let Ok(mut registry) = COMPLETION_REGISTRY.lock() {
                if let Some(mut handler) = registry.remove(&id) {
                    debug!("Found completion handler in registry, calling on_complete");
                    unsafe { handler.on_complete(store_index, err) };
                    // Put it back in the registry
                    registry.insert(id, handler);
                } else {
                    warn!("No completion handler found in registry for ID: {}", id);
                }
            } else {
                warn!("Failed to lock completion registry");
            }
        } else {
            warn!("No completion ID found for function: {:#x}", function_key);
        }
    } else {
        // This is a C API callback from Longtail, call it directly
        unsafe {
            match context.as_ref().expect("couldn't get ref").OnComplete {
                Some(func) => func(context, store_index, err),
                None => warn!("AsyncGetExistingContentAPIProxy::on_complete function missing"),
            }
        }
    }
}

/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
pub unsafe extern "C" fn async_get_existing_content_api_dispose(api: *mut Longtail_API) {
    let proxy = unsafe { &mut (*(api as *mut AsyncGetExistingContentAPIProxy)) };

    // Check if this is one of our Rust proxies that needs registry cleanup
    if let Some(function_key) =
        unsafe { AsyncGetExistingContentAPIProxy::get_function_key_if_ours(proxy.api) }
    {
        // This is our Rust proxy, clean up registry entries
        debug!(
            "Dispose: cleaning up registry entries for function: {:#x}",
            function_key
        );

        // Get the completion ID and clean up both registries
        if let Ok(mut context_map) = CONTEXT_TO_ID_MAP.lock()
            && let Some(completion_id) = context_map.remove(&function_key)
        {
            debug!("Dispose: removing completion ID {}", completion_id);
            if let Ok(mut registry) = COMPLETION_REGISTRY.lock() {
                registry.remove(&completion_id);
            }
        }

        // Free the boxed internal struct
        let internal_container_ptr = proxy.api as *mut AsyncGetExistingContentAPIInternal;
        let internal_container_ptr = unsafe {
            (internal_container_ptr as *const u8)
                .offset(-(std::mem::offset_of!(AsyncGetExistingContentAPIInternal, api) as isize))
                as *mut AsyncGetExistingContentAPIInternal
        };
        unsafe {
            let _ = Box::from_raw(internal_container_ptr);
        };
    } else {
        // This is an external C API, call its dispose function
        unsafe { (*proxy.api).m_API.Dispose.expect("couldn't find dispose")(api) };
    }
}

// AsyncPutStoredBlockAPI
// -------------------------------------------------------------------------------------------
pub trait AsyncPutStoredBlockAPI: std::fmt::Debug {
    fn on_complete(&self, err: i32);
}

#[derive(Debug)]
#[repr(C)]
pub struct AsyncPutStoredBlockAPIProxy {
    pub api: Longtail_AsyncPutStoredBlockAPI,
    pub context: *mut std::os::raw::c_void,
    _pin: std::marker::PhantomPinned,
}

impl Drop for AsyncPutStoredBlockAPIProxy {
    fn drop(&mut self) {
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut
        // Longtail_API) };
    }
}

impl Deref for AsyncPutStoredBlockAPIProxy {
    type Target = Longtail_AsyncPutStoredBlockAPI;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl DerefMut for AsyncPutStoredBlockAPIProxy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.api
    }
}

impl AsyncPutStoredBlockAPIProxy {
    pub fn new(
        async_put_stored_block_api: Box<dyn AsyncPutStoredBlockAPI>,
    ) -> AsyncPutStoredBlockAPIProxy {
        AsyncPutStoredBlockAPIProxy {
            api: Longtail_AsyncPutStoredBlockAPI {
                m_API: Longtail_API {
                    Dispose: Some(async_put_stored_block_api_dispose),
                },
                OnComplete: Some(async_put_stored_block_api_on_complete),
            },
            context: Box::into_raw(Box::new(async_put_stored_block_api))
                as *mut std::os::raw::c_void,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_from_api(async_api: Longtail_AsyncPutStoredBlockAPI) -> AsyncPutStoredBlockAPIProxy {
        AsyncPutStoredBlockAPIProxy {
            api: async_api,
            context: std::ptr::null_mut(),
            _pin: std::marker::PhantomPinned,
        }
    }
}

impl AsyncPutStoredBlockAPI for AsyncPutStoredBlockAPIProxy {
    fn on_complete(&self, err: i32) {
        let proxy = self as *const AsyncPutStoredBlockAPIProxy as *mut AsyncPutStoredBlockAPIProxy;
        let context = unsafe { (*proxy).context };
        let async_put_stored_block_api =
            unsafe { Box::from_raw(context as *mut Box<dyn AsyncPutStoredBlockAPI>) };
        async_put_stored_block_api.on_complete(err);
        let _ = Box::into_raw(async_put_stored_block_api);
    }
}

pub extern "C" fn async_put_stored_block_api_on_complete(
    context: *mut Longtail_AsyncPutStoredBlockAPI,
    err: i32,
) {
    let proxy = context as *mut AsyncPutStoredBlockAPIProxy;
    let context = unsafe { (*proxy).context };
    let async_put_stored_block_api =
        unsafe { Box::from_raw(context as *mut Box<dyn AsyncPutStoredBlockAPI>) };
    async_put_stored_block_api.on_complete(err);
    let _ = Box::into_raw(async_put_stored_block_api);
}

pub extern "C" fn async_put_stored_block_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut AsyncPutStoredBlockAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context as *mut Box<dyn AsyncPutStoredBlockAPI>) };
}

// AsyncPreflightStartedAPI
// -------------------------------------------------------------------------------------------
pub trait AsyncPreflightStartedAPI: std::fmt::Debug {
    fn on_complete(&self, block_hashes: Vec<u64>, err: i32);
}

#[derive(Debug)]
pub struct AsyncPreflightStartedAPIProxy {
    pub api: Longtail_AsyncPreflightStartedAPI,
    pub context: *mut std::os::raw::c_void,
    _pin: std::marker::PhantomPinned,
}

impl Drop for AsyncPreflightStartedAPIProxy {
    fn drop(&mut self) {
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut
        // Longtail_API) };
    }
}

impl Deref for AsyncPreflightStartedAPIProxy {
    type Target = Longtail_AsyncPreflightStartedAPI;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl DerefMut for AsyncPreflightStartedAPIProxy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.api
    }
}

impl AsyncPreflightStartedAPIProxy {
    pub fn new(
        async_preflight_started_api: Box<dyn AsyncPreflightStartedAPI>,
    ) -> AsyncPreflightStartedAPIProxy {
        AsyncPreflightStartedAPIProxy {
            api: Longtail_AsyncPreflightStartedAPI {
                m_API: Longtail_API {
                    Dispose: Some(async_preflight_started_api_dispose),
                },
                OnComplete: Some(async_preflight_started_api_on_complete),
            },
            context: Box::into_raw(Box::new(async_preflight_started_api))
                as *mut std::os::raw::c_void,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_from_api(
        async_api: Longtail_AsyncPreflightStartedAPI,
    ) -> AsyncPreflightStartedAPIProxy {
        AsyncPreflightStartedAPIProxy {
            api: async_api,
            context: std::ptr::null_mut(),
            _pin: std::marker::PhantomPinned,
        }
    }
}

impl AsyncPreflightStartedAPI for AsyncPreflightStartedAPIProxy {
    fn on_complete(&self, block_hashes: Vec<u64>, err: i32) {
        let proxy =
            self as *const AsyncPreflightStartedAPIProxy as *mut AsyncPreflightStartedAPIProxy;
        let context = unsafe { (*proxy).context };
        let async_preflight_started_api =
            unsafe { Box::from_raw(context as *mut Box<dyn AsyncPreflightStartedAPI>) };
        async_preflight_started_api.on_complete(block_hashes, err);
        let _ = Box::into_raw(async_preflight_started_api);
    }
}

/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
pub unsafe extern "C" fn async_preflight_started_api_on_complete(
    context: *mut Longtail_AsyncPreflightStartedAPI,
    block_count: u32,
    block_hashes: *mut u64,
    err: i32,
) {
    let proxy = context as *mut AsyncPreflightStartedAPIProxy;
    let context = unsafe { (*proxy).context };
    let async_preflight_started_api =
        unsafe { Box::from_raw(context as *mut Box<dyn AsyncPreflightStartedAPI>) };
    let block_hashes = unsafe { std::slice::from_raw_parts(block_hashes, block_count as usize) };
    async_preflight_started_api.on_complete(block_hashes.to_vec(), err);
    let _ = Box::into_raw(async_preflight_started_api);
}

pub extern "C" fn async_preflight_started_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut AsyncPreflightStartedAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context as *mut Box<dyn AsyncPreflightStartedAPI>) };
}

// AsyncGetStoredBlockAPI
// -------------------------------------------------------------------------------------------
pub trait AsyncGetStoredBlockAPI: std::fmt::Debug {
    fn on_complete(&self, stored_block: *mut Longtail_StoredBlock, err: i32);
}

#[repr(C)]
#[derive(Debug)]
pub struct AsyncGetStoredBlockAPIProxy {
    pub api: Longtail_AsyncGetStoredBlockAPI,
    pub context: *mut Box<dyn AsyncGetStoredBlockAPI>,
    mark: [u8; 4],
    _pin: std::marker::PhantomPinned,
}

// TODO: Unused, since we're relying on the dispose function to handle it?
impl Drop for AsyncGetStoredBlockAPIProxy {
    fn drop(&mut self) {
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut
        // Longtail_API) };
    }
}

impl Deref for AsyncGetStoredBlockAPIProxy {
    type Target = Longtail_AsyncGetStoredBlockAPI;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl DerefMut for AsyncGetStoredBlockAPIProxy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.api
    }
}

impl AsyncGetStoredBlockAPIProxy {
    pub fn new(
        async_get_stored_block_api: Box<dyn AsyncGetStoredBlockAPI>,
    ) -> AsyncGetStoredBlockAPIProxy {
        let inner = Box::into_raw(Box::new(async_get_stored_block_api));
        debug!("AsyncGetStoredBlockAPIProxy::new: conext: {:?}", inner);
        AsyncGetStoredBlockAPIProxy {
            api: Longtail_AsyncGetStoredBlockAPI {
                m_API: Longtail_API {
                    Dispose: Some(async_get_stored_block_api_dispose),
                },
                OnComplete: Some(async_get_stored_block_api_on_complete),
            },
            context: inner,
            mark: *b"ltrs",
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences a raw pointer.
    // FIXME: This can't move the pointer...
    pub unsafe fn new_from_api(
        async_api: *mut Longtail_AsyncGetStoredBlockAPI,
    ) -> *mut AsyncGetStoredBlockAPIProxy {
        let proxy_ptr = async_api as *mut AsyncGetStoredBlockAPIProxy;
        let mark_ptr = unsafe { (*proxy_ptr).mark };
        if mark_ptr == *b"ltrs" {
            debug!("AsyncGetStoredBlockAPIProxy::new_from_api: returning proxy");
            let api = unsafe { (*proxy_ptr).api };
            let context = unsafe { (*proxy_ptr).context };
            Box::into_raw(Box::new(AsyncGetStoredBlockAPIProxy {
                api,
                context,
                mark: *b"ltrs",
                _pin: std::marker::PhantomPinned,
            }))
        } else {
            async_api as *mut AsyncGetStoredBlockAPIProxy
        }
    }

    pub fn get_context(&self) -> Option<Box<Box<dyn AsyncGetStoredBlockAPI>>> {
        if self.context.is_null() || self.mark != *b"ltrs" {
            None
        } else {
            Some(unsafe { Box::from_raw(self.context) })
        }
    }
}

impl AsyncGetStoredBlockAPI for AsyncGetStoredBlockAPIProxy {
    // FIXME: horrible...
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn on_complete(&self, stored_block: *mut Longtail_StoredBlock, err: i32) {
        let proxy = self as *const AsyncGetStoredBlockAPIProxy as *mut AsyncGetStoredBlockAPIProxy;
        debug!(
            "AsyncGetStoredBlockAPIProxy::on_complete: proxy: {:p}",
            proxy
        );
        let context = self.get_context();
        if let Some(context) = context {
            context.on_complete(stored_block, err);
            let _ = Box::into_raw(context);
        } else if let Some(oncomplete) = self.api.OnComplete {
            unsafe { oncomplete(&mut (*proxy).api, stored_block, err) };
        } else {
            warn!("AsyncGetStoredBlockAPIProxy::on_complete: oncomplete function missing");
        };
        // let async_get_stored_block_api = unsafe { Box::from_raw(context) };
        // async_get_stored_block_api.on_complete(stored_block, err);
        // Box::into_raw(async_get_stored_block_api);
    }
}

/// # Safety
/// This function is unsafe because it dereferences a raw pointer.
pub unsafe extern "C" fn async_get_stored_block_api_on_complete(
    context: *mut Longtail_AsyncGetStoredBlockAPI,
    stored_block: *mut Longtail_StoredBlock,
    err: i32,
) {
    let proxy = context as *mut AsyncGetStoredBlockAPIProxy;
    let inner = unsafe { proxy.as_ref().expect("couldn't get ref").get_context() };
    if let Some(inner) = inner {
        inner.on_complete(stored_block, err);
        let _ = Box::into_raw(inner);
    } else {
        unsafe {
            match (*proxy).api.OnComplete {
                Some(func) => func(context, stored_block, err),
                None => warn!("AsyncGetStoredBlockAPIProxy::on_complete function missing"),
            }
        }
    }
    // let context = unsafe { (*proxy).context };
    // let async_get_stored_block_api = unsafe { Box::from_raw(context) };
    // async_get_stored_block_api.on_complete(stored_block, err);
    // Box::into_raw(async_get_stored_block_api);
}

pub extern "C" fn async_get_stored_block_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut AsyncGetStoredBlockAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context) };
}

// AsyncPruneBlocksAPI
// -------------------------------------------------------------------------------------------
pub trait AsyncPruneBlocksAPI: std::fmt::Debug {
    fn on_complete(&mut self, pruned_block_count: u32, err: i32);
}

#[derive(Debug)]
pub struct AsyncPruneBlocksAPIProxy {
    pub api: Longtail_AsyncPruneBlocksAPI,
    pub context: *mut std::os::raw::c_void,
    _pin: std::marker::PhantomPinned,
}

impl Drop for AsyncPruneBlocksAPIProxy {
    fn drop(&mut self) {
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut
        // Longtail_API) };
    }
}

impl Deref for AsyncPruneBlocksAPIProxy {
    type Target = Longtail_AsyncPruneBlocksAPI;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl DerefMut for AsyncPruneBlocksAPIProxy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.api
    }
}

impl AsyncPruneBlocksAPIProxy {
    pub fn new(async_prune_blocks_api: Box<dyn AsyncPruneBlocksAPI>) -> AsyncPruneBlocksAPIProxy {
        AsyncPruneBlocksAPIProxy {
            api: Longtail_AsyncPruneBlocksAPI {
                m_API: Longtail_API {
                    Dispose: Some(async_prune_blocks_api_dispose),
                },
                OnComplete: Some(async_prune_blocks_api_on_complete),
            },
            context: Box::into_raw(Box::new(async_prune_blocks_api)) as *mut std::os::raw::c_void,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_from_api(async_api: Longtail_AsyncPruneBlocksAPI) -> AsyncPruneBlocksAPIProxy {
        AsyncPruneBlocksAPIProxy {
            api: async_api,
            context: std::ptr::null_mut(),
            _pin: std::marker::PhantomPinned,
        }
    }
}

impl AsyncPruneBlocksAPI for AsyncPruneBlocksAPIProxy {
    fn on_complete(&mut self, pruned_block_count: u32, err: i32) {
        let proxy = self as *mut AsyncPruneBlocksAPIProxy;
        let context = unsafe { (*proxy).context };
        let mut async_prune_blocks_api =
            unsafe { Box::from_raw(context as *mut Box<dyn AsyncPruneBlocksAPI>) };
        async_prune_blocks_api.on_complete(pruned_block_count, err);
        let _ = Box::into_raw(async_prune_blocks_api);
    }
}

pub extern "C" fn async_prune_blocks_api_on_complete(
    context: *mut Longtail_AsyncPruneBlocksAPI,
    pruned_block_count: u32,
    err: i32,
) {
    let proxy = context as *mut AsyncPruneBlocksAPIProxy;
    let context = unsafe { (*proxy).context };
    let mut async_prune_blocks_api =
        unsafe { Box::from_raw(context as *mut Box<dyn AsyncPruneBlocksAPI>) };
    async_prune_blocks_api.on_complete(pruned_block_count, err);
    let _ = Box::into_raw(async_prune_blocks_api);
}

pub extern "C" fn async_prune_blocks_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut AsyncPruneBlocksAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context as *mut Box<dyn AsyncPruneBlocksAPI>) };
}

// AsyncFlushAPI
// -------------------------------------------------------------------------------------------
pub trait AsyncFlushAPI: std::fmt::Debug {
    fn on_complete(&mut self, err: i32);
}

pub struct AsyncFlushAPIProxy {
    pub api: Longtail_AsyncFlushAPI,
    pub context: *mut std::os::raw::c_void,
    _pin: std::marker::PhantomPinned,
}

impl Drop for AsyncFlushAPIProxy {
    fn drop(&mut self) {
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut
        // Longtail_API) };
    }
}

impl Deref for AsyncFlushAPIProxy {
    type Target = Longtail_AsyncFlushAPI;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl DerefMut for AsyncFlushAPIProxy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.api
    }
}

impl AsyncFlushAPIProxy {
    pub fn new(async_flush_api: Box<dyn AsyncFlushAPI>) -> AsyncFlushAPIProxy {
        AsyncFlushAPIProxy {
            api: Longtail_AsyncFlushAPI {
                m_API: Longtail_API {
                    Dispose: Some(async_flush_api_dispose),
                },
                OnComplete: Some(async_flush_api_on_complete),
            },
            context: Box::into_raw(Box::new(async_flush_api)) as *mut std::os::raw::c_void,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_from_api(async_api: Longtail_AsyncFlushAPI) -> AsyncFlushAPIProxy {
        AsyncFlushAPIProxy {
            api: async_api,
            context: std::ptr::null_mut(),
            _pin: std::marker::PhantomPinned,
        }
    }
}

pub extern "C" fn async_flush_api_on_complete(context: *mut Longtail_AsyncFlushAPI, err: i32) {
    let proxy = context as *mut AsyncFlushAPIProxy;
    let context = unsafe { (*proxy).context };
    let mut async_flush_api = unsafe { Box::from_raw(context as *mut Box<dyn AsyncFlushAPI>) };
    async_flush_api.on_complete(err);
    let _ = Box::into_raw(async_flush_api);
}

pub extern "C" fn async_flush_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut AsyncFlushAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context as *mut Box<dyn AsyncFlushAPI>) };
}
