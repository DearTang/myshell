# 开发启动脚本 - 使用完整 cargo 路径
$env:PATH = "C:\Users\argus\.cargo\bin;" + $env:PATH

Write-Host "=== 启动 Tauri 开发模式 ===" -ForegroundColor Cyan
Write-Host "Cargo 路径: $(Get-Command cargo -ErrorAction SilentlyContinue).Source" -ForegroundColor Gray

# 验证 cargo 可用
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "错误: 找不到 cargo，请检查 C:\Users\argus\.cargo\bin 目录" -ForegroundColor Red
    exit 1
}

npx tauri dev
