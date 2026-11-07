; Extra install and uninstall steps.
; This file must stay UTF-8 with a BOM: without one makensis reads the Russian text in the system
; code page. Language strings cannot be declared here (languages are defined after this include),
; so the text is chosen by language id at run time.

!define BEBRA_LANG_RUSSIAN 1049

; The installer and uninstaller run without elevation, but the player can choose or keep a folder
; that needs it, such as Program Files. There a failed Delete is silent: the uninstaller would
; report success and drop the Apps entry with the files still on disk. So look before changing
; anything. Sets $0 to 1 when $INSTDIR can be written, otherwise to 0.
!macro BEBRA_CHECK_INSTDIR_WRITABLE
  ClearErrors
  FileOpen $0 "$INSTDIR\.bebra-write-test" w
  ${If} ${Errors}
    ClearErrors
    StrCpy $0 0
  ${Else}
    FileClose $0
    Delete "$INSTDIR\.bebra-write-test"
    StrCpy $0 1
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro BEBRA_CHECK_INSTDIR_WRITABLE
  ${If} $0 = 0
    ${If} $LANGUAGE = ${BEBRA_LANG_RUSSIAN}
      MessageBox MB_ICONSTOP "Нет прав на запись в папку:$\r$\n$INSTDIR$\r$\n$\r$\nУстановка остановлена, ничего не изменено. Запустите установщик снова и выберите другую папку или запустите его от имени администратора." /SD IDOK
    ${Else}
      MessageBox MB_ICONSTOP "No permission to write to this folder:$\r$\n$INSTDIR$\r$\n$\r$\nSetup has stopped and nothing was changed. Run it again and choose another folder, or run it as administrator." /SD IDOK
    ${EndIf}
    SetErrorLevel 5
    Abort
  ${EndIf}
!macroend

; Undoes what the launcher's settings may have written to the registry.
; Links are removed only if they point at this installation (the official launcher may own them).
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro BEBRA_CHECK_INSTDIR_WRITABLE
  ${If} $0 = 0
    ${If} $LANGUAGE = ${BEBRA_LANG_RUSSIAN}
      MessageBox MB_ICONSTOP "Нет прав на удаление файлов из папки:$\r$\n$INSTDIR$\r$\n$\r$\nУдаление остановлено, ничего не изменено. Запустите uninstall.exe из этой папки от имени администратора." /SD IDOK
    ${Else}
      MessageBox MB_ICONSTOP "No permission to delete files from this folder:$\r$\n$INSTDIR$\r$\n$\r$\nUninstall has stopped and nothing was changed. Run uninstall.exe from this folder as administrator." /SD IDOK
    ${EndIf}
    SetErrorLevel 5
    Abort
  ${EndIf}

  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "BebraLauncher"

  ReadRegStr $0 HKCU "Software\Classes\ss14\shell\open\command" ""
  StrCmp $0 '"$INSTDIR\${MAINBINARYNAME}.exe" "%1"' 0 +2
    DeleteRegKey HKCU "Software\Classes\ss14"

  ReadRegStr $0 HKCU "Software\Classes\ss14s\shell\open\command" ""
  StrCmp $0 '"$INSTDIR\${MAINBINARYNAME}.exe" "%1"' 0 +2
    DeleteRegKey HKCU "Software\Classes\ss14s"
!macroend
