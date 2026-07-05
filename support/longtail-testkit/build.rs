//! Build script for longtail-testkit.
//!
//! When the `differential` feature is active, compile the committed HPCDC
//! discriminator shim (`shim/hpcdc_discriminator_shim.c`) with the host C
//! compiler via `cc`. The shim exposes the file-static
//! `HPCDCDiscriminatorFromAvg` expression under an exported symbol so the
//! discriminator differential can compare Rust against the literal C expression
//! (rust-port-3.md Task 6). Without `differential` this build script does
//! nothing, so the pure lane needs no C toolchain.
//!
//! This adds a **host C compiler requirement to the differential lane only**
//! (present on GitHub ubuntu/windows runners). Stage 1 designed the differential
//! lane as "prebuilt lib, no C toolchain"; the Task 6 fallback (golden `d` table
//! + boundary-table equality) covers environments without a C compiler.

fn main() {
    // `cc` is an optional build-dependency, pulled in only by the `differential`
    // feature. Cargo compiles this build script with the package's features as
    // `cfg(feature = ...)`, so guarding the `cc` usage means the crate builds
    // fine without a C toolchain when `differential` is off.
    #[cfg(feature = "differential")]
    {
        let mut build = cc::Build::new();
        build.file("shim/hpcdc_discriminator_shim.c");
        // Disable FP contraction so the multiply-add can't be fused into an FMA
        // and diverge (moot on x86-64, real on ARM). MSVC rejects the flag and
        // does not contract by default, so add it only when the compiler accepts.
        build.flag_if_supported("-ffp-contract=off");
        build.compile("hpcdc_discriminator_shim");
        println!("cargo:rerun-if-changed=shim/hpcdc_discriminator_shim.c");
    }
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_DIFFERENTIAL");
}
