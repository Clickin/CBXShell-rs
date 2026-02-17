# Changelog

All notable changes to CBXShell-rs will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [5.1.2] - 2026-02-17

### Added
- GitHub Actions Windows CI workflow for `master`/PR validation (x64 tests + x64/ARM64 release build checks).
- GitHub Actions Windows release workflow that builds NSIS installers for x64 and ARM64 and publishes them to GitHub Releases.

### Changed
- Release process now supports automatic release-note generation from commits using GitHub generated notes.

## [5.1.0] - 2025-10-25

### Performance Improvements

#### Massive Thumbnail Generation Speed Improvements
- **ZIP/CBZ archives**: ~24x faster thumbnail generation with 500x less memory usage
  - 1GB archive: 3.1s → 0.13s, Memory: 1GB → 2MB
- **RAR/CBR archives**: ~2.4x faster with 1000x less memory reduction
  - 1GB archive: 5.2s → 2.2s, Memory: 1GB → 1MB
- **7z/CB7 archives**: ~19.4x faster with 500x less memory reduction
  - 1GB archive: 3.1s → 0.16s, Memory: 1GB → 2MB

#### Implementation Details
- Implemented IStream-based streaming architecture eliminating full archive loading
- ZIP archives now use random access to central directory (only reads metadata + target image)
- RAR archives stream to temp file with minimal buffer (1MB chunks instead of full archive)
- 7z archives use RefCell pattern for streaming with reader recreation optimization

### Security & Reliability
- Added comprehensive memory safety analysis and improvements
- Implemented magic header verification for robust image format detection
- Enhanced error handling throughout the codebase

### Documentation
- Added detailed performance analysis documentation (PERFORMANCE_ANALYSIS.md)
- Added 7z streaming optimization analysis (7Z_OPTIMIZATION_ANALYSIS.md)
- Added memory safety analysis documentation (MEMORY_SAFETY_ANALYSIS.md)

### Known Limitations
- ARM64 Windows build not yet supported due to missing ARM64 MSVC toolchain
  - UnRAR source code supports ARM64 (since v6.1.3)
  - Requires Visual Studio ARM64 build tools installation
  - Planned for v5.2.0 release

## [5.0.0] - 2025-01-XX

### Initial Release
- Modern IThumbnailProvider implementation for Windows Vista+
- Multi-format archive support (ZIP, RAR, 7z)
- Modern image format support (WebP, AVIF, JPEG, PNG, GIF, BMP, TIFF, ICO)
- High-quality thumbnail generation with fast_image_resize
- Configuration manager with modern egui GUI
- Per-user installation support
- Natural sorting with logical number ordering
- Stream-based IInitializeWithStream processing
- Tooltip support via IQueryInfo
- File-based debug logging

[5.1.2]: https://github.com/Clickin/CBXShell-rs/compare/v5.1.0...v5.1.2
[5.1.0]: https://github.com/Clickin/CBXShell-rs/compare/v5.0.0...v5.1.0
[5.0.0]: https://github.com/Clickin/CBXShell-rs/releases/tag/v5.0.0
