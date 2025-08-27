use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};
use zip::{result::ZipResult, ZipArchive};

const UPSTREAM_RELEASE_BASE_URL: &str =
    "https://github.com/DanEngelbrecht/longtail/releases/download";
const UPSTREAM_VERSION: &str = "v0.4.3";

#[cfg(target_os = "windows")]
const UPSTREAM_FILENAME: &str = "win32-x64.zip";
#[cfg(target_os = "windows")]
const SHA256: &str = "5c136d4f3ff1809df559da1c971a0fd3b0c6a91323473893b6bafb07ac8425c8";

#[cfg(target_os = "linux")]
const UPSTREAM_FILENAME: &str = "linux-x64.zip";
#[cfg(target_os = "linux")]
const SHA256: &str = "2d101731c3005fbbd20cf3a9676090a0f65656a5b1a0fcf3228ca4d92a240cd0";

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const UPSTREAM_FILENAME: &str = "darwin-x64.zip";
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const SHA256: &str = "956a2587554a4341ec9d76673e18211484e617260ca7322480da000faece548c";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const UPSTREAM_FILENAME: &str = "darwin-arm64.zip";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const SHA256: &str = "e5c94d6733149ebb4cb5f1efbb85dc8708a83fffb9067cc115a6355b2d66e257";

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
    apply_patches();
}

fn apply_patches() {
    let patches_dir = Path::new("patches");
    if !patches_dir.exists() {
        return; // No patches directory, nothing to do
    }
    
    // Check if patches have already been applied by looking for a marker file
    let patch_marker = Path::new("longtail/.longtail-rs-patches-applied");
    if patch_marker.exists() {
        return; // Patches already applied
    }
    
    // Collect and sort patch files
    let mut patch_files: Vec<_> = fs::read_dir(patches_dir)
        .expect("Failed to read patches directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()?.to_str()? == "patch" {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    patch_files.sort();
    
    // Apply each patch
    for patch_file in &patch_files {
        println!("cargo:warning=Applying patch: {}", patch_file.display());
        let output = Command::new("git")
            .args(["apply", "--directory=longtail"])
            .arg(patch_file)
            .output()
            .expect("Failed to execute git apply");
            
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!("Failed to apply patch {}: {}", patch_file.display(), stderr);
        }
    }
    
    // Create marker file to indicate patches have been applied
    fs::File::create(patch_marker)
        .expect("Failed to create patch marker file");
        
    println!("cargo:warning=Applied {} patches to longtail submodule", patch_files.len());
}

fn zip_extract(archive_file: &PathBuf, target_dir: &PathBuf) -> ZipResult<()> {
    let file = File::open(archive_file)?;
    let mut archive = ZipArchive::new(file)?;
    archive.extract(target_dir)
}

fn try_download(dst: PathBuf, url: &str, filename: &str, sha256: &str) -> PathBuf {
    println!("Downloading {} to {}", url, dst.display());
    let file = dst.join(filename);
    if file.exists() {
        dst
    } else {
        let response = reqwest::blocking::get(url).expect("Failed to download");
        if !response.status().is_success() {
            panic!("Failed to download {url}");
        }
        let out = response.bytes().expect("Failed to read response");

        let digest = Sha256::digest(&out);
        let digest = format!("{digest:x}");
        if digest != sha256 {
            panic!("SHA256 mismatch for {filename}");
        }
        let mut f = fs::File::create(&file).expect("cannot create zip file");
        f.write_all(&out).expect("cannot write zip file");

        zip_extract(&file, &dst).expect("cannot extract zip");

        dst
    }
}

fn setup_windows(dst: PathBuf) -> PathBuf {
    let dl_path = try_download(
        dst,
        &format!("{UPSTREAM_RELEASE_BASE_URL}/{UPSTREAM_VERSION}/{UPSTREAM_FILENAME}"),
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
        &format!("{UPSTREAM_RELEASE_BASE_URL}/{UPSTREAM_VERSION}/{UPSTREAM_FILENAME}"),
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

    // println!("cargo:rustc-link-search=longtail/build/linux_x64/longtail_static/
    // debug");
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
    // println!("cargo::rustc-env=CARGO_ENCODED_RUSTFLAGS=-C
    // relocation-model=dynamic-no-pic");

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
        // On windows, we need to add the include path for the archiveblockstore to
        // resolve relative includes of the longtail.h in lib files.
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

    bindings
        .write_to_file(dst.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

fn vendored() {
    setup_submodule();

    let profile = env::var("PROFILE").unwrap();
    // let target = env::var("TARGET").unwrap();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    // let windows = target.contains("windows");
    let dst = PathBuf::from(env::var("OUT_DIR").unwrap());

    let rustflags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();

    let mut cfg = cc::Build::new();
    cfg.warnings(false);

    // Disable warnings for mismatched declarations, because longtail redefines
    // free's signature
    #[cfg(target_os = "linux")]
    cfg.flag("-Wno-builtin-declaration-mismatch");

    // Setup default build flags
    #[cfg(target_env = "msvc")]
    cfg.static_crt(true);
    if arch == "x86_64" {
        #[cfg(target_env = "msvc")]
        cfg.flag("/arch:AVX");
        #[cfg(not(target_env = "msvc"))]
        cfg.flag("-std=gnu99")
            .flag("-g")
            .flag("-pthread")
            .flag("-maes")
            .flag("-mssse3")
            .flag("-msse4.1");
    } else if arch == "aarch64" {
        cfg.flag("-march=armv8-a+crc+simd"); // untested, probably wrong
    }

    if rustflags.contains("sanitizer=address") {
        println!("cargo:warning=Building with address sanitizer");
        cfg.flag("-fsanitize=address");
    }

    // MSVC doesn't support this asm?
    #[cfg(not(target_env = "msvc"))]
    cfg.file("longtail/lib/zstd/ext/decompress/huf_decompress_amd64.S");

    // Add the include directory to the search path, and set the output directory
    cfg.out_dir(dst.join("build"))
        .include("longtail/src/")
        .include("longtail/lib/");

    // This should match the build order of the upstream build fairly well.
    // See:
    //  https://github.com/DanEngelbrecht/longtail/blob/v0.4.2/all_sources.sh#L63-L70
    //  https://github.com/DanEngelbrecht/longtail/blob/v0.4.2/static_lib/build.sh#L69-L96

    // THIRDPARTY_SRC
    add_c_files(&mut cfg, "longtail/src/ext");
    cfg.file("longtail/lib/blake3/ext/blake3.c");
    cfg.file("longtail/lib/blake3/ext/blake3_dispatch.c");
    cfg.file("longtail/lib/blake3/ext/blake3_portable.c");
    add_c_files(&mut cfg, "longtail/lib/lz4/ext");
    add_c_files(&mut cfg, "longtail/lib/brotli/ext/common");
    add_c_files(&mut cfg, "longtail/lib/brotli/ext/dec");
    add_c_files(&mut cfg, "longtail/lib/brotli/ext/enc");
    add_c_files(&mut cfg, "longtail/lib/zstd/ext/common");
    add_c_files(&mut cfg, "longtail/lib/zstd/ext/compress");
    add_c_files(&mut cfg, "longtail/lib/zstd/ext/decompress");

    // SRC
    add_c_files(&mut cfg, "longtail/src");
    add_c_files(&mut cfg, "longtail/lib/filestorage");
    add_c_files(&mut cfg, "longtail/lib/archiveblockstore");
    add_c_files(&mut cfg, "longtail/lib/atomiccancel");
    add_c_files(&mut cfg, "longtail/lib/blockstorestorage");
    add_c_files(&mut cfg, "longtail/lib/compressblockstore");
    add_c_files(&mut cfg, "longtail/lib/concurrentchunkwrite");
    add_c_files(&mut cfg, "longtail/lib/cacheblockstore");
    add_c_files(&mut cfg, "longtail/lib/shareblockstore");
    add_c_files(&mut cfg, "longtail/lib");
    add_c_files(&mut cfg, "longtail/lib/fsblockstore");
    add_c_files(&mut cfg, "longtail/lib/hpcdcchunker");
    add_c_files(&mut cfg, "longtail/lib/lrublockstore");
    add_c_files(&mut cfg, "longtail/lib/memstorage");
    add_c_files(&mut cfg, "longtail/lib/memtracer");
    add_c_files(&mut cfg, "longtail/lib/ratelimitedprogress");
    add_c_files(&mut cfg, "longtail/lib/compressionregistry");
    add_c_files(&mut cfg, "longtail/lib/hashregistry");
    add_c_files(&mut cfg, "longtail/lib/bikeshed");
    add_c_files(&mut cfg, "longtail/lib/blake2");
    add_c_files(&mut cfg, "longtail/lib/blake3");
    add_c_files(&mut cfg, "longtail/lib/meowhash");
    add_c_files(&mut cfg, "longtail/lib/lz4");
    add_c_files(&mut cfg, "longtail/lib/brotli");
    add_c_files(&mut cfg, "longtail/lib/zstd");

    if arch == "x86_64" {
        // THIRDPARTY_SSE
        add_c_files(&mut cfg, "longtail/lib/blake2/ext");
        cfg.file("longtail/lib/blake3/ext/blake3_sse2.c");
        cfg.file("longtail/lib/blake3/ext/blake3_sse41.c");

        // THIRDPARTY_SSE42
        // THIRDPARTY_SRC_AVX2
        let mut cfg_avx2 = cc::Build::new();
        #[cfg(target_env = "msvc")]
        cfg_avx2.flag("/arch:AVX2");
        #[cfg(not(target_env = "msvc"))]
        cfg_avx2.flag("-msse4.2").flag("-mavx2");

        cfg_avx2
            .warnings(false)
            .out_dir(dst.join("build"))
            .include("longtail/src/")
            .include("longtail/lib/");
        cfg_avx2.file("longtail/lib/blake3/ext/blake3_avx2.c");
        cfg_avx2.compile("longtail-cc-avx2");

        // THIRDPARTY_SRC_AVX512
        let mut cfg_avx512 = cc::Build::new();
        #[cfg(target_env = "msvc")]
        cfg_avx512.flag("/arch:AVX512");
        #[cfg(not(target_env = "msvc"))]
        cfg_avx512
            .flag("-mavx512vl")
            .flag("-mavx512f")
            .flag("-mvaes")
            .flag("-fno-asynchronous-unwind-tables");

        cfg_avx512
            .warnings(false)
            .out_dir(dst.join("build"))
            .include("longtail/src/")
            .include("longtail/lib/");
        cfg_avx512.file("longtail/lib/blake3/ext/blake3_avx512.c");
        cfg_avx512.compile("longtail-cc-avx512");
    } else if arch == "aarch64" {
        // THIRDPARTY_SRC_NEON
        cfg.file("longtail/lib/blake3/ext/blake3_neon.c");
    }

    if profile == "debug" {
        cfg.define("LONGTAIL_ASSERTS", None)
            .define("BIKESHED_ASSERTS", None);
    }

    match cfg.try_compile("longtail-cc") {
        Ok(_) => {}
        Err(e) => {
            println!("cargo:warning=Failed to compile");
            println!("cargo:warning={e:?}");
        }
    }

    let longtail_header_path = PathBuf::from("longtail/src/longtail.h");
    let longtail_header_path_str = longtail_header_path
        .to_str()
        .expect("Path is not a valid string");
    let libdir_path = PathBuf::from("longtail/lib");

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

    #[rustfmt::skip]
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
    bindings
        .write_to_file(dst.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

fn add_c_files(build: &mut cc::Build, path: impl AsRef<Path>) {
    let path = path.as_ref();
    if !path.exists() {
        let d = path.display();
        panic!("Path {d} does not exist");
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
    // by default, use the submodule version
    if cfg!(feature = "vendored") {
        vendored()
    } else {
        upstream_dist()
    }
}
