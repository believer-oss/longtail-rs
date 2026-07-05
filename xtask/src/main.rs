//! Fixture generation and verification for the pure-Rust longtail port.
//!
//! Subcommands:
//!   * `fetch-golongtail`  — download the pinned golongtail v0.4.5 binary to a
//!     cache dir and verify its sha256.
//!   * `gen-fixtures`      — regenerate the corpus and drive the pinned binary
//!     to (re)produce everything under `fixtures/`, then rewrite `manifest.json`.
//!     Requires `--features differential` (drives the C chunker for boundary
//!     tables).
//!   * `verify-fixtures`   — re-hash every file under `fixtures/` against
//!     `manifest.json`; fast, no network, no native lib.
//!
//! Never uses cwd-relative paths: everything resolves from the workspace root
//! (found by walking up from this crate's `CARGO_MANIFEST_DIR`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use longtail_testkit::fixture_manifest::{Manifest, list_fixture_files, sha256_file};
use longtail_testkit::paths::find_workspace_root;

#[cfg(feature = "differential")]
use longtail_testkit::corpus;
#[cfg(feature = "differential")]
use longtail_testkit::differential::store_index_block_tuples_sorted;
#[cfg(feature = "differential")]
use longtail_testkit::fixture_manifest::{FileEntry, Generator, sha256_hex};
#[cfg(feature = "differential")]
use longtail_testkit::paths::upstream_chunker_input;
#[cfg(feature = "differential")]
use longtail_testkit::tree_manifest::TreeManifest;

const GOLONGTAIL_VERSION: &str = "v0.4.5";
const GOLONGTAIL_URL: &str =
    "https://github.com/DanEngelbrecht/golongtail/releases/download/v0.4.5/longtail-linux-x64";
const GOLONGTAIL_SHA256: &str = "91094c3c28f48b66014f0f5d2679bf6fa1880ca6ce971f861f011c31251401f3";
const GOLONGTAIL_BIN: &str = "longtail-linux-x64";

#[cfg(feature = "differential")]
const TARGET_CHUNK_SIZE_DEFAULT: u32 = 32768;

fn workspace_root() -> PathBuf {
    find_workspace_root(Path::new(env!("CARGO_MANIFEST_DIR")))
}

fn cache_dir() -> PathBuf {
    workspace_root().join("target").join("golongtail")
}

fn golongtail_path() -> PathBuf {
    cache_dir().join(GOLONGTAIL_BIN)
}

fn fixtures_dir() -> PathBuf {
    workspace_root().join("fixtures")
}

fn main() -> Result<()> {
    let cmd = std::env::args().nth(1);
    match cmd.as_deref() {
        Some("fetch-golongtail") => fetch_golongtail(),
        Some("gen-fixtures") => gen_fixtures(),
        Some("verify-fixtures") => verify_fixtures(),
        Some("diff-fixtures") => diff_fixtures(),
        other => {
            eprintln!("unknown xtask command: {other:?}");
            eprintln!("usage: xtask <fetch-golongtail|gen-fixtures|verify-fixtures|diff-fixtures>");
            std::process::exit(2);
        }
    }
}

// ---------------------------------------------------------------------------
// fetch-golongtail
// ---------------------------------------------------------------------------

fn fetch_golongtail() -> Result<()> {
    let dst = golongtail_path();
    fs::create_dir_all(cache_dir())?;
    if dst.exists() {
        let got = sha256_file(&dst)?;
        if got == GOLONGTAIL_SHA256 {
            println!(
                "golongtail {GOLONGTAIL_VERSION} already cached at {}",
                dst.display()
            );
            make_executable(&dst)?;
            return Ok(());
        }
        eprintln!("cached binary sha256 {got} != pinned; re-downloading");
        fs::remove_file(&dst)?;
    }

    println!("downloading {GOLONGTAIL_URL}");
    download(GOLONGTAIL_URL, &dst)?;
    let got = sha256_file(&dst).context("hash downloaded binary")?;
    if got != GOLONGTAIL_SHA256 {
        fs::remove_file(&dst).ok();
        bail!("sha256 mismatch for golongtail: got {got}, pinned {GOLONGTAIL_SHA256}");
    }
    make_executable(&dst)?;
    println!(
        "verified golongtail {GOLONGTAIL_VERSION} -> {}",
        dst.display()
    );
    Ok(())
}

/// Download a URL to a path using the system `curl` (fallback `wget`), avoiding a
/// TLS stack dependency in the pure lane.
fn download(url: &str, dst: &Path) -> Result<()> {
    let curl = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(dst)
        .arg(url)
        .status();
    if curl.map(|s| s.success()).unwrap_or(false) {
        return Ok(());
    }
    let wget = Command::new("wget")
        .args(["-q", "-O"])
        .arg(dst)
        .arg(url)
        .status()
        .context("neither curl nor wget available to download golongtail")?;
    if !wget.success() {
        bail!("download failed for {url}");
    }
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(feature = "differential")]
fn ensure_golongtail() -> Result<PathBuf> {
    let bin = golongtail_path();
    if !bin.exists() {
        fetch_golongtail()?;
    }
    Ok(bin)
}

// ---------------------------------------------------------------------------
// gen-fixtures
// ---------------------------------------------------------------------------

fn gen_fixtures() -> Result<()> {
    #[cfg(not(feature = "differential"))]
    {
        bail!(
            "gen-fixtures requires the native lib for boundary tables; \
             run: cargo run -p xtask --features differential -- gen-fixtures"
        );
    }

    #[cfg(feature = "differential")]
    {
        gen_into(&fixtures_dir())
    }
}

/// Generate the entire fixture set into `fixtures` (the committed `fixtures/`
/// for `gen-fixtures`, or a scratch dir for `diff-fixtures`).
#[cfg(feature = "differential")]
fn gen_into(fixtures: &Path) -> Result<()> {
    let bin = ensure_golongtail()?;
    let corpus_tmp = tempfile::tempdir().context("create corpus temp dir")?;
    let corpus_root = corpus_tmp.path();

    println!("generating corpus into {}", corpus_root.display());
    corpus::generate_all(corpus_root);

    clean_fixtures(fixtures)?;

    // chunker.input (copied verbatim from the longtail submodule testdata).
    let chunker_src = upstream_chunker_input();
    fs::copy(&chunker_src, fixtures.join("chunker.input"))
        .with_context(|| format!("copy chunker.input from {}", chunker_src.display()))?;

    // Store cells.
    gen_default_cell(&bin, fixtures, corpus_root)?;
    gen_standard_cells(&bin, fixtures, corpus_root)?;
    gen_sharded_cell(&bin, fixtures, corpus_root)?;
    gen_get_configs(&bin, fixtures, corpus_root)?;

    // Tree manifests (source-of-truth trees).
    gen_manifests(fixtures, corpus_root)?;

    // Boundary golden tables (via the C chunker).
    gen_boundaries(fixtures, corpus_root)?;

    // golongtail leaves fs-store lock files behind on Linux.
    remove_lock_files(fixtures)?;

    // Finally, the fixture manifest.
    write_manifest(fixtures, &bin)?;

    let (files, bytes) = fixture_stats(fixtures);
    println!(
        "wrote {files} fixture files, {:.2} MiB total",
        bytes as f64 / (1024.0 * 1024.0)
    );
    Ok(())
}

#[cfg(feature = "differential")]
fn clean_fixtures(fixtures: &Path) -> Result<()> {
    fs::create_dir_all(fixtures)?;
    for name in ["stores", "boundaries", "manifests", "get-configs"] {
        let p = fixtures.join(name);
        if p.exists() {
            fs::remove_dir_all(&p)?;
        }
    }
    for name in ["chunker.input", "manifest.json"] {
        let p = fixtures.join(name);
        if p.exists() {
            fs::remove_file(&p)?;
        }
    }
    Ok(())
}

#[cfg(feature = "differential")]
fn run_golongtail(bin: &Path, cwd: Option<&Path>, args: &[&str]) -> Result<()> {
    let mut cmd = Command::new(bin);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    cmd.args(args);
    let out = cmd.output().context("spawn golongtail")?;
    if !out.status.success() {
        bail!(
            "golongtail {:?} failed ({})\nstdout:\n{}\nstderr:\n{}",
            args,
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
    Ok(())
}

/// Run one upsync of `source` into `store`, producing `lvi` + `store_lsi`.
#[cfg(feature = "differential")]
#[allow(clippy::too_many_arguments)]
fn upsync(
    bin: &Path,
    source: &Path,
    lvi: &Path,
    store_lsi: &Path,
    store: &Path,
    hash: &str,
    comp: &str,
    target: u32,
) -> Result<()> {
    if let Some(p) = lvi.parent() {
        fs::create_dir_all(p)?;
    }
    let target_s = target.to_string();
    run_golongtail(
        bin,
        None,
        &[
            "upsync",
            "--source-path",
            &source.to_string_lossy(),
            "--target-path",
            &lvi.to_string_lossy(),
            "--version-local-store-index-path",
            &store_lsi.to_string_lossy(),
            "--storage-uri",
            &store.to_string_lossy(),
            "--hash-algorithm",
            hash,
            "--compression-algorithm",
            comp,
            "--target-chunk-size",
            &target_s,
            "--worker-count",
            "1",
            "--log-level",
            "error",
        ],
    )
}

/// default cell: FULL corpus (zoo + chain v1/v2/v3), blake3 × zstd × 32768.
#[cfg(feature = "differential")]
fn gen_default_cell(bin: &Path, fixtures: &Path, corpus_root: &Path) -> Result<()> {
    println!("cell: default (full corpus)");
    let cell = fixtures.join("stores/default");
    let store = cell.join("store");

    // Zoo: materialize into a temp subset dir (all entries) and upsync.
    let zoo_tmp = tempfile::tempdir()?;
    corpus::copy_entries(corpus_root, zoo_tmp.path(), &corpus::zoo_all());
    upsync(
        bin,
        zoo_tmp.path(),
        &cell.join("zoo.lvi"),
        &cell.join("zoo-store.lsi"),
        &store,
        "blake3",
        "zstd",
        TARGET_CHUNK_SIZE_DEFAULT,
    )?;

    // Chain versions into the same store.
    for id in ["v1", "v2", "v3"] {
        let src = corpus_root.join("chain").join(id);
        upsync(
            bin,
            &src,
            &cell.join(format!("chain-{id}.lvi")),
            &cell.join(format!("chain-{id}-store.lsi")),
            &store,
            "blake3",
            "zstd",
            TARGET_CHUNK_SIZE_DEFAULT,
        )?;
    }
    Ok(())
}

#[cfg(feature = "differential")]
struct StdCell {
    name: &'static str,
    subset: Vec<&'static str>,
    hash: &'static str,
    comp: &'static str,
    target: u32,
    optional: bool,
}

#[cfg(feature = "differential")]
fn gen_standard_cells(bin: &Path, fixtures: &Path, corpus_root: &Path) -> Result<()> {
    let small = corpus::zoo_small();
    let medium = corpus::zoo_medium();
    let cells = vec![
        StdCell {
            name: "comp-none",
            subset: small.clone(),
            hash: "blake3",
            comp: "none",
            target: 32768,
            optional: false,
        },
        StdCell {
            name: "comp-lz4",
            subset: small.clone(),
            hash: "blake3",
            comp: "lz4",
            target: 32768,
            optional: false,
        },
        StdCell {
            name: "comp-zstd_min",
            subset: small.clone(),
            hash: "blake3",
            comp: "zstd_min",
            target: 32768,
            optional: false,
        },
        StdCell {
            name: "comp-zstd_max",
            subset: small.clone(),
            hash: "blake3",
            comp: "zstd_max",
            target: 32768,
            optional: false,
        },
        StdCell {
            name: "comp-brotli",
            subset: small.clone(),
            hash: "blake3",
            comp: "brotli",
            target: 32768,
            optional: false,
        },
        StdCell {
            name: "comp-brotli_text",
            subset: small.clone(),
            hash: "blake3",
            comp: "brotli_text",
            target: 32768,
            optional: false,
        },
        StdCell {
            name: "chunk-1024",
            subset: medium.clone(),
            hash: "blake3",
            comp: "zstd",
            target: 1024,
            optional: false,
        },
        StdCell {
            name: "chunk-131072",
            subset: medium.clone(),
            hash: "blake3",
            comp: "zstd",
            target: 131072,
            optional: false,
        },
        StdCell {
            name: "blake2",
            subset: small.clone(),
            hash: "blake2",
            comp: "zstd",
            target: 32768,
            optional: false,
        },
        StdCell {
            name: "meow",
            subset: vec![corpus::entries::MIN_CHUNK, corpus::entries::DUP_A],
            hash: "meow",
            comp: "zstd",
            target: 32768,
            optional: true,
        },
    ];

    for cell in cells {
        println!("cell: {}", cell.name);
        let cell_dir = fixtures.join("stores").join(cell.name);
        let src_tmp = tempfile::tempdir()?;
        corpus::copy_entries(corpus_root, src_tmp.path(), &cell.subset);
        let res = upsync(
            bin,
            src_tmp.path(),
            &cell_dir.join("zoo.lvi"),
            &cell_dir.join("zoo-store.lsi"),
            &cell_dir.join("store"),
            cell.hash,
            cell.comp,
            cell.target,
        );
        if let Err(e) = res {
            if cell.optional {
                eprintln!("WARNING: optional cell {} skipped: {e}", cell.name);
                if cell_dir.exists() {
                    fs::remove_dir_all(&cell_dir).ok();
                }
            } else {
                return Err(e);
            }
        }
    }
    Ok(())
}

/// Synthesize an S3-style sharded store: two disjoint subsets upsynced into two
/// separate stores; each store's `store.lsi` renamed to
/// `store_<sha256-of-bytes>.lsi`; both placed with the union of their `chunks/`
/// trees and NO canonical `store.lsi`. Also emits `version.lvi` spanning both
/// shards so the merge-on-read path can be exercised.
#[cfg(feature = "differential")]
fn gen_sharded_cell(bin: &Path, fixtures: &Path, corpus_root: &Path) -> Result<()> {
    println!("cell: sharded (merge-on-read synthesis)");
    let sharded = fixtures.join("stores/sharded");
    fs::create_dir_all(sharded.join("chunks"))?;

    let make_shard = |subset: &[&str], keep: &Path| -> Result<PathBuf> {
        let src = keep.join("src");
        corpus::copy_entries(corpus_root, &src, subset);
        upsync(
            bin,
            &src,
            &keep.join("v.lvi"),
            &keep.join("v-store.lsi"),
            &keep.join("store"),
            "blake3",
            "zstd",
            TARGET_CHUNK_SIZE_DEFAULT,
        )?;
        Ok(keep.join("store"))
    };

    let shard_a_tmp = tempfile::tempdir()?;
    let shard_b_tmp = tempfile::tempdir()?;
    let store_a = make_shard(&corpus::sharded_subset_a(), shard_a_tmp.path())?;
    let store_b = make_shard(&corpus::sharded_subset_b(), shard_b_tmp.path())?;

    // Union of chunk trees, and shard-named store indexes.
    for store in [&store_a, &store_b] {
        copy_chunks(&store.join("chunks"), &sharded.join("chunks"))?;
        let lsi = store.join("store.lsi");
        let bytes = fs::read(&lsi).with_context(|| format!("read {}", lsi.display()))?;
        let sha = sha256_hex(&bytes);
        fs::write(sharded.join(format!("store_{sha}.lsi")), &bytes)?;
    }

    // Combined version index spanning both shards (its store is discarded).
    let union_tmp = tempfile::tempdir()?;
    let union_src = union_tmp.path().join("src");
    corpus::copy_entries(corpus_root, &union_src, &corpus::sharded_union());
    let throwaway = tempfile::tempdir()?;
    upsync(
        bin,
        &union_src,
        &sharded.join("version.lvi"),
        &throwaway.path().join("v-store.lsi"),
        &throwaway.path().join("store"),
        "blake3",
        "zstd",
        TARGET_CHUNK_SIZE_DEFAULT,
    )?;
    Ok(())
}

#[cfg(feature = "differential")]
fn copy_chunks(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    for entry in walkdir::WalkDir::new(src).min_depth(1) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(p) = target.parent() {
                fs::create_dir_all(p)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// A self-contained get-config example produced by `put`, with relative URIs so
/// the JSON is byte-reproducible regardless of the generating machine's paths.
#[cfg(feature = "differential")]
fn gen_get_configs(bin: &Path, fixtures: &Path, corpus_root: &Path) -> Result<()> {
    println!("cell: get-configs (put)");
    let gc = fixtures.join("get-configs");
    fs::create_dir_all(&gc)?;
    let src_tmp = tempfile::tempdir()?;
    corpus::copy_entries(
        corpus_root,
        src_tmp.path(),
        &[corpus::entries::MIN_CHUNK, corpus::entries::DUP_A],
    );
    // Run with cwd = get-configs so target/storage URIs are written relative.
    run_golongtail(
        bin,
        Some(&gc),
        &[
            "put",
            "--source-path",
            &src_tmp.path().to_string_lossy(),
            "--target-path",
            "get-config.json",
            "--target-version-index-path",
            "version.lvi",
            "--version-local-store-index-path",
            "version-store.lsi",
            "--storage-uri",
            "store",
            "--hash-algorithm",
            "blake3",
            "--compression-algorithm",
            "zstd",
            "--target-chunk-size",
            "32768",
            "--worker-count",
            "1",
            "--log-level",
            "error",
        ],
    )
}

#[cfg(feature = "differential")]
fn gen_manifests(fixtures: &Path, corpus_root: &Path) -> Result<()> {
    println!("capturing tree manifests");
    let mdir = fixtures.join("manifests");
    fs::create_dir_all(&mdir)?;

    // Chain versions.
    for id in ["v1", "v2", "v3"] {
        let tm = TreeManifest::capture(&corpus_root.join("chain").join(id))?;
        fs::write(mdir.join(format!("chain-{id}.json")), tm.to_json())?;
    }

    // Full zoo (materialized into a temp dir so the tree matches what was
    // upsynced into stores/default).
    let zoo_tmp = tempfile::tempdir()?;
    corpus::copy_entries(corpus_root, zoo_tmp.path(), &corpus::zoo_all());
    let tm = TreeManifest::capture(zoo_tmp.path())?;
    fs::write(mdir.join("zoo.json"), tm.to_json())?;

    // Sharded union source tree.
    let union_tmp = tempfile::tempdir()?;
    corpus::copy_entries(corpus_root, union_tmp.path(), &corpus::sharded_union());
    let tm = TreeManifest::capture(union_tmp.path())?;
    fs::write(mdir.join("sharded-union.json"), tm.to_json())?;
    Ok(())
}

#[cfg(feature = "differential")]
fn remove_lock_files(fixtures: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(fixtures).min_depth(1) {
        let entry = entry?;
        if entry.file_type().is_file() && entry.file_name() == "store.lsi.sync" {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(feature = "differential")]
fn produced_by(rel: &str) -> &'static str {
    if rel == "chunker.input" {
        "copied from longtail testdata"
    } else if rel.starts_with("boundaries/") {
        "ffi chunker (C)"
    } else if rel.starts_with("manifests/") {
        "testkit tree-manifest"
    } else if rel.starts_with("stores/sharded/") {
        "synthesized shard"
    } else if rel.starts_with("get-configs/") {
        "golongtail put"
    } else if rel.starts_with("stores/") {
        "golongtail upsync"
    } else {
        "generated"
    }
}

#[cfg(feature = "differential")]
fn write_manifest(fixtures: &Path, bin: &Path) -> Result<()> {
    let mut entries = Vec::new();
    for (rel, abs) in list_fixture_files(fixtures) {
        let bytes = fs::read(&abs)?;
        entries.push(FileEntry {
            path: rel.clone(),
            size: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
            produced_by: produced_by(&rel).to_string(),
        });
    }
    let manifest = Manifest {
        generator: Generator {
            version: GOLONGTAIL_VERSION.to_string(),
            url: GOLONGTAIL_URL.to_string(),
            binary_sha256: sha256_file(bin)?,
            os: std::env::consts::OS.to_string(),
        },
        entries,
    };
    fs::write(fixtures.join("manifest.json"), manifest.to_json())?;
    Ok(())
}

fn fixture_stats(fixtures: &Path) -> (usize, u64) {
    let files = list_fixture_files(fixtures);
    let bytes = files
        .iter()
        .map(|(_, abs)| abs.metadata().map(|m| m.len()).unwrap_or(0))
        .sum();
    (files.len(), bytes)
}

// --- boundary tables (differential only) ---

#[cfg(feature = "differential")]
fn gen_boundaries(fixtures: &Path, corpus_root: &Path) -> Result<()> {
    use longtail_testkit::differential::{boundary_table_buffer, boundary_table_streaming};
    println!("generating boundary tables (C chunker)");
    let bdir = fixtures.join("boundaries");
    fs::create_dir_all(&bdir)?;

    let write_both = |id: &str, data: &[u8], target: u32| -> Result<()> {
        let safe = id.replace('/', "_");
        let s = boundary_table_streaming(id, data, target);
        fs::write(
            bdir.join(format!("{safe}.t{target}.streaming.json")),
            s.to_json(),
        )?;
        let b = boundary_table_buffer(id, data, target);
        fs::write(
            bdir.join(format!("{safe}.t{target}.buffer.json")),
            b.to_json(),
        )?;
        Ok(())
    };

    let chunker_input = fs::read(fixtures.join("chunker.input"))?;
    for target in [1024u32, 32768, 131072, 1048576] {
        write_both("chunker.input", &chunker_input, target)?;
    }

    let cases = [
        "big-stream",
        "compressible.txt",
        "incompressible.bin",
        "repetitive.bin",
        "win-47",
        "win-48",
        "win-49",
        "max-edge-minus",
        "max-edge",
        "max-edge-plus",
    ];
    for case in cases {
        let data =
            fs::read(corpus_root.join(case)).with_context(|| format!("read corpus case {case}"))?;
        write_both(case, &data, 32768)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// verify-fixtures
// ---------------------------------------------------------------------------

fn verify_fixtures() -> Result<()> {
    let fixtures = fixtures_dir();
    let manifest = Manifest::load(&fixtures).context("load fixtures/manifest.json")?;
    let problems = manifest.verify(&fixtures);
    if problems.is_empty() {
        let (files, bytes) = fixture_stats(&fixtures);
        println!(
            "verify-fixtures OK: {files} files, {:.2} MiB, all sha256 match",
            bytes as f64 / (1024.0 * 1024.0)
        );
        Ok(())
    } else {
        for p in &problems {
            eprintln!("  - {p}");
        }
        bail!("verify-fixtures FAILED with {} problem(s)", problems.len());
    }
}

// ---------------------------------------------------------------------------
// diff-fixtures (scheduled freshness check)
// ---------------------------------------------------------------------------

/// Regenerate the whole fixture set into a scratch dir and compare it against
/// the committed `fixtures/`. Everything is compared byte-exactly EXCEPT store
/// indexes (`*.lsi`), which golongtail emits with a non-deterministic block
/// ordering; those are compared semantically (equal sorted block-hash set).
/// Fails loudly on real drift — catching golongtail behavior changes and
/// accidental fixture rot — without false-positives on block reordering.
#[cfg(feature = "differential")]
fn diff_fixtures() -> Result<()> {
    use std::collections::BTreeSet;

    let committed = fixtures_dir();
    let scratch = tempfile::tempdir().context("scratch dir")?;
    let fresh = scratch.path().join("fixtures");
    fs::create_dir_all(&fresh)?;
    println!("regenerating into scratch {}", fresh.display());
    gen_into(&fresh)?;

    let committed_set: BTreeSet<String> = list_fixture_files(&committed)
        .into_iter()
        .map(|(r, _)| r)
        .collect();
    let fresh_set: BTreeSet<String> = list_fixture_files(&fresh)
        .into_iter()
        .map(|(r, _)| r)
        .collect();

    let mut problems = Vec::new();
    for m in committed_set.difference(&fresh_set) {
        problems.push(format!("present in committed, absent in fresh gen: {m}"));
    }
    for e in fresh_set.difference(&committed_set) {
        problems.push(format!("present in fresh gen, absent in committed: {e}"));
    }
    let mut reordered = 0usize;
    for rel in committed_set.intersection(&fresh_set) {
        let cbytes = fs::read(committed.join(rel))?;
        let fbytes = fs::read(fresh.join(rel))?;
        if cbytes == fbytes {
            continue;
        }
        if rel.ends_with(".lsi") {
            // A1: compare (block_hash, tag, chunk_hashes, chunk_sizes) tuples,
            // not just the block-hash set — a tag/size-altering regression can't
            // hide behind matching hashes. Block ordering (golongtail's Go-map
            // nondeterminism) is normalized by sorting the tuples by block hash.
            let ch = store_index_block_tuples_sorted(&cbytes)
                .map_err(|e| anyhow::anyhow!("parse committed {rel}: {e}"))?;
            let fh = store_index_block_tuples_sorted(&fbytes)
                .map_err(|e| anyhow::anyhow!("parse fresh {rel}: {e}"))?;
            if ch == fh {
                reordered += 1;
            } else {
                problems.push(format!(
                    "store-index drift for {rel}: block (hash, tag, chunk_sizes) set changed"
                ));
            }
        } else {
            problems.push(format!("byte drift for {rel}"));
        }
    }

    if problems.is_empty() {
        println!(
            "diff-fixtures OK: fresh gen matches committed \
             ({reordered} store index(es) differed only in block ordering)"
        );
        Ok(())
    } else {
        for p in &problems {
            eprintln!("  - {p}");
        }
        bail!("diff-fixtures found {} drift(s)", problems.len());
    }
}

#[cfg(not(feature = "differential"))]
fn diff_fixtures() -> Result<()> {
    bail!("diff-fixtures requires --features differential (regenerates via golongtail + C chunker)")
}
