@echo off
chcp 65001 >nul
setlocal EnableDelayedExpansion

echo ========================================
echo   MyShell 生产构建脚本
echo ========================================
echo.

:: 检查 Rust 环境
where cargo >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo [错误] 未找到 Rust/Cargo，请先安装 Rust
    echo 下载地址: https://rustup.rs/
    exit /b 1
)

:: 检查 Node.js 环境
where node >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo [错误] 未找到 Node.js，请先安装 Node.js
    exit /b 1
)

:: 显示版本信息
echo [信息] 环境检查...
for /f "tokens=*" %%i in ('cargo --version') do echo   Rust: %%i
for /f "tokens=*" %%i in ('node --version') do echo   Node: %%i
echo.

:: 安装依赖
echo [步骤 1/3] 检查 npm 依赖...
if not exist "node_modules" (
    echo 正在安装依赖...
    call npm install
    if %ERRORLEVEL% neq 0 (
        echo [错误] npm install 失败
        exit /b 1
    )
) else (
    echo 依赖已存在，跳过安装
)
echo.

:: TypeScript 类型检查
echo [步骤 2/3] TypeScript 类型检查...
call npx tsc --noEmit
if %ERRORLEVEL% neq 0 (
    echo [警告] TypeScript 类型检查失败，但继续构建...
) else (
    echo 类型检查通过
)
echo.

:: Tauri 构建
echo [步骤 3/3] 开始 Tauri 生产构建...
echo 这可能需要几分钟时间...
echo.

call npm run tauri:build
if %ERRORLEVEL% neq 0 (
    echo.
    echo [错误] 构建失败
    exit /b 1
)

echo.
echo ========================================
echo   构建完成！
echo ========================================
echo.
echo 输出目录: src-tauri\target\release\bundle\
echo.

:: 列出生成的文件
if exist "src-tauri\target\release\bundle\nsis" (
    echo NSIS 安装包:
    dir /b "src-tauri\target\release\bundle\nsis\*.exe" 2>nul
)

echo.
pause
