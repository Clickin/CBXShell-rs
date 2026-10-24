param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [switch]$ResourceOnly
)
$ErrorActionPreference = 'Stop'
$Executable = (Resolve-Path -LiteralPath $Executable).Path

if (-not ('ManagerSmokeNative' -as [type])) {
    Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class ManagerSmokeNative {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr LoadLibraryEx(string path, IntPtr file, uint flags);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr type);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll")]
    public static extern uint SizeofResource(IntPtr module, IntPtr resource);
    [DllImport("kernel32.dll")]
    public static extern IntPtr LockResource(IntPtr resource);
    [DllImport("kernel32.dll")]
    public static extern bool FreeLibrary(IntPtr module);
    private delegate bool EnumProc(IntPtr window, IntPtr arg);
    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumProc callback, IntPtr arg);
    [DllImport("user32.dll")]
    private static extern bool EnumChildWindows(IntPtr parent, EnumProc callback, IntPtr arg);
    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr window, uint message, IntPtr w, IntPtr l);
    public static string Text(IntPtr window) {
        var text = new StringBuilder(512);
        GetWindowText(window, text, text.Capacity);
        return text.ToString();
    }
    public static IntPtr FindManager(uint process) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, arg) => {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner == process && Text(window) == "CBXShell Manager") {
                found = window;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
    public static string[] Children(IntPtr parent) {
        var texts = new List<string>();
        EnumChildWindows(parent, (window, arg) => { texts.Add(Text(window).Replace("&", "")); return true; }, IntPtr.Zero);
        return texts.ToArray();
    }
}
"@
}

# Map resources without executing code. Also works for the cross-built ARM64 EXE.
$module = [ManagerSmokeNative]::LoadLibraryEx($Executable, [IntPtr]::Zero, 0x22)
if ($module -eq [IntPtr]::Zero) { throw "Cannot map executable resources: $Executable" }
try {
    $resource = [ManagerSmokeNative]::FindResource($module, [IntPtr]1, [IntPtr]24)
    if ($resource -eq [IntPtr]::Zero) { throw 'Missing embedded application manifest (RT_MANIFEST #1)' }
    $size = [ManagerSmokeNative]::SizeofResource($module, $resource)
    $data = [ManagerSmokeNative]::LockResource([ManagerSmokeNative]::LoadResource($module, $resource))
    if ($size -eq 0 -or $data -eq [IntPtr]::Zero) { throw 'Empty or unreadable application manifest' }
    $bytes = New-Object byte[] $size
    [Runtime.InteropServices.Marshal]::Copy($data, $bytes, 0, $size)
    [xml]$manifest = [Text.Encoding]::UTF8.GetString($bytes).Trim([char]0, [char]0xFEFF)
    $ns = New-Object Xml.XmlNamespaceManager($manifest.NameTable)
    $ns.AddNamespace('asm', 'urn:schemas-microsoft-com:asm.v1')
    $dependency = $manifest.SelectSingleNode('/asm:assembly/asm:dependency/asm:dependentAssembly/asm:assemblyIdentity[@name="Microsoft.Windows.Common-Controls"]', $ns)
    if (-not $dependency -or $dependency.version -ne '6.0.0.0' -or $dependency.publicKeyToken -ne '6595b64144ccf1df') {
        throw 'Missing Common Controls v6 dependency in embedded manifest'
    }
    Write-Output 'PASS: embedded Common Controls v6 manifest'
} finally {
    [void][ManagerSmokeNative]::FreeLibrary($module)
}
if ($ResourceOnly) { return }

# No settings are applied: construct the real GUI, check controls, then close it.
$process = Start-Process -FilePath $Executable -WorkingDirectory (Split-Path $Executable) -WindowStyle Hidden -PassThru
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    $window = [IntPtr]::Zero
    do {
        $process.Refresh()
        if ($process.HasExited) { throw "Manager exited before creating its GUI (exit $($process.ExitCode))" }
        $window = [ManagerSmokeNative]::FindManager([uint32]$process.Id)
        if ($window -ne [IntPtr]::Zero) {
            $controls = [ManagerSmokeNative]::Children($window)
            if ('CBZ Image Archives' -in $controls -and 'Apply' -in $controls -and 'Cancel' -in $controls) { break }
        }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($window -eq [IntPtr]::Zero -or 'CBZ Image Archives' -notin $controls -or 'Apply' -notin $controls -or 'Cancel' -notin $controls) {
        throw 'Manager did not construct its expected window and controls'
    }
    if (-not [ManagerSmokeNative]::PostMessage($window, 0x10, [IntPtr]::Zero, [IntPtr]::Zero)) { throw 'Cannot close manager window' }
    if (-not $process.WaitForExit(10000)) { throw 'Manager did not respond to window close' }
    if ($process.ExitCode -ne 0) { throw "Manager exited with code $($process.ExitCode)" }
    Write-Output 'PASS: manager GUI, archive controls, Apply/Cancel, and clean exit'
} finally {
    if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    $process.Dispose()
}
