; IDIN NSIS installer hooks — runs at install time (user just clicked Next).
; Registers the native-messaging host for Chrome/Edge/Firefox and stages the
; extension folder, so browser integration works immediately after install.

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "IDIN: registering browser integration..."

  Var /GLOBAL IDIN_HOSTDIR
  StrCpy $IDIN_HOSTDIR "$LOCALAPPDATA\IDIN"
  CreateDirectory "$IDIN_HOSTDIR"

  ; Copy host binary + launcher + extension next to the manifest.
  CopyFiles /SILENT "$INSTDIR\resources\idin-host.exe" "$IDIN_HOSTDIR\idin-host.exe"
  CopyFiles /SILENT "$INSTDIR\resources\extension\*.*" "$IDIN_HOSTDIR\extension\"

  ; Host manifest (allowed_origins wildcard via * is not supported by Chrome,
  ; so we write an empty ID that register-host.ps1 / app can refine later).
  FileOpen $0 "$IDIN_HOSTDIR\com.hatnux.idin.json" w
  FileWrite $0 '{"name": "com.hatnux.idin",'
  FileWrite $0 '"description": "IDIN Download Manager native messaging host",'
  FileWrite $0 '"path": "$IDIN_HOSTDIR\idin-native-host.bat",'
  FileWrite $0 '"type": "stdio",'
  FileWrite $0 '"allowed_origins": ["chrome-extension://EXTENSION_ID/"]}'
  FileClose $0

  FileOpen $1 "$IDIN_HOSTDIR\idin-native-host.bat" w
  FileWrite $1 '@echo off'
  FileWrite $1 '"$IDIN_HOSTDIR\idin-host.exe"'
  FileClose $1

  ; Register for Chrome, Edge, Firefox (HKCU — no admin prompt).
  WriteRegStr HKCU "Software\Google\Chrome\NativeMessagingHosts\com.hatnux.idin" "" "$IDIN_HOSTDIR\com.hatnux.idin.json"
  WriteRegStr HKCU "Software\Microsoft\Edge\NativeMessagingHosts\com.hatnux.idin" "" "$IDIN_HOSTDIR\com.hatnux.idin.json"
  WriteRegStr HKCU "Software\Mozilla\NativeMessagingHosts\com.hatnux.idin" "" "$IDIN_HOSTDIR\com.hatnux.idin.json"

  DetailPrint "IDIN: browser integration registered (host + 3 browsers)."
!macroend
