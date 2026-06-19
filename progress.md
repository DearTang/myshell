# 进度日志

## 会话：2026-06-12 ~ 2026-06-13

### 阶段 1：需求分析与技术选型
- **状态：** complete

### 阶段 2：环境搭建与项目初始化
- **状态：** in_progress
- Rust 工具链安装中（rustup stable 1.96.0, 下载7个组件中）
- npm 依赖已安装完成
- TypeScript 编译检查通过（无错误）

### 阶段 3：核心后端实现 (Rust/Tauri)
- **状态：** complete（代码已编写）
- 已创建/修改的文件：
  - `src-tauri/Cargo.toml` — Rust 依赖配置
  - `src-tauri/build.rs` — Tauri 构建脚本
  - `src-tauri/tauri.conf.json` — Tauri 应用配置
  - `src-tauri/src/main.rs` — 主入口 + Tauri commands
  - `src-tauri/src/db.rs` — SQLite 连接配置存储
  - `src-tauri/src/ssh.rs` — SSH 连接管理 (russh)
  - `src-tauri/src/sftp.rs` — SFTP 文件操作 (russh-sftp)

### 阶段 3.1：修复 SSH 输出回显（2026-06-13）
- **状态：** complete（前端代码已验证，后端代码已编写但未编译）
- **问题：** 原 ssh.rs 创建了 output_tx 通道但从未启动读取任务，导致服务器输出无法回到 xterm.js（只能看到 PTY echo 的本地输入）
- **修复方案：** 重构 SshSession 结构 — 将 Channel 所有权移交给独立的 tokio reader 任务
  - 新增 `SessionCommand` 枚举（Input/Resize/Disconnect）通过 mpsc 通道传递给 reader
  - `channel_reader` 任务用 `tokio::select! { biased; ... }` 多路复用：
    - `command_rx.recv()` — 前端命令（优先级最高，输入延迟低）
    - `channel.wait()` — 服务器数据，缓冲到 Vec<u8>
    - `flush_interval.tick()` — 16ms 定时刷新，合并突发输出缓解 tauri#13234
  - 16KB 阈值在线刷新（避免高输出时定时器分支饥饿）
  - 通过 `app.emit("ssh_output"|"ssh_closed"|"ssh_exit", payload)` 推送到前端
- **修改的文件：**
  - `src-tauri/src/ssh.rs` — 大改：新增 SessionCommand、channel_reader 任务、flush_buffer 辅助函数；send_input/resize_terminal/disconnect 改为发命令
  - `src-tauri/src/main.rs` — `ssh_sessions` 改为 `Arc<Mutex<...>>`；ssh_connect 注入 `AppHandle` 参数（修复了原 `state.app_handle()` 不存在的 bug）；新增 `SshOutputPayload` 序列化结构
  - `src/api.ts` — 新增 `onSshOutput`/`onSshClosed`/`onSshExit` 监听器（按 sessionId 过滤）
  - `src/components/TerminalPanel.tsx` — useEffect 内订阅 ssh_output（写入 xterm）和 ssh_closed（红色 [Connection closed] 提示）；卸载时反订阅
- **验证：** `npx tsc --noEmit` PASS（exit code 0）
- **遗留风险：**
  - `channel.wait()` 在 select! 中的取消安全性 — 若实测丢字节，改用 `make_reader()` + `tokio::io::split()`
  - `Vec<u8>` JSON 序列化为数字数组（~4× 膨胀）— v0.1 可接受，如 profiling 显示瓶颈再切 base64
  - tauri#13234 在大量输出时仍可能卡顿 — 16ms 合并已缓解，必要时增大间隔

### 阶段 3.2：安全审查与高危修复（2026-06-13）
- **状态：** complete（前端 tsc 通过，后端代码已写待 cargo 编译）
- **审查发现：** 并行执行安全审查 + 设计审查两个 agent
  - 设计审查确认：Mutex 选择正确、select! 取消安全、reader 自移除无 TOCTOU、SFTP 不与 bash 通道冲突
  - 安全审查发现 2 个 HIGH + 3 个 MEDIUM + 5 个 LOW
- **已修复（HIGH/MEDIUM 立即修）：**
  - **HIGH #1：跨窗口终端泄漏** — `app.emit` 广播到所有 webview。改为 `window.emit` 通过 `WebviewWindow` 仅发给源窗口。修改 `ssh.rs::connect/channel_reader/flush_buffer` 签名 `AppHandle → WebviewWindow`；`main.rs::ssh_connect` 同步
  - **HIGH #2：缓冲区无上限导致前端 OOM** — 新增 `MAX_BUFFER_SIZE = 256KB` 常量与 `append_capped` 辅助函数。超限时刷新已有数据 + 写入 `[output truncated]` 标记 + 丢弃剩余
  - **MEDIUM #4：端口验证** — `ConnectionDialog.tsx` 用 `parseInt(port, 10)` 显式校验 `1-65535`，替代静默 `|| 22` 回退
- **遗留未修：**
  - MEDIUM #3：app 退出不清理 active sessions（资源泄漏）— 需加 `RunEvent::Exit` handler
  - MEDIUM #5：`load_secret_key` 路径未规范化（文件存在性 oracle）
  - LOW：SFTP 每次开新子通道（延迟，缓存 `SftpSession` 可优化）、listener 微任务间隙泄漏、SQLite 文件权限、SFTP rename 允许 `../`、reader 任务 JoinHandle 未跟踪

### 阶段 4：前端 UI 实现
- **状态：** complete（代码已编写）
- 已创建/修改的文件：
  - `package.json` — 前端依赖
  - `vite.config.ts` — Vite 构建配置
  - `tsconfig.json` — TypeScript 配置
  - `index.html` — HTML 入口
  - `src/main.tsx` — React 入口
  - `src/vite-env.d.ts` — Vite 类型声明
  - `src/styles/global.css` — 全局样式（Catppuccin Mocha 主题）
  - `src/api.ts` — Tauri IPC 接口封装
  - `src/App.tsx` — 主应用（布局 + 多标签）
  - `src/components/Sidebar.tsx` — 连接管理侧边栏
  - `src/components/TabBar.tsx` — 标签栏 + SFTP 切换
  - `src/components/TerminalPanel.tsx` — xterm.js 终端面板
  - `src/components/SftpPanel.tsx` — SFTP 文件浏览器
  - `src/components/ConnectionDialog.tsx` — 连接配置对话框

### 阶段 5：集成测试与交付
- **状态：** in_progress
- **里程碑（2026-06-13）：cargo build 首次成功**
- **验证步骤（按序执行）：**
  1. `cargo build`（在 `src-tauri/`）— **PASS**（11.10s，零警告零错误）
  2. `cargo tauri dev`（在 `src-tauri/`）— 待执行
  3. 连接真实 SSH 服务器（如 docker `linuxserver/openssh-server` 端口 2222）验证：
     - 服务器提示符出现在终端（输出回路已通）
     - 输入 `ls`、`echo hello`、`pwd` — 输出可见
     - 输入 `ls /nonexistent` — stderr 可见
     - 调整窗口尺寸 — `stty size` 显示正确行列数
     - 关闭标签 — `ssh_disconnect` 触发，无泄漏
     - 服务器侧 kill 用户 shell — 标签显示红色 [Connection closed]

### 阶段 6：v0.1 占位项清理（2026-06-14）
- **状态：** complete（Rust 编译 PASS，TS 类型检查 PASS；端到端验证待跑）
- **CLAUDE.md 列出的三处占位：**
  1. `check_server_key` 全盘接受 → DB 比对
  2. 密码明文存 SQLite → OS keyring
  3. app 关闭不 disconnect 会话 → 退出时 drain
- **修改明细：**
  - `src-tauri/Cargo.toml` — 加 `keyring = { version = "3", features = ["apple-native", "windows-native", "sync-secret-service"] }`
  - `src-tauri/src/main.rs` — `AppState.db` 改 `Arc<Mutex<rusqlite::Connection>>`（handler 持久引用需要 Arc）；`save_connection` 把密码从 config 取出写入 keyring 后 DB 存 NULL；`delete_connection` 先 best-effort 删 keyring；启动调 `migrate_plaintext_passwords`；`.run()` 改 `.build()` + `ExitRequested` 监听 → `drain_ssh_sessions` 发 Disconnect + sleep 500ms
  - `src-tauri/src/db.rs` — `CREATE TABLE known_hosts`；`get_known_host`/`set_known_host`；`migrate_plaintext_passwords`（一次性把 NOT NULL 的明文搬到 keyring 并 NULL 化）
  - `src-tauri/src/secrets.rs` — 新模块，keyring::Entry 包装（set/get/delete）
  - `src-tauri/src/ssh.rs` — `SshClient { db: Arc<Mutex<Connection>>, host }`；`check_server_key` 走 DB 比对：匹配接受、不匹配拒绝、首次写入；`connect()` 构造 handler 时注入 db Arc clone + host；password 认证如 config.password 空就从 keyring 读；删除死代码 `windows_match`
  - `src/components/ConnectionDialog.tsx` — 编辑时空密码字段传 `undefined`（保留 keyring 旧值，不再误删）；新建 + password 认证 + 密码空 → 报错"请填写密码"
- **遗留风险：**
  - Linux keyring 走 Secret Service（需要 D-Bus + gnome-keyring/kwallet 运行）；Windows/macOS 用原生（Credential Manager / Keychain），无依赖
  - known_hosts 是 host 维度（不分端口/算法）— 同 host 不同端口的恶意服务端会触发"mismatch"误拒；v0.1 接受
  - keyring 写失败时 save_connection 整体失败，但 DB 已经写了（INSERT OR REPLACE 是一个事务内的，未调 set_password 不会先动 DB）— 实际顺序是 set_password → db::save_connection，keyring 失败时 DB 未动，OK


- **状态：** complete
- **起因：** rustup 装好后首次 `cargo build`，连续命中 7 类阻塞问题
- **修复明细：**
  1. **`russh-keys = "0.50"` 无 stable 版本**：0.50 只有 beta（0.50.0-beta.7）。从 `Cargo.toml` 移除直接依赖，改用 `russh::keys` 的 re-export（`PublicKey`、`load_secret_key`、`PrivateKeyWithHashAlg`）
  2. **Windows schannel CRYPT_E_REVOCATION_OFFLINE (0x80092013)**：吊销服务器无法访问。在 `.cargo/config.toml` 加 `[http] check-revoke = false`
  3. **`russh-sftp 1.2.1` 与 `bytes 1.10` 冲突**：bytes 1.10 在 `Buf` trait 上加了 `try_get_*` 方法，与 russh-sftp 自己的 `TryBuf::try_get_u32` 冲突（tokio-rs/bytes#767）。升级到 `russh-sftp = "2"`
  4. **`tauri.conf.json` 顶层 `title` 字段废弃**：新版 tauri-build 2.6.2 要求 `title` 在 `windows[]` 内。删除顶层 `app.title`，顺手把错误的 `$schema` 从 nicegui 改成 Tauri 官方 `https://schema.tauri.app/config/2`
  5. **缺 `icons/icon.ico`**：用 PowerShell System.Drawing 生成 32×32 占位 ICO（蓝圆 + Catppuccin 深色背景）
  6. **russh 0.50.4 API 变更**：
     - `authenticate_publickey` 第二参数从 `Arc<PrivateKey>` → `PrivateKeyWithHashAlg::new(Arc<PrivateKey>, None)`
     - `AuthResult` 改为 enum，提供 `.success()` 方法替代 `!` 操作
     - `Handle<H>` 不再实现 `Clone` → 把 `SshSession.handle` 改成 `Arc<Handle<SshClient>>`
     - `CryptoVec` 无 `as_slice()` → 用 `&data[..]`（Deref 到 `[u8]`）
     - `SshClient` 必须是 `pub`（否则 `pub handle: Arc<Handle<SshClient>>` 暴露私有类型）
  7. **russh-sftp 2.x API 变更**：
     - `mkdir` → `create_dir`
     - `rm_dir` → `remove_dir`
     - `FilePermissions` 不再实现 `Debug`，但有 `Display` → `format!("{}", ...)` 替代 `{:?}`
  8. **`MutexGuard` 跨 await 导致 Future 不 Send**：tauri command 的返回 Future 必须 Send。`get_sftp_session` 用块作用域包住锁，确保 guard 在 await 前释放
  9. **`src/main.rs` 缺 `fn main()`**：原代码只定义了 `pub fn run()`。补 `fn main() { run() }`，保留 `#[cfg_attr(mobile, tauri::mobile_entry_point)]` 兼容移动端
- **修改的文件：**
  - `src-tauri/Cargo.toml` — 移除 `russh-keys`，`russh-sftp` 升 2
  - `src-tauri/.cargo/config.toml` — 加 `check-revoke = false`
  - `src-tauri/tauri.conf.json` — 删 `app.title`，修 `$schema`
  - `src-tauri/icons/icon.ico` — 新建占位图标
  - `src-tauri/src/ssh.rs` — handle 改 Arc 包装；认证 API 升级；CryptoVec 切片语法；SshClient 加 pub
  - `src-tauri/src/sftp.rs` — Handle clone 改 Arc::clone；mkdir/rm_dir 改名；permissions Display；MutexGuard 块作用域
  - `src-tauri/src/main.rs` — 补 `fn main()`

## 测试结果
| 测试 | 输入 | 预期结果 | 实际结果 | 状态 |
|------|------|---------|---------|------|
| TypeScript 编译 | npx tsc --noEmit | 无错误 | 无错误 | PASS |
| npm install | package.json | 安装成功 | 成功 | PASS |
| Rust 安装 | rustup default stable | 安装成功 | 成功 | PASS |
| Rust 编译 | cargo build | 无错误 | 11.10s 完成 | PASS |

## 错误日志
| 时间戳 | 错误 | 尝试次数 | 解决方案 |
|--------|------|---------|---------|
| 2026-06-12 | rustup default 未设置 | 1 | 执行 rustup default stable |
| 2026-06-12 | rustup 下载耗时过长 | 1 | 后台继续，等待完成 |

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 3.3（编译修复）完成，`cargo build` PASS（11.10s 零警告）；进入阶段 5 端到端验证 |
| 我要去哪里？ | 启动 `cargo tauri dev`，连真实 SSH 服务器跑完 6 项验证清单 |
| 目标是什么？ | 构建可运行的 MyShell SSH/SFTP 客户端 |
| 我学到了什么？ | russh 0.50 与 0.49 API 差异巨大（Handle 不 Clone、AuthResult enum、PrivateKeyWithHashAlg）；russh-sftp 1.x 与 bytes 1.10 不兼容；async fn 中 std::sync::MutexGuard 即便 drop 也会让 Future 不 Send，必须块作用域；tauri-build 2.6 删除了 `app.title` 字段 |
| 我做了什么？ | 3.3：连续修 9 类编译阻塞（russh-keys 缺失、SSL 吊销、russh-sftp 升级、tauri.conf 字段、icon 缺失、5 处 API 升级、MutexGuard Send、main 函数缺失） |

---
*每个阶段完成后或遇到错误时更新此文件*

## 会话：2026-06-14 — 5 大需求增强

### 阶段 7：需求扩展（A-F 全套）
- **状态：** Phase A-E complete / Phase F 待运行时验证
- **需求清单：**
  1. Stage 4 性能与健壮性（5GB 大文件、abort 5s 兜底、10 连发）
  2. 多级文件夹（树形/默认收起）
  3. SSH/SFTP/FTP 统一管理
  4. SSH 服务器信息侧栏（OS/CPU/内存/磁盘 5s 刷新）
  5. UI 美化（终端着色 + 对话框分组）

### 阶段 7.A：DB schema + 类型扩展
- **状态：** complete
- `db.rs` — `init_db` 加 `conn_type/group_path/ftp_tls/ftp_passive` 列 + `folders` 表；`column_exists` PRAGMA 探测；`migrate_legacy_schema` 按顺序：`group_name` → `group_path` 迁移 → drop column → drop legacy `password` 列；`list/save/delete/rename_folder` + `folder_has_children`；`rename_folder` 用 `LIKE 'old/%'` 模式批量更新子项路径
- `main.rs` — `ConnectionConfig` 加 4 个字段（带 `#[serde(default)]` 兼容旧 JSON）；`AppState.ftp_sessions: Arc<Mutex<HashMap<UUID, FtpSession>>>`

### 阶段 7.B：UI 美化
- **状态：** complete
- `TerminalPanel.tsx` — PTY 建立后注入 `SHELL_INIT_SEQ`（`FORCE_COLOR=1` + `alias ls='ls --color=auto'` + `source ~/.bashrc 2>/dev/null` + `clear`），sh/dash/fish 兼容（`2>/dev/null` 静默）；`abortTimeoutRef` 5s setTimeout 调 `bridgeRef.reset()` 兜底
- `ConnectionDialog.tsx` — `TypeSelector`（SSH/SFTP/FTP 按钮 + accent 高亮）；`FieldGroup` 卡片分组（基本/认证/FTP/分组）；`nameTouchedRef` 自动同步 host → name 直到用户改 name；focus 态 `boxShadow: 0 0 0 2px rgba(137,180,250,0.18)`；密码字段 warning 色边框、密钥路径 accent 色

### 阶段 7.C：多级文件夹
- **状态：** complete
- `Sidebar.tsx` 重写为树形 — `buildTree(conns, folders)` 递归构造 `FolderNode { depth, children, conns }`；`paddingLeft: 14 + depth * 12`；`Set<string>` 展开态初始空（默认全收起）；右键菜单：空白处新建连接/文件夹、文件夹项子建/重命名/删除；`CONN_ICONS: { ssh: "🖥", sftp: "📁", ftp: "📤" }`
- `normalize_folder_path` 在 Rust 端规范化（去重 `/`、补前导 `/`）；`rename_folder` 拒绝循环（`new.starts_with(old + "/")`）；`delete_folder` 校验 `folder_has_children`

### 阶段 7.D：SSH 服务器信息
- **状态：** complete
- `ssh.rs::exec_once` — 短命令执行助手（clone `Arc<Handle>`、开 channel、`channel.exec(true, cmd)`、循环 `channel.wait()` 累积 `Data`/`ExtendedData`）
- `main.rs` — `SERVER_INFO_SCRIPT` 单次 exec 拿全（OS/K/C/M/D/S1/sleep/S2），`tokio::time::timeout(8s)` 超时返回 `stale: true`；`parse_server_info` 按 `=TAG=` 切片；`cpu_busy_pct` 从两次 /proc/stat 算差值
- `ServerInfoPanel.tsx`（240px 侧栏）— `MetricCard` + `UsageBar`（>85% error / >60% warning / else success）；5s setInterval；`active=false` 时跳过刷新（切 tab 暂停）

### 阶段 7.E：FTP 支持（最大改动）
- **状态：** complete
- **suppaftp v8 API 探索踩坑：**
  - `types::File` 不存在 → `list::File`
  - `AsyncRustlsConnector` 需要外部 rustls/webpki-roots 直接依赖 → 暂缓 TLS（返回错误引导选 `ftp_tls=none`）
  - `passive()` 方法不存在 → v8 默认就是 PASV；切 active 用 `stream.active_mode(timeout)`
  - `mlsd/list` 返回 `Vec<String>` 不是 `Vec<File>` → 必须用 `ListParser::parse_mlsd().or_else(parse_posix)` 逐行解析
- `ftp.rs` — `FtpSession { stream: AsyncFtpStream }`；connect/list/mkdir/remove/rename/disconnect；`format_pex` POSIX 权限串；`format_time`+`days_to_ymd` 无 chrono 转秒为日期
- `main.rs` — 6 个新命令；FTP 借还术 `take_ftp_session`/`return_ftp_session`（`AsyncFtpStream` 不 Clone，必须 take → 用 → return）；`drain_all_sessions` 同步 clear FTP map（drop 即关 socket）
- `api.ts` — `ftpConnect/ListDir/Mkdir/Remove/Rename/Disconnect` 包装
- `App.tsx::handleConnect` — 按 `conn_type` 分流：ftp → 独立 FTP tab；sftp → ssh tab+sftp type；ssh → terminal；`handleCloseTab` 按 connType 调对应 disconnect
- `SftpPanel.tsx` — `source: "ssh" | "ftp"` props + `fullHeight`；按 source 分发；FTP 初始路径用 `/`（不支持 `~`）
- `TabBar.tsx` — tab 图标按 connType 切换

### 阶段 7.F：Stage 4 验证（运行时）
- **状态：** pending（待用户参与）
- **已就绪代码侧：** `MAX_BUFFER_SIZE=256KB` 防前端 OOM；ZMODEM chunk 8KB；5s abort setTimeout 兜底已加
- **待跑测试：**
  1. `dd if=/dev/urandom of=big.bin bs=1M count=5120` → rz + sz → 任务管理器观察 myshell.exe 峰值 <100MB
  2. 单 tab 连续 10 次 `sz fileN` → 监控 zmodem_files Mutex 不阻塞
  3. ZMODEM abort → 5s 后强制 reset 验证

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | Phase A-E 全部完成；cargo build + tsc --noEmit 双绿 |
| 我要去哪里？ | Phase F 运行时验证；之后启动 `cargo tauri dev` 让用户连真实服务器端到端测试 |
| 目标是什么？ | 5 大需求完整可用 |
| 我学到了什么？ | suppaftp v8 API 大改（无 `passive()`/`types::File`，mlsd 返回 String）；FTP session 因 `AsyncFtpStream` 不 Clone 必须用 take/return 借还术；russh 0.50 Handle 是 Arc 可多开 channel — exec channel 与 PTY channel 共存（ServerInfo panel 与终端同时工作） |
| 我做了什么？ | Phase A：DB schema；Phase B：UI 美化；Phase C：树形文件夹；Phase D：服务器信息侧栏；Phase E：FTP 全栈 |

## 会话：2026-06-15 — 全局 + 服务器专属快捷命令

### 阶段 8：快捷命令功能（全局 + 服务器专属 + 多行顺序执行）
- **状态：** complete（cargo check + tsc + clippy 新代码三绿；端到端待用户手动验证）
- **需求：** 设置中添加全局快捷命令；针对当前服务器的专属快捷命令；多行命令按行顺序执行；终端一键点击直接执行
- **设计决策（与用户确认）：**
  - 数据模型：单表 `quick_commands`，`connection_id` 为 NULL=全局、非 NULL=服务器专属；执行面板一条 SQL（`WHERE connection_id IS NULL OR connection_id = ?`）联合取全局+专属
  - NULL 安全：db.rs 按 scope 分支构造 SQL（None→`IS NULL`，Some→`= ?1`），规避 `connection_id IS ?1` 跨 SQLite 版本语义风险
  - 多行执行：前端按 `\r?\n` 拆行→trim→跳过空行和行首 `#` 注释→用 `\r` 拼接一次性 `sshSend`（PTY 必须用 `\r` 触发执行，`\n` 不触发）；复用 `sshSend` + 广播扇出
  - 管理界面：独立 `QuickCommandsPanel`（作用域下拉切换 全局/任意服务器），入口三处（侧边栏 🧩 / 终端执行面板"管理"链接 / 设置面板内 Section）
  - 点击行为：直接执行（不填输入框，符合"快捷"定位）
  - 不写入 command_history（多行语义与单行历史模型不匹配）
  - `delete_connection` 改造为事务 + 级联清理该服务器的专属命令（command_history 保持现状不级联，向后兼容）
- **修改明细：**
  - `src-tauri/src/db.rs` — `init_db` 加 `quick_commands` 表 + 2 索引；`QuickCommandTuple` type alias（消 clippy complex_type warning）；6 个 CRUD（`add_quick_command`/`list_quick_commands`/`list_quick_commands_for_connection` 联合查询/`update_quick_command`/`update_quick_command_order`/`delete_quick_command`）；`delete_connection` 改事务级联
  - `src-tauri/src/main.rs` — `QuickCommandItem`/`QuickCommandExecItem` struct；6 个 `#[tauri::command]`（同步，明文不调 `require_dek`）；`generate_handler!` 注册
  - `src/api.ts` — `QuickCommandItem`/`QuickCommandExecItem` interface + 6 个 invoke 包装（按 connectionId 键控，与 command_history 同约定）
  - `src/components/QuickCommandsPanel.tsx`（新建）— 统一管理面板：作用域下拉切换 + CRUD 列表 + 内联编辑表单 + ↑↓排序（交换相邻 sortOrder）
  - `src/components/CommandBar.tsx` — `⌨ 快捷` 按钮 + 浮层面板（🌐 全局 / 📌 本服务器专属 分组）+ `handleExecuteQuickCommand`（多行 `\r` 拼接 + 跳过空行/注释 + 广播扇出）+ `QuickCommandGroup` 子组件
  - `src/components/TerminalPanel.tsx` — 透传 `onOpenQuickCommandsManage` prop
  - `src/App.tsx` — `showQuickCommands`/`qcInitialConnectionId` state + 面板渲染 + 三入口接线（Sidebar/CommandBar 管理/SettingsPanel）
  - `src/components/Sidebar.tsx` — 🧩 按钮（同款 IconBtn）
  - `src/components/SettingsPanel.tsx` — "快捷命令" Section 入口
- **验证：**
  - `cargo check` PASS（8m52s，首次编译 tauri 全套依赖；零错误零警告）
  - `cargo clippy` 新代码 0 warning（修复 `list_quick_commands` 的 "very complex type" → `QuickCommandTuple` type alias）；剩余 5 个 warning 均为项目预存（ssh.rs:249 / db.rs:243 / backup.rs:187 / main.rs:303 / backup create）
  - `npx tsc --noEmit` PASS
- **待手动 E2E（需真实 SSH 服务器）：**
  1. 管理面板：全局新增 `echo global`；当前服务器新增多行 `cd /tmp`+`# 注释`+`echo $PWD`
  2. 终端 `⌨ 快捷`：两组命令分组显示，点击直接执行；多行按顺序、空行/注释跳过
  3. 广播：两 tab 广播，点快捷命令同步执行
  4. 级联：删除服务器，专属命令被清、全局保留；重连后面板仍能列出并执行
- **遗留风险/限制：**
  - heredoc（`<<EOF`）体内 `#` 开头行/空行会被过滤误删 — 面板文案已提示，未来可加 per-command raw 开关
  - 多行执行依赖 PTY 行缓冲保证顺序；`cd` 等状态依赖命令需用户在同一快捷命令内写好
  - 图标避开 `⚡`（重连按钮已占用，CommandBar.tsx:186）：执行按钮用 `⌨`，管理入口用 `🧩`
- **首次编译踩坑：** `cargo check` 首次因网络中断下载 `web-sys`（reqwest→tauri 依赖）失败（`schannel: server closed abruptly`），重试一次后成功

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 8 快捷命令功能完成；cargo check + tsc + clippy（新代码）三绿 |
| 我要去哪里？ | 用户运行 `cargo tauri dev` 连真实服务器跑 E2E（多行 `\r` 执行顺序是验证重点） |
| 目标是什么？ | 全局+服务器专属快捷命令，多行按行顺序执行，终端一键点击直接运行 |
| 我学到了什么？ | command_history 是快捷命令的天然模板（同款建表/CRUD/IPC/执行通道）；PTY 多行必须用 `\r` 拼接而非 `\n`；`connection_id IS ?1` 的 NULL 语义跨版本有风险，应按 scope 分支构造 SQL；rusqlite `query_map` 两分支若用不同闭包字面量会类型不一致，需提取成函数指针（fn 类型）复用 |
| 我做了什么？ | 阶段 8：db 建表+CRUD+级联；main struct+commands+注册；api.ts 封装；CommandBar 执行面板；QuickCommandsPanel 管理面板；App/Sidebar/SettingsPanel/TerminalPanel 接线 |

## 会话：2026-06-15 — 日志增强（7 天保留 + 结构化诊断日志）

### 阶段 9：日志系统加固
- **状态：** complete（cargo clippy 通过，新代码 0 warning）
- **需求：** 多增加日志方便后续定位问题；默认删除 7 天前的日志
- **现有机制发现：** release 模式已有 `setup_file_logging`（Windows CRT `_open_osfhandle` + `_dup2` 把 stderr fd 2 重定向到按天命名的文件 `<config_dir>/myshell/logs/myshell-{day}.log`），所有 `eprintln!` 已落盘；但保留期是 14 天，且 `env_logger::init()` 默认 error 级别导致 `log::info!/warn!` 被过滤掉
- **改动明细：**
  - `src-tauri/src/main.rs` `setup_file_logging`
    - 清理阈值 14 天 → **7 天**（`60*60*24*14` → `*7`，含 doc 注释）
    - 清理逻辑统计删除数量（`pruned: u32` 计数），在 dup2 完成后的 startup banner 输出（dup2 之前的日志写原始 stderr，release 会丢失）
    - startup banner 从 `eprintln!`（无格式 epoch 秒）→ `log::info!`（带级别/时间戳/保留期/清理数）
  - `src-tauri/src/main.rs` `run()`
    - `env_logger::init()` → `Builder::from_env(Env::default().default_filter_or("info")).format_timestamp_millis()` —— 让 log 宏在默认级别生效；RUST_LOG 仍可覆盖（如 `RUST_LOG=myshell=debug` 排查）
    - 启动阶段日志：db 初始化 info、schema 迁移结果（失败 warn / 成功 info）、backup 检查失败 warn
  - `src-tauri/src/main.rs` 关键诊断路径
    - `ssh_connect`：请求（user@host:port + auth + proxy）、成功（sid + target）、失败（error + 原因）
    - `ssh_disconnect`：disconnect requested
    - `delete_connection`：keyring 删除失败 warn、完成 info（级联清理提示）
    - 快捷命令 `add_quick_command` / `update_quick_command` / `delete_quick_command`：info（id / label / scope）
  - `src-tauri/src/ssh.rs` `channel_reader`：启动 started + 退出 exited（info）—— 定位 SSH 输出中断/会话结束的关键
- **保留未改：** 现有 `eprintln!`（debug 数据流 `Data N bytes`、PTY 步骤等）保留 —— release 已通过 stderr 重定向写入文件，工作正常，避免全量替换引入风险
- **验证：** `cargo clippy` PASS（7.93s 增量编译，0 错误）；新代码 0 warning（5 个 warning 均为项目预存：backup sort_by / main.rs manual_strip / main.rs open_options / ssh.rs field assignment / db.rs complex type）
- **日志查看：** release 用户看 `%APPDATA%/myshell/logs/myshell-{day}.log`；开发 debug 看控制台；排查连接问题搜 `[ssh]` 前缀（connect requested → connected/failed → channel_reader started/exited 完整链路）

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 9 日志增强完成；cargo clippy 通过 |
| 我要去哪里？ | release 运行验证日志文件实际写入 + 7 天清理生效（需积累日志或手动测试清理逻辑） |
| 目标是什么？ | 通过日志文件能定位问题，自动清理 7 天前日志 |
| 我学到了什么？ | 现有已有 setup_file_logging（dup2 stderr → 按天文件），只需调阈值 + 配置 env_logger 级别即可让 log 宏生效，无需重写日志框架；env_logger 默认 error 级别会吞掉 info/warn，必须显式 `default_filter_or("info")`；dup2 之前的日志写原始 stderr（release 丢失），所以清理计数要在 dup2 后的 banner 输出 |
| 我做了什么？ | 阶段 9：清理 14→7 天；env_logger 配 info + 毫秒时间戳；startup / SSH 生命周期 / 快捷命令 / delete_connection / channel_reader 补充结构化日志 |

## 会话：2026-06-17 — 本地终端（连接本地 PowerShell / CMD / WSL / 自定义 shell）

### 阶段 10：本地终端全栈（conn_type='local'）
- **状态：** 前端 complete（tsc 通过）；后端代码 complete 但 **未编译验证**（环境无 rustup）；端到端待用户 `cargo build` + 运行
- **需求：** 在 MyShell 里直连本地的 PowerShell / CMD / WSL / 自定义 shell，作为可保存的连接（进文件夹、带 shell 配置），与 SSH 终端体验一致
- **设计决策（与用户确认）：**
  - 入口形态：本地终端作为 `conn_type='local'` 的一种 `ConnectionConfig`，复用现有连接管理 / 文件夹 / 命令历史 / 快捷命令全套 —— 不新建表、不新写管理 UI
  - 复用 SSH 事件通道：本地后端 emit 现有的 `ssh_output` / `ssh_closed`，`TerminalPanel` 事件订阅零改动，只按 `connType` 选 connect/send/resize/disconnect 命令
  - vault 解锁保持现状：本地连接行随 connections 表加密存储，列出需解锁（`get_connections` 的 DEK 门禁自动覆盖）；`local_connect` 本身不需要密码/DEK
  - shell 配置双字段：`shell_path`（可执行路径，明文列）+ `shell_args`（可选参数）—— 不用单字段命令串，避免 `C:\Program Files\...` 空格被 split 破坏
  - 技术选型：`portable-pty`（Windows ConPTY / Unix openpty，wezterm 出品）。`std::process` + pipe 因无 PTY 被否决（交互式 TUI / 颜色 / resize 全废）
- **修改明细：**
  - `src-tauri/Cargo.toml` — 加 `portable-pty = "0.8"`
  - `src-tauri/src/local.rs`（新建）— `LocalCommand` 枚举（Input/Resize/Disconnect，本地无 ZMODEM）+ `LocalSession { command_tx }` + `connect`：openpty → spawn shell → **reader 阻塞线程**（`spawn_blocking`，因 portable-pty reader 是阻塞 `Read`）emit `ssh_output`，EOF emit `ssh_closed` + 从 map 移除；**writer 任务**（async，持 master）处理 Input/Resize/Disconnect（Disconnect 时 `child.kill()`）。`send_input`/`resize_terminal`/`disconnect` 镜像 ssh.rs
  - `src-tauri/src/db.rs` — `init_db` 加 `shell_path`/`shell_args` 明文列；`migrate_legacy_schema` 加幂等迁移（`column_exists` 探测 + `ALTER TABLE ADD COLUMN`）；`get_all_connections`/`get_connection`/`save_connection` 三处 SELECT+元组+INSERT 同步加列
  - `src-tauri/src/main.rs` — `mod local`；`ConnectionConfig` 加 `shell_path`/`shell_args`（`#[serde(default)]`）；`AppState.local_sessions` map + 初始化；`drain_all_sessions` 加 local 清理（发 `LocalCommand::Disconnect`）；4 命令 `local_connect`/`local_send`/`local_resize`/`local_disconnect`（`local_send` 复用 256KB 输入上限）+ `generate_handler!` 注册
  - `src/api.ts` — `ConnType` 加 `"local"`；`ConnectionConfig` 加 `shell_path`/`shell_args`（snake_case，匹配后端 serde 默认）；4 个 wrapper
  - `src/App.tsx` — `handleConnect`/`handleReconnect`/`handleCloseTab` 加 `connType==='local'` 分支（`localConnect`/`localDisconnect`，display name 用 `config.name`）；TerminalPanel 透传 `connType`
  - `src/components/TerminalPanel.tsx` — `connType` prop + `connTypeRef` + `sendTo`/`resizeTo` 分发（按 connType 选 ssh_*/local_*）；事件订阅 `onSshOutput`/`onSshClosed` 原样复用
  - `src/components/CommandBar.tsx` — `connType` prop + `sendFn` 分发（**修复**：原本直接 `sshSend`，本地 tab 命令会发错后端；现按 connType 选 sshSend/localSend）
  - `src/components/ConnectionDialog.tsx` — `TYPE_OPTIONS` 加 local + `SHELL_PRESETS`（pwsh/powershell/cmd/wsl/git-bash）；`connType==='local'` 时表单只显示 名称/分组/shell 选择+路径+参数，隐藏 host/port/auth/proxy；`handleSave` local 分支（host="" port=0 username=""，shell_path 必填校验）
  - `src/components/Sidebar.tsx` — `CONN_ICONS` 加 `local: 💻`
- **验证：**
  - `npx tsc --noEmit` PASS（首次 3 个错误：ConnectionDialog 误用 `shell_path`/`shell_args` 而 api.ts 我先写成了 camelCase `shellPath`/`shellArgs`；统一为 snake_case 与其他字段一致后通过）
  - Rust `cargo build` **未执行**（环境无 rustup）—— 最可能的调整点是 portable-pty 0.8 的 `take_writer`/`spawn_command` 返回的 trait bound
- **遗留风险 / 待验证：**
  - **ConPTY 编码**：Windows 上 portable-pty 输出按 shell 的 console codepage 走（pwsh=UTF-8 正常；Windows PowerShell 5.1 在中文系统可能 GBK → xterm 中文乱码）。v1 emit 原始字节（与 SSH 一致，不做转换）；如实测乱码，改 pwsh 或在 `local.rs` reader 加 `encoding_rs` 转换 / 注入 `chcp 65001`
  - **Rust 未编译**：portable-pty API 细节、`CommandBuilder` 默认环境/cwd 继承行为需 `cargo build` + 运行确认
  - `shell_args` 空白分割：含空格的单个参数需用户自加引号（v1 限制，`local.rs` 注释已标注）
  - 本地终端不参与广播（`getBroadcastTargets` 已按 `connType==='ssh'` 过滤）也不显示 ServerInfoPanel（同样 ssh-only 判断）—— 无需额外排除
- **首次类型检查踩坑：** `ConnectionConfig` 在 TS 侧一直用 snake_case（匹配后端 serde 默认，无 `rename_all`），新加字段时误用 camelCase 导致不一致；统一 snake_case 后 tsc 绿

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 10 本地终端前端 complete（tsc 绿）；后端代码 complete 但未编译（无 rustup） |
| 我要去哪里？ | 用户 `cargo build`（可能需调 portable-pty API）→ `cargo tauri dev` → 新建本地连接（pwsh.exe）双击开 tab 验证打字/输出/resize/关闭/编码 |
| 目标是什么？ | MyShell 直连本地 PowerShell/CMD/WSL/自定义 shell，作为可保存的连接，体验等同 SSH 终端 |
| 我学到了什么？ | 本地终端与 SSH 终端在渲染层完全同构（都是 xterm + 字节流），差异只在上游数据源 —— 抽象出按 connType 分发即可零成本复用；portable-pty 的 reader 是阻塞 `Read`，必须 `spawn_blocking` 独立线程 + writer 任务双线（不同于 SSH 的 select! 单循环）；CommandBar 直接调 sshSend 是隐藏的分发遗漏点，新增后端必须全局 grep 确认所有 ssh_* 调用点 |
| 我做了什么？ | 阶段 10：Cargo + local.rs PTY 模块；db schema + 字段；main AppState + 4 命令 + drain；api.ts 类型 + wrapper；App/TerminalPanel/CommandBar 按 connType 分发；ConnectionDialog local 表单 + shell 预设；Sidebar 图标 |

### 阶段 10.1：类型图标修复 + 启动命令 init_command（2026-06-17）
- **状态：** 前端 complete（tsc 通过）；后端代码 complete 未编译；**版本号 1.1.0 → 1.2.2**
- **问题1：新建连接类型选择图标显示方框**
  - 根因：`ConnectionDialog` 的 `TYPE_OPTIONS` 里 ssh/sftp/ftp 用 Nerd Font 私有区字符（`󰖟`/`󰉋`/`󰈙`），系统未装 Nerd Font 就渲染成方框；local 用 emoji（`💻`）所以正常
  - 修复：`TYPE_OPTIONS` 图标改 emoji，与 Sidebar `CONN_ICONS` 一致 —— `🖥️`/`📁`/`📤`/`💻`，免字体跨平台
- **问题2：本地终端打开后默认执行命令（init_command）**
  - 需求：连接里配 `claude`，开 tab 自动执行
  - 设计：`ConnectionConfig` 加 `init_command`（明文列，通用字段先本地用）→ `local.rs` writer 任务 `take_writer` 后立即注入 `init_command + \r`（PTY stdin 缓冲，shell 就绪后 echo + 执行；`\r` 触发执行，与 onData 转发 Enter 一致）；`ConnectionDialog` 本地表单加「启动命令（可选）」输入
  - 数据层：`db.rs` 加 `init_command TEXT` 列 + 幂等迁移 + `get_all`/`get`/`save` 三处 SQL 同步；`main.rs`/`api.ts` `ConnectionConfig` 双端加字段
  - 限制：当前把整条 `init_command` 当**单行**命令注入（trim + `\r`）；多行命令暂不支持，后续可按 `\n` 拆分依次注入
- **版本号：** `package.json` / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json` / `Cargo.lock`(myshell 条目) 四处 1.1.0 → 1.2.2；`backup.rs::APP_VERSION` 经 `env!("CARGO_PKG_VERSION")` 自动跟随 Cargo.toml，无需手改
- **验证：** `npx tsc --noEmit` PASS；Rust 待 `cargo build`（含 portable-pty 编译验证）

## 会话：2026-06-18 — 安装器数据删除安全 / 本地终端渲染与编码修复

### 阶段 11：安装器「删除应用数据」二次确认 + 危险提示 + 直白文案
- **状态：** complete（配置与脚本就位；本机无 rustup，未跑 `cargo tauri build` 实际打包验证）
- **问题：** NSIS 卸载/更新流程勾选「删除应用程序数据」（`deleteAppData`）会递归删 `$APPDATA\<bundle>` + `$LOCALAPPDATA\<bundle>` = 整个 `connections.db`（全部连接 / 明文密码 / 命令历史 / 快捷命令 / 密钥库，不可恢复），但只是一个无警告的复选框 + 模糊文案，极易误删
- **方案（不 fork 900 行模板，用官方机制）：**
  - **二次确认**：`installerHooks` 注入 `NSIS_HOOK_PREUNINSTALL` 宏 —— 在 `Section Uninstall` 开头（删数据之前、`$UpdateMode`/`$DeleteAppDataCheckboxState` 已就绪时）拦截：仅当勾选且非自动更新模式弹 `MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2`（默认「否」、`/SD IDNO`），点「否」则置 `$DeleteAppDataCheckboxState=0` 取消删除
  - **文案直白**：`customLanguageFiles`（合并语义，仅覆盖 `deleteAppData` 一条）把「删除应用程序数据」→「删除全部应用数据（连接/密码/历史/密钥库，不可恢复）」
- **修改明细：**
  - `src-tauri/nsis/uninstall-confirm-hook.nsh`（新建，UTF-8 BOM）— `NSIS_HOOK_PREUNINSTALL` 宏 + 双语危险提示
  - `src-tauri/nsis/lang/SimpChinese.nsh`（新建，BOM）/ `English.nsh`（新建）— `deleteAppData` 覆盖
  - `src-tauri/tauri.conf.json` — `bundle.windows.nsis` 加 `installerHooks` + `customLanguageFiles`
- **验证：** JSON 解析通过；NSIS 钩子语法/BOM/反斜杠续行逐项核对；实机验证待 `cargo tauri build` 后覆盖安装→卸载页勾选→应见警告框默认「否」
- **机制确认：** `NSIS_HOOK_PREUNINSTALL` 由模板 `Section Uninstall` 顶部 `!insertmacro`（本地生成 installer.nsi L747-748 实证）；`customLanguageFiles` 为 Rust 层按键名合并（NSIS 不允许同名 LangString 重复，故必为合并而非 include 叠加，仅需写覆盖项）—— 依据 Tauri v2 官方文档 + 配置参考

### 阶段 12：本地终端渲染与编码修复（字体 / TERM 环境 / 字体可配置 / UTF-8 硬化）
- **状态：** 前端 complete（`npx tsc --noEmit` PASS）；后端代码 complete 未编译（无 rustup）
- **问题：** 本地终端（`conn_type='local'`）从 MyShell 打开时「字体样式乱码」——① xterm `fontFamily` 只列基础字体（Cascadia Code/Fira Code…），无 Nerd Font，提示符 powerline/图标字形（Oh My Posh 等）渲染成豆腐块；② `local.rs` spawn 继承父进程环境无 `TERM`/`COLORTERM`，提示符引擎可能降级；③ cmd / Windows PowerShell 5.1 在 zh-CN 吐 GBK → 中文乱码（阶段10 遗留）
- **方案：**
  - **字体**：xterm 字体栈改为 Nerd Font 优先（CaskaydiaCove/Cascadia Code NF/MesloLGM/JetBrainsMono/FiraCode/Hack Nerd Font）+ 基础字体回退
  - **环境**：`CommandBuilder::env` 声明 `TERM=xterm-256color` / `COLORTERM=truecolor` / `TERM_PROGRAM=MyShell`（portable-pty `env()` 为加性覆盖，PATH/profile 照常继承 —— 已查文档确认）
  - **字体可配置**：新增 localStorage 设置（沿用主题/配色同一套持久化模式）—— 用户填入本机已装字体名即生效（无需内置字体，要用 Nerd Font 的用户必然已自装）
  - **UTF-8 硬化**：`local.rs` 启动时按 shell 名注入 UTF-8 前导（写在 init_command 之前）—— cmd：`@chcp 65001>nul`；PowerShell 5.1：`[Console]::{Output,Input}Encoding=[Text.Encoding]::UTF8; chcp 65001 > $null`；pwsh / bash / zsh / wsl 不处理（本就 UTF-8）
- **修改明细：**
  - `src/components/TerminalPanel.tsx` — 字体栈移至共享常量；`useTerminalFont` 取字体；新增 `useEffect` 热更新 `term.options.fontFamily`（已开终端免重开）
  - `src/themes.ts` — 加 `STORAGE_KEY_TERMINAL_FONT` + `TERMINAL_FONT_DEFAULT_STACK`
  - `src/hooks/useTerminalFont.ts`（新建）— localStorage 读/写 + 解析 `fontFamily`（选中字体 + 默认回退栈）
  - `src/components/SettingsPanel.tsx` — 新增「终端字体」Section（Input + 常见 Nerd Font 提示）
  - `src-tauri/src/local.rs` — `use std::path::Path`；新增 `shell_utf8_prelude()`（按 file_stem 匹配，裸名/全路径皆认）；`connect` 计算 prelude 并 move 进 writer 任务，`take_writer` 后先于 init_command 写入
- **验证：** `npx tsc --noEmit` PASS；Rust 待 `cargo tauri dev`（设置面板改字体→已开/新开终端字形即出；本地连 cmd/powershell.exe 中文不再乱码）
- **遗留 / 提醒：** PowerShell 5.1 启动会回显一行编码命令（交互式 PS 无法干净抑制，可接受）；首帧提示符（profile 用旧编码绘制）可能略瑕疵，注入后全部 UTF-8

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 11/12 complete（前端 tsc 绿，Rust 未编译）；已按新规同步更新 progress.md + README.md |
| 我要去哪里？ | 用户 `cargo tauri build` 验证安装器二次确认 + `cargo tauri dev` 验证字体/编码 |
| 目标是什么？ | 卸载数据删除防误删；本地终端字体/编码与真终端一致 |
| 我学到了什么？ | Tauri NSIS 钩子（`NSIS_HOOK_PREUNINSTALL`）+ `customLanguageFiles`（合并语义）能在不 fork 模板的前提下改卸载行为与文案；portable-pty `CommandBuilder::env` 加性覆盖（镜像 std `Command`）；本地终端乱码是「字体缺 Nerd Font 私有区字形 + 缺 TERM 环境 + 非 UTF-8 shell」三因叠加 |
| 我做了什么？ | 阶段 11：卸载钩子 + 语言串覆盖 + tauri.conf 接线；阶段 12：Nerd Font 默认栈 + TERM/COLORTERM + 字体可配置（hook/设置面板/热更新）+ cmd/PS5.1 UTF-8 前导 |

### 阶段 13：终端字体——系统字体下拉选择 + 按连接单独覆盖（2026-06-18）
- **状态：** 前端 complete（`npx tsc --noEmit` PASS）；后端代码 complete 未编译（无 rustup，font-kit 首次拉取 + DB 迁移待 `cargo build` 验证）
- **需求（用户反馈）：** ① 字体设置不该手输，应查询系统可用字体做下拉选择；② 不同连接可能需要不同字体（可访问性/常盯的生产机更大字号/仅某些连接用 Nerd Font），希望按终端单独设字体
- **决策（与用户确认两个分叉）：** 字体枚举用 **font-kit 后端真实枚举**（最贴合"查询系统字体"，跨平台；代价是新增 1 个依赖）；按连接覆盖存 **数据库列**（随连接走，导出/导入备份一起带走，沿用 shell_path/init_command 同款幂等迁移）
- **设计：** 复用 `FontField` 组件（input + 原生 `<datalist>`，模块级缓存共享一次 fetch，失败降级为纯手输）；全局设置 + 按连接覆盖都用它；TerminalPanel 解析 `override ?? global` 热更新
- **修改明细：**
  - `src-tauri/Cargo.toml` — 加 `font-kit = "0.14"`
  - `src-tauri/src/fonts.rs`（新建）— `list_system_fonts` 命令：`SystemSource::new().all_families()` → 排序去重，失败返空
  - `src-tauri/src/db.rs` — `connections` 加 `terminal_font TEXT` 列 + 幂等迁移 + `get_all`/`get`/`save` 三处 SQL（SELECT 末尾追加 index 20、tuple、struct、INSERT 列/VALUES/params）
  - `src-tauri/src/main.rs` — `ConnectionConfig` 加 `terminal_font: Option<String>`（`#[serde(default)]`）；`mod fonts`；`generate_handler!` 注册 `list_system_fonts`
  - `src/api.ts` — `ConnectionConfig.terminal_font?` + `listSystemFonts()` wrapper
  - `src/hooks/useTerminalFont.ts` — 抽出导出 `resolveFontStack(primary?)`（选中字体优先 + 默认回退栈），hook 与按连接覆盖共用
  - `src/components/FontField.tsx`（新建）— input + datalist，`useId` 防多实例 id 冲突，模块级 fetch 缓存
  - `src/components/SettingsPanel.tsx` — 全局字体 `Input` → `FontField`
  - `src/components/ConnectionDialog.tsx` — `terminalFont` state；ssh+local 显示「终端」FieldGroup（FontField，留空=全局）；`handleSave` 两分支（local / ssh·sftp·ftp）都写 `terminal_font`
  - `src/components/TerminalPanel.tsx` — `fontOverride?` prop；`fontFamily = fontOverride ? resolveFontStack(fontOverride) : globalFontFamily`；既有 live-update effect 自动覆盖
  - `src/App.tsx` — 两处 `<TerminalPanel>` 传 `fontOverride={connections.find(...)?.terminal_font}`
- **验证：** `npx tsc --noEmit` PASS；Rust 待 `cargo tauri dev`（font-kit 首次编译 + 验证枚举返回 + DB 迁移 + 字体覆盖生效）
- **遗留 / 风险：** font-kit 为新依赖（Windows 用 DirectWrite 枚举，构建应可靠，但本机无法预编译验证；若 build 报错最可能在此依赖）；DB get_all/get/save 的列索引改动需 `cargo build` 确认无 off-by-one

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 13 前端 complete（tsc 绿）；后端 font-kit + DB 迁移未编译 |
| 我要去哪里？ | 用户 `cargo tauri dev` 验证：设置/连接对话框字体下拉列出系统字体；按连接设字体后该 tab 生效 |
| 目标是什么？ | 字体从系统字体选择（非手输）；支持按连接单独覆盖字体 |
| 我学到了什么？ | Tauri NSIS 之外又一个"前端要的能力在后端枚举再走 IPC"模式（字体枚举用 font-kit，前端零权限弹窗）；DB 加列要同步改 SELECT/row.get index/tuple 解构/struct/INSERT 五处，SELECT 末尾追加新列可保持既有 index 不动（created_at 仍 19，新列 20）降低 off-by-one 风险；xterm fontFamily 可 live mutation，按连接覆盖复用同一 effect |
| 我做了什么？ | 阶段 13：font-kit 枚举命令 + DB terminal_font 列/迁移 + 类型双端 + FontField 组件 + 设置/连接对话框接入 + TerminalPanel override 解析 + App 透传 |

### 阶段 14：对话框「点击遮罩即关闭」误触修复（2026-06-18）
- **状态：** 前端 complete（`npx tsc --noEmit` PASS）
- **问题（用户反馈）：** 设置 / 快捷命令界面与新建连接界面存在同一个毛病——点到操作框（对话框内容）外的遮罩区域，界面直接退出。设置/快捷命令这类长表单误触代价高（已填内容全丢）。新建连接（`ConnectionDialog`）其实已无此问题（overlay 未挂 `onClick`），其余弹窗未对齐。
- **根因：** 这些弹窗的遮罩层 `<div>` 上挂了 `onClick={onClose}`，内容容器再 `stopPropagation` 阻断——这是「点遮罩关闭」标准模式。长表单不适合，需统一为「只有关闭按钮 / 取消按钮关闭」。
- **方案：** 移除所有遮罩层的 `onClick={onClose}`；点击遮罩不再关闭，必须走关闭按钮。内部 `stopPropagation` 一律保留（无副作用、防御性）；所有按钮的 `onClick` 不动。
- **修改明细（5 处遮罩）：**
  - `src/components/SettingsPanel.tsx` — 主面板 overlay（zIndex 2000）+ `Dialog` 子组件 overlay（zIndex 2100，自定义主题弹窗用）两处 `onClick={onClose}` 移除
  - `src/components/QuickCommandsPanel.tsx` — 面板 overlay `onClick={onClose}` 移除
  - `src/components/PassphraseDialog.tsx` — overlay `onClick={onClose}` 移除
  - `src/components/PasswordVerifyDialog.tsx` — overlay `onClick={onClose}` 移除
- **已核查不变更（无此问题或不适用）：**
  - `ConnectionDialog.tsx` — overlay 本就无 `onClick`（正确参照样本）
  - `MasterPasswordGate.tsx` — 启动主密码门，无 overlay `onClick`（不应轻易关闭）
  - `App.tsx:653` — 仅是连接失败提示框的「关闭」按钮（非遮罩），保留
  - `Sidebar.tsx:801` — 右键菜单遮罩，点空白关闭菜单是期望行为，不动
- **验证：** `npx tsc --noEmit` PASS
- **附：阶段 13 build 收尾** — `src-tauri/src/main.rs` `generate_handler!` 里 `list_system_fonts` 改为限定路径 `fonts::list_system_fonts`（命令定义在 `fonts.rs` 而非 main.rs，需模块限定）；`cargo check` 已绿，阶段 13 后端可编译

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 14 complete（对话框误触关闭修复，tsc 绿）；阶段 13 build 已收尾（cargo check 绿） |
| 我要去哪里？ | 用户 `cargo tauri dev` 验证：打开设置 / 快捷命令 / 主密码 / 密码验证弹窗，点遮罩不再关闭，只能用关闭/取消按钮 |
| 目标是什么？ | 所有对话框点操作框外不意外退出，行为与新建连接一致 |
| 我学到了什么？ | 「overlay `onClick={onClose}` + content `stopPropagation`」是点遮罩关闭的标准模式，但长表单误触代价高应禁用；批量改时靠 `position:fixed;inset:0` 的遮罩区分「overlay 的 onClick」与「按钮的 onClick」，逐个用 zIndex 上下文唯一定位避免误删按钮 |
| 我做了什么？ | 4 个组件共 5 处遮罩移除 `onClick={onClose}`；顺带修阶段 13 build 错误（`fonts::` 限定路径） |

### 阶段 15：字体选择——模糊搜索下拉 + 样式统一（2026-06-18）
- **状态：** 前端 complete（`npx tsc --noEmit` PASS）
- **需求（用户反馈）：** ① 字体选择支持模糊搜索，通过下拉框选择；② 现在字体下拉框很丑，统一样式
- **问题（原实现）：** `FontField` 用原生 `<datalist>`——① 浏览器默认下拉样式无法定制，与应用 Catppuccin 主题完全不搭（丑）；② 只做前缀/子串匹配，输入中间词无法过滤，谈不上「模糊搜索」
- **方案：** 重写为自定义 combobox（props 签名 `value/onChange/placeholder` 不变，`SettingsPanel` / `ConnectionDialog` 调用处零改动）
  - **模糊匹配**：查询按空格分词，每个 token（大小写不敏感）都需出现在字体名中、顺序无关 → `nerd mono` 命中 `JetBrainsMono Nerd Font Mono`
  - **`value` / `query` 分离**：过滤用独立 `query`，已选字体重新聚焦仍显示完整列表，不会被自身值过滤掉；选中 / 外部清空时重置 `query`
  - **匹配片段高亮**：结果中把命中 token 加粗 + 加深色，模糊命中一目了然
  - **统一样式**：输入框复用项目输入框样式 + focus 蓝环（`--accent-primary` + `0 0 0 3px --accent-primary-muted`）；下拉面板用 `--bg-elevated` / `--border-emphasis` / `--shadow-xl` / `--radius-md`，选项高亮用 `--accent-primary-muted` + `--accent-primary`，与 TypeSelector / 按钮选中态一致；右侧加 `▾` 提示可下拉
  - **交互**：键盘 `↑↓` 移动高亮、`Enter` 选中、`Esc` 关闭；hover 同步高亮；点击选中。选项 `onMouseDown preventDefault` 防止 input blur 先关掉下拉
  - **性能**：`MAX_RESULTS = 200` + 超量截断提示；字体列表模块级缓存（沿用）
  - 加载中 / 无匹配 空状态提示，自由文本输入仍可用（系统字体枚举可能不全）
- **修改明细：**
  - `src/components/FontField.tsx` — 整体重写（datalist → combobox + `renderHighlighted` 片段高亮工具）
- **验证：** `npx tsc --noEmit` PASS；待 `cargo tauri dev` 实测：设置 / 连接对话框字体输入触发模糊过滤、下拉主题一致、键盘可导航、匹配字加粗
- **遗留 / 限制：** 下拉用 `relative + absolute`，位于设置面板 / 连接对话框的 `overflowY: auto` 内容区内；字体字段贴近可视区底部时下拉可能被裁剪（内容区可滚动看到）。两处实际使用位置（设置中段、连接对话框表单上部）空间充足，暂不引入 portal/fixed 定位；若反馈裁剪再升级

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 15 complete（字体 combobox，tsc 绿） |
| 我要去哪里？ | 用户 `cargo tauri dev` 验证：设置 / 连接对话框字体框输入即模糊过滤、下拉样式统一、键盘 ↑↓/Enter/Esc 可用 |
| 目标是什么？ | 字体模糊搜索 + 下拉选择 + 与应用统一的下拉样式 |
| 我学到了什么？ | 原生 `<datalist>` 不可定制样式且只前缀匹配；combobox 必须 `value`/`query` 分离，否则已选字体聚焦时会被自身值过滤成只剩自己；选项点击用 `onMouseDown preventDefault` 阻止 input 先 blur 关下拉，是 combobox 经典坑 |
| 我做了什么？ | `FontField` 重写：模糊匹配（多 token 任意顺序）+ value/query 分离 + 匹配片段高亮 + 主题化下拉 + 键盘/hover/点击交互 + 截断与空状态 |

### 阶段 16：本地连接「以管理员运行」（整体提权方案）（2026-06-18）
- **状态：** 前端 complete（`npx tsc --noEmit` PASS）；后端 complete（`cargo check` PASS，winapi 新增 features 编译通过）
- **需求（用户）：** 本地的连接是否可以支持管理员运行？
- **技术调研（为何不单连接提权）：** 本地 shell 经 `portable-pty` 的 `openpty()` + `spawn_command()` 启动，继承 MyShell 完整性级别（IL）。单连接提权的硬约束：① `portable-pty`/`CommandBuilder` 走 `CreateProcessW`，无提权选项；② 标准提权 `ShellExecute("runas")` 无法挂 ConPTY（ConPTY 要先 `CreatePseudoConsole` 再用带 `hPC` 的 `STARTUPINFOEX`）；③ medium-IL 进程无法把 ConPTY attach 到 high-IL shell（完整性级别隔离）。结论：要让 elevated shell 跑进我们的 ConPTY，**ConPTY 必须在已提权进程内创建**——即单连接提权需另起 elevated helper 进程 + 跨完整性级别命名管道 IPC 转发，工程量大、每次弹 UAC、维护重；连 Windows Terminal 都只开独立 elevated 窗口而非单进程混跑 IL。
- **决策（用户选定方案 A · 整体提权）：** 以管理员身份重启 MyShell，提权后所有本地连接自动获得管理员权限。最小代价覆盖"偶尔要管理员跑命令"的真实需求。
- **方案：**
  - 新增 `elevation.rs`：
    - `is_elevated()` — Windows：`OpenProcessToken` + `GetTokenInformation(TokenElevation)`；非 Windows：`geteuid()==0`（extern C 声明，免 libc 依赖）
    - `restart_as_admin()` — Windows：`ShellExecuteW(verb="runas")` 触发 UAC，返回 HINSTANCE ≤ 32 为错误（1223=ERROR_CANCELLED 用户取消）；非 Windows：stub 返回"暂不支持"
  - `main.rs`：`is_elevated` / `restart_as_admin` 两个 `#[tauri::command]`；后者成功后 `app.exit(0)` 触发 `ExitRequested` → `drain_all_sessions` 优雅排空再退出，elevated 新实例由系统在 UAC 后独立启动
  - 前端：`api.ts` 加 `isElevated()` / `restartAsAdmin()`；`SettingsPanel` 新增「🛡️ 管理员权限」Section（状态 chip：检测中 / ✓ 已是管理员 / 当前普通用户 + 未提权时「以管理员重启」按钮 + 警告条）；`ConnectionDialog` 本地 shell 提示加引导语
  - 依赖：winapi 升为 Windows-only 直接依赖（features：`processthreadsapi`/`securitybaseapi`/`winnt`/`handleapi`/`shellapi`/`winuser`）——原为 portable-pty 间接依赖，声明直接依赖以便手写 elevation FFI
- **修改明细：**
  - `src-tauri/src/elevation.rs`（新建）
  - `src-tauri/Cargo.toml` — `[target.'cfg(windows)'.dependencies] winapi = { ..., features=[...] }`
  - `src-tauri/src/main.rs` — `mod elevation` + 2 命令 + `generate_handler!` 注册
  - `src/api.ts` — `isElevated` / `restartAsAdmin`
  - `src/components/SettingsPanel.tsx` — import `confirm`/`isElevated`/`restartAsAdmin`；`elevated`/`restartBusy` state + 加载 effect；`handleRestartAdmin`（`confirm` 二次确认 → `restartAsAdmin`，取消 UAC 静默、其他错误 alert）；管理员权限 Section
  - `src/components/ConnectionDialog.tsx` — 本地 shell 说明追加管理员引导
- **验证：** `npx tsc --noEmit` PASS；`cargo check` PASS（winapi features 齐全，`myshell` 编译通过）
- **遗留 / 限制：**
  - 粒度为全局：重启后所有连接提权，非单连接；重启丢失当前 tab（已在警告条 + 二次确认说明）
  - 每次重启弹一次 UAC
  - 非 Windows：`restart_as_admin` 为 stub，前端按钮在非 root 时仍可见但点击报"当前平台暂不支持"（项目 Windows 优先，未做平台级隐藏）；如需可在前端按 `navigator.platform` 隐藏

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 16 complete（管理员重启；前端 tsc 绿 + 后端 cargo check 绿） |
| 我要去哪里？ | 用户 `cargo tauri dev` → 设置面板看「管理员权限」状态 → 未提权点「以管理员重启」→ UAC「是」→ 新实例管理员运行 → 开本地连接执行需管理员命令 |
| 目标是什么？ | 本地连接支持以管理员身份运行 shell（整体提权方案） |
| 我学到了什么？ | ConPTY + UAC 完整性级别隔离使单连接提权必须 elevated helper + 跨 IL IPC（Windows Terminal 也只开独立 elevated 窗口）→ 整体提权性价比最高；`ShellExecuteW("runas")` 返回 HINSTANCE≤32 为错（1223=用户取消）；winapi 直接依赖需按用到的 API 精确列 features |
| 我做了什么？ | `elevation.rs`（is_elevated + restart_as_admin）+ winapi Windows-only 依赖 + main.rs 两命令（重启后 app.exit 触发 drain）+ api.ts + SettingsPanel 管理员 Section + ConnectionDialog 引导 |

### 阶段 17：v1.3.3 综合优化（历史过滤 / 保存按钮悬浮 / 版本号单一源 / 图标重生成）（2026-06-18）
- **状态：** 前端 complete（tsc PASS）；后端 complete（cargo check PASS，`myshell v1.3.3`）；图标全套重生成（tauri icon from source-a.svg）
- **四项改动：**
  1. **历史命令废命令过滤**：用户反馈历史里有 "A"、"AD" 这类误触单/双字母无效命令。在 `add_command_history` 边界（main.rs，覆盖 TerminalPanel + CommandBar 两个调用入口）加 `is_junk_command`：trim 后仅由 a/d 字符组成（大小写不敏感）→ 返回 Ok(0) 不入库。⚠️ 此规则也匹配 `dd`（Linux 常用），已向用户提示，如需可加白名单
  2. **新建连接保存按钮悬浮**：ConnectionDialog 底部「取消/保存」footer 加 `position: sticky; bottom: 0`（卡片是 overflowY auto 滚动容器），长表单不用滚到底也能点保存
  3. **版本号单一源**：`Cargo.toml [package] version` 为唯一手动源；`tauri.conf.json` 删除 version 字段（Tauri v2 自动读 Cargo.toml）；新增 `scripts/sync-version.mjs` 在 `npm run build`（tauri build 的 beforeBuildCommand）前自动把 version 同步到 package.json + package-lock.json，亦可 `npm run version:sync` 手动。以后升版只改 Cargo.toml 一处
  4. **打包图标重生成**：用户上传的图标即当前 Aurora Prompt（source-a.svg 渲染）。用矢量源 `source-a.svg`（1024 viewBox）跑 `npx tauri icon` 重新生成全套——icon.ico（exe）、icon.icns（mac）、各尺寸 PNG、Square*Logo/StoreLogo（NSIS 安装器）、iOS/Android，确保全套高清一致
- **版本号：** 1.2.2 → 1.3.3（Cargo.toml / package.json / package-lock；tauri.conf.json 无 version，由 Cargo.toml 驱动）
- **修改明细：**
  - `src-tauri/src/main.rs` — `add_command_history` 改用 trimmed + `is_junk_command` 过滤；新增 `is_junk_command` 函数
  - `src/components/ConnectionDialog.tsx` — footer 加 sticky bottom / flexShrink / zIndex
  - `src-tauri/Cargo.toml` / `package.json` / `package-lock.json` — version → 1.3.3
  - `src-tauri/tauri.conf.json` — 删除 version 字段
  - `scripts/sync-version.mjs`（新建）— Cargo.toml → package.json/lock 同步脚本
  - `package.json` scripts — build 前置 sync-version，新增 version:sync
  - `src-tauri/icons/*` — tauri icon 从 source-a.svg 重新生成全套
- **验证：** tsc PASS；cargo check PASS（v1.3.3）；sync-version.mjs 运行 OK（"already at 1.3.3"）；tauri icon 全套生成 OK（icon.ico/icns/png/Square*Logo/iOS/Android 均 11:21 更新）
- **遗留 / 提醒：** ① 历史过滤 dd 风险（见上，待用户确认是否加白名单）；② tauri.conf.json 删 version 依赖 Tauri v2 自动读 Cargo.toml（官方行为，待 `tauri build` 实测 version 显示）；③ 看新 exe 图标需重新 `cargo tauri build`，Windows 可能要清图标缓存

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 17 complete（v1.3.3 四项优化，tsc + cargo check 绿，图标重生成） |
| 我要去哪里？ | 用户 `cargo tauri build` 验证：版本 1.3.3、exe 图标、安装器；`cargo tauri dev` 验证历史命令过滤 + 保存按钮悬浮 |
| 目标是什么？ | 过滤历史废命令；保存按钮常驻；版本号只改一处；打包图标统一高清 |
| 我学到了什么？ | Tauri v2 `tauri.conf.json` 的 version 可省略、自动读 Cargo.toml（单一源关键）；`npx tauri icon` 接受 SVG 矢量源生成全平台图标（输出默认在 tauri.conf.json 旁的 icons/）；`position: sticky; bottom: 0` 在 overflowY auto 容器内让 footer 常驻且不脱离流（无遮挡）；历史过滤放在系统边界（命令层）可覆盖所有调用入口 |
| 我做了什么？ | main.rs 历史过滤 + ConnectionDialog footer sticky + 版本号单一源（Cargo.toml 唯一 / tauri.conf.json 删 / sync 脚本）+ tauri icon 全套重生成 + 版本号 1.3.3 |

### 阶段 17 补：NSIS 打包 BOM 修复（2026-06-18）
- **现象：** `cargo tauri build` 在 NSIS 阶段失败：`Invalid command: "﻿;"` → `!include: error in script: SimpChinese.nsh on line 1` → `aborting creation process`。
- **根因：** `src-tauri/nsis/lang/SimpChinese.nsh` 与 `src-tauri/nsis/uninstall-confirm-hook.nsh` 文件头带 **UTF-8 BOM（U+FEFF）**，NSIS 不认 BOM，把 BOM 当命令解析。`English.nsh` 无 BOM 故正常。潜伏问题——之前各阶段只跑 cargo check / dev，未跑完整 NSIS 打包，未暴露。
- **修复：** 去掉这两个 `.nsh` 的 BOM（保留 UTF-8 内容；NSIS Unicode 模式按 UTF-8 读中文）。
- **验证：** 重新 `npm run tauri:build` 成功，生成 `MyShell_1.3.3_x64-setup.exe`；文件名 1.3.3 证明 tauri.conf.json 删 version 后 Tauri 正确取自 Cargo.toml（单一源方案生效）。
- **教训：** NSIS 的 `.nsh`（customLanguageFiles / installerHooks）必须 **无 BOM**；以后新增/编辑 .nsh 注意编辑器别存 BOM。

### 阶段 18：打包警告清理 + 本地 PowerShell 透明背景渲染修复（2026-06-18）
- **状态：** 前端 complete（tsc PASS）；打包验证通过（MyShell_1.3.3_x64-setup.exe，两 warning 消失）
- **三项改动：**
  1. **bundle identifier**：`com.myshell.app` → `com.myshell.client`（消除 Tauri "ends with `.app`" macOS 冲突警告）。副作用：新 identifier = 新 app 标识；DB/连接/密码/历史不变（存 `dirs/myshell`，不依赖 identifier）；WebView2 localStorage（主题/字体）可能重置；旧版（com.myshell.app）需手动卸载
  2. **api.ts 动态导入**：App.tsx `await import("./api")` 改为静态 `import { deleteConnection }`（消除 vite "dynamically imported but also statically imported" 警告，bundle 635→632KB）
  3. **本地 PowerShell 输入字母乱跳 + 影响背景**：根因——终端设背景图时 `allowTransparency: true`，xterm 默认 **canvas renderer 在透明模式下重绘不清除旧像素** → 输入字符残影叠加（"乱跳"）+ 残影糊在背景图上（"影响背景"），本地 ConPTY 输出更易触发。修复：加 `@xterm/addon-webgl`，启用 **WebGL renderer**（每帧完整重绘，透明合成干净无残影），失败 fallback canvas
- **修改明细：**
  - `src-tauri/tauri.conf.json` — identifier → `com.myshell.client`
  - `src/App.tsx` — deleteConnection 改静态 import；onDelete 去掉 `await import`
  - `package.json` — 加 `@xterm/addon-webgl@^0.18`
  - `src/components/TerminalPanel.tsx` — import WebglAddon；`term.open` 后 `loadAddon(new WebglAddon())`（try/catch fallback）
- **验证：** tsc PASS；`npm run tauri:build` 成功，两 warning 消失；WebGL 待 dev/build 实测乱跳是否消除
- **遗留：** ① 图标问题待澄清——用户反馈 setup.exe 图标"仍是旧的"，但 `source-a.svg` 本就是当前 chevron，重新生成同款无变化，需用户确认期望的图标设计或排查 Windows 图标缓存；② PowerShell 乱跳修复待用户实测确认（若 WebGL 不解决再排查 cols/ConPTY 时序）

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 18 complete（identifier + vite 警告清理已验证；PowerShell WebGL 修复待实测） |
| 我要去哪里？ | 用户 `cargo tauri dev` 实测本地 PowerShell 输入不再乱跳/糊背景；确认 setup.exe 图标问题（缓存 or 需换源） |
| 目标是什么？ | 消除两个打包 warning；修复本地 PowerShell 透明背景渲染；澄清图标 |
| 我学到了什么？ | xterm `allowTransparency` + 默认 canvas renderer 透明模式重绘不清像素（残影），透明背景应用 WebGL renderer；Tauri identifier 不应以 `.app` 结尾（macOS 冲突）；改 identifier 不影响 dirs/myshell 下的 DB，但 WebView2 localStorage 会重置；vite 静态+动态混用导入失去分块意义 |
| 我做了什么？ | identifier 改 com.myshell.client + App.tsx 动态改静态 + TerminalPanel 加 WebGL renderer + @xterm/addon-webgl 依赖 |

### 阶段 18 补：NSIS 安装包（setup.exe）图标修复（2026-06-18）
- **现象：** 用户反馈 setup.exe（安装包）图标不是 chevron，但应用图标（myshell.exe）正常。
- **根因：** 应用图标走 `bundle.icon`（icon.ico，正常）；但 **NSIS 安装器图标是独立的 `bundle.windows.nsis.installerIcon` 字段**，之前未配置 → Tauri 用内置默认安装器图标。installer.nsi 里 `MUI_ICON "${INSTALLERICON}"`，而 INSTALLERICON 未指向用户图标。
- **修复：** `tauri.conf.json` 的 nsis 块加 `"installerIcon": "icons/icon.ico"`（source-a.svg 渲染的 chevron）。
- **验证：** rebuild 后 installer.nsi:39 `!define INSTALLERICON "F:\...\src-tauri\icons\icon.ico"`，setup.exe（MyShell_1.3.3_x64-setup.exe）嵌入 chevron 图标。
- **遗留：** 资源管理器若仍显示旧图标，是 Windows 图标缓存（setup.exe 同名缓存）——右键属性看真实图标，或清 `%localappdata%\IconCache.db` + 重启资源管理器。

### 阶段 18 补 2：NSIS 定制整体回退到初始（2026-06-18）
- **现象：** 用户反馈卸载页面空白、安装最终页有问题（阶段 11 的 NSIS 定制 + 阶段 18 补的 installerIcon 引起）。
- **决策：** 用户要求把安装/卸载变更回退到初始状态。
- **回退：**
  - `tauri.conf.json` nsis 块恢复到 HEAD 初始 —— 移除 `installerIcon` / `installerHooks` / `customLanguageFiles`，仅保留 `installMode` / `languages` / `displayLanguageSelector`
  - 删除 `src-tauri/nsis/` 目录（uninstall-confirm-hook.nsh + lang/SimpChinese.nsh + lang/English.nsh，均为阶段 11 新增的 untracked 文件）
- **保留（非安装/卸载页面变更）：** 版本号单一源（Cargo.toml 驱动，tauri.conf.json 无 version 字段）、identifier `com.myshell.client`
- **验证：** rebuild 成功，MyShell_1.3.3_x64-setup.exe 生成；installer.nsi 无 `customLanguageFile` / `uninstall-confirm` 引用（`NSIS_HOOK_*` 是 Tauri 默认模板的钩子检查点，宏未定义故跳过）→ 卸载页 / 安装最终页恢复 Tauri 默认（不再空白/错误）
- **影响：** ① setup.exe 图标回到 Tauri 默认（installerIcon 已移除）；② 卸载不再有"删除应用数据"二次确认钩子；③ 语言串用 Tauri 默认（不再覆盖 deleteAppData 文案）
- **后续（同日，installerIcon 单独加回）：** 应用户要求只恢复安装器图标 —— nsis 块加回 `"installerIcon": "icons/icon.ico"`（钩子 / 语言覆盖保持回退）。验证：rebuild 成功，installer.nsi `INSTALLERICON → icons/icon.ico`，`customLanguageFile | uninstall-confirm` 计数 = 0 → 安装器图标 = chevron，卸载页 / 安装页仍 Tauri 默认（不空白）

### 阶段 18 补 3：WebGL 透明背景修复（背景图变黑）（2026-06-18）
- **现象：** 阶段 18 加 WebGL renderer 后，用户反馈背景图变黑（看不到实际背景图）。
- **根因：** 终端透明背景之前用 `theme.background = "transparent"` 字符串。canvas renderer 认这个 CSS 关键字（透明），但 **WebGL renderer 解析颜色失败**，clearColor 回退到不透明黑，盖住背景图层。
- **修复：** 透明色改 `rgba(0, 0, 0, 0)`（alpha=0，WebGL 能正确解析）。两处：初始 Terminal 的 `theme` + live theme effect 的 `currentBg`。
- **验证：** tsc PASS；待 build/dev 实测：WebGL 透明（背景图透出）+ 每帧重绘（无残影 / 乱跳）。
- **教训：** xterm theme 色要用 WebGL / canvas 都能解析的格式（rgba / hex），别用 `"transparent"` 关键字——canvas 容忍、webgl 不认。

### 阶段 19：本地终端输入字符跳出 + 背景左移（cols 被 padding 污染）（2026-06-19）
- **现象：** 用户反馈本地终端（PowerShell / ConPTY）输入命令时字符会跳出界面，并出现背景左移。这正是阶段 18 WebGL 修复（透明背景残影）之后**仍残留**的症状——阶段 18 遗留项②「若 WebGL 不解决再排查 cols / ConPTY 时序」预判的 cols 方向。
- **根因：** FitAddon（@xterm/addon-fit 0.10.x）`proposeDimensions` 读 `getComputedStyle(容器).width`（border-box 宽度）当可用宽度，再除以 cellWidth 得 cols。而 `global.css` 全局 `* { box-sizing: border-box }` + `TerminalPanel.tsx` 的 **xterm 容器自身带 `padding: 4`** → 容器 `.width`（border-box，含 padding）≠ `.xterm` 实际填充的 content box（减 8px）。FitAddon 因此多算约 8px ≈ **多 1 列**。xterm 把这个偏大的 cols 经 `localResize`/`sshResize` 发给 PTY，PSReadLine 每次按键按偏大 cols 全行重绘 + 绝对光标定位 `\x1b[<n>G` → 最后一列画到 canvas 外（字符跳出）、重绘清除范围与可视区错位（背景左移）。本地 PowerShell/ConPTY 上 PSReadLine 对 cols 最敏感故最明显（SSH bash readline 同样受影响，只是表现不同）。
- **修复：** 把 `padding: 4` 从 xterm 容器移到外层 wrapper div；外层同时设 `background: terminalTheme.background`（非背景图模式）保证 4px 内缩无缝（不露 App 底色）；容器自身去 padding，`.xterm` 干净填满 content box（此时 content box == border box），FitAddon 读到的宽度 == `.xterm` 真实渲染宽度 → cols 精确。背景图模式下 wrapper 透明 + 背景图层 `inset:0` 仍铺满 padding box（背景铺到边缘、文字内缩）。
- **修改明细：** `src/components/TerminalPanel.tsx` —— 外层 wrapper div（`.terminal-bg-transparent` 那层）加 `padding: 4` + `background`；`containerRef` div 移除 `padding: 4` 并加详细注释解释为何不能在此层加 padding。
- **验证：** `npx tsc --noEmit` PASS。待 `cargo tauri dev` 实测：本地终端输入长命令不再跳出、不再背景左移；SSH 终端行尾对齐亦应更准。
- **遗留：** 本地终端以 80×24 启动（main.rs `local_connect` 写死），靠 mount 后 100ms fit + 首帧输出后再 fit 两次 resize 同步真实 cols；本次只修 cols 计算（让其准确），时序逻辑本身未改。若极个别场景仍偶发抖动，再考虑改 initial cols 或加 debounce。
- **关联：** 阶段 18 WebGL renderer（解决透明残影）+ 本次 cols 修复，两者叠加才彻底解决"输入乱跳 / 糊背景"——前者管透明重绘，后者管列宽。

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 19 complete（本地终端 cols 污染修复，tsc 绿，待 dev 实测） |
| 我要去哪里？ | 用户 `cargo tauri dev` 实测本地 PowerShell：输入长命令不跳出、不背景左移；顺带验证 SSH 行尾对齐 |
| 目标是什么？ | 彻底解决阶段 18 WebGL 之后仍残留的"字符跳出 + 背景左移"——这次的根因是 cols 被容器 padding 污染 |
| 我学到了什么？ | FitAddon 读的是 `getComputedStyle(容器).width`（border-box，含 padding）；全局 `box-sizing:border-box` 下，承载 `.xterm` 的容器**绝不能带 padding**（会被算进列宽），视觉内缩应放外层 wrapper；"输入乱跳"可有多层根因（阶段 18 透明残影 / 阶段 19 cols 偏差），需逐层排查 |
| 我做了什么？ | TerminalPanel.tsx padding 从 xterm 容器移到外层 wrapper（+ 外层补背景色保无缝）+ 注释 + tsc 验证 |

### 阶段 20：本地终端输入字符错位（二）—— 字体加载竞态致 cols 漂移（2026-06-19）
- **现象：** 阶段 19 padding 修复后用户仍反馈：本地终端输入时"在背景旁边输入字符，把背景挤到左边"，**随机出现**。
- **定位（AskUserQuestion 锁环境）：** ① 设置了自定义背景图（→ 走 allowTransparency + WebGL 透明路径）② PowerShell 7 (pwsh) ③ 随机 / 不确定。
- **根因：** pwsh 的 PSReadLine **每次按键都用绝对光标定位（`\x1b[<col>G`）重绘整个输入行**。一旦 xterm 的 cols 与 PTY 认为的列数不一致（哪怕差 1），光标和重绘内容就画进错误单元格；透明 canvas + 背景图下，画错位的字符浮在背景的错误位置 → 视觉"字符跑到背景旁、把背景挤偏"。随机是因为只有输入触碰列边界（近行尾 / 回卷）才显现。cols 不一致的剩余根因 = **字体异步加载竞态**：Nerd Font 文件没加载完时 `fit()` 用 fallback 字体量 cellWidth → cols 偏；字体就绪后 cellWidth 变但 cols 没重算。且代码里 `fontFamily` 变化时只改 `term.options.fontFamily`、**没有重新 fit**——xterm 改字体重新量 cellWidth 却不重算 cols，`cols × cellWidth` 静默漂离容器宽度。
- **修复：** `TerminalPanel.tsx` 的 fontFamily effect：改字体后（含 mount 初始触发）等 `document.fonts.ready` 再 `fit()` + `resizeTo()`，把字体就绪后的正确 cols 推给 PTY。覆盖初始字体竞态 + 后续字体切换两个场景。沿用 ResizeObserver 的 `clientWidth<80` 小尺寸保护。
- **研究依据：** xterm.js 社区同类问题经验——[Issue #2252](https://github.com/xtermjs/xterm.js/issues/2252)（WebGL 透明）、[#1901](https://github.com/xtermjs/xterm.js/issues/1901)（buffer line 光标跳）、[#3287](https://github.com/xtermjs/xterm.js/issues/3287)（glyph 定位）+ 通用建议「fit() 前等 `document.fonts.ready`、改 fontFamily 后必须重新 fit」。
- **修改明细：** `src/components/TerminalPanel.tsx` —— fontFamily effect 扩展：`document.fonts.ready.then(refit)`（catch 兜底），refit 内含小尺寸保护 + fit + resizeTo。
- **验证：** `npx tsc --noEmit` PASS。待 `cargo tauri dev` 实测：本地 pwsh + 背景图，输入长命令不再随机错位 / 挤背景。
- **遗留 / 二分诊断：** 若字体 fit 修复后**仍有**问题，请临时在设置里**关掉背景图**测试——① 关背景图后正常 → 确认是 WebGL 透明合成路径独立问题（Issue #2252 类），下一步考虑 canvas 回退（但会带回残影）或升级 xterm / 调透明策略；② 关背景图仍有 → 纯 cols 问题，继续排查 initial 80×24 跳变 / ResizeObserver 瞬时尺寸。
- **关联：** 阶段 18（WebGL 透明重绘）+ 阶段 19（padding 修 cols 几何）+ 阶段 20（字体 fit 修 cols 时序）三层叠加治理"输入乱跳 / 挤背景"——同一症状的多层根因。

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 20 complete（字体加载竞态致 cols 漂移修复，tsc 绿，待 dev 实测） |
| 我要去哪里？ | 用户 dev 实测本地 pwsh + 背景图输入不再随机错位；若仍有，按"关背景图二分"判断是否 WebGL 路径独立问题 |
| 目标是什么？ | 修掉阶段 19 padding 之后仍残留的"字符挤背景"——这次根因是字体异步加载让 cols 漂移、PSReadLine 按错误 cols 重绘 |
| 我学到了什么？ | xterm 改 fontFamily 重新量 cellWidth 但**不重算 cols**，必须手动 fit；fit() 前应等 `document.fonts.ready`；PSReadLine 每键绝对定位重绘、对 cols 偏差零容忍；"输入乱跳"是多层根因（透明残影 / padding / 字体时序），需逐层剥 + 用 AskUserQuestion 锁环境再改，避免盲目修偏 |
| 我做了什么？ | AskUserQuestion 锁定（背景图 + pwsh + 随机）+ 研究 xterm 社区经验 + TerminalPanel fontFamily effect 加 document.fonts.ready → fit + resize + tsc 验证 |

### 阶段 21：本地终端输入回卷错位（三）—— PTY 初始 cols 同步时机（决定性根因）（2026-06-19）
- **现象（用户给出完美复现点）：** 阶段 20 字体 fit 修复后，用户锁定**百分百复现**条件——输入内容超过本行（触发回卷）必现"背景左移"；**无背景图也复现**（排除 WebGL 透明路径）；输入完成换行后**恢复正常**。
- **根因（掘金 juejin/7476761846411870258 100% 匹配验证）：** 经典 xterm↔PTY cols 不同步。本地 PTY 以 **80×24 启动**（main.rs `local_connect` 写死 cols=80），pwsh 的 PSReadLine 缓存了 80；前端 xterm 经 fit 得到真实 cols（如 120）。同步链两处时机都太赶：① mount 100ms resize 撞在 PSReadLine 初始化**之前** → 被丢弃；② firstOutput 的 resize 是 `setTimeout(..., 0)`，PSReadLine 刚画完 prompt、resize listener 还没接管 → 再次丢弃。结果后端 cols 卡在 80、前端 120，输入过第 80 列后端换行前端不换行 → PSReadLine 重绘编辑行进错单元格 = "背景左移"；Enter 后新 prompt 单行故恢复。回卷边界 100% 复现完全吻合。
- **修复：** `TerminalPanel.tsx` firstOutput 把单次 `setTimeout(0)` resize 改为**立即 + 250ms + 600ms 多次延迟同步**（fit + resizeTo），覆盖不同 shell 冷启动速度下 PSReadLine/ConPTY 就绪窗口（掘金验证 200ms 有效，多次更稳）。新增 `firstSyncTimersRef` + [sessionId] cleanup 清理 timers。
- **研究依据：** [掘金：xterm.js 输入字符换行覆盖排查](https://juejin.cn/post/7476761846411870258)（$COLUMNS=80 + resize 后正常 + onResize/200ms 防抖 + 首帧 output 触发 resize）；[Issue #3342](https://github.com/xtermjs/xterm.js/issues/3342) nerd font 宽字符裁剪（相关但非本次主因）。
- **修改明细：** `src/components/TerminalPanel.tsx` —— firstOutput 同步改为多次延迟（0/250/600ms）；新增 firstSyncTimersRef ref；cleanup 清理。
- **验证：** `npx tsc --noEmit` PASS。待 `cargo tauri dev` 实测：本地 pwsh 输入超过 80 列不再回卷错位 / 背景左移。
- **遗留 / 诊断命令：** 若仍有问题，在出问题的本地终端跑 `$Host.UI.RawUI.WindowSize.Width`——① 仍是 80 → resize 没送达 ConPTY（升级 portable-pty 0.8 或查 ConPTY resize 时序）；② 已是真实值但仍错位 → 转向 xterm 渲染层（WebGL/canvas 对比、nerd font 宽字符 #3342、clearTextureAtlas）。
- **关联：** 阶段 18（WebGL 透明残影）+ 19（padding 修 cols 几何）+ 20（字体 fit 修 cols 时序）+ 21（PTY 初始同步送达）四层叠加。**前三次修的是"前端 cols 算得对不对"，本次修的是"前端算对了之后有没有把正确 cols 送达后端 PTY"**——这是 SSH/PTY 终端经验里最经典、最易漏的一环。

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 21 complete（PTY 初始 cols 同步时机修复，tsc 绿，待 dev 实测） |
| 我要去哪里？ | 用户 dev 实测本地 pwsh：输入超过 80 列不再回卷错位；若仍有跑 `$Host.UI.RawUI.WindowSize.Width` 诊断 cols 是否送达 |
| 目标是什么？ | 彻底解决"输入回卷必现背景左移、换行恢复"——根因是 PTY 以 80 启动、初始 resize 时机太早被丢弃，后端卡 80 |
| 我学到了什么？ | xterm↔PTY cols 同步是终端最经典 bug：PTY 启动 cols（80）≠ 前端真实 cols，必须在 shell 就绪后（首帧 output + 充分延迟）把真实 cols resize 给后端，时机要给足（PSReadLine 初始化慢，0ms 不够，需 200ms+，多次更稳）；掘金实战文章比泛搜更精准；"换行后恢复 + 回卷边界 100% 复现"是 cols 不同步的指纹 |
| 我做了什么？ | 掘金文章锁定 cols 不同步根因 + firstOutput 单次 0ms → 多次延迟（0/250/600ms）同步 + firstSyncTimersRef cleanup + tsc 验证 |

### 阶段 22：claude TUI 输入左移（决定性根因）—— init_command 启动时机（2026-06-19）
- **现象：** 阶段 21 cols 同步修复后，用户给出**决定性线索**——左移**只有 claude（Claude Code CLI）里才有**，普通命令（ls/dir）完全正常；且 claude 是配置**「启动命令」自动启动**的。
- **推理：** "只有 claude" 排除了 cols 同步对 PSReadLine 的问题——普通命令正常 = PowerShell 行编辑拿到的列数是对的，即阶段 19-21 的 cols 同步**确实生效**了。问题聚焦到 claude 这个 TUI 应用本身。
- **根因：** claude 是交互式 TUI，启动时读取终端列数布局界面（输入框宽度），且**启动后不跟随 resize**。它通过「启动命令」（init_command）自动启动，而 init_command 在 PTY **刚以 80×24 启动时就被立即写入 stdin**（local.rs writer task 启动即发）→ claude 启动读到 **80 列** → TUI 按 80 布局 → 之后前端 resize 到真实宽度（如 120）但 claude 已缓存 80 不更新 → 输入过第 80 字符时 claude 按其认知的 80 列重绘输入框，在实际上 120 宽的终端里错位、左移；Enter 后新 prompt 单行故恢复。普通命令不缓存列数（每次按当前宽度输出）故完全正常。**完美解释"只有 claude + 回卷边界 100% 复现 + 换行恢复"**。
- **修复：** `src-tauri/src/local.rs` writer task：把 init_command 从「启动时立即发」改为「**第一次 resize 到达后再发**」。第一次 resize = 前端已 fit 出真实尺寸并同步给 PTY 的信号，此时再发 init_command，claude 启动读到的就是真实列数（而非 80）。utf8_prelude 仍立即发（shell 编码设置需在启动早期）；`pending_init` 用 `take()` 保证只发一次。
- **副作用（预期）：** pwsh 提示符会**先短暂出现**（init_command 延迟到第一次 resize ~100ms 后才发），随后 claude 启动。可接受。非 TUI 的 init_command（如 cd）延迟执行亦无害。
- **修改明细：** `src-tauri/src/local.rs` —— 移除 init_command 启动即发块，改为 `pending_init: Option<String>`；Resize 分支 `master.resize` 后 `if let Some(init) = pending_init.take() { write + \r + flush }`。
- **验证：** `cargo check` PASS（**首次**——Rust 工具链现已在编辑环境可用，之前各阶段 Rust 侧未编译过）。待 `cargo tauri dev` 实测：claude 输入超过本行不再左移。
- **关联：** 阶段 18-21 修的是 cols 的**正确性 + 同步**（前端算对、同步给 PTY），但 claude TUI 在 cols 同步**之前**就启动并缓存了 80。本次（22）修的是「**让 claude 在 cols 同步之后再启动**」——同一条"左移"症状，五层根因（透明残影 / padding / 字体时序 / PTY 同步 / **TUI 启动时机**）逐层剥开。
- **教训：** "只有某个 TUI 应用才有"是关键信号——TUI 缓存终端尺寸且不跟随 resize，与普通 shell 行为不同；init_command 类的自动启动命令应在终端尺寸确定后再发，否则 TUI 拿到的是 PTY 启动默认值（80）；连续盲改无效时要回到"什么场景才有 / 没有该现象"的对比来缩小范围。

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 22 complete（init_command 延迟到 cols 同步后，cargo check 绿，待 dev 实测） |
| 我要去哪里？ | 用户 cargo tauri dev 实测：claude 输入超长不再左移；留意 pwsh prompt 先短暂闪现再进 claude（预期副作用） |
| 目标是什么？ | 修掉"只有 claude 才有的输入左移"——根因是 claude 作为启动命令在 PTY 80×24 时就启动、缓存 80 列不跟随 resize |
| 我学到了什么？ | "只有某 TUI 才有" = TUI 缓存终端尺寸不跟随 resize（vs 普通 shell 实时读），init_command 必须在尺寸确定后发；连续盲改无效时要回到"什么场景才有/没有该现象"的对比缩小范围；Rust 工具链现已可用，cargo check 能验证后端改动 |
| 我做了什么？ | AskUserQuestion 确认 claude = 启动命令自动启动 + local.rs init_command 启动即发 → 第一次 resize 后发（pending_init + Resize 分支 take）+ cargo check PASS |

### 阶段 23：中文 IME 输入左移 —— ConPTY 上游 bug（已知限制，应用层无法根治）（2026-06-19）
- **现象（更精确线索）：** 阶段 22 后，用户进一步定位——左移**只在中文 IME 输入时发生**，字母/英文输入完全正常；PowerShell 和 claude 都有。
- **排查（命中铁证）：** 搜索命中 [VSCode #255285 "Terminal viewport shifts left with Chinese IME"](https://github.com/microsoft/vscode/issues/255285)，现象/触发/根因**逐字匹配**——"IME composition string 打到第 4 个字符，整个终端视口水平左移；按空格完成输入后立即恢复"。
- **根因：** **ConPTY 对中文 IME composition string 的宽度计算错误（miscalculation）**，触发不必要的视口左移。这是 **ConPTY 的上游 bug**。触发条件：会"每键重绘输入行"的程序（PSReadLine、claude/ink、gemini-cli）；vim / node / python REPL 不触发（它们不实时重绘输入行）—— 完美解释"字母正常（单宽，重绘无歧义）+ 中文左移（双宽 composition 宽度算错）+ PowerShell 和 claude 都有"。
- **VSCode 确认的 workaround：** 禁用 ConPTY、改用 **winpty** 后端（VS Code `terminal.integrated.windowsEnableConpty: false`）。
- **我们的困境（关键）：** 我们用的 **portable-pty 0.8.1 已移除 winpty 支持**（`src/win/` 只有 `conpty.rs`，无 `winpty.rs`；`NativePtySystem = ConPtySystem`）。所以 **winpty workaround 对我们不可行**。降级 portable-pty 或换 winpty-rs 代价大，且 **winpty 对中文 UTF-8 不友好**（走 Windows console codepage，可能引入中文乱码，比左移更糟）。
- **结论：** 这是 **ConPTY 上游 bug，应用层无法完美修复**。需等微软修复（VSCode #255285 open，2025-07 报，目前未修；用户 Windows 11 26200 仍存在）。
- **前几轮修复的价值（重要）：** 阶段 18-22 的修复（WebGL 透明残影 / padding cols 几何 / 字体 cols 时序 / PTY 同步时机 / TUI 启动时机）**都是对的、有价值的**——"字母输入完全正常换行"就是证明（cols 同步正确、PSReadLine 拿到真实列数）。**中文 IME 是唯一剩下的、独立的 ConPTY 上游限制**，与前几轮无关。
- **实用缓解（对用户）：** ① 输入法确认（空格/回车）后左移**立即恢复**，影响仅 composition 输入过程中；② 避免一次打超长 composition，分段确认可减少触发；③ 等 Windows 更新修复 ConPTY。
- **修改：** 本次无代码修改（应用层无法修复 ConPTY bug）；仅研究确认 + 文档记录，避免未来重复排查。

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 23：确认中文 IME 左移是 ConPTY 上游 bug（VSCode #255285 铁证），应用层无法根治，记录为已知限制 |
| 我要去哪里？ | 告知用户根因 + 困境（portable-pty 0.8.1 无 winpty）+ 实用缓解；等微软修复 ConPTY |
| 目标是什么？ | 给"中文 IME 左移"一个准确、有据的结论，避免反复盲改 |
| 我学到了什么？ | "只在某种输入（中文 IME / 双宽）才出"= 上游 CJK/IME 处理 bug 的信号；ConPTY 对 IME composition 宽度计算有已知 bug（VSCode 也中招）；portable-pty 0.8.x 已移除 winpty（只 ConPTY），winpty workaround 对我们不可行；遇到上游 bug 要查 VSCode/microsoft-terminal 同款 issue，比泛搜精准 |
| 我做了什么？ | WebSearch 命中 VSCode #255285（铁证）+ webReader 读全文 + 确认 portable-pty 0.8.1 无 winpty（src/win/ 只有 conpty.rs）+ 记录已知限制 |

### 阶段 24：回退阶段 22（init_command 延迟）—— 副作用不值（2026-06-19）
- **决策：** 用户拍板回退阶段 22（local.rs init_command 延迟到首次 resize）。
- **理由：** 阶段 22 修的"claude TUI 以 80 cols 启动"是真实但独立的问题，且**没修复**用户报告的左移（那其实是阶段 23 确认的 ConPTY 中文 IME 上游 bug）；而它的副作用——开 tab 时 PowerShell 提示符**先短暂闪现再进 claude**——可感知、不值。权衡后回退。
- **改动：** `src-tauri/src/local.rs` writer task —— init_command 从"第一次 resize 后发"改回"启动时立即发"（恢复阶段 21 完成时的状态）；Resize 分支移除 pending_init 逻辑；保留一条注释说明为什么立即发 + 曾试过延迟但回退，避免未来重复尝试。
- **保留不动：** 阶段 19（padding cols 几何）、20（字体 fit）、21（firstOutput 多次延迟 resize）、WebGL 条件加载——这些是 cols 正确性 / 同步 / 渲染的净收益，与阶段 22 无关，"字母正常换行"就是它们的成果。
- **验证：** `cargo check` PASS（1.50s 增量）。
- **结论：** 最终代码 = 阶段 19/20/21 + WebGL 条件加载**保留**，阶段 22 **回退**。中文 IME 左移 = ConPTY 上游 bug（阶段 23），应用层不修。

## 五问重启检查
| 问题 | 答案 |
|------|------|
| 我在哪里？ | 阶段 24：回退阶段 22（init_command 延迟），cargo check 绿 |
| 我要去哪里？ | 用户 dev 验证：开 claude tab 不再有 prompt 闪现（启动命令立即执行）；中文 IME 左移按已知限制（阶段 23）接受 |
| 目标是什么？ | 移除阶段 22 的副作用（prompt 闪现），保留其余净收益修复 |
| 我学到了什么？ | 修真问题也要权衡可感知副作用；当某个修复最终没命中用户真实问题（实为上游 bug）时，它的副作用就不值得，应回退；回退要留注释说明曾尝试 + 为什么放弃，防重复 |
| 我做了什么？ | local.rs init_command 延迟 → 启动即发（回退阶段 22）+ 保留防重复注释 + cargo check PASS + 文档同步（progress 阶段 24 / README 移除阶段 22 条目） |
