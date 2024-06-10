use crate::*;

pub fn logcontext(log_context: Longtail_LogContext) {
    let file = unsafe { std::ffi::CStr::from_ptr(log_context.file) };
    let function = unsafe { std::ffi::CStr::from_ptr(log_context.function) };
    println!("LogContext: {:?}", log_context);
    println!("LogContext.File: {:?}", file);
    println!("LogContext.Function: {:?}", function);
    for n in 1..=log_context.field_count as isize {
        let field = unsafe { *log_context.fields.offset(n - 1) };
        let name = unsafe { std::ffi::CStr::from_ptr(field.name) };
        let value = unsafe { std::ffi::CStr::from_ptr(field.value) };
        println!("Field {:?}: {:?}", name, value);
    }
}

pub fn setup_logging(level: u32) {
    unsafe {
        Longtail_SetLogLevel(level as i32);
        Longtail_SetLog(Some(log_callback), std::ptr::null_mut());
    }
    println!("Log Level: {0}", unsafe { Longtail_GetLogLevel() });
}

unsafe extern "C" fn log_callback(
    context: *mut Longtail_LogContext,
    log: *const std::os::raw::c_char,
) {
    let log = unsafe { std::ffi::CStr::from_ptr(log) };
    let context = unsafe { *context };
    logcontext(context);
    println!("Log: {}", log.to_str().unwrap());
}
