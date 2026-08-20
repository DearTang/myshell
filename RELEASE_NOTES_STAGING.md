# 发布暂存 / Release Notes Staging

打包（release）前的工作积攒区。每完成一项具体改动（功能 / 修复 / 优化 / 安全），
追加一行到下方「待发布条目」；`打包` 时**以本文件为主**生成 CHANGELOG 新版本节、
判定版本号，发布后**清空待发布条目**并把 `baseline` 更新为刚发布的版本号。

- `baseline: v2.11.2` —— 上一个已发布版本（`打包` 时作为 `git diff` 完整性校验的起点）

> 写入时机（高优先级，绑 doc-after-feature）：每完成一项改动，写 `progress.md` 阶段
> 日志的**同时**，在本文件「待发布条目」追加一行。一行格式：
> `- <类型emoji> <一句话描述>`（✨新增 / 🛠️优化 / 🐛修复 / 🔒安全）。纯讨论 / 问答不记。

---

## 待发布条目（打包后清空）
- ✨新增 启动时单实例检测：已有实例运行时弹 TaskDialog 选择「覆盖启动」（结束旧实例并继续启动）或「退出」（保持现有实例）；覆盖启动优先走 IPC 优雅退出（清理 SSH/PTY 会话），超时兜底强杀，named mutex 检测无 stale 风险（Windows）
- 🛠️优化 MCP 调用时若 GUI 未运行自动拉起应用（含识别并清理崩溃残留的陈旧 IPC 端口文件），不再直接报「GUI 未运行」
- 🛠️优化 保险库锁定时 MCP 工具立即返回明确的「保险库未解锁」提示并自动把 MyShell 窗口置顶（新增 focus_unlock IPC），取消原先 30 秒静默等待
- 🔒安全 MCP 保险库门禁覆盖全部工具（list_connections / open_in_gui / screenshot_terminal / zmodem_* / ssh_run 纳入，仅任务状态查询豁免）——锁定态下连接列表也不再可读
- 🔒安全 修复 upload_project / download_project 远端路径引号注入（拼入 sudo 命令与内联 Python 可导致 root 任意命令执行），统一 shell_quote 转义 + 环境变量传路径；顺带修复 zmodem 路径丢撇号、下载体积恒报 0、screenshot_terminal 空 user@host

<!-- 上一个版本 v2.11.2 已发布。新的改动完成后在此追加一行：`- <emoji> <一句话描述>` -->
