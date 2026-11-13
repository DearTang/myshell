# 发布暂存 / Release Notes Staging

打包（release）前的工作积攒区。每完成一项具体改动（功能 / 修复 / 优化 / 安全），
追加一行到下方「待发布条目」；`打包` 时**以本文件为主**生成 CHANGELOG 新版本节、
判定版本号，发布后**清空待发布条目**并把 `baseline` 更新为刚发布的版本号。

- `baseline: v2.20.0` —— 上一个已发布版本（`打包` 时作为 `git diff` 完整性校验的起点）

> 写入时机（高优先级，绑 doc-after-feature）：每完成一项改动，写 `progress.md` 阶段
> 日志的**同时**，在本文件「待发布条目」追加一行。一行格式：
> `- <类型emoji> <一句话描述>`（✨新增 / 🛠️优化 / 🐛修复 / 🔒安全）。纯讨论 / 问答不记。

---

## 待发布条目

- [BUG] 修复安装版设置壁纸后不渲染:CSP `connect-src` 漏放行 asset 域,壁纸可达性预检的 HEAD fetch 被无条件拦截,`applyBgImage` 永远判定不可用而退回光晕底;补上 `http(s)://asset.localhost` 后 dev 与安装版行为一致(dev 无 CSP 响应头故此前未暴露)
