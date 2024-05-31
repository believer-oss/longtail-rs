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
    println!("cargo:rustc-link-lib=static=longtail_linux_x64");

    libdir_path
}

fn main() {
    #[cfg(target_os = "windows")]
    let libdir_path = setup_windows();

    #[cfg(target_os = "linux")]
    let libdir_path = setup_linux();

    // This is the path to the `c` headers file.
    let headers_path = libdir_path.join("longtail.h");
    let headers_path_str = headers_path.to_str().expect("Path is not a valid string");

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let bindings = bindgen::Builder::default()
        // The input header we would like to generate
        // bindings for.
        .header(headers_path_str)
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
