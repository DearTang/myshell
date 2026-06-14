# 生产打包脚本 - PowerShell 版本
# 构建发布版本（安装包）

# 添加 cargo 到 PATH
$env:PATH += ";C:\Users\argus\.cargo\bin"

Write-Host "=== TypeScript 检查 ===" -ForegroundColor Cyan
npx tsc --noEmit
if ($LASTEXITCODE -ne 0) {
    Write-Host "TypeScript 检查失败" -ForegroundColor Red
    exit 1
}

Write-Host "=== 生产构建 ===" -ForegroundColor Cyan
Set-Location src-tauri
cargo tauri build
if ($LASTEXITCODE -ne 0) {
    Write-Host "构建失败" -ForegroundColor Red
    exit 1
}
Set-Location ..

Write-Host ""
Write-Host "=== 构建完成 ===" -ForegroundColor Green
Write-Host "输出目录: src-tauri\target\release\bundle\" -ForegroundColor Yellow
Get-ChildItem -Path "src-tauri\target\release\bundle" -ErrorAction SilentlyContinue
