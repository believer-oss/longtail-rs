#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

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

pub fn setup_logging() {
    unsafe {
        Longtail_SetLog(Some(log_callback), std::ptr::null_mut());
    }
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

#[allow(dead_code)]
fn read_version_index_from_buffer(buffer: &mut [u8]) -> Result<Longtail_VersionIndex, i32> {
    let buffer_size = buffer.len();
    let mut version_index = std::ptr::null_mut::<Longtail_VersionIndex>();
    let result = unsafe {
        Longtail_ReadVersionIndexFromBuffer(
            buffer.as_mut_ptr().cast(),
            buffer_size,
            &mut version_index,
        )
    };
    println!("Result: {}", result);
    if result != 0 {
        return Err(result);
    }
    let ret = unsafe { *version_index };
    Ok(ret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_read_version_index_from_buffer() {
        // let mut buffer = [0u8; 1024];
        // let result = read_version_index_from_buffer(&mut buffer);
        // assert_eq!(result, Err(9)); // Empty buffer

        setup_logging();

        let mut f = std::fs::File::open("test-data/storage/testdir/store.lsi").unwrap();
        let metadata = f.metadata().unwrap();
        let mut buffer = vec![0u8; metadata.len() as usize];
        println!("Reading {} bytes", metadata.len());
        f.read_exact(&mut buffer).unwrap();
        println!("Bytes read: {:?}", buffer);
        let result = read_version_index_from_buffer(&mut buffer);
        // assert_eq!(
        //     result,
        //     Ok(Longtail_VersionIndex {
        //         m_Version: todo!(),
        //         m_HashIdentifier: todo!(),
        //         m_TargetChunkSize: todo!(),
        //         m_AssetCount: todo!(),
        //         m_ChunkCount: todo!(),
        //         m_AssetChunkIndexCount: todo!(),
        //         m_PathHashes: todo!(),
        //         m_ContentHashes: todo!(),
        //         m_AssetSizes: todo!(),
        //         m_AssetChunkCounts: todo!(),
        //         m_AssetChunkIndexStarts: todo!(),
        //         m_AssetChunkIndexes: todo!(),
        //         m_ChunkHashes: todo!(),
        //         m_ChunkSizes: todo!(),
        //         m_ChunkTags: todo!(),
        //         m_NameOffsets: todo!(),
        //         m_NameDataSize: todo!(),
        //         m_Permissions: todo!(),
        //         m_NameData: todo!()
        //     })
        // );
        println!("Result: {:?}", result);
        assert_eq!(1, 0);
    }
}
