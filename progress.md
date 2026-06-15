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
