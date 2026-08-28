# 新功能：启动时检测已有实例 → 弹窗选择「重启 / 退出」

## 需求

启动 MyShell 时若检测到已有实例在运行，弹出对话框：
- **重启**：结束旧实例（优先优雅退出，兜底强杀），新实例继续启动
- **退出**：保持现有实例不动，本次启动的进程直接退出

## 方案设计

### 实例检测：Windows named mutex（`CreateMutexW`）

- 项目无任何 single-instance 机制；`gui-ipc-port` 文件有 stale 风险（崩溃时不删）
- named mutex 由 OS 管理，**进程崩溃自动释放**，零 stale 问题，是最标准的方案
- 跟随项目惯例（`confirm_dangerous_operation` 的 MessageBoxW 在非 Windows 是 no-op stub）：**完整功能 Windows-only，非 Windows 直接放行**

### 弹窗：Rust 侧 `MessageBoxW`（照抄 `myshell-mcp.rs:29-59` 模式）

- 检测发生在 `run()` 最开头、窗口/webview 创建**之前**，React 弹窗不可用
- 窗口本身 `visible: false` + dom-ready 才显示，系统弹窗是最自然的早期 UI
- `MB_YESNO | MB_ICONQUESTION | MB_SYSTEMMODAL`：是=重启，否=退出
- 文案：`检测到 MyShell 已在运行。\n\n[是] 重启 - 结束当前实例并启动新实例\n[否] 退出 - 保持现有实例，取消本次启动`

### "重启"路径：优雅退出 → 兜底强杀 → 等 mutex 空闲

1. **优雅退出（首选）**：读 `gui-ipc-port` 文件 → 连 localhost IPC bridge → 发新增的 `shutdown` action → 旧实例收到后 `app.exit(0)`，走现有 `RunEvent::ExitRequested` 清理（`drain_all_sessions` + 删 port file，本地 PTY 子进程被正确收尾）。最多等 5s
2. **兜底强杀**：mutex 仍被占 → `CreateToolhelp32Snapshot` 枚举进程，找 `myshell.exe` 且 PID ≠ 自己 → `OpenProcess(PROCESS_TERMINATE)` + `TerminateProcess`。最多等 5s
3. 都失败 → 报错弹窗（无法结束旧实例）→ 退出
4. 旧实例死透后 mutex 空闲，新实例 acquire 成功 → 继续正常 builder 流程

## 文件改动

### 1. `src-tauri/Cargo.toml`
winapi features 增加：`synchapi`（CreateMutexW/WaitForSingleObject）、`tlhelp32`（进程枚举）、`errhandlingapi`（GetLastError）

### 2. `src-tauri/src/main.rs`

**新增 `single_instance` 模块级函数组（约 150-180 行）：**

```rust
/// Windows: 创建全局 named mutex。返回 Ok(true)=首次持有（继续启动），
/// Ok(false)=已有实例（已处理完弹窗+用户选择"退出"），Err=等待旧实例死亡超时
fn acquire_single_instance_lock() -> Result<bool, String>
```

- `CreateMutexW(null, false, "Global\\MyShellSingleInstance")`（Global\ 前缀跨 session）
- `GetLastError() == ERROR_ALREADY_EXISTS` → 进入"已有实例"分支
- mutex handle 故意 leak（进程生命周期内保持）

**已有实例分支：**
- 弹 MessageBoxW（复用 mcp.rs 的宽字符串构造写法）
- 用户选"否/退出" → return false（进程退出）
- 用户选"是/重启" → `shutdown_existing_instance()`：
  - 读 port file → TCP 连接 → 发 `{"action":"shutdown"}` JSON → 旧实例 handler 里 `app.exit(0)`
  - 轮询 `WaitForSingleObject(mutex, 500ms)` 直到 `WAIT_OBJECT_0`，共 5s
  - 超时 → `kill_other_myshell_processes()`（Toolhelp32 枚举 + TerminateProcess）再等 5s
  - 仍失败 → Err

**`run()` 开头（line ~3938 之前）插入：**
```rust
if !acquire_single_instance_lock()? { return; }
```

**IPC bridge（line 4120-4535 区域）加 `shutdown` action handler（约 15 行）：**
- 收到后对 app handle 调 `app.exit(0)`（复用现有 handler 拿 app handle 的模式）

### 3. 不改前端

检测和弹窗全部发生在 webview 创建前，纯 Rust。

## 已知限制（写入代码注释）

- 非 Windows 平台：no-op（跟 `confirm_dangerous_operation` 惯例一致）
- 兜底强杀路径跳过旧实例的 `ExitRequested` 清理——本地 PTY 的 shell 子进程可能残留（影响极小）；正常路径（IPC 优雅退出）不受影响
- mutex 用 `Global\` 前缀：同机多用户/多 session 也互斥（符合单实例语义）

## 验证

1. `cargo check`（src-tauri/）通过
2. `npx tsc --noEmit` 通过（无前端改动，预期无影响）
3. 手动验收（用户侧）：
   - 开 MyShell → 再启动一个 → 弹窗 → 选"退出" → 新进程退出、旧实例还在
   - 再启动 → 选"重启" → 旧实例消失、新实例正常起来
   - 任务管理器强杀 myshell.exe 后立刻重启 → 不弹窗（mutex 已释放）

## 收尾（doc-after-feature）

- `progress.md` 追加阶段 105（含五问重启检查）
- `RELEASE_NOTES_STAGING.md` 追加 ✨新增 条目（有新增 → 下次打包为 minor bump）
- README 功能特性区补一条