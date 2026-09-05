# 发布暂存 / Release Notes Staging

打包（release）前的工作积攒区。每完成一项具体改动（功能 / 修复 / 优化 / 安全），
追加一行到下方「待发布条目」；`打包` 时**以本文件为主**生成 CHANGELOG 新版本节、
判定版本号，发布后**清空待发布条目**并把 `baseline` 更新为刚发布的版本号。

- `baseline: v2.13.0` —— 上一个已发布版本（`打包` 时作为 `git diff` 完整性校验的起点）

> 写入时机（高优先级，绑 doc-after-feature）：每完成一项改动，写 `progress.md` 阶段
> 日志的**同时**，在本文件「待发布条目」追加一行。一行格式：
> `- <类型emoji> <一句话描述>`（✨新增 / 🛠️优化 / 🐛修复 / 🔒安全）。纯讨论 / 问答不记。

---

## 待发布条目（打包后清空）

<!-- 上一个版本 v2.13.0 已发布。新的改动完成后在此追加一行：`- <emoji> <一句话描述>` -->
- 🐛 修复中文 locale 服务器（如 Ubuntu zh_CN）上系统监控内存显示 0 B/0 B——内存改为直读 /proc/meminfo（free -b 兜底），不再受 free 本地化标签影响
- 🛠️ 连接后展示登录横幅（Last login 时间/IP 与 MOTD），对齐 Xshell 首屏体验——修复前端监听就绪前的 banner 字节丢失
- 🔒 修复 MCP 命令确认白名单绕过：改为逐命令段判定（引号/转义感知），白名单不再能豁免黑名单命中，GUI 与 MCP 共用同一 Rust 判定
- 🔒 修复 Windows 下 SFTP 递归下载本地路径逃逸：远端文件名严格按单一路径组件校验（反斜杠/盘符/保留设备名/尾点尾空格等），构建目标前检查符号链接与 reparse point
- 🔒 升级 suppaftp 8.0.5 → 10.0.2（RUSTSEC-2026-0271 FTP CRLF 命令注入），并在后端对所有 FTP 参数统一拒绝 CR/LF/NUL
- 🔒 密码查看改为后端原子「验证主密码 + 解密」命令（reveal_connection_password），WebView 无法再绕过前端二次验证
- 🔒 GUI localhost IPC 加固：单请求上限 1 MiB、并发连接上限 32、逐连接线程处理，长 exec_in_tab 不再阻塞后续请求
- 🔒 修复「一键配置 MCP」对损坏配置的覆盖风险：无效 JSON 直接报错不写、非对象节点拒绝覆盖、临时文件 + rename 原子写、原文件自动备份 .bak
- 🔒 命令历史与快捷命令改为 AES-GCM 加密存储（保险库解锁后自动迁移旧明文），并提供后端强制的「不记录命令历史」开关
- ✨ 补齐 FTP/FTPS 功能闭环：支持 explicit/implicit FTPS（rustls 标准信任链）、FTP 上传/下载（递归展开、进度/取消与 SFTP 同一套 UI），并修复 FTP 会话归还导致第二次操作必失败的 bug
- ✨ MCP 高危命令确认弹窗新增「危害说明」：逐条列出命中的黑名单规则及具体危害（约 70 条内置描述，自定义规则回退显示正则），并自动置顶、聚焦、闪烁任务栏提醒人工审核（headless MessageBoxW 详情同步附带）
- 🔒 封堵黑名单 wrapper 绕过：nohup/xargs/env/timeout/nice/setsid/busybox 等 13 个中性前缀在判定前剥离，`find | xargs rm`、`nohup rm -rf`、`ps|awk|xargs kill` 不再免确认
- 🔒 补齐高危命令默认黑名单：npx/bun/uv、nc/socat（反弹 shell）、modprobe/insmod（内核模块）、chattr、wipefs/sfdisk、ip/ifconfig/nmcli（自断连接）、doas/pkexec/runuser（提权）、dpkg/rpm、systemd-run/update-rc.d（持久化）、supervisorctl、fuser -k、chpasswd、setenforce、sysctl、podman、terraform/ansible/helm、aws/gcloud/az 等云 CLI
- 🛠️ 降低确认噪音：mkdir/touch 移出默认黑名单（只创建不覆盖不删除）；git/docker/kubectl 改为写子命令才确认（status/log/diff、ps/logs、get 免确认）；curl 收窄为带写文件/发数据参数才确认（纯 GET 免确认），wget 保持整体确认
- 🛠️ MCP 文件传输顺序反转：zmodem（rz/sz）优先，技术性失败（如远端无 lrzsz、超时）后再回退 SFTP——工具描述与系统提示同步翻转，用户拒绝确认弹窗仍为硬停止不回退
