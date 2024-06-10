use std::env;
use std::path::PathBuf;

#[allow(dead_code)]
fn setup_windows() -> PathBuf {
    // This is the directory where the `c` library is located.
    let libdir_path = PathBuf::from("dist-win32-x64/include/src")
        // Canonicalize the path as `rustc-link-search` requires an absolute
        // path.
        .canonicalize()
        .expect("cannot canonicalize path");

    // Tell cargo to look for shared libraries in the specified directory
    println!("cargo:rustc-link-search=dist-win32-x64");

    // Tell cargo to tell rustc to link the system bzip2
    // shared library.
    println!("cargo:rustc-link-lib=static=longtail_win32_x64");

    libdir_path
}

#[allow(dead_code)]
fn setup_linux() -> PathBuf {
    // This is the directory where the `c` library is located.
    let libdir_path = PathBuf::from("dist-linux-x64/include/src")
        // Canonicalize the path as `rustc-link-search` requires an absolute
        // path.
        .canonicalize()
        .expect("cannot canonicalize path");

    // Tell cargo to look for shared libraries in the specified directory
    println!("cargo:rustc-link-search=dist-linux-x64");

    // Tell cargo to tell rustc to link the system bzip2
    // shared library.
    println!("cargo:rustc-link-lib=static=longtail_linux_x64_debug");

    libdir_path
}

#[allow(dead_code)]
fn setup_linux_debug() -> PathBuf {
    let libdir_path = PathBuf::from("longtail/src")
        // Canonicalize the path as `rustc-link-search` requires an absolute
        // path.
        .canonicalize()
        .expect("cannot canonicalize path");

    println!("cargo:rustc-link-search=longtail/build/linux_x64/longtail_static/debug");
    println!("cargo:rustc-link-lib=static=longtail_static");

    libdir_path
}

fn main() {
    #[cfg(target_os = "windows")]
    let libdir_path = setup_windows();

    #[cfg(target_os = "linux")]
    let libdir_path = setup_linux_debug();
    // let libdir_path = setup_linux();

    // This is the path to the `c` headers file.
    let longtail_header_path = libdir_path.join("longtail.h");
    let longtail_header_path_str = longtail_header_path
        .to_str()
        .expect("Path is not a valid string");

    const EXTRA_HEADERS: [(&str, &str); 27] = [
        ("archiveblockstore", "longtail_archiveblockstore.h"),
        ("atomiccancel", "longtail_atomiccancel.h"),
        ("bikeshed", "longtail_bikeshed.h"),
        ("blake2", "longtail_blake2.h"),
        ("blake3", "longtail_blake3.h"),
        ("blockstorestorage", "longtail_blockstorestorage.h"),
        ("brotli", "longtail_brotli.h"),
        ("cacheblockstore", "longtail_cacheblockstore.h"),
        ("compressblockstore", "longtail_compressblockstore.h"),
        ("compressionregistry", "longtail_compression_registry.h"),
        (
            "compressionregistry",
            "longtail_full_compression_registry.h",
        ),
        (
            "compressionregistry",
            "longtail_zstd_compression_registry.h",
        ),
        ("concurrentchunkwrite", "longtail_concurrentchunkwrite.h"),
        ("filestorage", "longtail_filestorage.h"),
        ("fsblockstore", "longtail_fsblockstore.h"),
        ("hashregistry", "longtail_blake3_hash_registry.h"),
        ("hashregistry", "longtail_full_hash_registry.h"),
        ("hashregistry", "longtail_hash_registry.h"),
        ("hpcdcchunker", "longtail_hpcdcchunker.h"),
        ("lrublockstore", "longtail_lrublockstore.h"),
        ("lz4", "longtail_lz4.h"),
        ("memstorage", "longtail_memstorage.h"),
        ("memtracer", "longtail_memtracer.h"),
        ("meowhash", "longtail_meowhash.h"),
        ("ratelimitedprogress", "longtail_ratelimitedprogress.h"),
        ("shareblockstore", "longtail_shareblockstore.h"),
        ("zstd", "longtail_zstd.h"),
    ];

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let builder = bindgen::Builder::default()
        // The input header we would like to generate
        // bindings for.
        .header(longtail_header_path_str);
    // .no_debug("Longtail_VersionIndex")

    let headers = EXTRA_HEADERS
        .iter()
        .map(|(module, header)| {
            let header_path = libdir_path.join("..").join("lib").join(module).join(header);
            header_path
                .to_str()
                .expect("Path is not a valid string")
                .to_string()
        })
        .collect::<Vec<_>>();

    let builder = headers
        .iter()
        .fold(builder, |builder, header| builder.header(header));

    // https://github.com/rust-lang/rust-bindgen/pull/2369
    // let builder = builder
    //     .header_contents("compression", "
    //         #define LONGTAIL_BROTLI_COMPRESSION_TYPE             ((((uint32_t)'b') << 24) + (((uint32_t)'t') << 16) + (((uint32_t)'l') << 8))
    //         #define LONGTAIL_BROTLI_GENERIC_MIN_QUALITY_TYPE     (LONGTAIL_BROTLI_COMPRESSION_TYPE + ((uint32_t)'0'))
    //         #define LONGTAIL_BROTLI_GENERIC_DEFAULT_QUALITY_TYPE (LONGTAIL_BROTLI_COMPRESSION_TYPE + ((uint32_t)'1'))
    //         #define LONGTAIL_BROTLI_GENERIC_MAX_QUALITY_TYPE     (LONGTAIL_BROTLI_COMPRESSION_TYPE + ((uint32_t)'2'))
    //         #define LONGTAIL_BROTLI_TEXT_MIN_QUALITY_TYPE        (LONGTAIL_BROTLI_COMPRESSION_TYPE + ((uint32_t)'a'))
    //         #define LONGTAIL_BROTLI_TEXT_DEFAULT_QUALITY_TYPE    (LONGTAIL_BROTLI_COMPRESSION_TYPE + ((uint32_t)'b'))
    //         #define LONGTAIL_BROTLI_TEXT_MAX_QUALITY_TYPE        (LONGTAIL_BROTLI_COMPRESSION_TYPE + ((uint32_t)'c'))
    //
    //         #define LONGTAIL_LZ4_DEFAULT_COMPRESSION_TYPE ((((uint32_t)'l') << 24) + (((uint32_t)'z') << 16) + (((uint32_t)'4') << 8) + ((uint32_t)'2'))
    //
    //         #define LONGTAIL_ZSTD_COMPRESSION_TYPE         ((((uint32_t)'z') << 24) + (((uint32_t)'t') << 16) + (((uint32_t)'d') << 8))
    //         #define LONGTAIL_ZSTD_MIN_COMPRESSION_TYPE     (LONGTAIL_ZSTD_COMPRESSION_TYPE + ((uint32_t)'1'))
    //         #define LONGTAIL_ZSTD_DEFAULT_COMPRESSION_TYPE (LONGTAIL_ZSTD_COMPRESSION_TYPE + ((uint32_t)'2'))
    //         #define LONGTAIL_ZSTD_MAX_COMPRESSION_TYPE     (LONGTAIL_ZSTD_COMPRESSION_TYPE + ((uint32_t)'3'))
    //         #define LONGTAIL_ZSTD_HIGH_COMPRESSION_TYPE    (LONGTAIL_ZSTD_COMPRESSION_TYPE + ((uint32_t)'4'))
    //         #define LONGTAIL_ZSTD_LOW_COMPRESSION_TYPE     (LONGTAIL_ZSTD_COMPRESSION_TYPE + ((uint32_t)'5'))
    //     ")
    //     .header_contents("hashes", "
    //         const uint32_t LONGTAIL_MEOW_HASH_TYPE = (((uint32_t)'m') << 24) + (((uint32_t)'e') << 16) + (((uint32_t)'o') << 8) + ((uint32_t)'w');
    //         const uint32_t LONGTAIL_BLAKE2_HASH_TYPE = (((uint32_t)'b') << 24) + (((uint32_t)'l') << 16) + (((uint32_t)'k') << 8) + ((uint32_t)'2');
    //         const uint32_t LONGTAIL_BLAKE3_HASH_TYPE = (((uint32_t)'b') << 24) + (((uint32_t)'l') << 16) + (((uint32_t)'k') << 8) + ((uint32_t)'3');
    //     ");

    let bindings = builder
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
