; NSIS installer hooks (tauri.windows.conf.json: bundle.windows.nsis.installerHooks).
;
; Windows Error Reporting only calls BugSplatWer.dll if its full path is a value under
; RuntimeExceptionHelperModules; without it, fail-fast crashes (__fastfail, /GS buffer overruns)
; produce no report. SHCTX is HKCU for Tauri's default per-user install (installMode
; "currentUser", no elevation) and HKLM for "perMachine". Tauri has already selected the 64-bit
; registry view for x64 and arm64 installers.
!define BUGSPLAT_WER_KEY "SOFTWARE\Microsoft\Windows\Windows Error Reporting\RuntimeExceptionHelperModules"

!macro NSIS_HOOK_POSTINSTALL
  WriteRegDWORD SHCTX "${BUGSPLAT_WER_KEY}" "$INSTDIR\BugSplatWer.dll" 0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; An update runs the old uninstaller first; the new installer writes the same value again.
  ${If} $UpdateMode <> 1
    DeleteRegValue SHCTX "${BUGSPLAT_WER_KEY}" "$INSTDIR\BugSplatWer.dll"
  ${EndIf}
!macroend
