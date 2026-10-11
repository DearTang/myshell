<!-- 命令栏：命令输入 + 历史命令浮层 + 快捷命令浮层 + 截图/重连入口。
     从 src-legacy/components/CommandBar.tsx 逐行移植；
     悬浮面板绝对定位于本栏上方，互斥展开（与旧版点击置位逻辑一致）。
     2026-10-10 三处调整：
     ① 位置：从 TerminalPanel 内部提升到壳层底部行（App.vue 的 .shell-bottom），
        与服务器状态栏一起占侧栏右侧的两列。此前两者只在终端区那一列，
        右侧 AI 栏自带的底部两行与之并排、互不对齐。同日再把底栏从"贯穿整个
        窗口宽度"收回为"侧栏右侧"，不再压到侧栏底下。
        多窗口模式例外：那种布局里每个格子自带终端与命令栏，仍由 TerminalPanel
        内嵌渲染（`command-bar` prop，默认开启）。
     ② 删去「多窗口」「AI」两枚按钮 —— 二者在顶栏已有同名常驻入口。
     ③ 布局改为「输入框占左半 + 工具钮靠右」。
     历史列表的刷新回调改走 store 注册表（见 registerSelf）：本组件不再与
     TerminalPanel 构成父子关系，原来那条 emit 直连已失效。 -->
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { MyButton, MyIcon, MyIconButton, MyInput, MyPopover } from "myui";
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
import { registerHistoryRefresher, unregisterHistoryRefresher } from "@/store/sessions";

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
  (e: "reconnect"): void;
  (e: "open-quick-commands-manage"): void;
  /** 截取终端视口 PNG（不含底栏）。保存到设置的附件目录。 */
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
/** 命令输入框实例（MyInput 暴露 focus()/blur()） */
const cmdInputRef = ref<InstanceType<typeof MyInput> | null>(null);
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

// 挂载 / connectionId 变化时加载一次，并把 reload 注册进 store 的历史刷新表。
//
// 旧版是 emit('register-refresh') 交给父组件存着、由 TerminalPanel 在录完命令后
// 调用。2026-10-10 本组件被提升到壳层底部行（不再是 TerminalPanel 的子组件），
// 这条 emit 直连断了，改走 store 的注册表。
//
// 注销要按"自己注册过的 key"来做：同一连接先挂 A 再挂 B 时，B 的注册会覆盖 A，
// A 卸载时若按自己的 connectionId 注销就会把 B 的那条一起清掉。
let registeredHistoryId = "";

function registerSelf(): void {
  if (registeredHistoryId) unregisterHistoryRefresher(registeredHistoryId);
  registeredHistoryId = props.connectionId;
  if (registeredHistoryId) registerHistoryRefresher(registeredHistoryId, reload);
  void reload();
}

onMounted(registerSelf);

onUnmounted(() => {
  if (registeredHistoryId) unregisterHistoryRefresher(registeredHistoryId);
});

// connectionId 变化时旧版会因 useCallback 重建而重跑；这里等价补齐。
watch(() => props.connectionId, registerSelf);

async function handleExecute(cmd: string): Promise<void> {
  if (!cmd.trim()) return;

  // Clear the input SYNCHRONOUSLY, before awaiting the send. Clearing after
  // the await meant a command typed while the send was in flight got wiped by
  // the trailing `input.value = ""`, leaving the box out of sync with history
  // (the first command was already recorded).

  // Determine broadcast destinations
  const destinations =
    props.broadcastTargets.length > 0 ? props.broadcastTargets : [props.sessionId];

  input.value = "";

  // Send to all destinations (broadcast or single)
  await Promise.allSettled(destinations.map((sid) => sendFn(sid, cmd + "\r")));

  // Record to history (only once, for this connection)
  if (props.connectionId) {
    addCommandHistory(props.connectionId, cmd)
      .then(() => reload())
      .catch(() => {});
  }
}

function onInputKeyDown(e: KeyboardEvent): void {
  // IME composition: pressing Enter to CONFIRM a Chinese/Japanese/Korean
  // candidate also dispatches keydown with key === "Enter" and isComposing
  // true. Treating that as a submit sent the raw pinyin to the host and then
  // wiped the input, losing the text the user just composed. keyCode 229 is
  // the legacy signal some IMEs emit instead.
  if (e.isComposing || e.keyCode === 229) return;
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
  // 聚焦到输入框方便用户修改后回车执行。MyInput 暴露了 focus()，直接用组件
  // 实例方法，不必再靠 data-* 属性 + querySelector 去捞 DOM。
  cmdInputRef.value?.focus();
}

// ── 面板互斥（与旧版 toggle 置位逻辑一致） ──
// 开关本身由 MyPopover 的 click trigger 驱动（EP 受控 visible），
// 这里只负责"开一个必关另一个"这条互斥规则。
// 迁移前两个面板是手搓 absolute 浮层：既无 click-outside 也无 Esc，
// 只能靠这两个 toggle 互斥关闭——点面板外或按 Esc 都不关。
watch(panelOpen, (open) => {
  if (open) quickPanelOpen.value = false;
});

watch(quickPanelOpen, (open) => {
  if (open) panelOpen.value = false;
});

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
    <!-- Command input.
         外层 div 不是多余的包装：MyInput 是 inheritAttrs:false 且把 $attrs
         **显式转给了内层 ElInput**，所以 class 落在 ElInput 的根上，而不是
         .command-bar 的直接 flex 子项（div.my-field.my-field--bare）。把
         flex:0 1 50% 直接挂在 MyInput 上，等于写在非 flex 子元素上 —— 完全不
         生效，输入框退回内容自适应宽度（实测约占本栏 15%）。
         宽度改由这层**块级**包裹 div 控制：.my-field--bare / .my-field__control /
         .el-input 在块上下文里逐级撑满，无需 :deep() 就能打穿到控件宽度。 -->
    <div class="cmd-slot">
      <MyInput
        ref="cmdInputRef"
        v-model="input"
        bare
        size="small"
        placeholder="$ 输入命令..."
        @keydown="onInputKeyDown"
      />
    </div>

    <!-- 输入框与工具钮之间的弹性留白：输入框固定占左半，工具钮整体靠右。
         没有它的话按钮会紧贴输入框右侧，栏的右半看起来是空的。 -->
    <div class="bar-spacer" />

    <!-- Quick commands button -->
    <!-- popper-style 承担面板宽度：宽度必须给到 popper **外壳**上，不能只给
         面板自己。EP 的 .el-popover 有 `min-width: 150px` 且 position:absolute，
         外壳按 shrink-to-fit 收敛到 150px 就再也不长（面板反而溢出外壳 281px，
         见 progress.md 阶段 180 的实测数据）；popper.js 定位/防溢出用的是外壳
         尺寸，于是 150px 的外壳被判定"放得下"，面板右半截直接顶出 webview。
         EP 的 contentStyle 合并顺序是 [zIndex, popperStyles, popperStyle]，
         popperStyle 在最后 —— 能覆盖 popper.js 写回的内联 width。
         min(定宽, 100vw - 32px)：窗口够宽用定宽，够窄自动收窄并由 popper.js 平移。 -->
    <MyPopover
      v-model="quickPanelOpen"
      placement="top-end"
      trigger="click"
      popper-class="cb-popper"
      :popper-style="{ width: 'min(420px, calc(100vw - 32px))' }"
    >
      <!-- 激活态不再自绘 .active 底色，改由 :variant 切到 primary：两态的
           对比度、focus 环都由 myui 负责，工具栏不再维护第二套按钮配色。 -->
      <MyButton
        class="bar-btn"
        :variant="quickPanelOpen ? 'primary' : 'secondary'"
        size="small"
        title="快捷命令"
      >
        <MyIcon :size="14" name="Lightning" />
        <span v-if="showLabels">快捷</span>
      </MyButton>

      <template #content>
        <div class="float-panel quick-panel">
          <!-- Header -->
          <div class="float-header">
            <span class="float-title">快捷命令 <span v-if="quickLoading">(加载中...)</span></span>
            <MyButton
              class="link-btn manage"
              variant="ghost-primary"
              size="small"
              title="管理快捷命令"
              @click="onManageClick"
            >管理</MyButton>
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
                  <MyIcon :size="12" name="ArrowRight" class="qc-arrow" />
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
                  <MyIcon :size="12" name="ArrowRight" class="qc-arrow" />
                  <div class="qc-text">
                    <div class="qc-label">{{ item.label }}</div>
                    <div class="qc-cmd">{{ firstLine(item.command) }}</div>
                  </div>
                </div>
              </div>
            </template>
          </div>
        </div>
      </template>
    </MyPopover>

    <!-- Screenshot button — captures the terminal viewport only (this
         CommandBar is excluded by the capture util). Saves to the
         attachment dir configured in Settings → MCP 支持. -->
    <MyButton
      class="bar-btn"
      size="small"
      title="截取当前终端（不含输入栏/工具栏）"
      @click="onScreenshotClick"
    >
      <MyIcon :size="14" name="Camera" />
      <span v-if="showLabels">截图</span>
    </MyButton>

    <!-- History button -->
    <MyPopover
      v-model="panelOpen"
      placement="top-end"
      trigger="click"
      popper-class="cb-popper"
      :popper-style="{ width: 'min(480px, calc(100vw - 32px))' }"
    >
      <MyButton
        class="bar-btn"
        :variant="panelOpen ? 'primary' : 'secondary'"
        size="small"
        title="历史命令"
      >
        <MyIcon :size="14" name="Clock" />
        <span v-if="showLabels">历史</span>
      </MyButton>

      <template #content>
        <div class="float-panel history-panel">
          <!-- Header -->
          <div class="float-header">
            <span class="float-title">历史命令 <span v-if="loading">(加载中...)</span></span>
            <MyButton class="link-btn" variant="ghost" size="small" title="清空未钉住的历史" @click="handleClearUnpinned">清空</MyButton>
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
                <!-- 钉住开关：myui v0.29.0 起改用 MyIconButton 的 active 态。
                     此前是裸 button + opacity 0.4/1 区分两态——同图标仅靠深浅
                     区分对低对比用户不可辨，也没有 aria-pressed，屏幕阅读器读不出
                     状态；现由底色+前景色双通道表达。图标也从 Top（向上箭头，
                     语义是"向上移动"）换成库自绘的 Pushpin（斜置 45° 经典图钉）。 -->
                <MyIconButton
                  :title="item.pinned ? '取消钉住' : '钉住'"
                  class="pin-btn"
                  :label="item.pinned ? '取消钉住' : '钉住'"
                  :active="item.pinned"
                  size="14"
                  name="Pushpin"
                  @click.stop="handlePin(item)"
                />

                <!-- Command text -->
                <span class="cmd-text">{{ item.command }}</span>

                <!-- Delete button -->
                <MyIconButton
                  title="删除"
                  class="del-btn"
                  label="删除"
                  size="12"
                  name="Close"
                  @click.stop="handleDelete(item.id)"
                />
              </div>
            </template>
          </div>
        </div>
      </template>
    </MyPopover>

    <!-- Reconnect button (only when disconnected/error) -->
    <MyButton
      v-if="showReconnect"
      class="bar-btn reconnect"
      variant="success"
      size="small"
      title="重新连接"
      @click="emit('reconnect')"
    >
      <MyIcon :size="14" name="RefreshRight" />
      <span v-if="showLabels">重连</span>
    </MyButton>
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

/* 命令输入框：外观（边框/圆角/聚焦环/字号）由 MyInput 的 bare+small 承担，
   这里只定它的宽度策略 —— **固定占本栏左半**（可收缩，窄栏时优先让输入框
   变窄而不是把按钮挤出可视区）。此前是 flex:1 撑满剩余空间，工具钮全被顶到
   输入框右边贴着，栏的右半一片空白。

   必须挂在 .cmd-slot（包裹层）而不是 MyInput 上，理由见模板处注释：MyInput 的
   class 会被转给内层 ElInput，而 flex 属性只对 .command-bar 的直接子元素生效。
   .cmd-slot 保持块级（不要改成 display:flex —— 那样 .my-field 又会退回按内容
   收缩），内部的 my-field--bare / my-field__control / .el-input 在块上下文里
   逐级撑满，无需 :deep()。 */
.cmd-slot {
  flex: 0 1 50%;
  min-width: 0;
}

/* 输入框与工具钮之间的弹性空档 */
.bar-spacer {
  flex: 1;
  min-width: 0;
}

/* 工具栏按钮改用 MyButton 后，这里只剩排版：不换行、不被 flex 压扁、
   图标与文字的间距（el-button 自身不给 gap）。配色 / 描边 / 圆角 /
   hover / focus 环全部由 variant 承担。
   激活态也不再自绘 .active 底色，改由 :variant 在 secondary ⇄ primary 间切。 */
.bar-btn {
  flex-shrink: 0;
  white-space: nowrap;
  gap: 4px;
}

/* 重连钮：配色交给 MyButton 的 success 变体，这里只留一点按下缩放的微动效
   （纯 transform，不与 myui 的颜色/边框规则打架）。 */
.bar-btn.reconnect:hover {
  transform: scale(1.05);
}

/* 浮层面板（历史 / 快捷命令）
   定位已交给 MyPopover（EP popper：teleport、视口碰撞翻转、点外关闭、Esc），
   这里只保留面板自身的盒子与内部布局。 */
.float-panel {
  max-height: 60vh;
  background: var(--bg-surface);
  border: 1px solid var(--border-emphasis);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* popper 外壳中和：EP 默认给 .el-popover 加了 12px padding 与自己的边框/圆角，
   面板自己有盒子，这里去掉避免双层。表面色由 myui 的 html.dark
   --el-bg-color-overlay 提供（暗色下为深色），故只清 padding 与边框。 */
:global(.cb-popper.el-popover) {
  padding: 0;
  border: none;
  background: transparent;
  box-shadow: none;
  overflow: visible;
}

/* 面板宽度改由 MyPopover 的 :popper-style 给到 popper 外壳（见模板注释），
   面板自身只占满外壳。这里**不能**再写死 width —— 外壳 150px / 面板 480px
   的错配正是「浮层溢出窗口右缘」的根因。
   max-height 同理：top-end 是底对齐，面板过高时顶边会越过窗口上沿；
   限高后由 .float-list（已有 flex:1 + overflow-y:auto）内部滚动。 */
.history-panel {
  width: 100%;
  max-height: min(60vh, calc(100vh - 96px));
}

.quick-panel {
  width: 100%;
  max-height: min(60vh, calc(100vh - 96px));
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

/* 浮层标题栏里的文字链按钮。myui small 尺寸的左右内边距相对 11px 字号
   的标题行偏宽，收一档；「管理」那枚改用 ghost-primary 变体表达强调色，
   不再自写 .link-btn.manage 的 color。 */
.link-btn {
  padding: 0 4px;
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

/* 钉住钮只保留布局；选中/未选中的视觉差异交给 MyIconButton 的 active
   （底色+前景色双通道，并输出 aria-pressed）。 */
.pin-btn {
  flex: none;
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

/* 删除钮：MyIconButton 提供按钮本体，这里只调尺寸感与默认色的弱化。 */
.del-btn {
  flex: none;
  opacity: 0.7;
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
  flex: none;
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
