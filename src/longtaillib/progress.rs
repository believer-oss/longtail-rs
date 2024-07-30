#[allow(unused_imports)]
use crate::{Longtail_API, Longtail_DisposeAPI, Longtail_ProgressAPI};

pub trait ProgressAPI {
    fn on_progress(&self, total_count: u32, done_count: u32);
}

#[repr(C)]
pub struct ProgressAPIProxy {
    pub api: Longtail_ProgressAPI,
    pub context: *mut std::os::raw::c_void,
    _pin: std::marker::PhantomPinned,
}

// TODO: Unused, since we're relying on the dispose function to handle it?
impl Drop for ProgressAPIProxy {
    fn drop(&mut self) {
        // unsafe { Longtail_DisposeAPI(&mut (*self.api).m_API as *mut
        // Longtail_API) };
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
    Box::into_raw(progress);
}

pub extern "C" fn progress_api_dispose(api: *mut Longtail_API) {
    let context = unsafe { (*(api as *mut ProgressAPIProxy)).context };
    let _ = unsafe { Box::from_raw(context as *mut Box<dyn ProgressAPI>) };
}
