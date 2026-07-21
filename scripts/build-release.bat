@echo off
REM Build script for MyShell — ensures MCP binary is compiled before Tauri packaging.
REM Usage: scripts\build-release.bat
REM Usage: scripts\build-release.bat --debug

cd /d "%~dp0\.."

set PROFILE=release
if "%~1"=="--debug" set PROFILE=debug

echo === Building myshell-mcp (%PROFILE%) ===
cd src-tauri
cargo build --bin myshell-mcp --profile %PROFILE%
if errorlevel 1 exit /b %errorlevel%

echo === Building myshell-cli (%PROFILE%) ===
cargo build --bin myshell-cli --profile %PROFILE%
if errorlevel 1 exit /b %errorlevel%

echo === Building Tauri app (%PROFILE%) ===
cd ..
if "%PROFILE%"=="debug" (
  npm run tauri:dev
) else (
  npm run tauri:build
)
if errorlevel 1 exit /b %errorlevel%

echo === Done ===
if "%PROFILE%"=="release" (
  echo Installer: src-tauri\target\release\bundle\nsis\MyShell_*_x64-setup.exe
)
