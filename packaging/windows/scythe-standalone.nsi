!include "FileFunc.nsh"
!include "LogicLib.nsh"

Name "Scythe"
OutFile "..\..\dist\scythe.exe"
Caption "Scythe Screen Recorder"
SilentInstall silent
RequestExecutionLevel user

Section
  ; Terminate any running instances so locked executables and DLLs can be updated cleanly
  nsExec::Exec 'cmd.exe /C taskkill /F /IM scythe-ui.exe /T >nul 2>&1'
  nsExec::Exec 'cmd.exe /C taskkill /F /IM scythe-daemon.exe /T >nul 2>&1'
  Sleep 500

  SetOverwrite on
  ; Extract self-contained binaries and all runtime DLLs directly to user app data
  SetOutPath "$LOCALAPPDATA\scythe"
  File /r "..\..\dist\bundle\*.*"

  ; Execute scythe-ui with any forwarded command-line arguments, or default to overlay menu
  ${GetParameters} $R0
  ${If} $R0 == ""
    Exec '"$LOCALAPPDATA\scythe\scythe-ui.exe" --menu'
  ${Else}
    Exec '"$LOCALAPPDATA\scythe\scythe-ui.exe" $R0'
  ${EndIf}
SectionEnd
