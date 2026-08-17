# Fixtures

Committed golden fixtures for the pure-Rust longtail test suites. These are the byte-exact
reference outputs the default test run checks parsing, serialization, chunking, decompression, and
end-to-end downsync against — no native library required. They were produced by a **pinned
upstream golongtail CLI** (`v0.4.5`); the exact binary URL, OS, and SHA256 are recorded in
`manifest.json`, whose `entries` list every committed file with its size and SHA256.

Treat everything here as read-only reference data. Regenerating it requires the pinned CLI and
must reproduce the recorded checksums exactly (`xtask gen-fixtures`); a mismatch is a
compatibility regression, not a fixture that needs updating.

## Layout

- `manifest.json` — the checksum manifest: generator provenance plus one entry (path, size,
  SHA256, producer) per committed file.
- `stores/` — miniature store layouts, one directory per matrix cell. Each holds a version
  index (`.lvi`), a store index (`.lsi`, canonical and/or sha256-named shards), and the block
  tree (`chunks/<hex>/…​.lsb`). Cells cover the hash/compression/target-size matrix
  (`comp-none`, `comp-lz4`, `comp-zstd_min`/`comp-zstd_max`, `comp-brotli`/`comp-brotli_text`,
  `chunk-1024`/`chunk-131072`), a read-only `blake2` and `meow` set, a concurrent-upsync
  `sharded` store (exercises merge-on-read), and `default` (the v1→v2→v3 chain plus edge-case
  zoo over one store).
- `boundaries/` — chunk-boundary golden tables: for each input × target chunk size, the ordered
  chunk boundaries and their longtail chunk hashes. `.streaming.json` is the canonical path;
  `.buffer.json` is the labeled buffer/mmap variant (the two seed the rolling hash differently
  and can diverge — see `docs/format-spec.md`).
- `manifests/` — tree-manifests (path/size/permission snapshots) for the version chains and the
  sharded union, used to assert downsynced trees byte-for-byte.
- `get-configs/` — a get-config JSON plus its referenced store and indexes, driving the `get`
  command tests.
- `chunker.input` — the upstream chunker corpus, the primary chunk-boundary differential input.

## Verifying

```
cargo run -p xtask -- verify-fixtures
```

recomputes every file's SHA256 and checks it against `manifest.json`.
