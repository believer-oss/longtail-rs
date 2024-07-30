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
}

#[derive(Debug)]
pub struct LongtailInternalError(i32);

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
        // https://en.wikipedia.org/wiki/Errno.h
        let msg: &str = match self.0 {
            1 => "Operation not permitted",
            2 => "No such file or directory",
            13 => "Permission denied",
            22 => "Invalid argument",
            28 => "No space left on device",
            30 => "Read-only file system",
            39 => "Directory not empty",
            40 => "Too many symbolic links encountered",
            42 => "File name too long",
            54 => "Connection reset by peer",
            55 => "No buffer space available",
            _ => "Unknown error",
        };

        write!(f, "{} ({})", msg, self.0)
    }
}
