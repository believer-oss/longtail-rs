use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs, io};

const UPSTREAM_RELEASE_BASE_URL: &str =
    "https://github.com/DanEngelbrecht/longtail/releases/download";
const UPSTREAM_VERSION: &str = "v0.4.2";

#[cfg(target_os = "windows")]
const UPSTREAM_FILENAME: &str = "win32-x64.zip";
#[cfg(target_os = "windows")]
const SHA256: &str = "775d7a890f9d9ed6f5912e46e0035ac5ba796cbe4178ac720c5eddd97f91a8fb";

#[cfg(target_os = "linux")]
const UPSTREAM_FILENAME: &str = "linux-x64.zip";
#[cfg(target_os = "linux")]
const SHA256: &str = "f915dafe38a7efae92b3f6ae2c11f1b5ff94efddf7fc5f3df14a6f904cc386e6";

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

fn setup_submodule() {
    if !Path::new("longtail/src").exists() {
        Command::new("git")
            .args(["submodule", "update", "--init", "longtail"])
            .status()
            .expect("Failed to update submodules");
    }
}

fn try_download(dst: PathBuf, url: &str, filename: &str, sha256: &str) -> PathBuf {
    println!("Downloading {} to {}", url, dst.display());
    let file = dst.join(filename);
    if file.exists() {
        dst
    } else {
        let response = reqwest::blocking::get(url).expect("Failed to download");
        if !response.status().is_success() {
            panic!("Failed to download {}", url);
        }
        let out = response.bytes().expect("Failed to read response");

        let digest = Sha256::digest(&out);
        let digest = format!("{:x}", digest);
        if digest != sha256 {
            panic!("SHA256 mismatch for {}", filename);
        }
        let mut f = fs::File::create(&file).expect("cannot create zip file");
        f.write_all(&out).expect("cannot write zip file");

        zip_extensions::zip_extract(&file, &dst).expect("cannot extract zip");

        dst
    }
}

fn setup_windows(dst: PathBuf) -> PathBuf {
    let dl_path = try_download(
        dst,
        &format!(
            "{}/{}/{}",
            UPSTREAM_RELEASE_BASE_URL, UPSTREAM_VERSION, UPSTREAM_FILENAME
        ),
        UPSTREAM_FILENAME,
        SHA256,
    );

    // This is the directory where the `c` library is located.
    let libdir_path = dl_path
        .join("dist-win32-x64/include/src")
        // Canonicalize the path as `rustc-link-search` requires an absolute
        // path.
        .canonicalize()
        .expect("cannot canonicalize path");

    // Tell cargo to look for shared libraries in the specified directory
    println!(
        "cargo:rustc-link-search={}\\dist-win32-x64",
        dl_path.display()
    );

    // Tell cargo to tell rustc to link the system bzip2
    // shared library.
    println!("cargo:rustc-link-lib=static=longtail_win32_x64");

    libdir_path
}

fn setup_linux(dst: PathBuf) -> PathBuf {
    let dl_path = try_download(
        dst,
        &format!(
            "{}/{}/{}",
            UPSTREAM_RELEASE_BASE_URL, UPSTREAM_VERSION, UPSTREAM_FILENAME
        ),
        UPSTREAM_FILENAME,
        SHA256,
    );

    // This is the directory where the `c` library is located.
    let libdir_path = dl_path
        .join("dist-linux-x64/include/src")
        // Canonicalize the path as `rustc-link-search` requires an absolute
        // path.
        .canonicalize()
        .expect("cannot canonicalize path");

    // Tell cargo to look for shared libraries in the specified directory
    println!(
        "cargo:rustc-link-search={}/dist-linux-x64",
        dl_path.display()
    );

    // Tell cargo to tell rustc to link the system bzip2
    // shared library.
    println!("cargo:rustc-link-lib=static=longtail_linux_x64_debug");

    libdir_path
}

#[allow(dead_code)]
fn setup_linux_debug(_dst: PathBuf) -> PathBuf {
    if fs::metadata("longtail/build/linux_x64/longtail_static/debug").is_err() {
        panic!("Please build the longtail library first");
    }

    let libdir_path = PathBuf::from("longtail/src")
        // Canonicalize the path as `rustc-link-search` requires an absolute
        // path.
        .canonicalize()
        .expect("cannot canonicalize path");

    let search_path = libdir_path
        .join("../build/linux_x64/longtail_static/debug")
        .canonicalize()
        .expect("cannot canonicalize path");

    // println!("cargo:rustc-link-search=longtail/build/linux_x64/longtail_static/debug");
    println!(
        "cargo:rustc-link-search={}",
        search_path
            .into_os_string()
            .into_string()
            .expect("cannot convert path")
    );
    println!("cargo:rustc-link-lib=static=longtail_static");
    // Neither of these worked, ended up making a .cargo/config.toml file
    // println!("cargo:rustc-codegen=relocation-model=dynamic-no-pic");
    // println!("cargo::rustc-env=CARGO_ENCODED_RUSTFLAGS=-C relocation-model=dynamic-no-pic");

    libdir_path
}

fn upstream_dist() {
    let target = env::var("TARGET").unwrap();
    let windows = target.contains("windows");
    let dst = PathBuf::from(env::var("OUT_DIR").unwrap());
    let upstream = dst.join("upstream");
    if !upstream.exists() {
        fs::create_dir_all(&upstream).expect("cannot create upstream directory");
    }

    let libdir_path = match windows {
        true => setup_windows(upstream.clone()),
        false => setup_linux(upstream.clone()),
    };

    // This is the path to the `c` headers file.
    let longtail_header_path = libdir_path.join("longtail.h");
    let longtail_header_path_str = longtail_header_path
        .to_str()
        .expect("Path is not a valid string");

    let builder = if windows {
        // On windows, we need to add the include path for the archiveblockstore to resolve
        // relative includes of the longtail.h in lib files.
        bindgen::Builder::default()
            .header(longtail_header_path_str)
            .clang_arg(format!(
                "-I{}",
                upstream
                    .join("dist-win32-x64/include/lib/archiveblockstore/")
                    .display()
            ))
    } else {
        bindgen::Builder::default().header(longtail_header_path_str)
    };

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

fn vendored() {
    setup_submodule();

    let profile = env::var("PROFILE").unwrap();
    let target = env::var("TARGET").unwrap();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let windows = target.contains("windows");
    let dst = PathBuf::from(env::var("OUT_DIR").unwrap());
    let include = dst.join("include");
    let mut cfg = cc::Build::new();
    fs::create_dir_all(&include).unwrap();

    cp_r_include("longtail/src", include.join("src"));
    cp_r_include("longtail/lib", include.join("lib"));

    cfg.include(&include).out_dir(dst.join("build"));
    add_c_files(&mut cfg, "longtail/src");
    add_c_files(&mut cfg, "longtail/src/ext");
    add_c_files(&mut cfg, "longtail/lib");

    for (module, _header) in EXTRA_HEADERS.iter() {
        add_c_files(&mut cfg, &format!("longtail/lib/{}", module));
    }

    add_c_files(&mut cfg, "longtail/lib/brotli/ext/common");
    add_c_files(&mut cfg, "longtail/lib/brotli/ext/dec");
    add_c_files(&mut cfg, "longtail/lib/brotli/ext/enc");

    add_c_files(&mut cfg, "longtail/lib/lz4/ext");

    add_c_files(&mut cfg, "longtail/lib/zstd/ext/common");
    add_c_files(&mut cfg, "longtail/lib/zstd/ext/compress");
    add_c_files(&mut cfg, "longtail/lib/zstd/ext/decompress");

    cfg.file("longtail/lib/blake2/ext/blake2s.c");
    cfg.file("longtail/lib/blake3/ext/blake3.c");
    cfg.file("longtail/lib/blake3/ext/blake3_dispatch.c");
    cfg.file("longtail/lib/blake3/ext/blake3_portable.c");

    if arch == "x86_64" {
        cfg.file("longtail/lib/blake3/ext/blake3_sse2.c");
        cfg.file("longtail/lib/blake3/ext/blake3_sse41.c");
        cfg.file("longtail/lib/blake3/ext/blake3_avx2.c");
        cfg.file("longtail/lib/blake3/ext/blake3_avx512.c");
    } else if arch == "aarch64" {
        cfg.file("longtail/lib/blake3/ext/blake3_neon.c");
    }

    if windows {
        if profile == "release" {
            cfg.flag("/O3");
        } else {
            cfg.flag("/DLONGTAIL_ASSERTS=1")
                .flag("/DBIKESHED_ASSERTS=1");
        }
        cfg.flag("/arch:AVX2");
        cfg.compile("longtail");
    } else {
        cfg.file("longtail/lib/zstd/ext/decompress/huf_decompress_amd64.S");
        if profile == "release" {
            cfg.flag("-O3");
        } else {
            cfg.flag("-DLONGTAIL_ASSERTS=1")
                .flag("-DBIKESHED_ASSERTS=1");
        }
        cfg.flag("-std=gnu99")
            .flag("-g")
            .flag("-pthread")
            .flag("-msse4.2")
            .flag("-mavx2")
            .flag("-mavx512vl")
            .flag("-mavx512f")
            .flag("-mvaes")
            .flag("-maes")
            .flag("-fno-asynchronous-unwind-tables")
            .compile("longtail");
    }

    let longtail_header_path = include.join("src").join("longtail.h");
    let longtail_header_path_str = longtail_header_path
        .to_str()
        .expect("Path is not a valid string");
    let libdir_path = include.join("lib");

    let builder = bindgen::Builder::default().header(longtail_header_path_str);

    let headers = EXTRA_HEADERS
        .iter()
        .map(|(module, header)| {
            let header_path = libdir_path.join(module).join(header);
            header_path
                .to_str()
                .expect("Path is not a valid string")
                .to_string()
        })
        .collect::<Vec<_>>();

    let builder = headers
        .iter()
        .fold(builder, |builder, header| builder.header(header));

    // These are implemented in rust currently because the macros are not supported in bindgen
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

fn cp_r_include(from: impl AsRef<Path>, to: impl AsRef<Path>) {
    for e in from.as_ref().read_dir().unwrap() {
        let e = e.unwrap();
        let from = e.path();
        let to = to.as_ref().join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            fs::create_dir_all(&to).unwrap();
            cp_r_include(&from, &to);
        } else {
            if !e.file_name().to_string_lossy().ends_with(".h") {
                continue;
            }
            println!("{} => {}", from.display(), to.display());
            fs::copy(&from, &to).unwrap();
        }
    }
}

fn add_c_files(build: &mut cc::Build, path: impl AsRef<Path>) {
    let path = path.as_ref();
    if !path.exists() {
        panic!("Path {} does not exist", path.display());
    }
    // sort the C files to ensure a deterministic build for reproducible builds
    let dir = path.read_dir().unwrap();
    let mut paths = dir.collect::<io::Result<Vec<_>>>().unwrap();
    paths.sort_by_key(|e| e.path());

    for e in paths {
        let path = e.path();
        if e.file_type().unwrap().is_dir() {
            // skip for now
        } else if path.extension().and_then(|s| s.to_str()) == Some("c") {
            build.file(&path);
        }
    }
}

fn main() {
    if cfg!(feature = "vendored") {
        vendored()
    } else {
        upstream_dist()
    }
}
