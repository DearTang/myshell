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


