$ErrorActionPreference = 'Stop'

$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$packageName = 'cbxshell-rs'
$installerType = 'exe'

$arch = $env:PROCESSOR_ARCHITECTURE
if ($env:PROCESSOR_ARCHITEW6432) {
    $arch = $env:PROCESSOR_ARCHITEW6432
}

if ($arch -eq 'ARM64') {
    $installer = Get-ChildItem -Path $toolsDir -Filter 'CBXShell-rs-Setup-*-ARM64.exe' | Select-Object -First 1
} else {
    $installer = Get-ChildItem -Path $toolsDir -Filter 'CBXShell-rs-Setup-*-x64.exe' | Select-Object -First 1
}

if (-not $installer) {
    throw "Installer not found in $toolsDir. Run build_chocolatey.ps1 to stage the installers."
}

$packageArgs = @{
    packageName    = $packageName
    fileType       = $installerType
    file64         = $installer.FullName
    silentArgs     = '/S'
    validExitCodes = @(0)
}

Install-ChocolateyPackage @packageArgs
