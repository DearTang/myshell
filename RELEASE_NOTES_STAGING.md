# 发布暂存 / Release Notes Staging

打包（release）前的工作积攒区。每完成一项具体改动（功能 / 修复 / 优化 / 安全），
追加一行到下方「待发布条目」；`打包` 时**以本文件为主**生成 CHANGELOG 新版本节、
判定版本号，发布后**清空待发布条目**并把 `baseline` 更新为刚发布的版本号。

- `baseline: v2.2.0` —— 上一个已发布版本（`打包` 时作为 `git diff` 完整性校验的起点）

> 写入时机（高优先级，绑 doc-after-feature）：每完成一项改动，写 `progress.md` 阶段
> 日志的**同时**，在本文件「待发布条目」追加一行。一行格式：
> `- <类型emoji> <一句话描述>`（✨新增 / 🛠️优化 / 🐛修复 / 🔒安全）。纯讨论 / 问答不记。

---

## 待发布条目（打包后清空）

<!-- 上一个版本 v2.2.0 已发布。新的改动完成后在此追加一行：`- <emoji> <一句话描述>` -->
- 🐛 修复 ssh_exec 空输出 bug（sentinel 订阅时序错误 + PS1 提示符剥离失败）
- 🛠️ 优化 sentinel 机制（stty -echo 隐藏终端回显）
- ✨ 新增 upload_project MCP 工具（一键上传项目目录到远程服务器）
- ✨ 新增 download_project MCP 工具（一键下载远程项目目录到本地）
- 🛠️ 优化 upload_project 传输方式（SSH 管道直传替代 SFTP 分块，速度提升 10x+）
- 🛠️ 修复 download_project 权限问题（sudo tar + UTF-8 locale 支持中文文件名）
- 🐛 修复 MCP ssh_exec 连续执行不停开新标签页（focus_existing 应为 true 复用已有 tab）
- 🐛 修复 MCP ssh_exec 返回 stdout 残留辅助命令回显（去掉 stty 三行，改为单行 sentinel + 截断式清洗）
- 🔒 安全加固：删除 MCP vault 密码 keyring 明文存储，强制所有服务器访问经 GUI 解锁（ssh_exec 走 GUI tab，SFTP 经 GUI IPC 解密密码）
- ✨ 新增终端渲染层 sentinel 过滤（__MCP_DONE_ 行在写入 xterm 前丢弃，终端无噪音）
