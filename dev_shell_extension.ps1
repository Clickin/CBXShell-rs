param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("register", "unregister")]
    [string]$Action,
    [string]$Configuration = "Release",
    [switch]$DebugBuild
)

$ErrorActionPreference = "Stop"

function Assert-Administrator {
    $currentIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($currentIdentity)

    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Run this script from an elevated PowerShell session."
    }
}

function Stop-Explorer {
    Get-Process -Name "explorer" -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 700
}

function Start-Explorer {
    Start-Process explorer.exe
}

function Clear-ThumbnailCache {
    $explorerCacheDir = Join-Path $env:LOCALAPPDATA "Microsoft\Windows\Explorer"
    if (-not (Test-Path -LiteralPath $explorerCacheDir)) {
        return
    }

    Get-ChildItem -LiteralPath $explorerCacheDir -Filter "thumbcache*" -File -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
}

Assert-Administrator

if ($DebugBuild) {
    $Configuration = "Debug"
}

if ($Configuration -notin @("Release", "Debug")) {
    throw "Invalid configuration '$Configuration'. Expected Release or Debug"
}

$profileDir = $Configuration.ToLowerInvariant()
$dllPath = Join-Path $PSScriptRoot "target\x86_64-pc-windows-msvc\$profileDir\cbxshell.dll"
if (-not (Test-Path -LiteralPath $dllPath)) {
    if ($Configuration -eq "Debug") {
        throw "DLL not found: $dllPath. Build first with cargo build --target x86_64-pc-windows-msvc"
    }

    throw "DLL not found: $dllPath. Build first with cargo build --release --target x86_64-pc-windows-msvc"
}

Stop-Explorer

if ($Action -eq "register") {
    & regsvr32.exe /s "$dllPath"
    Write-Host "Registered: $dllPath"
}
else {
    & regsvr32.exe /u /s "$dllPath"
    Write-Host "Unregistered: $dllPath"
}

Clear-ThumbnailCache
Write-Host "Cleared thumbnail cache: $env:LOCALAPPDATA\Microsoft\Windows\Explorer\thumbcache*"

Start-Explorer
Write-Host "Restarted explorer.exe"
