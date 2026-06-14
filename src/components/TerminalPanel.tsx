import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  sshSend,
  sshResize,
  onSshOutput,
  onSshClosed,
  onZmodemStart,
  onZmodemRaw,
  onZmodemEnd,
} from "../api";
import { ZmodemBridge, type ZmodemStatus } from "../zmodem-bridge";
import { ZmodemProgressOverlay } from "./ZmodemProgressOverlay";
import "@xterm/xterm/css/xterm.css";

interface Props {
  sessionId: string;
  /** When non-empty (and contains more than just this tab's own sessionId),
   * every keystroke is mirrored to all listed sessions in addition to the
   * local one. The list is read live from a ref so toggling broadcast on/
   * off doesn't need to re-bind onData. */
  broadcastTargets?: string[];
  /** Whether this panel is the currently-visible tab. All tabs stay mounted
   * (so their xterm history + onSshOutput subscriptions persist across tab
   * switches); we only refit and refocus when this becomes the active tab. */
  active?: boolean;
}

/**
 * Shell init sequence injected right after the PTY comes up. Enables ANSI
 * color on common GNU/BSD coreutils (ls, grep) and re-sources the user's
 * bashrc so PS1 + custom aliases are picked up. Designed to fail silently
 * on sh/dash/fish — the `2>/dev/null` swallow means the user still gets a
 * working terminal, just without color.
 */
const SHELL_INIT_SEQ =
  "export FORCE_COLOR=1; export CLICOLOR=1; " +
  "alias ls='ls --color=auto' grep='grep --color=auto' 2>/dev/null; " +
  "[ -n \"$BASH_VERSION\" ] && . ~/.bashrc 2>/dev/null; " +
  "clear;\n";

export function TerminalPanel({ sessionId, broadcastTargets, active = true }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const sessionIdRef = useRef(sessionId);
  const broadcastRef = useRef<string[]>(broadcastTargets || []);
  const bridgeRef = useRef<ZmodemBridge | null>(null);
  const isZmodemRef = useRef(false);
  const abortTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Keep the live ref in sync with the latest prop so onData (bound once at
  // mount) sees updates without rebinding.
  broadcastRef.current = broadcastTargets || [];

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

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: "'Cascadia Code', 'Fira Code', 'JetBrains Mono', Consolas, monospace",
      theme: {
        background: "#1e1e2e",
        foreground: "#cdd6f4",
        cursor: "#f5e0dc",
        cursorAccent: "#1e1e2e",
        selectionBackground: "#585b7055",
        black: "#45475a",
        red: "#f38ba8",
        green: "#a6e3a1",
        yellow: "#f9e2af",
        blue: "#89b4fa",
        magenta: "#f5c2e7",
        cyan: "#94e2d5",
        white: "#bac2de",
        brightBlack: "#585b70",
        brightRed: "#f38ba8",
        brightGreen: "#a6e3a1",
        brightYellow: "#f9e2af",
        brightBlue: "#89b4fa",
        brightMagenta: "#f5c2e7",
        brightCyan: "#94e2d5",
        brightWhite: "#a6adc8",
      },
      allowProposedApi: true,
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();

    term.loadAddon(fitAddon);
    term.loadAddon(webLinksAddon);
    term.open(containerRef.current);

    setTimeout(() => {
      fitAddon.fit();
      sshResize(sessionIdRef.current, term.cols, term.rows).catch(() => {});
      // Inject the color-enabling init sequence once the PTY is sized. Catches
      // are silent — failures (rare) leave the terminal monochrome but usable.
      sshSend(sessionIdRef.current, SHELL_INIT_SEQ).catch(() => {});
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
      Promise.allSettled(
        destinations.map((sid) => sshSend(sid, data))
      ).then((results) => {
        results.forEach((r, i) => {
          if (r.status === "rejected") {
            console.error(`Broadcast send to ${destinations[i]} failed:`, r.reason);
          }
        });
      });
    });

    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
      sshResize(sessionIdRef.current, term.cols, term.rows).catch(() => {});
      // Broadcast group: mirror cols to other members so every shell outputs
      // in the same column width. Without this, an inactive tab's shell keeps
      // its stale cols (ResizeObserver only fires on visible containers) and
      // commands like `ls` render with a different layout per tab.
      for (const sid of broadcastRef.current) {
        if (sid !== sessionIdRef.current) {
          sshResize(sid, term.cols, term.rows).catch(() => {});
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

    onSshOutput(sessionIdRef.current, (data) => {
      if (!closed) term.write(data);
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
      termRef.current = null;
      fitRef.current = null;
      bridgeRef.current = null;
    };
  }, [sessionId]);

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
      try { fit.fit(); } catch { /* container not yet sized */ }
      sshResize(sessionIdRef.current, term.cols, term.rows).catch(() => {});
      // Sync the freshly-refit cols to broadcast members so their shells
      // output in the same width — covers the case where this tab was
      // inactive through a window resize and the others had stale dims.
      for (const sid of broadcastRef.current) {
        if (sid !== sessionIdRef.current) {
          sshResize(sid, term.cols, term.rows).catch(() => {});
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
  const targets = broadcastTargets ?? [];
  const broadcastKey = targets.join(",");
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    for (const sid of targets) {
      if (sid !== sessionIdRef.current) {
        sshResize(sid, term.cols, term.rows).catch(() => {});
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [broadcastKey]);

  return (
    <div style={{ position: "relative", width: "100%", height: "100%" }}>
      <div
        ref={containerRef}
        style={{
          width: "100%",
          height: "100%",
          background: "#1e1e2e",
          padding: 4,
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
  );
}
