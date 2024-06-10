use crate::*;

#[repr(C)]
pub struct BikeshedJobAPI {
    pub job: *mut Longtail_JobAPI,
    _pin: std::marker::PhantomPinned,
}

impl Default for BikeshedJobAPI {
    fn default() -> Self {
        let workers = num_cpus::get() as u32;
        Self::new(workers, 0)
    }
}

impl Drop for BikeshedJobAPI {
    fn drop(&mut self) {
        unsafe { Longtail_DisposeAPI(&mut (*self.job).m_API as *mut Longtail_API) };
    }
}

impl BikeshedJobAPI {
    pub fn new(workers: u32, workers_priority: i32) -> BikeshedJobAPI {
        let job_api = unsafe { Longtail_CreateBikeshedJobAPI(workers, workers_priority) };
        BikeshedJobAPI {
            job: job_api,
            _pin: std::marker::PhantomPinned,
        }
    }
}
