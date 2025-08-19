use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub struct FileCacheData {
    pub path: PathBuf,
    pub size: u64,
    pub timestamp: SystemTime,
}

impl FileCacheData {
    pub fn collect(path: &Path) -> Vec<FileCacheData> {
        let mut vec: Vec<FileCacheData> = vec![];
        Self::collect_internal(path, &mut vec);
        vec
    }

    fn collect_internal(path: &Path, data: &mut Vec<FileCacheData>) {
        if let Ok(dir_entries) = fs::read_dir(path) {
            for entry in dir_entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_dir() {
                        Self::collect_internal(&entry.path(), data);
                    } else if metadata.is_file() {
                        let last_modified = metadata.modified().unwrap();
                        let last_accessed = metadata.accessed().unwrap();

                        let timestamp = last_modified.max(last_accessed);

                        data.push(FileCacheData {
                            path: entry.path().clone(),
                            size: metadata.len(),
                            timestamp,
                        });
                    }
                }
            }
        }
    }
}
