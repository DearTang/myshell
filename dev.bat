@echo off
chcp 65001 >nul
set "PATH=C:\Users\argus\.cargo\bin;%PATH%"
echo === Starting Tauri Dev ===
"C:\Users\argus\.cargo\bin\cargo.exe" --version
npx tauri dev
