!include "MUI2.nsh"
!include "FileFunc.nsh"

SetCompressor /SOLID lzma
SetCompressorDictSize 64

Name "Scythe"
OutFile "..\..\dist\scythe-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\scythe"
RequestExecutionLevel user

!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_RUN "$INSTDIR\scythe-ui.exe"
!define MUI_FINISHPAGE_RUN_PARAMETERS "--menu"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Scythe"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Scythe" SecMain
  SetOutPath "$INSTDIR"
  File /r "..\..\dist\bundle\*.*"

  ; Create Start Menu and Desktop shortcuts
  CreateDirectory "$SMPROGRAMS\scythe"
  CreateShortcut "$SMPROGRAMS\scythe\scythe.lnk" "$INSTDIR\scythe-ui.exe" "--menu"
  CreateShortcut "$DESKTOP\scythe.lnk" "$INSTDIR\scythe-ui.exe" "--menu"

  ; Register uninstaller with Windows Control Panel / Settings
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\scythe" "DisplayName" "Scythe Screen Recorder"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\scythe" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\scythe" "DisplayIcon" "$INSTDIR\scythe-ui.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\scythe" "Publisher" "oiupoyt"
SectionEnd

Section "Uninstall"
  Delete "$DESKTOP\scythe.lnk"
  RMDir /r "$SMPROGRAMS\scythe"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\scythe"
SectionEnd
