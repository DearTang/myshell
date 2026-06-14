# MyShell

一款基于 Tauri v2 构建的跨平台 SSH/SFTP/FTP 桌面客户端，灵感来自 FinalShell。

![Version](https://img.shields.io/badge/version-0.2.0-blue)
![Tauri](https://img.shields.io/badge/Tauri-v2-green)
![React](https://img.shields.io/badge/React-18-blue)
![Rust](https://img.shields.io/badge/Rust-1.70+-orange)

## 功能特性

### 连接管理
- 支持 **SSH**、**SFTP**、**FTP** 协议
- 连接分组管理（树形结构）
- 导入/导出连接配置（AES-256-GCM 加密）
- 连接配置持久化存储（SQLite + OS Keyring）
- 代理支持（SOCKS5、HTTP）

### 终端功能
- 基于 xterm.js 的高性能终端模拟
- Catppuccin Mocha 深色主题
- 终端自适应尺寸调整
- **命令历史栏**：最近 50 条命令 + 钉住置顶
- **广播输入**：同时向多个终端发送相同命令
- ZMODEM 协议支持（rz/sz 文件传输）
- 服务器状态实时监控（CPU、内存、磁盘）

### 文件传输
- SFTP/FTP 文件浏览器
- 文件操作：上传、下载、重命名、删除
- 目录导航历史记录

### 安全特性
- 应用启动密码保护
- 登录密码与数据加密密钥（DEK）分离
- PBKDF2-HMAC-SHA256 密钥派生
- AES-256-GCM 数据加密
- 密码错误锁定机制（3 次错误锁定 5 分钟，每日最多 30 次）
- 敏感信息存储于 OS Keyring

### 版本管理
- 自动版本备份（升级前）
- 一键回滚到历史版本
- 最多保留 5 个备份版本

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
| `lockout.json` | 密码错误锁定状态 |
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
