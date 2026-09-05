!include "MUI2.nsh"
!include "FileFunc.nsh"

Name "vrec"
OutFile "..\..\dist\vrec-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\vrec"
RequestExecutionLevel user

!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_RUN "$INSTDIR\vrec-ui.exe"
!define MUI_FINISHPAGE_RUN_PARAMETERS "--menu"
!define MUI_FINISHPAGE_RUN_TEXT "Launch vrec"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "vrec" SecMain
  SetOutPath "$INSTDIR"
  File /r "..\..\dist\bundle\*.*"

  ; Create Start Menu and Desktop shortcuts
  CreateDirectory "$SMPROGRAMS\vrec"
  CreateShortcut "$SMPROGRAMS\vrec\vrec.lnk" "$INSTDIR\vrec-ui.exe" "--menu"
  CreateShortcut "$DESKTOP\vrec.lnk" "$INSTDIR\vrec-ui.exe" "--menu"

  ; Register uninstaller with Windows Control Panel / Settings
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\vrec" "DisplayName" "vrec Screen Recorder"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\vrec" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\vrec" "DisplayIcon" "$INSTDIR\vrec-ui.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\vrec" "Publisher" "oiupoyt"
SectionEnd

Section "Uninstall"
  Delete "$DESKTOP\vrec.lnk"
  RMDir /r "$SMPROGRAMS\vrec"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\vrec"
SectionEnd
