param(
    [string]$Version,
    [string]$OutputDir = "dist",
    [switch]$BuildInstallers
)

$ErrorActionPreference = "Stop"

Write-Host "Building CBXShell-rs Chocolatey package" -ForegroundColor Cyan

if (-not $Version) {
    $cargoToml = Get-Content -Path "Cargo.toml" -Raw
    $match = [regex]::Match($cargoToml, 'version\s*=\s*"([^"]+)"')
    if (-not $match.Success) {
        throw "Unable to determine version from Cargo.toml"
    }
    $Version = $match.Groups[1].Value
}

Write-Host "Version: $Version" -ForegroundColor Gray

if ($BuildInstallers) {
    Write-Host "Building NSIS installers..." -ForegroundColor Yellow
    .\build_nsis.ps1 -Configuration Release -Architecture Both
}

if (-not (Get-Command choco -ErrorAction SilentlyContinue)) {
    throw "Chocolatey (choco) is not installed or not on PATH. Install from https://chocolatey.org/install"
}

$distDir = Join-Path $PSScriptRoot "dist"
$toolsDir = Join-Path $PSScriptRoot "chocolatey\tools"

if (-not (Test-Path $toolsDir)) {
    New-Item -ItemType Directory -Path $toolsDir | Out-Null
}

Get-ChildItem -Path $toolsDir -Filter 'CBXShell-rs-Setup-*.exe' -ErrorAction SilentlyContinue | Remove-Item -Force

$x64Installer = Join-Path $distDir "CBXShell-rs-Setup-$Version-x64.exe"
$arm64Installer = Join-Path $distDir "CBXShell-rs-Setup-$Version-ARM64.exe"

if (-not (Test-Path $x64Installer)) {
    throw "Missing x64 installer at $x64Installer. Run build_nsis.ps1 first or use -BuildInstallers."
}

if (-not (Test-Path $arm64Installer)) {
    throw "Missing ARM64 installer at $arm64Installer. Run build_nsis.ps1 first or use -BuildInstallers."
}

Copy-Item -Path $x64Installer -Destination $toolsDir -Force
Copy-Item -Path $arm64Installer -Destination $toolsDir -Force

if (-not (Test-Path $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir | Out-Null
}

$nuspecPath = Join-Path $PSScriptRoot "chocolatey\cbxshell-rs.nuspec"

Write-Host "Packing Chocolatey package..." -ForegroundColor Yellow
choco pack $nuspecPath --version $Version --outputdirectory $OutputDir

Write-Host "Chocolatey package created in $OutputDir" -ForegroundColor Green
