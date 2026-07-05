#![allow(clippy::empty_line_after_outer_attr)]
#[rustfmt::skip]
// Progress API
// pub fn Longtail_GetProgressAPISize() -> u64;
// pub fn Longtail_MakeProgressAPI( mem: *mut ::std::os::raw::c_void, dispose_func: Longtail_DisposeFunc, on_progress_func: Longtail_Progress_OnProgressFunc,) -> *mut Longtail_ProgressAPI;
// pub fn Longtail_Progress_OnProgress( progressAPI: *mut Longtail_ProgressAPI, total_count: u32, done_count: u32,);
// pub fn Longtail_CreateRateLimitedProgress( progress_api: *mut Longtail_ProgressAPI, percent_rate_limit: u32,) -> *mut Longtail_ProgressAPI;

use crate::{Longtail_API,  Longtail_ProgressAPI};

pub trait ProgressAPI {
    fn on_progress(&self, total_count: u32, done_count: u32);
}

#[repr(C)]
pub struct ProgressAPIProxy {
    pub api: Longtail_ProgressAPI,
    pub context: *mut std::os::raw::c_void,
    _pin: std::marker::PhantomPinned,
}

impl Drop for ProgressAPIProxy {
    fn drop(&mut self) {
        let _context = unsafe { Box::from_raw(self.context as *mut Box<dyn ProgressAPI>) };
    }
}

impl ProgressAPIProxy {
    pub fn new(progress: Box<dyn ProgressAPI>) -> ProgressAPIProxy {
        ProgressAPIProxy {
            api: Longtail_ProgressAPI {
                m_API: Longtail_API {
                    Dispose: Some(progress_api_dispose),
                },
                OnProgress: Some(progress_api_on_progress),
            },
            context: Box::into_raw(Box::new(progress)) as *mut std::os::raw::c_void,
            _pin: std::marker::PhantomPinned,
        }
    }
}

pub extern "C" fn progress_api_on_progress(
    context: *mut Longtail_ProgressAPI,
    total_count: u32,
    done_count: u32,
) {
    let proxy = context as *mut ProgressAPIProxy;
    let context = unsafe { (*proxy).context };
    let progress = unsafe { Box::from_raw(context as *mut Box<dyn ProgressAPI>) };
    progress.on_progress(total_count, done_count);
    let _ = Box::into_raw(progress);
}

pub extern "C" fn progress_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut ProgressAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context as *mut Box<dyn ProgressAPI>) };
}
