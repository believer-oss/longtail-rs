use crate::*;
use tracing::Level;

// This is a macro that allows us to log dynamically based on the log level provided by longtail.
// https://github.com/tokio-rs/tracing/issues/2730#issuecomment-1943022805
#[macro_export]
macro_rules! dyn_event {
    ($lvl:ident, $($arg:tt)+) => {
        match $lvl {
            ::tracing::Level::TRACE => ::tracing::trace!($($arg)+),
            ::tracing::Level::DEBUG => ::tracing::debug!($($arg)+),
            ::tracing::Level::INFO => ::tracing::info!($($arg)+),
            ::tracing::Level::WARN => ::tracing::warn!($($arg)+),
            ::tracing::Level::ERROR => ::tracing::error!($($arg)+),
        }
    };
}

pub fn logcontext(log_context: Longtail_LogContext, context: &str) {
    let file = unsafe { std::ffi::CStr::from_ptr(log_context.file) };
    let function = unsafe { std::ffi::CStr::from_ptr(log_context.function) };
    let level = match log_context.level as u32 {
        LONGTAIL_LOG_LEVEL_DEBUG => Level::DEBUG,
        LONGTAIL_LOG_LEVEL_INFO => Level::INFO,
        LONGTAIL_LOG_LEVEL_WARNING => Level::WARN,
        LONGTAIL_LOG_LEVEL_ERROR => Level::ERROR,
        _ => Level::TRACE,
    };
    let mut fields = Vec::with_capacity(log_context.field_count as usize);
    for n in 1..=log_context.field_count as isize {
        let field = unsafe { *log_context.fields.offset(n - 1) };
        fields.push(tracing::field::display(field));
    }
    dyn_event!(
        level,
        context,
        file = file.to_str().unwrap(),
        function = function.to_str().unwrap(),
        line = log_context.line,
        fields = ?fields,
    );
}

pub fn set_longtail_loglevel(level: u32) {
    unsafe {
        Longtail_SetLogLevel(level as i32);
        Longtail_SetLog(Some(log_callback), std::ptr::null_mut());
    }
    // println!("Log Level: {0}", unsafe { Longtail_GetLogLevel() });
}

unsafe extern "C" fn log_callback(
    context: *mut Longtail_LogContext,
    log: *const std::os::raw::c_char,
) {
    let log = unsafe { std::ffi::CStr::from_ptr(log) };
    let context = unsafe { *context };
    logcontext(context, log.to_str().unwrap());
}

#[cfg(test)]
pub(crate) fn init_logging() -> Result<tracing::subscriber::DefaultGuard, Box<dyn std::error::Error>>
{
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .finish();
    crate::set_longtail_loglevel(crate::LONGTAIL_LOG_LEVEL_DEBUG);
    Ok(tracing::subscriber::set_default(subscriber))
}
