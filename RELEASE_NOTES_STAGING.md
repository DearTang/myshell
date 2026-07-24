# 发布暂存 / Release Notes Staging

打包（release）前的工作积攒区。每完成一项具体改动（功能 / 修复 / 优化 / 安全），
追加一行到下方「待发布条目」；`打包` 时**以本文件为主**生成 CHANGELOG 新版本节、
判定版本号，发布后**清空待发布条目**并把 `baseline` 更新为刚发布的版本号。

- `baseline: v2.3.0` —— 上一个已发布版本（`打包` 时作为 `git diff` 完整性校验的起点）

> 写入时机（高优先级，绑 doc-after-feature）：每完成一项改动，写 `progress.md` 阶段
> 日志的**同时**，在本文件「待发布条目」追加一行。一行格式：
> `- <类型emoji> <一句话描述>`（✨新增 / 🛠️优化 / 🐛修复 / 🔒安全）。纯讨论 / 问答不记。

---

## 待发布条目（打包后清空）

<!-- 上一个版本 v2.3.0 已发布。新的改动完成后在此追加一行：`- <emoji> <一句话描述>` -->

- ✨ 新增 `screenshot_terminal` MCP 工具：AI 可请求 GUI 对指定连接的终端 tab 截图（PNG data URL 回传），用于 AI 视觉检查终端输出
- 🔒 `list_connections` 不再泄露 host/port/username：改用纯文本列查询（`get_all_connections_plaintext`），IP/用户名等加密字段仅在 GUI 解锁后可访问，MCP 永不向 AI 暴露
- 🐛 修复 `sz ./*` 批量下载时首个文件完成后卡住、后续文件不下载的问题（offer 竞态丢失）
- 🐛 修复 sz 下载全部文件完成后 UI 卡几十秒才返回终端的问题（未排干写入即关闭句柄导致 lrzsz 超时重传）
- 🐛 去除 sz 传输开始前终端里多出的一行 `rz`（lrzsz auto-start 触发串噪音）
