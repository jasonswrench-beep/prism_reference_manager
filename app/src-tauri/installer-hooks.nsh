; Prism Reference Manager installer hooks.
; The uninstaller removes the program's own install folder (handled by the standard
; uninstaller, which deletes only the files it installed) and the program's own settings
; folders below. It never touches research libraries, which live in folders the person
; chose (by default Documents\Prism Reference Libraries).

; prism-rm:// links open items in the program (for this user only).
!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Classes\prism-rm" "" "URL:Prism Reference Manager link"
  WriteRegStr HKCU "Software\Classes\prism-rm" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\prism-rm\DefaultIcon" "" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr HKCU "Software\Classes\prism-rm\shell\open\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" "%1"'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ; Only the link registration this program added.
    DeleteRegKey HKCU "Software\Classes\prism-rm"
    SetShellVarContext current
    ; Settings, device id and window state.
    RmDir /r "$APPDATA\org.prism.referencemanager"
    ; Embedded web view cache for this program only.
    RmDir /r "$LOCALAPPDATA\org.prism.referencemanager"
  ${EndIf}
!macroend
