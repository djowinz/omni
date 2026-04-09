!macro customUnInit
  ; Stop running instances before uninstall
  nsExec::ExecToLog '"$INSTDIR\omni-host.exe" --stop'
!macroend

!macro customRemoveFiles
  ; Only prompt for data cleanup during user-initiated uninstalls, not silent updates.
  ; electron-updater passes /S for silent uninstall during auto-update.
  ${IfNot} ${Silent}
    MessageBox MB_YESNO "Also remove all user data (overlays, themes, configuration)?" IDYES removeData IDNO skipData
    removeData:
      RMDir /r "$APPDATA\Omni"
    skipData:
  ${EndIf}
!macroend
