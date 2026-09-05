!include "FileFunc.nsh"
!include "LogicLib.nsh"

Name "Scythe"
OutFile "..\..\dist\scythe.exe"
Caption "Scythe Screen Recorder"
SilentInstall silent
RequestExecutionLevel user

Section
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
