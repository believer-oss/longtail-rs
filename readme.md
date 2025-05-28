# Rust Bindings for Longtail

These are Rust FFI [bindgen](https://github.com/rust-lang/rust-bindgen) binding of the [longtail](https://github.com/DanEngelbrecht/longtail) C library and a partial port of [golongtail](https://github.com/DanEngelbrecht/golongtail). Refer to the upstream repo for more information.

## Current state

These bindings have been successfully used for downsync from a [tauri](https://v2.tauri.app/) application. The bindings are not complete, and the API is not stable.

These are the APIs wrapped with at least partial abstractions:

[BlockIndex](src/longtaillib/block_index.rs)
[BlockStore](src/longtaillib/block_store.rs)
[Chunker](src/longtaillib/chunker.rs)
[Compression](src/longtaillib/compression.rs)
[Concurrent Chunk Worker](src/longtaillib/concurrent_chunk_write.rs)
[FileInfos](src/longtaillib/file_infos.rs)
[Hash](src/longtaillib/hash.rs)
[Job](src/longtaillib/job.rs)
[Logging](src/longtaillib/logging.rs)
[PathFilter](src/longtaillib/path_filter.rs)
[Progress](src/longtaillib/progress.rs)
[Storage](src/longtaillib/storage.rs)
[StoreIndex](src/longtaillib/store_index.rs)
[StoredBlock](src/longtaillib/stored_block.rs)
[VersionDiff](src/longtaillib/version_diff.rs)
[VersionIndex](src/longtaillib/version_index.rs)
