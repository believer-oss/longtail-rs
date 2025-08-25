use thiserror::Error;

#[derive(Debug, Error)]
pub enum LongtailError {
    #[error("Longtail internal error: {0}")]
    Internal(LongtailInternalError),

    #[error("UTF8 error: {0}")]
    UTF8Error(std::str::Utf8Error),

    #[error("JSON error: {0}")]
    JSONError(serde_json::Error),

    #[error("Misc error: {0}")]
    Misc(Box<dyn std::error::Error>),

    #[error("Invalid JSON error: {0}")]
    JSONInvalid(String),

    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),
}

impl From<i32> for LongtailError {
    fn from(code: i32) -> Self {
        Self::Internal(code.into())
    }
}

#[derive(Debug)]
pub struct LongtailInternalError(i32);

impl From<LongtailInternalError> for LongtailError {
    fn from(err: LongtailInternalError) -> Self {
        Self::Internal(err)
    }
}

impl From<i32> for LongtailInternalError {
    fn from(code: i32) -> Self {
        Self(code)
    }
}

impl LongtailInternalError {
    pub fn new(code: i32) -> Self {
        Self(code)
    }
}

impl std::error::Error for LongtailInternalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl std::fmt::Display for LongtailInternalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // https://github.com/DanEngelbrecht/golongtail/blob/main/longtaillib/longtaillib.go#L15-L90
        let msg: &str = match self.0 {
            libc::E2BIG => "Argument list too long.",
            libc::EACCES => "Permission denied.",
            libc::EADDRINUSE => "Address in use.",
            libc::EADDRNOTAVAIL => "Address not available.",
            libc::EAFNOSUPPORT => "Address family not supported.",
            libc::EAGAIN => {
                "Resource unavailable, try again (may be the same value as [EWOULDBLOCK])."
            }
            libc::EALREADY => "Connection already in progress.",
            libc::EBADF => "Bad file descriptor.",
            libc::EBADMSG => "Bad message.",
            libc::EBUSY => "Device or resource busy.",
            libc::ECANCELED => "Operation canceled.",
            libc::ECHILD => "No child processes.",
            libc::ECONNABORTED => "Connection aborted.",
            libc::ECONNREFUSED => "Connection refused.",
            libc::ECONNRESET => "Connection reset.",
            libc::EDEADLK => "Resource deadlock would occur.",
            libc::EDESTADDRREQ => "Destination address required.",
            libc::EDOM => "Mathematics argument out of domain of function.",
            libc::EEXIST => "File exists.",
            libc::EFAULT => "Bad address.",
            libc::EFBIG => "File too large.",
            libc::EHOSTUNREACH => "Host is unreachable.",
            libc::EIDRM => "Identifier removed.",
            libc::EILSEQ => "Illegal byte sequence.",
            libc::EINPROGRESS => "Operation in progress.",
            libc::EINTR => "Interrupted function.",
            libc::EINVAL => "Invalid argument.",
            libc::EIO => "I/O error.",
            libc::EISCONN => "Socket is connected.",
            libc::EISDIR => "Is a directory.",
            libc::ELOOP => "Too many levels of symbolic links.",
            libc::EMFILE => "Too many open files.",
            libc::EMLINK => "Too many links.",
            libc::EMSGSIZE => "Message too large.",
            libc::ENAMETOOLONG => "Filename too long.",
            libc::ENETDOWN => "Network is down.",
            libc::ENETRESET => "Connection aborted by network.",
            libc::ENETUNREACH => "Network unreachable.",
            libc::ENFILE => "Too many files open in system.",
            libc::ENOBUFS => "No buffer space available.",
            libc::ENODATA => {
                "[XSR] [Option Start] No message is available on the STREAM head read queue. [Option End]"
            }
            libc::ENODEV => "No such device.",
            libc::ENOENT => "No such file or directory.",
            libc::ENOEXEC => "Executable file format error.",
            libc::ENOLCK => "No locks available.",
            libc::ENOLINK => "Link has been severed (POSIX.1-2001).",
            libc::ENOMEM => "Not enough space.",
            libc::ENOMSG => "No message of the desired type.",
            libc::ENOPROTOOPT => "Protocol not available.",
            libc::ENOSPC => "No space left on device.",
            libc::ENOSR => "[XSR] [Option Start] No STREAM resources. [Option End]",
            libc::ENOSTR => "[XSR] [Option Start] Not a STREAM. [Option End]",
            libc::ENOSYS => "Function not supported.",
            libc::ENOTCONN => "The socket is not connected.",
            libc::ENOTDIR => "Not a directory.",
            libc::ENOTEMPTY => "Directory not empty.",
            libc::ENOTSOCK => "Not a socket.",
            libc::ENOTSUP => "Not supported.",
            libc::ENOTTY => "Inappropriate I/O control operation.",
            libc::ENXIO => "No such device or address.",
            libc::EOVERFLOW => "Value too large to be stored in data type.",
            libc::EPERM => "Operation not permitted.",
            libc::EPIPE => "Broken pipe.",
            libc::EPROTO => "Protocol error.",
            libc::EPROTONOSUPPORT => "Protocol not supported.",
            libc::EPROTOTYPE => "Protocol wrong type for socket.",
            libc::ERANGE => "Result too large.",
            libc::EROFS => "Read-only file system.",
            libc::ESPIPE => "Invalid seek.",
            libc::ESRCH => "No such process.",
            libc::ETIME => "[XSR] [Option Start] Stream ioctl() timeout. [Option End]",
            libc::ETIMEDOUT => "Connection timed out.",
            libc::ETXTBSY => "Text file busy.",
            libc::EXDEV => "Cross-device link. ",
            _ => "Unknown error",
        };

        write!(f, "{} ({})", msg, self.0)
    }
}
