# 发布暂存 / Release Notes Staging

打包（release）前的工作积攒区。每完成一项具体改动（功能 / 修复 / 优化 / 安全），
追加一行到下方「待发布条目」；`打包` 时**以本文件为主**生成 CHANGELOG 新版本节、
判定版本号，发布后**清空待发布条目**并把 `baseline` 更新为刚发布的版本号。

- `baseline: v2.0.0` —— 上一个已发布版本（`打包` 时作为 `git diff` 完整性校验的起点）

> 写入时机（高优先级，绑 doc-after-feature）：每完成一项改动，写 `progress.md` 阶段
> 日志的**同时**，在本文件「待发布条目」追加一行。一行格式：
> `- <类型emoji> <一句话描述>`（✨新增 / 🛠️优化 / 🐛修复 / 🔒安全）。纯讨论 / 问答不记。

---

## 待发布条目（打包后清空）

<!-- 上一个版本 v2.0.0 已发布。新的改动完成后在此追加一行：`- <emoji> <一句话描述>` -->

- ✨ MCP open_in_gui 工具：AI 可驱动 MyShell GUI 自动打开已保存连接的 tab 并聚焦窗口（用户说"在 MyShell 打开 prod-db"即可）。通过 localhost TCP IPC 桥实现（MCP 与 GUI 独立进程，零新依赖、只绑 127.0.0.1）；GUI 未运行时返回明确错误并建议改用 ssh_exec。支持 `tab_type` 参数（auto/terminal/sftp，可对 SSH 连接强制开 SFTP 文件浏览 tab）；默认聚焦已有 tab——同一连接已打开时直接切换过去不重复开。MCP 工具数 10 → 11。
