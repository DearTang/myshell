#!/usr/bin/env bash
# 测试编译脚本 - 验证 TypeScript 和 Rust 都能编译通过
set -e

# 确保 cargo 在 PATH（标准 rustup 安装位置）
export PATH="$HOME/.cargo/bin:$PATH"

echo "=== TypeScript 检查 ==="
npx tsc --noEmit

echo "=== Rust 编译 ==="
cd src-tauri
cargo build

echo "=== 全部通过 ==="
