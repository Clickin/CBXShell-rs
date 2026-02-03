# NSIS Build Script for CBXShell
# This script builds separate x64 and ARM64 NSIS installers for GitHub releases

param(
    [string]$Configuration = "Release",
    [string]$NsisPath = "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
    [ValidateSet("All", "Both", "x64", "ARM64")]
    [string]$Architecture = "Both"
)

$ErrorActionPreference = "Stop"

Write-Host "Building CBXShell-rs NSIS Installer(s)" -ForegroundColor Cyan
Write-Host "Configuration: $Configuration" -ForegroundColor Gray
Write-Host "Architecture: $Architecture" -ForegroundColor Gray
Write-Host ""

# Check if NSIS is installed
if (-not (Test-Path $NsisPath)) {
    Write-Host "ERROR: NSIS not found at $NsisPath" -ForegroundColor Red
    Write-Host ""
    Write-Host "Please install NSIS from:" -ForegroundColor Yellow
    Write-Host "  https://nsis.sourceforge.io/Download" -ForegroundColor Cyan
    Write-Host ""
    exit 1
}

# Architecture to Rust target mapping
$TargetMap = @{
    "x64"   = "x86_64-pc-windows-msvc"
    "ARM64" = "aarch64-pc-windows-msvc"
}

# Determine which architectures to build
$ArchitecturesToBuild = @()
if ($Architecture -eq "All" -or $Architecture -eq "Both") {
    $ArchitecturesToBuild = @("x64", "ARM64")
} else {
    $ArchitecturesToBuild = @($Architecture)
}

Write-Host "Note: UnRAR support is statically linked via unrar crate" -ForegroundColor Gray
Write-Host ""

# Step 1: Build Rust project
Write-Host "[1/3] Building Rust project..." -ForegroundColor Yellow
Push-Location CBXShell

foreach ($Arch in $ArchitecturesToBuild) {
    $RustTarget = $TargetMap[$Arch]

    Write-Host "  Building for $Arch ($RustTarget)..." -ForegroundColor Cyan
    cargo build --release --target $RustTarget
    if ($LASTEXITCODE -ne 0) {
        Pop-Location
        throw "Cargo build failed for $Arch ($RustTarget)"
    }
}

Pop-Location
Write-Host "  ✓ Rust build completed for all architectures" -ForegroundColor Green

# Step 2: Create dist directory
Write-Host "[2/3] Preparing distribution directory..." -ForegroundColor Yellow
$DistDir = ".\dist"
if (-not (Test-Path $DistDir)) {
    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
}
Get-ChildItem "$DistDir\CBXShell-rs-Setup-*-*.exe" -ErrorAction SilentlyContinue | Remove-Item -Force
Write-Host "  ✓ Distribution directory ready" -ForegroundColor Green

# Step 3: Build NSIS installers
Write-Host "[3/3] Building NSIS installer(s)..." -ForegroundColor Yellow

$InstallerFiles = @()

foreach ($Arch in $ArchitecturesToBuild) {
    Write-Host ""
    Write-Host "  Building $Arch installer..." -ForegroundColor Cyan

    & $NsisPath "/DARCH=$Arch" "installer.nsi"
    if ($LASTEXITCODE -ne 0) {
        throw "NSIS build failed for $Arch"
    }

    $InstallerFile = Get-ChildItem "$DistDir\CBXShell-rs-Setup-*-$Arch.exe" | Select-Object -First 1
    if ($InstallerFile) {
        $InstallerFiles += $InstallerFile
        Write-Host "  ✓ $Arch installer created: $($InstallerFile.Name)" -ForegroundColor Green
        Write-Host "    Size: $([math]::Round($InstallerFile.Length / 1MB, 2)) MB" -ForegroundColor Gray
    } else {
        Write-Host "  ✗ Failed to find $Arch installer output" -ForegroundColor Red
    }
}

Write-Host ""
Write-Host "NSIS installer build completed successfully!" -ForegroundColor Green
Write-Host ""
Write-Host "Output files:" -ForegroundColor Cyan
foreach ($file in $InstallerFiles) {
    Write-Host "  - $($file.FullName)" -ForegroundColor Gray
}
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Yellow
Write-Host "  1. Test both installers on a clean Windows system" -ForegroundColor Gray
Write-Host "  2. Create a GitHub release and upload both installers" -ForegroundColor Gray
Write-Host "  3. Update release notes with installation instructions" -ForegroundColor Gray
