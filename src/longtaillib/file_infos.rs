#[rustfmt::skip]
// File Infos API
// pub fn Longtail_FileInfos_GetCount(file_infos: *const Longtail_FileInfos) -> u32;
// pub fn Longtail_FileInfos_GetPath( file_infos: *const Longtail_FileInfos, index: u32,) -> *const ::std::os::raw::c_char;
// pub fn Longtail_FileInfos_GetSize(file_infos: *const Longtail_FileInfos, index: u32) -> u64;
// pub fn Longtail_FileInfos_GetPermissions( file_infos: *const Longtail_FileInfos, index: u32,) -> *const u16;
//
// struct Longtail_FileInfos
// {
//     uint32_t m_Count;
//     uint32_t m_PathDataSize;
//     uint64_t* m_Sizes;
//     uint32_t* m_PathStartOffsets;
//     uint16_t* m_Permissions;
//     char* m_PathData;
// };

use crate::Longtail_FileInfos;

#[derive(Debug)]
pub struct FileInfos(pub *mut Longtail_FileInfos);

impl FileInfos {
    pub fn get_file_count(&self) -> u32 {
        unsafe { (*self.0).m_Count }
    }
    fn get_path_data_size(&self) -> u32 {
        unsafe { (*self.0).m_PathDataSize }
    }
    fn get_sizes_ptr(&self) -> *const u64 {
        unsafe { (*self.0).m_Sizes }
    }
    fn get_permissions_ptr(&self) -> *const u16 {
        unsafe { (*self.0).m_Permissions }
    }
    fn get_path_data_ptr(&self) -> *const u8 {
        unsafe { (*self.0).m_PathData as *const _ }
    }

    fn get_path_start_offsets(&self, index: u32) -> u32 {
        // The index should be less than the file count
        assert!(index < self.get_file_count());
        let index = isize::try_from(index).expect("Failed to convert index to isize");
        unsafe { *(*self.0).m_PathStartOffsets.offset(index) }
    }
    pub fn get_file_path(&self, index: u32) -> String {
        let offset = self.get_path_start_offsets(index);

        // The offset should be less than the path data size
        assert!(offset < self.get_path_data_size());
        let offset = usize::try_from(offset).expect("Failed to convert offset to usize");
        unsafe {
            let data = self.get_path_data_ptr().add(offset);
            std::ffi::CStr::from_ptr(data as *const _)
                .to_string_lossy()
                .into_owned()
        }
    }
    pub fn get_file_size(&self, index: u32) -> u64 {
        // The index should be less than the file count
        assert!(index < self.get_file_count());
        let index = isize::try_from(index).expect("Failed to convert index to isize");
        unsafe { *self.get_sizes_ptr().offset(index) }
    }
    pub fn get_file_permissions(&self, index: u32) -> u16 {
        // The index should be less than the file count
        assert!(index < self.get_file_count());
        let index = isize::try_from(index).expect("Failed to convert index to isize");
        unsafe { *self.get_permissions_ptr().offset(index) }
    }
    pub fn iter(&self) -> FileInfosIterator {
        FileInfosIterator {
            file_infos: self,
            index: 0,
        }
    }
    pub fn get_compression_types_for_files(&self, compression_type: u32) -> Vec<u32> {
        let len = self
            .get_file_count()
            .try_into()
            .expect("Failed to convert usize to u32");
        vec![compression_type; len]
    }
}

pub struct FileInfosIterator<'a> {
    file_infos: &'a FileInfos,
    index: u32,
}
type FileInfosItem = (String, u64, u16);

impl Iterator for FileInfosIterator<'_> {
    type Item = FileInfosItem;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.file_infos.get_file_count() {
            return None;
        }
        let path = (*self.file_infos).get_file_path(self.index);
        let size = (*self.file_infos).get_file_size(self.index);
        let permissions = (*self.file_infos).get_file_permissions(self.index);
        self.index += 1;
        Some((path, size, permissions))
    }
}
