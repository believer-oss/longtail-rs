use std::env;
use std::path::PathBuf;

fn main() {
    // This is the directory where the `c` library is located.
    let libdir_path = PathBuf::from("longtail/src")
        // Canonicalize the path as `rustc-link-search` requires an absolute
        // path.
        .canonicalize()
        .expect("cannot canonicalize path");

    println!("{:?}", libdir_path);

    // This is the path to the `c` headers file.
    let headers_path = libdir_path.join("longtail.h");
    let headers_path_str = headers_path.to_str().expect("Path is not a valid string");

    // This is the path to the intermediate object file for our library.
    let obj_path = libdir_path.join("longtail.o");
    // This is the path to the static library file.
    let lib_path = libdir_path.join("liblongtail.a");

    // Tell cargo to look for shared libraries in the specified directory
    println!("cargo:rustc-link-search={}", libdir_path.to_str().unwrap());

    // Tell cargo to tell rustc to link the system bzip2
    // shared library.
    println!("cargo:rustc-link-lib=longtail");

    // Run `clang` to compile the `longtail.c` file into a `longtail.o` object file.
    // Unwrap if it is not possible to spawn the process.

    let mut cmd = std::process::Command::new("clang");
    cmd.arg("-c")
        .arg("-o")
        .arg(&obj_path)
        .arg(libdir_path.join("longtail.c"));

    println!("{:?}", cmd);

    let output = cmd.output().expect("could not spawn `clang`");
    if !output.status.success()
    {
        // print stderr
        println!("{}", String::from_utf8_lossy(&output.stderr));

        // Panic if the command was not successful.
        panic!("could not compile object file");
    }

    // Run `ar` to generate the `liblongtail.a` file from the `longtail.o` file.
    // Unwrap if it is not possible to spawn the process.
    if !std::process::Command::new("ar")
        .arg("rcs")
        .arg(lib_path)
        .arg(obj_path)
        .output()
        .expect("could not spawn `ar`")
        .status
        .success()
    {
        // Panic if the command was not successful.
        panic!("could not emit library file");
    }

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