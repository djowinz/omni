!macro customUnInit
  ; Stop running instances before uninstall
  nsExec::ExecToLog '"$INSTDIR\omni-host.exe" --stop'
!macroend

!macro customRemoveFiles
  MessageBox MB_YESNO "Also remove all user data (overlays, themes, configuration)?" IDYES removeData IDNO skipData
  removeData:
    RMDir /r "$APPDATA\Omni"
  skipData:
!macroend
