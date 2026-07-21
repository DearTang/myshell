#!/bin/bash
# Build script for MyShell — ensures MCP binary is compiled before Tauri packaging.
#
# Usage: ./scripts/build-release.sh
# Usage: ./scripts/build-release.sh --debug

set -e

cd "$(dirname "$0")/.."

PROFILE="release"
if [ "$1" == "--debug" ]; then
  PROFILE="debug"
fi

echo "=== Building myshell-mcp ($PROFILE) ==="
cargo build --bin myshell-mcp --profile "$PROFILE"

echo "=== Building myshell-cli ($PROFILE) ==="
cargo build --bin myshell-cli --profile "$PROFILE"

echo "=== Building Tauri app ($PROFILE) ==="
if [ "$PROFILE" == "debug" ]; then
  cargo tauri dev
else
  cargo tauri build
fi

echo "=== Done ==="
if [ "$PROFILE" == "release" ]; then
  echo "Installer: src-tauri/target/release/bundle/nsis/MyShell_*_x64-setup.exe"
fi
