#!/usr/bin/env bash
# 测试编译脚本 - 验证 TypeScript 和 Rust 都能编译通过
set -e

echo "=== TypeScript 检查 ==="
npx tsc --noEmit

echo "=== Rust 编译 ==="
cd src-tauri
/c/Users/argus/.cargo/bin/cargo build

echo "=== 全部通过 ==="
