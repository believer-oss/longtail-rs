//! The C implementation's spawnable downsync driver (differential feature).
//!
//! A single `longtail_ffi::downsync` per process, `enable_file_mapping = false`
//! (matching golongtail's default and how the fixtures were chunked). Kept a
//! separate process so its RUSAGE (peak RSS, CPU) is measured in isolation by
//! the e2e harness, and so the `get_existing_store_index_sync` missed-wake race
//! is recovered by the harness killing + re-spawning with fresh C state (the
//! process-granularity form of testkit's in-thread watchdog).
//!
//! Flags (a subset mirroring the CLI): `--storage-uri`, `--source-path`,
//! `--target-path`, `--worker-count`, `--cache-path`,
//! `--version-local-store-index-path`.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut storage_uri = String::new();
    let mut source_path = String::new();
    let mut target_path = String::new();
    let mut worker_count: usize = 8;
    let mut cache_path: Option<PathBuf> = None;
    let mut vlsi: Option<Vec<String>> = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--storage-uri" => storage_uri = args.next().unwrap_or_default(),
            "--source-path" => source_path = args.next().unwrap_or_default(),
            "--target-path" => target_path = args.next().unwrap_or_default(),
            "--worker-count" => {
                worker_count = args.next().and_then(|v| v.parse().ok()).unwrap_or(8)
            }
            "--cache-path" => cache_path = args.next().map(PathBuf::from),
            "--version-local-store-index-path" => {
                vlsi = args
                    .next()
                    .map(|v| v.split('|').map(str::to_string).collect())
            }
            other => {
                eprintln!("ffi-driver: unknown arg {other}");
                return ExitCode::from(2);
            }
        }
    }

    let res = longtail_ffi::downsync(
        worker_count,
        &storage_uri,
        None,
        None,
        &[source_path],
        &target_path,
        "",
        cache_path.as_deref(),
        true,  // retain_permissions
        false, // validate
        vlsi,
        None,
        None,
        true,  // scan_target
        false, // cache_target_index
        false, // enable_file_mapping — canonical streaming target scan
        false, // use_legacy_write — ChangeVersion2
        None,
    );

    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ffi-driver downsync failed: {e:?}");
            ExitCode::FAILURE
        }
    }
}
