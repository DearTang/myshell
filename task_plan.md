# 任务计划：基于 Rust 的 SSH/SFTP Shell 工具

## 目标
参考 FinalShell，构建一个支持 SSH 终端、SFTP 文件传输、多标签终端、连接管理的 GUI 桌面应用。

## 当前阶段
阶段 1

## 各阶段

### 阶段 1：需求分析与技术选型
- [x] 理解用户意图：参考 FinalShell，Rust 实现，GUI，SSH+SFTP+连接管理，多终端
- [x] 确定技术栈
- [ ] 将发现记录到 findings.md
- **状态：** in_progress

### 阶段 2：环境搭建与项目初始化
- [ ] 安装 Rust 工具链
- [ ] 安装 Node.js 依赖
- [ ] 初始化 Tauri v2 项目
- **状态：** pending

### 阶段 3：核心后端实现 (Rust/Tauri)
- [ ] SSH 连接管理（配置存储 CRUD）
- [ ] SSH 终端会话（russh crate）
- [ ] SFTP 文件操作
- [ ] Tauri commands 暴露给前端
- **状态：** pending

### 阶段 4：前端 UI 实现
- [ ] 主布局（侧边栏 + 工作区）
- [ ] 连接管理界面
- [ ] 多标签终端（xterm.js）
- [ ] SFTP 文件浏览器面板
- **状态：** pending

### 阶段 5：集成测试与交付
- [ ] 验证 SSH 连接与终端交互
- [ ] 验证 SFTP 上传/下载
- [ ] 验证多终端切换
- [ ] 交付给用户
- **状态：** pending

## 关键问题
1. Windows 上 Rust 工具链未安装，需要先安装
2. russh 在 Windows 上的兼容性需要验证

## 已做决策
| 决策 | 理由 |
|------|------|
| 使用 Tauri v2 做 GUI 框架 | 轻量、原生性能、Rust 原生、跨平台 |
| 使用 russh 做 SSH 后端 | 纯 Rust 实现，异步支持，活跃维护 |
| 使用 tokio 做异步运行时 | russh 依赖 tokio，生态成熟 |
| 使用 xterm.js 做终端渲染 | 成熟的 Web 终端模拟器，广泛使用 |
| 使用 SQLite (rusqlite) 存储连接配置 | 轻量本地存储，无需外部数据库 |
| 前端使用 React + TypeScript | 生态成熟，Tauri 官方模板支持 |

## 遇到的错误
| 错误 | 尝试次数 | 解决方案 |
|------|---------|---------|
| Rust 未安装 | 1 | 需要安装 rustup |

## 备注
- 随着进度更新阶段状态：pending → in_progress → complete
- 做重大决策前重新读取此计划
- 记录所有错误，避免重复
