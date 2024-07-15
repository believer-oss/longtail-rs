use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, RwLock},
};

use tracing::debug;

use crate::{
    Longtail_API, Longtail_AsyncFlushAPI, Longtail_AsyncGetExistingContentAPI,
    Longtail_AsyncGetStoredBlockAPI, Longtail_AsyncPreflightStartedAPI,
    Longtail_AsyncPruneBlocksAPI, Longtail_AsyncPutStoredBlockAPI, Longtail_StoreIndex,
    Longtail_StoredBlock, StoreIndex,
};

// AsyncGetExistingContentAPI
// TODO: This needs to be a macro
pub trait AsyncGetExistingContentAPI: std::fmt::Debug {
    fn on_complete(&mut self, store_index: *mut Longtail_StoreIndex, err: i32);
    fn get_store_index(&self) -> Result<Option<StoreIndex>, i32>;
}
#[repr(C)]
// FIXME: We need to deal with the memory management of this clone
#[derive(Debug, Clone)]
pub struct AsyncGetExistingContentAPIProxy {
    pub api: Longtail_AsyncGetExistingContentAPI,
    pub context: *mut std::os::raw::c_void,
    mark: [u8; 4],
    _pin: std::marker::PhantomPinned,
}

// TODO: Unused, since we're relying on the dispose function to handle it?
impl Drop for AsyncGetExistingContentAPIProxy {
    fn drop(&mut self) {
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut Longtail_API) };
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
            context: context as *mut std::os::raw::c_void,
            mark: [0xf0, 0x0f, 0xf0, 0x0f],
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_from_api(
        async_api: Longtail_AsyncGetExistingContentAPI,
    ) -> AsyncGetExistingContentAPIProxy {
        debug!(
            "AsyncGetExistingContentAPIProxy::new_from_api: async_api: {:?}",
            async_api
        );
        let proxy_ptr = std::ptr::addr_of!(async_api) as *mut AsyncGetExistingContentAPIProxy;
        let mut context = std::ptr::null_mut();
        let mut mark = [0; 4];
        unsafe {
            if (*proxy_ptr).mark == [0xf0, 0x0f, 0xf0, 0x0f] {
                context = (*proxy_ptr).context;
                mark.copy_from_slice(&(*proxy_ptr).mark);
            }
        }

        // FIXME: We need to validate that this really is a Proxy
        // let context = unsafe { (*proxy_ptr).context };
        AsyncGetExistingContentAPIProxy {
            api: async_api,
            context,
            mark,
            _pin: std::marker::PhantomPinned,
        }
    }

    /// # Safety
    /// This function is unsafe because it dereferences `context`.
    pub unsafe fn get_store_index(&self) -> Result<Option<StoreIndex>, i32> {
        let context = self.context as *mut Box<dyn AsyncGetExistingContentAPI>;

        (*context).get_store_index()
    }
}

impl AsyncGetExistingContentAPI for AsyncGetExistingContentAPIProxy {
    // FIXME: horrible...
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn on_complete(&mut self, store_index: *mut Longtail_StoreIndex, err: i32) {
        debug!("AsyncGetExistingContentAPIProxy::on_complete");
        debug!(
            "AsyncGetExistingContentAPIProxy::on_complete: store_index: {:?}",
            store_index
        );
        let proxy = self as *mut AsyncGetExistingContentAPIProxy;
        debug!(
            "AsyncGetExistingContentAPIProxy::on_complete: proxy: {:?}",
            proxy
        );
        let context = unsafe { (*proxy).context };
        debug!(
            "AsyncGetExistingContentAPIProxy::on_complete: context: {:?}",
            context
        );
        if context.is_null() {
            tracing::warn!("AsyncGetExistingContentAPIProxy::on_complete: context is null");
            unsafe { self.api.OnComplete.unwrap()(&mut self.api, store_index, err) }
        } else {
            let mut async_get_existing_content_api =
                unsafe { Box::from_raw(context as *mut Box<dyn AsyncGetExistingContentAPI>) };
            // FIXME: Blowing up here, presumably because the context is not being passed correctly
            debug!(
                "AsyncGetExistingContentAPIProxy::on_complete: async_get_existing_content_api: {:?}",
                async_get_existing_content_api
            );
            async_get_existing_content_api.on_complete(store_index, err);
            Box::into_raw(async_get_existing_content_api);
        }
    }
    fn get_store_index(&self) -> Result<Option<StoreIndex>, i32> {
        unsafe { self.get_store_index() }
    }
}

pub unsafe extern "C" fn async_get_existing_content_api_on_complete(
    context: *mut Longtail_AsyncGetExistingContentAPI,
    store_index: *mut Longtail_StoreIndex,
    err: i32,
) {
    let proxy = context as *mut AsyncGetExistingContentAPIProxy;
    debug!(
        "async_get_existing_content_api_on_complete: proxy: {:?}",
        proxy
    );
    let inner = unsafe { (*proxy).context };
    debug!(
        "async_get_existing_content_api_on_complete: context: {:?}",
        inner
    );
    if inner.is_null() {
        tracing::warn!("async_get_existing_content_api_on_complete: context is null");
        unsafe { (*proxy).api.OnComplete.unwrap()(context, store_index, err) }
    } else {
        let mut async_get_existing_content_api =
            unsafe { Box::from_raw(inner as *mut Box<dyn AsyncGetExistingContentAPI>) };
        debug!(
            "async_get_existing_content_api_on_complete: async_get_existing_content_api: {:?}",
            async_get_existing_content_api
        );
        async_get_existing_content_api.on_complete(store_index, err);
        Box::into_raw(async_get_existing_content_api);
    }
}

pub extern "C" fn async_get_existing_content_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut AsyncGetExistingContentAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context as *mut Box<dyn AsyncGetExistingContentAPI>) };
}

// TODO: Placeholder for a default implementation
#[derive(Debug, Default, Clone)]
pub struct GetExistingContentCompletion {
    pub store_index: Option<StoreIndex>,
    pub err: Arc<Mutex<Option<i32>>>,
}

impl AsyncGetExistingContentAPI for GetExistingContentCompletion {
    fn on_complete(&mut self, store_index: *mut Longtail_StoreIndex, err: i32) {
        self.store_index = Some(StoreIndex::new_from_lt(store_index));
        self.err = Arc::new(Mutex::new(Some(err)));
    }
    fn get_store_index(&self) -> Result<Option<StoreIndex>, i32> {
        let err = self.err.lock().unwrap();
        match *err {
            // TODO: This is a clone, should it be?
            Some(0) => Ok(Some(self.store_index.clone().unwrap())),
            Some(err) => Err(err),
            None => Ok(None),
        }
    }
}

// AsyncPutStoredBlockAPI
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
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut Longtail_API) };
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
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut Longtail_API) };
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
pub trait AsyncGetStoredBlockAPI: std::fmt::Debug {
    fn on_complete(&self, stored_block: *mut Longtail_StoredBlock, err: i32);
}

#[derive(Debug)]
pub struct AsyncGetStoredBlockAPIProxy {
    pub api: Longtail_AsyncGetStoredBlockAPI,
    pub context: *mut std::os::raw::c_void,
    _pin: std::marker::PhantomPinned,
}

impl Drop for AsyncGetStoredBlockAPIProxy {
    fn drop(&mut self) {
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut Longtail_API) };
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
        debug!(
            "AsyncGetStoredBlockAPIProxy::new: {:?}",
            std::ptr::addr_of!(async_get_stored_block_api)
        );
        let outer = Box::into_raw(Box::new(async_get_stored_block_api));
        debug!("AsyncGetStoredBlockAPIProxy::new: outer: {:?}", outer);
        AsyncGetStoredBlockAPIProxy {
            api: Longtail_AsyncGetStoredBlockAPI {
                m_API: Longtail_API {
                    Dispose: Some(async_get_stored_block_api_dispose),
                },
                OnComplete: Some(async_get_stored_block_api_on_complete),
            },
            context: outer as *mut std::os::raw::c_void,
            _pin: std::marker::PhantomPinned,
        }
    }

    pub fn new_from_api(async_api: Longtail_AsyncGetStoredBlockAPI) -> AsyncGetStoredBlockAPIProxy {
        AsyncGetStoredBlockAPIProxy {
            api: async_api,
            context: std::ptr::null_mut(),
            _pin: std::marker::PhantomPinned,
        }
    }
}

impl AsyncGetStoredBlockAPI for AsyncGetStoredBlockAPIProxy {
    fn on_complete(&self, stored_block: *mut Longtail_StoredBlock, err: i32) {
        let proxy = self as *const AsyncGetStoredBlockAPIProxy as *mut AsyncGetStoredBlockAPIProxy;
        let context = unsafe { (*proxy).context };
        let async_get_stored_block_api =
            unsafe { Box::from_raw(context as *mut Box<dyn AsyncGetStoredBlockAPI>) };
        async_get_stored_block_api.on_complete(stored_block, err);
        Box::into_raw(async_get_stored_block_api);
    }
}

pub extern "C" fn async_get_stored_block_api_on_complete(
    context: *mut Longtail_AsyncGetStoredBlockAPI,
    stored_block: *mut Longtail_StoredBlock,
    err: i32,
) {
    let proxy = context as *mut AsyncGetStoredBlockAPIProxy;
    let context = unsafe { (*proxy).context };
    let async_get_stored_block_api =
        unsafe { Box::from_raw(context as *mut Box<dyn AsyncGetStoredBlockAPI>) };
    async_get_stored_block_api.on_complete(stored_block, err);
    Box::into_raw(async_get_stored_block_api);
}

pub extern "C" fn async_get_stored_block_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut AsyncGetStoredBlockAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context as *mut Box<dyn AsyncGetStoredBlockAPI>) };
}

// AsyncPruneBlocksAPI
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
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut Longtail_API) };
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
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut Longtail_API) };
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
