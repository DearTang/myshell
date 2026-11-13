<!-- 终端面板：xterm.js 完整生命周期 + ZMODEM 原生传输 UI + CommandBar 装配。
     从 src-legacy/components/TerminalPanel.tsx 逐行移植。关键不变量：
     - xterm 实例绝不放进响应式对象（普通 let + ref 容器 DOM）；
     - tab.id 稳定（组件实例跨重连存活），sessionId 变化由 watch 驱动完整
       重建（等价于旧版 [sessionId] effect），scrollback 经 reconnectSnapshot
       恢复；
     - 所有 tab 面板常驻挂载（App.vue 用绝对定位 + visibility 切换），
       xterm 容器必须保持真实尺寸。 -->
<script setup lang="ts">
import { MyButton, MyDialog, MyIcon, MyProgress, toast } from "myui";
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { Terminal } from "@xterm/xterm";
import type { ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { CanvasAddon } from "@xterm/addon-canvas";
import { WebglAddon } from "@xterm/addon-webgl";
import { WebLinksAddon } from "@xterm/addon-web-links";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  addCommandHistory,
  localResize,
  localSend,
  nudgeMouseCursor,
  onSftpTransferDone,
  onSftpTransferProgress,
  onSshClosed,
  onSshOutput,
  onZmodemEnd,
  onZmodemError,
  onZmodemFileComplete,
  onZmodemFileSkipped,
  onZmodemOffer,
  onZmodemProgress,
  onZmodemRaw,
  onZmodemStart,
  saveScreenshot,
  sftpCancelTransfer,
  sftpCheckDirConflicts,
  sftpUpload,
  probeUploadCapabilities,
  showInFolder,
  sshReady,
  sshResize,
  sshSend,
  sshSendZmodemAbort,
  zmodemAcceptOffer,
  zmodemStartUpload,
} from "@/api";
import type {
  ConnType,
  SftpDirConflictReport,
  SftpTransferProgressPayload,
  UploadCapabilities,
  Tab,
} from "@/api";
import { open } from "@tauri-apps/plugin-dialog";
import { captureTerminalToDataUrl } from "@/utils/screenshot";
import { createSentinelFilter } from "@/utils/sentinel-filter";
import { recordKeystroke } from "@/utils/cmd-buffer";
import { getActivePalette } from "@/store/appearance";
import {
  refreshCommandHistory,
  registerSessionAction,
  unregisterSessionAction,
} from "@/store/sessions";
import { ui } from "@/store/ui";
import { primaryFont, resolveFontStack, terminalFontSize } from "@/composables/useTerminalFont";
import { resolveRenderer } from "@/composables/useRendererPref";
import type { RendererBackend } from "@/composables/useRendererPref";
import { ZmodemBridge } from "@/zmodem-bridge";
import type { ZmodemStatus } from "@/zmodem-bridge";
import CommandBar from "./CommandBar.vue";
import ZmodemProgressOverlay from "./ZmodemProgressOverlay.vue";
import "@xterm/xterm/css/xterm.css";

defineOptions({ name: "TerminalPanel" });

const props = withDefaults(
  defineProps<{
    /** STABLE tab identifier — never changes across reconnects. Keeps the
     * component instance (and thus the TerminalPanel DOM) stable across a
     * reconnect; only `sessionId` is replaced, and the sessionId watch rebinds
     * the terminal lifecycle without remounting this component. */
    tabId: string;
    sessionId: string;
    /** Tab connection type — selects ssh_* vs local_* backend commands.
     * Defaults to "ssh". sftp/ftp tabs never render a TerminalPanel. */
    connType?: ConnType;
    /** ConnectionConfig.id this session belongs to. Used as the persistence
     * key for command history — survives reconnect (sessionId changes each
     * connect, connectionId doesn't). Empty for sessions without a backing
     * connection row (shouldn't normally happen). */
    connectionId: string;
    /** Per-connection terminal font override (family name). When set, wins over
     * the global terminal font for this tab. Undefined/empty → use global. */
    fontOverride?: string;
    /** Terminal renderer backend preference ("auto" | "dom" | "canvas" |
     * "webgl"). "auto" = canvas by default, WebGL when a background image needs
     * transparency. Fixed per app (not per-connection); read once at terminal
     * setup. */
    rendererBackend: RendererBackend;
    /** When non-empty (and contains more than just this tab's own sessionId),
     * every keystroke is mirrored to all listed sessions in addition to the
     * local one. Read live from props so toggling broadcast on/off doesn't
     * need to re-bind onData. */
    broadcastTargets: string[];
    /** Whether this panel is the currently-visible tab. All tabs stay mounted
     * (so their xterm history + onSshOutput subscriptions persist across tab
     * switches); we only refit and refocus when this becomes the active tab. */
    active: boolean;
    /** Connection status: "connecting" | "connected" | "disconnected" | "error" */
    status?: Tab["status"];
    /** Tab display name — used as part of the screenshot filename when the
     * user hits the 📷 button in CommandBar. */
    connectionName?: string;
    /** Terminal text captured from the previous xterm instance before a
     * reconnect. If present when the terminal (re)builds, it's written to the
     * new terminal to restore scrollback history. The caller is responsible
     * for clearing it (via snapshot-consumed) so it's only restored once. */
    reconnectSnapshot?: string;
    /** 是否在本面板内部渲染命令栏。默认 true（多窗口网格：每个格子自带一条）。
     *  2026-10-10 起主标签区传 false —— 命令栏被提升到壳层底部行，由 App.vue 统一
     *  渲染一条给活动标签用，与服务器状态栏一起贯穿整个窗口宽度。 */
    commandBar?: boolean;
    /** 分屏叶子专用：false 时本叶子不把 keystroke 发给后端（焦点在别的
     *  叶子上），但 xterm 仍照常渲染输出。默认 true —— 单 pane 与多窗口
     *  网格的调用点不传即为焦点行为，零改动。 */
    inputEnabled?: boolean;
    /** 分屏叶子专用：false 时本叶子的 fit() 只做本地测量，不把 cols/rows
     *  推给远端。SSH PTY 只有一个 window size，多个 xterm 各自 resize
     *  会互相打架；只有焦点叶子的尺寸能代表这个会话。默认 true。 */
    resizeEnabled?: boolean;
  }>(),
  // 三个可选 boolean prop 必须显式声明默认值。Vue 3 对 Boolean 类型的
  // prop 有 boolean casting：调用点不传时它被解析成 **false**（不是
  // undefined）—— 于是「不传即默认开启」的门禁会反过来把调用点全部拦死
  // （实测：单 pane 与多窗口网格不传 inputEnabled，终端输入被整条吞掉，
  // 表现为「敲了没反应、enter 无效」）。给了 default 后 casting 不再介入。
  { commandBar: true, inputEnabled: true, resizeEnabled: true },
);

const emit = defineEmits<{
  /** Phase 3: registers the xterm instance with the session-store registry
   * (AI 面板读输出/选区、重连快照都经它)。terminal 打开后触发；重建/关闭时
   * 由 terminal-gone 注销。 */
  (e: "terminal-ready", sessionId: string, term: Terminal): void;
  (e: "terminal-gone", sessionId: string): void;
  (e: "reconnect"): void;
  /** reconnectSnapshot 已写入新终端，父层据此清掉 tab 上的快照。 */
  (e: "snapshot-consumed"): void;
  /** SSH 会话被远端/网络关闭；父层把 tab 状态置为 disconnected。 */
  (e: "disconnected"): void;
  (e: "open-quick-commands-manage", connectionId: string): void;
}>();

/**
 * Bump a selection color's alpha so the selected range is clearly visible.
 * Many palettes set `selectionBackground` at ~27% alpha (#RRGGBBAA with AA=44),
 * which renders nearly invisibly on the terminal background and makes it hard
 * to tell what's selected. Raise low alpha to a clearly visible ~80% while
 * preserving each palette's chosen hue. For colors that are already opaque
 * (6-digit hex), keep them; for ones with high alpha, keep them too.
 */
function visibleSelection(color: string | undefined): string | undefined {
  if (!color) return color;
  // 8-digit #RRGGBBAA → replace a low alpha byte with cc (≈80%).
  if (/^#[0-9a-fA-F]{8}$/.test(color)) {
    return color.slice(0, 7) + "cc";
  }
  // 6-digit #RRGGBB (opaque) → already visible; keep as-is.
  return color;
}

/**
 * Force the cursor color to the terminal's foreground color, and cursorAccent
 * (the block's interior text color) to the background. This guarantees the
 * cursor is visible in every environment — some users reported the cursor
 * being near-invisible (palette cursor color too close to the background on
 * their display/GPU), and `cursorBlink` under certain WebGL drivers can
 * render a low-contrast cursor as effectively invisible. Since `foreground`
 * is, by definition, the color chosen to be legible against `background`,
 * using it for the cursor guarantees the same contrast regardless of display
 * or driver.
 *
 * The cursor SHAPE is set separately at Terminal construction (see the
 * `cursorStyle: "bar"` / `cursorWidth` options in setupTerminal) — this
 * function only owns the colors.
 *
 * Returns the cursor overrides to merge into the xterm theme.
 */
function forceVisibleCursor(theme: ITheme): { cursor?: string; cursorAccent?: string } {
  const out: { cursor?: string; cursorAccent?: string } = {};
  // Always force cursor → foreground: it's the canonical legible-on-bg color,
  // so this is the most reliable cross-environment fix. A palette's own cursor
  // color can still be too low-contrast on some monitors.
  if (theme.foreground) out.cursor = theme.foreground;
  // cursorAccent = color of the glyph under the cursor; contrast it with the
  // (forced foreground) cursor fill by using the background.
  if (theme.background) out.cursorAccent = theme.background;
  return out;
}

// ── 非响应式句柄：xterm/插件实例一律用普通 let，禁止进响应式系统 ──
const containerRef = ref<HTMLDivElement | null>(null);
let term: Terminal | null = null;
let fitAddon: FitAddon | null = null;
let bridge: ZmodemBridge | null = null;
// 当前一轮终端生命周期的清理函数（等价于旧版 [sessionId] effect 的 cleanup）
let disposeFn: (() => void) | null = null;

// 命令历史击键缓冲。非响应式 —— 每次击键都不能触发重渲染。Enter 时 flush，
// 命令写回后端。
const cmdBufRef = { current: "" };
const ansiEscRef = { current: false };
// 2026-10-10：原先这里是 `let refreshHistory` —— CommandBar 挂载时通过
// `@register-refresh` 把自己的 reload 回调存进来，录完命令后调用。命令栏提升到
// 壳层底部行后两者不再是父子关系，改为 CommandBar 把 reload 注册进
// store/sessions 的 historyRefreshers（按 connectionId），这里直接触发。
// abort 兜底定时器：zmodem_finish 正常应在一个 IPC 往返内触发 zmodem_end，
// 若未到达（会话已死 / IPC 失败）2s 后强制重置。
let abortTimeout: ReturnType<typeof setTimeout> | null = null;

// Native ZMODEM state. Both download (sz) and upload (rz) are handled
// entirely in Rust — data never crosses IPC. The frontend only prompts
// for a save dir (download) or file selection (upload) and renders progress.
// （跨重建持久，等价于旧版 useRef 生命周期）
let isZmodem = false;
let nativeDownload = false;
let nativeDir: string | null = null;
let nativeCancelled = false;

// ── 响应式 UI 状态 ──
const zmodemStatus = ref<ZmodemStatus>({
  active: false,
  direction: null,
  currentFile: "",
  bytesTransferred: 0,
  bytesTotal: 0,
  speedBps: 0,
  error: null,
  startTime: 0,
});

// 手动 rz 的文件选择直接弹系统对话框（见 onZmodemStart → pickUploadFiles）；
// 内嵌「文件/文件夹」二选一浮层已删除 —— 多一步中间层是卡死多发地，
// 文件夹上传由拖拽路径承担（拖入即递归）。

// ── 拖拽上传（把本地文件拖进终端 → SFTP 上传到远端当前目录）──
// Tauri 2 的拖放是窗口级原生事件（tauri://drag-*），不是 DOM dragover ——
// WebView2 里 HTML5 拖放被 wry 的 IDropTarget 接管，前端 dragover/drop
// 永远不会触发。所以这里在 onMounted 里订阅窗口事件，再自己判断落点
// 是否在本面板内。
const fileDropActive = ref(false);
/** in-flight 拖拽上传；null = 无浮层。 */
const dropTransfer = ref<{
  phase: "upload";
  currentFile: string;
  fileIndex: number;
  fileCount: number;
  bytesDone: number;
  bytesTotal: number;
  errors: string[];
  done: boolean;
  startTime: number;
  requestId: string;
} | null>(null);
const dropTickNow = ref(Date.now());
let dropTickTimer: ReturnType<typeof setInterval> | null = null;
let dropUnlisten: Array<() => void> = [];
/** 取远端 cwd 的 in-flight 哨兵等待；同时只允许一次（拖一堆文件不会并发）。 */
let cwdProbe: Promise<string | null> | null = null;
/** 拖放落点判定用：本面板 DOM 是否覆盖 (x, y)。 */
const panelEl = ref<HTMLElement | null>(null);

/** 远端已存在同名目录时挂起的待确认上传（null = 无对话框）。 */
const pendingDrop = ref<null | {
  dest: string;
  paths: string[];
  conflictNames: string[];
}>(null);
/** 两条上传通道都不可用时的安装指引（null = 无对话框）。 */
const capsUnavailable = ref<null | { message: string | null; hint: string | null }>(null);

/** 拖拽上传走 ZMODEM 通道时挂起的待传路径；onZmodemStart 见到它就不弹选择器。 */
const autoUploadPaths = ref<string[] | null>(null);
/** rz 启动看门狗：12s 内没等到 zmodem_start 就按「rz 不可用」收场。 */
let rzWatchdog: ReturnType<typeof setTimeout> | null = null;

/** 拖入后、通道确定前的「准备中」遮罩（能力探测 + cwd 探针可长达十几秒）。 */
const dropPreparing = ref(false);

/** 一次拖拽上传的 ZMODEM 会话是否还在进行。true 期间，远端 rz 再次发出
 *  ZMODEM 邀请（被拒后重邀、多文件间隙）一律主动收场，绝不弹手动选择器 ——
 *  实测：文件被 ZSKIP 后 rz 会重新邀请，选择器弹出来用户点取消反而卡死。 */
const dragZmodemActive = ref(false);

/** 本地路径的 basename（拖入路径可能是 Windows 或 POSIX 分隔符）。 */
function basenameOf(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i >= 0 ? p.slice(i + 1) : p;
}

// ── 拖拽上传：发起传输 / 冲突出口 / 计时器 ──
// 放在组件作用域而不是 setupTerminal 内：冲突对话框由模板直接调用。
/** 真正发起传输（预检与用户确认都已完成）。 */
async function beginDropUpload(paths: string[], dest: string): Promise<void> {
  if (paths.length === 0) return;
  const requestId = crypto.randomUUID();
  dropTransfer.value = {
    phase: "upload",
    currentFile: "",
    fileIndex: 0,
    fileCount: 0,
    bytesDone: 0,
    bytesTotal: 0,
    errors: [],
    done: false,
    startTime: Date.now(),
    requestId,
  };
  startDropTick();
  // 先订阅再调用：最早的 progress 事件不能丢（与 SftpPanel.runTransfer 同序）。
  // 订阅本身是异步的（IPC 往返）：在 await 期间 dispose 可能先跑完，那时
  // closeDropTransfer 已经退过、拿不到这两个 unlisten，监听就会泄漏（并在
  // 下一个会话里继续收到旧 requestId 的事件）。所以用一个显式标志让
  // closeDropTransfer 知道「有订阅在路上」，等它们到位再补摘。
  dropListenersDetached = false;
  const unP = await onSftpTransferProgress(requestId, (p: SftpTransferProgressPayload) => {
    const tr = dropTransfer.value;
    if (!tr || tr.requestId !== requestId) return;
    tr.currentFile = p.currentFile;
    tr.fileIndex = p.fileIndex;
    tr.fileCount = p.fileCount;
    tr.bytesDone = p.bytesDone;
    tr.bytesTotal = p.bytesTotal;
  });
  const unD = await onSftpTransferDone(requestId, (errors, createdDirs) => {
    const tr = dropTransfer.value;
    if (!tr || tr.requestId !== requestId) return;
    tr.done = true;
    tr.errors = errors;
    dropTickNow.value = Date.now();
    // 完成提示只在这里发：createdDirs 只有事件载荷带，命令返回值没有。
    // await 分支若也发，事件晚到一帧就会变成两条 toast。
    announceDropResult(tr.fileCount, createdDirs);
  });
  if (dropListenersDetached) {
    // 订阅还没到位，closeDropTransfer 已经执行过——立刻自摘，别泄漏。
    dropListenersDetached = false;
    unP();
    unD();
    return;
  }
  dropUnlisten.push(unP, unD);
  try {
    await sftpUpload(props.sessionId, paths, dest, requestId);
    // 事件通常已先到；若还没到，只把浮层收成「完成」态，不重复提示。
    const tr = dropTransfer.value;
    if (tr && tr.requestId === requestId && !tr.done) {
      tr.done = true;
      dropTickNow.value = Date.now();
    }
  } catch (e) {
    const tr = dropTransfer.value;
    if (tr && tr.requestId === requestId) {
      tr.done = true;
      tr.errors = [...tr.errors, String(e)];
      dropTickNow.value = Date.now();
    }
  }
}

/** 完成提示。目录数由后端给出（前端从进度事件里推不出来）。 */
function announceDropResult(fileCount: number, createdDirs: number): void {
  const files = Math.max(0, fileCount);
  if (files === 0 && createdDirs === 0) return;
  let msg: string;
  if (files > 0 && createdDirs > 0) msg = `已上传 ${files} 个文件，创建 ${createdDirs} 个目录`;
  else if (files > 0) msg = `已上传 ${files} 个文件`;
  else msg = `已创建 ${createdDirs} 个目录`;
  toast(msg, { type: "success", duration: 4000 });
}

/** 冲突对话框的三个出口。 */
function resolveDropConflict(choice: "merge" | "skip" | "cancel"): void {
  const pending = pendingDrop.value;
  pendingDrop.value = null;
  if (!pending) return;
  if (choice === "cancel") return;
  let paths = pending.paths;
  if (choice === "skip") {
    const names = new Set(pending.conflictNames);
    paths = pending.paths.filter((p) => !names.has(basenameOf(p)));
    if (paths.length === 0) {
      toast("已存在的目录均已跳过，没有需要上传的内容", { type: "info", duration: 4000 });
      return;
    }
  }
  void beginDropUpload(paths, pending.dest);
}

/** 传输监听是否已被 closeDropTransfer 摘除（供 beginDropUpload 在订阅
 * 到位的瞬间判断自己是不是迟到了）。 */
let dropListenersDetached = false;

function startDropTick(): void {
  stopDropTick();
  dropTickNow.value = Date.now();
  dropTickTimer = setInterval(() => {
    dropTickNow.value = Date.now();
  }, 1000);
}


function stopDropTick(): void {
  if (dropTickTimer !== null) {
    clearInterval(dropTickTimer);
    dropTickTimer = null;
  }
}
function closeDropTransfer(): void {
  stopDropTick();
  dropUnlisten.forEach((u) => u());
  dropUnlisten = [];
  // 让「订阅还在路上」的 beginDropUpload 自行摘除刚拿到的监听。
  dropListenersDetached = true;
  dropTransfer.value = null;
}
function onCancelDropTransfer(): void {
  const tr = dropTransfer.value;
  if (!tr) return;
  sftpCancelTransfer(tr.requestId).catch(() => {});
}
/** 复制安装指引到剪贴板（安装指引对话框的「复制」按钮）。 */
function copyInstallHint(): void {
  const hint = capsUnavailable.value?.hint;
  if (!hint) return;
  void navigator.clipboard
    .writeText(hint)
    .then(() => toast("已复制安装命令", { type: "success", duration: 2500 }))
    .catch(() => toast("复制失败，请手动选择命令复制", { type: "warning", duration: 3000 }));
}

// Screenshot 状态——迁移到 myui 的 toast 后只剩"正在截取"这一瞬时态：
// 成功/失败两种结果都改由 toast 承载（成功带「打开」动作，失败带类型色），
// 不再需要手工 4s 定时器与自绘横幅。
const screenshotCapturing = ref(false);

// ── 主题 / 字体 / 背景图（computed：预设或深浅切换即重算） ──
const activePalette = computed(() => getActivePalette());
const terminalTheme = computed(() =>
  (ui.isDark ? activePalette.value.dark : activePalette.value.light).terminal,
);
// 终端背景恒透明（底色由 .terminal-wrap 的玻璃承载），渲染器选择不再依赖
// 「有没有背景图」—— 见 useRendererPref.resolveRenderer 的说明。
// Per-connection override wins over the global setting; empty → global.
const fontFamily = computed(() =>
  props.fontOverride ? resolveFontStack(props.fontOverride) : resolveFontStack(primaryFont.value),
);
// ── 终端底色：与侧栏/顶栏/命令栏共用同一套玻璃 ──
// 这里曾经是「设了壁纸且实心度 <1 → 纯透明，否则用配色预设的写死底色」
// 的二选一。后果是终端区与菜单彻底割裂：菜单能拉到几乎全透，终端却压着
// 一块 preset 写死的板子（如 #0d1117），中间没有任何档位；一旦设壁纸又
// 干脆全透，文字直接压在壁纸上没有兜底层。
//
// 现在改成 ompDesktop 的方式：**底色交给 .terminal-wrap 这一层 div**，
// 它用 var(--glass-bg)（颜色随主题色相、alpha 随界面实心度滑杆，与侧栏
// 顶栏完全同源）；xterm 只负责画字（theme.background 恒透明、
// allowTransparency 常开）。配色预设从此只管 16 色与光标，不再决定终端底色。
// 铺在哪一层见样式段 .terminal-wrap 的注释（避免与工作区双层叠加）。

// Route send/resize to the ssh_* or local_* backend depending on the tab's
// connection type. Read live from props so the closures bound in
// setupTerminal always hit the right backend (connType is fixed per tab in
// practice, but the live read keeps it honest).
const sendTo = (sid: string, data: string): Promise<void> =>
  props.connType === "local" ? localSend(sid, data) : sshSend(sid, data);
const resizeTo = (sid: string, cols: number, rows: number): Promise<void> =>
  props.connType === "local" ? localResize(sid, cols, rows) : sshResize(sid, cols, rows);

/** 取消 ZMODEM 会话的统一出口：发 abort + 立刻本地复位 + 2s 兜底。
 *
 * 三个动作缺一不可：
 * - 发 abort 让 Rust 走 CAN×8、切回 Normal、发 zmodem_end、清 sender；
 * - 本地立刻复位 isZmodem —— `onData` 在它为 true 时吞掉所有按键，多等
 *   一拍就是「终端不可操作」；
 * - 2s 兜底防 zmodem_end 丢失（IPC 失败 / 会话已死）。Rust 的 abort 分支
 *   正常会在一个 IPC 往返内回 end，到时 onZmodemEnd 会清掉这个定时器。
 *
 * `targetSid`：setupTerminal 闭包里的事件处理器要传当时的 `sid` ——
 * 组件重建后 props.sessionId 已换新值，迟到的旧事件不该误伤新会话。
 */
function abortZmodemSession(targetSid: string = props.sessionId): void {
  isZmodem = false;
  dragZmodemActive.value = false;
  sshSendZmodemAbort(targetSid).catch(() => {});
  if (abortTimeout) clearTimeout(abortTimeout);
  abortTimeout = setTimeout(() => {
    isZmodem = false;
    dragZmodemActive.value = false;
    abortTimeout = null;
    term?.write("\r\n\x1b[33m[abort 超时 — 强制重置]\x1b[0m\r\n");
  }, 2000);
}

/** Hand the picked files/folders to the native Rust uploader, or abort
 * when the user dismissed the picker without a selection. */
async function commitNativeUpload(paths: string[]): Promise<void> {
  if (paths.length === 0) {
    abortZmodemSession();
    return;
  }
  try {
    await zmodemStartUpload(props.sessionId, paths);
    // Activate the progress overlay. bytesTotal starts at 0 — the first
    // zmodem_progress event carries the real file size (the backend
    // stats/expands the selection). onZmodemProgress ignores events while
    // active is false, so this must be set before bytes flow.
    const firstName = paths[0].split(/[\\/]/).pop() || "file";
    zmodemStatus.value = {
      active: true,
      direction: "upload",
      currentFile: paths.length === 1 ? firstName : `${firstName} 等 ${paths.length} 项`,
      bytesTransferred: 0,
      bytesTotal: 0,
      speedBps: 0,
      error: null,
      startTime: Date.now(),
    };
  } catch (e) {
    console.error("native upload start failed:", e);
    abortZmodemSession();
  }
}

async function pickUploadFiles(): Promise<void> {
  await nudgeMouseCursor().catch(() => {});
  let selected: string | string[] | null = null;
  try {
    selected = await open({ multiple: true });
  } catch (e) {
    // 对话框本身失败也不能搁浅会话：走空选择 = 取消。
    console.error("upload file picker failed:", e);
  }
  const paths: string[] = !selected ? [] : Array.isArray(selected) ? selected : [selected];
  await commitNativeUpload(paths);
}

function cancelNativeUpload(): void {
  abortZmodemSession();
}

/** Capture the terminal viewport and save it to the attachment directory.
 * Bound to the 📷 button in CommandBar. No-op if the xterm instance isn't
 * mounted yet (rare race during tab open). */
async function handleScreenshot(): Promise<void> {
  const t = term;
  if (!t) {
    toast("终端尚未就绪，请稍候重试", { type: "error", duration: 4000 });
    return;
  }
  screenshotCapturing.value = true;

  try {
    const dataUrl = await captureTerminalToDataUrl(t);
    if (!dataUrl) {
      toast("截图失败：无法捕获终端画面", { type: "error", duration: 4000 });
      return;
    }
    const path = await saveScreenshot(dataUrl, props.connectionName || props.sessionId);
    // 带 action 的 toast 默认 duration=0（避免动作来不及点），这里显式给时长；
    // 「打开」是用户真正会去点的动作，给足 8s（旧的横幅只有 4s 且要精准命中按钮）。
    toast(`已保存：${path}`, {
      type: "success",
      duration: 8000,
      action: { label: "打开", onClick: () => openSavedPath(path) },
    });
  } catch (e: unknown) {
    const msg = e instanceof Error && e.message ? String(e.message) : String(e);
    // The Rust side returns a localized error when the attachment dir isn't
    // configured — surface it verbatim so the user knows what to fix.
    toast(msg.includes("附件目录") ? msg : `保存失败: ${msg}`, { type: "error", duration: 4000 });
  } finally {
    screenshotCapturing.value = false;
  }
}

/** 打开已保存截图所在目录（资源管理器中选中该文件）。 */
async function openSavedPath(path: string): Promise<void> {
  try {
    if (path) await showInFolder(path);
  } catch {
    /* best-effort — ignore if open fails */
  }
}

// 2026-10-10：命令栏被提升到壳层底部行后，它对所有 tab 都可见，但「📷 截图」
// 只有拥有那一个 xterm 实例的 TerminalPanel 能做 —— 截图要用闭包里的 term，
// 过程态（「正在截取终端…」）也挂在本组件上。故把动作注册进 store，壳层按
// sessionId 调用。
//
// 必须 watch sessionId 而不是只在 onMounted 注册：重连会换 sessionId
// （tabId 才是跨重连稳定的标识），只在挂载时注册的话，重连后壳层按新
// sessionId 查不到动作，截图会静默失灵。
const runScreenshot = (): void => void handleScreenshot();

/** 手动修复远端终端设置。
 *
 * 远端程序（lrzsz 的 rz/sz、崩溃的 vim、`cat` 二进制文件）可能把 tty 留在
 * raw 模式：不回显、readline 收不到整行、退格与方向键全乱，只有回车能出东西。
 * 客户端侧无法感知，只能靠用户手动触发。发 Ctrl-C（中断残留前台进程并让
 * readline 放弃当前行）+ `stty sane`（恢复 ECHO/ICANON/ISIG）。
 * 前导空格：配了 HISTCONTROL=ignorespace 的机器上不进历史。 */
function repairTerminal(): void {
  sendTo(props.sessionId, "\x03").catch(() => {});
  void sendTo(props.sessionId, " stty sane\r").catch(() => {});
  term?.write("\r\n\x1b[33m[终端] 已发送 Ctrl-C + stty sane\x1b[0m\r\n");
}

const runRepairTerminal = (): void => repairTerminal();

watch(
  () => props.sessionId,
  (sid, oldSid) => {
    if (oldSid) unregisterSessionAction(oldSid, "screenshot");
    if (oldSid) unregisterSessionAction(oldSid, "repair-terminal");
    registerSessionAction(sid, "screenshot", runScreenshot);
    registerSessionAction(sid, "repair-terminal", runRepairTerminal);
  },
  { immediate: true },
);

onUnmounted(() => {
  unregisterSessionAction(props.sessionId, "screenshot");
  unregisterSessionAction(props.sessionId, "repair-terminal");
});

// ── Native ZMODEM download 辅助（目录记忆 + 路径净化） ──

// Join the picked dir with an offer name. `sz -r 目录` offers carry
// relative subpaths ("dir/sub/file.txt") — sanitize each segment and
// keep the tree: empty / "." / ".." / drive-letter segments are dropped,
// so the path can never escape the picked directory. All segments gone
// (pathological name) → benign flat fallback, same as before.
function joinZmodemPath(dir: string, name: string): string {
  const sep = dir.includes("/") && !dir.includes("\\") ? "/" : "\\";
  const trimmed = dir.endsWith(sep) ? dir.slice(0, -1) : dir;
  const segs = name
    .split(/[\\/]/)
    .map((s) => s.trim().replace(/[. ]+$/, ""))
    .filter((s) => s && s !== "." && s !== ".." && !s.includes(":"));
  if (segs.length === 0) return `${trimmed}${sep}myshell-download.bin`;
  return `${trimmed}${sep}${segs.join(sep)}`;
}

async function promptNativeDir(): Promise<string | null> {
  if (nativeCancelled) return null;
  if (nativeDir !== null) return nativeDir;
  // The picker opens right after the user typed `sz …` — nudge the
  // pointer visible first (WebView2 "hide pointer while typing" keeps
  // it invisible over native dialogs, see nudgeMouseCursor docs).
  await nudgeMouseCursor().catch(() => {});
  const dir = await open({ directory: true });
  if (typeof dir === "string" && dir.length > 0) {
    nativeDir = dir;
    return dir;
  }
  nativeCancelled = true;
  return null;
}

// ── 终端生命周期：一次 setup = 旧版 [sessionId] effect 的一次运行 ──

function setupTerminal(): void {
  const container = containerRef.value;
  if (!container) return;

  // 本轮生命周期绑定的会话 id（旧版 effect 闭包捕获的 sessionId prop）。
  const sid = props.sessionId;
  // 每轮独立的关闭标记：dispose 之后，仍在途的事件订阅 Promise 回调据此
  // 自行 unlisten（与旧版 effect 局部 `closed` 语义一致）。
  let closed = false;
  let firstOutputHandled = false;
  const firstSyncTimers: Array<ReturnType<typeof setTimeout>> = [];

  let unlistenOutput: UnlistenFn | null = null;
  let unlistenClosed: UnlistenFn | null = null;
  let unlistenZmodemStart: UnlistenFn | null = null;
  let unlistenZmodemRaw: UnlistenFn | null = null;
  let unlistenZmodemEnd: UnlistenFn | null = null;
  let unlistenZmodemError: UnlistenFn | null = null;
  let unlistenZmodemOffer: UnlistenFn | null = null;
  let unlistenZmodemProgress: UnlistenFn | null = null;
  let unlistenZmodemFileComplete: UnlistenFn | null = null;
  let unlistenZmodemFileSkipped: UnlistenFn | null = null;

  // 拖拽上传的 cwd 探针等待者：filterSentinel 在输出流里截到哨兵行时
  // 逐个 resolve。生命周期绑定本轮终端（dispose 时清空并以 null 收场）。
  const cwdWaiters: Array<(cwd: string | null) => void> = [];
  // 窗口级拖放事件的 unlisten；dispose 时统一取消。
  let dragEventUnlisten: UnlistenFn[] = [];

  const t = new Terminal({
    cursorBlink: true,
    // Bar cursor (a 1-cell-wide vertical line) instead of the default solid
    // block. The bar stays continuously visible even where a filled block
    // would blend into a similar-colored background, and it doesn't depend
    // on focus to render (the DOM renderer only paints the block cursor
    // while focused). Pairs with forceVisibleCursor() — that owns the
    // color, this owns the shape. cursorWidth is in CSS px (1 is a hairline;
    // 2 reads clearly without eating the next glyph).
    cursorStyle: "bar",
    cursorWidth: 2,
    fontSize: terminalFontSize.value,
    fontFamily: fontFamily.value,
    theme: {
      ...terminalTheme.value,
      ...forceVisibleCursor(terminalTheme.value),
      selectionBackground: visibleSelection(terminalTheme.value.selectionBackground),
      // 恒定透明：底色由 .terminal-wrap 的 var(--glass-bg) 承载，
      // xterm 若再画一层实底，玻璃就被彻底挡住（见 .terminal-wrap 处注释）。
      background: "rgba(0, 0, 0, 0)",
    },
    allowProposedApi: true,
    // 常开：xterm 背景透明需要它，且与实心度无关 —— 底色现在恒由玻璃 div 提供。
    allowTransparency: true,
  });

  const fit = new FitAddon();
  const webLinksAddon = new WebLinksAddon();

  t.loadAddon(fit);
  t.loadAddon(webLinksAddon);
  t.open(container);

  // Renderer choice drives the "cursor / selection invisible" reports, so
  // it's user-overridable (see useRendererPref). Default behavior:
  //  • Background image set → WebGL renderer. It redraws every frame, which
  //    is required for clean compositing under allowTransparency: the canvas
  //    renderer leaves ghosts/smearing on transparent backgrounds (input
  //    chars appear to "jump", worst on the local ConPTY path).
  //  • No background image → Canvas renderer (addon-canvas). It paints the
  //    cursor AND the selection directly onto the same canvas as the text —
  //    unlike the WebGL renderer, which draws the cursor on a SEPARATE
  //    transparent 2D-canvas overlay (xtermjs/xterm.js#2614) that can fail
  //    to composite on some GPU/driver + WebView2 combos, making the cursor
  //    and selection never appear. The canvas renderer has no such overlay,
  //    so it's the most robust against this whole class of bug. It also
  //    beats the xterm 5.x default DOM renderer, which only shows the cursor
  //    while focused and has its own known cursor bugs (#3271).
  //  • The DOM renderer is opt-in only (for a user who hits trouble with the
  //    other two). It's the lightest, no-canvas path.
  // Falls back to canvas if a forced WebGL/Canvas renderer isn't usable
  // (old GPU/drivers). Toggling the background-image setting or the
  // renderer pref after open needs a tab reopen to switch renderers (this
  // runs once per terminal build).
  const renderer = resolveRenderer(props.rendererBackend);
  const loadRenderer = (): void => {
    if (renderer === "webgl") {
      try {
        t.loadAddon(new WebglAddon());
        return;
      } catch (e) {
        console.warn("[TerminalPanel] WebGL renderer unavailable, falling back to canvas:", e);
      }
    }
    if (renderer === "dom") {
      // No addon to load — xterm 5.x's built-in DOM renderer is the default
      // when no renderer addon is registered. Intentionally load nothing.
      return;
    }
    // canvas (default + webgl/canvas fallback)
    try {
      t.loadAddon(new CanvasAddon());
    } catch (e) {
      // Extremely unlikely (canvas is the most compatible), but don't let a
      // renderer failure kill the terminal — fall through to the DOM default.
      console.warn("[TerminalPanel] Canvas renderer unavailable, using DOM default:", e);
    }
  };
  loadRenderer();

  // Renderer readiness gate: after loadRenderer() swaps in the Canvas/WebGL
  // renderer, xterm's _renderService is briefly undefined during the
  // transition. Any operation that triggers Viewport.syncScrollArea (write,
  // scroll, ResizeObserver) in that gap throws "Cannot read properties of
  // undefined (reading 'dimensions')" — which crashes WebView2's render
  // process, freezing the entire UI. We buffer SSH data until two animation
  // frames pass (ensuring the renderer has settled) or 300ms elapses.
  let rendererReady = false;
  const pendingData: Uint8Array[] = [];
  const flush = () => {
    rendererReady = true;
    for (const d of pendingData) {
      try {
        t.write(d);
      } catch {
        /* renderer still settling */
      }
    }
    pendingData.length = 0;
  };
  requestAnimationFrame(() => requestAnimationFrame(() => flush()));
  // Safety net: if rAF never fires (background tab), force-flush after 300ms.
  setTimeout(flush, 300);

  // Block OSC 52 clipboard writes from the remote side. Without this,
  // a malicious SSH server can silently replace the user's clipboard
  // (e.g., swap a wallet address) by emitting the OSC 52 escape sequence.
  // Returning true tells xterm the sequence is fully handled — no
  // clipboard mutation occurs.
  t.parser.registerOscHandler(52, () => true);

  setTimeout(() => {
    // Guard against xterm.js race: fit() calls Viewport.syncScrollArea
    // which accesses renderer.dimensions — if the renderer isn't ready
    // yet (canvas init async), this throws "Cannot read properties of
    // undefined (reading 'dimensions')". Wrap in try/catch so the error
    // doesn't crash the WebView2 render process.
    try {
      fit.fit();
    } catch {
      // Retry once after a longer delay — by then the renderer should be ready.
      setTimeout(() => {
        try {
          fit.fit();
        } catch {
          /* give up */
        }
      }, 300);
    }
    resizeTo(props.sessionId, t.cols, t.rows).catch(() => {});
  }, 100);

  // ── sz/rz 无声失败的提示 ────────────────────────────────────────────────
  // lrzsz 的 sz 对"文件不存在"是**静默**退出：实测 `sz 123.5xt` 退出码 128、
  // 一个字节都不输出（连 ZDLE 都没有），所以后端状态机压根不会被唤醒，
  // 拿不到任何信号——终端上只表现为敲完命令直接回到提示符，用户无法区分
  // "传完了"、"失败了"还是"命令打错了"。这里在客户端补一个兜底提示：敲了
  // sz/rz 之后若 ZMODEM 会话迟迟没启动，就告诉用户最可能的原因。
  // 纯提示行（不弹窗、不改变终端状态），误报代价很低。
  const ZMODEM_SILENT_HINT_MS = 6000;
  // sawSession 用"本次意图之后是否真的起过会话"来判断，而不是看当前
  // isZmodem —— 无权限那种场景会话会在 1 秒内起又立刻结束，用实时状态
  // 会误判成"没起过"从而多打一条提示。
  let zmodemIntentTimer: ReturnType<typeof setTimeout> | null = null;
  let sawZmodemSession = false;

  function clearZmodemIntentHint(): void {
    if (zmodemIntentTimer !== null) {
      clearTimeout(zmodemIntentTimer);
      zmodemIntentTimer = null;
    }
  }

  /** 任何 ZMODEM 会话信号（start / offer）都算"起过了"，撤销待发提示。 */
  function markZmodemSessionStarted(): void {
    sawZmodemSession = true;
    clearZmodemIntentHint();
  }

  function noteZmodemIntent(cmd: string): void {
    clearZmodemIntentHint();
    // 只认整条命令就是 sz / rz 及其参数，避免 `echo sz`、`ls | sz` 之类的
    // 误伤（那类命令同样不会起会话，但用户并不需要这个提示）。
    if (!/^\s*(sz|rz)(\s|$)/.test(cmd)) return;
    sawZmodemSession = false;
    zmodemIntentTimer = setTimeout(() => {
      zmodemIntentTimer = null;
      if (sawZmodemSession || closed) return;
      t.write(
        "\r\n\x1b[33m[ZMODEM 提示] 未检测到远端发起传输。lrzsz 的 sz/rz 在文件不存在时" +
          "会静默退出（无任何输出）；请确认文件名与路径是否正确。\x1b[0m\r\n",
      );
    }, ZMODEM_SILENT_HINT_MS);
  }

  // Handle user input. In ZMODEM mode, swallow keystrokes so the user
  // can't corrupt the protocol stream by typing into the terminal.
  // When broadcast targets are configured, mirror the keystrokes to all
  // of them in parallel — the local session is included in the list so
  // a single loop handles both single + multi target cases.
  t.onData((data) => {
    // ZMODEM 会话期间吞掉键盘输入，防止用户敲键污染协议流。
    // 唯一例外是 Ctrl+C（\x03）：会话卡住（rz 挂起、zmodem_end 丢失）时
    // 这是用户手里唯一的自救手段。lrzsz 的 canit() 本身就以 CAN 序列响应
    // SIGINT，把它交给 shell 反而能中断挂住的 rz。
    if (isZmodem && data !== "\x03") return;
    // Ctrl+C 在 ZMODEM 期间还要额外补一次 abort：光把字节转给 shell 未必
    // 能唤醒已经卡死的 rz，而 Rust 侧的 ZmodemAbort 会发完整 CAN 序列并
    // 立刻切回 Normal + 发 zmodem_end。
    if (isZmodem && data === "\x03") {
      // 统一出口（复位 + abort + 2s 兜底）；\x03 字节本身仍照常发往远端
      // （下面的 sendTo）—— 直接中断远端前台进程是它份内的活。
      abortZmodemSession();
    }
    // 分屏：非焦点叶子只渲染不发送。PTY 是单个会话，两个 xterm 同时
    // 往里写会让光标位置错乱、命令交叉。
    if (props.inputEnabled === false) return;
    const targets = props.broadcastTargets;
    const destinations = targets.length > 0 ? targets : [props.sessionId];
    // Fire-and-forget. Promise.allSettled already swallows per-target
    // rejections (a target session may have just closed), so there's
    // nothing to do on completion.
    void Promise.allSettled(destinations.map((targetSid) => sendTo(targetSid, data)));

    // Record keystrokes for command-history. This runs AFTER the send
    // so we don't block the critical path. We only record for the
    // local tab's connectionId (not broadcast targets) — each tab
    // records its own perspective.
    if (props.connectionId) {
      recordKeystroke(data, cmdBufRef, ansiEscRef, (cmd) => {
        noteZmodemIntent(cmd);
        addCommandHistory(props.connectionId, cmd)
          .then(() => {
            refreshCommandHistory(props.connectionId);
          })
          .catch(() => {
            // Silently ignore history-write failures (not critical).
          });
      });
    }
  });

  const resizeObserver = new ResizeObserver(() => {
    // Guard against transient zero/tiny container sizes. We've seen the
    // terminal cols collapse mid-session (ls going from multi-column to
    // one-per-line, PS1 truncated to "user@host") — symptom of fit()
    // reading a momentarily-collapsed container (Sidebar collapse
    // animation, ServerInfoPanel mount transition, HMR re-render, etc.)
    // and shrinking cols to garbage. Skip the fit entirely when the
    // container is implausibly small rather than poison the shell.
    const c = containerRef.value;
    if (!c) return;
    const w = c.clientWidth;
    const h = c.clientHeight;
    if (w < 80 || h < 40) {
      console.warn(`[TerminalPanel] skipping fit — container too small (${w}x${h})`);
      return;
    }
    const prevCols = t.cols;
    try {
      fit.fit();
    } catch (e) {
      console.warn("[TerminalPanel] fit() threw:", e);
      return;
    }
    // Log suspicious shrinkage for diagnosis. Normal fits don't drop cols
    // by more than a few; a 60→11 drop is the bug we're hunting.
    if (t.cols < prevCols - 10) {
      console.warn(`[TerminalPanel] cols shrank ${prevCols}→${t.cols} at container ${w}x${h}`);
    }
    // 分屏：非焦点叶子 fit 完就停。本地栅格已经跟着容器变了，但远端的
    // window size 只能有一个值 —— 推第二次会把上一个叶子的尺寸覆盖掉。
    if (props.resizeEnabled === false) return;
    resizeTo(props.sessionId, t.cols, t.rows).catch(() => {});
    for (const targetSid of props.broadcastTargets) {
      if (targetSid !== props.sessionId) {
        resizeTo(targetSid, t.cols, t.rows).catch(() => {});
      }
    }
  });
  resizeObserver.observe(container);

  const b = new ZmodemBridge(sid);
  bridge = b;
  const unsubscribeStatus = b.onStatus((s) => {
    zmodemStatus.value = s;
  });

  // ── 哨兵行过滤 ────────────────────────────────────────────────────
  // MCP 的 `__MCP_DONE_<rand>__:$?` 与拖拽上传的 cwd 探针
  // `__MYSHLL_CWD_<rand>__$PWD` 都会在终端里留下用户不需要的回显行，
  // 统一由 utils/sentinel-filter 在数据进 xterm 前删掉。cwd 值经回调
  // 旁路取出给 cwdWaiters（同一个 pass 里取值 + 删行，不会分叉）。
  // 规则与踩过的坑见该模块的文档注释。
  const sentinel = createSentinelFilter((cwd) => {
    if (cwdWaiters.length === 0) return;
    const waiters = cwdWaiters.splice(0);
    for (const w of waiters) w(cwd);
  });
  const filterSentinel = (data: Uint8Array): Uint8Array | null => sentinel.feed(data);

  onSshOutput(sid, (data) => {
    if (closed) return;
    const filtered = filterSentinel(data);
    if (!filtered) return; // entire chunk was sentinel noise — skip
    if (!rendererReady) {
      pendingData.push(filtered);
      return;
    }
    t.write(filtered);
    if (!firstOutputHandled) {
      firstOutputHandled = true;
      // CRITICAL — push the real cols to the PTY AFTER the shell settles.
      // The PTY starts at 80×24 (main.rs local_connect) and the shell
      // (PSReadLine on pwsh, readline on bash) caches those cols. We MUST
      // overwrite them with the real width once the shell is ready; miss
      // the window and the backend stays at 80 while the frontend is e.g.
      // 120. Input past col 80 then makes the backend wrap while the
      // frontend doesn't, so PSReadLine repaints the edit line into the
      // wrong cells — characters "jump out" / the background "shifts left",
      // recovering only after Enter (a fresh prompt is a single line).
      // This is the classic xterm↔PTY cols desync.
      //
      // The first output frame means the shell drew its prompt (PSReadLine
      // is up), but its resize listener needs a beat to fully take over —
      // and the mount-time 100ms fit/resize usually lands BEFORE PSReadLine
      // initializes, so that one is lost. Re-fit + resize now AND on two
      // increasing delays so a slow shell init can't strand us at 80 cols.
      // (The community writeup that pinned this exact symptom used a 200ms
      // delay; we cover 0/250/600ms for varying shell cold-start times.)
      const syncRealCols = () => {
        try {
          fit.fit();
        } catch {
          /* container mid-transition */
        }
        resizeTo(props.sessionId, t.cols, t.rows).catch(() => {});
      };
      syncRealCols();
      firstSyncTimers.push(setTimeout(syncRealCols, 250), setTimeout(syncRealCols, 600));
    }
  })
    .then((un) => {
      if (closed) un();
      else unlistenOutput = un;
      // Listeners are attached — release the backend's startup hold so the
      // login banner (MOTD / "Last login") captured during connect is
      // delivered. Local tabs have no hold; ssh_ready is a no-op there but
      // skip the IPC anyway.
      if (props.connType !== "local") {
        sshReady(props.sessionId).catch(() => {});
      }
    })
    .catch((e) => console.error("Failed to subscribe to ssh_output:", e));

  onSshClosed(sid, () => {
    if (closed) return;
    closed = true;
    t.write("\r\n\x1b[31m[Connection closed]\x1b[0m\r\n");
    t.options.cursorBlink = false;
    emit("disconnected");
  })
    .then((un) => {
      if (closed) un();
      else unlistenClosed = un;
    })
    .catch((e) => console.error("Failed to subscribe to ssh_closed:", e));

  // ZMODEM — Rust has already filtered terminal output from protocol bytes,
  // so zmodem_raw only fires when a session is actually starting.
  //
  // Both downloads (remote `sz`) and uploads (remote `rz`) are handled
  // natively in Rust. The frontend only handles UI (file picker / save dir
  // + progress). zmodem_raw is kept for the subscription lifecycle but no
  // longer fed to the JS bridge.
  onZmodemStart(sid, (direction) => {
    isZmodem = true;
    markZmodemSessionStarted();
    nativeDownload = direction === "download";
    if (direction === "download") {
      nativeDir = null;
      nativeCancelled = false;
    } else if (direction === "upload") {
      // 拖拽上传的自动路径：等的就是这一刻。把挂起的路径直接交给
      // zmodemStartUpload（它自己点亮进度浮层），不弹文件选择器。
      if (autoUploadPaths.value !== null) {
        const paths = autoUploadPaths.value;
        autoUploadPaths.value = null;
        if (rzWatchdog !== null) {
          clearTimeout(rzWatchdog);
          rzWatchdog = null;
        }
        void commitNativeUpload(paths);
      } else if (dragZmodemActive.value) {
        // 同一轮拖拽上传还在进行（远端 rz 被拒后重新邀请 / 多文件间隙的
        // ZRINIT），此时弹选择器等于把用户拽回手动流程。主动收场。
        //
        // 必须立刻自清：dragZmodemActive 是模块级 ref，若上一次拖拽上传
        // 的清理路径漏了（看门狗未触发 / onZmodemEnd 未达），残留的 true
        // 会把用户此后每一次手动 `rz` 都吃掉 —— abort → end → rz 再邀请
        // → 再 abort，形成截图里那种「开始/结束」无限循环，选择器永远
        // 不弹。自清后最多误吃一次。
        dragZmodemActive.value = false;
        cancelNativeUpload();
        t.write("\r\n\x1b[33m[拖拽上传] 传输已结束\x1b[0m\r\n");
      } else {
        // 手动 rz：直接弹系统文件选择框，不再经过内嵌的「文件/文件夹」
        // 二选一中间层（用户反馈：多一步徒增复杂且是卡死多发地）。关闭
        // 对话框不选 = 取消（commitNativeUpload 的空路径走 abortZmodemSession）。
        // 文件夹上传走拖拽路径（拖入即递归），不在这条手动路径上保留。
        void pickUploadFiles();
      }
    }
    t.write("\r\n\x1b[36m[ZMODEM 传输开始 — 终端输入已屏蔽]\x1b[0m\r\n");
  })
    .then((un) => {
      if (closed) un();
      else unlistenZmodemStart = un;
    })
    .catch((e) => console.error("Failed to subscribe to zmodem_start:", e));

  onZmodemRaw(sid, (_data) => {
    // Native Rust handles both upload and download protocol bytes.
    // Nothing to feed to the JS bridge.
  })
    .then((un) => {
      if (closed) un();
      else unlistenZmodemRaw = un;
    })
    .catch((e) => console.error("Failed to subscribe to zmodem_raw:", e));

  // Native download: a file is offered — prompt for dir (once), accept/skip.
  onZmodemOffer(sid, (p) => {
    markZmodemSessionStarted();
    void (async () => {
      try {
        const dir = await promptNativeDir();
        const path = dir !== null ? joinZmodemPath(dir, p.fileName) : null;
        if (path !== null) {
          zmodemStatus.value = {
            active: true,
            direction: "download",
            currentFile: p.fileName,
            bytesTransferred: 0,
            bytesTotal: p.fileSize,
            speedBps: 0,
            error: null,
            startTime: Date.now(),
          };
        }
        await zmodemAcceptOffer(props.sessionId, path);
      } catch (e) {
        // 拿不到目录 / IPC 失败时必须主动收场。接收端此时停在 WaitingAccept，
        // 而该状态被有意排除在空闲超时之外（目录选择器可以合法地开着几分钟），
        // 所以这里一旦不吭声，会话就永远停在 ZMODEM 模式：终端吞掉所有后续
        // 输出、按键也进不去，用户只能 Ctrl+C。先发取消序列再报给用户。
        console.error("zmodemAcceptOffer failed:", e);
        // 统一出口：这里原先只发 abort 不复位 isZmodem 也不设兜底，
        // zmodem_end 丢失时下载方终端同样「能看不能打」。
        abortZmodemSession(sid);
        t.write(
          `\r\n\x1b[31m[ZMODEM 错误] 无法选择保存目录或提交失败，已取消本次传输：${e}\x1b[0m\r\n`,
        );
      }
    })();
  })
    .then((un) => {
      if (closed) un();
      else unlistenZmodemOffer = un;
    })
    .catch((e) => console.error("Failed to subscribe to zmodem_offer:", e));

  // Native download: live byte progress.
  onZmodemProgress(sid, (p) => {
    const prev = zmodemStatus.value;
    if (!prev.active) return;
    const elapsed = (Date.now() - prev.startTime) / 1000;
    const speed = elapsed > 0 ? p.bytesTransferred / elapsed : 0;
    zmodemStatus.value = {
      ...prev,
      bytesTransferred: p.bytesTransferred,
      bytesTotal: p.bytesTotal,
      speedBps: speed,
    };
  })
    .then((un) => {
      if (closed) un();
      else unlistenZmodemProgress = un;
    })
    .catch((e) => console.error("Failed to subscribe to zmodem_progress:", e));

  // Native download: single file finished — update progress to 100%.
  onZmodemFileComplete(sid, (p) => {
    const prev = zmodemStatus.value;
    if (!prev.active) return;
    zmodemStatus.value = {
      ...prev,
      bytesTransferred: p.bytesWritten,
      bytesTotal: p.bytesWritten,
      currentFile: p.fileName,
    };
  })
    .then((un) => {
      if (closed) un();
      else unlistenZmodemFileComplete = un;
    })
    .catch((e) => console.error("Failed to subscribe to zmodem_file_complete:", e));

  // Receiver declined a file (ZSKIP). Not fatal — the transfer continues
  // with the remaining files (rz skips an existing file unless `-y`).
  // Tell the user which file never landed instead of silently dropping it.
  onZmodemFileSkipped(sid, (p) => {
    t.write(
      `\r\n\x1b[33m[ZMODEM] ${p.fileName} 被远端跳过（同名文件已存在）\x1b[0m\r\n`,
    );
  })
    .then((un) => {
      if (closed) un();
      else unlistenZmodemFileSkipped = un;
    })
    .catch((e) => console.error("Failed to subscribe to zmodem_file_skipped:", e));

  onZmodemEnd(sid, () => {
    isZmodem = false;
    nativeDownload = false;
    nativeDir = null;
    nativeCancelled = false;
    dragZmodemActive.value = false;
    b.reset();
    // Cancel any pending force-reset — the Rust backend reported an
    // orderly ZFIN/CAN sequence so the bridge is clean.
    if (abortTimeout) {
      clearTimeout(abortTimeout);
      abortTimeout = null;
    }
    t.write("\r\n\x1b[36m[ZMODEM 传输结束]\x1b[0m\r\n");
  })
    .then((un) => {
      if (closed) un();
      else unlistenZmodemEnd = un;
    })
    .catch((e) => console.error("Failed to subscribe to zmodem_end:", e));

  // Native transfer: Rust-side fatal error (remote abort burst, ZABORT
  // frame, idle timeout). Write it to the terminal so the user sees WHY —
  // the overlay may not even be active yet (sz dies before offering the
  // first file when it can't read it), and the following zmodem_end hides
  // the overlay anyway.
  onZmodemError(sid, (message) => {
    zmodemStatus.value = { ...zmodemStatus.value, error: message };
    t.write(`\r\n\x1b[31m[ZMODEM 错误] ${message}\x1b[0m\r\n`);
    // error 语义 = 传输已终结。isZmodem 若不复位，onData 全部被吞 ——
    // zmodem_end 丢失/迟到时终端表现为「能看不能打」（用户实报）。
    // Rust 侧 error 后通常还发 end（那里会再清一次，重复复位幂等）。
    isZmodem = false;
    nativeDownload = false;
    nativeDir = null;
    nativeCancelled = false;
    dragZmodemActive.value = false;
  })
    .then((un) => {
      if (closed) un();
      else unlistenZmodemError = un;
    })
    .catch((e) => console.error("Failed to subscribe to zmodem_error:", e));

  term = t;
  fitAddon = fit;

  // Restore scrollback from a previous terminal instance after a reconnect.
  // The snapshot was captured by reconnectOne (reading the old xterm's
  // buffer) before the sessionId changed. Write it now so the user sees
  // their history continuity, then mark it consumed.
  if (props.reconnectSnapshot) {
    t.write(props.reconnectSnapshot);
    t.write("\r\n\x1b[33m[—— 以上为重连前历史，连接已重建 ——]\x1b[0m\r\n");
    emit("snapshot-consumed");
  }

  // ── 拖拽上传：取远端 cwd ──────────────────────────────────────────
  // 往 PTY 注入一条带哨兵的 `echo __MCP_CWD_<rand>__$PWD`，从输出流
  // 截回真实 cwd（回显与结果行都被上面的 filterSentinel 挡掉，用户看不到）。
  // 复用 MCP sentinel 的既有通道，不新增后端命令。
  // 哨兵命名约束见 utils/sentinel-filter：必须含 "__MCP" 前缀，否则被
  // chunk 切开的半行不会被缓存；needle 与回显行的区分靠结果行正则。
  // 3s 超时 → resolve(null)，调用方回退 home（"~"，后端 resolve_path 处理）。
  const CWD_PROBE_TIMEOUT_MS = 3000;
  function probeRemoteCwd(): Promise<string | null> {
    if (cwdProbe) return cwdProbe;
    const token = Math.random().toString(36).slice(2, 10);
    cwdProbe = new Promise<string | null>((resolve) => {
      let timer: ReturnType<typeof setTimeout> | null = null;
      const done = (v: string | null) => {
        if (timer !== null) clearTimeout(timer);
        const i = cwdWaiters.indexOf(onHit);
        if (i >= 0) cwdWaiters.splice(i, 1);
        cwdProbe = null;
        resolve(v);
      };
      const onHit = (cwd: string | null) => done(cwd);
      cwdWaiters.push(onHit);
      timer = setTimeout(() => done(null), CWD_PROBE_TIMEOUT_MS);
      // 两条隔离手段叠加，覆盖不同的远端配置：
      // 1) 前导空格 —— HISTCONTROL=ignorespace 的机器直接跳过记录；
      // 2) `set +o history` 包裹 —— 没设 HISTCONTROL 时仍能阻止本条及后续
      //    命令写入历史（关闭动作发生在记录本条之前，故本条也不入史）。
      // 每个 set 都带 2>/dev/null：非 bash 登录 shell（sh/dash/fish）没有
      // history 选项，裸 set 会在用户终端打出 "Illegal option" 垃圾行。
      // 实测某台机器 HISTCONTROL 为空，仅靠空格无效，上下键仍会翻出探针。
      sendTo(
        sid,
        ` set +o history 2>/dev/null; echo __MCP_CWD_${token}__$PWD; set -o history 2>/dev/null\n`,
      ).catch(() => done(null));
    });
    return cwdProbe;
  }

  // ── 拖拽上传：通道选择与执行 ──────────────────────────────────────
  // 后端按「每个拖入项自己的名字」在远端重建目录树（递归到叶子、保留空目录）。
  // 通道选择：先探测服务端能力（SFTP 子系统 / lrzsz，按 session 缓存），
  // SFTP 可用走 SFTP（含同名目录弹窗），否则 lrzsz 可用走 ZMODEM（向 PTY
  // 发 rz，远端 rz 自己按相对路径重建目录树），两者皆无给安装指引。
  // 背景：SSH 连得上不等于 SFTP 用得了（restricted shell / chroot / 只开
  // exec 的跳板机），此前无条件走 SFTP 的结果是「SFTP session init failed:
  // Timeout」且没有任何下一步提示。
  async function startDropUpload(rawPaths: string[]): Promise<void> {
    if (rawPaths.length === 0) return;
    dropPreparing.value = true;
    try {
      await startDropUploadInner(rawPaths);
    } finally {
      dropPreparing.value = false;
    }
  }

  async function startDropUploadInner(rawPaths: string[]): Promise<void> {
    let caps: UploadCapabilities;
    try {
      caps = await probeUploadCapabilities(sid);
    } catch (e) {
      toast(`无法探测服务端上传能力：${e}`, { type: "error", duration: 6000 });
      return;
    }

    const cwd = await probeRemoteCwd();

    // 探测没跑完（exec 也不可用/超时）→ 能力未知，按旧逻辑试 SFTP，
    // 失败只能如实报错——此时不能建议装 lrzsz（可能装不上）。
    if (caps.sftpAvailable || !caps.probeCompleted) {
      await startDropUploadSftp(rawPaths, cwd, caps.zmodemAvailable && cwd !== null);
      return;
    }

    if (caps.zmodemAvailable) {
      // ZMODEM 在 PTY 的当前目录执行 rz；cwd 未知时不猜目录，直接取消
      // （此时往 PTY 发命令可能落进用户正开着的全屏程序里）。
      if (cwd === null) {
        toast("未能确定终端当前目录，已取消上传；请回到 shell 提示符后重试", {
          type: "warning",
          duration: 6000,
        });
        return;
      }
      startDropUploadZmodem(rawPaths);
      return;
    }

    // 两条路都没有：给可操作的安装指引，而不是一句无法执行的错误。
    capsUnavailable.value = {
      message: caps.message ?? "服务端既不支持 SFTP 子系统，也没有 lrzsz",
      hint: caps.installHint,
    };
  }

  /** SFTP 通道（含目标目录预检、重名剔除、同名目录弹窗）。zmodemFallback
   *  为 true 时，预检因 SFTP 不可用而失败会自动改走 ZMODEM。 */
  async function startDropUploadSftp(
    rawPaths: string[],
    cwd: string | null,
    zmodemFallback: boolean,
  ): Promise<void> {
    if (cwd === null) {
      toast("未能获取终端当前目录，将上传到主目录", { type: "warning", duration: 5000 });
    }
    let dest = cwd ?? "~";

    let report: SftpDirConflictReport;
    try {
      report = await sftpCheckDirConflicts(sid, rawPaths, dest);
    } catch (e) {
      if (zmodemFallback) {
        // SFTP 探测说可用但预检失败（探测与使用之间服务端拒绝/挂起）——
        // 有 lrzsz 就别让用户白等，直接换通道。
        toast("SFTP 子系统不可用，改用 ZMODEM 通道上传", { type: "info", duration: 4000 });
        startDropUploadZmodem(rawPaths);
        return;
      }
      toast(`无法检查远端目标目录，已取消上传：${e}`, { type: "error", duration: 6000 });
      return;
    }

    // 目标目录在拖入后、上传前被删掉（或探针拿到的是个已消失的路径）：
    // 回退主目录再检一次，避免后面每一步 create_dir 都失败。
    if (!report.destExists) {
      if (dest !== "~") {
        toast(`目标目录不存在或不可访问，已改用主目录：${dest}`, {
          type: "warning",
          duration: 6000,
        });
        dest = "~";
        try {
          report = await sftpCheckDirConflicts(sid, rawPaths, dest);
        } catch (e) {
          toast(`无法检查远端主目录，已取消上传：${e}`, { type: "error", duration: 6000 });
          return;
        }
      }
      if (!report.destExists) {
        toast(`远端目录不可用，已取消上传：${report.resolvedDest}`, {
          type: "error",
          duration: 6000,
        });
        return;
      }
    }

    // 批次内同名目录：两棵不同的本地树会落到同一个远端路径并静默混合，
    // 任何自动选择（先到先得/合并/改名）都是替用户猜。剔除并明确告知。
    let paths = rawPaths;
    const dupPaths = new Set(report.duplicatePaths);
    if (dupPaths.size > 0) {
      paths = rawPaths.filter((p) => !dupPaths.has(p));
      toast(
        `以下目录本次拖入重名，已跳过：${report.duplicateDirs.join("、")}（请分开拖入）`,
        { type: "warning", duration: 7000 },
      );
      if (paths.length === 0) return;
    }

    // 远端已存在同名目录 → 让用户决定合并还是跳过，不默认覆盖。
    if (report.existingDirs.length > 0) {
      pendingDrop.value = {
        dest: report.resolvedDest,
        paths,
        conflictNames: report.existingDirs,
      };
      return;
    }
    await beginDropUpload(paths, report.resolvedDest);
  }

  /** ZMODEM 通道：向 PTY 发 rz，等 zmodem_start 后把拖入的路径交给
   *  zmodemStartUpload（远端 rz 按 offer 的相对路径自动重建目录树）。
   *  这条路上没有同名目录弹窗——目录创建语义归远端 rz，客户端无法预检。 */
  function startDropUploadZmodem(paths: string[]): void {
    if (autoUploadPaths.value !== null) return; // 已有一轮在等 rz 启动
    autoUploadPaths.value = paths;
    dragZmodemActive.value = true;
    // rz 12s 内没起会话就收场。command not found 时 shell 早已回到提示符，
    // 不用补发任何按键；若恰好赶上启动，迟到的 zmodem_start 走正常选择器。
    rzWatchdog = setTimeout(() => {
      rzWatchdog = null;
      dragZmodemActive.value = false;
      if (autoUploadPaths.value !== null) {
        autoUploadPaths.value = null;
        toast("远端 rz 未在 12 秒内启动，已取消上传（安装 lrzsz 后重试）", {
          type: "error",
          duration: 6000,
        });
      }
    }, 12000);
    // `-y` = 覆盖已存在的同名文件。裸 rz 对已存在文件回 ZSKIP 直接拒收，
    // 用户看到的只是「传输失败」——实测 lrzsz 的默认行为就是跳过。
    sendTo(sid, "rz -y\n").catch(() => {
      if (rzWatchdog !== null) {
        clearTimeout(rzWatchdog);
        rzWatchdog = null;
      }
      autoUploadPaths.value = null;
      dragZmodemActive.value = false;
      toast("发送 rz 失败，已取消上传", { type: "error", duration: 5000 });
    });
  }

  // 落点判定。Tauri 的 drag-drop 载荷带的是 **PhysicalPosition（物理像素）**
  // —— wry 在 WebView2 上把 ScreenToClient 的结果原样传出，经 tauri-runtime-wry
  // 包成 PhysicalPosition 交给前端；而 getBoundingClientRect() 返回的是
  // **CSS 像素**。DPR ≠ 1 的屏幕（125%/150%/200% 缩放）上两者不等，
  // 直接比会让整个落点区域按 DPR 缩放后偏移：150% 缩放下拖到面板左上角
  // 会被判成落在面板外，拖到窗口右下角反而命中。
  // 用 devicePixelRatio 把物理坐标换成 CSS 坐标再比。
  function hitSelf(physX: number, physY: number): boolean {
    const el = panelEl.value;
    if (!el) return false;
    const dpr = window.devicePixelRatio || 1;
    const x = physX / dpr;
    const y = physY / dpr;
    const r = el.getBoundingClientRect();
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  }

  // Tauri 2 的拖放是窗口级原生事件（tauri://drag-enter/over/drop/leave），
  // 由 wry 在 WebView2 上注册 IDropTarget 后经 manager 转发成前端事件。
  // 注意：注册了原生拖放后 WebView2 的 HTML5 拖放被完全接管，DOM 的
  // dragover/drop 不会再触发，所以只能走这条通道。
  // 四个事件共用同一套判空 + 落点判定；只有 enter/over 需要维护高亮态。
  const dragPos = (e: { payload: { position: { x: number; y: number } } }) =>
    hitSelf(e.payload.position.x, e.payload.position.y);
  const onDragEnter = (e: { payload?: { paths?: string[] } }) => {
    const paths = e.payload?.paths;
    if (!paths || paths.length === 0) return;
    fileDropActive.value = dragPos(e as never);
  };
  const onDragOver = (e: { payload: { position: { x: number; y: number } } }) => {
    fileDropActive.value = dragPos(e);
  };
  const onDragLeave = () => {
    fileDropActive.value = false;
  };
  const onDrop = (e: { payload: { paths?: string[]; position: { x: number; y: number } } }) => {
    fileDropActive.value = false;
    const paths = e.payload?.paths;
    if (!paths || paths.length === 0) return;
    if (!dragPos(e)) return;
    // 本地终端没有 SSH 会话，sftp_upload 无从打开子系统；本地 tab
    // 拖入文件没有意义（用户要传的是远端），直接忽略而不是报错。
    if (props.connType === "local") return;
    // ZMODEM 会话期间终端输入被吞以保护协议流，探针的 echo 会直接打进
    // 字节流污染传输 —— 宁可拒绝本次拖拽。
    if (isZmodem) {
      toast("ZMODEM 传输进行中，请等待结束再拖拽上传", { type: "warning", duration: 4000 });
      return;
    }
    if (dropTransfer.value || pendingDrop.value || dropPreparing.value || autoUploadPaths.value) {
      toast("已有上传任务在进行中", { type: "warning", duration: 3000 });
      return;
    }
    void startDropUpload(paths);
  };

  emit("terminal-ready", sid, t);

  t.focus();
  // Retry focus after a short delay. The initial term.focus() above can run
  // before the xterm DOM layers are fully attached, so the internal cursor
  // blink state machine never starts — leaving the cursor frozen (not
  // blinking) until something else (tab switch, clicking into the app)
  // re-triggers focus. The retry ensures the blink timer is armed even on a
  // slow first paint. 150ms is well past xterm's own open/refresh cycle.
  const focusRetry = setTimeout(() => {
    if (!closed) t.focus();
  }, 150);

  // Window focus/blur — Chromium suspends timers (including xterm's cursor
  // blink interval) when the window loses focus, and does NOT automatically
  // resume the blink state machine when focus returns. Without re-focusing
  // here, the cursor stays frozen (solid or invisible) after switching away
  // from the app and back. Only act when this tab is the active one, so we
  // don't steal focus from another tab the user switched to.
  const onWindowFocus = () => {
    if (props.active && !closed) {
      t.focus();
    }
  };
  window.addEventListener("focus", onWindowFocus);

  // ── 窗口级拖放事件订阅（拖拽上传）──
  // 用 Tauri 的 event.listen 而不是 getCurrentWindow().onDragDropEvent：
  // 后者内部同样转发 tauri://drag-drop，但 enter/over 的载荷形状在不同
  // 版本有差异；直接监听原始事件、按 payload 字段判空更稳。订阅放进本轮
  // 生命周期，dispose 时一并取消。
  void (async () => {
    const { listen } = await import("@tauri-apps/api/event");
    const dragEvents: Array<[string, (e: never) => void]> = [
      ["tauri://drag-enter", onDragEnter as (e: never) => void],
      ["tauri://drag-over", onDragOver as (e: never) => void],
      ["tauri://drag-leave", onDragLeave as (e: never) => void],
      ["tauri://drag-drop", onDrop as (e: never) => void],
    ];
    const unlisteners = await Promise.all(
      dragEvents.map(([name, fn]) => listen(name, fn as (e: unknown) => void)),
    );
    if (closed) {
      unlisteners.forEach((u) => u());
      return;
    }
    dragEventUnlisten.push(...unlisteners);
  })();

  disposeFn = () => {
    closed = true;
    clearTimeout(focusRetry);
    clearZmodemIntentHint();
    dragEventUnlisten.forEach((u) => u());
    dragEventUnlisten = [];
    // 未完成的 cwd 探针不能挂着：dispose 之后 PTY 输出不会再进来，
    // 等待者会一直挂到 3s 超时。直接以 null 收场。
    cwdWaiters.splice(0).forEach((w) => w(null));
    cwdProbe = null;
    // 进行中的拖拽上传随本次终端一起收场。底层 sftp_upload 仍会在 Rust 侧
    // 跑完（或随旧 SSH 会话断开而失败），它 emit 的 progress/done 已无人
    // 监听、被丢弃 —— 重连后从零开始符合用户预期，不跨会话继承半截传输。
    closeDropTransfer();
    // 挂起的冲突对话框也必须一起作废：它记录的是这一次拖入的 dest 与路径，
    // 若用户在 dispose 前最后一点击到「合并覆盖」，beginDropUpload 会拿
    // 旧 sid 发起上传，而那个会话已经不在 ssh_sessions 里。
    pendingDrop.value = null;
    // ZMODEM 自动上传同样作废：挂起的路径属于旧会话，迟到的 zmodem_start
    // 会把文件发给已断开的连接。看门狗一并停掉。
    if (rzWatchdog !== null) {
      clearTimeout(rzWatchdog);
      rzWatchdog = null;
    }
    autoUploadPaths.value = null;
    dragZmodemActive.value = false;
    window.removeEventListener("focus", onWindowFocus);
    unlistenOutput?.();
    unlistenClosed?.();
    unlistenZmodemStart?.();
    unlistenZmodemRaw?.();
    unlistenZmodemEnd?.();
    unlistenZmodemError?.();
    unlistenZmodemOffer?.();
    unlistenZmodemProgress?.();
    unlistenZmodemFileComplete?.();
    unlistenZmodemFileSkipped?.();
    unsubscribeStatus();
    resizeObserver.disconnect();
    t.dispose();
    if (abortTimeout) {
      clearTimeout(abortTimeout);
      abortTimeout = null;
    }
    firstSyncTimers.forEach(clearTimeout);
    firstSyncTimers.length = 0;
    emit("terminal-gone", sid);
  };

  // 旧版 [fontFamily] effect 在挂载时也跑一次（options.fontFamily 已是同值，
  // 关键是 document.fonts.ready 后的 refit，覆盖 Nerd Font 异步加载的首帧
  // 测量漂移）。
  refitAfterFontSettle();
}

function teardownTerminal(): void {
  disposeFn?.();
  disposeFn = null;
  term = null;
  fitAddon = null;
  bridge = null;
}

// ── 拖拽上传浮层的派生量（对齐 SftpPanel 的 pctDone/speed/remaining）──
const dropPct = computed(() => {
  const t = dropTransfer.value;
  if (!t || t.bytesTotal <= 0) return 0;
  return Math.min(100, Math.round((t.bytesDone / t.bytesTotal) * 100));
});
const dropElapsed = computed(() => {
  const t = dropTransfer.value;
  if (!t || t.startTime <= 0) return 0;
  return (dropTickNow.value - t.startTime) / 1000;
});
const dropSpeed = computed(() =>
  dropElapsed.value > 1 ? (dropTransfer.value?.bytesDone ?? 0) / dropElapsed.value : 0,
);
const dropRemaining = computed(() => {
  const t = dropTransfer.value;
  const sp = dropSpeed.value;
  return sp > 0 && t !== null && t.bytesTotal > 0 ? (t.bytesTotal - t.bytesDone) / sp : Infinity;
});

function dropFormatSize(bytes: number): string {
  if (bytes === 0) return "-";
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  let size = bytes;
  while (size >= 1024 && i < units.length - 1) {
    size /= 1024;
    i++;
  }
  return `${size.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}
function dropFormatClock(epochMs: number): string {
  if (!epochMs) return "—";
  const d = new Date(epochMs);
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map((n) => String(n).padStart(2, "0"))
    .join(":");
}
function dropFormatDuration(seconds: number): string {
  if (!isFinite(seconds) || seconds <= 0) return "—";
  if (seconds < 60) return `${Math.ceil(seconds)}秒`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = Math.ceil(seconds % 60);
    return `${m}分${s}秒`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}小时${m}分`;
}

/** 字体设置后的重排。xterm re-measures the cell width when fontFamily changes
 * but does NOT recompute cols, so cols × cellWidth silently drifts from the
 * container width. The terminal then keeps reporting stale cols to the PTY;
 * pwsh's PSReadLine absolutely-positions the cursor on EVERY keystroke and
 * repaints the whole input line against those stale cols — so glyphs land in
 * the wrong cells. On the transparent background + background-image path
 * those mis-drawn glyphs float over the wrong part of the image, which reads
 * as "characters typed beside the background, pushing it left" (worst when
 * input nears the right margin).
 *
 * The same drift bites the INITIAL fit: Nerd Font files load asynchronously,
 * so the mount-time fit() (100ms) can measure against the fallback font and
 * be wrong the instant the real font finishes loading. Waiting on
 * document.fonts.ready covers both the first-paint race and later font
 * switches. */
function refitAfterFontSettle(): void {
  const t = term;
  const fit = fitAddon;
  if (!t) return;
  t.options.fontFamily = fontFamily.value;

  const refit = () => {
    if (!fit) return;
    const container = containerRef.value;
    // Same tiny-size guard as the ResizeObserver — don't fit against a
    // collapsed container (tab hidden mid-transition) and poison cols.
    if (!container || container.clientWidth < 80 || container.clientHeight < 40) return;
    try {
      fit.fit();
    } catch {
      return;
    }
    resizeTo(props.sessionId, t.cols, t.rows).catch(() => {});
  };

  if (typeof document !== "undefined" && document.fonts?.ready) {
    // document.fonts.ready resolves once all pending font loads settle; if
    // the font is already loaded it resolves immediately. catch → refit
    // anyway so a failed/never-resolving font load can't leave us on the
    // fallback metrics forever.
    document.fonts.ready.then(refit).catch(refit);
  } else {
    refit();
  }
}

// Live font update: apply a newly chosen terminal font to the existing
// terminal without re-creating it. xterm.js re-rasterizes glyphs when
// fontFamily changes, so the new font (and its Nerd Font glyphs) shows up
// immediately on open tabs. Must refit after the change (see
// refitAfterFontSettle for why).
watch(fontFamily, () => {
  if (!term) return;
  refitAfterFontSettle();
});

// 字号变化同样要重排（字符格尺寸变了，列数必须重算），走同一条
// refitAfterFontSettle 延迟路径。
watch(terminalFontSize, (px) => {
  const t = term;
  if (!t) return;
  t.options.fontSize = px;
  refitAfterFontSettle();
});

// Live theme update: when the user switches palette or toggles dark/light
// (applyTheme 会改 html[data-theme] 并重跑 applyColorPreset），update the
// existing terminal theme without losing scrollback.
// xterm.js 5.x supports live `term.options.theme` mutation.
watch([() => activePalette.value.id, () => ui.isDark], () => {
  const t = term;
  if (!t) return;

  // NOTE: must be a real alpha-0 color, not the "transparent" keyword — the
  // WebGL renderer can't parse "transparent" (canvas could), and falls back
  // to an opaque black clearColor, hiding the background image.
  const theme = terminalTheme.value;
  t.options.theme = {
    ...theme,
    ...forceVisibleCursor(theme),
    // 恒透明：底色由 .terminal-wrap 的玻璃承载（见样式段注释）。
    background: "rgba(0, 0, 0, 0)",
    selectionBackground: visibleSelection(theme.selectionBackground),
  };
  t.options.allowTransparency = true;
});

// Active-tab transitions: refit on show (the last fit ran against a
// hidden container with zero geometry) and grab focus so the user
// can type immediately. Blur on hide so keystrokes don't leak to an
// invisible terminal. Mount/unmount is handled by setup/teardown —
// this only fires when visibility flips.
function handleActiveChange(): void {
  const t = term;
  const fit = fitAddon;
  if (!t || !fit) return;

  if (props.active) {
    // Guard against container not yet sized or terminal disposed
    const container = containerRef.value;
    if (!container) return;

    const w = container.clientWidth;
    const h = container.clientHeight;
    if (w < 80 || h < 40) {
      // Container not yet sized, skip fit to avoid RenderService error
      return;
    }

    // Additional safety: check if terminal is still usable
    if (!t.element || !t.element.parentElement) {
      return;
    }

    try {
      fit.fit();
    } catch {
      // Silently ignore fit errors during transitions
      return;
    }

    // 分屏：焦点从一个叶子切到另一个时，新焦点叶子在这里补一次 fit 并把
    // 尺寸推给远端；旧叶子走 else 分支 blur。非焦点叶子不推尺寸（见
    // ResizeObserver 里同样的门禁），否则两个叶子的 cols 互相覆盖。
    if (props.resizeEnabled !== false) {
      resizeTo(props.sessionId, t.cols, t.rows).catch(() => {});
      // Sync the freshly-refit cols to broadcast members so their shells
      // output in the same width — covers the case where this tab was
      // inactive through a window resize and the others had stale dims.
      for (const targetSid of props.broadcastTargets) {
        if (targetSid !== props.sessionId) {
          resizeTo(targetSid, t.cols, t.rows).catch(() => {});
        }
      }
    }
    if (props.inputEnabled !== false) {
      t.focus();
    }
  } else {
    t.blur();
  }
}

watch(() => props.active, handleActiveChange);

// Broadcast membership change (a tab joined/left the group): push our
// current cols to the other members right away. This is the only signal
// we get when the user toggles 📡 — ResizeObserver won't fire because
// nothing resized, but the shells still need to align before the next
// broadcast keystroke lands.
//
// CRITICAL: gate on `active`. An inactive tab's term.cols is a stale/
// possibly-zero value (its container is hidden — ResizeObserver saw a 0x0
// size transition and may have poisoned cols). Letting it push that zero
// to other members collapses everyone's COLUMNS and turns ls output into
// one-file-per-line. Only the active (visible, freshly-fit) tab is a
// reliable source of truth for cols.
function syncBroadcastDims(): void {
  if (!props.active) return;
  const t = term;
  if (!t) return;
  // Defensive: if our own cols is implausibly small, don't push it —
  // we'd just propagate garbage.
  if (t.cols < 20) return;
  for (const targetSid of props.broadcastTargets) {
    if (targetSid !== props.sessionId) {
      resizeTo(targetSid, t.cols, t.rows).catch(() => {});
    }
  }
}

watch(() => [props.broadcastTargets.join(","), props.active] as const, syncBroadcastDims);

// commandBar 默认 true（由 props 的 withDefaults 声明），主标签区显式传
// false 改用壳层底部行那条；多窗口网格不传 → 每格自带一条。
const showCommandBar = computed(() => props.commandBar);

function onZmodemCancel(): void {
  bridge?.abort();
  // bridge?.abort() 只作用于 JS 版 zmodem 桥；上传/下载走 Rust 原生通道，
  // 统一出口负责发 abort、本地复位与 2s 兜底（见 abortZmodemSession）。
  bridge?.reset();
  abortZmodemSession();
}

onMounted(() => {
  setupTerminal();
  // 旧版 [active] / [broadcastKey] effect 在挂载时也会执行一次（此时 term
  // 已就绪），这里补齐等价的首次执行。
  handleActiveChange();
  syncBroadcastDims();
});

// sessionId 变化（重连换新会话）→ 完整重建终端生命周期：注销旧会话的事件
// 订阅与注册表项、重建 xterm、恢复 scrollback 快照。组件实例本身不销毁
// （App.vue 以 tab.id 为 key，跨重连保持挂载）。
watch(
  () => props.sessionId,
  () => {
    teardownTerminal();
    setupTerminal();
  },
);

onUnmounted(() => {
  teardownTerminal();
});
</script>

<template>
  <div class="terminal-panel" ref="panelEl">
    <!-- xterm 容器 —— flex:1 占据 CommandBar 之上全部空间。padding 放在
         wrapper 上，NOT on the xterm container below. See the comment on
         xterm-container for why.
         底色不在这里内联：.terminal-wrap 是否铺玻璃取决于工作区有没有铺
         （见样式段注释），用 CSS 条件比在这里算更准确。 -->
    <div class="terminal-wrap">
      <!-- 底色由这一层承担，与侧栏/顶栏/命令栏同源（var(--glass-bg)：
           颜色随主题色、alpha 随界面实心度）；xterm 自身背景恒透明，
           只负责画字。壁纸由 body 级壁纸层统一渲染（app.css 的
           body::before），透过玻璃自然显现。 -->
      <!-- xterm 挂载容器。NO padding here — the 4px visual inset lives on the
           wrapper above. xterm's .xterm element fills this container's content
           box, and FitAddon reads getComputedStyle(thisContainer).width as
           the usable width. Under the global `* { box-sizing: border-box }`
           rule, padding on THIS div would be included in that width but
           excluded from .xterm's actual render area, so FitAddon over-
           counts cols by ~1 (8px / cellWidth). xterm then tells the PTY
           one more column than it can actually paint: the last column
           renders off-canvas and PSReadLine mis-positions the cursor on
           every keystroke, so the input line redraws with characters
           spilling out and the background shifted left ("字符跳出界面 /
           背景左移", worst on the local PowerShell/ConPTY path). Keeping
           the inset on the wrapper leaves the column math exact. -->
      <div ref="containerRef" class="xterm-container" />
      <ZmodemProgressOverlay :status="zmodemStatus" @cancel="onZmodemCancel" />
      <!-- 手动 rz 的文件选择直接弹系统对话框（onZmodemStart → pickUploadFiles），
           关闭不选即取消；不再有内嵌的「文件/文件夹」二选一浮层。 -->
    </div>

    <!-- 命令栏。多窗口网格里每个格子自带一条（commandBar 默认开启）；主标签区
         传 :command-bar="false"，改由壳层底部行统一渲染一条给活动标签用 ——
         那条与服务器状态栏一起贯穿整个窗口宽度。 -->
    <CommandBar
      v-if="showCommandBar && connectionId"
      :session-id="sessionId"
      :connection-id="connectionId"
      :conn-type="connType"
      :broadcast-targets="broadcastTargets"
      :status="status"
      @reconnect="emit('reconnect')"
      @open-quick-commands-manage="emit('open-quick-commands-manage', connectionId)"
      @screenshot="handleScreenshot"
    />

    <!-- Screenshot status banner — transient toast that appears after the
         user clicks 📷. Sits at the bottom-right so it doesn't cover the
         terminal content. Auto-dismisses after 4s (see flashScreenshot). -->
    <!-- 「正在截取」瞬时提示；成功/失败结果改由 myui 的 toast 承载
         （成功带「打开」动作，见 handleScreenshot）。 -->
    <div v-if="screenshotCapturing" class="screenshot-banner is-capturing">
      <span class="spinner">
        <MyIcon :size="14" name="Loading" spin />
      </span>
      <span>正在截取终端...</span>
    </div>

    <!-- 拖拽上传：拖入时的落点提示（覆盖终端区，不吃点击）。 -->
    <div v-if="fileDropActive" class="drop-hint">
      <MyIcon :size="22" name="Upload" />
      <span>松开上传到当前目录（支持文件夹）</span>
    </div>

    <!-- 松手后的「准备中」遮罩：能力探测（SFTP 子系统探测 + exec 脚本各
         最多 8s）与 cwd 探针期间不能没有任何反馈——此前这段静默十几秒，
         用户以为拖拽没生效。 -->
    <div v-else-if="dropPreparing" class="drop-hint">
      <span class="spinner">
        <MyIcon :size="16" name="Loading" spin />
      </span>
      <span>正在准备上传（检测服务器传输能力）…</span>
    </div>

    <!-- 两条上传通道都不可用：给出可执行的安装指引而不是一句死路错误。 -->
    <MyDialog
      :model-value="capsUnavailable !== null"
      title="无法上传：服务器缺少传输组件"
      size="sm"
      @update:model-value="(v: boolean) => { if (!v) capsUnavailable = null; }"
    >
      <div v-if="capsUnavailable" class="caps-body">
        <p class="caps-msg">{{ capsUnavailable.message }}</p>
        <template v-if="capsUnavailable.hint">
          <p class="caps-step">在服务器上安装 lrzsz：</p>
          <div class="caps-cmd-row">
            <code class="caps-cmd">{{ capsUnavailable.hint }}</code>
            <MyButton variant="secondary" size="small" @click="copyInstallHint">复制</MyButton>
          </div>
        </template>
        <p class="caps-alt">
          或者让服务端启用 SFTP 子系统：确认 /etc/ssh/sshd_config 中保留
          <code>Subsystem sftp internal-sftp</code> 后重载 sshd。
        </p>
      </div>
      <template #footer>
        <MyButton variant="primary" size="small" @click="capsUnavailable = null">知道了</MyButton>
      </template>
    </MyDialog>

    <!-- 远端已存在同名目录：默认不覆盖，由用户决定合并还是跳过。 -->
    <!-- 不传 hide-footer：myui 的 hideFooter 会把自定义 #footer 一起丢掉
         （源码 `hideFooter ? void 0 : { name: "footer" }`），三个出口按钮
         会全部不渲染。提供 #footer 插槽即覆盖默认的确认/取消按钮。 -->
    <MyDialog
      :model-value="pendingDrop !== null"
      title="远端已存在同名目录"
      size="sm"
      @update:model-value="(v: boolean) => { if (!v) resolveDropConflict('cancel'); }"
    >
      <div v-if="pendingDrop" class="conflict-body">
        <p>以下目录在目标位置已存在：</p>
        <ul class="conflict-list">
          <li v-for="name in pendingDrop.conflictNames" :key="name">{{ name }}</li>
        </ul>
        <p class="conflict-note">
          「合并覆盖」会把新文件写进已有目录，同名文件被覆盖；「跳过这些」只上传其余内容。
        </p>
      </div>
      <template #footer>
        <div class="conflict-actions">
          <MyButton variant="ghost" size="small" @click="resolveDropConflict('cancel')">取消</MyButton>
          <MyButton variant="ghost" size="small" @click="resolveDropConflict('skip')">跳过这些</MyButton>
          <MyButton variant="primary" size="small" @click="resolveDropConflict('merge')">合并覆盖</MyButton>
        </div>
      </template>
    </MyDialog>

    <!-- 拖拽上传进度浮层（SFTP，非 ZMODEM）—— 版式对齐 ZmodemProgressOverlay
         与 SftpPanel 的 transfer-overlay：四行 + 错误列表。 -->
    <div v-if="dropTransfer" class="transfer-overlay">
      <div class="tx-row1">
        <div class="tx-left">
          <span class="tx-arrow">↑</span>
          <span class="tx-label">
            {{ dropTransfer.done ? "上传完成" : "上传中" }}
          </span>
          <span v-if="dropTransfer.fileCount > 1" class="tx-count">
            {{ Math.min(dropTransfer.fileIndex + (dropTransfer.done ? 0 : 1), dropTransfer.fileCount) }}/{{ dropTransfer.fileCount }}
          </span>
          <span class="tx-file">{{ dropTransfer.currentFile || "准备中…" }}</span>
        </div>
        <MyButton v-if="dropTransfer.done" variant="primary" size="small" @click="closeDropTransfer">
          关闭
        </MyButton>
        <MyButton v-else variant="danger" size="small" @click="onCancelDropTransfer">取消</MyButton>
      </div>
      <MyProgress class="tx-bar" :percentage="dropPct" :stroke-width="8" :show-text="false" />
      <div class="tx-row3">
        <span>
          {{ dropFormatSize(dropTransfer.bytesDone) }} / {{ dropFormatSize(dropTransfer.bytesTotal) }}
          <template v-if="dropTransfer.bytesTotal > 0">({{ dropPct }}%)</template>
        </span>
        <span>{{ dropSpeed >= 1 ? dropFormatSize(dropSpeed) + "/s" : "—" }}</span>
      </div>
      <div class="tx-row4">
        <span>开始 {{ dropFormatClock(dropTransfer.startTime) }}</span>
        <span>已用 {{ dropFormatDuration(dropElapsed) }}</span>
        <span>剩余 {{ dropTransfer.done ? "—" : dropFormatDuration(dropRemaining) }}</span>
      </div>
      <div v-if="dropTransfer.done && dropTransfer.errors.length > 0" class="tx-errors">
        <div v-for="(er, i) in dropTransfer.errors.slice(0, 3)" :key="i">• {{ er }}</div>
        <div v-if="dropTransfer.errors.length > 3">…等 {{ dropTransfer.errors.length }} 个错误</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.terminal-panel {
  position: relative;
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  /* 同 .shell-side 的坑（见 app.css）：本元素是 .tab-stack 列向 flex 的
     子项且 overflow 可见，块轴的「自动最小尺寸」会退化成内容的完整高度。
     海量输出（ls 上万行）时终端内容极高，整块面板被撑大、把下方的输入栏
     顶出可视区。显式 min-height:0 让它能被压缩到 .tab-stack 给的高度，
     由 xterm 自己的视口负责滚动。 */
  min-height: 0;
}

/* xterm 内部层常驻透明：底色由 .terminal-wrap 的玻璃承担，xterm 只画字。
   xterm.css 自带实底 background-color 规则，必须覆盖掉，否则玻璃被挡住。
   （xterm 运行时生成的 DOM 不带 scoped 标记，须用 :deep() 穿透） */
:deep(.xterm-viewport),
:deep(.xterm-screen),
:deep(.xterm) {
  background: transparent !important;
}

:deep(.xterm-viewport::-webkit-scrollbar-track) {
  background: transparent !important;
}

.terminal-wrap {
  flex: 1;
  min-height: 0;
  position: relative;
  padding: 4px;
  /* ── 终端底色：与侧栏/顶栏/命令栏同一套玻璃 ──
     但要避免**双层叠加**：.shell-work 本身在「无壁纸」模式下已经铺了
     一层 var(--glass-bg)（alpha 0.38 起），终端若再铺一层同值玻璃，
     叠加后 1-(1-0.38)² ≈ 0.62 —— 终端区会比侧栏明显更实，正是「不
     一致」的来源。所以这里默认透明，让工作区那层玻璃透出来；只有
     透出模式（工作区改为完全透明、壁纸直出）才由终端自己铺。
     两条路径最终都是「一层 --glass-bg」，与菜单同源同值。 */
  background: transparent;
}

/* 透出模式下工作区已透明（app.css 的 html[data-translucent="1"]
   .shell-work），终端必须自己铺这一层，否则文字直接压在壁纸上。
   整条选择器包进 :global()：Vue 对 `:global(A) B` 的编译会丢掉 B
   （实测输出变成 `html[data-translucent="1"] { background: … }`，
   直接作用到 html 上，终端一条没铺）；包成 :global(A B) 才按原样输出。
   `.terminal-wrap` 全仓只有本组件使用，不担心误伤。 */
:global(html[data-translucent="1"] .terminal-wrap) {
  background: var(--glass-bg);
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
}

.xterm-container {
  width: 100%;
  height: 100%;
  position: relative;
  z-index: 1;
  background: transparent;
}

.bg-layer {
  position: absolute;
  inset: 0;
  background-size: cover;
  background-position: center;
  background-repeat: no-repeat;
  pointer-events: none;
  z-index: 0;
}

/* rz 上传的内嵌「文件/文件夹」选择条已删除（直接弹系统对话框）。 */

/* 截图状态横幅 */
.screenshot-banner {
  position: absolute;
  bottom: 12px;
  right: 12px;
  max-width: 420px;
  padding: 10px 14px;
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  border: 1px solid var(--border-default);
  box-shadow: var(--shadow-md);
  font-size: var(--fs-xs);
  color: var(--text-primary);
  z-index: 1000;
  display: flex;
  align-items: center;
  gap: 8px;
}

/* 拖拽上传落点提示（虚线框居中，不吃点击） */
.drop-hint {
  position: absolute;
  inset: 8px;
  z-index: 20;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  border: 2px dashed var(--accent-primary);
  border-radius: var(--radius-md);
  background: var(--accent-primary-muted);
  color: var(--text-primary);
  font-size: var(--fs-sm);
  font-weight: 600;
  pointer-events: none;
}

/* 冲突确认：列出远端已存在的目录名，三个出口（取消 / 跳过 / 合并）。 */
.conflict-body p {
  margin: 0 0 8px;
  color: var(--text-secondary);
}

.conflict-list {
  margin: 0 0 10px;
  padding-left: 18px;
  max-height: 160px;
  overflow-y: auto;
  color: var(--text-primary);
  font-family: "Cascadia Code", Consolas, monospace;
  font-size: var(--fs-xs);
  line-height: 1.6;
}

.conflict-note {
  color: var(--text-tertiary);
  font-size: var(--fs-xs);
}

.conflict-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
/* 安装指引对话框：结论 + 可复制的安装命令 + SFTP 替代方案。 */
.caps-body p {
  margin: 0 0 10px;
  color: var(--text-secondary);
}

.caps-msg {
  color: var(--text-primary);
}

.caps-step {
  font-weight: 600;
}

.caps-cmd-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
}

.caps-cmd {
  flex: 1;
  min-width: 0;
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  color: var(--text-primary);
  font-family: "Cascadia Code", Consolas, monospace;
  font-size: var(--fs-xs);
  overflow-x: auto;
  white-space: nowrap;
}

.caps-alt {
  color: var(--text-tertiary);
  font-size: var(--fs-xs);
  line-height: 1.5;
}

/* 拖拽上传进度浮层：版式与 ZmodemProgressOverlay / SftpPanel.transfer-overlay
   一致（同一套 CSS 变量与行结构），只改类名前缀避免与 scoped 规则串味。 */
.transfer-overlay {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  background: var(--glass-bg);
  border-top: 1px solid var(--border-emphasis);
  padding: 8px 16px 10px;
  font-size: var(--fs-xs);
  font-family: "Cascadia Code", Consolas, monospace;
  color: var(--text-primary);
  z-index: 10;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.tx-row1 {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}

.tx-left {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.tx-arrow {
  color: var(--accent-primary);
  font-size: var(--fs-lg);
  line-height: 1;
}

.tx-label {
  color: var(--success);
  font-weight: 600;
  white-space: nowrap;
}

.tx-count {
  color: var(--text-tertiary);
  white-space: nowrap;
}

.tx-file {
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tx-bar :deep(.el-progress-bar__outer) {
  background: var(--bg-surface-active);
  border-radius: 4px;
}

.tx-row3 {
  display: flex;
  justify-content: space-between;
  color: var(--text-secondary);
}

.tx-row4 {
  display: flex;
  justify-content: space-between;
  color: var(--text-tertiary);
  font-size: var(--fs-2xs);
}

.tx-errors {
  color: var(--error);
  max-height: 42px;
  overflow-y: auto;
  line-height: 1.4;
}

.spinner {
  display: inline-flex;
}
</style>
