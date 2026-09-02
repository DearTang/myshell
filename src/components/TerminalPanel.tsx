import { useEffect, useRef, useState } from "react";
import type { CSSProperties } from "react";
import { Terminal } from "@xterm/xterm";
import type { ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { CanvasAddon } from "@xterm/addon-canvas";
import { WebglAddon } from "@xterm/addon-webgl";
import { WebLinksAddon } from "@xterm/addon-web-links";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  sshSend,
  sshResize,
  localSend,
  localResize,
  onSshOutput,
  onSshClosed,
  onZmodemStart,
  onZmodemRaw,
  onZmodemEnd,
  onZmodemError,
  onZmodemOffer,
  onZmodemProgress,
  onZmodemFileComplete,
  zmodemAcceptOffer,
  zmodemStartUpload,
  sshSendZmodemAbort,
  addCommandHistory,
  saveScreenshot,
  getAttachmentDir,
  showInFolder,
} from "../api";
import { open } from "@tauri-apps/plugin-dialog";
import { captureTerminalToDataUrl } from "../utils/screenshot";
import type { ConnType } from "../api";
import { ZmodemBridge, type ZmodemStatus } from "../zmodem-bridge";
import { ZmodemProgressOverlay } from "./ZmodemProgressOverlay";
import { CommandBar } from "./CommandBar";
import { recordKeystroke } from "../utils/cmd-buffer";
import { useColorScheme } from "../hooks/useColorScheme";
import { useTheme } from "../hooks/useTheme";
import { useTerminalFont, resolveFontStack } from "../hooks/useTerminalFont";
import {
  type RendererBackend,
  resolveRenderer,
} from "../hooks/useRendererPref";
import "@xterm/xterm/css/xterm.css";

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
 * `cursorStyle: "bar"` / `cursorWidth` options below) — this function only
 * owns the colors.
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

/** Button style for the rz upload chooser bar (files vs folder vs cancel). */
const uploadChooserBtn: CSSProperties = {
  background: "transparent",
  border: "1px solid #89b4fa",
  color: "#89b4fa",
  padding: "3px 12px",
  borderRadius: 4,
  cursor: "pointer",
  fontSize: 12,
  fontFamily: "inherit",
  whiteSpace: "nowrap",
};

interface Props {
  /** STABLE tab identifier — never changes across reconnects. Used as the
   * React-effect mount key so the xterm instance survives a sessionId change
   * (reconnect). Without this, changing sessionId would tear down and rebuild
   * the whole terminal, losing scrollback history. */
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
   * transparency. See useRendererPref for the full rationale. Fixed per app
   * (not per-connection), but threaded through here so each tab reads the
   * current pref at mount. */
  rendererBackend?: RendererBackend;
  /** When non-empty (and contains more than just this tab's own sessionId),
   * every keystroke is mirrored to all listed sessions in addition to the
   * local one. The list is read live from a ref so toggling broadcast on/
   * off doesn't need to re-bind onData. */
  broadcastTargets?: string[];
  /** Whether this panel is the currently-visible tab. All tabs stay mounted
   * (so their xterm history + onSshOutput subscriptions persist across tab
   * switches); we only refit and refocus when this becomes the active tab. */
  active?: boolean;
  /** Callback fired when the SSH session is closed (server-initiated or
   * network error). Used to update the tab status to "disconnected". */
  onDisconnected?: () => void;
  /** Connection status: "connecting" | "connected" | "disconnected" | "error" */
  status?: "connecting" | "connected" | "disconnected" | "error";
  /** Callback to reconnect when status is disconnected/error */
  onReconnect?: () => void;
  /** Callback to open the quick-commands management panel (scoped to this
   * connection). Passed through to CommandBar's "管理" link. */
  onOpenQuickCommandsManage?: () => void;
  /** Open the docked AI assistant panel (the trigger button lives in
   * CommandBar, next to the history button). */
  onOpenAi?: () => void;
  /** Open the multi-window session picker. */
  onOpenMultiWindow?: () => void;
  /** Phase 3: registers the xterm instance with the App-level terminal
   * registry so the AI panel can read selection / recent output and paste
   * commands into it. Fired after the terminal opens; torn down on close. */
  onTerminalReady?: (sessionId: string, term: Terminal) => void;
  onTerminalGone?: (sessionId: string) => void;
  /** Tab display name — used as part of the screenshot filename when the
   * user hits the 📷 button in CommandBar. */
  connectionName?: string;
  /** Terminal text captured from the previous xterm instance before a
   * reconnect. If present on mount, it's written to the new terminal to
   * restore scrollback history. The caller is responsible for clearing it
   * (via onSnapshotConsumed) so it's only restored once. */
  reconnectSnapshot?: string;
  /** Called after reconnectSnapshot has been written to the new terminal,
   * so the parent can clear it from the tab state. */
  onSnapshotConsumed?: () => void;
}

export function TerminalPanel({ tabId, sessionId, connType, connectionId, fontOverride, rendererBackend, broadcastTargets, active = true, onDisconnected, status, onReconnect, onTerminalReady, onTerminalGone, onOpenQuickCommandsManage, onOpenAi, onOpenMultiWindow, connectionName, reconnectSnapshot, onSnapshotConsumed }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef(sessionId);
  const connTypeRef = useRef(connType);
  // Mirror of the `active` prop for use inside the mount effect's window-focus
  // listener (which is bound once at mount but must read the current active
  // state on every focus event to avoid stealing focus from another tab).
  const activeRef = useRef(active);
  const broadcastRef = useRef<string[]>(broadcastTargets || []);
  const bridgeRef = useRef<ZmodemBridge | null>(null);
  const isZmodemRef = useRef(false);
  const abortTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Native ZMODEM state. Both download (sz) and upload (rz) are handled
  // entirely in Rust — data never crosses IPC. The frontend only prompts
  // for a save dir (download) or file selection (upload) and renders progress.
  const nativeDownloadRef = useRef(false);
  const nativeDirRef = useRef<string | null>(null);
  const nativeCancelledRef = useRef(false);
  // Timers for the first-output cols re-sync (see onSshOutput below). Held in
  // a ref so the [sessionId] cleanup can clear them if the panel tears down
  // before the last delayed sync fires.
  const firstSyncTimersRef = useRef<ReturnType<typeof setTimeout>[]>([]);
  // Command-history keystroke buffer. These refs are NOT React state —
  // we don't want every keystroke to trigger a re-render. The buffer is
  // flushed on Enter and the resulting command is sent to the backend.
  const cmdBufRef = useRef<string>("");
  const ansiEscRef = useRef<boolean>(false);
  const connectionIdRef = useRef(connectionId);
  // Callback registered by CommandBar on mount so we can trigger a history
  // list refresh after recording a new command.
  const refreshHistoryRef = useRef<(() => void) | null>(null);

  // Keep the live ref in sync with the latest prop so onData (bound once at
  // mount) sees updates without rebinding.
  broadcastRef.current = broadcastTargets || [];
  connectionIdRef.current = connectionId;
  activeRef.current = active;

  // Color scheme & background image from context
  const { getActivePalette, bgImage } = useColorScheme();
  const { theme } = useTheme();
  const { fontFamily: globalFontFamily } = useTerminalFont();
  // Per-connection override wins over the global setting; empty → global.
  const fontFamily = fontOverride ? resolveFontStack(fontOverride) : globalFontFamily;

  // Derive the terminal theme from the active palette for the current mode.
  // Re-resolved on every render so a palette switch instantly rebinds.
  const activePalette = getActivePalette();
  const variant = theme === "dark" ? activePalette.dark : activePalette.light;
  const terminalTheme = variant.terminal;
  const hasBgImage = bgImage.dataUrl !== null;

  const [zmodemStatus, setZmodemStatus] = useState<ZmodemStatus>({
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
  const [uploadChooserOpen, setUploadChooserOpen] = useState(false);

  /** Hand the picked files/folders to the native Rust uploader, or abort
   * when the user dismissed the picker without a selection. */
  const commitNativeUpload = async (paths: string[]) => {
    setUploadChooserOpen(false);
    if (paths.length === 0) {
      sshSendZmodemAbort(sessionIdRef.current).catch(() => {});
      return;
    }
    try {
      await zmodemStartUpload(sessionIdRef.current, paths);
      // Activate the progress overlay. bytesTotal starts at 0 — the first
      // zmodem_progress event carries the real file size (the backend
      // stats/expands the selection). onZmodemProgress ignores events while
      // active is false, so this must be set before bytes flow.
      const firstName = paths[0].split(/[\\/]/).pop() || "file";
      setZmodemStatus({
        active: true,
        direction: "upload",
        currentFile: paths.length === 1 ? firstName : `${firstName} 等 ${paths.length} 项`,
        bytesTransferred: 0,
        bytesTotal: 0,
        speedBps: 0,
        error: null,
        startTime: Date.now(),
      });
    } catch (e) {
      console.error("native upload start failed:", e);
      sshSendZmodemAbort(sessionIdRef.current).catch(() => {});
    }
  };

  const pickUploadFiles = async () => {
    const selected = await open({ multiple: true });
    const paths: string[] = !selected ? [] : Array.isArray(selected) ? selected : [selected];
    await commitNativeUpload(paths);
  };

  const pickUploadFolder = async () => {
    const selected = await open({ directory: true, multiple: true });
    const paths: string[] = !selected ? [] : Array.isArray(selected) ? selected : [selected];
    await commitNativeUpload(paths);
  };

  const cancelNativeUpload = () => {
    setUploadChooserOpen(false);
    sshSendZmodemAbort(sessionIdRef.current).catch(() => {});
  };

  // Screenshot status banner — shown transiently after the user clicks 📷.
  // `state` is one of: "capturing" | "saved" | "error". Auto-clears after 4s
  // via the timeout stored in `screenshotTimerRef`.
  type ScreenshotState = { state: "capturing" } | { state: "saved"; path: string } | { state: "error"; message: string } | null;
  const [screenshotStatus, setScreenshotStatus] = useState<ScreenshotState>(null);
  const screenshotTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  /** Capture the terminal viewport and save it to the attachment directory.
   * Bound to the 📷 button in CommandBar. No-op if the xterm instance isn't
   * mounted yet (rare race during tab open). */
  const handleScreenshot = async () => {
    const term = termRef.current;
    if (!term) {
      flashScreenshot({ state: "error", message: "终端尚未就绪，请稍候重试" });
      return;
    }
    // Clear any previous banner + timer.
    if (screenshotTimerRef.current) clearTimeout(screenshotTimerRef.current);
    setScreenshotStatus({ state: "capturing" });

    try {
      const dataUrl = await captureTerminalToDataUrl(term);
      if (!dataUrl) {
        flashScreenshot({ state: "error", message: "截图失败：无法捕获终端画面" });
        return;
      }
      const path = await saveScreenshot(dataUrl, connectionName || sessionId);
      flashScreenshot({ state: "saved", path });
    } catch (e: any) {
      const msg = e?.message ? String(e.message) : String(e);
      // The Rust side returns a localized error when the attachment dir isn't
      // configured — surface it verbatim so the user knows what to fix.
      flashScreenshot({ state: "error", message: msg.includes("附件目录") ? msg : `保存失败: ${msg}` });
    }
  };

  /** Set the banner and auto-clear it after 4s. */
  function flashScreenshot(s: ScreenshotState) {
    setScreenshotStatus(s);
    if (screenshotTimerRef.current) clearTimeout(screenshotTimerRef.current);
    screenshotTimerRef.current = setTimeout(() => setScreenshotStatus(null), 4000);
  }

  sessionIdRef.current = sessionId;
  connTypeRef.current = connType;

  // Route send/resize to the ssh_* or local_* backend depending on the tab's
  // connection type. Read via ref so the closures bound in the [sessionId]
  // effect always hit the right backend (connType is fixed per tab in
  // practice, but the ref keeps it honest).
  const sendTo = (sid: string, data: string) =>
    connTypeRef.current === "local" ? localSend(sid, data) : sshSend(sid, data);
  const resizeTo = (sid: string, cols: number, rows: number) =>
    connTypeRef.current === "local" ? localResize(sid, cols, rows) : sshResize(sid, cols, rows);

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new Terminal({
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
      fontFamily,
      theme: {
        ...terminalTheme,
        ...forceVisibleCursor(terminalTheme),
        selectionBackground: visibleSelection(terminalTheme.selectionBackground),
        ...(hasBgImage ? { background: "rgba(0, 0, 0, 0)" } : {}),
      },
      allowProposedApi: true,
      allowTransparency: hasBgImage,
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();

    term.loadAddon(fitAddon);
    term.loadAddon(webLinksAddon);
    term.open(containerRef.current);

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
    // runs once at mount).
    const renderer = resolveRenderer(
      rendererBackend ?? "auto",
      hasBgImage
    );
    const loadRenderer = (): void => {
      if (renderer === "webgl") {
        try {
          term.loadAddon(new WebglAddon());
          return;
        } catch (e) {
          console.warn(
            "[TerminalPanel] WebGL renderer unavailable, falling back to canvas:",
            e
          );
        }
      }
      if (renderer === "dom") {
        // No addon to load — xterm 5.x's built-in DOM renderer is the default
        // when no renderer addon is registered. Intentionally load nothing.
        return;
      }
      // canvas (default + webgl/canvas fallback)
      try {
        term.loadAddon(new CanvasAddon());
      } catch (e) {
        // Extremely unlikely (canvas is the most compatible), but don't let a
        // renderer failure kill the terminal — fall through to the DOM default.
        console.warn(
          "[TerminalPanel] Canvas renderer unavailable, using DOM default:",
          e
        );
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
        try { term.write(d); } catch { /* renderer still settling */ }
      }
      pendingData.length = 0;
    };
    requestAnimationFrame(() =>
      requestAnimationFrame(() => flush()),
    );
    // Safety net: if rAF never fires (background tab), force-flush after 300ms.
    setTimeout(flush, 300);

    // Block OSC 52 clipboard writes from the remote side. Without this,
    // a malicious SSH server can silently replace the user's clipboard
    // (e.g., swap a wallet address) by emitting the OSC 52 escape sequence.
    // Returning true tells xterm the sequence is fully handled — no
    // clipboard mutation occurs.
    term.parser.registerOscHandler(52, () => true);

    setTimeout(() => {
      // Guard against xterm.js race: fit() calls Viewport.syncScrollArea
      // which accesses renderer.dimensions — if the renderer isn't ready
      // yet (canvas init async), this throws "Cannot read properties of
      // undefined (reading 'dimensions')". Wrap in try/catch so the error
      // doesn't crash the WebView2 render process.
      try {
        fitAddon.fit();
      } catch {
        // Retry once after a longer delay — by then the renderer should be ready.
        setTimeout(() => { try { fitAddon.fit(); } catch { /* give up */ } }, 300);
      }
      resizeTo(sessionIdRef.current, term.cols, term.rows).catch(() => {});
    }, 100);

    // Handle user input. In ZMODEM mode, swallow keystrokes so the user
    // can't corrupt the protocol stream by typing into the terminal.
    // When broadcast targets are configured, mirror the keystrokes to all
    // of them in parallel — the local session is included in the list so
    // a single loop handles both single + multi target cases.
    term.onData((data) => {
      if (isZmodemRef.current) return;
      const targets = broadcastRef.current;
      const destinations =
        targets.length > 0 ? targets : [sessionIdRef.current];
      // Fire-and-forget. Promise.allSettled already swallows per-target
      // rejections (a target session may have just closed), so there's
      // nothing to do on completion.
      void Promise.allSettled(destinations.map((sid) => sendTo(sid, data)));

      // Record keystrokes for command-history. This runs AFTER the send
      // so we don't block the critical path. We only record for the
      // local tab's connectionId (not broadcast targets) — each tab
      // records its own perspective.
      if (connectionIdRef.current) {
        recordKeystroke(data, cmdBufRef, ansiEscRef, (cmd) => {
          addCommandHistory(connectionIdRef.current, cmd)
            .then(() => {
              refreshHistoryRef.current?.();
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
      const container = containerRef.current;
      if (!container) return;
      const w = container.clientWidth;
      const h = container.clientHeight;
      if (w < 80 || h < 40) {
        console.warn(
          `[TerminalPanel] skipping fit — container too small (${w}x${h})`
        );
        return;
      }
      const prevCols = term.cols;
      try {
        fitAddon.fit();
      } catch (e) {
        console.warn("[TerminalPanel] fit() threw:", e);
        return;
      }
      // Log suspicious shrinkage for diagnosis. Normal fits don't drop cols
      // by more than a few; a 60→11 drop is the bug we're hunting.
      if (term.cols < prevCols - 10) {
        console.warn(
          `[TerminalPanel] cols shrank ${prevCols}→${term.cols} at container ${w}x${h}`
        );
      }
      resizeTo(sessionIdRef.current, term.cols, term.rows).catch(() => {});
      for (const sid of broadcastRef.current) {
        if (sid !== sessionIdRef.current) {
          resizeTo(sid, term.cols, term.rows).catch(() => {});
        }
      }
    });
    resizeObserver.observe(containerRef.current);

    const bridge = new ZmodemBridge(sessionIdRef.current);
    bridgeRef.current = bridge;
    const unsubscribeStatus = bridge.onStatus(setZmodemStatus);

    let unlistenOutput: UnlistenFn | null = null;
    let unlistenClosed: UnlistenFn | null = null;
    let unlistenZmodemStart: UnlistenFn | null = null;
    let unlistenZmodemRaw: UnlistenFn | null = null;
    let unlistenZmodemEnd: UnlistenFn | null = null;
    let unlistenZmodemError: UnlistenFn | null = null;
    let unlistenZmodemOffer: UnlistenFn | null = null;
    let unlistenZmodemProgress: UnlistenFn | null = null;
    let unlistenZmodemFileComplete: UnlistenFn | null = null;
    let closed = false;
    let firstOutputHandled = false;

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

    onSshOutput(sessionIdRef.current, (data) => {
      if (closed) return;
      const filtered = filterSentinel(data);
      if (!filtered) return; // entire chunk was sentinel noise — skip
      if (!rendererReady) {
        pendingData.push(filtered);
        return;
      }
      term.write(filtered);
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
            fitAddon.fit();
          } catch {
            /* container mid-transition */
          }
          resizeTo(sessionIdRef.current, term.cols, term.rows).catch(() => {});
        };
        syncRealCols();
        firstSyncTimersRef.current = [
          setTimeout(syncRealCols, 250),
          setTimeout(syncRealCols, 600),
        ];
      }
    })
      .then((un) => {
        if (closed) un();
        else unlistenOutput = un;
      })
      .catch((e) => console.error("Failed to subscribe to ssh_output:", e));

    onSshClosed(sessionIdRef.current, () => {
      if (closed) return;
      closed = true;
      term.write("\r\n\x1b[31m[Connection closed]\x1b[0m\r\n");
      term.options.cursorBlink = false;
      onDisconnected?.();
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
    // Join the picked dir with an offer name. `sz -r 目录` offers carry
    // relative subpaths ("dir/sub/file.txt") — sanitize each segment and
    // keep the tree: empty / "." / ".." / drive-letter segments are dropped,
    // so the path can never escape the picked directory. All segments gone
    // (pathological name) → benign flat fallback, same as before.
    const joinZmodemPath = (dir: string, name: string): string => {
      const sep = dir.includes("/") && !dir.includes("\\") ? "/" : "\\";
      const trimmed = dir.endsWith(sep) ? dir.slice(0, -1) : dir;
      const segs = name
        .split(/[\\/]/)
        .map((s) => s.trim().replace(/[. ]+$/, ""))
        .filter((s) => s && s !== "." && s !== ".." && !s.includes(":"));
      if (segs.length === 0) return `${trimmed}${sep}myshell-download.bin`;
      return `${trimmed}${sep}${segs.join(sep)}`;
    };
    const promptNativeDir = async (): Promise<string | null> => {
      if (nativeCancelledRef.current) return null;
      if (nativeDirRef.current !== null) return nativeDirRef.current;
      const dir = await open({ directory: true });
      if (typeof dir === "string" && dir.length > 0) {
        nativeDirRef.current = dir;
        return dir;
      }
      nativeCancelledRef.current = true;
      return null;
    };

    onZmodemStart(sessionIdRef.current, (direction) => {
      isZmodemRef.current = true;
      nativeDownloadRef.current = direction === "download";
      if (direction === "download") {
        nativeDirRef.current = null;
        nativeCancelledRef.current = false;
      } else if (direction === "upload") {
        // Native upload: show the inline files-vs-folder chooser (the OS
        // dialog can't select both kinds at once). The chosen paths then go
        // through zmodemStartUpload — folders are recursed in Rust.
        setUploadChooserOpen(true);
      }
      term.write("\r\n\x1b[36m[ZMODEM 传输开始 — 终端输入已屏蔽]\x1b[0m\r\n");
    })
      .then((un) => {
        if (closed) un();
        else unlistenZmodemStart = un;
      })
      .catch((e) => console.error("Failed to subscribe to zmodem_start:", e));

    onZmodemRaw(sessionIdRef.current, (_data) => {
      // Native Rust handles both upload and download protocol bytes.
      // Nothing to feed to the JS bridge.
    })
      .then((un) => {
        if (closed) un();
        else unlistenZmodemRaw = un;
      })
      .catch((e) => console.error("Failed to subscribe to zmodem_raw:", e));

    // Native download: a file is offered — prompt for dir (once), accept/skip.
    onZmodemOffer(sessionIdRef.current, (p) => {
      void (async () => {
        const dir = await promptNativeDir();
        const path = dir !== null ? joinZmodemPath(dir, p.fileName) : null;
        if (path !== null) {
          setZmodemStatus({
            active: true,
            direction: "download",
            currentFile: p.fileName,
            bytesTransferred: 0,
            bytesTotal: p.fileSize,
            speedBps: 0,
            error: null,
            startTime: Date.now(),
          });
        }
        zmodemAcceptOffer(sessionIdRef.current, path).catch((e) =>
          console.error("zmodemAcceptOffer failed:", e)
        );
      })();
    })
      .then((un) => {
        if (closed) un();
        else unlistenZmodemOffer = un;
      })
      .catch((e) => console.error("Failed to subscribe to zmodem_offer:", e));

    // Native download: live byte progress.
    onZmodemProgress(sessionIdRef.current, (p) => {
      setZmodemStatus((prev) => {
        if (!prev.active) return prev;
        const elapsed = (Date.now() - prev.startTime) / 1000;
        const speed = elapsed > 0 ? p.bytesTransferred / elapsed : 0;
        return {
          ...prev,
          bytesTransferred: p.bytesTransferred,
          bytesTotal: p.bytesTotal,
          speedBps: speed,
        };
      });
    })
      .then((un) => {
        if (closed) un();
        else unlistenZmodemProgress = un;
      })
      .catch((e) => console.error("Failed to subscribe to zmodem_progress:", e));

    // Native download: single file finished — update progress to 100%.
    onZmodemFileComplete(sessionIdRef.current, (p) => {
      setZmodemStatus((prev) => {
        if (!prev.active) return prev;
        return {
          ...prev,
          bytesTransferred: p.bytesWritten,
          bytesTotal: p.bytesWritten,
          currentFile: p.fileName,
        };
      });
    })
      .then((un) => {
        if (closed) un();
        else unlistenZmodemFileComplete = un;
      })
      .catch((e) => console.error("Failed to subscribe to zmodem_file_complete:", e));

    onZmodemEnd(sessionIdRef.current, () => {
      isZmodemRef.current = false;
      nativeDownloadRef.current = false;
      nativeDirRef.current = null;
      nativeCancelledRef.current = false;
      setUploadChooserOpen(false);
      bridge.reset();
      // Cancel any pending force-reset — the Rust backend reported an
      // orderly ZFIN/CAN sequence so the bridge is clean.
      if (abortTimeoutRef.current) {
        clearTimeout(abortTimeoutRef.current);
        abortTimeoutRef.current = null;
      }
      term.write("\r\n\x1b[36m[ZMODEM 传输结束]\x1b[0m\r\n");
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
    onZmodemError(sessionIdRef.current, (message) => {
      setZmodemStatus((prev) => ({ ...prev, error: message }));
      term.write(`\r\n\x1b[31m[ZMODEM 错误] ${message}\x1b[0m\r\n`);
    })
      .then((un) => {
        if (closed) un();
        else unlistenZmodemError = un;
      })
      .catch((e) => console.error("Failed to subscribe to zmodem_error:", e));

    termRef.current = term;
    fitRef.current = fitAddon;

    // Restore scrollback from a previous terminal instance after a reconnect.
    // The snapshot was captured by reconnectOne (reading the old xterm's
    // buffer) before the sessionId changed. Write it now so the user sees
    // their history continuity, then mark it consumed.
    if (reconnectSnapshot) {
      term.write(reconnectSnapshot);
      term.write("\r\n\x1b[33m[—— 以上为重连前历史，连接已重建 ——]\x1b[0m\r\n");
      onSnapshotConsumed?.();
    }

    onTerminalReady?.(sessionId, term);

    term.focus();
    // Retry focus after a short delay. The initial term.focus() above can run
    // before the xterm DOM layers are fully attached, so the internal cursor
    // blink state machine never starts — leaving the cursor frozen (not
    // blinking) until something else (tab switch, clicking into the app)
    // re-triggers focus. The retry ensures the blink timer is armed even on a
    // slow first paint. 150ms is well past xterm's own open/refresh cycle.
    const focusRetry = setTimeout(() => {
      if (!closed) term.focus();
    }, 150);

    // Window focus/blur — Chromium suspends timers (including xterm's cursor
    // blink interval) when the window loses focus, and does NOT automatically
    // resume the blink state machine when focus returns. Without re-focusing
    // here, the cursor stays frozen (solid or invisible) after switching away
    // from the app and back. Only act when this tab is the active one, so we
    // don't steal focus from another tab the user switched to.
    const onWindowFocus = () => {
      if (activeRef.current && !closed) {
        term.focus();
      }
    };
    window.addEventListener("focus", onWindowFocus);

    return () => {
      closed = true;
      clearTimeout(focusRetry);
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
      term.dispose();
      if (abortTimeoutRef.current) {
        clearTimeout(abortTimeoutRef.current);
        abortTimeoutRef.current = null;
      }
      firstSyncTimersRef.current.forEach(clearTimeout);
      firstSyncTimersRef.current = [];
      onTerminalGone?.(sessionId);
      termRef.current = null;
      fitRef.current = null;
      bridgeRef.current = null;
    };
  }, [sessionId]);

  // Live theme update: when the user switches palette or toggles dark/light,
  // update the existing terminal theme without losing scrollback.
  // xterm.js 5.x supports live `term.options.theme` mutation.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;

    // NOTE: must be a real alpha-0 color, not the "transparent" keyword — the
    // WebGL renderer can't parse "transparent" (canvas could), and falls back
    // to an opaque black clearColor, hiding the background image.
    const currentBg = hasBgImage ? "rgba(0, 0, 0, 0)" : terminalTheme.background;
    term.options.theme = {
      ...terminalTheme,
      ...forceVisibleCursor(terminalTheme),
      background: currentBg,
      selectionBackground: visibleSelection(terminalTheme.selectionBackground),
    };
    term.options.allowTransparency = hasBgImage;

    // Also sync the container background
    if (containerRef.current) {
      containerRef.current.style.background = currentBg;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activePalette.id, theme, hasBgImage]);

  // Live font update: apply a newly chosen terminal font to the existing
  // terminal without re-creating it. xterm.js re-rasterizes glyphs when
  // fontFamily changes, so the new font (and its Nerd Font glyphs) shows up
  // immediately on open tabs.
  //
  // CRITICAL — refit after the font change. xterm re-measures the cell width
  // when fontFamily changes but does NOT recompute cols, so cols × cellWidth
  // silently drifts from the container width. The terminal then keeps
  // reporting stale cols to the PTY; pwsh's PSReadLine absolutely-positions
  // the cursor on EVERY keystroke and repaints the whole input line against
  // those stale cols — so glyphs land in the wrong cells. On the transparent
  // background + background-image path those mis-drawn glyphs float over the
  // wrong part of the image, which reads as "characters typed beside the
  // background, pushing it left" (worst when input nears the right margin).
  //
  // The same drift bites the INITIAL fit: Nerd Font files load asynchronously,
  // so the mount-time fit() (100ms) can measure against the fallback font and
  // be wrong the instant the real font finishes loading. This effect runs on
  // mount too (fontFamily's initial value), so waiting on document.fonts.ready
  // covers both the first-paint race and later font switches.
  useEffect(() => {
    const term = termRef.current;
    const fit = fitRef.current;
    if (!term) return;
    term.options.fontFamily = fontFamily;

    const refit = () => {
      if (!fit) return;
      const container = containerRef.current;
      // Same tiny-size guard as the ResizeObserver — don't fit against a
      // collapsed container (tab hidden mid-transition) and poison cols.
      if (!container || container.clientWidth < 80 || container.clientHeight < 40) return;
      try {
        fit.fit();
      } catch {
        return;
      }
      resizeTo(sessionIdRef.current, term.cols, term.rows).catch(() => {});
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
  }, [fontFamily]);

  // Active-tab transitions: refit on show (the last fit ran against a
  // display:none container with zero geometry) and grab focus so the user
  // can type immediately. Blur on hide so keystrokes don't leak to an
  // invisible terminal. Mount/unmount is handled by [sessionId] above —
  // this only fires when visibility flips without remount.
  useEffect(() => {
    const term = termRef.current;
    const fit = fitRef.current;
    if (!term || !fit) return;

    if (active) {
      // Guard against container not yet sized or terminal disposed
      const container = containerRef.current;
      if (!container) return;

      const w = container.clientWidth;
      const h = container.clientHeight;
      if (w < 80 || h < 40) {
        // Container not yet sized, skip fit to avoid RenderService error
        return;
      }

      // Additional safety: check if terminal is still usable
      if (!term.element || !term.element.parentElement) {
        return;
      }

      try {
        fit.fit();
      } catch (error) {
        // Silently ignore fit errors during transitions
        return;
      }

      resizeTo(sessionIdRef.current, term.cols, term.rows).catch(() => {});
      // Sync the freshly-refit cols to broadcast members so their shells
      // output in the same width — covers the case where this tab was
      // inactive through a window resize and the others had stale dims.
      for (const sid of broadcastRef.current) {
        if (sid !== sessionIdRef.current) {
          resizeTo(sid, term.cols, term.rows).catch(() => {});
        }
      }
      term.focus();
    } else {
      term.blur();
    }
  }, [active]);

  // Broadcast membership change (a tab joined/left the group): push our
  // current cols to the other members right away. This is the only signal
  // we get when the user toggles 📡 — ResizeObserver won't fire because
  // nothing resized, but the shells still need to align before the next
  // broadcast keystroke lands.
  //
  // CRITICAL: gate on `active`. An inactive tab's term.cols is a stale/
  // possibly-zero value (its container is display:none — ResizeObserver
  // saw a 0x0 size transition and may have poisoned cols). Letting it push
  // that zero to other members collapses everyone's COLUMNS and turns ls
  // output into one-file-per-line. Only the active (visible, freshly-fit)
  // tab is a reliable source of truth for cols.
  const targets = broadcastTargets ?? [];
  const broadcastKey = targets.join(",");
  useEffect(() => {
    if (!active) return;
    const term = termRef.current;
    if (!term) return;
    // Defensive: if our own cols is implausibly small, don't push it —
    // we'd just propagate garbage.
    if (term.cols < 20) return;
    for (const sid of targets) {
      if (sid !== sessionIdRef.current) {
        resizeTo(sid, term.cols, term.rows).catch(() => {});
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [broadcastKey, active]);

  return (
    <div
      style={{
        position: "relative",
        width: "100%",
        height: "100%",
        display: "flex",
        flexDirection: "column",
      }}
    >
      {/* When a background image is active, force xterm.js internal DOM
          layers (.xterm-viewport, .xterm-screen, canvas) transparent so the
          image shows through.  xterm.css ships with solid background-color
          rules that must be overridden. */}
      {hasBgImage && (
        <style>{`
          .terminal-bg-transparent .xterm-viewport,
          .terminal-bg-transparent .xterm-screen,
          .terminal-bg-transparent .xterm {
            background: transparent !important;
          }
          .terminal-bg-transparent .xterm-viewport::-webkit-scrollbar-track {
            background: transparent !important;
          }
        `}</style>
      )}
      {/* xterm container — flex:1 so it takes all space above CommandBar.
          padding lives HERE on the wrapper, NOT on the xterm container below.
          See the comment on containerRef for why. The wrapper also takes the
          terminal background so the 4px inset stays seamless (no gap to the
          app chrome behind it) in the non-background-image case. */}
      <div
        className={hasBgImage ? "terminal-bg-transparent" : undefined}
        style={{
          flex: 1,
          minHeight: 0,
          position: "relative",
          padding: 4,
          background: hasBgImage ? "transparent" : terminalTheme.background,
        }}
      >
        {/* Background image layer — rendered behind the terminal when configured */}
        {hasBgImage && (
          <div
            style={{
              position: "absolute",
              inset: 0,
              backgroundImage: `url(${bgImage.dataUrl})`,
              backgroundSize: "cover",
              backgroundPosition: "center",
              backgroundRepeat: "no-repeat",
              opacity: bgImage.opacity,
              pointerEvents: "none",
              zIndex: 0,
            }}
          />
        )}
        <div
          ref={containerRef}
          style={{
            width: "100%",
            height: "100%",
            background: hasBgImage ? "transparent" : terminalTheme.background,
            // NO padding here — the 4px visual inset lives on the wrapper
            // above. xterm's .xterm element fills this container's content
            // box, and FitAddon reads getComputedStyle(thisContainer).width as
            // the usable width. Under the global `* { box-sizing: border-box }`
            // rule, padding on THIS div would be included in that width but
            // excluded from .xterm's actual render area, so FitAddon over-
            // counts cols by ~1 (8px / cellWidth). xterm then tells the PTY
            // one more column than it can actually paint: the last column
            // renders off-canvas and PSReadLine mis-positions the cursor on
            // every keystroke, so the input line redraws with characters
            // spilling out and the background shifted left ("字符跳出界面 /
            // 背景左移", worst on the local PowerShell/ConPTY path). Keeping
            // the inset on the wrapper leaves the column math exact.
            position: "relative",
            zIndex: 1,
          }}
        />
        <ZmodemProgressOverlay
          status={zmodemStatus}
          onCancel={() => {
            bridgeRef.current?.abort();
            // Safety net: zmodem_finish normally triggers zmodem_end within
            // one IPC round-trip. If it somehow doesn't arrive (session dead,
            // IPC failure), force-reset after 2s so the terminal is usable.
            if (abortTimeoutRef.current) clearTimeout(abortTimeoutRef.current);
            abortTimeoutRef.current = setTimeout(() => {
              bridgeRef.current?.reset();
              isZmodemRef.current = false;
              termRef.current?.write(
                "\r\n\x1b[33m[abort 超时 — 强制重置]\x1b[0m\r\n"
              );
              abortTimeoutRef.current = null;
            }, 2000);
          }}
        />
        {/* rz upload chooser — files vs folder. Shown while the remote rz
            waits; dismissed on pick, cancel, or zmodem_end. */}
        {uploadChooserOpen && (
          <div
            style={{
              position: "absolute",
              left: 0,
              right: 0,
              bottom: 0,
              background: "rgba(30, 30, 46, 0.96)",
              borderTop: "1px solid #45475a",
              padding: "10px 16px",
              display: "flex",
              alignItems: "center",
              gap: 10,
              flexWrap: "wrap",
              fontFamily: "'Cascadia Code', Consolas, monospace",
              fontSize: 12,
              color: "#cdd6f4",
              zIndex: 10,
            }}
          >
            <span style={{ color: "#a6e3a1", fontWeight: 600 }}>↑ ZMODEM 上传</span>
            <span style={{ color: "#a6adc8" }}>远端 rz 已就绪，选择要发送的内容：</span>
            <button onClick={pickUploadFiles} style={uploadChooserBtn}>
              📄 选择文件
            </button>
            <button onClick={pickUploadFolder} style={uploadChooserBtn}>
              📁 选择文件夹（递归上传）
            </button>
            <button
              onClick={cancelNativeUpload}
              style={{ ...uploadChooserBtn, color: "#f38ba8", borderColor: "#f38ba8" }}
            >
              取消
            </button>
          </div>
        )}
      </div>
      {/* CommandBar — only for SSH terminal tabs (connectionId provided) */}
      {connectionId && (
        <CommandBar
          sessionId={sessionId}
          connectionId={connectionId}
          connType={connType}
          broadcastTargets={broadcastTargets}
          status={status}
          onReconnect={onReconnect}
          onOpenQuickCommandsManage={onOpenQuickCommandsManage}
          onOpenAi={onOpenAi}
          onOpenMultiWindow={onOpenMultiWindow}
          onScreenshot={handleScreenshot}
          onRegisterRefresh={(fn) => {
            refreshHistoryRef.current = fn;
          }}
        />
      )}

      {/* Screenshot status banner — transient toast that appears after the
          user clicks 📷. Sits at the bottom-right so it doesn't cover the
          terminal content. Auto-dismisses after 4s (see flashScreenshot). */}
      {screenshotStatus && (
        <div
          style={{
            position: "absolute",
            bottom: 12,
            right: 12,
            maxWidth: 420,
            padding: "10px 14px",
            borderRadius: "var(--radius-md)",
            background: "var(--bg-elevated)",
            border: `1px solid ${
              screenshotStatus.state === "error" ? "var(--error)"
              : screenshotStatus.state === "saved" ? "var(--success)"
              : "var(--border-default)"
            }`,
            boxShadow: "var(--shadow-md)",
            fontSize: 12,
            color: "var(--text-primary)",
            zIndex: 1000,
            display: "flex",
            alignItems: "center",
            gap: 8,
          }}
        >
          {screenshotStatus.state === "capturing" && (
            <>
              <span style={{ animation: "spin 1s linear infinite" }}>⏳</span>
              <span>正在截取终端...</span>
            </>
          )}
          {screenshotStatus.state === "saved" && (
            <>
              <span style={{ color: "var(--success)" }}>✓</span>
              <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                已保存：{screenshotStatus.path}
              </span>
              <button
                onClick={async () => {
                  // Open the containing folder in Explorer, selecting the file.
                  try {
                    const path = screenshotStatus?.state === "saved" ? screenshotStatus.path : "";
                    if (path) await showInFolder(path);
                  } catch {
                    /* best-effort — ignore if open fails */
                  }
                }}
                title="在文件管理器中显示"
                style={{
                  background: "var(--bg-input)",
                  color: "var(--text-secondary)",
                  border: "1px solid var(--border-default)",
                  borderRadius: "var(--radius-sm)",
                  padding: "3px 8px",
                  fontSize: 11,
                  cursor: "pointer",
                  whiteSpace: "nowrap",
                }}
              >
                打开
              </button>
            </>
          )}
          {screenshotStatus.state === "error" && (
            <>
              <span style={{ color: "var(--error)" }}>✕</span>
              <span style={{ flex: 1 }}>{screenshotStatus.message}</span>
            </>
          )}
        </div>
      )}
    </div>
  );
}
