# 测试编译脚本 - PowerShell 版本
# 验证 TypeScript 和 Rust 都能编译通过

# 添加 cargo 到 PATH
$env:PATH += ";$env:USERPROFILE\.cargo\bin"

Write-Host "=== TypeScript 检查 ===" -ForegroundColor Cyan
npx tsc --noEmit
if ($LASTEXITCODE -ne 0) {
    Write-Host "TypeScript 检查失败" -ForegroundColor Red
    exit 1
}

Write-Host "=== Rust 编译 ===" -ForegroundColor Cyan
Set-Location src-tauri
cargo build
if ($LASTEXITCODE -ne 0) {
    Write-Host "Rust 编译失败" -ForegroundColor Red
    exit 1
}
Set-Location ..

Write-Host "=== 全部通过 ===" -ForegroundColor Green
