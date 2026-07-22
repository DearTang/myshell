#!/bin/bash
# 直接通过 stdio 调用 myshell-mcp.exe 的 ssh_exec，绕过 ZCode MCP 客户端
# 用换行分隔的 JSON-RPC（MCP stdio 也接受一行一条的 JSON）

MCP_EXE="E:\Program Files\MyShell\myshell-mcp.exe"

# 构造完整的 MCP 会话：initialize → tools/call(ssh_exec)
{
  printf '%s\n' '{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}},"id":1}'
  printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}'
  printf '%s\n' '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"ssh_exec","arguments":{"connection":"135.32.56.70","command":"whoami"}},"id":2}'
  sleep 8
} | "$MCP_EXE" 2>nul
