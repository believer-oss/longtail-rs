//! HPCDC vs FastCDC dedup comparison (the FastCDC go/no-go input; fastcdc
//! feature). Chunks the synthetic v1/v2 dataset (and the fixture chain) with
//! both algorithms at target 32768 and reports, per algorithm:
//!   - unique-chunk bytes ÷ total bytes (v1 and v2 self-dedup),
//!   - the DIRECTIONAL cross-version metric: bytes of v2's chunks already
//!     present in v1's chunk set ÷ v2 total = the download-avoided fraction,
//!   - the chunk-size distribution (count, mean, median).
//!
//! The distribution is reported alongside the ratio deliberately: without it the
//! ratio comparison is meaningless, since a smaller average chunk trivially
//! dedups better. FastCDC has NO compat obligation — switching would orphan
//! existing store dedup — so this is a roadmap input only.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use longtail_bench::{bench_root, chunk_tree, dataset_plan, write_churned_v2, write_dataset};
#[cfg(feature = "fastcdc")]
use longtail_core::FastCdcChunker;
use longtail_core::{Chunker, HpcdcChunker};

const TARGET: u32 = 32768;

struct Algo {
    name: &'static str,
    v1: Vec<(u64, u32)>,
    v2: Vec<(u64, u32)>,
}

fn unique_bytes(chunks: &[(u64, u32)]) -> (u64, u64) {
    let mut seen: HashMap<u64, u32> = HashMap::new();
    let mut total = 0u64;
    for &(k, s) in chunks {
        total += s as u64;
        seen.entry(k).or_insert(s);
    }
    let unique: u64 = seen.values().map(|&s| s as u64).sum();
    (total, unique)
}

/// bytes of v2 chunks whose content is already in v1's chunk set, ÷ v2 total.
fn avoided(v1: &[(u64, u32)], v2: &[(u64, u32)]) -> (u64, u64) {
    let v1set: std::collections::HashSet<u64> = v1.iter().map(|&(k, _)| k).collect();
    let mut avoided = 0u64;
    let mut total = 0u64;
    for &(k, s) in v2 {
        total += s as u64;
        if v1set.contains(&k) {
            avoided += s as u64;
        }
    }
    (avoided, total)
}

fn dist(chunks: &[(u64, u32)]) -> (usize, f64, f64) {
    if chunks.is_empty() {
        return (0, 0.0, 0.0);
    }
    let count = chunks.len();
    let mean = chunks.iter().map(|&(_, s)| s as f64).sum::<f64>() / count as f64;
    let mut sizes: Vec<u32> = chunks.iter().map(|&(_, s)| s).collect();
    sizes.sort_unstable();
    let median = if count % 2 == 1 {
        sizes[count / 2] as f64
    } else {
        (sizes[count / 2 - 1] as f64 + sizes[count / 2] as f64) / 2.0
    };
    (count, mean, median)
}

fn report(section: &str, algos: &[Algo]) {
    println!("\n### {section}\n");
    println!(
        "| algo | v1 chunks | v1 mean | v1 median | v1 self-dedup (unique/total) | v2 self-dedup | download-avoided (v2∩v1 / v2) |"
    );
    println!("|---|---|---|---|---|---|---|");
    for a in algos {
        let (v1_total, v1_unique) = unique_bytes(&a.v1);
        let (v2_total, v2_unique) = unique_bytes(&a.v2);
        let (av, av_total) = avoided(&a.v1, &a.v2);
        let (count, mean, median) = dist(&a.v1);
        let pct = |n: u64, d: u64| {
            if d == 0 {
                0.0
            } else {
                n as f64 * 100.0 / d as f64
            }
        };
        println!(
            "| {} | {} | {:.0} | {:.0} | {:.1}% | {:.1}% | {:.1}% |",
            a.name,
            count,
            mean,
            median,
            pct(v1_unique, v1_total),
            pct(v2_unique, v2_total),
            pct(av, av_total),
        );
    }
    println!(
        "\n(self-dedup = unique-chunk bytes ÷ total bytes; lower = more internal duplication. download-avoided = fraction of v2's bytes whose chunks already exist in v1 → what an incremental downsync skips.)"
    );
}

fn ensure_synthetic(v1: &Path, v2: &Path) {
    let size_mb: usize = std::env::var("LONGTAIL_BENCH_DATA_SIZE_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1024);
    let plan = dataset_plan(size_mb * 1024 * 1024);
    if std::fs::read_dir(v1)
        .map(|mut r| r.next().is_none())
        .unwrap_or(true)
    {
        eprintln!("dedup: generating v1 ({size_mb} MiB)");
        std::fs::create_dir_all(v1).unwrap();
        write_dataset(v1, &plan).expect("write v1");
    }
    if std::fs::read_dir(v2)
        .map(|mut r| r.next().is_none())
        .unwrap_or(true)
    {
        eprintln!("dedup: generating v2 (churned)");
        std::fs::create_dir_all(v2).unwrap();
        write_churned_v2(v1, v2, &plan).expect("write v2");
    }
}

fn chunk_pair(v1: &Path, v2: &Path) -> Vec<Algo> {
    let hpcdc = HpcdcChunker::from_target(TARGET).unwrap();
    let mut out = vec![Algo {
        name: "HPCDC",
        v1: chunk_tree(v1, &hpcdc).expect("chunk v1 hpcdc"),
        v2: chunk_tree(v2, &hpcdc).expect("chunk v2 hpcdc"),
    }];
    #[cfg(feature = "fastcdc")]
    {
        let fc = FastCdcChunker::from_target(TARGET);
        out.push(Algo {
            name: "FastCDC",
            v1: chunk_tree(v1, &fc).expect("chunk v1 fastcdc"),
            v2: chunk_tree(v2, &fc).expect("chunk v2 fastcdc"),
        });
    }
    // touch the trait so the pure (non-fastcdc) build doesn't warn.
    let _ = &hpcdc as &dyn Chunker;
    out
}

fn main() {
    let root = bench_root();
    let v1 = root.join("data/v1");
    let v2 = root.join("data/v2");
    ensure_synthetic(&v1, &v2);

    println!("# HPCDC vs FastCDC dedup comparison (target {TARGET})");
    let algos = chunk_pair(&v1, &v2);
    report("Synthetic ~1 GiB dataset (v1 → v2 churn)", &algos);

    // Fixture chain (tiny — caveat: files below min-chunk are single chunks).
    let chain_root: PathBuf = root.join("dedup-chain");
    let _ = std::fs::remove_dir_all(&chain_root);
    longtail_testkit::corpus::generate_chain(&chain_root);
    let cv1 = chain_root.join("chain/v1");
    let cv2 = chain_root.join("chain/v2");
    let chain_algos = chunk_pair(&cv1, &cv2);
    report(
        "Fixture chain v1 → v2 (tiny — most files < min chunk)",
        &chain_algos,
    );
    let _ = std::fs::remove_dir_all(&chain_root);
}
