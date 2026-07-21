# 发布暂存 / Release Notes Staging

打包（release）前的工作积攒区。每完成一项具体改动（功能 / 修复 / 优化 / 安全），
追加一行到下方「待发布条目」；`打包` 时**以本文件为主**生成 CHANGELOG 新版本节、
判定版本号，发布后**清空待发布条目**并把 `baseline` 更新为刚发布的版本号。

- `baseline: v1.11.2` —— 上一个已发布版本（`打包` 时作为 `git diff` 完整性校验的起点）

> 写入时机（高优先级，绑 doc-after-feature）：每完成一项改动，写 `progress.md` 阶段
> 日志的**同时**，在本文件「待发布条目」追加一行。一行格式：
> `- <类型emoji> <一句话描述>`（✨新增 / 🛠️优化 / 🐛修复 / 🔒安全）。纯讨论 / 问答不记。

---

## 待发布条目（打包后清空）

<!-- 上一个版本 v1.11.2 已发布。新的改动完成后在此追加一行：`- <emoji> <一句话描述>` -->

- 🛠️ 新增 Linux deb 打包支持：`tauri.conf.json` 把 `deb` 加入 `bundle.targets`，声明 libwebkit2gtk-4.1-0 / libgtk-3-0 / libsecret-1-0 / libayatana-appindicator3-1 运行时依赖（apt 安装时自动拉取）；修复 `setup_file_logging` 缺跨平台守卫导致 Linux 编译失败（Windows CRT 代码改为 `cfg(windows)`，Unix 用 stub）。Ubuntu/Debian 用 `sudo apt install ./MyShell_*_amd64.deb` 安装（apt 自动解决依赖，不要用裸 `dpkg -i`）。
- 🛠️ 应用内更新支持平台分发：`UpdateInfo` 新增 `update_strategy` 字段（Windows=auto 走内置下载安装；Linux/macOS=browser 跳浏览器让用户手动下载）。`check_for_updates` 按平台筛选 asset（Windows 找 `.exe`，Linux 找 `.deb`）。Linux 用户检查到新版本时弹窗显示「打开下载页」按钮而非「更新」按钮——Linux 暂不做自动安装（`install_update` 是 Windows 专属，调 pkexec/apt 风险高，留待后续方案）。
- ✨ CLI + MCP Server 支持：新增 `myshell-cli` 命令行工具和 `myshell-mcp` MCP 协议服务器，让 AI agent（Claude Desktop / Cursor / ZCode 等）能通过命令行或 MCP 协议使用 MyShell 已保存的 SSH/SFTP 连接。支持远程命令执行（exec）、SFTP 文件操作（ls/get/put/mkdir/rm/rename）、连接测试、交互式 SSH。架构重构：提取 `myshell_core` 库，GUI/CLI/MCP 三个二进制共享同一核心逻辑和数据库。
- ✨ MCP 管理设置页：GUI 设置面板新增"MCP支持"页面，支持一键启用/禁用 MCP、vault 密码安全同步到 Windows 凭证管理器（DPAPI 加密）、自动检测已安装 AI 工具（Claude/Opencode/Zcode）并写入 MCP 配置、密码修改时自动同步。
- 🔒 MCP 高危操作人工确认：AI agent 通过 MCP 执行 ssh_exec、sftp_remove、sftp_rename、sftp_upload 时，必须弹出 OS 级确认对话框，用户明确点击"确认"后才执行，无法跳过。
- 🐛 修复 MCP server 连接超时（ZCode 30000ms）：vault 解锁（PBKDF2 600k 次迭代 ≈ 1s）原本在主 async 任务里同步执行，阻塞了 JSON-RPC 循环导致客户端等不到 initialize 响应。改为后台线程解锁 + 主任务立即进入消息循环，并补充 recv/sent 诊断日志到 `%APPDATA%\myshell\logs\mcp.log`。
- 🐛 修复 MCP 协议分帧错误（真根因）：原本用 LSP 风格 `Content-Length: N\r\n\r\n{...}` 分帧，但 MCP 2025-06-18 规范要求 NDJSON（一行一个 JSON）。opencode / ZCode / Cursor 等现代客户端按 NDJSON 解析，看到 `Content-Length:` 头无法识别为 JSON，立即关闭 stdin，导致 server 看到 EOF 后超时。改为 NDJSON 发送 + 兼容两种格式接收（保留对老客户端 Claude Desktop 的兼容）。opencode / ZCode 验证全部 connected。
- 🛠️ 优化 MCP 工具描述：9 个工具的 description 全部重写，每个都包含 WHEN TO USE / WHEN NOT TO USE / OUTPUT / SIDE EFFECTS，让 AI 准确判断何时用 myshell 而不是自己的 shell 工具。新增 server-level \`instructions\` 字段（MCP 2025-06-18 规范），客户端会 prepend 到 AI 的系统提示，告知整个 myshell MCP 的定位、触发关键词、工作流、安全模型。



- ✨ MCP 连接查找支持 host/IP + conn_type 自动消歧：用户说"ssh 到 135.32.64.30"时，AI 可直接传 IP 给 ssh_exec，myshell 自动按 host 反查到对应连接。重名场景（同一 IP 既存 ssh 又存 sftp）自动按工具类型消歧（ssh_exec 选 ssh 那条，sftp_* 选 sftp 那条），仍歧义时返回错误列出候选项让用户明确指定。连接参数现支持三种形式：name / group-path（/production/prod-db）/ host-IP。

- ✨ 终端截图功能：每个终端窗口的 CommandBar 新增📷截图按钮（紧挨"快捷"按钮），一键截取当前终端 viewport（仅终端内容，不含上方的标签栏、下方的工具栏/输入命令栏）。直接读 xterm buffer 数据自绘（不走 canvas/webgl renderer），支持完整颜色（16 色/216 cube/24 灰阶/24-bit RGB）、bold/italic/underline/inverse 属性；背景色沿 DOM 树解析第一个不透明层，避免透明背景被图片查看器显示为黑白棋盘格。文件名带毫秒防覆盖。
- ✨ 附件目录设置：「设置 → MCP 支持」新增附件目录配置，支持选择/更改/打开目录，首次进入若未配置显示警告横幅。截图自动以「截图_<连接名>_<时间戳>.png」命名保存到此目录；保存成功后弹"已保存 + 打开"toast。
- ✨ MCP screenshot_terminal 工具：AI 可触发截图动作。由于 MCP server 是独立进程无法访问 GUI DOM，工具返回详细中文指引（含连接信息、附件目录路径）让 AI 转告用户在 GUI 点击 📷——诚实告知架构限制，不假装能做到实际做不到的事。
