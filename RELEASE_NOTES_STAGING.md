# 发布暂存 / Release Notes Staging

打包（release）前的工作积攒区。每完成一项具体改动（功能 / 修复 / 优化 / 安全），
追加一行到下方「待发布条目」；`打包` 时**以本文件为主**生成 CHANGELOG 新版本节、
判定版本号，发布后**清空待发布条目**并把 `baseline` 更新为刚发布的版本号。

- `baseline: v2.11.1` —— 上一个已发布版本（`打包` 时作为 `git diff` 完整性校验的起点）

> 写入时机（高优先级，绑 doc-after-feature）：每完成一项改动，写 `progress.md` 阶段
> 日志的**同时**，在本文件「待发布条目」追加一行。一行格式：
> `- <类型emoji> <一句话描述>`（✨新增 / 🛠️优化 / 🐛修复 / 🔒安全）。纯讨论 / 问答不记。

---

## 待发布条目（打包后清空）
- 🛠️ MCP exec 互斥锁改 Map+timestamp，超过 60s 自动 force-release，根治"上一条命令还在跑"误锁；ssh_exec 输出上限改用 `MCP_SSH_MAX_OUTPUT_BYTES` 环境变量配置（默认 4 MiB，范围 64 KiB~1 GiB）；新增 `ssh_cancel` 工具支持中断后台任务，已积累的 stdout/stderr 截至取消点保留

- 🛠️ Umami 统计事件名前缀加 `myshell_`，与 zcode-assistant 区分（避免数据混入对方 dashboard）
<!-- 上一个版本 v2.11.1 已发布。新的改动完成后在此追加一行：`- <emoji> <一句话描述>` -->
