# Register the IDIN native messaging host for Chrome / Edge / Firefox (Windows).
# Usage (PowerShell, from repo root):
#   powershell -ExecutionPolicy Bypass -File scripts\register-host.ps1 -ExtensionId <your-extension-id>
param(
  [Parameter(Mandatory = $true)]
  [string]$ExtensionId
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$manifestSrc = Join-Path $PSScriptRoot 'com.hatnux.idin.json'
$installDir = Join-Path $env:LOCALAPPDATA 'IDIN'
New-Item -ItemType Directory -Force -Path $installDir | Out-Null

# Copy host manifest + the compiled host exe next to it.
# (The manifest points directly at idin-host.exe — the old .bat wrapper
#  caused "Error 32: broken pipe" with Chrome and is no longer used.)
$manifestDst = Join-Path $installDir 'com.hatnux.idin.json'
$exeSrc = Join-Path $repoRoot 'src-tauri\resources\idin-host.exe'
$exeDst = Join-Path $installDir 'idin-host.exe'
if (-not (Test-Path $exeSrc)) {
  Write-Error "Host exe not found at '$exeSrc'. Build it first (npm run tauri build or scripts\release.cmd)."
}
Copy-Item $manifestSrc $manifestDst -Force
Copy-Item $exeSrc $exeDst -Force

# Fill in the real extension ID
$json = Get-Content $manifestDst -Raw
$json = $json -replace 'EXTENSION_ID_PLACEHOLDER', $ExtensionId
Set-Content -Path $manifestDst -Value $json -Encoding UTF8

# Registry keys: Chrome, Edge, Firefox
$keys = @(
  'HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.hatnux.idin',
  'HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.hatnux.idin',
  'HKCU:\Software\Mozilla\NativeMessagingHosts\com.hatnux.idin'
)
foreach ($k in $keys) {
  New-Item -Path $k -Force | Out-Null
  Set-ItemProperty -Path $k -Name '(Default)' -Value $manifestDst
}

Write-Host "IDIN host registered for Chrome, Edge, and Firefox."
Write-Host "Manifest: $manifestDst"
Write-Host "Host exe: $exeDst"
