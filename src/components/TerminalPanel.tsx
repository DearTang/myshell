import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import type { ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
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
  addCommandHistory,
} from "../api";
import type { ConnType } from "../api";
import { ZmodemBridge, type ZmodemStatus } from "../zmodem-bridge";
import { ZmodemProgressOverlay } from "./ZmodemProgressOverlay";
import { CommandBar } from "./CommandBar";
import { recordKeystroke } from "../utils/cmd-buffer";
import { useColorScheme } from "../hooks/useColorScheme";
import { useTheme } from "../hooks/useTheme";
import { useTerminalFont, resolveFontStack } from "../hooks/useTerminalFont";
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
 * or driver. We also set `cursorStyle: "bar"` (a 1-cell-wide vertical line)
 * at Terminal construction, which stays continuously visible even where a
 * solid block might blend in.
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

interface Props {
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
  /** Phase 3: registers the xterm instance with the App-level terminal
   * registry so the AI panel can read selection / recent output and paste
   * commands into it. Fired after the terminal opens; torn down on close. */
  onTerminalReady?: (sessionId: string, term: Terminal) => void;
  onTerminalGone?: (sessionId: string) => void;
}

export function TerminalPanel({ sessionId, connType, connectionId, fontOverride, broadcastTargets, active = true, onDisconnected, status, onReconnect, onTerminalReady, onTerminalGone, onOpenQuickCommandsManage, onOpenAi }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef(sessionId);
  const connTypeRef = useRef(connType);
  const broadcastRef = useRef<string[]>(broadcastTargets || []);
  const bridgeRef = useRef<ZmodemBridge | null>(null);
  const isZmodemRef = useRef(false);
  const abortTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
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
  });

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

    // Renderer choice depends on whether a background image is active:
    //  • Background image set → WebGL renderer. It redraws every frame, which
    //    is required for clean compositing under allowTransparency: the canvas
    //    renderer leaves ghosts/smearing on transparent backgrounds (input
    //    chars appear to "jump", worst on the local ConPTY path).
    //  • No background image → canvas renderer (xterm default). On an opaque
    //    terminal it has zero ghosting, AND it sidesteps the WebGL renderer's
    //    known wrap-edge / glyph-atlas offset artifacts that can shift a
    //    repainted wrapped line ("background shifts left"). Robustness over
    //    FPS when we don't need transparency.
    // Falls back to canvas if WebGL isn't usable (old GPU/drivers). Toggling
    // the background-image setting after open needs a tab reopen to switch
    // renderers (this runs once at mount).
    if (hasBgImage) {
      try {
        term.loadAddon(new WebglAddon());
      } catch (e) {
        console.warn("[TerminalPanel] WebGL renderer unavailable, using canvas:", e);
      }
    }

    // Block OSC 52 clipboard writes from the remote side. Without this,
    // a malicious SSH server can silently replace the user's clipboard
    // (e.g., swap a wallet address) by emitting the OSC 52 escape sequence.
    // Returning true tells xterm the sequence is fully handled — no
    // clipboard mutation occurs.
    term.parser.registerOscHandler(52, () => true);

    setTimeout(() => {
      fitAddon.fit();
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
    let closed = false;
    let firstOutputHandled = false;

    onSshOutput(sessionIdRef.current, (data) => {
      if (closed) return;
      term.write(data);
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
    onZmodemStart(sessionIdRef.current, (_direction) => {
      isZmodemRef.current = true;
      term.write("\r\n\x1b[36m[ZMODEM 传输开始 — 终端输入已屏蔽]\x1b[0m\r\n");
    })
      .then((un) => {
        if (closed) un();
        else unlistenZmodemStart = un;
      })
      .catch((e) => console.error("Failed to subscribe to zmodem_start:", e));

    onZmodemRaw(sessionIdRef.current, (data) => {
      bridge.feed(data);
    })
      .then((un) => {
        if (closed) un();
        else unlistenZmodemRaw = un;
      })
      .catch((e) => console.error("Failed to subscribe to zmodem_raw:", e));

    onZmodemEnd(sessionIdRef.current, () => {
      isZmodemRef.current = false;
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

    termRef.current = term;
    fitRef.current = fitAddon;
    onTerminalReady?.(sessionId, term);

    term.focus();

    return () => {
      closed = true;
      unlistenOutput?.();
      unlistenClosed?.();
      unlistenZmodemStart?.();
      unlistenZmodemRaw?.();
      unlistenZmodemEnd?.();
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
            // 5s safety net: if the backend doesn't emit zmodem_end (Rust state
            // machine stuck, network dropped mid-ZFIN), force the bridge back to
            // a clean state so the user can resume typing.
            if (abortTimeoutRef.current) clearTimeout(abortTimeoutRef.current);
            abortTimeoutRef.current = setTimeout(() => {
              bridgeRef.current?.reset();
              isZmodemRef.current = false;
              termRef.current?.write(
                "\r\n\x1b[33m[abort 超时 5s — 强制重置]\x1b[0m\r\n"
              );
              abortTimeoutRef.current = null;
            }, 5000);
          }}
        />
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
          onRegisterRefresh={(fn) => {
            refreshHistoryRef.current = fn;
          }}
        />
      )}
    </div>
  );
}
