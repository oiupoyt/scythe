!include "FileFunc.nsh"
!include "LogicLib.nsh"

Name "Scythe"
OutFile "..\..\dist\scythe.exe"
Caption "Scythe Screen Recorder"
SilentInstall silent
RequestExecutionLevel user

Section
  ; Terminate any running instances so locked executables and DLLs can be updated cleanly
  nsExec::Exec 'taskkill /F /IM scythe-ui.exe /T'
  nsExec::Exec 'taskkill /F /IM scythe-daemon.exe /T'
  nsExec::Exec 'taskkill /F /IM vrec-ui.exe /T'
  nsExec::Exec 'taskkill /F /IM vrec-daemon.exe /T'
  nsExec::Exec 'powershell -NoProfile -NonInteractive -WindowStyle Hidden -Command "Get-Process -Name scythe-ui, scythe-daemon, vrec-ui, vrec-daemon -ErrorAction SilentlyContinue | Stop-Process -Force"'
  Sleep 1500

  SetOverwrite try
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
