; CBXShell NSIS Installer Script
; Provides thumbnail previews for comic book archives (CBZ/CBR/CB7)
;
; Build with: makensis.exe /DARCH=x64 installer.nsi
;         or: makensis.exe /DARCH=ARM64 installer.nsi
; Optional:   /DBUILD_PROFILE=Debug (default: Release)

!include "MUI2.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"

;--------------------------------
; Architecture Configuration
; Must be passed via command line: /DARCH=x64 or /DARCH=ARM64

!ifndef BUILD_PROFILE
  !define BUILD_PROFILE "Release"
!endif

!if "${BUILD_PROFILE}" == "Release"
  !define PROFILE_DIR "release"
  !define PROFILE_TAG ""
!else if "${BUILD_PROFILE}" == "Debug"
  !define PROFILE_DIR "debug"
  !define PROFILE_TAG "-debug"
!else
  !error "Invalid BUILD_PROFILE value! Must be Release or Debug"
!endif

!ifndef ARCH
  !error "ARCH must be defined! Use /DARCH=x64 or /DARCH=ARM64"
!endif

!if "${ARCH}" == "x64"
  !define ARCH_NAME "x64"
  !define ARCH_BITS "64-bit"
  !define BUILD_DIR "target\x86_64-pc-windows-msvc\${PROFILE_DIR}"
  !define PROGRAMFILES_DIR "$PROGRAMFILES64"
!else if "${ARCH}" == "ARM64"
  !define ARCH_NAME "ARM64"
  !define ARCH_BITS "ARM64 (64-bit)"
  !define BUILD_DIR "target\aarch64-pc-windows-msvc\${PROFILE_DIR}"
  !define PROGRAMFILES_DIR "$PROGRAMFILES64"
!else
  !error "Invalid ARCH value! Must be x64 or ARM64"
!endif

;--------------------------------
; Configuration

!define PRODUCT_NAME "CBXShell-rs"
!define PRODUCT_VERSION "5.1.1"
!define PRODUCT_WEB_SITE "https://github.com/Clickin/CBXShell-rs"
!define PRODUCT_UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
!define PRODUCT_UNINST_ROOT_KEY "HKLM"
!define PRODUCT_SETTINGS_KEY "Software\CBXShell-rs\{9E6ECB90-5A61-42BD-B851-D3297D9C7F39}"

; CLSID for COM registration
!define CLSID "{9E6ECB90-5A61-42BD-B851-D3297D9C7F39}"

Name "${PRODUCT_NAME} ${PRODUCT_VERSION} (${ARCH_BITS})"
!ifdef SNAPSHOT_TIMESTAMP
  OutFile "dist\CBXShell-rs-Setup-${PRODUCT_VERSION}-snapshot-${SNAPSHOT_TIMESTAMP}-${ARCH_NAME}${PROFILE_TAG}.exe"
!else
  OutFile "dist\CBXShell-rs-Setup-${PRODUCT_VERSION}-${ARCH_NAME}${PROFILE_TAG}.exe"
!endif
InstallDir "${PROGRAMFILES_DIR}\CBXShell"
InstallDirRegKey HKLM "${PRODUCT_UNINST_KEY}" "InstallLocation"
ShowInstDetails show
ShowUnInstDetails show
RequestExecutionLevel admin

;--------------------------------
; Version Information

VIProductVersion "${PRODUCT_VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "Comments" "Windows Shell Extension for comic book archive thumbnails"
VIAddVersionKey "FileDescription" "${PRODUCT_NAME} Installer"
VIAddVersionKey "FileVersion" "${PRODUCT_VERSION}"
VIAddVersionKey "ProductVersion" "${PRODUCT_VERSION}"

;--------------------------------
; Modern UI Configuration

!define MUI_ABORTWARNING
!define MUI_ICON "${NSISDIR}\Contrib\Graphics\Icons\modern-install.ico"
!define MUI_UNICON "${NSISDIR}\Contrib\Graphics\Icons\modern-uninstall.ico"
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_BITMAP "${NSISDIR}\Contrib\Graphics\Header\nsis3-metro.bmp"
!define MUI_WELCOMEFINISHPAGE_BITMAP "${NSISDIR}\Contrib\Graphics\Wizard\nsis3-metro.bmp"

; Installer pages
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "LICENSE.txt"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_RUN "$INSTDIR\CBXManager.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch CBXShell Manager"
!define MUI_FINISHPAGE_SHOWREADME "$INSTDIR\README.md"
!define MUI_FINISHPAGE_SHOWREADME_TEXT "View README"
!insertmacro MUI_PAGE_FINISH

; Uninstaller pages
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

;--------------------------------
; Helper Functions

Function .onInit
!if "${ARCH}" == "x64"
  ; Check if running on 64-bit Windows
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "This is a 64-bit installer and requires 64-bit Windows.$\n$\nPlease visit https://github.com/Clickin/CBXShell-rs/releases for more information."
    Abort
  ${EndIf}
!else if "${ARCH}" == "ARM64"
  ; Check if running on ARM64 Windows
  ${IfNot} ${IsNativeARM64}
    MessageBox MB_OK|MB_ICONSTOP "This is an ARM64 installer and requires ARM64 Windows.$\n$\nPlease download the x64 installer instead."
    Abort
  ${EndIf}
!endif

  ; Check if already installed
  ReadRegStr $R0 ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "UninstallString"
  StrCmp $R0 "" done

  MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION \
  "${PRODUCT_NAME} is already installed.$\n$\nClick OK to remove the previous version or Cancel to cancel this installation." \
  IDOK uninst
  Abort

uninst:
  ExecWait '"$R0" /S _?=$INSTDIR'
  Delete "$R0"

done:
FunctionEnd

Function RegisterDLL
  ; Parameters: DLL path on stack
  Pop $R0

  DetailPrint "Registering: $R0"
  ExecWait 'regsvr32.exe /s "$R0"' $1

  ${If} $1 != 0
    DetailPrint "Warning: Registration failed for $R0 (Error: $1)"
    MessageBox MB_OK|MB_ICONEXCLAMATION "Failed to register $R0. Error code: $1"
  ${Else}
    DetailPrint "Successfully registered: $R0"
  ${EndIf}
FunctionEnd

Function Un.RegisterDLL
  ; Parameters: DLL path on stack
  Pop $R0

  DetailPrint "Unregistering: $R0"
  ExecWait 'regsvr32.exe /s /u "$R0"'
  DetailPrint "Unregistered: $R0"
FunctionEnd

;--------------------------------
; Installation Sections

Section "CBXShell (Required)" SecCore
  SectionIn RO
  SetOutPath "$INSTDIR"

  ; Install CBXShell.dll for the target architecture
  DetailPrint "Installing ${ARCH_BITS} version..."
  File "${BUILD_DIR}\CBXShell.dll"

  ; Register DLL
  Push "$INSTDIR\CBXShell.dll"
  Call RegisterDLL

  ; Note: UnRAR support is statically linked via unrar crate, no separate DLL needed

  ; Create registry entries for shell extension settings
  ; NoSort=1 (default): Fast mode - return first image found (recommended for large archives)
  ; NoSort=0: Sort alphabetically - slower but predictable order
  WriteRegDWORD HKCU "${PRODUCT_SETTINGS_KEY}" "NoSort" 1

  ; Write uninstaller
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; Write registry keys for Add/Remove Programs
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayName" "${PRODUCT_NAME} (${ARCH_BITS})"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\CBXManager.exe"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "URLInfoAbout" "${PRODUCT_WEB_SITE}"
  WriteRegStr ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegDWORD ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "NoModify" 1
  WriteRegDWORD ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "NoRepair" 1

  ; Calculate installed size
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}" "EstimatedSize" "$0"

  ; Install documentation
  File "README.md"
  File "LICENSE.txt"
SectionEnd

Section "CBXManager (Configuration Tool)" SecManager
  SetOutPath "$INSTDIR"

  ; Install manager executable
  File "${BUILD_DIR}\CBXManager.exe"

  ; Create Start Menu shortcuts
  CreateDirectory "$SMPROGRAMS\${PRODUCT_NAME}"
  CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\CBXShell Manager.lnk" "$INSTDIR\CBXManager.exe"
  CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall.lnk" "$INSTDIR\Uninstall.exe"
  CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\README.lnk" "$INSTDIR\README.md"

  ; Create Desktop shortcut (optional)
  CreateShortCut "$DESKTOP\CBXShell Manager.lnk" "$INSTDIR\CBXManager.exe"
SectionEnd

Section "Enable for ZIP files" SecZIP
  ; Enable thumbnail handler for .zip files
  WriteRegStr HKCU "Software\Classes\.zip\shellex\{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}" "" "${CLSID}"
  WriteRegStr HKCU "Software\Classes\.zip\shellex\{00021500-0000-0000-C000-000000000046}" "" "${CLSID}"
SectionEnd

Section "Enable for CBZ files" SecCBZ
  ; Enable thumbnail handler for .cbz files
  WriteRegStr HKCU "Software\Classes\.cbz\shellex\{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}" "" "${CLSID}"
  WriteRegStr HKCU "Software\Classes\.cbz\shellex\{00021500-0000-0000-C000-000000000046}" "" "${CLSID}"
SectionEnd

Section "Enable for RAR files" SecRAR
  ; Enable thumbnail handler for .rar files
  WriteRegStr HKCU "Software\Classes\.rar\shellex\{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}" "" "${CLSID}"
  WriteRegStr HKCU "Software\Classes\.rar\shellex\{00021500-0000-0000-C000-000000000046}" "" "${CLSID}"
SectionEnd

Section "Enable for CBR files" SecCBR
  ; Enable thumbnail handler for .cbr files
  WriteRegStr HKCU "Software\Classes\.cbr\shellex\{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}" "" "${CLSID}"
  WriteRegStr HKCU "Software\Classes\.cbr\shellex\{00021500-0000-0000-C000-000000000046}" "" "${CLSID}"
SectionEnd

Section "Enable for 7Z files" Sec7Z
  ; Enable thumbnail handler for .7z files
  WriteRegStr HKCU "Software\Classes\.7z\shellex\{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}" "" "${CLSID}"
  WriteRegStr HKCU "Software\Classes\.7z\shellex\{00021500-0000-0000-C000-000000000046}" "" "${CLSID}"
  WriteRegStr HKCU "Software\Classes\.cb7\shellex\{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}" "" "${CLSID}"
  WriteRegStr HKCU "Software\Classes\.cb7\shellex\{00021500-0000-0000-C000-000000000046}" "" "${CLSID}"
SectionEnd

;--------------------------------
; Section Descriptions

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecCore} "Core shell extension DLL (required)"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecManager} "Configuration tool for managing file type associations"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecZIP} "Enable thumbnail preview for .zip files"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecCBZ} "Enable thumbnail preview for .cbz (Comic Book ZIP) files"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecRAR} "Enable thumbnail preview for .rar files"
  !insertmacro MUI_DESCRIPTION_TEXT ${SecCBR} "Enable thumbnail preview for .cbr (Comic Book RAR) files"
  !insertmacro MUI_DESCRIPTION_TEXT ${Sec7Z} "Enable thumbnail preview for .7z and .cb7 files"
!insertmacro MUI_FUNCTION_DESCRIPTION_END

;--------------------------------
; Uninstaller Section

Section "Uninstall"
  ; Unregister DLL
  Push "$INSTDIR\CBXShell.dll"
  Call Un.RegisterDLL

  ; Remove registry entries for file associations
  DeleteRegKey HKCU "Software\Classes\.zip\shellex\{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}"
  DeleteRegKey HKCU "Software\Classes\.zip\shellex\{00021500-0000-0000-C000-000000000046}"
  DeleteRegKey HKCU "Software\Classes\.cbz\shellex\{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}"
  DeleteRegKey HKCU "Software\Classes\.cbz\shellex\{00021500-0000-0000-C000-000000000046}"
  DeleteRegKey HKCU "Software\Classes\.rar\shellex\{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}"
  DeleteRegKey HKCU "Software\Classes\.rar\shellex\{00021500-0000-0000-C000-000000000046}"
  DeleteRegKey HKCU "Software\Classes\.cbr\shellex\{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}"
  DeleteRegKey HKCU "Software\Classes\.cbr\shellex\{00021500-0000-0000-C000-000000000046}"
  DeleteRegKey HKCU "Software\Classes\.7z\shellex\{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}"
  DeleteRegKey HKCU "Software\Classes\.7z\shellex\{00021500-0000-0000-C000-000000000046}"
  DeleteRegKey HKCU "Software\Classes\.cb7\shellex\{BB2E617C-0920-11d1-9A0B-00C04FC2D6C1}"
  DeleteRegKey HKCU "Software\Classes\.cb7\shellex\{00021500-0000-0000-C000-000000000046}"

  ; Remove settings
  DeleteRegKey HKCU "${PRODUCT_SETTINGS_KEY}"

  ; Remove shortcuts
  Delete "$SMPROGRAMS\${PRODUCT_NAME}\*.*"
  RMDir "$SMPROGRAMS\${PRODUCT_NAME}"
  Delete "$DESKTOP\CBXShell Manager.lnk"

  ; Remove files
  Delete "$INSTDIR\CBXShell.dll"
  Delete "$INSTDIR\CBXManager.exe"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\Uninstall.exe"

  ; Remove installation directory
  RMDir "$INSTDIR"

  ; Remove registry keys
  DeleteRegKey ${PRODUCT_UNINST_ROOT_KEY} "${PRODUCT_UNINST_KEY}"

  ; Notify Windows to refresh icon cache
  System::Call 'shell32.dll::SHChangeNotify(i, i, i, i) v (0x08000000, 0, 0, 0)'

  MessageBox MB_OK "CBXShell has been successfully removed from your computer.$\n$\nNote: You may need to restart Windows Explorer for changes to take full effect."
SectionEnd
