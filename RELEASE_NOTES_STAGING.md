# 发布暂存 / Release Notes Staging

打包（release）前的工作积攒区。每完成一项具体改动（功能 / 修复 / 优化 / 安全），
追加一行到下方「待发布条目」；`打包` 时**以本文件为主**生成 CHANGELOG 新版本节、
判定版本号，发布后**清空待发布条目**并把 `baseline` 更新为刚发布的版本号。

- `baseline: v2.10.0` —— 上一个已发布版本（`打包` 时作为 `git diff` 完整性校验的起点）

> 写入时机（高优先级，绑 doc-after-feature）：每完成一项改动，写 `progress.md` 阶段
> 日志的**同时**，在本文件「待发布条目」追加一行。一行格式：
> `- <类型emoji> <一句话描述>`（✨新增 / 🛠️优化 / 🐛修复 / 🔒安全）。纯讨论 / 问答不记。

---

## 待发布条目（打包后清空）

<!-- 上一个版本 v2.10.0 已发布。新的改动完成后在此追加一行：`- <emoji> <一句话描述>` -->
- ✨ MCP 新增 lrzsz(ZMODEM) 文件传输支持：`zmodem_download`（远端 `sz`→本地，复用原生接收器）与 `zmodem_upload`（本地→远端 `rz`，新写 Rust 发送状态机），覆盖 SFTP 子系统不可用的受限 shell/堡垒机/嵌入式设备场景
- 🛠️ 底部状态栏内存占用改用内核 MemAvailable 口径（`(total − available) / total`），与 htop / node_exporter 一致；老 procps（无 available 列）自动回退到 `-/+ buffers/cache:` 修正值，不再虚高
- 🛠️ rz 上传提速：原生 ZMODEM 发送改为专用 pump task + 1 MB 批量写（不再阻塞终端 select! 循环），子包 64→128 KB，上传进度条正常显示
- 🛠️ rz 上传再提速：进度 IPC 事件 100ms 节流（每个 128KB 数据块不再各触发 1 次跨进程 IPC + 前端重渲染，100MB 文件减少 ~8× IPC），移除 sender task 的无效 flush()（russh flush 为 no-op）
- 🐛 修复 MCP `ssh_exec` 高频调用导致 GUI 卡死：GUI 侧 IPC 监听器为单线程串行 accept，`exec_in_tab`/`screenshot_terminal` 在 accept 线程内同步 block_on 等待前端回结果，单条命令阻塞会冻结整个 IPC 通道（后续 open_connection / vault_status / 其他 exec 全部排队）；改为将阻塞等待 detach 到独立 std 线程，accept 线程立即回到循环；同时移除 exec_in_tab 每次调用都 window.show()+set_focus() 抢焦点的高频骚扰（open_connection/screenshot 仍保留抢焦点）
- 🐛 修复 ZMODEM(rz) 上传中/大文件卡死：2MB+ 文件上传进度卡 100% 但远端无文件、任务永不结束。根因是上传发送状态机的 ZEOF 收尾帧经跨进程回路送出时丢失/时序错乱，远端 rz 收不到 ZEOF → 不回 ZFIN → 状态机永久卡在 WaitingZrinit2。已重构为 native pump 架构（ssh.rs reader 线程内联驱动 ZmodemSender，数据直发 SSH channel，不再经 MCP 进程跨回路），并加 fast-finish + 30s 超时双重兜底。实测 2MB 上传 + 5MB 下载均 SHA256 完全一致
