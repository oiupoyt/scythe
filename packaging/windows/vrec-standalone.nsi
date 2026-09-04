!include "FileFunc.nsh"
!include "LogicLib.nsh"

Name "vrec"
OutFile "..\..\dist\vrec.exe"
Caption "vrec Screen Recorder"
SilentInstall silent
RequestExecutionLevel user

Section
  ; Extract self-contained binaries and all runtime DLLs directly to user app data
  SetOutPath "$LOCALAPPDATA\vrec"
  File /r "..\..\dist\bundle\*.*"

  ; Execute vrec-ui with any forwarded command-line arguments, or default to overlay menu
  ${GetParameters} $R0
  ${If} $R0 == ""
    Exec '"$LOCALAPPDATA\vrec\vrec-ui.exe" --menu'
  ${Else}
    Exec '"$LOCALAPPDATA\vrec\vrec-ui.exe" $R0'
  ${EndIf}
SectionEnd
