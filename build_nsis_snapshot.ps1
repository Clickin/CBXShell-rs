# NSIS Snapshot Build Script for CBXShell
# Builds x64/ARM64 installers with timestamped snapshot filenames.

param(
    [string]$Configuration = "Release",
    [string]$NsisPath = "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
    [ValidateSet("All", "Both", "x64", "ARM64")]
    [string]$Architecture = "Both",
    [switch]$DebugBuild,
    [string]$Timestamp = (Get-Date -Format "yyyyMMdd-HHmmss")
)

$ErrorActionPreference = "Stop"

Write-Host "Building CBXShell-rs NSIS Snapshot Installer(s)" -ForegroundColor Cyan
Write-Host "Architecture: $Architecture" -ForegroundColor Gray
Write-Host "Snapshot Timestamp: $Timestamp" -ForegroundColor Gray
Write-Host ""

if ($DebugBuild) {
    $Configuration = "Debug"
}

if ($Configuration -notin @("Release", "Debug")) {
    throw "Invalid configuration '$Configuration'. Expected Release or Debug"
}

$IsRelease = $Configuration -eq "Release"
Write-Host "Configuration: $Configuration" -ForegroundColor Gray

if (-not (Test-Path $NsisPath)) {
    Write-Host "ERROR: NSIS not found at $NsisPath" -ForegroundColor Red
    Write-Host ""
    Write-Host "Please install NSIS from:" -ForegroundColor Yellow
    Write-Host "  https://nsis.sourceforge.io/Download" -ForegroundColor Cyan
    Write-Host ""
    exit 1
}

if ($Timestamp -notmatch '^[0-9]{8}-[0-9]{6}$') {
    throw "Invalid timestamp format '$Timestamp'. Expected yyyyMMdd-HHmmss"
}

$TargetMap = @{
    "x64"   = "x86_64-pc-windows-msvc"
    "ARM64" = "aarch64-pc-windows-msvc"
}

$ArchitecturesToBuild = @()
if ($Architecture -eq "All" -or $Architecture -eq "Both") {
    $ArchitecturesToBuild = @("x64", "ARM64")
} else {
    $ArchitecturesToBuild = @($Architecture)
}

Write-Host "Note: UnRAR support is statically linked via unrar crate" -ForegroundColor Gray
Write-Host ""

Write-Host "[1/3] Building Rust project..." -ForegroundColor Yellow
Push-Location CBXShell

foreach ($Arch in $ArchitecturesToBuild) {
    $RustTarget = $TargetMap[$Arch]

    Write-Host "  Building for $Arch ($RustTarget)..." -ForegroundColor Cyan
    if ($IsRelease) {
        cargo build --release --target $RustTarget
    }
    else {
        cargo build --target $RustTarget
    }
    if ($LASTEXITCODE -ne 0) {
        Pop-Location
        throw "Cargo build failed for $Arch ($RustTarget)"
    }
}

Pop-Location
Write-Host "  ✓ Rust build completed for all architectures" -ForegroundColor Green

Write-Host "[2/3] Preparing distribution directory..." -ForegroundColor Yellow
$DistDir = ".\dist"
if (-not (Test-Path $DistDir)) {
    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
}
Write-Host "  ✓ Distribution directory ready" -ForegroundColor Green

Write-Host "[3/3] Building NSIS snapshot installer(s)..." -ForegroundColor Yellow

$InstallerFiles = @()
$MissingArchitectures = @()

foreach ($Arch in $ArchitecturesToBuild) {
    Write-Host ""
    Write-Host "  Building $Arch snapshot installer..." -ForegroundColor Cyan

    & $NsisPath "/DARCH=$Arch" "/DBUILD_PROFILE=$Configuration" "/DSNAPSHOT_TIMESTAMP=$Timestamp" "installer.nsi"
    if ($LASTEXITCODE -ne 0) {
        throw "NSIS build failed for $Arch"
    }

    $ProfileSuffix = if ($Configuration -eq "Debug") { "-debug" } else { "" }
    $InstallerPattern = "$DistDir\CBXShell-rs-Setup-*-snapshot-$Timestamp-$Arch$ProfileSuffix.exe"
    $InstallerFile = Get-ChildItem $InstallerPattern -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1

    if ($InstallerFile) {
        $InstallerFiles += $InstallerFile
        Write-Host "  ✓ $Arch snapshot installer created: $($InstallerFile.Name)" -ForegroundColor Green
        Write-Host "    Size: $([math]::Round($InstallerFile.Length / 1MB, 2)) MB" -ForegroundColor Gray
    } else {
        Write-Host "  ✗ Failed to find $Arch snapshot installer output" -ForegroundColor Red
        $MissingArchitectures += $Arch
    }
}

if ($MissingArchitectures.Count -gt 0) {
    throw "Snapshot installer output missing for: $($MissingArchitectures -join ', '). Ensure installer.nsi handles SNAPSHOT_TIMESTAMP."
}

Write-Host ""
Write-Host "NSIS snapshot installer build completed successfully!" -ForegroundColor Green
Write-Host ""
Write-Host "Output files:" -ForegroundColor Cyan
foreach ($file in $InstallerFiles) {
    Write-Host "  - $($file.FullName)" -ForegroundColor Gray
}
Write-Host ""
Write-Host "Snapshot tag: $Timestamp" -ForegroundColor Yellow
