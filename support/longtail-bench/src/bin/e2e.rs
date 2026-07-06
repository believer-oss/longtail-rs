//! Stage 6 end-to-end downsync harness (differential feature, Linux-only).
//!
//! Measures wall time, user+sys CPU, and peak RSS for three implementations —
//! (1) the Rust `longtail` CLI, (2) the C library via the `ffi-driver` bin,
//! (3) the pinned golongtail binary — across cold / warm-cache / incremental
//! scenarios over a synthetic ~1 GiB dataset.
//!
//! ## Fairness rules (all binding, from the work order)
//!
//! - **Every timed run is a fresh spawned child process** for all three
//!   implementations — never in-process. `RUSAGE_SELF` is a process-lifetime
//!   cumulative max and `RUSAGE_CHILDREN.ru_maxrss` is a running max across ALL
//!   reaped children, so both cross-contaminate; instead we `libc::wait4` the
//!   exact child pid, which returns *that child's* rusage (`ru_maxrss` in KiB on
//!   Linux, `ru_utime + ru_stime` for CPU).
//! - **All drivers are --release** (the harness + CLI + ffi-driver are built in
//!   release; golongtail is a release binary).
//! - **Worker parity**: every impl is pinned to the same worker count per run;
//!   the cold scenario additionally sweeps {2, 4, 8, NumCPU} (ffi needs ≥2 — the
//!   single-worker bikeshed deadlock).
//! - **Page-cache policy**: the store is pre-warmed once (all its files read into
//!   the OS page cache) and caches are never dropped, so every impl reads a warm
//!   store — isolating implementation differences from cold disk latency.
//! - **Store-index resolution parity**: no `--version-local-store-index-path` is
//!   passed, so all three resolve blocks via the store's canonical `store.lsi`
//!   (the fs optimistic path) — uniform, and cheap because upsync wrote it.
//! - **Per-run hard timeout + retry**: the ffi `get_existing_store_index_sync`
//!   missed-wake race can hang a run; each timed run has a hard timeout (default
//!   5× a measured baseline), a timed-out run is killed, recorded as such, and
//!   retried with a fresh process (fresh C state) — never averaged in.
//!
//! Config via env (all optional):
//!   LONGTAIL_BENCH_DATA_SIZE_MB (1024) · LONGTAIL_BENCH_ITERS (5)
//!   LONGTAIL_BENCH_COLD_WORKERS ("2,4,8,0"; 0 = NumCPU)
//!   LONGTAIL_BENCH_MAIN_WORKERS (8) · LONGTAIL_BENCH_SCENARIOS
//!   ("cold,warm,incremental") · LONGTAIL_BENCH_TIMEOUT_MULT (5)
//!   LONGTAIL_BENCH_SKIP_GO (unset) · LONGTAIL_BENCH_SKIP_FFI (unset)

use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use longtail_bench::{ChurnSummary, bench_root, dataset_plan, write_churned_v2, write_dataset};
use longtail_testkit::paths::golongtail_binary;

// ---------------------------------------------------------------------------
// Child measurement (wait4 per pid — isolated CPU + peak RSS)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Timed {
    wall: Duration,
    cpu: Duration,
    maxrss_kib: i64,
    ok: bool,
    timed_out: bool,
}

fn tv_to_dur(tv: libc::timeval) -> Duration {
    Duration::from_secs(tv.tv_sec as u64) + Duration::from_micros(tv.tv_usec as u64)
}

/// Spawn `cmd` (stdio nulled), wait4 the exact child pid with a hard timeout,
/// and return its isolated rusage. On timeout the child is SIGKILLed and reaped.
fn run_timed(mut cmd: Command, timeout: Duration) -> Timed {
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    let start = Instant::now();
    let child = cmd.spawn().expect("spawn child");
    let pid = child.id() as libc::pid_t;
    // We reap via wait4 on the pid ourselves; prevent std's Child from also
    // touching it. (Child::drop does not reap on unix, but forgetting is belt
    // and braces and avoids any fd bookkeeping surprises.)
    std::mem::forget(child);

    let poll = Duration::from_millis(20);
    loop {
        let mut status: libc::c_int = 0;
        let mut ru = MaybeUninit::<libc::rusage>::zeroed();
        let r = unsafe { libc::wait4(pid, &mut status, libc::WNOHANG, ru.as_mut_ptr()) };
        if r == pid {
            let ru = unsafe { ru.assume_init() };
            return Timed {
                wall: start.elapsed(),
                cpu: tv_to_dur(ru.ru_utime) + tv_to_dur(ru.ru_stime),
                maxrss_kib: ru.ru_maxrss,
                ok: status == 0,
                timed_out: false,
            };
        }
        if r == -1 {
            return Timed {
                wall: start.elapsed(),
                cpu: Duration::ZERO,
                maxrss_kib: 0,
                ok: false,
                timed_out: false,
            };
        }
        if start.elapsed() >= timeout {
            unsafe { libc::kill(pid, libc::SIGKILL) };
            let mut st: libc::c_int = 0;
            let mut ru2 = MaybeUninit::<libc::rusage>::zeroed();
            unsafe { libc::wait4(pid, &mut st, 0, ru2.as_mut_ptr()) };
            let ru2 = unsafe { ru2.assume_init() };
            return Timed {
                wall: start.elapsed(),
                cpu: tv_to_dur(ru2.ru_utime) + tv_to_dur(ru2.ru_stime),
                maxrss_kib: ru2.ru_maxrss,
                ok: false,
                timed_out: true,
            };
        }
        std::thread::sleep(poll);
    }
}

/// Run one timed downsync with the per-run watchdog: retry a timed-out run up to
/// `MAX_ATTEMPTS` with fresh processes (fresh C state). Returns the first
/// successful `Timed`, or the last timed-out/failed one.
const MAX_ATTEMPTS: u32 = 3;
fn run_watched(make: &dyn Fn() -> Command, timeout: Duration, label: &str) -> Timed {
    let mut last = Timed {
        wall: Duration::ZERO,
        cpu: Duration::ZERO,
        maxrss_kib: 0,
        ok: false,
        timed_out: false,
    };
    for attempt in 1..=MAX_ATTEMPTS {
        let t = run_timed(make(), timeout);
        if t.ok {
            return t;
        }
        if t.timed_out {
            eprintln!(
                "  watchdog: {label} attempt {attempt}/{MAX_ATTEMPTS} timed out after {:.0}s; retrying with fresh process",
                timeout.as_secs_f64()
            );
        } else {
            eprintln!("  {label} attempt {attempt}/{MAX_ATTEMPTS} exited non-zero");
        }
        last = t;
    }
    last
}

// ---------------------------------------------------------------------------
// Layout / setup
// ---------------------------------------------------------------------------

struct Paths {
    rust_cli: PathBuf,
    ffi_driver: PathBuf,
    go: Option<PathBuf>,
    data_v1: PathBuf,
    data_v2: PathBuf,
    store: PathBuf,
    v1_lvi: PathBuf,
    v2_lvi: PathBuf,
    scratch: PathBuf,
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn read_all_files(dir: &Path) {
    // Pre-warm: read every file in `dir` into the OS page cache.
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                read_all_files(&p);
            } else if p.is_file() {
                let _ = std::fs::read(&p);
            }
        }
    }
}

fn dir_nonempty(p: &Path) -> bool {
    std::fs::read_dir(p)
        .map(|mut r| r.next().is_some())
        .unwrap_or(false)
}

fn rm(p: &Path) {
    let _ = std::fs::remove_dir_all(p);
}

/// Upsync `src` into `store` via golongtail, writing `lvi`. worker-count raised
/// (byte-determinism is irrelevant to the bench; 1 GiB at one worker is slow).
fn go_upsync(go: &Path, src: &Path, lvi: &Path, store: &Path, store_lsi: &Path) {
    let status = Command::new(go)
        .args([
            "upsync",
            "--source-path",
            &src.to_string_lossy(),
            "--target-path",
            &lvi.to_string_lossy(),
            "--version-local-store-index-path",
            &store_lsi.to_string_lossy(),
            "--storage-uri",
            &store.to_string_lossy(),
            "--hash-algorithm",
            "blake3",
            "--compression-algorithm",
            "zstd",
            "--target-chunk-size",
            "32768",
            "--worker-count",
            "16",
            "--log-level",
            "error",
        ])
        .status()
        .expect("spawn golongtail upsync");
    assert!(
        status.success(),
        "golongtail upsync failed for {}",
        src.display()
    );
}

fn setup() -> Paths {
    let root = bench_root();
    std::fs::create_dir_all(&root).expect("create target/bench");

    let exe_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let rust_cli = exe_dir.join("longtail");
    let ffi_driver = exe_dir.join("ffi-driver");
    assert!(
        rust_cli.is_file(),
        "Rust CLI not found at {} — build it: cargo build --release -p longtail-cli",
        rust_cli.display()
    );
    assert!(
        ffi_driver.is_file(),
        "ffi-driver not found at {} — build it: cargo build --release -p longtail-bench --features differential --bin ffi-driver",
        ffi_driver.display()
    );
    let go = if std::env::var("LONGTAIL_BENCH_SKIP_GO").is_ok() {
        None
    } else {
        let g = golongtail_binary();
        if g.is_none() {
            eprintln!(
                "golongtail binary not cached (run: cargo run -p xtask -- fetch-golongtail); the go leg will be skipped"
            );
        }
        g
    };

    let data_v1 = root.join("data/v1");
    let data_v2 = root.join("data/v2");
    let store = root.join("store");
    let v1_lvi = store.join("v1.lvi");
    let v2_lvi = store.join("v2.lvi");
    let scratch = root.join("scratch");
    rm(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();

    let size_mb = env_usize("LONGTAIL_BENCH_DATA_SIZE_MB", 1024);
    let total = size_mb * 1024 * 1024;
    let plan = dataset_plan(total);

    if !dir_nonempty(&data_v1) {
        eprintln!(
            "generating v1 dataset (~{size_mb} MiB, {} files) into {}",
            plan.len(),
            data_v1.display()
        );
        std::fs::create_dir_all(&data_v1).unwrap();
        let n = write_dataset(&data_v1, &plan).expect("write v1");
        eprintln!("  wrote {:.1} MiB", n as f64 / (1024.0 * 1024.0));
    }
    if !dir_nonempty(&data_v2) {
        eprintln!("generating v2 (churned) into {}", data_v2.display());
        std::fs::create_dir_all(&data_v2).unwrap();
        let s: ChurnSummary = write_churned_v2(&data_v1, &data_v2, &plan).expect("write v2");
        eprintln!("  churn: {s:?}");
    }

    // Upsync both versions into a single fs store via golongtail (the shared
    // source of truth every impl downloads from). Requires golongtail.
    if !v1_lvi.is_file() || !v2_lvi.is_file() {
        let go_bin = go.clone().expect("golongtail required to build the bench store (upsync); cache it with `xtask fetch-golongtail` or set LONGTAIL_BENCH_SKIP_GO only after the store exists");
        std::fs::create_dir_all(&store).unwrap();
        if !v1_lvi.is_file() {
            eprintln!("upsync v1 -> store (golongtail)");
            go_upsync(
                &go_bin,
                &data_v1,
                &v1_lvi,
                &store,
                &store.join("v1-store.lsi"),
            );
        }
        if !v2_lvi.is_file() {
            eprintln!("upsync v2 -> store (golongtail)");
            go_upsync(
                &go_bin,
                &data_v2,
                &v2_lvi,
                &store,
                &store.join("v2-store.lsi"),
            );
        }
    }

    Paths {
        rust_cli,
        ffi_driver,
        go,
        data_v1,
        data_v2,
        store,
        v1_lvi,
        v2_lvi,
        scratch,
    }
}

// ---------------------------------------------------------------------------
// Command builders per implementation
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Impl {
    Rust,
    Ffi,
    Go,
}
impl Impl {
    fn name(self) -> &'static str {
        match self {
            Impl::Rust => "rust",
            Impl::Ffi => "ffi",
            Impl::Go => "go",
        }
    }
}

fn build_cmd(
    p: &Paths,
    imp: Impl,
    source_lvi: &Path,
    target: &Path,
    workers: usize,
    cache: Option<&Path>,
) -> Command {
    let w = workers.to_string();
    match imp {
        Impl::Rust => {
            let mut c = Command::new(&p.rust_cli);
            // Pin BOTH planes to N: --worker-count is the CPU rayon pool
            // (chunk/hash), --remote-worker-count is the store's block-I/O
            // concurrency (downsync.rs:136). The download is I/O-bound, so
            // sweeping only --worker-count would leave the I/O plane at its
            // default — pin both so the worker sweep actually varies download
            // concurrency, matching golongtail's remote workers.
            c.args([
                "downsync",
                "--worker-count",
                &w,
                "--remote-worker-count",
                &w,
                "--storage-uri",
                &p.store.to_string_lossy(),
                "--source-path",
                &source_lvi.to_string_lossy(),
                "--target-path",
                &target.to_string_lossy(),
                "--no-cache-target-index",
                "--log-level",
                "error",
            ]);
            if let Some(c2) = cache {
                c.args(["--cache-path", &c2.to_string_lossy()]);
            }
            c
        }
        Impl::Ffi => {
            let mut c = Command::new(&p.ffi_driver);
            c.args([
                "--storage-uri",
                &p.store.to_string_lossy(),
                "--source-path",
                &source_lvi.to_string_lossy(),
                "--target-path",
                &target.to_string_lossy(),
                "--worker-count",
                &w,
            ]);
            if let Some(c2) = cache {
                c.args(["--cache-path", &c2.to_string_lossy()]);
            }
            c
        }
        Impl::Go => {
            let go = p.go.as_ref().unwrap();
            let mut c = Command::new(go);
            // golongtail also splits the planes (--worker-count +
            // --remote-worker-count, per its --help); pin both to N to match.
            c.args([
                "downsync",
                "--worker-count",
                &w,
                "--remote-worker-count",
                &w,
                "--storage-uri",
                &p.store.to_string_lossy(),
                "--source-path",
                &source_lvi.to_string_lossy(),
                "--target-path",
                &target.to_string_lossy(),
                "--no-cache-target-index",
                "--log-level",
                "error",
            ]);
            if let Some(c2) = cache {
                c.args(["--cache-path", &c2.to_string_lossy()]);
            }
            c
        }
    }
}

fn impls(p: &Paths) -> Vec<Impl> {
    let mut v = vec![Impl::Rust];
    if std::env::var("LONGTAIL_BENCH_SKIP_FFI").is_err() {
        v.push(Impl::Ffi);
    }
    if p.go.is_some() {
        v.push(Impl::Go);
    }
    v
}

// ---------------------------------------------------------------------------
// Stats + reporting
// ---------------------------------------------------------------------------

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

struct Cell {
    scenario: String,
    imp: String,
    workers: usize,
    n_ok: usize,
    n_timeout: usize,
    wall_ms: f64,
    cpu_ms: f64,
    rss_mib: f64,
}

fn measure_cell(
    make: &dyn Fn() -> Command,
    prep: &dyn Fn(),
    cleanup: &dyn Fn(),
    iters: usize,
    timeout: Duration,
    label: &str,
) -> (Vec<f64>, Vec<f64>, Vec<f64>, usize) {
    let mut walls = Vec::new();
    let mut cpus = Vec::new();
    let mut rss = Vec::new();
    let mut timeouts = 0usize;
    for i in 0..iters {
        prep();
        let t = run_watched(make, timeout, &format!("{label} iter{}", i + 1));
        if t.ok {
            walls.push(t.wall.as_secs_f64() * 1000.0);
            cpus.push(t.cpu.as_secs_f64() * 1000.0);
            rss.push(t.maxrss_kib as f64 / 1024.0);
        } else if t.timed_out {
            timeouts += 1;
        }
        cleanup();
    }
    (walls, cpus, rss, timeouts)
}

fn main() {
    let p = setup();
    let iters = env_usize("LONGTAIL_BENCH_ITERS", 5);
    let main_workers = env_usize("LONGTAIL_BENCH_MAIN_WORKERS", 8);
    let timeout_mult = env_usize("LONGTAIL_BENCH_TIMEOUT_MULT", 5) as u32;
    let cold_workers: Vec<usize> = std::env::var("LONGTAIL_BENCH_COLD_WORKERS")
        .unwrap_or_else(|_| "2,4,8,0".to_string())
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .map(|w: usize| if w == 0 { num_cpus() } else { w })
        .collect();
    let scenarios: Vec<String> = std::env::var("LONGTAIL_BENCH_SCENARIOS")
        .unwrap_or_else(|_| "cold,warm,incremental".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    eprintln!("== pre-warming store page cache ==");
    read_all_files(&p.store);

    // Establish a baseline (single warm golongtail-or-rust cold downsync of v1)
    // to size the per-run hard timeout = mult × baseline.
    eprintln!("== measuring timeout baseline ==");
    let baseline_target = p.scratch.join("baseline");
    rm(&baseline_target);
    let base_imp = if p.go.is_some() { Impl::Go } else { Impl::Rust };
    let base = run_timed(
        build_cmd(
            &p,
            base_imp,
            &p.v1_lvi,
            &baseline_target,
            main_workers,
            None,
        ),
        Duration::from_secs(1800),
    );
    rm(&baseline_target);
    let baseline = if base.ok {
        base.wall
    } else {
        Duration::from_secs(120)
    };
    let timeout = (baseline * timeout_mult).max(Duration::from_secs(60));
    eprintln!(
        "baseline downsync ({}) = {:.1}s; per-run hard timeout = {:.0}s",
        base_imp.name(),
        baseline.as_secs_f64(),
        timeout.as_secs_f64()
    );

    let impls = impls(&p);
    let mut cells: Vec<Cell> = Vec::new();

    for scenario in &scenarios {
        match scenario.as_str() {
            "cold" => {
                for &workers in &cold_workers {
                    for &imp in &impls {
                        let target = p.scratch.join(format!("cold-{}-{workers}", imp.name()));
                        let label = format!("cold/{}/w{workers}", imp.name());
                        eprintln!("== {label} ({iters} iters) ==");
                        let prep = || rm(&target);
                        let cleanup = || rm(&target);
                        let make = || build_cmd(&p, imp, &p.v1_lvi, &target, workers, None);
                        let (mut w, mut c, mut r, to) =
                            measure_cell(&make, &prep, &cleanup, iters, timeout, &label);
                        cells.push(Cell {
                            scenario: scenario.clone(),
                            imp: imp.name().to_string(),
                            workers,
                            n_ok: w.len(),
                            n_timeout: to,
                            wall_ms: median(&mut w),
                            cpu_ms: median(&mut c),
                            rss_mib: median(&mut r),
                        });
                    }
                }
            }
            "warm" => {
                for &imp in &impls {
                    // Populate a cache once (untimed), then time downsyncs that
                    // read blocks from the warm .lrb cache.
                    let cache = p.scratch.join(format!("cache-{}", imp.name()));
                    let seed_target = p.scratch.join(format!("warm-seed-{}", imp.name()));
                    rm(&cache);
                    rm(&seed_target);
                    let seed = run_watched(
                        &|| build_cmd(&p, imp, &p.v1_lvi, &seed_target, main_workers, Some(&cache)),
                        timeout,
                        &format!("warm-seed/{}", imp.name()),
                    );
                    rm(&seed_target);
                    if !seed.ok {
                        eprintln!(
                            "  warm cache seed failed for {}; skipping warm cell",
                            imp.name()
                        );
                        continue;
                    }
                    read_all_files(&cache);
                    let target = p.scratch.join(format!("warm-{}", imp.name()));
                    let label = format!("warm/{}/w{main_workers}", imp.name());
                    eprintln!("== {label} ({iters} iters) ==");
                    let prep = || rm(&target);
                    let cleanup = || rm(&target);
                    let make =
                        || build_cmd(&p, imp, &p.v1_lvi, &target, main_workers, Some(&cache));
                    let (mut w, mut c, mut r, to) =
                        measure_cell(&make, &prep, &cleanup, iters, timeout, &label);
                    cells.push(Cell {
                        scenario: scenario.clone(),
                        imp: imp.name().to_string(),
                        workers: main_workers,
                        n_ok: w.len(),
                        n_timeout: to,
                        wall_ms: median(&mut w),
                        cpu_ms: median(&mut c),
                        rss_mib: median(&mut r),
                    });
                    rm(&cache);
                }
            }
            "incremental" => {
                for &imp in &impls {
                    let target = p.scratch.join(format!("inc-{}", imp.name()));
                    let label = format!("incremental/{}/w{main_workers}", imp.name());
                    eprintln!("== {label} ({iters} iters) ==");
                    // prep: fresh target already at v1 (untimed). timed: v2 into it.
                    let prep = || {
                        rm(&target);
                        let t = run_watched(
                            &|| build_cmd(&p, imp, &p.v1_lvi, &target, main_workers, None),
                            timeout,
                            &format!("inc-prep/{}", imp.name()),
                        );
                        if !t.ok {
                            eprintln!("  incremental prep (v1) failed for {}", imp.name());
                        }
                    };
                    let cleanup = || rm(&target);
                    let make = || build_cmd(&p, imp, &p.v2_lvi, &target, main_workers, None);
                    let (mut w, mut c, mut r, to) =
                        measure_cell(&make, &prep, &cleanup, iters, timeout, &label);
                    cells.push(Cell {
                        scenario: scenario.clone(),
                        imp: imp.name().to_string(),
                        workers: main_workers,
                        n_ok: w.len(),
                        n_timeout: to,
                        wall_ms: median(&mut w),
                        cpu_ms: median(&mut c),
                        rss_mib: median(&mut r),
                    });
                }
            }
            other => eprintln!("unknown scenario '{other}' — skipping"),
        }
    }

    rm(&p.scratch);
    print_report(&p, &cells, iters, baseline, timeout);
}

fn print_report(p: &Paths, cells: &[Cell], iters: usize, baseline: Duration, timeout: Duration) {
    let store_bytes = dir_bytes(&p.store);
    let v1_bytes = dir_bytes(&p.data_v1);
    let v2_bytes = dir_bytes(&p.data_v2);
    println!("\n===== E2E RESULTS (markdown) =====\n");
    println!(
        "Dataset: v1 = {:.1} MiB, v2 = {:.1} MiB, store = {:.1} MiB; iters/cell = {iters}; median reported.",
        v1_bytes as f64 / 1048576.0,
        v2_bytes as f64 / 1048576.0,
        store_bytes as f64 / 1048576.0
    );
    println!(
        "Timeout baseline = {:.1}s, per-run hard timeout = {:.0}s.\n",
        baseline.as_secs_f64(),
        timeout.as_secs_f64()
    );
    println!(
        "| scenario | impl | workers | n_ok | timeouts | wall (ms) | CPU (ms) | peak RSS (MiB) |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    for c in cells {
        println!(
            "| {} | {} | {} | {} | {} | {:.0} | {:.0} | {:.0} |",
            c.scenario, c.imp, c.workers, c.n_ok, c.n_timeout, c.wall_ms, c.cpu_ms, c.rss_mib
        );
    }
    println!("\n(peak RSS from wait4 ru_maxrss, KiB→MiB; CPU = user+sys of the child.)");
}

fn dir_bytes(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                total += dir_bytes(&p);
            } else if let Ok(m) = p.metadata() {
                total += m.len();
            }
        }
    }
    total
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8)
}
