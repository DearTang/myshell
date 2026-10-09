<!-- 终端面板：xterm.js 完整生命周期 + ZMODEM 原生传输 UI + CommandBar 装配。
     从 src-legacy/components/TerminalPanel.tsx 逐行移植。关键不变量：
     - xterm 实例绝不放进响应式对象（普通 let + ref 容器 DOM）；
     - tab.id 稳定（组件实例跨重连存活），sessionId 变化由 watch 驱动完整
       重建（等价于旧版 [sessionId] effect），scrollback 经 reconnectSnapshot
       恢复；
     - 所有 tab 面板常驻挂载（App.vue 用绝对定位 + visibility 切换），
       xterm 容器必须保持真实尺寸。 -->
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { Document, FolderOpened, Loading } from "@element-plus/icons-vue";
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
  onSshClosed,
  onSshOutput,
  onZmodemEnd,
  onZmodemError,
  onZmodemFileComplete,
  onZmodemOffer,
  onZmodemProgress,
  onZmodemRaw,
  onZmodemStart,
  saveScreenshot,
  showInFolder,
  sshReady,
  sshResize,
  sshSend,
  sshSendZmodemAbort,
  zmodemAcceptOffer,
  zmodemStartUpload,
} from "@/api";
import type { ConnType, Tab } from "@/api";
import { open } from "@tauri-apps/plugin-dialog";
import { captureTerminalToDataUrl } from "@/utils/screenshot";
import { recordKeystroke } from "@/utils/cmd-buffer";
import { appearance, getActivePalette } from "@/store/appearance";
import { ui } from "@/store/ui";
import { primaryFont, resolveFontStack } from "@/composables/useTerminalFont";
import { resolveRenderer } from "@/composables/useRendererPref";
import type { RendererBackend } from "@/composables/useRendererPref";
import { ZmodemBridge } from "@/zmodem-bridge";
import type { ZmodemStatus } from "@/zmodem-bridge";
import CommandBar from "./CommandBar.vue";
import ZmodemProgressOverlay from "./ZmodemProgressOverlay.vue";
import "@xterm/xterm/css/xterm.css";

defineOptions({ name: "TerminalPanel" });

const props = defineProps<{
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
}>();

const emit = defineEmits<{
  /** Phase 3: registers the xterm instance with the session-store registry
   * (AI 面板读输出/选区、重连快照都经它)。terminal 打开后触发；重建/关闭时
   * 由 terminal-gone 注销。 */
  (e: "terminal-ready", sessionId: string, term: Terminal): void;
  (e: "terminal-gone", sessionId: string): void;
  (e: "open-ai"): void;
  (e: "open-multiwindow"): void;
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
// CommandBar 挂载时注册的历史列表刷新回调：录入新命令后触发。
let refreshHistory: (() => void) | null = null;
// abort 兜底定时器：zmodem_finish 正常应在一个 IPC 往返内触发 zmodem_end，
// 若未到达（会话已死 / IPC 失败）2s 后强制重置。
let abortTimeout: ReturnType<typeof setTimeout> | null = null;
// 截图横幅自动清除定时器
let screenshotTimer: ReturnType<typeof setTimeout> | null = null;

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

// Inline chooser shown when the remote runs `rz`: OS dialogs can't mix
// files and folders in one pick, so we ask which kind to send first.
// Folders are expanded recursively on the Rust side (ZFILE offers carry
// relative paths; lrzsz rz recreates the tree remotely).
const uploadChooserOpen = ref(false);

// Screenshot status banner — shown transiently after the user clicks 📷.
// `state` is one of: "capturing" | "saved" | "error". Auto-clears after 4s
// via the timeout stored in `screenshotTimer`.
type ScreenshotState =
  | { state: "capturing" }
  | { state: "saved"; path: string }
  | { state: "error"; message: string }
  | null;
const screenshotStatus = ref<ScreenshotState>(null);

// ── 主题 / 字体 / 背景图（computed：预设或深浅切换即重算） ──
const activePalette = computed(() => getActivePalette());
const terminalTheme = computed(() =>
  (ui.isDark ? activePalette.value.dark : activePalette.value.light).terminal,
);
const bgImage = computed(() => appearance.bgImage);
const hasBgImage = computed(() => bgImage.value.dataUrl !== null);
// Per-connection override wins over the global setting; empty → global.
const fontFamily = computed(() =>
  props.fontOverride ? resolveFontStack(props.fontOverride) : resolveFontStack(primaryFont.value),
);
const wrapBg = computed(() => (hasBgImage.value ? "transparent" : terminalTheme.value.background));

// Route send/resize to the ssh_* or local_* backend depending on the tab's
// connection type. Read live from props so the closures bound in
// setupTerminal always hit the right backend (connType is fixed per tab in
// practice, but the live read keeps it honest).
const sendTo = (sid: string, data: string): Promise<void> =>
  props.connType === "local" ? localSend(sid, data) : sshSend(sid, data);
const resizeTo = (sid: string, cols: number, rows: number): Promise<void> =>
  props.connType === "local" ? localResize(sid, cols, rows) : sshResize(sid, cols, rows);

/** Hand the picked files/folders to the native Rust uploader, or abort
 * when the user dismissed the picker without a selection. */
async function commitNativeUpload(paths: string[]): Promise<void> {
  uploadChooserOpen.value = false;
  if (paths.length === 0) {
    sshSendZmodemAbort(props.sessionId).catch(() => {});
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
    sshSendZmodemAbort(props.sessionId).catch(() => {});
  }
}

async function pickUploadFiles(): Promise<void> {
  await nudgeMouseCursor().catch(() => {});
  const selected = await open({ multiple: true });
  const paths: string[] = !selected ? [] : Array.isArray(selected) ? selected : [selected];
  await commitNativeUpload(paths);
}

async function pickUploadFolder(): Promise<void> {
  await nudgeMouseCursor().catch(() => {});
  const selected = await open({ directory: true, multiple: true });
  const paths: string[] = !selected ? [] : Array.isArray(selected) ? selected : [selected];
  await commitNativeUpload(paths);
}

function cancelNativeUpload(): void {
  uploadChooserOpen.value = false;
  sshSendZmodemAbort(props.sessionId).catch(() => {});
}

/** Capture the terminal viewport and save it to the attachment directory.
 * Bound to the 📷 button in CommandBar. No-op if the xterm instance isn't
 * mounted yet (rare race during tab open). */
async function handleScreenshot(): Promise<void> {
  const t = term;
  if (!t) {
    flashScreenshot({ state: "error", message: "终端尚未就绪，请稍候重试" });
    return;
  }
  // Clear any previous banner + timer.
  if (screenshotTimer) clearTimeout(screenshotTimer);
  screenshotStatus.value = { state: "capturing" };

  try {
    const dataUrl = await captureTerminalToDataUrl(t);
    if (!dataUrl) {
      flashScreenshot({ state: "error", message: "截图失败：无法捕获终端画面" });
      return;
    }
    const path = await saveScreenshot(dataUrl, props.connectionName || props.sessionId);
    flashScreenshot({ state: "saved", path });
  } catch (e: unknown) {
    const msg = e instanceof Error && e.message ? String(e.message) : String(e);
    // The Rust side returns a localized error when the attachment dir isn't
    // configured — surface it verbatim so the user knows what to fix.
    flashScreenshot({ state: "error", message: msg.includes("附件目录") ? msg : `保存失败: ${msg}` });
  }
}

/** Set the banner and auto-clear it after 4s. */
function flashScreenshot(s: ScreenshotState): void {
  screenshotStatus.value = s;
  if (screenshotTimer) clearTimeout(screenshotTimer);
  screenshotTimer = setTimeout(() => (screenshotStatus.value = null), 4000);
}

/** 打开已保存截图所在目录（资源管理器中选中该文件）。 */
async function openSavedPath(path: string): Promise<void> {
  try {
    if (path) await showInFolder(path);
  } catch {
    /* best-effort — ignore if open fails */
  }
}

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
    fontSize: 14,
    fontFamily: fontFamily.value,
    theme: {
      ...terminalTheme.value,
      ...forceVisibleCursor(terminalTheme.value),
      selectionBackground: visibleSelection(terminalTheme.value.selectionBackground),
      ...(hasBgImage.value ? { background: "rgba(0, 0, 0, 0)" } : {}),
    },
    allowProposedApi: true,
    allowTransparency: hasBgImage.value,
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
  const renderer = resolveRenderer(props.rendererBackend, hasBgImage.value);
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
    if (isZmodem) return;
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
            refreshHistory?.();
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

  // ── MCP sentinel line filter ──────────────────────────────────────
  // When the MCP server runs ssh_exec in show_in_gui mode, App.tsx sends
  // a sentinel line `echo __MCP_DONE_<rand>__:$?` to capture the exit
  // code. That line + its output would be visible in the terminal as
  // noise. We filter out any line containing `__MCP_DONE_` before it
  // reaches xterm.
  //
  // CRITICAL: we must NOT buffer incomplete lines that don't look like
  // they could be a sentinel — otherwise the shell prompt, user
  // keystrokes, and all interactive output get stuck in the buffer until
  // a newline arrives, breaking the terminal completely. Only buffer a
  // partial line if it contains the sentinel prefix "__MCP" (meaning it
  // might be a sentinel line split across chunks).
  let sentinelLineBuf = "";
  const SENTINEL_PREFIX = "__MCP";
  const SENTINEL_NEEDLE = "__MCP_DONE_";
  const filterSentinel = (data: Uint8Array): Uint8Array | null => {
    const chunk = new TextDecoder("utf-8", { fatal: false }).decode(data);
    // Prepend any previously buffered partial line, then process.
    sentinelLineBuf += chunk;
    const outLines: string[] = [];
    let start = 0;
    for (;;) {
      const nl = sentinelLineBuf.indexOf("\n", start);
      if (nl < 0) break; // incomplete line at the tail
      const line = sentinelLineBuf.slice(start, nl + 1);
      if (!line.includes(SENTINEL_NEEDLE)) outLines.push(line);
      start = nl + 1;
    }
    // Tail = incomplete line (no trailing \n).
    const tail = sentinelLineBuf.slice(start);
    if (tail.includes(SENTINEL_PREFIX)) {
      // This partial line looks like it might be a sentinel — buffer it
      // until we see the rest (next chunk will have the \n).
      sentinelLineBuf = tail;
    } else {
      // Normal content (prompt, keystrokes, output) — output immediately,
      // don't buffer. Reset the buffer.
      if (tail) outLines.push(tail);
      sentinelLineBuf = "";
    }
    if (outLines.length === 0) return null; // whole chunk was a buffered sentinel
    return new TextEncoder().encode(outLines.join(""));
  };

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
      // Native upload: show the inline files-vs-folder chooser (the OS
      // dialog can't select both kinds at once). The chosen paths then go
      // through zmodemStartUpload — folders are recursed in Rust.
      uploadChooserOpen.value = true;
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
        sshSendZmodemAbort(props.sessionId).catch(() => {});
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

  onZmodemEnd(sid, () => {
    isZmodem = false;
    nativeDownload = false;
    nativeDir = null;
    nativeCancelled = false;
    uploadChooserOpen.value = false;
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

  disposeFn = () => {
    closed = true;
    clearTimeout(focusRetry);
    clearZmodemIntentHint();
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

// Live theme update: when the user switches palette or toggles dark/light
// (applyTheme 会改 html[data-theme] 并重跑 applyColorPreset），update the
// existing terminal theme without losing scrollback.
// xterm.js 5.x supports live `term.options.theme` mutation.
watch([() => activePalette.value.id, () => ui.isDark, hasBgImage], () => {
  const t = term;
  if (!t) return;

  // NOTE: must be a real alpha-0 color, not the "transparent" keyword — the
  // WebGL renderer can't parse "transparent" (canvas could), and falls back
  // to an opaque black clearColor, hiding the background image.
  const theme = terminalTheme.value;
  const currentBg = hasBgImage.value ? "rgba(0, 0, 0, 0)" : theme.background;
  t.options.theme = {
    ...theme,
    ...forceVisibleCursor(theme),
    background: currentBg,
    selectionBackground: visibleSelection(theme.selectionBackground),
  };
  t.options.allowTransparency = hasBgImage.value;
  // 容器背景由模板的 :style 绑定（wrapBg computed）自动跟随，无需手动同步。
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

    resizeTo(props.sessionId, t.cols, t.rows).catch(() => {});
    // Sync the freshly-refit cols to broadcast members so their shells
    // output in the same width — covers the case where this tab was
    // inactive through a window resize and the others had stale dims.
    for (const targetSid of props.broadcastTargets) {
      if (targetSid !== props.sessionId) {
        resizeTo(targetSid, t.cols, t.rows).catch(() => {});
      }
    }
    t.focus();
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

function onRegisterRefresh(fn: () => void): void {
  refreshHistory = fn;
}

function onZmodemCancel(): void {
  bridge?.abort();
  // Safety net: zmodem_finish normally triggers zmodem_end within
  // one IPC round-trip. If it somehow doesn't arrive (session dead,
  // IPC failure), force-reset after 2s so the terminal is usable.
  if (abortTimeout) clearTimeout(abortTimeout);
  abortTimeout = setTimeout(() => {
    bridge?.reset();
    isZmodem = false;
    term?.write("\r\n\x1b[33m[abort 超时 — 强制重置]\x1b[0m\r\n");
    abortTimeout = null;
  }, 2000);
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
  if (screenshotTimer) {
    clearTimeout(screenshotTimer);
    screenshotTimer = null;
  }
});
</script>

<template>
  <div class="terminal-panel">
    <!-- xterm 容器 —— flex:1 占据 CommandBar 之上全部空间。padding 放在
         wrapper 上，NOT on the xterm container below. See the comment on
         xterm-container for why. The wrapper also takes the terminal
         background so the 4px inset stays seamless (no gap to the app
         chrome behind it) in the non-background-image case. -->
    <div
      class="terminal-wrap"
      :class="{ 'terminal-bg-transparent': hasBgImage }"
      :style="{ background: wrapBg }"
    >
      <!-- 背景图由壳层统一渲染（App.vue 的 .app-shell::before，响应式），
           此处只保持容器透明让图片透出 —— 避免双层叠加重影 -->
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
      <div ref="containerRef" class="xterm-container" :style="{ background: wrapBg }" />
      <ZmodemProgressOverlay :status="zmodemStatus" @cancel="onZmodemCancel" />
      <!-- rz upload chooser — files vs folder. Shown while the remote rz
           waits; dismissed on pick, cancel, or zmodem_end. -->
      <div v-if="uploadChooserOpen" class="upload-chooser">
        <span class="chooser-title">↑ ZMODEM 上传</span>
        <span class="chooser-hint">远端 rz 已就绪，选择要发送的内容：</span>
        <button type="button" class="chooser-btn" @click="pickUploadFiles">
          <el-icon :size="13"><Document /></el-icon>
          选择文件
        </button>
        <button type="button" class="chooser-btn" @click="pickUploadFolder">
          <el-icon :size="13"><FolderOpened /></el-icon>
          选择文件夹（递归上传）
        </button>
        <button type="button" class="chooser-btn chooser-cancel" @click="cancelNativeUpload">取消</button>
      </div>
    </div>

    <!-- CommandBar — only for SSH terminal tabs (connectionId provided) -->
    <CommandBar
      v-if="connectionId"
      :session-id="sessionId"
      :connection-id="connectionId"
      :conn-type="connType"
      :broadcast-targets="broadcastTargets"
      :status="status"
      @reconnect="emit('reconnect')"
      @open-ai="emit('open-ai')"
      @open-multiwindow="emit('open-multiwindow')"
      @open-quick-commands-manage="emit('open-quick-commands-manage', connectionId)"
      @screenshot="handleScreenshot"
      @register-refresh="onRegisterRefresh"
    />

    <!-- Screenshot status banner — transient toast that appears after the
         user clicks 📷. Sits at the bottom-right so it doesn't cover the
         terminal content. Auto-dismisses after 4s (see flashScreenshot). -->
    <div v-if="screenshotStatus" class="screenshot-banner" :class="`is-${screenshotStatus.state}`">
      <template v-if="screenshotStatus.state === 'capturing'">
        <span class="spinner">
          <el-icon :size="14"><Loading /></el-icon>
        </span>
        <span>正在截取终端...</span>
      </template>
      <template v-else-if="screenshotStatus.state === 'saved'">
        <span class="ok-mark">✓</span>
        <span class="saved-path">已保存：{{ screenshotStatus.path }}</span>
        <button type="button" class="open-btn" title="在文件管理器中显示" @click="openSavedPath(screenshotStatus.path)">
          打开
        </button>
      </template>
      <template v-else>
        <span class="err-mark">✕</span>
        <span class="err-msg">{{ screenshotStatus.message }}</span>
      </template>
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

/* When a background image is active, force xterm.js internal DOM
    layers (.xterm-viewport, .xterm-screen, canvas) transparent so the
    image shows through. xterm.css ships with solid background-color
    rules that must be overridden.（xterm 运行时生成的 DOM 不带 scoped
    标记，须用 :deep() 穿透） */
.terminal-bg-transparent :deep(.xterm-viewport),
.terminal-bg-transparent :deep(.xterm-screen),
.terminal-bg-transparent :deep(.xterm) {
  background: transparent !important;
}

.terminal-bg-transparent :deep(.xterm-viewport::-webkit-scrollbar-track) {
  background: transparent !important;
}

.terminal-wrap {
  flex: 1;
  min-height: 0;
  position: relative;
  padding: 4px;
}

.xterm-container {
  width: 100%;
  height: 100%;
  position: relative;
  z-index: 1;
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

/* rz 上传选择条（文件 vs 文件夹） */
.upload-chooser {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  background: var(--glass-bg);
  border-top: 1px solid var(--border-emphasis);
  padding: 10px 16px;
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  font-family: "Cascadia Code", Consolas, monospace;
  font-size: 12px;
  color: var(--text-primary);
  z-index: 10;
}

.chooser-title {
  color: var(--success);
  font-weight: 600;
}

.chooser-hint {
  color: var(--text-secondary);
}

.chooser-btn {
  background: transparent;
  border: 1px solid var(--accent-primary);
  color: var(--accent-primary);
  padding: 3px 12px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: 12px;
  font-family: inherit;
  white-space: nowrap;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.chooser-cancel {
  color: var(--error);
  border-color: var(--error);
}

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
  font-size: 12px;
  color: var(--text-primary);
  z-index: 1000;
  display: flex;
  align-items: center;
  gap: 8px;
}

.screenshot-banner.is-error {
  border-color: var(--error);
}

.screenshot-banner.is-saved {
  border-color: var(--success);
}

.spinner {
  animation: spin 1s linear infinite;
}

.ok-mark {
  color: var(--success);
}

.saved-path {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.open-btn {
  background: var(--bg-input);
  color: var(--text-secondary);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  padding: 3px 8px;
  font-size: 11px;
  cursor: pointer;
  white-space: nowrap;
}

.err-mark {
  color: var(--error);
}

.err-msg {
  flex: 1;
}
</style>
