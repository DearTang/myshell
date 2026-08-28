# 快捷命令多行执行：行间延迟优化

## 问题
`CommandBar.tsx` 的 `handleExecuteQuickCommand` 把所有命令行 `.join("\r")` 后**一次性**发给 PTY。当某行触发交互提示（`sudo`/`mysql -p`/`ssh` 密码），下一行会过早到达，被当成对该提示的回答或当作命令，导致密码输入失败。

## 方案（纯前端，零 Rust/DB 改动）
两种互补机制：

1. **行内延迟指令** `##delay:<N>`：命令文本里单独占一行，在该处插入指定延迟。
   - `##delay:500` → 500 毫秒
   - `##delay:1s` / `##delay:0.5s` → 秒（自动换算）
   - `##pause:N` 作为同义别名
2. **全局默认行间延迟**：localStorage `myshell-quick-command-line-delay-ms`，默认 0（关闭=旧行为，向后兼容）。在设置 → 快捷命令里提供预设下拉。

**语义**：某两行之间若有显式 `##delay:` 指令，则该间隔用显式值（**不叠加**默认值）；若没有显式指令且默认值 > 0，则用默认值。这样用户"标了的地方精确等，没标的地方有兜底"，不重复计时。

## 改动文件

### 1. `src/components/CommandBar.tsx`（核心）
重写 `handleExecuteQuickCommand`（152–165 行）：从"拼接一次性发送"改为"解析为步骤序列 + 循环逐行发送，间隔插入延迟"。
- 新增 `parseQuickCommand(command)`：逐行解析为 `{type:'cmd',text}` | `{type:'delay',ms}`，跳过空行和 `#` 注释（先识别 `##delay`/`##pause` 再判注释）。
- 新增 `sleep(ms)` 与 `readQuickCmdLineDelayMs()`（读 localStorage，默认 0）。
- 执行循环：对每个 cmd 广播到所有目标（同现状）；cmd 之间的前置延迟按上面语义计算（显式指令优先，否则用默认值）。
- 面板立即关闭（保持响应感）；中途断连由 `Promise.allSettled` 吞掉。
- 单行输入框执行 `handleExecute`（93–111 行）不受影响。

### 2. `src/components/SettingsPanel.tsx`（全局设置 UI）
在 `quickCommands` 分类（2901–2939 行）现有 Section 后新增一个 `<Section title="行间延迟">`，放一个 `<select>` 预设（0关闭 / 100 / 300 / 500 / 1000 / 2000 ms），读写 `myshell-quick-command-line-delay-ms`，复用 `myshell-auto-lock-minutes` 的写法。附说明文字介绍 `##delay:N` 指令用法。

### 3. `src/components/QuickCommandsPanel.tsx`（编辑器提示）
更新 textarea 的 label（505 行）和 placeholder（511 行），说明 `##delay:N` 用法，placeholder 增加含延迟的示例（如 `sudo` 命令后 `##delay:800` 再输密码）。

## 文档收尾（按 AGENTS.md doc-after-feature 规则，实现完成后）
- `progress.md` 追加阶段条目 + 五问重启检查
- `README.md` 同步功能特性/更新日志
- `RELEASE_NOTES_STAGING.md` 追加：`- 🛠️优化 快捷命令多行执行支持行间延迟（##delay:N 语法 + 全局默认延迟），避免交互提示/密码输入被下一条命令冲掉`

## 验证
- `npx tsc --noEmit` 通过
- 手动：建一条 `mysql -u root -p` + `##delay:800` + 密码 的快捷命令，确认延迟生效、密码正确进入提示