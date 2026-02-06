$ErrorActionPreference = 'Stop'

$packageName = 'cbxshell-rs'
$installerType = 'exe'

$keys = Get-UninstallRegistryKey -SoftwareName 'CBXShell-rs*'
if (-not $keys) {
    Write-Warning 'CBXShell-rs is not installed.'
    return
}

$uninstaller = $keys[0]
if (-not $uninstaller.UninstallString) {
    throw 'Unable to find uninstall string for CBXShell-rs.'
}

$packageArgs = @{
    packageName    = $packageName
    fileType       = $installerType
    file           = $uninstaller.UninstallString
    silentArgs     = '/S'
    validExitCodes = @(0)
}

Uninstall-ChocolateyPackage @packageArgs
