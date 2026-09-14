# MyShell 前端迁移约定（React → Vue 3 + myui 0.7.0）

> 本文档是所有迁移组件的统一契约。移植组件前先读完本文，再读你负责的 React 源文件（在 `src-legacy/` 下）。

## 1. 背景

- MyShell 是 Tauri v2 桌面 SSH/SFTP 客户端。后端（Rust）与 IPC 契约**零改动**：`src/api.ts`（invoke/listen 封装 + 全部类型）原样保留，直接 `import { xxx } from "@/api"`。
- 前端从 React 18 迁移到 Vue 3 + Element Plus + myui 0.7.0（Gitee git 依赖，**禁止修改/绕过 myui**）。
- 参考实现：`F:\personalProject\MyUI\src\views\cases\unified-ui`（布局与交互参照，MyShell 自身结构优先）。

## 2. 技术约定

- **语言**：`<script setup lang="ts">`，strict 模式，`vue-tsc --noEmit` 必须零错误。禁止 `as any`。
- **文案**：界面文字一律简体中文（硬编码，与旧版一致；不走 i18n）。
- **样式**：scoped CSS + 令牌。可用令牌见 `src/styles/app.css`（MyShell 令牌 `--bg-*`/`--text-*`/`--accent-*`/`--success/warning/error/info`/`--border-*`/`--radius-*`/`--duration-*` 等）+ myui 令牌（`--accent`/`--surface-translucent` 等，已被桥接到 MyShell 令牌）。**禁止硬编码色值/字号**。
- **图标**：优先直接 `import { Xxx } from "@element-plus/icons-vue"` 渲染 `<el-icon><Xxx /></el-icon>`；字符串动态名字才用 `<MyIcon name="Xxx" />`（需已在 `src/icons/register.ts` 注册）。连接类型图标沿用旧 iconfont 类（`iconfont icon-ssh` 等，见 `src/assets/iconfont/`）。
- **通用确认**：简单确认框用 myui 的 `confirmDialog({ message, title, type })`（返回 `Promise<boolean>`）；提示用 `toast(msg, { type })`。禁止 `window.confirm/alert`（Tauri webview 会被吞，旧代码里的 alert 保留原样即可）。
- **EP 组件**：`unplugin-vue-components + ElementPlusResolver` 已配置，`el-*` 组件模板里直接用，无需 import。

## 3. myui 0.7.0 组件清单（`import { MyButton, ... } from "myui"`）

MyButton(variant: primary/secondary/ghost/danger, size, loading, disabled) · MyInput(label, required, error, placeholder, v-model) · MyTextArea · MySelect(options: {label,value}[], filterable, clearable, multiple, label, required, error, v-model) · MyNumberField · MyToggle(v-model) · MySlider · MyCheckbox · MyRadioGroup · MyDateRangeField · MyDialog(modelValue, title, width, danger, confirmText, cancelText, confirmLoading, manualClose; slots: default/footer; emits confirm/cancel) · MyDrawer · MyPanel · MySection · MyFieldRow/MyFieldShell · MyTabs/MySegmented · MyTree(data, nodeKey, props, defaultExpandAll, draggable, showFilter, highlightCurrent; emits node-click/node-drag-start/node-drag-end/node-drop; empty slot) · MyBadge/MyStatusTag/MyStatusDot(tone, pulse) · MyResultState · MySpinner/MyProgress · MyIcon/MyIconButton · MyTag · MyDescriptions · MyStatCard · MySignalTrack · MyPageHeader/MyFilterBar · MyDataTable · MyCommandPalette · MyNotificationBell · MyMenuTreeEditor · MyRow/MyCol(栅格) · confirmDialog/toast/notify · registerIcon/resolveIcon。

API 形态：全部受控式（`v-model` / `update:modelValue`）。**myui 满足不了的 UI 原语：本地用 scoped CSS + EP 组件实现（允许），并在你的报告里明确列出「缺失组件 + 需要的 API 形状」，不要硬凑。**

## 4. 共享状态（`src/store/`，响应式单例，直接 import 使用）

```ts
// store/ui.ts —— 壳层与全局对话框
ui: reactive<{ themeChoice, isDark, sidebarCollapsed, sidebarWidth, showAiPanel, aiPanelWidth,
  multiWindowMode, multiWindowIds, showMultiWindowPicker, appVersion, showSettings,
  showQuickCommands, qcInitialConnectionId, showFeedback, showRecycle,
  about: {open, mode}, statsPrompt, mwOverflowPrompt, mcpConfirm }>
connectionDialog: reactive<{ open, editConfig, initialConnType, initialFolderPath }>
openConnectionDialog({ editConfig?, initialConnType?, initialFolderPath? }) / closeConnectionDialog()
setThemeChoice("dark"|"light") / applyTheme()
resolveMcpConfirm(ok) / showMcpConfirm(command, connectionName, reasons)  // MCP 桥专用

// store/connections.ts
connectionsStore: reactive<{ connections: ConnectionConfig[], folders: string[], loaded }>
reloadConnections() / findConnection(id)

// store/sessions.ts —— 标签编排与连接生命周期
sessions: reactive<{ tabs: Tab[], activeTabId, broadcastIds: Set<string> }>
connect(config) / closeTab(tabId) / reconnect(tabId) / reconnectOne(tabId) / reconnectAll()
resetHostKeyAndReconnect(tabId) / closeDisconnected() / setActiveTab(tabId)
toggleBroadcast(tabId) / getBroadcastTargets(tab) / broadcastAll() / exitAllBroadcast()
broadcastDup.prompt（BroadcastDup 对话框数据）/ registerTerminal / unregisterTerminal / getTerminal
enterMultiWindow(ids) / exitMultiWindow()

// store/appearance.ts —— 配色预设（Settings 外观区用）
appearance: reactive<{ paletteId, customPalette, bgImage }>
setPaletteId / setCustomPalette / clearCustomPalette / setBgImage / getActivePalette / applyColorPreset

// composables/
useVault.ts: vault(ref: "checking"|"setup"|"unlock"|"ready"), refreshVaultStatus()
useUpdateCheck.ts: updateInfo(ref), updateChecking(ref), checkNow(), autoCheckUpdates()
useRendererPref.ts: rendererBackend(ref), setRendererBackend, gpuDisabled(ref), setGpuDisabled, readGpuDisabled, resolveRenderer(pref, hasBgImage)
useTerminalFont.ts: primaryFont(ref), setPrimaryFont, resolvedFontFamily(), resolveFontStack(primary)
useMcpBridge.ts: startMcpBridge()   // App.vue 已启动，勿重复
```

`Tab` / `ConnectionConfig` / `FileEntry` 等类型全部来自 `@/api`（原样保留的旧文件，类型名不变）。

## 5. 移植规则（重要）

1. **行为语义零变化**：localStorage key、IPC command 名、事件名、中文文案、安全相关文本（MCP 拒绝文案等）逐字保留。
2. **内联样式 → scoped CSS + 令牌**；React state → `ref`/`reactive`/`computed`；`useEffect` → `watch`/`onMounted`；`useRef` DOM → `ref<HTMLxxx>`。React `key` → `v-for :key`。
3. **xterm 尺寸不变量**：终端容器用绝对定位堆叠（App.vue 已处理，你只负责组件内部）；`ResizeObserver`/Fit 逻辑照旧。
4. **旧源码里的注释约束**（并发、时序、为什么不能改）全部保留到新代码。
5. 对话框类组件：根元素用 `<MyDialog>`/`<el-dialog>` 或本地遮罩皆可；表单类必须防误关（`close-on-click-modal=false` / myui 无此 prop 时本地遮罩实现）。
6. **不改共享文件**（App.vue / store/* / composables/* / main.ts / styles/app.css / i18n / icons）。确需改动 → 在最终报告里列出精确 diff 需求。
7. 完成后跑 `npx vue-tsc --noEmit` 确认你新增的文件不引入类型错误（其他组件缺失导致的报错可忽略，报告即可）。
8. 组件文件的 `defineOptions({ name })` 用 PascalCase 组件名。

## 6. 目标文件位置

全部新建在 `src/components/` 下（Vue SFC）。React 原文在 `src-legacy/components/`。
