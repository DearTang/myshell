#!/usr/bin/env bash
# 生产打包脚本 - 构建发布版本
set -e

echo "=== TypeScript 检查 ==="
npx tsc --noEmit

echo "=== 生产构建 ==="
cd src-tauri
/c/Users/argus/.cargo/bin/cargo tauri build

echo ""
echo "=== 构建完成 ==="
echo "输出目录: src-tauri/target/release/bundle/"
ls -la target/release/bundle/ 2>/dev/null || echo "请检查 bundle 目录"
