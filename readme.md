# Rust Bindings for Longtail

These are Rust FFI [bindgen](https://github.com/rust-lang/rust-bindgen) binding of the [longtail](https://github.com/DanEngelbrecht/longtail) C library and a partial port of [golongtail](https://github.com/DanEngelbrecht/golongtail). Refer to the upstream repo for more information.

## Current state

These bindings have been successfully used for downsync from a [tauri](https://v2.tauri.app/) application. The bindings are not complete, and the API is not stable.

These are the APIs wrapped with at least partial abstractions:

Primary data structures:

| Link | Description |
| --- | --- |
|[BlockIndex](src/longtaillib/block_index.rs)|Index of blocks of compressed chunks|
|[FileInfos](src/longtaillib/file_infos.rs)|Filesystem entry|
|[StoreIndex](src/longtaillib/store_index.rs)|Index of blocks stored in block storage|
|[StoredBlock](src/longtaillib/stored_block.rs)|Stored block, consisting of a Block Index and data|
|[VersionDiff](src/longtaillib/version_diff.rs)|Description of block and metadata changes between two versions|
|[VersionIndex](src/longtaillib/version_index.rs)|Index of chunks and metadata describing a version of a path|

Abstractions:

| Link | Description |
| --- | --- |
|[BlockStore](src/longtaillib/block_store.rs)|Abstract API for block storage|
|[Chunker](src/longtaillib/chunker.rs)|Abstract API for splitting files|
|[Compression](src/longtaillib/compression.rs)|Abstract API for compression libraries|
|[Concurrent Chunk Worker](src/longtaillib/concurrent_chunk_write.rs)|Abstract API for concurrent writes|
|[Hash](src/longtaillib/hash.rs)|Abstract API for hashing libraries|
|[Job](src/longtaillib/job.rs)|Abstract API for job scheduling|
|[Storage](src/longtaillib/storage.rs)|Abstract API for file storage|

Utilities:

| Link | Description |
| --- | --- |
|[Logging](src/longtaillib/logging.rs)|Logging utilities|
|[PathFilter](src/longtaillib/path_filter.rs)|Path filitering API|
|[Progress](src/longtaillib/progress.rs)|Progress API|
