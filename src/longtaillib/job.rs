use crate::*;
use std::ops::{Deref, DerefMut};

// If we want to implement our own job api, we'll need some changes here...

#[repr(C)]
pub struct BikeshedJobAPI {
    pub job_api: *mut Longtail_JobAPI,
    _pin: std::marker::PhantomPinned,
}

impl Default for BikeshedJobAPI {
    fn default() -> Self {
        let workers = num_cpus::get() as u32;
        Self::new(workers, 0)
    }
}

impl Deref for BikeshedJobAPI {
    type Target = *mut Longtail_JobAPI;
    fn deref(&self) -> &Self::Target {
        &self.job_api
    }
}

impl DerefMut for BikeshedJobAPI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.job_api
    }
}

impl Drop for BikeshedJobAPI {
    fn drop(&mut self) {
        unsafe { Longtail_DisposeAPI(&mut (*self.job_api).m_API as *mut Longtail_API) };
    }
}

impl BikeshedJobAPI {
    pub fn new(workers: u32, workers_priority: i32) -> BikeshedJobAPI {
        let job_api = unsafe { Longtail_CreateBikeshedJobAPI(workers, workers_priority) };
        BikeshedJobAPI {
            job_api,
            _pin: std::marker::PhantomPinned,
        }
    }
}
