// use crate::*;

const UNCPrefix: &str = "\\\\?\\";
const NetworkPrefix: &str = "\\";

// TODO: Not sure if this is the correct implementation...
pub fn NormalizeFileSystemPath(path: String) -> String {
    match path {
        path if path.starts_with(UNCPrefix) => {
            let forward_slash_replaced = path.replace('/', "\\");
            let double_backward_removed = forward_slash_replaced.replace("\\\\", "\\");
            UNCPrefix.to_string() + &double_backward_removed
        }
        path if path.starts_with(NetworkPrefix) => {
            let forward_slash_replaced = path.replace('/', "\\");
            let double_backward_removed = forward_slash_replaced.replace("\\\\", "\\");
            NetworkPrefix.to_string() + &double_backward_removed
        }
        _ => {
            let backward_removed = path.replace('\\', "/");
            backward_removed.replace("//", "/")
        }
    }
}
