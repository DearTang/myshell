@echo off
chcp 65001 >/dev/null
setlocal EnableDelayedExpansion

echo ========================================
echo   MyShell Production Build
echo ========================================
echo.

:: Add Rust to PATH (default installation location)
set "CARGO_PATH=%USERPROFILE%\.cargo\bin"
if exist "%CARGO_PATH%\cargo.exe" (
    set "PATH=%CARGO_PATH%;%PATH%"
    echo [INFO] Added Rust to PATH: %CARGO_PATH%
)

:: Check Rust
where cargo >/dev/null 2>&1
if %ERRORLEVEL% neq 0 (
    echo [ERROR] Rust/Cargo not found. Please install Rust first.
    echo Download: https://rustup.rs/
    pause
    exit /b 1
)

:: Check Node.js
where node >/dev/null 2>&1
if %ERRORLEVEL% neq 0 (
    echo [ERROR] Node.js not found. Please install Node.js first.
    pause
    exit /b 1
)

:: Show versions
echo [INFO] Checking environment...
for /f "tokens=*" %%i in ('cargo --version') do echo   Rust: %%i
for /f "tokens=*" %%i in ('node --version') do echo   Node: %%i
echo.

:: Install dependencies
echo [STEP 1/3] Checking npm dependencies...
if not exist "node_modules" (
    echo Installing dependencies...
    call npm install
    if %ERRORLEVEL% neq 0 (
        echo [ERROR] npm install failed
        pause
        exit /b 1
    )
) else (
    echo Dependencies exist, skipping...
)
echo.

:: TypeScript check
echo [STEP 2/3] TypeScript type check...
call npx tsc --noEmit
if %ERRORLEVEL% neq 0 (
    echo [WARNING] TypeScript check failed, but continuing...
) else (
    echo Type check passed
)
echo.

:: Tauri build
echo [STEP 3/3] Starting Tauri production build...
echo This may take several minutes...
echo.

call npm run tauri:build
if %ERRORLEVEL% neq 0 (
    echo.
    echo [ERROR] Build failed
    pause
    exit /b 1
)

echo.
echo ========================================
echo   Build completed!
echo ========================================
echo.
echo Output: src-tauri\target\release\bundle\
echo.

:: List generated files
if exist "src-tauri\target\release\bundle\nsis" (
    echo NSIS installer:
    dir /b "src-tauri\target\release\bundle\nsis\*.exe" 2>/dev/null
)

echo.
pause
