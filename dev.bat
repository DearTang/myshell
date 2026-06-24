@echo off
chcp 65001 >nul
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
echo === Starting Tauri Dev ===
"%USERPROFILE%\.cargo\bin\cargo.exe" --version
npx tauri dev
