#![allow(clippy::empty_line_after_outer_attr)]
#[rustfmt::skip]
// Job API
// pub fn Longtail_GetJobAPISize() -> u64;
// pub fn Longtail_MakeJobAPI( mem: *mut ::std::os::raw::c_void, dispose_func: Longtail_DisposeFunc, get_worker_count_func: Longtail_Job_GetWorkerCountFunc, reserve_jobs_func: Longtail_Job_ReserveJobsFunc, create_jobs_func: Longtail_Job_CreateJobsFunc, add_dependecies_func: Longtail_Job_AddDependeciesFunc, ready_jobs_func: Longtail_Job_ReadyJobsFunc, wait_for_all_jobs_func: Longtail_Job_WaitForAllJobsFunc, resume_job_func: Longtail_Job_ResumeJobFunc, get_max_batch_count_func: Longtail_Job_GetMaxBatchCountFunc,) -> *mut Longtail_JobAPI;
// pub fn Longtail_Job_GetWorkerCount(job_api: *mut Longtail_JobAPI) -> u32;
// pub fn Longtail_Job_ReserveJobs( job_api: *mut Longtail_JobAPI, job_count: u32, out_job_group: *mut Longtail_JobAPI_Group,) -> ::std::os::raw::c_int;
// pub fn Longtail_Job_CreateJobs( job_api: *mut Longtail_JobAPI, job_group: Longtail_JobAPI_Group, progressAPI: *mut Longtail_ProgressAPI, optional_cancel_api: *mut Longtail_CancelAPI, optional_cancel_token: Longtail_CancelAPI_HCancelToken, job_count: u32, job_funcs: *mut Longtail_JobAPI_JobFunc, job_contexts: *mut *mut ::std::os::raw::c_void, job_channel: u8, out_jobs: *mut Longtail_JobAPI_Jobs,) -> ::std::os::raw::c_int;
// pub fn Longtail_Job_AddDependecies( job_api: *mut Longtail_JobAPI, job_count: u32, jobs: Longtail_JobAPI_Jobs, dependency_job_count: u32, dependency_jobs: Longtail_JobAPI_Jobs,) -> ::std::os::raw::c_int;
// pub fn Longtail_Job_ReadyJobs( job_api: *mut Longtail_JobAPI, job_count: u32, jobs: Longtail_JobAPI_Jobs,) -> ::std::os::raw::c_int;
// pub fn Longtail_Job_WaitForAllJobs( job_api: *mut Longtail_JobAPI, job_group: Longtail_JobAPI_Group, progressAPI: *mut Longtail_ProgressAPI, optional_cancel_api: *mut Longtail_CancelAPI, optional_cancel_token: Longtail_CancelAPI_HCancelToken,) -> ::std::os::raw::c_int;
// pub fn Longtail_Job_ResumeJob( job_api: *mut Longtail_JobAPI, job_id: u32,) -> ::std::os::raw::c_int;
// pub fn Longtail_Job_GetMaxBatchCount( job_api: *mut Longtail_JobAPI, out_max_job_batch_count: *mut u32, out_max_dependency_batch_count: *mut u32,) -> ::std::os::raw::c_int;
// pub fn Longtail_CreateBikeshedJobAPI( worker_count: u32, worker_priority: ::std::os::raw::c_int,) -> *mut Longtail_JobAPI;
// pub fn Longtail_RunJobsBatched( job_api: *mut Longtail_JobAPI, progress_api: *mut Longtail_ProgressAPI, optional_cancel_api: *mut Longtail_CancelAPI, optional_cancel_token: Longtail_CancelAPI_HCancelToken, total_job_count: u32, job_funcs: *mut Longtail_JobAPI_JobFunc, job_ctxs: *mut *mut ::std::os::raw::c_void, out_jobs_submitted: *mut u32,) -> ::std::os::raw::c_int;
//
// struct Longtail_JobAPI
// {
//     struct Longtail_API m_API;
//     Longtail_Job_GetWorkerCountFunc GetWorkerCount;
//     Longtail_Job_ReserveJobsFunc ReserveJobs;
//     Longtail_Job_CreateJobsFunc CreateJobs;
//     Longtail_Job_AddDependeciesFunc AddDependecies;
//     Longtail_Job_ReadyJobsFunc ReadyJobs;
//     Longtail_Job_WaitForAllJobsFunc WaitForAllJobs;
//     Longtail_Job_ResumeJobFunc ResumeJob;
//     Longtail_Job_GetMaxBatchCountFunc GetMaxBatchCount;
// };

use crate::*;
use std::ops::{Deref, DerefMut};

/// The Job API provides functions for managing jobs and workers. This is
/// implemented in Longtail using the [Bikeshed](https://github.com/DanEngelbrecht/bikeshed) job system.
#[repr(C)]
#[derive(Debug, Clone)]
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
