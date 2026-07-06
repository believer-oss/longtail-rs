# CI/CD Switchover Checklist — golongtail → pure-Rust `longtail`

> **Status: PREPARED. Staging execution is a manual gate (NOT run by the port
> executor).** Work through this on a staging store first, sign off each step,
> then flip production. Steps 0-4 are the download-path staging gate
> (still **pending**, folded in here so both halves run together); steps 5-9 are
> the upload/maintenance path.

The pure-Rust CLI is a drop-in for the golongtail v0.4.5 commands the pipeline
uses. Build once:

```sh
cargo build --release -p longtail-cli
RUST=./target/release/longtail
GO=./target/golongtail/longtail-linux-x64   # pinned v0.4.5 (xtask fetch-golongtail)
```

Global flags are identical in name/shape: `--worker-count`, `--remote-worker-count`,
`--log-level`, `--show-stats`, and per-command `--s3-endpoint-resolver-uri`.

---

## Command mapping (flag-by-flag)

Every pipeline invocation maps 1:1. The Rust flag names are the golongtail v0.4.5
names verbatim; only the binary changes.

| golongtail | pure-Rust `longtail` | Notes |
|---|---|---|
| `upsync --storage-uri … --source-path DIR --target-path X.lvi [--version-local-store-index-path X.lsi] [--compression-algorithm zstd] [--hash-algorithm blake3] [--target-chunk-size 32768] [--target-block-size 8388608] [--max-chunks-per-block 1024] [--min-block-usage-percent 80] [--source-index-path Y.lvi] [--include/--exclude-filter-regex]` | same | defaults identical; `--source-index-path` skips scan+chunk |
| `put --target-path CFG.json --source-path DIR [--storage-uri …] [--target-version-index-path X.lvi] [--version-local-store-index-path X.lsi] [--no-version-local-store-index]` | same | path-defaulting identical: `<parent>/store`, `<parent>/version-data/version-index/<name>.lvi`, `<parent>/version-data/version-store-index/<name>.lsi`; get-config keys `storage-uri`/`source-path`/`version-local-store-index-path`/`s3-endpoint-resolver-uri` |
| `downsync --storage-uri … --source-path X.lvi --target-path DIR [--cache-path C] [--version-local-store-index-path X.lsi] [--validate] [--[no-]retain-permissions] [--[no-]scan-target] [--[no-]cache-target-index]` | same | download path |
| `get --source-path CFG.json --target-path DIR [--cache-path C]` | same | reads the get-config `put` wrote |
| `init-remote-store --storage-uri …` | same | Init rebuild from block scan |
| `create-version-store-index --storage-uri … --source-path X.lvi --version-local-store-index-path X.lsi` | same | `--source-path` is a **version-index** URI |
| `prune-store --storage-uri … --source-paths FILES.txt [--version-local-store-index-paths LSIS.txt] [--dry-run] [--validate-versions] [--skip-invalid-versions] [--write-version-local-store-index]` | same | `--source-paths` is a text file of `.lvi` URIs, one per line; overwrites index BEFORE deleting blocks |
| `prune-store-index --store-index-path STORE.lsi --source-paths FILES.txt [flags as above]` | same | rewrites only the index |
| `prune-store-blocks --store-index-path STORE.lsi --blocks-root-path …/chunks [--block-extension .lsb] [--dry-run]` | same | deletes orphan `.lsb` only |
| `clone-store --source-storage-uri … --target-storage-uri … --target-path DIR --source-paths S.txt --target-paths T.txt [--source-zip-paths Z.txt] [--create-version-local-store-index] [--skip-validate] [--cache-path C] [--[no-]retain-permissions]` | same | materialize-then-reupload; see Divergences |
| `print-store --store-index-path X.lsi [--compact] [--details]` | same | output format matched |
| `print-version-usage --storage-uri … --version-index-path X.lvi [--cache-path C]` | same | prints Block Usage / Asset Fragmentation |
| `dump-version-assets --version-index-path X.lvi [--details]` | same | one line per asset |
| `cp --storage-uri … --version-index-path X.lvi SRC_ASSET DST` | same | targeted asset extraction |
| `pack` / `unpack` | **not ported** (the `archive` feature, not yet implemented) | if the pipeline uses these, keep golongtail for those two commands until the archive feature lands |

### Divergences to be aware of before switchover

- **clone-store `--source-zip-paths` (zip fallback) is NOT implemented** in the
  Rust port. The normal materialize-then-reupload path is fully supported; the
  zip re-scan fallback (only reached when the source store is missing blocks and
  a zip is supplied) is not. If the pipeline relies on the zip fallback, keep
  golongtail for clone-store until it is added.
- **clone-store `--hash-algorithm` / `--compression-algorithm` are accepted but
  ignored** (the hash + tags come from the source version index — this matches
  golongtail, which also ignores them here).
- **`--use-legacy-write` returns a typed error** (the port implements only
  ChangeVersion2 / the non-legacy write path). Pipelines must not pass it.
- **clone-store already-cloned skip**: the port implements the *intended*
  behaviour (skip a version whose target `.lvi` exists and validates), fixing the
  v0.4.5 swapped-args bug where the skip never fired. Re-running clone-store is
  therefore cheaper than with golongtail (it truly skips), but the end state is
  identical.
- **`gs://` (GCS) is not supported** — a clear error. Our stores are S3 + fs.
- **minio / S3-compatible endpoints**: golongtail's AWS SDK uses **virtual-host**
  bucket addressing. Stock minio does not serve that. Run minio with
  `MINIO_DOMAIN=<host>` and use an endpoint host that resolves `<bucket>.<host>`
  (e.g. `http://127.0.0.1.nip.io:PORT`). Real AWS S3 is unaffected.

---

## Staging validation sequence

Replace `s3://<staging-bucket>/…` with real staging values. Use a **scratch
prefix** so nothing production is touched. If the staging endpoint is minio, add
`--s3-endpoint-resolver-uri <endpoint-url>` to every command.

### Permission-aware tree compare helper

`diff -r` compares content + structure but **misses mode bits**. Use this
permission-aware compare wherever a tree comparison is called for:

```sh
tree_fingerprint() {
  # mode + relative path + size + sha256, sorted — permission-aware.
  ( cd "$1" && find . \( -type f -o -type d \) -printf '%m %y %s %P\n' | sort ) 
  ( cd "$1" && find . -type f -print0 | sort -z | xargs -0 sha256sum | sed "s#$1/##" )
}
compare_trees() {  # compare_trees A B
  diff <(tree_fingerprint "$1") <(tree_fingerprint "$2") && echo "TREES IDENTICAL (content+mode)"
}
```

### Step 0 — build

```sh
cargo build --release -p longtail-cli
RUST=./target/release/longtail
GO=./target/golongtail/longtail-linux-x64
```

### Step 1 — download path: Rust `get` vs golongtail `get`

```sh
"$RUST" get --source-path s3://<staging-bucket>/<path>/get-config.json \
  --target-path /tmp/staging-rust --no-cache-target-index
"$GO"   get --source-path s3://<staging-bucket>/<path>/get-config.json \
  --target-path /tmp/staging-go   --no-cache-target-index
compare_trees /tmp/staging-rust /tmp/staging-go   # expect: TREES IDENTICAL
```

### Step 2 — download path: validate the store covers the version

```sh
"$RUST" validate-version \
  --storage-uri s3://<staging-bucket>/<store-path> \
  --version-index-path s3://<staging-bucket>/<path>/<version>.lvi
```

### Step 3 — upload path round-trip (Rust upsync → golongtail downsync)

Use a **scratch store prefix**. Upsync a real staging build folder with Rust,
then confirm golongtail reads it back identically.

```sh
"$RUST" upsync \
  --storage-uri s3://<staging-bucket>/scratch/store \
  --source-path /path/to/a/real/build \
  --target-path s3://<staging-bucket>/scratch/build.lvi \
  --version-local-store-index-path s3://<staging-bucket>/scratch/build.lsi
"$GO" downsync \
  --storage-uri s3://<staging-bucket>/scratch/store \
  --source-path s3://<staging-bucket>/scratch/build.lvi \
  --target-path /tmp/rt-go --no-cache-target-index
compare_trees /path/to/a/real/build /tmp/rt-go     # expect: TREES IDENTICAL
```

### Step 4 — upload path round-trip (golongtail upsync → Rust downsync)

```sh
"$GO" upsync \
  --storage-uri s3://<staging-bucket>/scratch2/store \
  --source-path /path/to/a/real/build \
  --target-path s3://<staging-bucket>/scratch2/build.lvi
"$RUST" downsync \
  --storage-uri s3://<staging-bucket>/scratch2/store \
  --source-path s3://<staging-bucket>/scratch2/build.lvi \
  --target-path /tmp/rt-rust --no-cache-target-index
compare_trees /path/to/a/real/build /tmp/rt-rust   # expect: TREES IDENTICAL
```

### Step 5 — `put` + `get` (the real pipeline shape, if used)

```sh
"$RUST" put --target-path s3://<staging-bucket>/scratch3/get-config.json \
  --source-path /path/to/a/real/build
"$RUST" get --source-path s3://<staging-bucket>/scratch3/get-config.json \
  --target-path /tmp/put-get --no-cache-target-index
compare_trees /path/to/a/real/build /tmp/put-get
```

### Step 6 — maintenance commands smoke (scratch store only)

```sh
"$RUST" print-store --store-index-path s3://<staging-bucket>/scratch/store/store.lsi
"$RUST" print-version-usage --storage-uri s3://<staging-bucket>/scratch/store \
  --version-index-path s3://<staging-bucket>/scratch/build.lvi
"$RUST" dump-version-assets --version-index-path s3://<staging-bucket>/scratch/build.lvi --details | head
```

### Step 7 — prune **dry-run** first, then real (scratch store only)

```sh
printf '%s\n' s3://<staging-bucket>/scratch/build.lvi > /tmp/keep.txt
"$RUST" prune-store --storage-uri s3://<staging-bucket>/scratch/store \
  --source-paths /tmp/keep.txt --dry-run           # prints "Prune would keep N blocks"
# Only after reviewing the dry-run count:
"$RUST" prune-store --storage-uri s3://<staging-bucket>/scratch/store \
  --source-paths /tmp/keep.txt
```

### Step 8 — init-remote-store (rebuild) sanity (scratch store only)

```sh
"$RUST" init-remote-store --storage-uri s3://<staging-bucket>/scratch/store
"$RUST" validate-version --storage-uri s3://<staging-bucket>/scratch/store \
  --version-index-path s3://<staging-bucket>/scratch/build.lvi
```

### Step 9 — flip production (per command, canary first)

1. Swap ONE low-risk command (e.g. `print-store`, `dump-version-assets`) to
   `$RUST` in the pipeline; observe one cycle.
2. Swap the download path (`get`/`downsync`); observe.
3. Swap the upload path (`upsync`/`put`); observe several real builds.
4. Swap maintenance (`prune-*`, `clone-store`, `init-remote-store`) last.
5. Keep `pack`/`unpack` on golongtail until the archive feature lands (if used at all).

### Rollback

- The on-disk formats are **byte-compatible** in both directions (interop gate ⑥
  and the mixed-writer gate ⑧ prove this). Reverting a pipeline step to
  golongtail requires no data migration — point the step back at `$GO`.
- `prune-store` / `prune-store-index` are the only destructive commands. Always
  `--dry-run` first; a store index overwrite is done BEFORE any block delete, so
  a mid-run failure leaves harmless orphan blocks (recoverable via
  `init-remote-store`), never a dangling index.
- Keep the pinned golongtail binary available for the duration of the rollout.

---

## Sign-off

| Step | Command(s) | Run by | Date | Result |
|---|---|---|---|---|
| 1 | get vs get | | | |
| 2 | validate-version | | | |
| 3 | rust upsync → go downsync | | | |
| 4 | go upsync → rust downsync | | | |
| 5 | put + get | | | |
| 6 | print-* / dump | | | |
| 7 | prune dry-run + real | | | |
| 8 | init-remote-store | | | |
| 9 | production flip | | | |
