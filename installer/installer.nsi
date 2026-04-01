!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"

; Product info
Name "Omni Overlay"
OutFile "..\dist\OmniSetup.exe"
InstallDir "$PROGRAMFILES64\Omni"
InstallDirRegKey HKCU "Software\OmniOverlay" "InstallDir"
RequestExecutionLevel admin

; MUI Settings
!define MUI_ABORTWARNING

; Installer pages
!insertmacro MUI_PAGE_LICENSE "license.txt"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\Omni.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Omni"
!insertmacro MUI_PAGE_FINISH

; Uninstaller pages
!insertmacro MUI_UNPAGE_CONFIRM
UninstPage custom un.DataCleanupPage un.DataCleanupPageLeave
!insertmacro MUI_UNPAGE_INSTFILES

; Language
!insertmacro MUI_LANGUAGE "English"

; Variables
Var DataCleanupCheckbox
Var RemoveUserData

; ─── Installer ───────────────────────────────────────────────────────────────

Section "Install"
  SetOutPath "$INSTDIR"

  ; Stop any running instances first
  nsExec::ExecToLog '"$INSTDIR\omni-host.exe" --stop'

  ; Electron app files (packaged by electron-builder)
  File /r "..\desktop\dist\win-unpacked\*.*"

  ; Rust binaries
  File "..\target\release\omni-host.exe"

  ; Overlay DLL
  SetOutPath "$INSTDIR\overlay"
  File "..\target\release\omni_overlay.dll"
  SetOutPath "$INSTDIR"

  ; Start Menu shortcut
  CreateDirectory "$SMPROGRAMS\Omni"
  CreateShortcut "$SMPROGRAMS\Omni\Omni.lnk" "$INSTDIR\Omni.exe"

  ; Write uninstaller
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; Registry for Add/Remove Programs
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OmniOverlay" \
    "DisplayName" "Omni Overlay"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OmniOverlay" \
    "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OmniOverlay" \
    "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\OmniOverlay" "InstallDir" "$INSTDIR"
SectionEnd

; ─── Uninstaller ─────────────────────────────────────────────────────────────

Function un.DataCleanupPage
  nsDialogs::Create 1018
  Pop $0

  ${NSD_CreateCheckBox} 0 0 100% 12u \
    "Also remove all user data (overlays, themes, configuration)"
  Pop $DataCleanupCheckbox
  ${NSD_SetState} $DataCleanupCheckbox ${BST_UNCHECKED}

  nsDialogs::Show
FunctionEnd

Function un.DataCleanupPageLeave
  ${NSD_GetState} $DataCleanupCheckbox $RemoveUserData
FunctionEnd

Section "Uninstall"
  ; Stop running instances and eject DLLs
  nsExec::ExecToLog '"$INSTDIR\omni-host.exe" --stop'

  ; Remove scheduled task if it exists
  nsExec::ExecToLog 'schtasks /delete /tn "OmniOverlay" /f'

  ; Remove installed files
  RMDir /r "$INSTDIR"

  ; Remove shortcuts
  RMDir /r "$SMPROGRAMS\Omni"
  Delete "$DESKTOP\Omni.lnk"

  ; Remove registry entries
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OmniOverlay"
  DeleteRegKey HKCU "Software\OmniOverlay"

  ; Conditionally remove user data
  ${If} $RemoveUserData == ${BST_CHECKED}
    RMDir /r "$APPDATA\Omni"
  ${EndIf}
SectionEnd
