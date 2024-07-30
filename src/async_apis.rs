use std::ops::{
    Deref,
    DerefMut,
};

use tracing::{
    debug,
    warn,
};

use crate::{
    Longtail_API,
    Longtail_AsyncFlushAPI,
    Longtail_AsyncGetExistingContentAPI,
    Longtail_AsyncGetStoredBlockAPI,
    Longtail_AsyncPreflightStartedAPI,
    Longtail_AsyncPruneBlocksAPI,
    Longtail_AsyncPutStoredBlockAPI,
    Longtail_StoreIndex,
    Longtail_StoredBlock,
};

// AsyncGetExistingContentAPI
// -------------------------------------------------------------------------------------------
// TODO: This needs to be a macro
pub trait AsyncGetExistingContentAPI: std::fmt::Debug {
    fn on_complete(&mut self, store_index: *mut Longtail_StoreIndex, err: i32);
}

// FIXME: We need to deal with the memory management of this clone
#[repr(C)]
#[derive(Debug, Clone)]
pub struct AsyncGetExistingContentAPIProxy {
    pub api: Longtail_AsyncGetExistingContentAPI,
    pub context: *mut Box<dyn AsyncGetExistingContentAPI>,
    mark: [u8; 4],
    _pin: std::marker::PhantomPinned,
}

// TODO: Unused, since we're relying on the dispose function to handle it?
impl Drop for AsyncGetExistingContentAPIProxy {
    fn drop(&mut self) {
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut
        // Longtail_API) };
    }
}

impl Deref for AsyncGetExistingContentAPIProxy {
    type Target = Longtail_AsyncGetExistingContentAPI;
    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl DerefMut for AsyncGetExistingContentAPIProxy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.api
    }
}

impl AsyncGetExistingContentAPIProxy {
    pub fn new(
        async_get_existing_content_api: Box<dyn AsyncGetExistingContentAPI>,
    ) -> AsyncGetExistingContentAPIProxy {
        let context = Box::into_raw(Box::new(async_get_existing_content_api));
        debug!(
            "AsyncGetExistingContentAPIProxy::new: context: {:?}",
            context
        );
        AsyncGetExistingContentAPIProxy {
            api: Longtail_AsyncGetExistingContentAPI {
                m_API: Longtail_API {
                    Dispose: Some(async_get_existing_content_api_dispose),
                },
                OnComplete: Some(async_get_existing_content_api_on_complete),
            },
            context,
            mark: [0xf0, 0x0f, 0xf0, 0x0f],
            _pin: std::marker::PhantomPinned,
        }
    }

    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn new_from_api(
        async_api: *mut Longtail_AsyncGetExistingContentAPI,
    ) -> AsyncGetExistingContentAPIProxy {
        let proxy_ptr = async_api as *mut AsyncGetExistingContentAPIProxy;
        let mark_ptr = unsafe { (*proxy_ptr).mark };
        if mark_ptr == [0xf0, 0x0f, 0xf0, 0x0f] {
            debug!("AsyncGetExistingContentAPIProxy::new_from_api: returning proxy");
            let api = unsafe { (*proxy_ptr).api };
            let context = unsafe { (*proxy_ptr).context };
            let mark = unsafe { (*proxy_ptr).mark };
            AsyncGetExistingContentAPIProxy {
                api,
                context,
                mark,
                _pin: std::marker::PhantomPinned,
            }
        } else {
            let api = unsafe { *async_api };
            let context = std::ptr::null_mut();
            AsyncGetExistingContentAPIProxy {
                api,
                context,
                mark: [0; 4],
                _pin: std::marker::PhantomPinned,
            }
        }
    }

    pub fn get_context(&self) -> Option<Box<Box<dyn AsyncGetExistingContentAPI>>> {
        if self.context.is_null() || self.mark != [0xf0, 0x0f, 0xf0, 0x0f] {
            tracing::warn!("AsyncGetExistingContentAPIProxy::get_context: context is null");
            None
        } else {
            tracing::debug!(
                "AsyncGetExistingContentAPIProxy::get_context: context: {:?}",
                self.context
            );
            Some(unsafe { Box::from_raw(self.context) })
        }
    }
}

impl AsyncGetExistingContentAPI for AsyncGetExistingContentAPIProxy {
    // FIXME: horrible...
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    #[tracing::instrument]
    fn on_complete(&mut self, store_index: *mut Longtail_StoreIndex, err: i32) {
        let context = self.get_context();
        if let Some(mut context) = context {
            context.on_complete(store_index, err);
            Box::into_raw(context);
        } else {
            let oncomplete = self.api.OnComplete.unwrap();
            tracing::debug!(
                "AsyncGetExistingContentAPIProxy::on_complete: oncomplete: {:?}",
                oncomplete
            );
            unsafe { oncomplete(&mut self.api, store_index, err) };
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
    let proxy = context as *mut AsyncGetExistingContentAPIProxy;
    // let inner = unsafe { (*proxy).context };
    let inner = proxy.as_ref().expect("couldn't get ref").get_context();
    if let Some(mut inner) = inner {
        inner.on_complete(store_index, err);
        Box::into_raw(inner);
    } else {
        unsafe { (*proxy).api.OnComplete.unwrap()(context, store_index, err) }
    }
}

pub extern "C" fn async_get_existing_content_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut AsyncGetExistingContentAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context) };
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
        Box::into_raw(async_put_stored_block_api);
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
    Box::into_raw(async_put_stored_block_api);
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
        Box::into_raw(async_preflight_started_api);
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
    Box::into_raw(async_preflight_started_api);
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
            // warn!("AsyncGetStoredBlockAPIProxy::new_from_api: context is null");
            // let api = unsafe { *async_api };
            // let context = std::ptr::null_mut();
            // AsyncGetStoredBlockAPIProxy {
            //     api,
            //     context,
            //     mark: [0; 4],
            //     _pin: std::marker::PhantomPinned,
            // }
            async_api as *mut AsyncGetStoredBlockAPIProxy
        }
    }

    pub fn get_context(&self) -> Option<Box<Box<dyn AsyncGetStoredBlockAPI>>> {
        if self.context.is_null() || self.mark != *b"ltrs" {
            warn!("AsyncGetStoredBlockAPIProxy::get_context: context is null");
            None
        } else {
            debug!(
                "AsyncGetStoredBlockAPIProxy::get_context: context: {:?}",
                self.context
            );
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
            Box::into_raw(context);
        } else {
            let oncomplete = self.api.OnComplete.unwrap();
            unsafe { oncomplete(&mut (*proxy).api, stored_block, err) };
        };
        // let async_get_stored_block_api = unsafe { Box::from_raw(context) };
        // async_get_stored_block_api.on_complete(stored_block, err);
        // Box::into_raw(async_get_stored_block_api);
    }
}

// FIXME: horrible...
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn async_get_stored_block_api_on_complete(
    context: *mut Longtail_AsyncGetStoredBlockAPI,
    stored_block: *mut Longtail_StoredBlock,
    err: i32,
) {
    let proxy = context as *mut AsyncGetStoredBlockAPIProxy;
    let inner = unsafe { proxy.as_ref().expect("couldn't get ref").get_context() };
    if let Some(inner) = inner {
        inner.on_complete(stored_block, err);
        Box::into_raw(inner);
    } else {
        unsafe { (*proxy).api.OnComplete.unwrap()(context, stored_block, err) };
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
        Box::into_raw(async_prune_blocks_api);
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
    Box::into_raw(async_prune_blocks_api);
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
    Box::into_raw(async_flush_api);
}

pub extern "C" fn async_flush_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut AsyncFlushAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context as *mut Box<dyn AsyncFlushAPI>) };
}
