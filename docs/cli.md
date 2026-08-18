# The `longtail-rs` CLI

A task-oriented guide. **`longtail-rs <command> --help` is the authority on flags** — it is
generated from the code and cannot drift. This document covers what the commands are *for*, the
shapes a pipeline actually uses, and the behaviour that is easy to get wrong.

The binary is `longtail-rs`, deliberately not `longtail`: golongtail installs under that name, and
both will sit on the same machines for as long as they write the same stores. Commands and flags
are golongtail v0.4.5's verbatim, so a pipeline step ports by changing the program name. Nine
commands also answer to golongtail's camelCase spelling (`validate`, `printVersionIndex`,
`printStoreIndex`, `stats`, `dump`, `init`, `createVersionStoreIndex`, `cloneStore`, `pruneStore`).

```sh
cargo build --release -p longtail-cli   # -> target/release/longtail-rs
```

## The two flows

A **store** holds deduplicated blocks. A **version index** (`.lvi`) describes one snapshot of a
folder: which chunks, in which order, with what permissions. Publishing writes blocks into the
store and a `.lvi` beside it; installing reads a `.lvi` and materialises the folder.

```
publish:  folder ──upsync──> store + version.lvi
          folder ──put─────> store + version.lvi + get-config.json   (paths derived for you)

install:  version.lvi ──downsync──> folder
          get-config.json ──get───> folder                            (config names the rest)
```

`put`/`get` are the pairing to prefer in a pipeline: `put` derives the store, index and
store-index paths from one target path and writes a get-config JSON naming them, so the installing
side needs only that one URL. `upsync`/`downsync` are the explicit forms when you want to control
every path.

## Commands by purpose

| Purpose | Commands |
|---|---|
| Publish | `upsync`, `put` |
| Install | `downsync`, `get` |
| Inspect (no store needed) | `print-version`, `dump-version-assets`, `ls`, `print-store` |
| Inspect (reads the store) | `validate-version`, `print-version-usage`, `cp` |
| Store maintenance | `init-remote-store`, `create-version-store-index`, `clone-store` |
| Destructive maintenance | `prune-store`, `prune-store-index`, `prune-store-blocks` |

## Recipes

**Publish a build.**

```sh
longtail-rs put --source-path ./build --target-path s3://bucket/artifacts/v1.4.2.json
```

Everything else is derived: blocks under `.../store`, the index under
`.../version-data/version-index/v1.4.2.lvi`, and a version-local store index beside it. Hand
consumers the JSON URL.

**Install or update.**

```sh
longtail-rs get --source-path s3://bucket/artifacts/v1.4.2.json \
                --target-path ./install --cache-path ./cache
```

Updating is the same command with a new config: the target is scanned, diffed, and only the
missing blocks are fetched. A local `--cache-path` is reused across versions and is the single
biggest win on repeated installs.

**Repair an install** — check every asset the version names, without touching anything else:

```sh
longtail-rs downsync --storage-uri s3://bucket/artifacts/store \
                     --source-path s3://bucket/artifacts/version-data/version-index/v1.4.2.lvi \
                     --target-path ./install \
                     --no-cache-target-index --no-delete-removed
```

**Both flags are required for this to work.** `--no-delete-removed` keeps files the version does
not contain — saved state, logs, local config — which a normal run deletes.
`--no-cache-target-index` forces the content-hash scan; without it the run trusts the cached index,
finds nothing to do, and repairs nothing. The run warns on stderr if you pass only the first.

**Verify a store covers a version** (no download):

```sh
longtail-rs validate-version --storage-uri s3://bucket/artifacts/store \
                             --version-index-path .../v1.4.2.lvi
```

**Look inside an index** without a store: `print-version` for a summary, `dump-version-assets` for
every path, `ls` to walk one directory, `cp` to extract a single asset.

**Reclaim space.** `prune-store` takes a *keep* list — a text file of `.lvi` URIs, one per line —
rewrites the store index, then deletes the blocks no kept version references. Run `--dry-run`
first, always. An empty keep-set is refused rather than obeyed, because "keep nothing" and "the
list failed to load" look identical; `--allow-empty-keep-set` says you meant it.

## Behaviour worth knowing

**Interrupting is safe, and resuming is re-running.** Ctrl-C finishes in-flight blocks, flushes the
store and exits 130, leaving the target resumable. Re-run the same command: the target is scanned,
diffed and only the remainder fetched. A second Ctrl-C exits immediately.

**The target-index cache is a speed/accuracy trade.** By default a successful run leaves
`.longtail.index.cache.lvi` in the target and the next run trusts it instead of scanning, which is
much faster. It is deleted before anything is written and rewritten only on success, so an
interrupted run cannot leave a stale one *that it wrote*. It can still leave an unreadable one:
the file sits inside the target, so a folder that was downsynced and then upsynced carries it into
the version index as ordinary content, and a later download re-creates it like any other asset —
sized up front, zero-filled until its blocks arrive. A run that fails in between leaves the zeros.
That is not fatal; an unreadable cache is ignored with a warning and the target is scanned instead.
If you never want it in your indexes, exclude it on upsync with `--exclude-filter-regex`.

What the cache cannot detect is damage done to the tree *after* a successful run — for that, scan
(`--no-cache-target-index`).

**`--validate` checks the download against the index it was given**, by re-scanning the target and
comparing content hashes. That proves the download matched the `.lvi`; it does not prove the `.lvi`
is authentic. On Windows only the writable bit of a recorded mode is compared, because the platform
synthesizes the rest.

**`--verify-chunks` re-hashes every chunk against the version index before writing**, turning a
substituted or corrupted block into a hard error. It is off by default and costs one hash pass over
data already in memory. It authenticates blocks against the `.lvi`, which is itself unsigned — see
`docs/rust-port.md` §Trust boundary for what that does and does not buy.

**S3-compatible endpoints** are reached with `--s3-endpoint-resolver-uri`. The AWS SDK uses
virtual-host bucket addressing, which most local S3 stand-ins do not serve out of the box: the
endpoint host must resolve `<bucket>.<host>`. `gs://` is not supported and says so.

**Worker counts.** `--worker-count` sizes the CPU pool (chunking, hashing); `--remote-worker-count`
bounds concurrent block I/O. Both default to a value derived from the machine and the scheme —
raise the remote count for high-latency stores, lower it if you are being rate-limited.

**Output.** Progress goes to stderr as a single bar on a terminal, or throttled plain lines when
redirected. Logs are `tracing`; `--log-level` or `RUST_LOG` control them, `--log-file-path` writes
JSON, and colour is used only on a terminal. `--show-stats` prints a per-phase summary at the end.

**Exit codes.** `0` success, `1` failure, `130` cancelled by Ctrl-C. A cancelled run is not a failed
one: the target is resumable and the store was flushed cleanly.

## Compatibility notes

- `--use-legacy-write` returns an error. Only the modern write path is implemented.
- `pack`/`unpack` are not implemented; keep golongtail for those.
- `clone-store` accepts `--hash-algorithm`/`--compression-algorithm` and ignores them, as
  golongtail does — the hash and tags come from the source version index. Its `--source-zip-paths`
  fallback is not implemented.
- `--mem-trace`, `--mem-trace-detailed` and `--mem-trace-csv` are accepted and do nothing; they
  instrument a C allocator this implementation does not use, and say so on stderr.
