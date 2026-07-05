//! FileInfos scan-path semantics: the exact byte-wise `strcmp` sort order and
//! canonical name-blob building (`docs/format-spec.md` §1).

use longtail_core::{FileEntry, FileInfos, Permissions};

fn file(path: &str) -> FileEntry {
    FileEntry {
        relative_path: path.to_string(),
        size: path.len() as u64,
        permissions: Permissions(0o644),
        is_dir: false,
    }
}

fn dir(path: &str) -> FileEntry {
    FileEntry {
        relative_path: path.to_string(),
        size: 0,
        permissions: Permissions(0o755),
        is_dir: true,
    }
}

fn sorted_paths(entries: Vec<FileEntry>) -> Vec<String> {
    let fi = FileInfos::from_scanned_entries(entries);
    (0..fi.count() as usize)
        .map(|i| fi.path(i).unwrap().to_string())
        .collect()
}

#[test]
fn byte_wise_strcmp_order() {
    // '-'=0x2D < '.'=0x2E < '/'=0x2F < '0'=0x30 < 'B'=0x42 < 'a'=0x61 < 'b'=0x62
    // (the adversarial chain from the work order).
    let entries = vec![
        file("ab"),
        file("aB"),
        file("a0"),
        file("a/b"),
        file("a.b"),
        file("a-b"),
    ];
    assert_eq!(
        sorted_paths(entries),
        vec!["a-b", "a.b", "a/b", "a0", "aB", "ab"]
    );
}

#[test]
fn uppercase_sorts_before_lowercase() {
    // 'B' (0x42) < 'a' (0x61): case-sensitive, no folding.
    assert_eq!(
        sorted_paths(vec![file("a.txt"), file("B.txt")]),
        vec!["B.txt", "a.txt"]
    );
}

#[test]
fn prefix_sorts_before_longer() {
    // strcmp: "ab" < "abc" (NUL < 'c'); "ab" < "ab/".
    assert_eq!(
        sorted_paths(vec![file("abc"), file("ab"), dir("ab-dir")]),
        // 'ab' < 'ab-dir' (NUL<'-') < 'abc' ; dir sort key is bare "ab-dir".
        vec!["ab", "ab-dir/", "abc"]
    );
}

#[test]
fn directory_sort_key_has_no_trailing_slash() {
    // A directory "a" sorts by bare "a" — so "a" (dir) < "a.b" (file), because
    // the sort key is "a" (NUL) vs "a.b", not "a/" (which would sort AFTER
    // "a.b" since '/'=0x2F > '.'=0x2E).
    let paths = sorted_paths(vec![file("a.b"), dir("a")]);
    assert_eq!(paths, vec!["a/", "a.b"]);
}

#[test]
fn name_blob_encoding_files_and_dirs() {
    // Files: "<name>\0"; dirs: "<name>/\0". Offsets accumulate by that width.
    let fi = FileInfos::from_scanned_entries(vec![dir("d"), file("a")]);
    // Sorted: "a" (file), "d" (dir).
    assert_eq!(fi.path_data, b"a\0d/\0");
    assert_eq!(fi.path_start_offsets, vec![0, 2]);
    assert_eq!(fi.path(0).unwrap(), "a");
    assert_eq!(fi.path(1).unwrap(), "d/");
    assert!(!fi.is_dir(0).unwrap());
    assert!(fi.is_dir(1).unwrap());
    assert_eq!(fi.path_data_size(), 5);
    assert_eq!(fi.count(), 2);
}

#[test]
fn metadata_follows_sort() {
    // Sizes/permissions must travel with their entry through the sort.
    let entries = vec![
        FileEntry {
            relative_path: "zzz".into(),
            size: 111,
            permissions: Permissions(0o600),
            is_dir: false,
        },
        FileEntry {
            relative_path: "aaa".into(),
            size: 222,
            permissions: Permissions(0o755),
            is_dir: false,
        },
    ];
    let fi = FileInfos::from_scanned_entries(entries);
    assert_eq!(fi.path(0).unwrap(), "aaa");
    assert_eq!(fi.size(0).unwrap(), 222);
    assert_eq!(fi.permissions(0).unwrap(), Permissions(0o755));
    assert_eq!(fi.path(1).unwrap(), "zzz");
    assert_eq!(fi.size(1).unwrap(), 111);
    assert_eq!(fi.permissions(1).unwrap(), Permissions(0o600));
}

#[test]
fn empty_is_well_formed() {
    let fi = FileInfos::from_scanned_entries(vec![]);
    assert_eq!(fi.count(), 0);
    assert_eq!(fi.path_data_size(), 0);
    assert!(fi.path(0).is_err());
}

#[test]
fn nested_paths_sort_globally() {
    // Single global sort across folders: '/' (0x2F) is an ordinary byte.
    let entries = vec![file("b/x"), file("a/z"), file("a/a"), dir("a"), file("ab")];
    // "a" (dir, key "a") < "a/a" < "a/z" < "ab" < "b/x"
    assert_eq!(sorted_paths(entries), vec!["a/", "a/a", "a/z", "ab", "b/x"]);
}
