# MSIX Build Script for CBXShell-rs
# This script builds the MSIX package for Windows Store distribution

param(
    [string]$Configuration = "Release",
    [ValidateSet("x64", "x86", "ARM64")]
    [string]$Platform = "x64",
    [string]$OutputDir = ".\dist\msix",
    [string]$CertificateThumbprint = $null
)

$ErrorActionPreference = "Stop"

Write-Host "Building CBXShell-rs MSIX Package" -ForegroundColor Cyan
Write-Host "Configuration: $Configuration" -ForegroundColor Gray
Write-Host "Platform: $Platform" -ForegroundColor Gray
Write-Host ""

# Step 1: Build Rust project
Write-Host "[1/6] Building Rust project..." -ForegroundColor Yellow

# Map Platform to Rust target triple
$RustTarget = switch ($Platform) {
    "x64"   { "x86_64-pc-windows-msvc" }
    "x86"   { "i686-pc-windows-msvc" }
    "ARM64" { "aarch64-pc-windows-msvc" }
}

Write-Host "  Building for target: $RustTarget" -ForegroundColor Gray

Push-Location CBXShell
cargo build --release --target $RustTarget
if ($LASTEXITCODE -ne 0) {
    Pop-Location
    throw "Cargo build failed for target $RustTarget"
}
Pop-Location
Write-Host "  ✓ Rust build completed" -ForegroundColor Green

# Step 2: Create package directory structure
Write-Host "[2/6] Creating package directory..." -ForegroundColor Yellow
$PackageDir = Join-Path $OutputDir "Package"
if (Test-Path $PackageDir) {
    Remove-Item -Recurse -Force $PackageDir
}
New-Item -ItemType Directory -Force -Path $PackageDir | Out-Null
New-Item -ItemType Directory -Force -Path "$PackageDir\Assets" | Out-Null
Write-Host "  ✓ Package directory created" -ForegroundColor Green

# Step 3: Copy binaries
Write-Host "[3/6] Copying binaries..." -ForegroundColor Yellow

# Determine build output path based on target
$BuildPath = "target\$RustTarget\release"

# Check if binaries exist
$DllPath = Join-Path $BuildPath "CBXShell.dll"
$ExePath = Join-Path $BuildPath "CBXManager.exe"

if (-not (Test-Path $DllPath)) {
    throw "CBXShell.dll not found at $DllPath"
}
if (-not (Test-Path $ExePath)) {
    throw "CBXManager.exe not found at $ExePath"
}

Copy-Item $DllPath "$PackageDir\"
Copy-Item $ExePath "$PackageDir\"

Write-Host "  ✓ Binaries copied from $BuildPath" -ForegroundColor Green

# Note: UnRAR.dll is statically linked, no need to copy

# Step 4: Copy manifest and assets
Write-Host "[4/6] Copying manifest and assets..." -ForegroundColor Yellow
Copy-Item "AppxManifest.xml" "$PackageDir\"
Copy-Item -Recurse "Assets\*" "$PackageDir\Assets\"
Write-Host "  ✓ Manifest and assets copied" -ForegroundColor Green

# Step 5: Create MSIX package
Write-Host "[5/6] Creating MSIX package..." -ForegroundColor Yellow
$MsixPath = Join-Path $OutputDir "CBXShell_5.1.1.0_$Platform.msix"
$MakeAppxPath = "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.0.22621.0\x64\makeappx.exe"

if (-not (Test-Path $MakeAppxPath)) {
    # Try to find makeappx.exe in other locations
    $MakeAppxPath = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\" -Recurse -Filter "makeappx.exe" |
                    Where-Object { $_.DirectoryName -like "*\x64" } |
                    Select-Object -First 1 -ExpandProperty FullName
}

if (-not $MakeAppxPath -or -not (Test-Path $MakeAppxPath)) {
    throw "makeappx.exe not found. Please install Windows SDK 10."
}

& $MakeAppxPath pack /d "$PackageDir" /p "$MsixPath" /o
if ($LASTEXITCODE -ne 0) {
    throw "makeappx.exe failed"
}
Write-Host "  ✓ MSIX package created: $MsixPath" -ForegroundColor Green

# Step 6: Sign package (optional)
if ($CertificateThumbprint) {
    Write-Host "[6/6] Signing package..." -ForegroundColor Yellow
    $SignToolPath = "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.0.22621.0\x64\signtool.exe"

    if (-not (Test-Path $SignToolPath)) {
        $SignToolPath = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\" -Recurse -Filter "signtool.exe" |
                        Where-Object { $_.DirectoryName -like "*\x64" } |
                        Select-Object -First 1 -ExpandProperty FullName
    }

    if (Test-Path $SignToolPath) {
        & $SignToolPath sign /fd SHA256 /sha1 $CertificateThumbprint /t http://timestamp.digicert.com "$MsixPath"
        if ($LASTEXITCODE -eq 0) {
            Write-Host "  ✓ Package signed successfully" -ForegroundColor Green
        } else {
            Write-Host "  ⚠ Signing failed (non-critical)" -ForegroundColor Yellow
        }
    } else {
        Write-Host "  ⚠ signtool.exe not found, skipping signing" -ForegroundColor Yellow
    }
} else {
    Write-Host "[6/6] Skipping signing (no certificate specified)" -ForegroundColor Gray
}

Write-Host ""
Write-Host "MSIX package build completed successfully!" -ForegroundColor Green
Write-Host "Output: $MsixPath" -ForegroundColor Cyan
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Yellow
Write-Host "  1. Test the package locally: Add-AppxPackage -Path '$MsixPath'" -ForegroundColor Gray
Write-Host "  2. Sign with your Windows Store certificate for submission" -ForegroundColor Gray
Write-Host "  3. Submit to Windows Partner Center" -ForegroundColor Gray
