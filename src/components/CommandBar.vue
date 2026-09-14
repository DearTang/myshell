<!-- 命令栏：命令输入 + 历史命令浮层 + 快捷命令浮层 + 多窗口/AI/截图/重连入口。
     从 src-legacy/components/CommandBar.tsx 逐行移植；
     由 TerminalPanel 嵌（每 tab 一个实例），悬浮面板绝对定位于本栏上方，
     互斥展开（与旧版点击置位逻辑一致）。 -->
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import {
  Camera,
  ChatDotRound,
  Clock,
  Grid,
  Lightning,
  RefreshRight,
  Top,
} from "@element-plus/icons-vue";
import {
  addCommandHistory,
  clearCommandHistory,
  deleteCommandHistory,
  listCommandHistory,
  listQuickCommandsForConnection,
  localSend,
  onSshOutput,
  setCommandHistoryPinned,
  sshSend,
} from "@/api";
import type { CommandHistoryItem, ConnType, QuickCommandExecItem, Tab } from "@/api";

defineOptions({ name: "CommandBar" });

const props = withDefaults(
  defineProps<{
    sessionId: string;
    connectionId: string;
    /** Tab 连接类型 —— 决定走 ssh_send 还是 local_send。默认 ssh。 */
    connType?: ConnType;
    /** 广播目标 sessionId 列表。非空时输入框发出的命令发给所有目标
     * （含本会话）。 */
    broadcastTargets?: string[];
    /** 连接状态："connecting" | "connected" | "disconnected" | "error" */
    status?: Tab["status"];
  }>(),
  { broadcastTargets: () => [] },
);

const emit = defineEmits<{
  /** 把本栏的历史 reload 注册给父组件（TerminalPanel 的 onData 录入新命令后
   * 触发刷新）。等价于旧版 onRegisterRefresh 回调。 */
  (e: "register-refresh", fn: () => void): void;
  (e: "reconnect"): void;
  (e: "open-quick-commands-manage"): void;
  (e: "open-ai"): void;
  (e: "open-multiwindow"): void;
  /** 截取终端视口 PNG（不含本栏）。保存到设置的附件目录。 */
  (e: "screenshot"): void;
}>();

// ============ Quick command parsing & inter-line delay ============

/** localStorage keys for inter-line delay between multi-line quick commands. */
const QUICK_CMD_LINE_DELAY_KEY = "myshell-quick-command-line-delay-ms";
const QUICK_CMD_MODE_KEY = "myshell-quick-command-mode";

/** How lines of a multi-line quick command are spaced when sent to the PTY.
 *  - `off`:   send all lines at once (legacy behaviour).
 *  - `fixed`: wait a fixed number of ms between consecutive lines.
 *  - `idle`:  wait until the previous line's output has gone quiet for `ms`
 *             before sending the next line. Handles interactive prompts
 *             (sudo/mysql password) that a fixed delay or shell sentinel
 *             cannot — when output stops, either the shell prompt is back or
 *             the program is waiting for input. */
type QuickCmdDelayMode = "off" | "fixed" | "idle";

/** Read the configured inter-line delay mode + duration. `ms` is the fixed
 *  delay (mode=`fixed`) or the output-quiescence window (mode=`idle`). */
function readQuickCmdDelayConfig(): { mode: QuickCmdDelayMode; ms: number } {
  const msRaw = Number(localStorage.getItem(QUICK_CMD_LINE_DELAY_KEY));
  const ms = Number.isFinite(msRaw) && msRaw > 0 ? Math.min(msRaw, 60_000) : 0;
  const stored = localStorage.getItem(QUICK_CMD_MODE_KEY) as QuickCmdDelayMode | null;
  const mode: QuickCmdDelayMode =
    stored === "off" || stored === "fixed" || stored === "idle"
      ? stored
      : // Migration: before the mode key existed, a non-zero delay-ms meant
        // fixed delay. Anything else (unset / 0) = off.
        ms > 0
        ? "fixed"
        : "off";
  return { mode, ms };
}

/** Promise-based sleep. */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

type QuickCmdStep =
  | { type: "cmd"; text: string }
  | { type: "delay"; ms: number };

/** Parse a quick command's raw text into an ordered list of steps.
 *
 *  - Empty lines and `#`-comment lines are dropped.
 *  - A line `##delay:<N>` / `##pause:<N>` inserts a delay of <N> ms. `<N>`
 *    may be suffixed with `s` for seconds (e.g. `##delay:1s`, `##delay:0.5s`).
 *    These directive lines are NOT sent to the shell — they only gate timing
 *    between the surrounding command lines (useful before a password prompt).
 *  - Everything else is a command line, sent verbatim with a trailing CR. */
function parseQuickCommand(command: string): QuickCmdStep[] {
  const steps: QuickCmdStep[] = [];
  for (const raw of command.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.length === 0) continue;
    const m = line.match(/^##(?:delay|pause):\s*(\d+(?:\.\d+)?)\s*(ms|s)?\s*$/i);
    if (m) {
      const value = parseFloat(m[1]);
      const unit = (m[2] ?? "ms").toLowerCase();
      const ms = unit === "s" ? Math.round(value * 1000) : Math.round(value);
      steps.push({ type: "delay", ms: Math.max(0, ms) });
      continue;
    }
    if (line.startsWith("#")) continue; // comment
    steps.push({ type: "cmd", text: line });
  }
  return steps;
}

// ============ 组件状态 ============

// 按连接类型选发送后端 —— local tab 必须走 local_send 而非 ssh_send。
const sendFn = (sid: string, data: string): Promise<void> =>
  props.connType === "local" ? localSend(sid, data) : sshSend(sid, data);

const input = ref("");
const history = ref<CommandHistoryItem[]>([]);
const panelOpen = ref(false);
const loading = ref(false);

// 快捷命令浮层状态。列出全局 + 本连接专属命令的并集，在浮层里按作用域分组。
const quickPanelOpen = ref(false);
const quickCommands = ref<QuickCommandExecItem[]>([]);
const quickLoading = ref(false);

// 根容器 ref：把 DOM 查询（如选中历史项后聚焦输入框）限定在本 CommandBar
// 内 —— 全局 querySelector 会命中第一个渲染的 tab 的输入栏，把焦点劫持到
// 错误的 tab。
const containerRef = ref<HTMLDivElement | null>(null);
// 窄幅模式：栏太窄（多窗口格子、小窗口）时隐藏按钮文字，防止输入框被挤扁。
const showLabels = ref(true);
let labelResizeObserver: ResizeObserver | null = null;

onMounted(() => {
  const el = containerRef.value;
  if (!el) return;
  labelResizeObserver = new ResizeObserver(() => {
    showLabels.value = el.clientWidth > 520;
  });
  labelResizeObserver.observe(el);
  showLabels.value = el.clientWidth > 520;
});

onUnmounted(() => {
  labelResizeObserver?.disconnect();
  labelResizeObserver = null;
});

async function reload(): Promise<void> {
  if (!props.connectionId) return;
  loading.value = true;
  try {
    const items = await listCommandHistory(props.connectionId);
    history.value = items;
  } catch {
    // Silently ignore — not critical.
  } finally {
    loading.value = false;
  }
}

// 挂载时加载一次并注册刷新回调（旧版 effect [reload, onRegisterRefresh]）。
onMounted(() => {
  void reload();
  emit("register-refresh", reload);
});

// connectionId 变化时旧版会因 useCallback 重建而重跑；这里等价补齐。
watch(
  () => props.connectionId,
  () => {
    void reload();
    emit("register-refresh", reload);
  },
);

async function handleExecute(cmd: string): Promise<void> {
  if (!cmd.trim()) return;

  // Determine broadcast destinations
  const destinations =
    props.broadcastTargets.length > 0 ? props.broadcastTargets : [props.sessionId];

  // Send to all destinations (broadcast or single)
  await Promise.allSettled(destinations.map((sid) => sendFn(sid, cmd + "\r")));

  // Record to history (only once, for this connection)
  if (props.connectionId) {
    addCommandHistory(props.connectionId, cmd)
      .then(() => reload())
      .catch(() => {});
  }
  input.value = "";
}

function onInputKeyDown(e: KeyboardEvent): void {
  if (e.key === "Enter" && input.value.trim()) {
    void handleExecute(input.value);
  }
}

async function handlePin(item: CommandHistoryItem): Promise<void> {
  await setCommandHistoryPinned(item.id, !item.pinned);
  await reload();
}

async function handleDelete(id: number): Promise<void> {
  await deleteCommandHistory(id);
  await reload();
}

async function handleClearUnpinned(): Promise<void> {
  if (!props.connectionId) return;
  await clearCommandHistory(props.connectionId, false);
  await reload();
}

// ============ Quick Commands ============

async function reloadQuickCommands(): Promise<void> {
  if (!props.connectionId) return;
  quickLoading.value = true;
  try {
    const items = await listQuickCommandsForConnection(props.connectionId);
    quickCommands.value = items;
  } catch {
    // Silently ignore — not critical.
  } finally {
    quickLoading.value = false;
  }
}

// Load on open (cheap; the panel is usually closed).
watch(quickPanelOpen, (open) => {
  if (open) void reloadQuickCommands();
});

/** Execute a quick command: parse into ordered steps (command lines + delay
 *  directives), send each command line with a trailing CR, fanning out to
 *  broadcast targets (or just this session).
 *
 *  Inter-line timing is controlled by the configured mode (see
 *  `readQuickCmdDelayConfig`) plus any `##delay:<N>` directives:
 *  - A `##delay:<N>` directive between two lines is always honoured as a
 *    minimum wait floor of <N> ms.
 *  - mode=`fixed` adds a fixed baseline between lines with no directive
 *    (combined with the floor by max).
 *  - mode=`idle` watches the session's output stream and only sends the next
 *    line once the previous line's output has gone quiet for `ms`. This
 *    reliably handles interactive prompts (sudo/mysql/ssh password) that
 *    neither a fixed delay nor a shell sentinel can detect — when output
 *    stops, either the shell prompt is back or the program is waiting for
 *    input. The floor (directive/fixed) AND the idle wait both apply.
 *  - mode=`off` sends immediately unless a directive provides a floor. */
async function handleExecuteQuickCommand(command: string): Promise<void> {
  if (props.status === "disconnected" || props.status === "error") return;
  const steps = parseQuickCommand(command);
  if (steps.length === 0) return;
  const { mode, ms: configMs } = readQuickCmdDelayConfig();
  const destinations =
    props.broadcastTargets.length > 0 ? props.broadcastTargets : [props.sessionId];
  const useIdle = mode === "idle";
  // Idle quiescence window — floor at 300 ms so a misconfigured 0 still waits
  // for output to actually settle.
  const quietMs = useIdle ? Math.max(configMs, 300) : 0;

  quickPanelOpen.value = false;

  // ── Idle-detection state (shared across the whole run) ──────────────
  // local.rs emits its PTY output on the same `ssh_output` event, so this
  // listener covers local tabs too.
  let lastDataAt = Date.now();
  let landed = false; // has ANY output arrived since the most recent send?
  let unlisten: (() => void) | null = null;

  /** Wait until the previous line's output has quiesced. Phase 1 waits for
   *  the line's PTY echo to land (so a stale lastDataAt isn't mistaken for
   *  "settled"); phase 2 waits for `quietMs` of silence. A hard cap stops a
   *  hung command (tail -f) from blocking the sequence forever. */
  const waitForQuiescence = async () => {
    const FIRST_OUTPUT_TIMEOUT_MS = 1500;
    const MAX_GAP_WAIT_MS = 30_000;
    const echoDeadline = Date.now() + FIRST_OUTPUT_TIMEOUT_MS;
    while (!landed && Date.now() < echoDeadline) await sleep(40);
    const start = Date.now();
    while (Date.now() - start < MAX_GAP_WAIT_MS) {
      if (Date.now() - lastDataAt >= quietMs) return;
      await sleep(40);
    }
  };

  try {
    if (useIdle) {
      unlisten = await onSshOutput(props.sessionId, () => {
        lastDataAt = Date.now();
        landed = true;
      });
    }

    let pendingDelay = 0; // ms accumulated from ##delay directives since last cmd
    let hadExplicitDelay = false; // any directive seen since last cmd?
    let firstCmd = true;
    for (const step of steps) {
      if (step.type === "delay") {
        pendingDelay += step.ms;
        hadExplicitDelay = true;
        continue;
      }
      if (!firstCmd) {
        // Floor: explicit directive OR fixed-mode baseline, combined by max.
        const floorMs = Math.max(
          hadExplicitDelay ? pendingDelay : 0,
          mode === "fixed" ? configMs : 0,
        );
        if (floorMs > 0) await sleep(floorMs);
        if (useIdle) await waitForQuiescence();
      }
      // Arm the landing flag BEFORE sending so this line's echo flips it
      // true and arms the next gap's quiescence check.
      if (useIdle) landed = false;
      await Promise.allSettled(destinations.map((sid) => sendFn(sid, step.text + "\r")));
      pendingDelay = 0;
      hadExplicitDelay = false;
      firstCmd = false;
    }
  } finally {
    unlisten?.();
  }
}

/** 点击历史项：填入输入框，不自动执行 */
function handleSelectItem(cmd: string): void {
  input.value = cmd;
  panelOpen.value = false;
  // 聚焦到输入框方便用户修改后回车执行。Scope to this CommandBar's
  // container so focus stays in the active tab when several are open.
  const inputEl = containerRef.value?.querySelector<HTMLInputElement>("[data-cmd-input]");
  inputEl?.focus();
}

// ── 面板互斥开关（与旧版 onClick 置位逻辑一致） ──
function toggleQuickPanel(): void {
  panelOpen.value = false;
  quickPanelOpen.value = !quickPanelOpen.value;
}

function toggleHistoryPanel(): void {
  quickPanelOpen.value = false;
  panelOpen.value = !panelOpen.value;
}

function onScreenshotClick(): void {
  panelOpen.value = false;
  quickPanelOpen.value = false;
  emit("screenshot");
}

function onManageClick(): void {
  quickPanelOpen.value = false;
  emit("open-quick-commands-manage");
}

const showReconnect = computed(
  () => props.status === "disconnected" || props.status === "error",
);

// 快捷命令按作用域分组（旧版 QuickCommandGroup 子组件，空组不渲染）
const globalQuickCommands = computed(() => quickCommands.value.filter((q) => q.isGlobal));
const serverQuickCommands = computed(() => quickCommands.value.filter((q) => !q.isGlobal));

function firstLine(cmd: string): string {
  return cmd.split(/\r?\n/)[0];
}
</script>

<template>
  <div ref="containerRef" class="command-bar">
    <!-- Command input -->
    <input
      v-model="input"
      data-cmd-input
      class="cmd-input"
      placeholder="$ 输入命令..."
      @keydown="onInputKeyDown"
    />

    <!-- Quick commands button -->
    <button
      type="button"
      class="bar-btn"
      :class="{ active: quickPanelOpen }"
      title="快捷命令"
      @click="toggleQuickPanel"
    >
      <el-icon :size="14"><Lightning /></el-icon>
      <span v-if="showLabels">快捷</span>
    </button>

    <!-- Screenshot button — captures the terminal viewport only (this
         CommandBar is excluded by the capture util). Saves to the
         attachment dir configured in Settings → MCP 支持. -->
    <button type="button" class="bar-btn" title="截取当前终端（不含输入栏/工具栏）" @click="onScreenshotClick">
      <el-icon :size="14"><Camera /></el-icon>
      <span v-if="showLabels">截图</span>
    </button>

    <!-- History button -->
    <button
      type="button"
      class="bar-btn"
      :class="{ active: panelOpen }"
      title="历史命令"
      @click="toggleHistoryPanel"
    >
      <el-icon :size="14"><Clock /></el-icon>
      <span v-if="showLabels">历史</span>
    </button>

    <!-- Multi-window picker button -->
    <button type="button" class="bar-btn" title="多窗口" @click="emit('open-multiwindow')">
      <el-icon :size="14"><Grid /></el-icon>
      <span v-if="showLabels">多窗口</span>
    </button>

    <!-- AI assistant button — opens the docked right panel -->
    <button type="button" class="bar-btn" title="AI 助手" @click="emit('open-ai')">
      <el-icon :size="14"><ChatDotRound /></el-icon>
      <span v-if="showLabels">AI</span>
    </button>

    <!-- Reconnect button (only when disconnected/error) -->
    <button v-if="showReconnect" type="button" class="bar-btn reconnect" title="重新连接" @click="emit('reconnect')">
      <el-icon :size="14"><RefreshRight /></el-icon>
      <span v-if="showLabels">重连</span>
    </button>

    <!-- Expanded history panel (floating overlay) -->
    <div v-if="panelOpen" class="float-panel history-panel">
      <!-- Header -->
      <div class="float-header">
        <span class="float-title">历史命令 <span v-if="loading">(加载中...)</span></span>
        <button type="button" class="link-btn" title="清空未钉住的历史" @click="handleClearUnpinned">清空</button>
      </div>

      <!-- List -->
      <div class="float-list">
        <div v-if="history.length === 0" class="empty">暂无历史记录</div>
        <template v-else>
          <div
            v-for="item in history"
            :key="item.id"
            class="history-item"
            @click="handleSelectItem(item.command)"
          >
            <!-- Pin icon：EP 无 Pushpin，用 Top 代钉住形态。两种状态同图标 ——
                 钉住全亮，未钉住靠 opacity 减淡。 -->
            <button
              type="button"
              class="pin-btn"
              :class="{ pinned: item.pinned }"
              :title="item.pinned ? '取消钉住' : '钉住'"
              @click.stop="handlePin(item)"
            >
              <el-icon :size="14"><Top /></el-icon>
            </button>

            <!-- Command text -->
            <span class="cmd-text">{{ item.command }}</span>

            <!-- Delete button -->
            <button type="button" class="del-btn" title="删除" @click.stop="handleDelete(item.id)">✕</button>
          </div>
        </template>
      </div>
    </div>

    <!-- Expanded quick commands panel (floating overlay) -->
    <div v-if="quickPanelOpen" class="float-panel quick-panel">
      <!-- Header -->
      <div class="float-header">
        <span class="float-title">快捷命令 <span v-if="quickLoading">(加载中...)</span></span>
        <button type="button" class="link-btn manage" title="管理快捷命令" @click="onManageClick">管理</button>
      </div>

      <!-- Grouped list -->
      <div class="float-list">
        <div v-if="quickCommands.length === 0" class="empty">暂无快捷命令</div>
        <template v-else>
          <div v-if="globalQuickCommands.length > 0" class="qc-group">
            <div class="qc-group-title">全局命令</div>
            <div
              v-for="item in globalQuickCommands"
              :key="item.id"
              class="qc-item"
              :title="item.command"
              @click="handleExecuteQuickCommand(item.command)"
            >
              <span class="qc-arrow">▶</span>
              <div class="qc-text">
                <div class="qc-label">{{ item.label }}</div>
                <div class="qc-cmd">{{ firstLine(item.command) }}</div>
              </div>
            </div>
          </div>
          <div v-if="serverQuickCommands.length > 0" class="qc-group">
            <div class="qc-group-title">本服务器专属</div>
            <div
              v-for="item in serverQuickCommands"
              :key="item.id"
              class="qc-item"
              :title="item.command"
              @click="handleExecuteQuickCommand(item.command)"
            >
              <span class="qc-arrow">▶</span>
              <div class="qc-text">
                <div class="qc-label">{{ item.label }}</div>
                <div class="qc-cmd">{{ firstLine(item.command) }}</div>
              </div>
            </div>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.command-bar {
  height: 36px;
  min-height: 36px;
  background: var(--bg-elevated);
  border-top: 1px solid var(--border-default);
  display: flex;
  align-items: center;
  padding: 0 12px;
  gap: 8px;
  position: relative;
}

.cmd-input {
  flex: 1;
  min-width: 0;
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 6px 10px;
  color: var(--text-primary);
  font-size: 13px;
  font-family: inherit;
  outline: none;
}

.bar-btn {
  background: var(--bg-input);
  color: var(--text-secondary);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 6px 12px;
  flex-shrink: 0;
  white-space: nowrap;
  font-size: 12px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 4px;
  transition: all var(--duration-fast) var(--ease-in-out);
}

.bar-btn.active {
  background: var(--accent-primary);
  color: var(--text-inverse);
}

.bar-btn.reconnect {
  background: var(--success-muted);
  color: var(--success);
  border: 1px solid var(--success);
  border-radius: var(--radius-sm);
  font-weight: 600;
}

.bar-btn.reconnect:hover {
  background: var(--success);
  color: var(--text-inverse);
  transform: scale(1.05);
}

/* 浮层面板（历史 / 快捷命令） */
.float-panel {
  position: absolute;
  bottom: 36px;
  right: 8px;
  max-height: 60vh;
  background: var(--bg-surface);
  border: 1px solid var(--border-emphasis);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  display: flex;
  flex-direction: column;
  z-index: 10;
}

.history-panel {
  width: 480px;
}

.quick-panel {
  width: 420px;
}

.float-header {
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-default);
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.float-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
}

.link-btn {
  background: transparent;
  border: none;
  color: var(--text-muted);
  font-size: 11px;
  cursor: pointer;
}

.link-btn.manage {
  color: var(--accent-primary);
}

.float-list {
  flex: 1;
  overflow-y: auto;
  padding: 4px 0;
}

.empty {
  padding: 16px;
  text-align: center;
  color: var(--text-muted);
  font-size: 12px;
}

.history-item {
  display: flex;
  align-items: center;
  padding: 6px 12px;
  gap: 8px;
  cursor: pointer;
  transition: background var(--duration-fast) var(--ease-in-out);
}

.history-item:hover {
  background: var(--bg-surface-hover);
}

.pin-btn {
  background: transparent;
  border: none;
  cursor: pointer;
  font-size: 14px;
  opacity: 0.4;
  padding: 0;
}

.pin-btn.pinned {
  opacity: 1;
}

.cmd-text {
  flex: 1;
  min-width: 0;
  font-size: 12px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.del-btn {
  background: transparent;
  border: none;
  color: var(--text-muted);
  font-size: 12px;
  cursor: pointer;
  opacity: 0.6;
}

.qc-group-title {
  padding: 8px 12px 4px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-muted);
}

.qc-item {
  display: flex;
  align-items: center;
  padding: 7px 12px;
  gap: 8px;
  cursor: pointer;
  transition: background var(--duration-fast) var(--ease-in-out);
}

.qc-item:hover {
  background: var(--bg-surface-hover);
}

.qc-arrow {
  font-size: 11px;
  color: var(--accent-primary);
}

.qc-text {
  flex: 1;
  min-width: 0;
}

.qc-label {
  font-size: 12px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.qc-cmd {
  font-size: 11px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
