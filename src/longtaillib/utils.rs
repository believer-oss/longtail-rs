use std::{ffi::CString, path::Path};

pub const UNC_PREFIX: &str = "\\\\?\\";
const NETWORK_PREFIX: &str = "\\";

// TODO: Not sure if this is the correct implementation...
/// Normalize a file system path, based on the golongtail code. This function
/// appears to be primarily used to normalize Windows paths.
pub fn normalize_file_system_path(path: String) -> String {
    match path {
        path if path.starts_with(UNC_PREFIX) => {
            let forward_slash_replaced = path.replace('/', "\\");
            let double_backward_removed = forward_slash_replaced.replace("\\\\", "\\");
            UNC_PREFIX.to_string() + &double_backward_removed
        }
        path if path.starts_with(NETWORK_PREFIX) => {
            let forward_slash_replaced = path.replace('/', "\\");
            let double_backward_removed = forward_slash_replaced.replace("\\\\", "\\");
            NETWORK_PREFIX.to_string() + &double_backward_removed
        }
        _ => {
            let backward_removed = path.replace('\\', "/");
            backward_removed.replace("//", "/")
        }
    }
}

pub fn path_to_cstring(path: &Path) -> CString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        CString::new(path.as_os_str().as_bytes()).expect("Failed to convert path to CString")
    }

    // This should work because longtail assumes a UTF-8 codepage in a call to
    // MultiByteToWideChar on the inbound paths on Windows.
    #[cfg(windows)]
    CString::new(path.to_string_lossy().into_owned().into_bytes())
        .expect("Failed to convert path to CString")
}
