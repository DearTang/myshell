# 发现与决策

## 需求
- 参考 FinalShell 的 SSH 客户端功能
- 基于 Rust 实现
- 支持 SSH 终端连接
- 支持 SFTP 文件传输
- 支持连接管理（保存/编辑/删除主机配置）
- 支持多终端同时编辑（多标签）
- GUI 图形界面

## 研究发现
- FinalShell 特性：SSH终端、SFTP文件管理、多标签、连接管理器、服务器监控
- russh：纯 Rust SSH2 实现，异步（tokio），支持会话/通道/SFTP子系统
- Tauri v2：Rust 原生桌面框架，支持前端渲染，IPC 通信

## 技术决策
| 决策 | 理由 |
|------|------|
| Tauri v2 | 轻量、原生性能、Rust 后端直接调用 SSH 库 |
| russh | 纯 Rust、异步、活跃维护、支持 SFTP 子系统 |
| tokio | russh 依赖，生态成熟 |
| xterm.js | Web 终端标准，Unicode 支持好 |
| SQLite (rusqlite) | 本地连接配置存储，轻量无依赖 |
| React + TypeScript | Tauri 官方支持，类型安全 |

## 遇到的问题
| 问题 | 解决方案 |
|------|---------|
| Windows 上 Rust 未安装 | 需要安装 rustup-init |

## 资源
- russh: https://github.com/Eugeny/russh
- Tauri v2: https://tauri.app/
- xterm.js: https://xtermjs.org/

---
*每执行2次查看/浏览器/搜索操作后更新此文件*
*防止视觉信息丢失*
