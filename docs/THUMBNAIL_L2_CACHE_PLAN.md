# CBXShell Persistent Thumbnail L2 Cache Plan

## Status

Implementation plan. This document defines the intended architecture and sequencing for adding a persistent CBXShell-owned thumbnail cache behind the Windows Shell thumbnail cache.

## Motivation

CBXShell currently relies entirely on the Windows thumbnail cache. When Windows invalidates or removes `thumbcache_*.db`, Explorer invokes `IThumbnailProvider::GetThumbnail` again and CBXShell has to reopen the archive, locate the cover image, extract it, decode it, resize it, and create an `HBITMAP`.

That is unnecessarily expensive for large comic collections, especially when the source archive is large or the selected image uses an expensive codec such as AVIF or WebP.

The target workload includes libraries containing tens of thousands of archives. The cached representation must therefore balance storage size, decode speed, and preview quality.

CBXShell thumbnails are also used by Windows preview surfaces such as the Explorer preview pane (`Alt+P`), so a cache designed only around 96/256 px icon thumbnails is insufficient.

## Goals

1. Add a persistent CBXShell-owned L2 thumbnail cache under `%LOCALAPPDATA%`.
2. Avoid archive access on a Windows thumbnail-cache miss when the source archive has not changed.
3. Store one canonical preview proxy per archive variant rather than one file per Explorer-requested `cx`.
4. Preserve enough image quality for Explorer preview-pane usage.
5. Keep storage practical for libraries containing tens of thousands of archives.
6. Keep the COM thumbnail hot path free of database access, directory-wide scans, global cache-size calculations, and network operations.
7. Support deterministic invalidation when the archive, cover-selection policy, renderer behavior, or cache generation changes.
8. Add GUI-visible cache management and explicit thumbnail regeneration.
9. Make regeneration rebuild both CBXShell L2 and Windows L1 thumbnail state.
10. Treat the cache strictly as a performance layer: cache failures must never make thumbnail extraction fail.

## Non-goals

1. Do not replace the Windows thumbnail cache.
2. Do not introduce SQLite or another shared metadata database into the shell extension hot path.
3. Do not hash entire archive contents to determine cache validity.
4. Do not cache one proxy for every requested Explorer thumbnail size.
5. Do not make cache maintenance synchronous with `IThumbnailProvider::GetThumbnail`.
6. Do not add background/network activity to Explorer or COM thumbnail callbacks.
7. Do not implement rename tracking through NTFS file IDs in the first version.
8. Do not implement negative-result caching in the first version unless real-world profiling shows it is necessary.

---

# 1. Overall Cache Hierarchy

The intended cache hierarchy is:

```text
Explorer / Shell
    |
    v
Windows thumbnail cache                         L1
    | cache miss
    v
CBXShell IThumbnailProvider::GetThumbnail(cx)
    |
    +--> Resolve stable source identity
    |
    +--> CBXShell persistent canonical proxy     L2
    |       | hit
    |       v
    |    Decode canonical proxy
    |       |
    |    Resize to requested cx if required
    |       |
    |    Create HBITMAP
    |
    +--> miss
            |
            v
         Open archive
            |
         Select cover image
            |
         Extract source image
            |
         Decode source image
            |
         Build canonical preview proxy
            |
         Persist L2 entry (best effort)
            |
         Resize to requested cx if required
            |
         Create HBITMAP
```

The expected steady-state behavior after Windows clears its own thumbnail DB is therefore:

```text
Windows L1 miss
    -> CBXShell L2 hit
    -> decode small canonical proxy
    -> resize if needed
    -> HBITMAP
```

No archive read should occur on an L2 hit.

---

# 2. Canonical Proxy Strategy

## 2.1 One proxy per archive variant

Do not persist thumbnails keyed by `cx`.

Instead, persist one canonical cover proxy for each valid source/configuration variant. Explorer requests such as 96, 256, 512, or 1024 px should all be derived from that same canonical proxy.

This prevents duplicate persistent data and keeps preview-pane quality independent of whichever thumbnail size Explorer requested first.

## 2.2 Maximum dimension

Initial design target:

```text
canonical maximum dimension = 1024 px
```

Rules:

- Preserve aspect ratio.
- Never upscale the source image.
- If the source cover is already smaller than 1024 px on its longest edge, store it at its natural decoded size after the existing background/compositing policy is applied.
- If it is larger, downscale to a maximum 1024 px edge.

Examples:

```text
2400 x 3600 -> 683 x 1024
1200 x 800  -> 1024 x 683
300 x 450   -> 300 x 450
```

## 2.3 Cache image codec

Preferred initial codec:

```text
lossy WebP, approximately quality 90
```

The exact encoder and quality level must be benchmarked with representative comic covers before being finalized.

Reasons:

- Tens of thousands of entries make raw BGRA or QOI too large as the primary persistent representation.
- JPEG is viable but WebP should provide a better size/quality trade-off for a mixed workload containing artwork, line art, text, and photographic material.
- AVIF should not be used as the cache codec because decode latency is contrary to the purpose of the L2 cache.
- The source image format must not dictate the cache format. JPEG, PNG, WebP, AVIF, BMP, TIFF, etc. all converge to the same canonical intermediate representation.

The codec implementation must be abstracted enough that the cache format can be revised later without changing shell/cache ownership rules.

Suggested API shape:

```rust
pub trait PreviewCacheCodec {
    fn encode(image: &RenderedThumbnail) -> Result<Vec<u8>>;
    fn decode(data: &[u8]) -> Result<DecodedPreview>;
}
```

The first implementation can be WebP, with JPEG as a fallback if benchmarking or dependency constraints make WebP encoding unsuitable.

---

# 3. Thumbnail Rendering Refactor

The current thumbnail pipeline returns an `HBITMAP` directly. L2 persistence needs access to the rendered pixel image before it is converted into a Windows GDI object.

Refactor the thumbnail pipeline to expose a renderer-level result.

Suggested shape:

```rust
pub struct RenderedThumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn render_thumbnail(
    image_data: &[u8],
    config: ThumbnailConfig,
) -> Result<RenderedThumbnail>;

impl RenderedThumbnail {
    pub fn to_hbitmap(&self) -> Result<HBITMAP>;
}
```

Keep the existing public API as a compatibility wrapper:

```rust
pub fn create_thumbnail(
    image_data: &[u8],
    config: ThumbnailConfig,
) -> Result<HBITMAP> {
    render_thumbnail(image_data, config)?.to_hbitmap()
}
```

The canonical proxy must be produced after the same visible rendering policy used for the returned thumbnail, including current alpha/background handling.

---

# 4. Source Identity

## 4.1 Do not hash archive contents

Cache lookup must remain O(metadata), not O(file size).

Never hash entire CBZ/CBR/CB7/ZIP/RAR/7z files as part of cache lookup.

## 4.2 Identity fields

The source identity should contain enough metadata to distinguish unchanged and modified files:

```rust
pub struct SourceIdentity {
    pub locator: StableSourceLocator,
    pub size: u64,
    pub modified_filetime: u64,
}
```

The locator should normally represent the absolute Windows path.

Do not hash `Path` using `to_string_lossy()`. Hash Windows path data as UTF-16 code units so that non-UTF-8-representable Windows paths are not collapsed or modified.

## 4.3 IInitializeWithFile / IInitializeWithItem

Use the resolved filesystem path plus file metadata.

## 4.4 IInitializeWithStream

Use `IStream::Stat()` before reading archive content.

Capture at least:

- `pwcsName` when present and path-like
- `cbSize`
- `mtime`

If a stable source locator cannot be resolved, bypass L2 in v1 and use the normal archive path.

Correctness is more important than forcing caching for anonymous streams.

Log the bypass in debug mode, for example:

```text
L2 CACHE BYPASS: stream has no stable source identity
```

Do not introduce content probing as a fallback in the first implementation.

---

# 5. Cache Identity and Invalidation

Use separate source and variant identities so that specific archives can be invalidated efficiently.

## 5.1 Source ID

```text
source_id = SHA-256(normalized absolute path encoded as UTF-16)
```

The source ID identifies the logical archive location and is used as the directory name.

## 5.2 Variant ID

The variant ID identifies one valid rendering of that source.

Suggested fields:

```rust
pub struct CacheVariant {
    pub source_size: u64,
    pub source_mtime: u64,

    pub sort_images: bool,

    pub renderer_revision: u32,
    pub cache_generation: u64,
    pub proxy_max_dimension: u16,
    pub cache_codec_revision: u16,
}
```

Then:

```text
variant_id = SHA-256(serialized CacheVariant)
```

`cx` must not be part of the key.

The original source image codec must not be part of the key. A source format change necessarily changes file metadata and therefore creates a different variant.

## 5.3 Renderer revision

Introduce an explicit renderer revision independent of the application version.

```rust
const RENDERER_REVISION: u32 = 1;
```

Increment it only when rendering semantics change in a way that should invalidate cached visual output, for example:

- cover-selection semantics
- sorting semantics
- resize algorithm
- background/compositing policy
- color-management behavior

Do not invalidate all thumbnails merely because CBXShell itself is upgraded.

## 5.4 Cache generation

Introduce a user-visible/global logical generation value stored in configuration, for example:

```text
HKCU\Software\CBXShell-rs\{...}\CacheGeneration
```

A full cache regeneration can increment this value in O(1) time.

Old generations become unreachable immediately and are later deleted by background maintenance.

---

# 6. Filesystem Layout

Recommended layout:

```text
%LOCALAPPDATA%\CBXShell\thumbnail-cache\v1\

  4a\
    4a82203c...source-id\
        a19432...variant.webp

  c7\
    c7a123ab...source-id\
        f91382...variant.webp
```

The first two hex characters shard top-level directories and avoid putting tens of thousands of entries into one directory.

Advantages of the source-directory/variant-file split:

- Invalidating one archive means deleting one source directory.
- Old variants caused by file changes can coexist temporarily and be removed later by maintenance.
- No central index is required.
- No database corruption/locking mode exists.

---

# 7. Cache Storage API

Add a dedicated workspace crate, tentatively:

```text
crates/cbxshell-thumbnail-cache/
```

Suggested modules:

```text
src/
  lib.rs
  key.rs
  store.rs
  config.rs
  maintenance.rs
```

Core API should remain platform-light where practical.

Example surface:

```rust
pub struct ThumbnailCache {
    ...
}

impl ThumbnailCache {
    pub fn load(&self, source: &SourceIdentity, variant: &CacheVariant)
        -> Result<Option<Vec<u8>>>;

    pub fn store(
        &self,
        source: &SourceIdentity,
        variant: &CacheVariant,
        encoded_proxy: &[u8],
    ) -> Result<()>;

    pub fn invalidate_source(&self, path: &Path) -> Result<()>;

    pub fn clear_all_physical(&self) -> Result<()>;

    pub fn maintenance(&self, limits: CacheLimits) -> Result<MaintenanceReport>;
}
```

Cache errors must be handled as soft failures by the shell extension.

---

# 8. Cache Hit Path

The `extract_thumbnail_internal(cx)` flow should be reorganized approximately as follows:

```text
1. Normalize requested size.
2. Read cover-selection/sort settings required by cache identity.
3. Resolve SourceIdentity without reading archive content.
4. Build CacheVariant.
5. Attempt L2 load.
6. If hit:
   a. Decode canonical proxy.
   b. Resize to cx if needed.
   c. Convert to HBITMAP.
   d. Return.
7. If corrupt:
   a. Delete the corrupt cache entry best-effort.
   b. Continue as cache miss.
8. On miss:
   a. Execute normal archive extraction.
   b. Render canonical proxy at max 1024 px.
   c. Encode and store L2 best-effort.
   d. Derive requested cx from canonical rendered image.
   e. Return HBITMAP.
```

The key acceptance condition is:

```text
L2 HIT => no archive open and no archive Read() calls
```

---

# 9. Cache Write Semantics and Concurrency

Do not introduce a global mutex, named mutex, or database lock for cache writes.

Duplicate work is preferable to contention inside a shell extension.

Use immutable final files and atomic publication:

```text
Thread/process A -> <variant>.<pid>.<nonce>.tmp
Thread/process B -> <variant>.<pid>.<nonce>.tmp

A atomically publishes <variant>.webp
B sees destination already exists and discards its temp file
```

Rules:

- Write to a temporary file in the destination directory.
- Flush/close it.
- Atomically rename/move into the final filename where possible.
- Treat destination-already-exists as success.
- Best-effort delete abandoned temp files during maintenance.

Cache write failure must not affect the already-generated `HBITMAP` result.

---

# 10. Corrupt Cache Recovery

A cache entry is disposable.

If decode fails:

```text
cache file exists
    -> decode fails
    -> delete cache file best-effort
    -> normal archive extraction
    -> store replacement
```

Do not surface a broken-cache error to Explorer if the archive itself can still produce a thumbnail.

---

# 11. Capacity and Maintenance

## 11.1 Default capacity

The target workload includes tens of thousands of comic archives, so a 1 GiB default is too small.

Initial GUI/default proposal:

```text
2 GiB
4 GiB
8 GiB   <- default
16 GiB
32 GiB
Unlimited
```

The exact default can be revisited after measuring real canonical WebP sizes.

## 11.2 No maintenance in GetThumbnail

`IThumbnailProvider::GetThumbnail` must not:

- recursively enumerate the cache
- compute total cache size
- update a global LRU database
- delete old entries unrelated to the requested source

## 11.3 Background maintenance

CBXManager already has a per-user login background execution mode for update checks. Extend manager-owned background work to run cache maintenance there as well.

Maintenance should:

- remove stale temporary files
- remove physically unreachable old cache generations
- enforce configured maximum cache size
- preferentially remove oldest entries first

A simple filesystem timestamp-based eviction policy is sufficient for v1.

Avoid building an exact persistent LRU database unless profiling proves the filesystem-only approach inadequate.

When over capacity, delete enough old entries to fall meaningfully below the maximum (for example approximately 90%) rather than repeatedly deleting one file at a time.

---

# 12. GUI Cache Management

Add cache management to CBXManager rather than expanding shell-extension responsibilities.

Suggested Tools menu structure:

```text
Tools
  Register DLL
  Unregister DLL
  ----------------
  Check for Updates
  ----------------
  Thumbnail Cache...
  Regenerate Thumbnails...
  ----------------
  About
```

## 12.1 Thumbnail Cache dialog

Display at least:

```text
Thumbnail Cache

Location
C:\Users\...\AppData\Local\CBXShell\thumbnail-cache

Size
4.7 GB / 8 GB

Entries
31,482

Maximum cache size
[ 8 GB v ]

[Clear CBXShell Cache]
```

`Clear CBXShell Cache` means physical L2 deletion/storage reclamation. It must not silently trigger regeneration of tens of thousands of archives.

## 12.2 Clear Cache vs Regenerate

These are distinct operations and must remain distinct in the UI.

### Clear CBXShell Cache

Purpose: reclaim storage.

Behavior:

```text
Delete/retire CBXShell L2 data.
Do not automatically scan the library.
Do not automatically warm Windows L1.
```

### Regenerate Thumbnails

Purpose: intentionally rebuild thumbnails after cover-selection/rendering changes or when the user explicitly requests refresh.

Behavior:

```text
Invalidate CBXShell L2 for the selected scope.
Force Windows thumbnail extraction.
Warm both L2 and Windows L1.
```

---

# 13. Regenerate Workflow

## 13.1 Windows API behavior

Regeneration must not depend on deleting `thumbcache_*.db` and restarting Explorer.

Use Windows thumbnail cache APIs to force extraction for selected files.

Target flow:

```text
1. Invalidate the relevant CBXShell L2 source entry.
2. Create an IShellItem for the archive.
3. Invoke IThumbnailCache::GetThumbnail with force-extraction semantics.
4. Windows invokes CBXShell IThumbnailProvider again.
5. CBXShell sees an L2 miss.
6. CBXShell extracts and renders the archive cover.
7. CBXShell writes the new canonical L2 proxy.
8. CBXShell returns the requested HBITMAP.
9. Windows stores the newly extracted thumbnail in its own L1 cache.
```

Do not use the flag variant that explicitly prevents Windows from caching the result.

The result of regeneration should therefore be:

```text
CBXShell L2 = warm
Windows L1  = warm
```

## 13.2 Regenerate selected folder

Suggested UX:

```text
Regenerate Thumbnails

Folder: D:\Comics\...
[x] Include subfolders

Files found: 24,319

[Regenerate]
```

During execution:

```text
Regenerating thumbnails

8,421 / 24,319
[progress bar]

D:\Comics\...\foo.cbz

[Cancel]
```

Implementation rules:

- Enumerate only currently supported/registered archive extensions.
- Invalidate each source ID before force extraction.
- Run generation outside the shell-extension process, owned by CBXManager.
- Bound parallelism. Do not issue unbounded thousands of simultaneous Shell extraction calls.
- Prefer conservative worker concurrency and benchmark on HDD, SSD, and network-backed libraries before raising it.
- Cancellation must stop scheduling new work cleanly.
- A failed file should be reported and skipped without aborting the whole batch.

## 13.3 Regenerate all

For a global logical invalidation, increment `CacheGeneration` first.

This makes every previous L2 entry stale immediately without synchronously deleting a multi-gigabyte directory.

Then, if the user explicitly requested active regeneration/prewarming, enumerate the selected library roots and force extraction.

Physical deletion of previous generations belongs to background maintenance.

## 13.4 Per-file/folder invalidation

Do not bump the global generation when regenerating a limited scope.

Instead delete the `source_id` directory for each selected archive and force extraction for those files.

---

# 14. Library Root Handling for Regenerate All

CBXShell currently does not maintain a library catalog.

Do not silently invent one as part of L2 v1.

For the first implementation, `Regenerate Thumbnails...` should operate on a user-selected folder, with optional recursion.

If later there is demand for persistent comic-library roots, add that as a separate explicit feature rather than coupling the core cache format to a library database.

---

# 15. Debug Logging

The existing persistent debug logger performs repeated file open/append operations in thumbnail code.

Before judging L2 hit performance, ensure production/release logging is effectively no-op unless explicitly enabled through a debug environment/configuration switch.

Suggested behavior:

```text
CBXSHELL_DEBUG=1
```

or an explicit debug-log path enables persistent file logging.

Normal release thumbnail extraction should not synchronously append multiple log lines per L2 hit.

Useful debug events when enabled:

```text
L2 CACHE HIT
L2 CACHE MISS
L2 CACHE BYPASS
L2 CACHE CORRUPT
L2 CACHE STORE
L2 CACHE STORE FAILED
```

Include source ID and requested/canonical dimensions, but do not log excessive archive internals on a cache hit.

---

# 16. Installer Registration Fix

Before or alongside the L2 work, fix the existing thumbnail handler registration inconsistency.

The installer currently uses the legacy thumbnail-handler GUID:

```text
{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}
```

while the Rust COM implementation and Manager use the modern `IThumbnailProvider` handler GUID:

```text
{E357FCCD-A995-4576-B01F-234630154E96}
```

The installer must use the modern GUID consistently.

During upgrade/uninstall, remove legacy association keys only when their default value is CBXShell's own CLSID. Never remove another application's association.

This is separate from the L2 architecture but should be corrected before evaluating final Windows thumbnail-cache behavior.

---

# 17. Proposed Workspace Changes

Tentative dependency/module changes:

```text
Cargo.toml
  + hashing dependency (for example sha2)
  + chosen WebP encoder dependency if current image stack is unsuitable

crates/
  cbxshell-thumbnail-cache/
    Cargo.toml
    src/
      lib.rs
      key.rs
      store.rs
      config.rs
      maintenance.rs

  cbxshell-thumbnail-win/
    + RenderedThumbnail
    + render_thumbnail()
    + render/resize helpers that operate on an already-decoded canonical proxy

  cbxshell-image-codec/
    + cache proxy encoder/decoder abstraction as appropriate

CBXShell/src/com/cbxshell.rs
  + source identity resolution
  + L2 lookup before archive open
  + canonical proxy creation/store on miss

CBXShell/src/manager/
  + cache configuration
  + cache status dialog
  + folder scanner
  + force-regeneration worker
  + maintenance integration

installer.nsi
  + modern thumbnail-handler GUID fix
  + safe legacy-key cleanup
```

Keep Windows COM-specific source identity and force-regeneration code at Windows-facing boundaries. Keep cache key/store logic testable without Explorer where possible.

---

# 18. Implementation Phases

## Phase 0 - Correct existing registration and instrumentation

1. Replace the installer's legacy thumbnail-handler GUID with the modern `IThumbnailProvider` GUID.
2. Add safe cleanup of CBXShell-owned legacy registration keys during upgrade/uninstall.
3. Make release debug logging opt-in rather than synchronous by default.
4. Add explicit debug markers for future L1/L2 tracing.

Acceptance:

- Fresh install and upgrade register the same thumbnail provider GUID as CBXManager.
- Normal release thumbnail generation does not repeatedly write a debug log unless enabled.

## Phase 1 - Rendering boundary

1. Introduce `RenderedThumbnail`.
2. Split rendering from `HBITMAP` creation.
3. Keep existing public thumbnail APIs working through wrappers.
4. Add tests verifying existing visual dimensions/background behavior.

Acceptance:

- Existing thumbnail tests pass.
- Existing callers do not require semantic changes.

## Phase 2 - Cache core

1. Add `cbxshell-thumbnail-cache` crate.
2. Implement UTF-16 path hashing and source IDs.
3. Implement variant IDs.
4. Implement sharded filesystem layout.
5. Implement atomic best-effort writes.
6. Implement corrupt-entry deletion behavior.
7. Add unit tests for deterministic keys and invalidation.

Acceptance:

- Identical inputs always produce identical IDs.
- Changing size, mtime, generation, renderer revision, or sort policy changes the variant ID.
- Changing `cx` does not change the L2 key.

## Phase 3 - Canonical proxy codec

1. Implement canonical max-1024 rendering.
2. Add WebP encode/decode path.
3. Benchmark representative JPEG, PNG, WebP, and AVIF comic covers.
4. Compare at least:
   - encoded size
   - encode latency
   - decode latency
   - visible preview quality
5. Finalize WebP quality or select JPEG fallback if results justify it.

Acceptance:

- Canonical proxies maintain acceptable quality at 1024 px preview-pane scale.
- Storage projection remains practical for 10k/30k/50k-entry libraries.

## Phase 4 - Integrate L2 into COM thumbnail flow

1. Resolve source metadata before archive open.
2. Attempt L2 read before archive access.
3. On hit, decode proxy and produce requested `HBITMAP`.
4. On miss, execute current archive path and store the canonical proxy best-effort.
5. Ensure anonymous/unresolvable streams safely bypass the cache.

Acceptance:

- First access with empty L1/L2 opens the archive and stores L2.
- After deleting Windows thumbnail cache only, second access hits L2.
- L2 hit performs no archive open/read.
- Touching/replacing the source archive creates an L2 miss.
- Cache corruption self-recovers through normal extraction.

## Phase 5 - Capacity and manager maintenance

1. Add configurable cache-size limit.
2. Add cache stats collection to CBXManager only.
3. Extend background manager execution with cache maintenance.
4. Remove old generations and stale temp files.
5. Implement oldest-first eviction when above limit.

Acceptance:

- COM hot path performs no global cache maintenance.
- Manager can report approximate/exact cache size and entry count.
- Capacity enforcement does not require SQLite.

## Phase 6 - GUI cache management

1. Add `Thumbnail Cache...` UI.
2. Display path, size, entry count, and limit.
3. Add `Clear CBXShell Cache`.
4. Keep clear-cache semantics separate from regeneration.

Acceptance:

- User can reclaim L2 disk usage without triggering library scans or mass regeneration.

## Phase 7 - Regenerate workflow

1. Add folder picker and recursive option.
2. Enumerate supported archives.
3. Invalidate source L2 directories.
4. Use Windows thumbnail APIs with force extraction to regenerate.
5. Show progress and current file.
6. Support cancellation.
7. Bound concurrency.
8. Record/report failed files without aborting the full run.

Acceptance:

- Regeneration creates a new L2 canonical proxy.
- Regeneration refreshes Windows L1 without deleting all `thumbcache_*.db` files.
- Reopening Explorer after regeneration does not require archive extraction for regenerated files.

## Phase 8 - Global generation support

1. Add persistent `CacheGeneration` configuration.
2. Include generation in variant identity.
3. Implement logical global invalidation via generation increment.
4. Let background maintenance delete old-generation files later.

Acceptance:

- Global invalidation is O(1) before optional prewarming begins.
- Previous generations cannot be returned after a bump.

---

# 19. Testing Matrix

Test archive types:

```text
CBZ / ZIP
CBR / RAR
CB7 / 7z
```

Test source image codecs:

```text
JPEG
PNG
WebP
AVIF
GIF/BMP/TIFF/ICO where currently supported
```

Test source sizes:

```text
small archive
large ZIP/CBZ
large RAR/CBR
large 7z/CB7
```

Test requested thumbnail sizes:

```text
96
256
512
1024
```

Test source cover dimensions:

```text
smaller than 1024 (must not upscale)
portrait > 1024
landscape > 1024
very large AVIF/WebP source
```

Test invalidation:

```text
same path/size/mtime -> hit
mtime changed -> miss
size changed -> miss
sort policy changed -> miss
renderer revision changed -> miss
cache generation changed -> miss
cx changed -> same L2 proxy
```

Test failure modes:

```text
cache root unavailable
permission denied
partial temp file
corrupt encoded proxy
concurrent store of same variant
source deleted during regeneration
archive corrupted
anonymous IStream
regeneration cancellation
```

Test environments:

```text
local SSD
local HDD if available
SMB/network-backed comic library
Windows reboot with Windows thumbnail cache preserved
Windows thumbnail DB explicitly removed
```

---

# 20. Performance Measurements

Collect at least the following timings separately:

```text
L1 Windows cache hit       (outside CBXShell when measurable)
L2 canonical proxy hit
L2 decode time
L2 resize + HBITMAP time
L2 miss archive-open time
cover extraction time
source decode time
canonical proxy encode time
```

For representative libraries, also record:

```text
average canonical proxy size
p50/p90/p99 proxy size
projected 10,000-entry cache size
projected 30,000-entry cache size
projected 50,000-entry cache size
```

The L2 cache succeeds if Windows L1 loss no longer causes expensive archive extraction for unchanged archives.

Do not over-optimize L2 decoding merely to approach Windows L1 speed. L1 remains the normal hot cache; L2 exists to make L1 loss cheap.

---

# 21. Final Acceptance Criteria

The feature is complete when all of the following are true:

1. An archive initially missing from both caches generates a canonical L2 proxy.
2. Deleting only Windows thumbnail-cache data and reopening the same folder does not reopen unchanged archives.
3. Preview-pane requests receive quality derived from a canonical proxy up to 1024 px rather than an upscaled small icon thumbnail.
4. Multiple `cx` requests share one persistent L2 entry.
5. Source modification invalidates the cached variant automatically through metadata identity.
6. Renderer/configuration changes can invalidate cached output deterministically.
7. A full cache generation can be invalidated in O(1) through a generation bump.
8. Cache read/write/corruption failures fall back to normal thumbnail extraction rather than failing Explorer thumbnail generation.
9. Cache maintenance does not run inside the COM thumbnail hot path.
10. CBXManager exposes cache usage, size limit, and clear-cache operations.
11. CBXManager can regenerate a selected folder recursively with progress and cancellation.
12. Regeneration refreshes both CBXShell L2 and Windows L1 through force-extraction APIs rather than global `thumbcache_*.db` deletion.
13. The cache remains practical for collections containing tens of thousands of archives.
14. Installer and Manager consistently register the modern `IThumbnailProvider` handler GUID.

---

# 22. Deferred Follow-ups

Only consider these after v1 data shows they are useful:

- persistent library-root registry/catalog
- negative caching for archives without valid images
- NTFS file-ID-based rename tracking
- multi-level L2 proxies (for example separate 256 and 1024 tiers)
- adaptive WebP quality by image type
- a memory-resident L2.5 decoded proxy cache
- more advanced LRU metadata
- proactive idle-time prewarming unrelated to explicit user regeneration

The first implementation should remain intentionally simple: one canonical persistent proxy per archive variant, metadata-based invalidation, filesystem-only storage, manager-owned maintenance, and explicit Windows-integrated regeneration.
