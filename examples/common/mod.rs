use longtail::VersionIndex;
use std::io::Read;

#[allow(dead_code)]
pub fn version_index_from_file(filename: &str) -> VersionIndex {
    let mut f =
        std::fs::File::open(filename).unwrap_or_else(|_| panic!("Failed to open file: {filename}"));
    let metadata = f.metadata().unwrap();
    let mut buffer = vec![0u8; metadata.len() as usize];
    f.read_exact(&mut buffer).unwrap();
    let result = VersionIndex::new_from_buffer(&mut buffer);
    result.unwrap()
}
