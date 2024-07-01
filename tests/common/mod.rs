// https://github.com/DanEngelbrecht/golongtail/blob/main/commands/commands_test.go#L119
use std::collections::HashMap;

use lazy_static::lazy_static;
use longtail::{create_blob_store, BlobClient, BlobObject, BlobStore, MemBlobStore};

lazy_static! {
    static ref V1_FILES: HashMap<&'static str, &'static str> = HashMap::from([
        ("empty-file", ""),
        ("abitoftext.txt", "this is a test file"),
        (
            "folder/abitoftextinasubfolder.txt",
            "this is a test file in a subfolder",
        ),
        (
            "folder/anotherabitoftextinasubfolder.txt",
            "this is a second test file in a subfolder",
        ),
    ]);
}

lazy_static! {
    static ref V2_FILES: HashMap<&'static str, &'static str> = HashMap::from([
        ("empty-file", ""),
        ("abitoftext.txt", "this is a test file"),
        (
            "folder/abitoftextinasubfolder.txt",
            "this is a test file in a subfolder",
        ),
        (
            "folder/anotherabitoftextinasubfolder.txt",
            "this is a second test file in a subfolder",
        ),
        ("stuff.txt", "we have some stuff"),
        (
            "folder2/anotherabitoftextinasubfolder2.txt",
            "and some more text that we need",
        ),
    ]);
}
lazy_static! {
    static ref V3_FILES: HashMap<&'static str, &'static str> = HashMap::from([
        ("empty-file", ""),
        ("abitoftext.txt", "this is a test file"),
        (
            "folder/abitoftextinasubfolder.txt",
            "this is a test file in a subfolder",
        ),
        (
            "folder/anotherabitoftextinasubfolder.txt",
            "this is a second test file in a subfolder",
        ),
        ("stuff.txt", "we have some stuff"),
        ("morestuff.txt", "we have even more stuff"),
        (
            "folder2/anotherabitoftextinasubfolder2.txt",
            "and some more text that we need",
        ),
    ]);
}

lazy_static! {
    static ref LAYER_DATA: HashMap<&'static str, &'static str> = HashMap::from([
        ("empty-file", ""),
        ("abitoftext.txt", "this is a test file"),
        ("abitoftext.layer2", "second layer test file"),
        (
            "folder/abitoftextmvinasubfolder.txt",
            "this is a test file in a subfolder",
        ),
        (
            "folder/abitoftextmvinasubfolder.layer2",
            "layer 2 data in folder",
        ),
        (
            "folder/anotherabitoftextinasubfolder.txt",
            "this is a second test file in a subfolder",
        ),
        ("stuff.txt", "we have some stuff"),
        ("blobby/fluff.layer2", "more fluff is always essential"),
        ("glob.layer2", "glob is all you need"),
        ("morestuff.txt", "we have some more stuff"),
        (
            "folder2/anotherabitoftextinasubfolder2.txt",
            "and some more text that we need",
        ),
        (
            "folder2/anotherabitoftextinasubfolder2.layer3",
            "stuff for layer 3 is good stuff for any layer",
        ),
        ("folder3/wefewef.layer3", "layer3 on top of the world"),
    ]);
}

fn create_content(path: &str, content: HashMap<&str, &str>) {
    let mut store = MemBlobStore::new("test", true);
    content.iter().for_each(|(name, data)| {
        let mut client = store.new_client().unwrap();
        let mut newname = path.to_string();
        newname.push_str(name);
        let mut f = client.new_object(newname).unwrap();
        f.write(data.as_bytes());
    });
}

fn validate_content(store: MemBlobStore, path: String, content: HashMap<&str, &str>) {
    let mut store = MemBlobStore::new("test", true);
    let client = store.new_client().unwrap();
    let items = client.get_objects(path).unwrap();
    let mut found_items = HashMap::new();
    items.iter().for_each(|item| {
        let mut client = store.new_client().unwrap();
        let orig = content.get(item.name.as_str());
        if let Some(data) = orig {
            let f = client.new_object(item.name.clone()).unwrap();
            let mut buffer = Vec::new();
            f.read(&mut buffer);
            assert_eq!(*data.as_bytes(), buffer);
            found_items.insert(item.name.clone(), item.size);
        }
    });
}

fn create_version_data(base_uri: String) {
    // let store = create_blob_store(base_uri);
    create_content("version/v1/", V1_FILES.clone());
    create_content("version/v2/", V2_FILES.clone());
    create_content("version/v3/", V3_FILES.clone());
}

fn create_layer_data(base_uri: String) {
    // let store = create_blob_store(base_uri);
    create_content("source/", LAYER_DATA.clone());
}
