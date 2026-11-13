; Prism Reference Manager installer hooks.
; The uninstaller removes the program's own install folder (handled by the standard
; uninstaller, which deletes only the files it installed) and the program's own settings
; folders below. It never touches research libraries, which live in folders the person
; chose (by default Documents\Prism Reference Libraries).

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    SetShellVarContext current
    ; Settings, device id and window state.
    RmDir /r "$APPDATA\org.prism.referencemanager"
    ; Embedded web view cache for this program only.
    RmDir /r "$LOCALAPPDATA\org.prism.referencemanager"
  ${EndIf}
!macroend
