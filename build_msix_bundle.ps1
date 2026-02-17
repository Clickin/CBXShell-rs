# MSIX Bundle Build Script for CBXShell-rs
# This script builds MSIX packages for all architectures and bundles them
# for Windows Store distribution

param(
    [string]$Configuration = "Release",
    [string]$OutputDir = ".\dist\msix",
    [string]$CertificateThumbprint = $null,
    [switch]$SkipBuild = $false
)

$ErrorActionPreference = "Stop"

Write-Host "=====================================" -ForegroundColor Cyan
Write-Host "CBXShell-rs MSIX Bundle Build" -ForegroundColor Cyan
Write-Host "=====================================" -ForegroundColor Cyan
Write-Host ""

# Version from AppxManifest.xml
$Version = "5.1.2.0"

# Architectures to build
$Platforms = @("x64", "ARM64")

# Map Platform to Rust target and MSIX architecture
$TargetMap = @{
    "x64"   = @{
        RustTarget = "x86_64-pc-windows-msvc"
        MsixArch   = "x64"
    }
    "ARM64" = @{
        RustTarget = "aarch64-pc-windows-msvc"
        MsixArch   = "arm64"
    }
}

# Find Windows SDK tools
function Find-SdkTool {
    param([string]$ToolName)

    $SdkPath = "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.0.22621.0\x64\$ToolName"
    if (Test-Path $SdkPath) {
        return $SdkPath
    }

    $Tool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\" -Recurse -Filter $ToolName |
            Where-Object { $_.DirectoryName -like "*\x64" } |
            Select-Object -First 1 -ExpandProperty FullName

    if (-not $Tool) {
        throw "$ToolName not found. Please install Windows SDK 10."
    }

    return $Tool
}

$MakeAppxPath = Find-SdkTool "makeappx.exe"
Write-Host "Using makeappx: $MakeAppxPath" -ForegroundColor Gray
Write-Host ""

# Array to store generated MSIX paths
$MsixPackages = @()

# Build each platform
foreach ($Platform in $Platforms) {
    $Target = $TargetMap[$Platform]
    $RustTarget = $Target.RustTarget
    $MsixArch = $Target.MsixArch

    Write-Host "=====================================" -ForegroundColor Yellow
    Write-Host "Building for $Platform ($MsixArch)" -ForegroundColor Yellow
    Write-Host "=====================================" -ForegroundColor Yellow
    Write-Host ""

    # Step 1: Build Rust project
    if (-not $SkipBuild) {
        Write-Host "[1/5] Building Rust project..." -ForegroundColor Yellow
        Write-Host "  Target: $RustTarget" -ForegroundColor Gray

        Push-Location CBXShell
        cargo build --release --target $RustTarget
        if ($LASTEXITCODE -ne 0) {
            Pop-Location
            throw "Cargo build failed for target $RustTarget"
        }
        Pop-Location
        Write-Host "  ✓ Rust build completed" -ForegroundColor Green
        Write-Host ""
    } else {
        Write-Host "[1/5] Skipping Rust build (--SkipBuild specified)" -ForegroundColor Gray
        Write-Host ""
    }

    # Step 2: Create package directory
    Write-Host "[2/5] Creating package directory..." -ForegroundColor Yellow
    $PackageDir = Join-Path $OutputDir "Package_$Platform"
    if (Test-Path $PackageDir) {
        Remove-Item -Recurse -Force $PackageDir
    }
    New-Item -ItemType Directory -Force -Path $PackageDir | Out-Null
    New-Item -ItemType Directory -Force -Path "$PackageDir\Assets" | Out-Null
    Write-Host "  ✓ Package directory created: $PackageDir" -ForegroundColor Green
    Write-Host ""

    # Step 3: Copy binaries
    Write-Host "[3/5] Copying binaries..." -ForegroundColor Yellow
    $BuildPath = "target\$RustTarget\release"

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
    Write-Host ""

    # Step 4: Create architecture-specific manifest
    Write-Host "[4/5] Creating manifest for $MsixArch..." -ForegroundColor Yellow

    # Read base manifest and update ProcessorArchitecture
    [xml]$Manifest = Get-Content "AppxManifest.xml"
    $Manifest.Package.Identity.ProcessorArchitecture = $MsixArch

    # Save modified manifest
    $ManifestPath = Join-Path $PackageDir "AppxManifest.xml"
    $Manifest.Save($ManifestPath)

    # Copy assets
    Copy-Item -Recurse "Assets\*" "$PackageDir\Assets\"
    Write-Host "  ✓ Manifest and assets copied" -ForegroundColor Green
    Write-Host ""

    # Step 5: Create MSIX package
    Write-Host "[5/5] Creating MSIX package..." -ForegroundColor Yellow
    $MsixPath = Join-Path $OutputDir "CBXShell_${Version}_${Platform}.msix"

    & $MakeAppxPath pack /d "$PackageDir" /p "$MsixPath" /o
    if ($LASTEXITCODE -ne 0) {
        throw "makeappx.exe failed for $Platform"
    }

    $MsixPackages += $MsixPath
    Write-Host "  ✓ MSIX package created: $MsixPath" -ForegroundColor Green
    Write-Host ""

    # Clean up package directory
    Remove-Item -Recurse -Force $PackageDir
}

# Create MSIX Bundle
Write-Host "=====================================" -ForegroundColor Cyan
Write-Host "Creating MSIX Bundle" -ForegroundColor Cyan
Write-Host "=====================================" -ForegroundColor Cyan
Write-Host ""

$BundlePath = Join-Path $OutputDir "CBXShell_${Version}.msixbundle"

Write-Host "Bundling packages..." -ForegroundColor Yellow
foreach ($pkg in $MsixPackages) {
    Write-Host "  - $(Split-Path -Leaf $pkg)" -ForegroundColor Gray
}
Write-Host ""

# Create bundle mapping file
$MappingFile = Join-Path $OutputDir "bundle_mapping.txt"
$MappingContent = @()
$MappingContent += "[Files]"
foreach ($pkg in $MsixPackages) {
    $MappingContent += "`"$pkg`" `"$(Split-Path -Leaf $pkg)`""
}
$MappingContent | Set-Content -Path $MappingFile -Encoding UTF8

Write-Host "Created bundle mapping file" -ForegroundColor Gray
Write-Host ""

# Create bundle using makeappx with mapping file
& $MakeAppxPath bundle /f "$MappingFile" /p "$BundlePath" /o
if ($LASTEXITCODE -ne 0) {
    throw "Failed to create MSIX bundle"
}

# Clean up mapping file
Remove-Item $MappingFile -Force

Write-Host "✓ MSIX bundle created successfully!" -ForegroundColor Green
Write-Host ""

# Optional: Sign the bundle
if ($CertificateThumbprint) {
    Write-Host "Signing bundle..." -ForegroundColor Yellow
    $SignToolPath = Find-SdkTool "signtool.exe"

    & $SignToolPath sign /fd SHA256 /sha1 $CertificateThumbprint /t http://timestamp.digicert.com "$BundlePath"
    if ($LASTEXITCODE -eq 0) {
        Write-Host "✓ Bundle signed successfully" -ForegroundColor Green
    } else {
        Write-Host "⚠ Signing failed (non-critical)" -ForegroundColor Yellow
    }
    Write-Host ""
}

# Summary
Write-Host "=====================================" -ForegroundColor Cyan
Write-Host "Build Summary" -ForegroundColor Cyan
Write-Host "=====================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "Bundle: $BundlePath" -ForegroundColor Green
Write-Host "Size:   $([math]::Round((Get-Item $BundlePath).Length / 1MB, 2)) MB" -ForegroundColor Gray
Write-Host ""
Write-Host "Individual packages:" -ForegroundColor Gray
foreach ($pkg in $MsixPackages) {
    $size = [math]::Round((Get-Item $pkg).Length / 1MB, 2)
    Write-Host "  - $(Split-Path -Leaf $pkg) ($size MB)" -ForegroundColor Gray
}
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Yellow
Write-Host "  1. Test the bundle locally: Add-AppxPackage -Path '$BundlePath'" -ForegroundColor Gray
Write-Host "  2. Submit to Microsoft Partner Center" -ForegroundColor Gray
Write-Host "     → Upload the .msixbundle file (recommended)" -ForegroundColor Gray
Write-Host ""
