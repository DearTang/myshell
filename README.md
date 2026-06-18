# MyShell

一款基于 Tauri v2 构建的跨平台 SSH/SFTP/FTP 桌面客户端，灵感来自 FinalShell。

<p align="center">
  <img src="src-tauri/icons/icon.png" width="120" alt="MyShell" />
</p>

![Version](https://img.shields.io/badge/version-1.0.0-blue)
![Tauri](https://img.shields.io/badge/Tauri-v2-green)
![React](https://img.shields.io/badge/React-18-blue)
![Rust](https://img.shields.io/badge/Rust-1.70+-orange)

## 功能特性

### 连接管理
- 支持 **SSH**、**SFTP**、**FTP** 协议
- 连接分组管理（树形结构）
- **模糊搜索**：快速定位服务器连接
- **文件夹快速选择**：编辑服务器时可快速选择所属文件夹
- 导入/导出连接配置（AES-256-GCM 加密）
- 连接配置持久化存储（SQLite + OS Keyring）
- 代理支持（SOCKS5、HTTP）

### 终端功能
- 基于 xterm.js 的高性能终端模拟
- Catppuccin Mocha 深色主题
- **暗黑模式**：支持深色/浅色主题切换
- 终端自适应尺寸调整
- **连接状态指示**：tab 页顶部显示在线（绿色）/离线（红色）/异常（红色×）状态
- **连接失败重连**：tab 页右下角显示失败提示，支持一键重连
- **命令历史栏**：最近 50 条命令 + 钉住置顶
- **快捷命令**：全局快捷命令 + 服务器专属快捷命令，多行命令按行顺序一键执行（`#` 注释与空行自动跳过），支持广播到多终端
- **广播输入**：同时向多个终端发送相同命令
- ZMODEM 协议支持（rz/sz 文件传输）
- 服务器状态实时监控（CPU、内存、磁盘）
- **本地终端**：直连本地 PowerShell / CMD / WSL / 自定义 shell，作为可保存的连接，体验等同 SSH 终端
- **终端字体**：从系统已安装字体下拉选择（也可手输），默认字体栈 Nerd Font 优先，正确渲染 Powerline / 图标字形（Oh My Posh / Starship / powerlevel10k 等）；支持**按连接单独覆盖字体**
- **本地终端编码自适应**：cmd / Windows PowerShell 5.1 启动自动切 UTF-8，中文系统不再 GBK 乱码；并向 shell 声明 `TERM=xterm-256color` / `COLORTERM=truecolor`，提示符引擎完整渲染

### 文件传输
- SFTP/FTP 文件浏览器
- 文件操作：上传、下载、重命名、删除
- 目录导航历史记录

### 诊断与日志
- **结构化日志**：关键路径（启动、SSH 生命周期、连接管理、快捷命令）记录 `info` / `warn` / `error` 级日志，按天落盘，便于事后定位问题
- **日志自动清理**：默认保留 **7 天**，每次启动自动清理超期日志
- **动态调试**：需要更详细输出时设置环境变量 `RUST_LOG=myshell=debug` 提升日志级别

### 安全特性
- 应用启动密码保护
- 登录密码与数据加密密钥（DEK）分离
- PBKDF2-HMAC-SHA256 密钥派生（600k 迭代，OWASP 2023 推荐；旧 vault 首次解锁时透明迁移）
- AES-256-GCM 数据加密
- SSH 主机键 TOFU 验证（按 `(host, port)` 隔离，指纹变更自动拒绝）
- 严格 CSP 白名单（关闭 XSS 暴露面）
- OSC 52 剪贴板劫持防护
- 命令历史敏感命令过滤（passwd/sudo/密钥参数等不入库）
- Tauri 命令加固：PEM 头校验、文件大小上限、路径校验、错误脱敏
- ZMODEM 写入路径保护（拒绝软链与系统目录）
- 密码错误锁定机制（3 次错误锁定 5 分钟，每日最多 30 次）
- 敏感信息存储于 OS Keyring
- **安装器数据删除二次确认**：卸载/更新时勾选「删除应用数据」会弹危险提示并要求二次确认（默认「否」），明确告知将删除连接 / 密码 / 历史 / 密钥库且不可恢复，防止误删

### 版本管理
- 自动版本备份（升级前）
- 一键回滚到历史版本
- 最多保留 5 个备份版本

## 更新日志

### v1.0.0

首个正式发布版本。

#### ✨ 新增
- **快捷命令**：支持「全局快捷命令」+「服务器专属快捷命令」两级管理，多行命令按行顺序一键执行；行首 `#` 注释与空行自动跳过；支持广播到多个终端同时执行
- **结构化诊断日志**：启动、SSH 生命周期、连接管理、快捷命令等关键路径落盘 `info`/`warn`/`error` 日志；按天滚动，默认保留 **7 天**，启动时自动清理超期日志；支持 `RUST_LOG=myshell=debug` 动态提级
- **全新应用图标**：Aurora Prompt `>_`，OLED 暗底 + 极光辉光 + 靛蓝→青色品牌渐变，与应用视觉语言统一（源文件 `src-tauri/icons/source-a.svg`，可一键重新生成全套尺寸）

#### 🎨 优化
- 连接管理侧栏交互重设计：新建按钮统一为渐变主操作、展开/收起改为圆形「边缝手柄」搭配旋转指示箭头
- 暗黑/亮色主题、终端在线/离线/异常状态指示、连接失败一键重连、连接模糊搜索、文件夹快速选择等体验打磨

#### 🔒 安全加固（延续 0.x）
- PBKDF2-HMAC-SHA256（600k 迭代）密钥派生、AES-256-GCM 数据加密、SSH 主机键 TOFU、CSP 白名单、OSC 52 防护、密码错误锁定等

---

### v1.2.x

本地终端与体验增强。

#### ✨ 新增
- **本地终端**：直连本地 PowerShell / CMD / WSL / 自定义 shell，作为可保存的连接，体验等同 SSH 终端；支持「启动命令」开 tab 自动执行
- **终端字体选择**：从系统已安装字体下拉选择（非手输），默认字体栈 Nerd Font 优先渲染 Powerline / 图标字形；支持按连接单独覆盖字体
- **安装器数据删除二次确认**：卸载/更新勾选「删除应用数据」时弹危险提示 + 二次确认（默认「否」），并改用直白文案，防误删全部连接 / 密码 / 历史 / 密钥库
- **以管理员运行**：设置面板可一键以管理员重启 MyShell（Windows UAC 授权），重启后所有本地终端获得管理员权限；自动检测并展示当前权限状态。本地终端继承 MyShell 权限，需管理员命令时整体提权重启即可（单连接提权受 ConPTY + UAC 完整性级别隔离限制）

#### 🛠️ 优化
- 本地终端编码自适应：cmd / Windows PowerShell 5.1 启动自动切 UTF-8（中文系统不再 GBK 乱码），并向 shell 声明 `TERM` / `COLORTERM` 让提示符引擎完整渲染
- **对话框防误关**：设置 / 快捷命令 / 主密码 / 密码验证等弹窗，点击遮罩区域（操作框外）不再意外关闭，必须用关闭/取消按钮，避免误触丢失已填内容（行为与新建连接对话框一致）
- **字体选择升级**：终端字体改为模糊搜索下拉——输入关键字（如 `nerd mono`）即时过滤、匹配片段高亮，自定义主题化下拉面板替换原生 datalist；支持 `↑↓` / 回车 / Esc 键盘导航，仍可手动输入任意字体名

### v1.3.x

体验打磨与工程整理。

#### 🛠️ 优化
- **历史命令过滤**：仅由 a/d 字符组成的误触命令（如 "A"、"AD"）不再进入历史命令
- **新建连接保存按钮悬浮**：底部「取消/保存」常驻可见，长表单无需滚到底即可保存
- **版本号单一源**：只需改 `Cargo.toml` 一处版本号，`tauri.conf.json` / `package.json` 自动同步（build 时）
- **图标重生成**：用矢量源重新生成全平台图标，确保 exe / 安装器 / 各尺寸高清一致

---

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端框架 | React 18 + TypeScript |
| 桌面框架 | Tauri v2 |
| 终端模拟 | xterm.js + FitAddon + WebLinksAddon |
| 后端语言 | Rust |
| SSH 协议 | russh + russh-sftp |
| FTP 协议 | suppaftp |
| 数据库 | SQLite (rusqlite) |
| 加密 | AES-256-GCM + PBKDF2 |
| 密钥存储 | OS Keyring (keyring-rs) |

## 项目结构

```
myShell/
├── src/                      # React 前端
│   ├── components/           # UI 组件
│   │   ├── App.tsx           # 主应用入口
│   │   ├── Sidebar.tsx       # 连接管理侧边栏
│   │   ├── TerminalPanel.tsx # 终端面板
│   │   ├── SftpPanel.tsx     # SFTP 文件浏览器
│   │   ├── CommandBar.tsx    # 命令历史栏
│   │   ├── SettingsPanel.tsx # 设置面板
│   │   └── ...
│   ├── utils/                # 工具函数
│   ├── api.ts                # Tauri 命令封装
│   └── main.tsx              # 入口文件
├── src-tauri/                # Rust 后端
│   ├── src/
│   │   ├── main.rs           # Tauri 命令定义
│   │   ├── ssh.rs            # SSH 连接管理
│   │   ├── sftp.rs           # SFTP 文件操作
│   │   ├── ftp.rs            # FTP 协议实现
│   │   ├── db.rs             # SQLite 数据库
│   │   ├── vault.rs          # 加密与密钥管理
│   │   ├── crypto.rs         # 加密工具
│   │   ├── backup.rs         # 版本备份
│   │   └── ...
│   ├── Cargo.toml            # Rust 依赖
│   └── tauri.conf.json       # Tauri 配置
└── scripts/                  # 构建脚本
```

## 快速开始

### 环境要求

- Node.js >= 18
- Rust >= 1.70
- pnpm/npm/yarn

### 安装依赖

```bash
# 安装前端依赖
npm install

# Rust 依赖会在首次构建时自动安装
```

### 开发模式

```bash
# 启动开发服务器（热重载）
npm run tauri:dev

# 或
cd src-tauri && cargo tauri dev
```

### 构建生产版本

```bash
# 构建桌面应用
npm run tauri:build

# 输出位置：src-tauri/target/release/bundle/
```

### 类型检查

```bash
# TypeScript 类型检查
npm run test:ts

# Rust 编译检查
npm run test:rust

# 同时检查
npm run test:compile
```

## 配置文件

应用数据存储位置：

| 平台 | 路径 |
|------|------|
| Windows | `%APPDATA%\myshell\` |
| macOS | `~/Library/Application Support/myshell/` |
| Linux | `~/.config/myshell/` |

配置文件说明：

| 文件 | 说明 |
|------|------|
| `connections.db` | 连接配置数据库（加密） |
| `vault.salt` | 密钥派生盐值 |
| `vault.verifier` | 密码验证数据 |
| `dek.enc` | 加密的 DEK（数据加密密钥） |
| `vault.kdf` | KDF 算法与迭代数元数据（旧版本缺失，首次解锁自动生成） |
| `lockout.json` | 密码错误锁定状态 |
| `logs/` | 按天滚动的运行日志（`myshell-YYYY-MM-DD.log`，默认保留 7 天） |
| `backups/` | 版本备份目录 |

## 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Ctrl+Shift+T` | 新建终端标签 |
| `Ctrl+W` | 关闭当前标签 |
| `Ctrl+Tab` | 切换到下一个标签 |
| `Ctrl+Shift+Tab` | 切换到上一个标签 |

## 开发说明

### 添加新的 Tauri 命令

需要同步修改三个位置：

1. `src-tauri/src/main.rs` — 定义 `#[tauri::command]` 函数
2. 相关模块（`ssh.rs` / `sftp.rs` / `db.rs`）— 实现逻辑
3. `src/api.ts` — 添加 TypeScript 封装

### 类型定义

共享类型（如 `ConnectionConfig`、`FileEntry`）需要在 Rust 和 TypeScript 两边定义并保持同步。

## 许可证

MIT License

## 致谢

- [Tauri](https://tauri.app/) - 跨平台桌面应用框架
- [russh](https://github.com/warp-tech/russh) - Rust SSH 库
- [xterm.js](https://xtermjs.org/) - 终端模拟器
- [FinalShell](http://www.hostbuf.com/) - 设计灵感来源
