use std::path::Path;

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

// Convert a Path to os-specific bytes to pass into C
// https://doc.rust-lang.org/std/os/windows/ffi/trait.OsStrExt.html#tymethod.encode_wide
// https://stackoverflow.com/a/59224987
// I don't see any better method, sadly
pub fn path_to_bytes(path: &Path) -> Vec<u8> {
    let mut buf = Vec::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        buf.extend(path.as_os_str().as_bytes());
        buf.push(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        buf.extend(
            path.as_os_str()
                .encode_wide()
                .chain(Some(0))
                .map(|b| {
                    let b = b.to_ne_bytes();
                    b.get(0).map(|s| *s).into_iter().chain(b.get(1).map(|s| *s))
                })
                .flatten(),
        );
    }
    buf
}
