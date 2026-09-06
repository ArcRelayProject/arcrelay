; ArcRelay's business data lives outside Tauri's bundle-id directory.
; Keep it during upgrades and ordinary uninstalls. Only explicit removal of
; application data in the interactive uninstaller may remove this directory.
!macro NSIS_HOOK_POSTINSTALL
  ; The static verb is available on every supported Windows 10/11 build. The
  ; sparse package additionally registers ArcRelay in the system Share sheet
  ; on Windows 10 2004 and newer when a signed package was produced by CI.
  WriteRegStr SHCTX "Software\Classes\*\shell\ArcRelay.Share" "" "Share with ArcRelay"
  WriteRegStr SHCTX "Software\Classes\*\shell\ArcRelay.Share" "MUIVerb" "Share with ArcRelay"
  WriteRegStr SHCTX "Software\Classes\*\shell\ArcRelay.Share" "Icon" '"$INSTDIR\${MAINBINARYNAME}.exe",0'
  WriteRegStr SHCTX "Software\Classes\*\shell\ArcRelay.Share" "MultiSelectModel" "Player"
  WriteRegStr SHCTX "Software\Classes\*\shell\ArcRelay.Share" "Position" "Top"
  WriteRegStr SHCTX "Software\Classes\*\shell\ArcRelay.Share\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" --arcrelay-share-source windows-context-menu --arcrelay-share "%1"'

  IfFileExists "$INSTDIR\system-share\windows\ArcRelay.SystemShare.msix" 0 system_share_registration_done
    nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\system-share\windows\register-sparse-package.ps1" -ExternalLocation "$INSTDIR" -PackagePath "$INSTDIR\system-share\windows\ArcRelay.SystemShare.msix"'
    Pop $0
    ${If} $0 != 0
      DetailPrint "ArcRelay system Share registration failed (exit $0). Share with ArcRelay remains available. See $APPDATA\ArcRelay\system-share\registration.log."
    ${EndIf}
  system_share_registration_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegKey SHCTX "Software\Classes\*\shell\ArcRelay.Share"
  IfFileExists "$INSTDIR\system-share\windows\unregister-sparse-package.ps1" 0 system_share_unregistration_done
    nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\system-share\windows\unregister-sparse-package.ps1"'
    Pop $0
  system_share_unregistration_done:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = ${BST_CHECKED}
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "$APPDATA\ArcRelay"
  ${EndIf}
!macroend
