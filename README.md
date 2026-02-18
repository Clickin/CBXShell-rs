# CBXShell-rs

Modern Windows Shell Extension for ZIP/RAR/7z comic book archive thumbnail preview with WebP/AVIF support, built on IThumbnailProvider.

> **Rust reimplementation** of the original [CBXShell](https://github.com/T800G/CBXShell) C++ project by T800 Productions ([CodeProject article](https://www.codeproject.com/Articles/2270/CBXShell-A-Thumbnail-Shell-Extension-for-Comic-Boo)), modernized with Rust for improved memory safety, maintainability, and Windows 11 compatibility using the modern IThumbnailProvider interface.

## Features

- **Modern Windows Integration**: Uses IThumbnailProvider for native Windows Vista+ compatibility
- **Multi-Format Support**: ZIP, RAR, 7z archives (.cbz, .cbr, .cb7)
- **Modern Image Formats**: JPEG, PNG, GIF, BMP, TIFF, ICO, **WebP**, **AVIF**
- **Pure Rust**: Memory-safe implementation using `windows-rs`
- **High-Quality Thumbnails**: Advanced resizing with `fast_image_resize` for crisp previews
- **Shell Integration**: Thumbnail previews and tooltips in Windows Explorer
- **Stream-Based Processing**: Efficient IInitializeWithStream for better performance
- **Natural Sorting**: Alphabetical image sorting with logical number ordering
- **Large File Support**: Handles archives up to 10GB with individual image files up to 32MB

## Building

### Prerequisites

- Rust 1.70+ (stable)
- Windows 10 SDK or later
- Visual Studio Build Tools (for MSVC linker)

### Build Commands

```bash
# Clone the repository
git clone https://github.com/Clickin/CBXShell-rs.git
cd CBXShell-rs

# Build release version (defaults to host architecture)
cargo build --release

# Build for specific architectures
cargo build --release --target x86_64-pc-windows-msvc  # x64
cargo build --release --target aarch64-pc-windows-msvc # ARM64

# Run tests
cargo test
```

## Installation

### Option 1: NSIS Installer (GitHub Releases)

Download the appropriate installer for your system from the [latest release](https://github.com/Clickin/CBXShell-rs/releases/latest). This is the primary release channel:

- **x64 (Intel/AMD 64-bit)**: `CBXShell-rs-Setup-x.x.x-x64.exe` - Most Windows PCs
- **ARM64 (Windows on ARM)**: `CBXShell-rs-Setup-x.x.x-ARM64.exe` - Surface Pro X, Windows Dev Kit 2023

**How to check your system architecture:**
```powershell
# Run in PowerShell
$env:PROCESSOR_ARCHITECTURE
# Output: AMD64 (x64) or ARM64
```

**Installation Steps:**
1. Download the correct installer for your architecture
2. Run the installer (requires administrator privileges)
3. Select which file formats to enable (CBZ, CBR, ZIP, RAR, 7Z)
4. Restart Windows Explorer when prompted

**System Requirements:**
- Windows 10 or later (64-bit)
- For Windows on ARM: Windows 11 or later

### AVIF Support Policy (v5.1.2)

CBXShell uses the Windows Imaging Component (WIC) path as the supported AVIF decode path.

- **Supported path**: WIC AVIF decoding (recommended for production use)
- **Required Windows codecs**:
  - [HEIF Image Extensions](https://apps.microsoft.com/detail/9PMMSR1CGPWG)
  - [AV1 Video Extension](https://apps.microsoft.com/detail/9MVZQVXJBQ9V)
- **Enterprise/offline deployments**: If Microsoft Store is blocked, IT should provision the same extension packages through managed/offline deployment.

CBXShell still contains a software fallback path for compatibility, but AVIF support and issue triage are based on the WIC path first.

**AVIF troubleshooting quick check:**
1. Install/update both extensions listed above
2. Restart Explorer (or run `dev_shell_extension.ps1` register flow)
3. Recreate thumbnail and confirm in log (`$env:TEMP\cbxshell_debug.log`) that `WIC decode path used successfully` appears

### Option 2: Manual Installation (Advanced Users)

For development or custom deployment:

```cmd
# Build the project
cargo build --release

# Navigate to the build output
cd target\release

# Register (run as administrator)
regsvr32 cbxshell.dll

# Unregister
regsvr32 /u cbxshell.dll
```

### Development Feedback Loop (No Installer)

For faster iteration during shell-extension development, use the project-local build output directly:

```powershell
# Register local development DLL + clear thumbcache + restart Explorer
.\dev_shell_extension.ps1 -Action register

# Unregister local development DLL + clear thumbcache + restart Explorer
.\dev_shell_extension.ps1 -Action unregister
```

The script always targets:

`target\x86_64-pc-windows-msvc\release\cbxshell.dll`

Run from an elevated PowerShell session.

### Restart Windows Explorer

After installation, restart Windows Explorer to see thumbnails:

```cmd
taskkill /f /im explorer.exe
start explorer.exe
```

## Development Status

### ✅ Phase 1: COM Infrastructure (COMPLETE)
- [x] Rust workspace setup
- [x] COM DLL exports (DllGetClassObject, DllCanUnloadNow, DllRegisterServer, DllUnregisterServer)
- [x] COM class factory with reference counting
- [x] Modern IThumbnailProvider implementation (replacing legacy IExtractImage)
- [x] IInitializeWithStream for stream-based initialization (replacing IPersistFile)
- [x] IQueryInfo for tooltips
- [x] Registry management
- [x] Build system with proper Windows SDK integration

### ✅ Phase 2: Archive Support (COMPLETE)
- [x] ZIP/CBZ extraction (`zip` crate)
- [x] RAR/CBR extraction (`unrar` crate)
- [x] 7z/CB7 extraction (`sevenz-rust` crate)
- [x] Archive trait abstraction fully implemented
- [x] Alphabetical sorting with natural order (`natord` crate)
- [x] Stream-based archive reading from IStream

### ✅ Phase 3: Image Processing (COMPLETE)
- [x] Image format detection
- [x] WebP, AVIF, JPEG, PNG, GIF, BMP, TIFF, ICO support via `image` crate
- [x] High-quality thumbnail generation using `fast_image_resize`
- [x] HBITMAP conversion for Windows
- [x] Proper aspect ratio preservation

### ✅ Phase 4: Shell Extension Integration (COMPLETE)
- [x] IThumbnailProvider thumbnail extraction logic fully implemented
- [x] IQueryInfo tooltip generation
- [x] Error handling and file-based debug logging
- [x] Full integration with Windows Explorer

### ✅ Phase 5: Configuration Manager (COMPLETE)
- [x] Modern GUI using egui framework
- [x] Enable/disable handlers for file formats
- [x] Sort preference configuration
- [x] Registry operations for handler management

## Project Structure

```
CBXShell/
├── Cargo.toml                   # Workspace configuration
├── CBXShell/
│   ├── Cargo.toml               # Library + binary configuration
│   ├── build.rs                 # Build script with resource embedding
│   ├── src/
│   │   ├── lib.rs               # DLL entry point & COM exports
│   │   ├── com/                 # COM implementation
│   │   │   ├── mod.rs
│   │   │   ├── class_factory.rs # COM class factory
│   │   │   ├── cbxshell.rs      # IThumbnailProvider + IInitializeWithStream + IQueryInfo
│   │   │   ├── persist_file.rs  # (legacy support)
│   │   │   ├── extract_image.rs # (legacy support)
│   │   │   └── query_info.rs    # Tooltip implementation
│   │   ├── archive/             # Archive format support
│   │   │   ├── mod.rs           # Archive trait and unified API
│   │   │   ├── zip.rs           # ZIP/CBZ support
│   │   │   ├── rar.rs           # RAR/CBR support
│   │   │   ├── sevenz.rs        # 7z/CB7 support
│   │   │   ├── utils.rs         # Image detection, natural sorting
│   │   │   ├── config.rs        # Registry configuration reading
│   │   │   └── stream_reader.rs # IStream to memory conversion
│   │   ├── image_processor/     # Image processing
│   │   │   ├── mod.rs
│   │   │   ├── decoder.rs       # Image decoding (WebP, AVIF, etc.)
│   │   │   ├── resizer.rs       # High-quality resizing
│   │   │   ├── hbitmap.rs       # Windows HBITMAP conversion
│   │   │   └── thumbnail.rs     # Thumbnail creation pipeline
│   │   ├── registry.rs          # COM registration
│   │   ├── utils/               # Utilities
│   │   │   ├── mod.rs
│   │   │   ├── error.rs         # Error types
│   │   │   ├── file.rs          # File utilities
│   │   │   └── debug_log.rs     # File-based debug logging
│   │   └── manager/             # Configuration GUI
│   │       ├── main.rs          # Entry point
│   │       ├── ui.rs            # egui UI implementation
│   │       ├── state.rs         # Application state
│   │       ├── registry_ops.rs  # Registry operations
│   │       └── utils.rs         # Helper functions
│   └── tests/                   # Integration tests
│       └── test_webp_decode.rs  # WebP decoding tests
├── build_nsis.ps1               # NSIS installer script
└── README.md
```

## Architecture

### COM Implementation

CBXShell implements the modern Windows thumbnail handler interfaces:

1. **IThumbnailProvider**: Primary interface for thumbnail extraction (Windows Vista+)
2. **IInitializeWithStream**: Stream-based initialization for better performance and security
3. **IQueryInfo**: Provides tooltip information with archive metadata

Legacy interfaces are maintained for compatibility:
- **IPersistFile**: File-based initialization (legacy)
- **IExtractImage2**: Thumbnail extraction (legacy, superseded by IThumbnailProvider)

### Archive Support

Uses a trait-based architecture for extensibility:

```rust
pub trait Archive: Send {
    fn find_first_image(&mut self, sort: bool) -> Result<ArchiveEntry>;
    fn extract_to_memory(&mut self, entry: &ArchiveEntry) -> Result<Vec<u8>>;
    fn get_info(&self) -> Result<ArchiveInfo>;
}
```

Implementations:
- **ZipArchive**: Pure Rust via `zip` crate with full streaming support
- **RarArchive**: Via `unrar` crate with solid archive handling
- **SevenZipArchive**: Pure Rust via `sevenz-rust` crate with LZMA compression

All archive implementations support:
- Stream-based reading from IStream interface
- Natural order sorting using `natord` crate
- Efficient image detection and extraction
- Memory-safe operations with proper error handling

### Image Processing

Advanced image handling pipeline:

- **Format Support**: WebP, AVIF, JPEG, PNG, GIF, BMP, TIFF, ICO via `image` crate
- **High-Quality Resizing**: Uses `fast_image_resize` with Lanczos3 filter
- **Aspect Ratio Preservation**: Intelligent scaling to fit thumbnail dimensions
- **HBITMAP Generation**: Native Windows bitmap creation for Explorer integration
- **Memory Efficiency**: Streaming decode and resize to minimize memory usage

## Logging

CBXShell includes file-based debug logging for troubleshooting:

```cmd
# Debug logs are automatically written to:
# C:\Users\<username>\AppData\Local\Temp\cbxshell_debug.log
```

The log file includes:
- COM interface calls and parameters
- Archive processing operations
- Image decoding and resizing steps
- Error messages with full context
- Performance timing information

To view logs in real-time:
```powershell
Get-Content "$env:TEMP\cbxshell_debug.log" -Wait
```

## Configuration Manager

CBXShell includes a modern configuration GUI built with egui:

```cmd
# Run the configuration manager
CBXShell.exe --manager
```

Features:
- **Handler Management**: Enable/disable thumbnail and tooltip handlers per file type
- **Format Selection**: Choose which archive formats to handle (CBZ, CBR, CB7, ZIP, RAR, 7Z)
- **Sorting Options**: Configure alphabetical vs. discovery order for images

The manager provides a clean, modern interface for customizing CBXShell behavior without manual registry editing.

## Contributing

This is a rewrite of the original C++ CBXShell project. Contributions are welcome!

### Guidelines

- Follow Rust best practices and idioms
- Maintain memory safety (no unsafe code without justification)
- Add tests for new functionality
- Document public APIs

### Building NSIS Installer (GitHub Releases)

Create a standalone installer for direct distribution:

```powershell
# Build NSIS installer
.\build_nsis.ps1 -Configuration Release

# Output: dist\CBXShell-Setup-5.0.0.exe
```

**Requirements:**
- [NSIS 3.x](https://nsis.sourceforge.io/Download) or later

### Building a Chocolatey Package

Create a Chocolatey package that bundles the NSIS installers:

```powershell
# Build Chocolatey package (includes installers)
.\build_chocolatey.ps1 -BuildInstallers

# Output: dist\cbxshell-rs.<version>.nupkg
```

**Requirements:**
- [Chocolatey CLI](https://chocolatey.org/install)
- [NSIS 3.x](https://nsis.sourceforge.io/Download) or later


## License

This project is licensed under the Code Project Open License (CPOL) 1.02.
See `The Code Project Open License (CPOL) 1.02.md` for details.

## Credits

- Original C++ implementation: [CBXShell](https://github.com/T800G/CBXShell) by T800 Productions
- Rust rewrite incorporating modern archive and image format support
- Uses `windows-rs` for COM interop (Microsoft)
- Uses `unrar` crate from https://github.com/muja/unrar.rs

## Roadmap

### ✅ Completed (v5.0.0)
- ✅ Modern IThumbnailProvider implementation
- ✅ Multi-format archive support (ZIP, RAR, 7z)
- ✅ Modern image format support (WebP, AVIF)
- ✅ High-quality thumbnail generation with `fast_image_resize`
- ✅ Configuration manager with modern egui GUI
- ✅ Per-user installation support

### Future Enhancements

#### v5.1.0 (Planned)
- **Context Menu Integration**: Right-click folder thumbnail generation
  - Extract and display thumbnails for folders containing comic archives
  - Batch thumbnail generation for archive collections
  - Folder icon overlay with first archive's cover
- **Test Coverage Improvements**
  - Comprehensive integration test suite
  - Performance benchmarking
  - Memory leak testing
  - Windows 11 compatibility verification
- **Windows 11 Enhancements**
  - Dark mode support for thumbnails
  - Improved thumbnail caching
  - Enhanced error reporting

#### v5.2.0
- ARM64 support for Windows on ARM
- Performance optimizations for large archives
- Additional sorting options (by date, by size)

#### v6.0.0
- Additional archive formats (TAR.GZ, CBT)
- Enhanced metadata extraction (reading order, series information)
- Custom thumbnail templates
- Cloud storage provider integration
