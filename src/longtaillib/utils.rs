// use crate::*;

pub const UNC_PREFIX: &str = "\\\\?\\";
const NETWORK_PREFIX: &str = "\\";

// TODO: Not sure if this is the correct implementation...
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
