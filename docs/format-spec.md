# Longtail On-Disk Format Specification

> Authoritative reference for the pure-Rust port's byte-compatibility work.
> Verified line-by-line against upstream C source at
> `~/github/longtail` (commit `96241fe`, 2025-06-29) and `~/github/golongtail`
> (commit `49a20e1`, 2025-06-29). File:function citations below are exact as of
> these commits; re-verify if upstream has since moved (this repo pins a
> specific upstream release via `UPSTREAM_VERSION` in
> `support/longtail-sys/build.rs` — check that version's tag/commit matches
> before trusting citations blindly on a version bump).

## Cross-cutting rules

- All multi-byte formats are little-endian raw memcpy of packed struct-of-arrays (SoA) data.
  There is **no alignment padding** — `u64` arrays may begin on 4-byte boundaries, so readers
  and writers must use unaligned loads/stores (never cast to `&[u64]`/transmute).
- No magic bytes in any format.
- No CRCs or checksums anywhere. Integrity is implicit: content/chunk hashes are recomputable,
  and decompression already validates the uncompressed-size field. `Longtail_ValidateStore`
  (longtail.h:1773 @96241fe) additionally checks that every required chunk is present and that
  reconstructed asset sizes match.
- All stored hashes are `TLongtail_Hash` = `u64` (hash function output truncated to 64 bits
  where the underlying algorithm produces more).
- Every format is a flat sequence of fixed-size scalar header fields first, followed by the
  variable-length arrays in a fixed order — arrays are never interleaved. VersionIndex and
  StoreIndex headers happen to be all-`u32`; BlockIndex's header leads with a `u64`
  (`m_BlockHash`) before its `u32` fields (§3) — don't assume "header = u32s" when porting.

## 1. VersionIndex (`.lvi`)

Verified against `Longtail_GetVersionIndexDataSize` (longtail.c:2552) and
`InitVersionIndexFromData` (longtail.c:2606).

### Header (6 × u32, in order)

| Field | Type | Semantics |
|---|---|---|
| `m_Version` | u32 | Format version. Current value `0x00000002` (`LONGTAIL_VERSION(0,0,2)`). Readers **reject** any other value (longtail.c:2633). |
| `m_HashIdentifier` | u32 | Hash algorithm ID used for all hashes in this index (§6). |
| `m_TargetChunkSize` | u32 | Target chunk size the version was chunked with (informational; not re-validated on read). |
| `m_AssetCount` (A) | u32 | Number of assets (files + directories). |
| `m_ChunkCount` (C) | u32 | Number of unique chunks referenced. |
| `m_AssetChunkIndexCount` (ACI) | u32 | Total length of the asset→chunk index map; `ACI >= C`. |

### Arrays (in order immediately following the header)

| Array | Element type | Length | Semantics |
|---|---|---|---|
| `m_PathHashes` | u64 | A | Hash of each asset's path string (§6, "path hash"). |
| `m_ContentHashes` | u64 | A | Hash of each asset's chunk-hash array (§6, "content hash"). |
| `m_AssetSizes` | u64 | A | Byte size of each asset; `0` for directories. |
| `m_AssetChunkCounts` | u32 | A | Number of chunks belonging to each asset. |
| `m_AssetChunkIndexStarts` | u32 | A | Start offset into `m_AssetChunkIndexes` for each asset. |
| `m_AssetChunkIndexes` | u32 | ACI | Flat map: index into `m_ChunkHashes`/`m_ChunkSizes` for each (asset, chunk-of-asset) pair. |
| `m_ChunkHashes` | u64 | C | Hash of each unique chunk's bytes (§6, "chunk hash"). |
| `m_ChunkSizes` | u32 | C | Byte length of each chunk. |
| `m_ChunkTags` | u32 | C | Compression ID (§4) the chunk's owning block was written with; `0` if unset (`memset` to 0 when no tags supplied — longtail.c:2797). |
| `m_NameOffsets` | u32 | A | Byte offset of each asset's path string within `m_NameData` (offsets are relative to the start of `m_NameData`, i.e. offset 0 = first byte of `m_NameData`). |
| `m_Permissions` | u16 | A | Permission bits (§7). |
| `m_NameData` | bytes | remaining bytes to EOF | Concatenated, **NUL-terminated** path strings in `m_NameOffsets` order (no padding between strings). `m_NameDataSize` is derived on read as `data_size - (offset of m_NameData)` (longtail.c:2702), i.e. it is *not* stored explicitly in the header — it is implicit in the total buffer length. |

Total data size (excluding the in-memory `Longtail_VersionIndex` struct header, which does not
exist on disk) is exactly the sum of the header and all arrays above —
`Longtail_GetVersionIndexDataSize(A, C, ACI, path_data_size)` (longtail.c:2552-2587). This is
also literally the on-disk file size of a `.lvi` file (the struct wrapper is a runtime
convenience with pointers into this buffer, never serialized).

Note: `struct Longtail_VersionIndex` contains commented-out `m_CreationDates` /
`m_ModificationDates` pointer fields (longtail.h:1868-1869) — these are NOT part of the
v0.0.2 on-disk format; do not reserve space for them.

### Path/name construction (`m_NameData` encoding)

Per-asset names are built by `Longtail_GetFilesRecursively2` (longtail.c:1806-1893) — the
scan path used by upsync and target scanning — or, for caller-supplied path lists, by
`LongtailPrivate_MakeFileInfos` (longtail.c:1382-1427). **Caveat**: the sorting and
directory-trailing-`/` behaviors described here and under "Path sort order" below are
implemented in `Longtail_GetFilesRecursively2` only; `LongtailPrivate_MakeFileInfos` performs
no sorting and no trailing-`/` insertion — it copies the caller's paths verbatim.

- Each entry is written as `strcpy` of the relative path (root-relative, `/`-separated,
  case-preserved) followed by a NUL.
- **Directories** get an extra trailing `/` byte inserted before the NUL
  (longtail.c:1873-1877): `<path>/\0`. Directories are otherwise ordinary assets in the index
  (see below) — there is no separate directory table.
- `m_NameOffsets[i]` is the cumulative byte offset (starting at 0) at which asset `i`'s string
  begins; offsets increase monotonically by `strlen(name) + 1` (+1 more for directories' `/`).

### Directory-as-asset representation

Directories appear as ordinary entries in every per-asset array:
- `m_AssetSizes[i] == 0`
- Path in `m_NameData` ends with `/` before the NUL
- `m_AssetChunkCounts[i] == 0` (directories have no chunks)
- `m_Permissions[i]` still carries real permission bits (see §7)

### Path sort order / comparator

Source: `SortScannedPaths` (longtail.c:1604-1632), driving `QSORT` over the flat, cross-folder
candidate list built by `Longtail_GetFilesRecursively2` (longtail.c:1845). Per-entry names are
the storage-relative path as built by `ScanFolder` (longtail.c:1435 ff, via
`storage_api->ConcatPath`, `/`-joined, no case folding).

- Comparator is a **plain, byte-wise `strcmp`** over the full relative path string
  (longtail.c:1631: `return strcmp(a_name, b_name);`). This is a single global sort across
  *all* scanned entries (not a per-directory sort merged afterwards) — result order is
  therefore pure lexicographic byte order over the whole `/`-joined relative path, `/` (0x2F)
  sorting as an ordinary byte among the other path bytes.
- **Case-sensitive** (no folding) — this is what the underlying OS/storage API surfaces, so
  behavior differs by platform storage backend, not by comparator logic. The comparator itself
  never normalizes case.
- **No separator normalization** in the comparator: whatever `ConcatPath` produces is compared
  verbatim. On POSIX, `Longtail_ConcatPath` always joins with `/`
  (longtail_platform.c:2316-2329); on Windows it defaults to `/`, falling back to `\` only
  when the accumulating path already contains one — which relative sub-paths built from entry
  names never do (longtail_platform.c:1180-1198). Cross-check: golongtail builds no paths of
  its own — `longtailutils/folderscanner.go` delegates entirely to the C
  `Longtail_GetFilesRecursively2` via cgo (folderscanner.go:31-35), normalizing only the root
  path — so Go and C are literally the same code path, and `/` is the on-disk/index separator
  universally, including on Windows; backslashes are not part of the stored format.
- Directories sort using their name **without** the trailing `/` appended for the sort key
  candidate itself — the sort operates on `properties->m_Name` which is the raw scanned name;
  the trailing `/` is only added afterward when writing `m_NameData` (longtail.c:1833-1841 vs
  1866-1877). A directory `"foo"` and a file `"foo"` cannot coexist, so this is not ambiguous
  in practice, but implementers should sort on the bare relative path (no synthetic trailing
  slash) to match exactly.

### Version constants & reader strictness

Defined at longtail.c:16-23:

```c
#define LONGTAIL_VERSION(major, minor, patch)  ((((uint32_t)major) << 24) | ((uint32_t)minor << 16) | ((uint32_t)patch))
#define LONGTAIL_VERSION_INDEX_VERSION_0_0_1  LONGTAIL_VERSION(0,0,1)
#define LONGTAIL_VERSION_INDEX_VERSION_0_0_2  LONGTAIL_VERSION(0,0,2)
#define LONGTAIL_STORE_INDEX_VERSION_1_0_0    LONGTAIL_VERSION(1,0,0)

uint32_t Longtail_CurrentVersionIndexVersion = LONGTAIL_VERSION_INDEX_VERSION_0_0_2; // 0x00000002
uint32_t Longtail_CurrentStoreIndexVersion = LONGTAIL_STORE_INDEX_VERSION_1_0_0;     // 0x01000000
```

Both `InitVersionIndexFromData` (longtail.c:2633) and `InitStoreIndexFromData` (longtail.c:9004)
reject on any version mismatch — there is no legacy-version read support at the format-parsing
layer (`0_0_1` is defined but never accepted by these readers; legacy handling, if any, lives
above this layer in `ChangeVersion` vs `ChangeVersion2`, which is a semantic — not format —
difference and out of scope for this format layer's decode/encode gate; the port does not
implement legacy `ChangeVersion` at all). A truncated buffer (declared
counts imply a data size larger than the actual buffer) is also rejected
(longtail.c:2663, longtail.c:9025).

v0.0.2 added the `m_ChunkTags` array over v0.0.1; the only version-downgrade handling anywhere
is diff code that special-cases `< 0_0_2` (longtail.c:7555, :7583 @96241fe), not a format-read
path — so this layer reads only VersionIndex v0.0.2 and StoreIndex v1.0.0.

### Caller path conventions (golongtail, informational — not format)

The `.lvi`/`.lsi` extensions and file locations are caller convention, not format. golongtail's
`put` writes the version index to `<parent>/version-data/version-index/<name>.lvi` and the
version-local store index to `<parent>/version-data/version-store-index/<name>.lsi`
(cmd_put.go:82, :87-88); `downsync` caches a target-folder index at
`<target>/.longtail.index.cache.lvi`. CLI defaults: hash `blake3`, compression `zstd`
(options.go:13, :17).

## 2. StoreIndex (`.lsi`)

Verified against `Longtail_GetStoreIndexDataSize` (longtail.c:8913) and
`InitStoreIndexFromData` (longtail.c:8979).

### Header (4 × u32, in order)

| Field | Type | Semantics |
|---|---|---|
| `m_Version` | u32 | `0x01000000` (`LONGTAIL_STORE_INDEX_VERSION_1_0_0`). Reject on mismatch. |
| `m_HashIdentifier` | u32 | Hash algorithm ID for all hashes in this index. |
| `m_BlockCount` (B) | u32 | Number of blocks described. |
| `m_ChunkCount` (C) | u32 | Total number of chunk entries across all blocks. |

### Arrays (in order)

| Array | Element type | Length | Semantics |
|---|---|---|---|
| `m_BlockHashes` | u64 | B | Hash identifying each block (§6, "block hash"). |
| `m_ChunkHashes` | u64 | C | Chunk hashes, **grouped per block, contiguous** — block `i`'s chunks occupy `[m_BlockChunksOffsets[i], m_BlockChunksOffsets[i] + m_BlockChunkCounts[i])`. |
| `m_BlockChunksOffsets` | u32 | B | Start offset into `m_ChunkHashes`/`m_ChunkSizes` for each block's chunk group. |
| `m_BlockChunkCounts` | u32 | B | Number of chunks in each block. |
| `m_BlockTags` | u32 | B | Compression ID (§4) each block was written with. |
| `m_ChunkSizes` | u32 | C | Uncompressed byte size of each chunk (parallel to `m_ChunkHashes`). |

Total data size = `Longtail_GetStoreIndexDataSize(B, C)` exactly (longtail.c:8913-8931); this is
the full on-disk `.lsi` file size.

### Naming & locking

Verified against `lib/fsblockstore/longtail_fsblockstore.c` and
`~/github/golongtail/remotestore/remotestore.go`:

- **Canonical index**: `<store_root>/store.lsi` (longtail_fsblockstore.c:167, 637, 1332).
- **Shards** (written by concurrent/optimistic writers before merge): S3/blob backends name
  shards `store_<sha256-hex-of-serialized-bytes>.lsi`, where the sha256 is over the exact
  serialized store-index byte buffer (`remotestore.go:1213-1214`,
  `key := fmt.Sprintf("store_%x.lsi", sha256)`; same pattern at `remotestore.go:1392-1393`).
- **Lock file** (fs backend only): `<store_root>/store.lsi.sync`
  (longtail_fsblockstore.c:1443), acquired via `Longtail_LockFile`
  (`lib/longtail_platform.c`) around the read-merge-write cycle
  (longtail_fsblockstore.c:623-637, 1175-1179, 1325-1334).
- **Shard discovery + merge** (remote/blob backends): readers list objects under prefix
  `store` and filter for suffix `.lsi` (remotestore.go:1678, :1700), then merge every
  discovered index — the canonical `store.lsi` (if present) and all `store_<sha256>.lsi`
  shards — via `Longtail_MergeStoreIndex` (longtail.c:9151). Merge-on-read is what makes the
  lockless shard-write scheme coherent.
- **Several shards are a steady state, not a race artifact.** A writer deletes only the shards
  it actually read and merged (`tryAddRemoteStoreIndex`, remotestore.go:1260-1297). It cannot
  delete a shard that appeared underneath it, because that file is a superset of *its own*
  additions and removing it would permanently lose those block-index entries. Two writers whose
  windows overlap therefore both leave a shard behind, and a store can sit at two or more
  `store_<sha256>.lsi` files indefinitely. A reader that consults only one of them sees a
  partial store, which is why merge-on-read is mandatory rather than an optimisation. Nothing
  in either implementation is aware of how the shards were grouped — any monthly or per-release
  arrangement is a pipeline convention layered on top.

## 3. StoredBlock (`.lsb`)

Verified against `Longtail_InitBlockIndex`/`Longtail_GetBlockIndexDataSize` (longtail.c:3585-
3637), `Longtail_InitBlockIndexFromData` (longtail.c:3654), and `Longtail_WriteStoredBlock`
(longtail.c:4194-4239). **No version field** — the block index header has no version u32 at all.

### On-disk layout

A `.lsb` file is the block-index data immediately followed by the block payload, with no
separator and no length prefix beyond what's implied by `m_ChunkCount`:

| Field | Type | Semantics |
|---|---|---|
| `m_BlockHash` | u64 | Hash of the block's chunk-hash array (§6, "block hash"). |
| `m_HashIdentifier` | u32 | Hash algorithm ID used to compute `m_BlockHash` / chunk hashes. |
| `m_ChunkCount` (n) | u32 | Number of chunks packed in this block. |
| `m_Tag` | u32 | Compression ID (§4) applied to the payload; `0` = stored raw. |
| `m_ChunkHashes` | u64[n] | Hash of each packed chunk, in payload order. |
| `m_ChunkSizes` | u32[n] | **Uncompressed** byte size of each packed chunk, in payload order. |
| payload | bytes | See below. |

`Longtail_GetBlockIndexDataSize(n)` (longtail.c:3585) gives the byte length of everything up to
and including `m_ChunkSizes`; the payload follows immediately
(`Longtail_WriteStoredBlock` writes the block-index bytes first, then
`stored_block->m_BlockData` for `stored_block->m_BlockChunksDataSize` bytes — longtail.c:4216-
4227). File size = block-index data size + `m_BlockChunksDataSize`.

### Payload

- **`m_Tag == 0`** (uncompressed): payload is the raw, concatenated bytes of all `n` chunks in
  order, total length `Σ m_ChunkSizes` — no header.
- **`m_Tag != 0`** (compressed): payload is
  `[uncompressed_size: u32][compressed_size: u32][compressed bytes]`
  (`CompressBlock`, `lib/compressblockstore/longtail_compressblockstore.c:67-141`):
  - `header_ptr[0] = block_chunk_data_size` — i.e. `uncompressed_size == Σ m_ChunkSizes`
    (compressblockstore.c:135).
  - `header_ptr[1] = compressed_chunk_data_size` (compressblockstore.c:136).
  - `compressed bytes` follow immediately, `compressed_size` bytes, produced by the codec
    identified by `m_Tag & the registry dispatch rule` (§4).
  - Decoding needs only `uncompressed_size` (to size the output buffer) and the codec
    identified by `m_Tag`; `compressed_size` lets a reader skip/validate trailing bytes.
    Decompression additionally verifies the decoded length equals `uncompressed_size`
    (`DecompressBlock`, compressblockstore.c:328-333 @96241fe) — the "no CRC, but implicit
    integrity" rule from the cross-cutting section.
  - Concrete compressed `.lsb` byte layout, end to end: `[block hash u64][hash id u32][chunk
    count u32][tag u32][chunk hashes n×u64][chunk sizes n×u32][uncompressed_size u32][compressed_size
    u32][codec bytes…]`.

### Path scheme (fs and remote/S3 stores — identical)

Verified against `GetBlockName`/`GetBlockPath` (`lib/fsblockstore/longtail_fsblockstore.c:66-
118`) and `getBlockPath` (`~/github/golongtail/remotestore/remotestore.go:1941`):

```
chunks/<top-4-hex-of-block_hash>/0x<16-lowercase-hex-of-block_hash>.lsb
```

Concretely (fsblockstore.c:66-92): the filename is built as `chunks/` + 4 hex nibbles taken
from the **top** bits of `block_hash` (bits 63..48, i.e. the same nibbles as the first 4 hex
digits of the full 16-digit representation) + `/0x` + the full 16-lowercase-hex-digit
`block_hash` + extension (`.lsb`). Both the fs C store and the Go remote store compute this
identically — sharding directory = leading 4 hex chars of the hash, filename = full hash with a
literal `0x` prefix. `BLOCK_NAME_LENGTH = 23` = 4 (dir nibbles) + 1 (`/`) + 2 (`0x`) + 16
(hash hex) (fsblockstore.c:40).

### fs-store write path & integrity behaviors

- **Write-then-rename**: blocks are first written to `<final block path>.<16-hex-unique-id>`
  (`GetUniqueExtension`, fsblockstore.c:44-64; `TMP_EXTENSION_LENGTH = 1+16`,
  fsblockstore.c:17), then renamed into place (fsblockstore.c:273-292). Stray
  `.lsb.<16hex>` files in a store are abandoned temp writes.
- **Path-derivation check on read**: the fs store re-derives the expected path from the parsed
  block's hash and compares it against the actual file path (fsblockstore.c:424-431) — part of
  the implicit-integrity model (no CRCs, see cross-cutting rules).

## 4. Compression IDs (`m_Tag` / block tag)

| Codec | ID (hex) | Notes |
|---|---|---|
| None | `0x00000000` | Raw payload, no header. |
| LZ4 | `0x6C7A3432` (`'l','z','4','2'`) | `LZ4_compress_fast`, acceleration 1. Single ID, no variants. |
| Zstd min | `0x7A746431` | Level 0. |
| Zstd default | `0x7A746432` | Level 3. golongtail's default codec. |
| Zstd max | `0x7A746433` | Level 22. |
| Zstd high | `0x7A746434` | Level 8 (`LONGTAIL_ZSTD_HIGH_COMPRESSION_LEVEL`, longtail_zstd.c:14,53-54). |
| Zstd low | `0x7A746435` | Upstream write-path oddity: `SettingsIDToCompressionSetting`'s `case LONGTAIL_ZSTD_LOW_COMPRESSION_TYPE` (longtail_zstd.c:55-56) is shadowed by the `#define` on line 22, so it resolves to the *type ID itself* (not a real level) — a nonsensical `ZSTD_compressCCtx` level argument. Irrelevant to compat: `zstd_low` is never actually written this way in practice — **golongtail's CLI maps both `zstd_high` and `zstd_low` compression-name requests to the `zstd_max` ID (`0x7A746433`) instead** (`longtailutils/longtailutils.go:468-471`), and regardless, decode (`ZStdCompressionAPI_Decompress`, longtail_zstd.c:144-166) never reads `settings_id`/level at all — it dispatches purely on the registry mask below, so every valid zstd ID decodes identically no matter which level (if any) produced it. |
| Brotli generic min/default/max | `0x62746C30` / `0x62746C31` / `0x62746C32` | |
| Brotli text min/default/max | `0x62746C61` / `0x62746C62` / `0x62746C63` | |

**Registry dispatch**: LZ4 is matched by exact ID equality (`compression_type !=
LONGTAIL_LZ4_DEFAULT_COMPRESSION_TYPE`, `lib/lz4/longtail_lz4.c:14`). Zstd and brotli variants
are dispatched by masking off the low byte and matching the 3-char-plus-NUL family prefix —
`(compression_type & 0xffffff00) != LONGTAIL_ZSTD_COMPRESSION_TYPE` (`lib/zstd/longtail_zstd.c:32`)
and the analogous check in `lib/brotli/longtail_brotli.c:41` — with the low byte (an ASCII digit
or letter: `'0'..'5'` for zstd, `'0'..'2'`/`'a'..'c'` for brotli) selecting the specific
min/default/max/high/low or generic/text variant within that family. This means decoding any of
the 5 zstd or 6 brotli IDs above routes to the same decoder regardless of which specific ID was
used to compress.

Byte-identical re-compression is a **deliberate non-gate**: block identity is the hash of the
chunk-hash array, not compressed bytes, so exact codec version/parameter parity across
zstd 1.5.6 / brotli 1.1.0 / lz4 1.10.0 is not required — only correct **decoding** of all IDs
above is a gate.

## 5. Hash IDs and hash-input definitions

Verified against `lib/blake2/longtail_blake2.c`, `lib/blake3/longtail_blake3.c`,
`lib/meowhash/longtail_meowhash.c`, and call sites in `src/longtail.c`.

### IDs (4 ASCII chars packed big-endian into a u32, i.e. `(a<<24)|(b<<16)|(c<<8)|d`)

| Hash | ID (hex) | ASCII | Digest | Notes |
|---|---|---|---|---|
| BLAKE2s | `0x626C6B32` | `"blk2"` | 8 bytes | `blake2s(out, 8, data, length, key=0, keylen=0)` — no key (`longtail_blake2.c:111`). Read-capable priority only in the Rust port. |
| BLAKE3 | `0x626C6B33` | `"blk3"` | 8 bytes | `blake3_hasher_init` → `_update` → `_finalize(hasher, out, 8)` — **first 8 bytes of the XOF output** (`longtail_blake3.c:41,62,76` and `:98-100`). golongtail/production default. |
| Meow | `0x6D656F77` | `"meow"` | 8 bytes | `MeowBegin(state, MeowDefaultSeed)` → `MeowAbsorb` → `MeowU64From(MeowEnd(state, 0), 0)` — **lane 0** of the 128-bit Meow output, default seed, x86 AES-NI only (`longtail_meowhash.c:21-37`). Verify-only in the Rust port (parse without ability to produce new meow-hashed data; clear error on write attempt). |

**Digest-bytes → u64 mapping**: for blake2/blake3, the 8 digest bytes are written directly into
the output `u64`'s memory (`blake2s(out_hash, sizeof(uint64_t), data, length, 0, 0)`,
longtail_blake2.c:111; `blake3_hasher_finalize` into the out pointer, longtail_blake3.c:76,
:100). The stored value is therefore the little-endian interpretation of the first 8 digest
bytes — equivalently, the on-disk index bytes for a hash field are exactly those 8 digest
bytes. Meow instead extracts lane 0 of its 128-bit state as a `u64` via
`MeowU64From(MeowEnd(&state, 0), 0)`.

### Hash-input definitions (what bytes are fed to the hash function)

| Name | Input | Call site |
|---|---|---|
| Chunk hash | The chunk's raw bytes (post-chunking, pre-compression) | `longtail.c:2089`, `:2197`, `:2270` (`hash_job->m_HashAPI->HashBuffer(..., chunk bytes ...)`) |
| Content hash | The asset's `m_ChunkHashes` sub-array (i.e. hash of `sizeof(u64) * chunk_count` bytes of already-computed chunk hashes for that asset, not the asset's raw data) | `longtail.c:2522` |
| Path hash | The path string, **`pathlen` bytes, no NUL terminator** included (caller passes `strlen`-derived length, not `strlen+1`) | `longtail.c:1269-1272` (`GetPathHashWithLength`) |
| Block hash | The block's `m_ChunkHashes` array (`sizeof(u64) * chunk_count` bytes) | `longtail.c:3756-3757` (`Longtail_CreateBlockIndex`) |

## 6. HPCDC Chunker

Source: `lib/hpcdcchunker/longtail_hpcdcchunker.c`. Also known upstream as "high-performance
content-defined chunking"; based on https://moinakg.wordpress.com/2013/06/22/high-performance-content-defined-chunking/.

### Constants

- Window size: **48 bytes** (`ChunkerWindowSize`, hpcdcchunker.c:12). This doubles as the
  absolute floor for `min_chunk_size` (`Longtail_HPCDCCreateChunker` asserts
  `params.min >= ChunkerWindowSize`, hpcdcchunker.c:139) and the size below which any remaining
  data becomes one final chunk (see boundary logic below).
- Rotate helper: `rotl32(x, r) = (x << r) | (x >> (32 - r))` (hpcdcchunker.c:220 @96241fe).
- Gear table: 256 `u32` entries, copied verbatim below (hpcdcchunker.c:22-95):

```c
static uint32_t hashTable[] = {
    0x458be752, 0xc10748cc, 0xfbbcdbb8, 0x6ded5b68,
    0xb10a82b5, 0x20d75648, 0xdfc5665f, 0xa8428801,
    0x7ebf5191, 0x841135c7, 0x65cc53b3, 0x280a597c,
    0x16f60255, 0xc78cbc3e, 0x294415f5, 0xb938d494,
    0xec85c4e6, 0xb7d33edc, 0xe549b544, 0xfdeda5aa,
    0x882bf287, 0x3116737c, 0x05569956, 0xe8cc1f68,
    0x0806ac5e, 0x22a14443, 0x15297e10, 0x50d090e7,
    0x4ba60f6f, 0xefd9f1a7, 0x5c5c885c, 0x82482f93,
    0x9bfd7c64, 0x0b3e7276, 0xf2688e77, 0x8fad8abc,
    0xb0509568, 0xf1ada29f, 0xa53efdfe, 0xcb2b1d00,
    0xf2a9e986, 0x6463432b, 0x95094051, 0x5a223ad2,
    0x9be8401b, 0x61e579cb, 0x1a556a14, 0x5840fdc2,
    0x9261ddf6, 0xcde002bb, 0x52432bb0, 0xbf17373e,
    0x7b7c222f, 0x2955ed16, 0x9f10ca59, 0xe840c4c9,
    0xccabd806, 0x14543f34, 0x1462417a, 0x0d4a1f9c,
    0x087ed925, 0xd7f8f24c, 0x7338c425, 0xcf86c8f5,
    0xb19165cd, 0x9891c393, 0x325384ac, 0x0308459d,
    0x86141d7e, 0xc922116a, 0xe2ffa6b6, 0x53f52aed,
    0x2cd86197, 0xf5b9f498, 0xbf319c8f, 0xe0411fae,
    0x977eb18c, 0xd8770976, 0x9833466a, 0xc674df7f,
    0x8c297d45, 0x8ca48d26, 0xc49ed8e2, 0x7344f874,
    0x556f79c7, 0x6b25eaed, 0xa03e2b42, 0xf68f66a4,
    0x8e8b09a2, 0xf2e0e62a, 0x0d3a9806, 0x9729e493,
    0x8c72b0fc, 0x160b94f6, 0x450e4d3d, 0x7a320e85,
    0xbef8f0e1, 0x21d73653, 0x4e3d977a, 0x1e7b3929,
    0x1cc6c719, 0xbe478d53, 0x8d752809, 0xe6d8c2c6,
    0x275f0892, 0xc8acc273, 0x4cc21580, 0xecc4a617,
    0xf5f7be70, 0xe795248a, 0x375a2fe9, 0x425570b6,
    0x8898dcf8, 0xdc2d97c4, 0x0106114b, 0x364dc22f,
    0x1e0cad1f, 0xbe63803c, 0x5f69fac2, 0x4d5afa6f,
    0x1bc0dfb5, 0xfb273589, 0x0ea47f7b, 0x3c1c2b50,
    0x21b2a932, 0x6b1223fd, 0x2fe706a8, 0xf9bd6ce2,
    0xa268e64e, 0xe987f486, 0x3eacf563, 0x1ca2018c,
    0x65e18228, 0x2207360a, 0x57cf1715, 0x34c37d2b,
    0x1f8f3cde, 0x93b657cf, 0x31a019fd, 0xe69eb729,
    0x8bca7b9b, 0x4c9d5bed, 0x277ebeaf, 0xe0d8f8ae,
    0xd150821c, 0x31381871, 0xafc3f1b0, 0x927db328,
    0xe95effac, 0x305a47bd, 0x426ba35b, 0x1233af3f,
    0x686a5b83, 0x50e072e5, 0xd9d3bb2a, 0x8befc475,
    0x487f0de6, 0xc88dff89, 0xbd664d5e, 0x971b5d18,
    0x63b14847, 0xd7d3c1ce, 0x7f583cf3, 0x72cbcb09,
    0xc0d0a81c, 0x7fa3429b, 0xe9158a1b, 0x225ea19a,
    0xd8ca9ea3, 0xc763b282, 0xbb0c6341, 0x020b8293,
    0xd4cd299d, 0x58cfa7f8, 0x91b4ee53, 0x37e4d140,
    0x95ec764c, 0x30f76b06, 0x5ee68d24, 0x679c8661,
    0xa41979c2, 0xf2b61284, 0x4fac1475, 0x0adb49f9,
    0x19727a23, 0x15a7e374, 0xc43a18d5, 0x3fb1aa73,
    0x342fc615, 0x924c0793, 0xbee2d7f0, 0x8a279de9,
    0x4aa2d70c, 0xe24dd37f, 0xbe862c0b, 0x177c22c2,
    0x5388e5ee, 0xcd8a7510, 0xf901b4fd, 0xdbc13dbc,
    0x6c0bae5b, 0x64efe8c7, 0x48b02079, 0x80331a49,
    0xca3d8ae6, 0xf3546190, 0xfed7108b, 0xc49b941b,
    0x32baf4a9, 0xeb833a4a, 0x88a3f1a5, 0x3a91ce0a,
    0x3cc27da1, 0x7112e684, 0x4a3096b1, 0x3794574c,
    0xa3c8b6f3, 0x1d213941, 0x6e0a2e00, 0x233479f1,
    0x0f4cd82f, 0x6093edd2, 0x5d7d209e, 0x464fe319,
    0xd4dcac9e, 0x0db845cb, 0xfb5e4bc3, 0xe0256ce1,
    0x09fb4ed1, 0x0914be1e, 0xa5bdb2c3, 0xc6eb57bb,
    0x30320350, 0x3f397e91, 0xa67791bc, 0x86bc0e2c,
    0xefa0a7e2, 0xe9ff7543, 0xe733612c, 0xd185897b,
    0x329e5388, 0x91dd236b, 0x2ecb0d93, 0xf4d82a3d,
    0x35b5c03f, 0xe4e606f0, 0x05b21843, 0x37b45964,
    0x5eff22f4, 0x6027f4cc, 0x77178b3c, 0xae507131,
    0x7bf7cabc, 0xf9c18d66, 0x593ade65, 0xd95ddf11,
};
```

### Derivation from target chunk size

`min_chunk_size` returned by the chunker API is always `ChunkerWindowSize` (48)
(`HPCDCChunker_GetMinChunkSize`, hpcdcchunker.c:332). Callers (longtail.c:1985-1987, used at
longtail.c:2111-2118) derive the actual chunker params from a caller-supplied
`target_chunk_size`:

```c
#define MIN_CHUNKER_SIZE(min_chunk_size, target_chunk_size) (((target_chunk_size / 8) < min_chunk_size) ? min_chunk_size : (target_chunk_size / 8))
#define AVG_CHUNKER_SIZE(min_chunk_size, target_chunk_size) (((target_chunk_size / 2) < min_chunk_size) ? min_chunk_size : (target_chunk_size / 2))
#define MAX_CHUNKER_SIZE(min_chunk_size, target_chunk_size) (((target_chunk_size * 2) < min_chunk_size) ? min_chunk_size : (target_chunk_size * 2))
```

i.e. `min = max(48, target/8)`, `avg = max(48, target/2)`, `max = max(48, target*2)`. Default
`target_chunk_size = 32768` (golongtail `commands/options.go:97`) → min=4096, avg=16384,
max=65536. Default target block size = 8388608 (8 MiB) and max chunks/block = 1024
(golongtail `commands/options.go:101,105`, and the same literals hardcoded at every
`CreateBlockStoreForURI` call site in `commands/cmd_*.go`).

### Discriminator

Computed once per chunker instance, in **f64** (`HPCDCDiscriminatorFromAvg`,
hpcdcchunker.c:126-129):

```c
static uint32_t HPCDCDiscriminatorFromAvg(double avg)
{
    return (uint32_t)(avg / (-1.42888852e-7*avg + 1.33237515));
}
```

Called as `HPCDCDiscriminatorFromAvg((double)params->avg)` — the integer `avg` param is
widened to `double` before division. This exact expression (including operator/precedence and
the f64 width) must be reproduced bit-for-bit in Rust; any deviation (e.g. f32, or a differently
associated expression) risks drifting the boundary decision by ±1 on the discriminator, which
is fatal to compat.

### Boundary / rolling-hash loop

Source: `Longtail_HPCDCNextChunk` (hpcdcchunker.c:224-307).

1. If remaining unconsumed bytes (`left = buf.len - off`) is `<= params.min`: emit all of
   `left` as one final chunk, no hashing (hpcdcchunker.c:258-263).
2. Otherwise, seed the rolling hash over the **window immediately preceding `params.min`**:
   `window = scoped_data[params.min - ChunkerWindowSize .. params.min)` (48 bytes). For each
   byte `b` at window-relative index `i` (`i` from 0 to 47):
   `hash ^= rotl32(hashTable[b], (48 - i - 1) & 31)`; the window ring buffer (`c->hWindow`) is
   seeded with these same 48 bytes in order (hpcdcchunker.c:266-278).
3. Roll forward byte-by-byte from `pos = params.min` up to
   `data_len = min(scoped_data.len, params.max)`:
   - `in = scoped_buf[pos]`, `pos += 1`
   - `out = window[idx]` (the byte about to be evicted from the 48-byte ring at position `idx`),
     then `window[idx] = in; idx += 1` (wrapping to 0 at 48)
   - `hash = rotl32(hash, 1) ^ rotl32(hashTable[out], 16) ^ hashTable[in]` — note the rotate
     amount for the evicted byte's table entry is the **constant** `ChunkerWindowSize & 31 = 16`
     (hpcdcchunker.c:292-294), not related to loop position (unlike the seed loop's per-index
     rotate).
   - Boundary hit when `hash % d == d - 1`, where `d = c->hDiscriminator` (hpcdcchunker.c:297-
     300) — break the loop immediately (this occurrence of `pos` is included in the chunk;
     the loop already incremented `pos` past `in` before testing).
   - If `idx` reaches 48, wrap to 0.
4. The chunk emitted is `scoped_buf[0..pos)` (relative to the current scope start), and
   `c->off += pos` advances the cursor for the next call.

### Two entry points: streaming vs. buffer (mmap) — boundaries can DIVERGE

> Verified against the C source: the two paths do **not** always produce identical boundaries
> (a claim earlier drafts of this spec got wrong).

The chunker API has two distinct next-chunk implementations:

- **Streaming**: `Longtail_HPCDCNextChunk` (hpcdcchunker.c:224-307) — the feeder-driven path
  described by the boundary loop above. Seeds the rolling hash over the 48 bytes **immediately
  preceding `params.min`**: `scoped_data[params.min - 48 .. params.min)`
  (hpcdcchunker.c:269-278).
- **Buffer/mmap**: `HPCDCChunker_NextChunkFromBuffer` (hpcdcchunker.c:452-534) — a separate
  function registered as its own chunker-API entry (hpcdcchunker.c:544) and invoked by the
  memory-mapped-file path (longtail.c:2182). Same gear table, discriminator, and roll loop —
  but it seeds the rolling hash over the **first 48 bytes of the scope**: `buf[0..48)`
  (hpcdcchunker.c:489-494).

Both paths start rolling from `pos = params.min`. Because the rolling-hash state only fully
converges 48 rolled bytes after seeding, boundary tests at positions `(min, min + 47]` can
disagree between the two paths whenever `params.min > 48` — which is effectively always
(default `min` = 4096). Expected divergence is roughly `47/d` per chunk (~0.4% at default
sizes) when file mapping is enabled.

**Port-parity decision**: golongtail's `--enable-file-mapping` defaults to **false**
(`commands/options.go:116-118`), so production `.lvi` data was chunked by the **streaming**
path — streaming seed-window semantics are canonical for the Rust port. Implement ONE boundary
algorithm (streaming semantics) regardless of I/O mechanism; the buffer path's differing seed
window is an upstream quirk, replicated only as a labeled variant for differential testing
against C-with-mmap (the committed boundary tables record both paths).

### Streaming refill contract

The streaming path's internal feed buffer is `max_feed = params.max * 4` (clamped to u32 max;
hpcdcchunker.c:148-152). Within the streaming path, the refill buffer size affects only I/O
batching — never boundary placement.

### Golden input

`longtail/test/testdata/chunker.input`, exactly 1 MiB — the standard upstream fixture for
boundary differential testing (committed under `fixtures/chunker.input`).

## 7. Permission bits

Source: `lib/longtail_platform.c` (`Longtail_GetEntryProperties`, POSIX at line 2149, Windows
at line 801) and bit constants in `src/longtail.h:314-324`.

### Bit layout (u16, only the low 9 bits used)

```c
Longtail_StorageAPI_OtherExecuteAccess  = 0001,  // 0x001
Longtail_StorageAPI_OtherWriteAccess    = 0002,  // 0x002
Longtail_StorageAPI_OtherReadAccess     = 0004,  // 0x004
Longtail_StorageAPI_GroupExecuteAccess  = 0010,  // 0x008
Longtail_StorageAPI_GroupWriteAccess    = 0020,  // 0x010
Longtail_StorageAPI_GroupReadAccess     = 0040,  // 0x020
Longtail_StorageAPI_UserExecuteAccess   = 0100,  // 0x040
Longtail_StorageAPI_UserWriteAccess     = 0200,  // 0x080
Longtail_StorageAPI_UserReadAccess      = 0400,  // 0x100
```

These are exactly the standard POSIX `rwxrwxrwx` bit positions (octal `0400`/`0200`/... equal
`S_IRUSR`/`S_IWUSR`/... numerically) packed into the low 9 bits of a `u16`; bits 9-15 are
unused/reserved (always 0 in practice).

### POSIX mapping

`*out_permissions = (uint16_t)(stat_buf.st_mode & 0x1FF)` (longtail_platform.c:2162) — a direct
mask of `st_mode`'s low 9 bits. No translation needed; the on-disk value **is** the POSIX mode's
permission bits.

### Windows mapping

Windows has no POSIX mode bits, so `Longtail_GetEntryProperties` (longtail_platform.c:801-822)
synthesizes a value:
- Base: read access for user+group+other always set
  (`UserReadAccess | GroupReadAccess | OtherReadAccess`).
- If `FILE_ATTRIBUTE_DIRECTORY`: also set execute for user+group+other.
- If `FILE_ATTRIBUTE_READONLY` is **not** set: also set write for user+group+other.
- Windows therefore only ever produces one of two effective values per entry:
  read-only file → `0444`; writable file → `0666`; directory (always "executable" +
  read, write depends on read-only attribute) → `0555` or `0777`. There's no user/group/other
  distinction on Windows — the three triplets are always identical.

Write-back (`chmod(path, permissions)`, longtail_platform.c:2242) applies the low 9 bits
directly on POSIX; the platform layer's Windows write path degrades this back down to the
read-only attribute only (setting/clearing `FILE_ATTRIBUTE_READONLY` based on whether any write
bit is set) — fine-grained user/group/other distinctions are lossy round-tripping through
Windows storage, which is expected/accepted upstream behavior, not a compatibility gap for
this port.

## 8. ArchiveIndex (`.la`) — header shape only

Full spec deferred to the archive feature (feature-gated and droppable — archives are not
currently used). For orientation, the top-level struct
(`src/longtail.h` ~1883-1890):

```c
struct Longtail_ArchiveIndex
{
    uint32_t* m_Version;
    uint32_t* m_IndexDataSize;
    struct Longtail_StoreIndex m_StoreIndex;
    uint64_t* m_BlockStartOffets;
    uint32_t* m_BlockSizes;
    struct Longtail_VersionIndex m_VersionIndex;
};
```

i.e. `[m_Version u32][m_IndexDataSize u32]` followed by an embedded `StoreIndex` (§2 layout,
minus its own outer file-level framing), a `u64[[block count]]` array of block start offsets
into the archive's block-data section, a parallel `u32[[block count]]` array of block sizes,
and an embedded `VersionIndex` (§1 layout). Current archive format version:
`LONGTAIL_ARCHIVE_VERSION_0_0_1 = 0x00000001` (`Longtail_CurrentArchiveVersion`,
longtail.c:20, :24). Do not treat this as complete — block-data section placement,
`m_IndexDataSize` semantics, and pack/unpack behavior are unverified and out of scope until
the archive feature is implemented.

## 9. Edge cases

- **Empty index** (`asset_count == 0` / `block_count == 0` / `chunk_count == 0`): the full
  fixed-size header is still written; all array sections are simply zero-length. Readers must
  not special-case a zero count as "absent header" — `Longtail_GetVersionIndexDataSize`/
  `Longtail_GetStoreIndexDataSize` compute a valid (small) positive size even when every count
  is 0.
- **Chunking is per-RANGE, not per-asset**: `ChunkAssets` splits every asset into
  independent chunking jobs of `max_hash_size = target_chunk_size * 1024` bytes
  (longtail.c:2397-2404; part loop :2434-2460; `asset_part_count = 1 + size/max_hash_size`,
  a trailing zero-size range yields zero chunks). Each range gets its own chunker session —
  **boundaries reset at range boundaries**. At the default target (32768) ranges are
  32 MiB, so only large files hit the split; at target 1024 the split is 1 MiB (live in the
  committed `chunk-1024` fixtures via the 2 MiB `repetitive.bin`).
- **Ranges ≤ 48 bytes = single chunk**: enforced in two places, both worth porting
  faithfully:
  - Primary rule, per range: the hashing job (`DynamicChunking`, longtail.c:2035-2051)
    calls `GetMinChunkSize` (always 48) and, `if (hash_size <= chunker_min_size)` — where
    `hash_size` is the **range** size (`m_SizeRange`) — reads the entire range and hashes
    it as one chunk **without ever invoking the chunker/gear table at all**. For files
    smaller than one range (the overwhelmingly common case) this reads as "files ≤ 48
    bytes never get chunked".
  - General tail rule, inside the chunker itself: `Longtail_HPCDCNextChunk` never
    seeds/rolls the hash when the remaining unconsumed bytes `left <= params.min`
    (hpcdcchunker.c:257-263) — it just emits the remainder as one final chunk. This is a
    broader rule about chunk *tails*, not specific to 48 bytes: `params.min` can be larger
    than 48 for large target chunk sizes (`min = max(48, target/8)`), so a multi-KB tail can
    also be swallowed whole by this path.
- **Misaligned u64 arrays (odd counts)**: because there is no padding anywhere in `.lvi`/`.lsi`/
  `.lsb`, a `u64` array following an odd number of preceding `u32`s (or an odd-length `u16`
  array as in `m_Permissions`) is **not guaranteed 8-byte aligned** in the mapped/read buffer.
  Implementations must read/write these fields with unaligned-safe primitives
  (`from_le_bytes`/byte-cursor, never a `&[u64]` reinterpret cast or `bytemuck`/zerocopy cast
  of the raw buffer) — this is a known UB risk and is why the fixture corpus commits
  odd-asset-count cases.
- **Reader version strictness**: every format's version field is checked for **exact equality**
  against the single current constant; there is no "accept older, upgrade on write" behavior at
  this layer (see §1 "Version constants & reader strictness" — this applies identically to
  StoreIndex).
- **Truncated buffers**: both `InitVersionIndexFromData` and `InitStoreIndexFromData` compute
  the expected data size from the header's counts and compare against the actual buffer length
  before touching the arrays, rejecting (`EBADF`) if the buffer is smaller than the header
  implies. `InitBlockIndexFromData` does the same check but has no version field to validate
  first.

## 10. Golden-file test matrix

The compat suites exercise every claim above against the committed `fixtures/` (produced by the
pinned upstream golongtail CLI). The load-bearing checks:

1. Round-trip parse + re-serialize each real `.lvi`/`.lsi`/`.lsb` → byte-identical (validates
   §§1–3, little-endian packing, and unaligned handling).
2. Recompute block hashes from the parsed chunk-hash arrays → equals the stored block hash and
   the encoded block path (§3 naming, §5 block hash).
3. Recompute path/content hashes from a `.lvi` → equal the stored values, for each of
   blake3/blake2/meow (§5).
4. Re-chunk a known input at target 32768 with blake3 → identical chunk boundaries + chunk
   hashes vs a golongtail-produced `.lvi` (§6). Include an input larger than `max × 4` (refill)
   and a `< 48`-byte file (single-chunk rule).
5. Decompress each `.lsb` payload per `m_Tag` and confirm the reconstructed size = Σ chunk
   sizes (§3, §4, integrity model).
