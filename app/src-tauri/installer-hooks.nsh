; Extra uninstall steps: undo what the launcher's settings may have written to the registry.
; Links are removed only if they point at this installation (the official launcher may own them).

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "BebraLauncher"

  ReadRegStr $0 HKCU "Software\Classes\ss14\shell\open\command" ""
  StrCmp $0 '"$INSTDIR\${MAINBINARYNAME}.exe" "%1"' 0 +2
    DeleteRegKey HKCU "Software\Classes\ss14"

  ReadRegStr $0 HKCU "Software\Classes\ss14s\shell\open\command" ""
  StrCmp $0 '"$INSTDIR\${MAINBINARYNAME}.exe" "%1"' 0 +2
    DeleteRegKey HKCU "Software\Classes\ss14s"
!macroend
