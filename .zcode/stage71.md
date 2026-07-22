
### 阶段 71：修复 ssh_exec 空输出 + 新增 upload_project/download_project MCP 工具（2026-07-22）

- **问题：** ssh_exec 在 show_in_gui 模式下返回空 stdout（exit_code 正确但 stdout 为空）；终端里能看到 sentinel 回显；无法一键上传/下载整个项目目录。
- **根因：** src/App.tsx 的 runExec 函数存在时序 bug —— onSshOutput 订阅在 sshSend 命令发送之后才建立，导致快速命令的输出在订阅建立前就被消费完毕，outputBuf 里只剩 sentinel 行。PS1 提示符剥离逻辑用 `command.startsWith(lines[0].trim())` 对带前缀的行（`[host]$ cmd`）也失败。
- **改动：**
  1. `src/App.tsx` —— 把 onSshOutput 订阅移到 sshSend 之前；改用 `stdout.indexOf(command)` 定位回显边界；增加 CRLF→LF 规范化；尾部提示符行检测
  2. `src/App.tsx` —— 发送 sentinel 前先执行 `stty -echo` 隐藏终端回显，发完再 `stty echo` 恢复
  3. `src-tauri/src/bin/myshell-mcp.rs` —— 新增 `upload_project` 工具：本地 tar 打包（排除 .venv 等）→ SSH exec 管道直传 → 远程解压
  4. `src-tauri/src/bin/myshell-mcp.rs` —— 新增 `download_project` 工具：远程 sudo tar 打包 → SFTP 下载 → 本地解压到附件目录
  5. `src-tauri/src/bin/myshell-mcp.rs` —— upload_project/download_project 同时支持 ssh/sftp 连接类型（`find_connection(None)`）
- **验证：** `npx tsc --noEmit` 通过；`cargo check --bin myshell-mcp` 通过；upload_project 成功上传 13MB sftpMonitor 到 135.32.56.70:/opt/py；download_project 成功下载到 G:\桌面\myshell附件\

## 五问重启检查（阶段 71）
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 71 complete —— ssh_exec 空输出修复 + upload_project/download_project 工具，端到端验证通过。 |
| 我要去哪里？ | 打包发布 v2.3.0（含 ✨新增 2 个工具 + 🐛 修复 1 个 bug + 🛠️ 优化 3 项）。 |
| 什么可能导致偏离？ | (1) 使用 ZCode 30s MCP 超时的大项目上传需确认；(2) 不同服务器 locale 设置差异可能影响 tar 中文文件名。 |
| 下一步最小可验证动作？ | 打包后跑一次完整安装 + 上传/下载回归测试。 |
| 目标是什么？ | ssh_exec 可靠返回 stdout；AI 可一键部署/备份远程项目。 |
