@echo off
rem ============================================================
rem  IDIN release pipeline — one command does everything:
rem    1. build native-messaging host (idin-host.exe)
rem    2. build the app (npm run tauri build)
rem    3. stage host + extension next to the installer payloads
rem  Usage:  scripts\release.cmd          (build only)
rem          scripts\release.cmd sign     (also sign with signtool
rem                                        if SIGNTOOL_PFX + SIGNTOOL_PWD set)
rem ============================================================
setlocal enabledelayedexpansion
cd /d "%~dp0.."

echo [1/3] Building native-messaging host...
pushd src-host
cargo build --release || goto :err
popd

echo [2/3] Building app + installers...
npm run tauri build || goto :err

echo [3/3] Staging host binary for installer bundling...
set "STAGE=src-tauri\resources"
if not exist "%STAGE%" mkdir "%STAGE%"
copy /y src-host\target\release\idin-host.exe "%STAGE%\" >nul || goto :err
xcopy /e /i /y extension "%STAGE%\extension" >nul || goto :err

if /i "%1"=="sign" call :sign

echo.
echo DONE. Installers:
dir /b src-tauri\target\release\bundle\nsis\*.exe src-tauri\target\release\bundle\msi\*.msi
exit /b 0

:sign
echo [sign] Signing binaries with Authenticode...
where signtool >nul 2>nul || (echo signtool not in PATH - skipping & exit /b 0)
if not defined SIGNTOOL_PFX (echo SIGNTOOL_PFX not set - skipping & exit /b 0)
signtool sign /fd SHA256 /f "%SIGNTOOL_PFX%" /p "%SIGNTOOL_PWD%" ^
  src-tauri\target\release\idin.exe ^
  src-host\target\release\idin-host.exe ^
  src-tauri\target\release\bundle\nsis\*.exe
exit /b 0

:err
echo FAILED.
exit /b 1
