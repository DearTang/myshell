import { useState, useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { Sidebar } from "./components/Sidebar";
import { TabBar } from "./components/TabBar";
import { TerminalPanel } from "./components/TerminalPanel";
import { SftpPanel } from "./components/SftpPanel";
import { ServerInfoPanel } from "./components/ServerInfoPanel";
import { ConnectionDialog } from "./components/ConnectionDialog";
import { MasterPasswordGate } from "./components/MasterPasswordGate";
import { SettingsPanel } from "./components/SettingsPanel";
import { AiPanel } from "./components/AiPanel";
import { QuickCommandsPanel } from "./components/QuickCommandsPanel";
import { AboutDialog } from "./components/AboutDialog";
import { FeedbackDialog } from "./components/FeedbackDialog";
import { StatsConsentDialog } from "./components/StatsConsentDialog";
import {
  checkReportNeeded,
  markVersionHandled,
  reportVersion,
  setStatsConsent,
} from "./lib/usageStats";
import { BroadcastDupDialog } from "./components/BroadcastDupDialog";
import { UpdateNotification } from "./components/UpdateNotification";
import { BrandLogo } from "./components/BrandLogo";
import type { Terminal } from "@xterm/xterm";
import {
  getConnections,
  deleteConnection,
  resetKnownHost,
  listFolders,
  sshConnect,
  sshSend,
  sshDisconnect,
  ftpConnect,
  ftpDisconnect,
  localConnect,
  localDisconnect,
  vaultStatus,
  lockVault,
  getAppVersion,
  openExternalUrl,
  onSshOutput,
  mcpExecResult,
  getCommandRules,
} from "./api";
import type { ConnectionConfig, ConnType, Tab } from "./api";
import { useUpdateCheck } from "./hooks/useUpdateCheck";
import { useRendererPref } from "./hooks/useRendererPref";
import { ConfirmDialog } from "./components/ConfirmDialog";
import type { CommandRules } from "./api";

/**
 * Map a VISIBLE-character index (counting only non-ANSI chars) to the
 * corresponding byte offset in the ORIGINAL string (which contains ANSI
 * escape sequences). Used to translate a position found in an ANSI-stripped
 * copy back to a position in the ANSI-preserving original, so we can
 * truncate the original at the right point WITHOUT losing color codes.
 *
 * Returns `str.length` if `visibleIndex` exceeds the number of visible chars.
 */
function stripFromAnsiPosition(str: string, visibleIndex: number): string {
  let visible = 0;
  let i = 0;
  const len = str.length;
  while (i < len) {
    if (visible >= visibleIndex) break;
    const code = str.charCodeAt(i);
    // CSI: ESC [ ... final-byte(0x40-0x7E)
    if (code === 0x1b && i + 1 < len && str.charCodeAt(i + 1) === 0x5b) {
      i += 2;
      while (i < len) {
        const c = str.charCodeAt(i);
        i++;
        if (c >= 0x40 && c <= 0x7e) break;
      }
      continue;
    }
    // OSC: ESC ] ... (BEL \x07 or ST \x1b\\)
    if (code === 0x1b && i + 1 < len && str.charCodeAt(i + 1) === 0x5d) {
      i += 2;
      while (i < len) {
        if (str.charCodeAt(i) === 0x07) { i++; break; }
        if (str.charCodeAt(i) === 0x1b && i + 1 < len && str.charCodeAt(i + 1) === 0x5c) { i += 2; break; }
        i++;
      }
      continue;
    }
    // Other ESC sequences (e.g. \x1b= ): ESC + one byte
    if (code === 0x1b) {
      i += 2;
      continue;
    }
    // Normal visible character
    visible++;
    i++;
  }
  return str.slice(0, i);
}

/**
 * Client-side mirror of the Rust `command_rules::command_needs_confirmation`.
 * Checks if a command matches any blacklist regex (and isn't exempted by
 * whitelist). Used in show_in_gui mode to show a React dialog instead of the
 * MCP process's raw OS MessageBoxW.
 *
 * Returns true if the command needs confirmation, false if it can run freely.
 * On any regex error, returns true (fail-safe).
 */
function checkCommandNeedsConfirmation(command: string, rules: CommandRules): boolean {
  const cmd = command.trim();
  if (!cmd) return true;

  // Dangerous patterns (hard floor) — always confirm.
  if (cmd.includes("$(") || cmd.includes("`")) return true;
  if (hasWriteRedirect(cmd)) return true;

  try {
    // Compile regexes (case-insensitive). Invalid patterns are dropped.
    const compile = (pats: string[]) =>
      pats.map((p) => { try { return new RegExp(p, "i"); } catch { return null; } }).filter(Boolean) as RegExp[];

    const blacklistRe = compile(rules.blacklist);
    const whitelistRe = compile(rules.whitelist);

    const blacklisted = blacklistRe.some((re) => re.test(cmd));
    if (blacklisted) {
      // Whitelist exemption?
      if (whitelistRe.some((re) => re.test(cmd))) return false;
      return true;
    }
    return rules.confirm_unknown;
  } catch {
    return true; // fail-safe
  }
}

/** Check for write-redirect to a real file (not /dev/null or fd-dup). */
function hasWriteRedirect(cmd: string): boolean {
  for (let i = 0; i < cmd.length; i++) {
    if (cmd[i] === ">") {
      let j = i + 1;
      if (cmd[j] === ">") j++; // append
      while (cmd[j] === " " || cmd[j] === "\t") j++;
      const rest = cmd.slice(j);
      if (!rest.startsWith("/dev/null") && !rest.startsWith("&")) {
        return true;
      }
      i = j;
    }
  }
  return false;
}

/**
 * Highlight dangerous keywords in a command string for the MCP confirmation
 * dialog. Matches each blacklist regex against the full command; for each
 * match, wraps the matched substring in a red bold `<span>`. Also highlights
 * dangerous pattern characters (`$(...)`, backticks, write-redirects `>`).
 *
 * The result is a React node array so it renders inline inside a `<div>`.
 */
function highlightDangerous(command: string, rules: CommandRules): ReactNode {
  // Collect all match ranges [start, end) from blacklist regexes + dangerous
  // literal patterns. We merge overlapping ranges, then slice the command
  // into highlighted/unhighlighted segments.
  type Range = [number, number];
  const ranges: Range[] = [];

  // 1. Dangerous literal patterns (hard floor).
  for (let i = 0; i < command.length; i++) {
    if (command.startsWith("$(", i)) ranges.push([i, i + 2]);
    if (command[i] === "`") ranges.push([i, i + 1]);
    // Write redirect: `>` or `>>` not followed by /dev/null or &
    if (command[i] === ">") {
      let j = i + 1;
      if (command[j] === ">") j++;
      while (command[j] === " " || command[j] === "\t") j++;
      const rest = command.slice(j);
      if (!rest.startsWith("/dev/null") && !rest.startsWith("&")) {
        ranges.push([i, j]); // highlight the > or >> part
      }
    }
  }

  // 2. Blacklist regex matches.
  for (const pat of rules.blacklist) {
    try {
      const re = new RegExp(pat, "gi");
      let m: RegExpExecArray | null;
      while ((m = re.exec(command)) !== null) {
        if (m[0].length > 0) {
          ranges.push([m.index, m.index + m[0].length]);
        }
        // Prevent zero-length match infinite loop.
        if (m.index === re.lastIndex) re.lastIndex++;
      }
    } catch {
      // Invalid regex — skip.
    }
  }

  if (ranges.length === 0) return command;

  // Merge overlapping/adjacent ranges.
  ranges.sort((a, b) => a[0] - b[0]);
  const merged: Range[] = [ranges[0]];
  for (let i = 1; i < ranges.length; i++) {
    const last = merged[merged.length - 1];
    if (ranges[i][0] <= last[1]) {
      last[1] = Math.max(last[1], ranges[i][1]);
    } else {
      merged.push(ranges[i]);
    }
  }

  // Slice into segments.
  const parts: ReactNode[] = [];
  let pos = 0;
  merged.forEach(([start, end], idx) => {
    if (pos < start) {
      parts.push(command.slice(pos, start));
    }
    parts.push(
      <span
        key={`hl-${idx}`}
        style={{
          color: "var(--error)",
          fontWeight: 700,
          background: "var(--error-muted, rgba(255,80,80,0.12))",
          borderRadius: 3,
          padding: "0 2px",
        }}
      >
        {command.slice(start, end)}
      </span>
    );
    pos = end;
  });
  if (pos < command.length) {
    parts.push(command.slice(pos));
  }
  return parts;
}

export default function App() {
  const [connections, setConnections] = useState<ConnectionConfig[]>([]);
  const [folders, setFolders] = useState<string[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  // Mirror of `tabs` for use inside async callbacks (setInterval/setTimeout)
  // that close over a stale `tabs` value. Updated on every render.
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  // ── MCP exec mutex lock ──
  // Per-connection command lock. An interactive PTY processes commands
  // sequentially — if a long-running command (pip install, sleep, etc.) is
  // still running, injecting the next command's bytes into the same PTY
  // corrupts both commands (the bytes queue behind the running process,
  // sentinels get crossed, and the session eventually hangs). This set
  // tracks which connection_ids currently have an MCP exec in flight; a
  // second exec for the same connection is rejected until the first one
  // completes (sentinel, idle-timeout, or hard-timeout) or the user is
  // told to use `nohup ... &` for long tasks.
  const mcpExecLocksRef = useRef<Set<string>>(new Set());
  // ── MCP exec_in_tab command confirmation ──
  // When show_in_gui=true and a command needs confirmation, we show a
  // ConfirmDialog. A Promise resolver stored in a ref connects the async
  // exec flow to the user's button click.
  const [mcpConfirm, setMcpConfirm] = useState<{
    command: string;
    connectionName: string;
    rules: CommandRules;
  } | null>(null);
  const mcpConfirmResolver = useRef<((ok: boolean) => void) | null>(null);

  /** Show a confirmation dialog for an MCP-triggered command. Returns a
   * Promise that resolves to true (confirm) or false (cancel). */
  function showMcpConfirm(command: string, connectionName: string, rules: CommandRules): Promise<boolean> {
    return new Promise((resolve) => {
      mcpConfirmResolver.current = resolve;
      setMcpConfirm({ command, connectionName, rules });
    });
  }

  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [showDialog, setShowDialog] = useState(false);
  const [editConfig, setEditConfig] = useState<ConnectionConfig | null>(null);
  const [initialConnType, setInitialConnType] = useState<ConnType | undefined>(undefined);
  const [initialFolderPath, setInitialFolderPath] = useState<string | undefined>(undefined);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  // Sidebar width persists in localStorage so a user who widens it to read long
  // connection names doesn't lose that on reload. Clamped to Sidebar's [200,560]
  // bounds (see Sidebar.onResizeStart) so a stale value can't render it broken.
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    const stored = Number(localStorage.getItem("myshell.sidebarWidth"));
    if (!Number.isFinite(stored)) return 240;
    return Math.min(560, Math.max(200, stored));
  });
  const [showSettings, setShowSettings] = useState(false);
  const [showQuickCommands, setShowQuickCommands] = useState(false);
  const [showFeedback, setShowFeedback] = useState(false);
  // Anonymous usage stats consent dialog. Shown on first launch of each new
  // version (if the user hasn't previously agreed). See lib/usageStats.ts.
  const [statsPrompt, setStatsPrompt] = useState<{ version: string } | null>(null);
  // AI assistant panel (global docked-right chat bar). Width persists in
  // localStorage so it survives reloads.
  const [showAiPanel, setShowAiPanel] = useState(false);
  const [aiPanelWidth, setAiPanelWidth] = useState(() => {
    // Clamp stored value to the panel's own [300, 720] bounds (see
    // AiPanel.onResizeStart) so a stale/out-of-range value can't render the
    // panel unusably narrow or absurdly wide.
    const stored = Number(localStorage.getItem("myshell.aiPanelWidth"));
    if (!Number.isFinite(stored)) return 380;
    return Math.min(720, Math.max(300, stored));
  });
  // Phase 3: live xterm registry, keyed by sessionId. The AI panel reads
  // terminal output/selection and pastes commands through it. Populated by
  // TerminalPanel.onTerminalReady, drained on close/reconnect.
  const terminalRegistryRef = useRef<Map<string, Terminal>>(new Map());
  const handleTerminalReady = (sid: string, term: Terminal) => {
    terminalRegistryRef.current.set(sid, term);
  };
  const handleTerminalGone = (sid: string) => {
    terminalRegistryRef.current.delete(sid);
  };
  const getTerminal = (sid?: string): Terminal | undefined =>
    sid ? terminalRegistryRef.current.get(sid) : undefined;
  /** Clear the reconnectSnapshot from a tab after TerminalPanel has consumed
   *  it, so it doesn't get written again on a later remount. */
  const clearReconnectSnapshot = (tabId: string) => {
    setTabs((prev) =>
      prev.map((t) =>
        t.id === tabId ? { ...t, reconnectSnapshot: undefined } : t
      )
    );
  };
  // Preset scope when opening the quick-commands panel: null = global,
  // a connection id = that server's per-server scope. Set by the entry point
  // (Sidebar global button → null, CommandBar "管理" → current connection).
  const [qcInitialConnectionId, setQcInitialConnectionId] = useState<string | null>(null);

  // Vault gate: null = checking, "setup" = no vault yet, "unlock" = vault
  // exists but locked, "ready" = master key loaded. Render the gate until
  // ready, blocking all other UI so connection commands can't be invoked
  // without a derived key (which would just error with "Vault 未解锁").
  const [vault, setVault] = useState<"checking" | "setup" | "unlock" | "ready">("checking");

  // ── Version + update check ──
  // appVersion drives the sidebar footer label and the whats-new trigger.
  // `knownVersion` in localStorage records the version we last showed the
  // changelog for; on first launch after an upgrade it differs from the
  // running version and we surface the changelog once.
  const [appVersion, setAppVersion] = useState<string>("");
  // about: a single dialog component used two ways — "whatsnew" (auto on
  // upgrade) and "about" (manual, from the sidebar footer / update toast).
  const [about, setAbout] = useState<{ open: boolean; mode: "whatsnew" | "about" }>({
    open: false,
    mode: "about",
  });
  const { info: updateInfo, loading: updateChecking, checkNow } = useUpdateCheck(vault === "ready");
  // Terminal renderer backend (dom/canvas/webgl) — global pref, threaded into
  // every TerminalPanel so each tab picks the same renderer at mount. Changing
  // it only affects NEWLY opened tabs (a renderer is chosen once per terminal).
  const { rendererBackend } = useRendererPref();

  useEffect(() => {
    if (vault !== "ready") return;
    let cancelled = false;
    getAppVersion()
      .then((v) => {
        if (cancelled) return;
        setAppVersion(v);
        // First-run-after-upgrade changelog. Read what we last acknowledged.
        let known: string | null = null;
        try {
          known = localStorage.getItem("myshell.knownVersion");
        } catch {
          known = null;
        }
        if (known === null) {
          // First ever launch (or cleared storage): record silently, don't
          // pester a brand-new user with a changelog popup.
          try {
            localStorage.setItem("myshell.knownVersion", v);
          } catch {
            // best-effort
          }
        } else if (known !== v) {
          // Version changed since last run → show what's new.
          setAbout({ open: true, mode: "whatsnew" });
        }

        // ── Anonymous per-version usage stats ──
        // Check whether the current version needs to be reported. If the user
        // has previously agreed, report silently. Otherwise, prompt.
        const { shouldReport, hasConsent } = checkReportNeeded(v);
        if (shouldReport) {
          if (hasConsent) {
            void reportVersion(v, navigator.platform);
          } else {
            setStatsPrompt({ version: v });
          }
        }
      })
      .catch(() => {
        // getVersion should not fail; if it does, just skip the feature.
      });
    return () => {
      cancelled = true;
    };
  }, [vault]);

  // When the background check finds a newer release, show the bottom-left
  // UpdateNotification card (dismissable, per-version). This replaces the old
  // auto-open About modal — less intrusive. The sidebar footer also shows a
  // green dot. Manual re-check / changelog still live in the About dialog.

  const closeAbout = () => {
    // Closing the whats-new dialog acknowledges the version: stamp it so the
    // changelog doesn't reappear on next launch.
    if (about.mode === "whatsnew" && appVersion) {
      try {
        localStorage.setItem("myshell.knownVersion", appVersion);
      } catch {
        // best-effort
      }
    }
    setAbout({ open: false, mode: "about" });
  };


  useEffect(() => {
    if (vault !== "ready") {
      vaultStatus()
        .then((s) => {
          if (!s.initialized) setVault("setup");
          else if (s.unlocked) setVault("ready");
          else setVault("unlock");
        })
        .catch(() => setVault("setup"));
    }
  }, [vault]);

  useEffect(() => {
    if (vault === "ready") reload();
  }, [vault]);

  // ── Auto-lock: idle timeout locks the vault ──────────────────────────────
  // Reads `myshell-auto-lock-minutes` from localStorage (default "30", "0" =
  // disabled). When vault is "ready" and the setting is > 0, starts a timer
  // that fires after N minutes of inactivity. Any user interaction (mouse,
  // keyboard, scroll, touch) resets the timer. On fire: lockVault() + flip
  // vault state to "checking" so the status effect re-queries → shows unlock.
  //
  // IMPORTANT: a bare setTimeout is NOT reliable for this. When the window is
  // minimized / loses focus, the OS webview (WebView2/WebKit) throttles background
  // timers — the callback silently fails to fire on time, so the vault never
  // locks. We therefore ALSO track `lastActivityAt` (a wall-clock timestamp)
  // and re-check it when the window becomes visible/focused again. If the idle
  // gap already exceeds the threshold at that moment, we lock immediately. This
  // catches the throttle gap regardless of how the window regains foreground.
  useEffect(() => {
    if (vault !== "ready") return;

    const getMinutes = () => {
      const raw = localStorage.getItem("myshell-auto-lock-minutes") ?? "30";
      const n = parseInt(raw, 10);
      return isNaN(n) ? 30 : n;
    };

    let minutes = getMinutes();
    if (minutes <= 0) return; // disabled

    const timeoutMs = () => minutes * 60 * 1000;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let lastActivityAt = Date.now();
    let lastReset = 0;
    let locked = false;

    const doLock = () => {
      if (locked) return;
      locked = true;
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
      lockVault().catch(() => { /* best effort */ });
      setVault("checking");
    };

    const armTimer = () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(doLock, timeoutMs());
    };

    // If already past the idle threshold (e.g. timer was throttled while the
    // window was in the background), lock now instead of re-arming.
    const checkElapsed = () => {
      if (locked) return;
      if (Date.now() - lastActivityAt >= timeoutMs()) {
        doLock();
      } else {
        armTimer();
      }
    };

    const onActivity = () => {
      const now = Date.now();
      lastActivityAt = now;
      // Throttle: at most one reset per 5 seconds (avoids mousemove floods).
      if (now - lastReset < 5000) return;
      lastReset = now;
      if (timer) armTimer();
    };

    // Fired when the window/tab becomes visible again. While it was hidden the
    // setTimeout above may have been throttled past its deadline; this catches
    // up by comparing wall-clock idle time against the threshold.
    const onVisibility = () => {
      if (document.visibilityState === "visible") checkElapsed();
    };

    armTimer();
    const events: Array<keyof WindowEventMap> = ["mousemove", "keydown", "click", "wheel", "touchstart"];
    events.forEach((ev) => window.addEventListener(ev, onActivity, { capture: true, passive: true }));
    window.addEventListener("visibilitychange", onVisibility);
    // focus/pageshow also cover the case where visibilitychange doesn't fire
    // (e.g. switching between two windows of the same app on some platforms).
    window.addEventListener("focus", checkElapsed);
    window.addEventListener("pageshow", checkElapsed);

    // React to setting changes (SettingsPanel dispatches this event).
    const onSettingChanged = () => {
      minutes = getMinutes();
      if (minutes <= 0) {
        if (timer) {
          clearTimeout(timer);
          timer = null;
        }
      } else if (!locked) {
        checkElapsed();
      }
    };
    window.addEventListener("myshell-auto-lock-changed", onSettingChanged);

    return () => {
      if (timer) clearTimeout(timer);
      events.forEach((ev) => window.removeEventListener(ev, onActivity, { capture: true }));
      window.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("focus", checkElapsed);
      window.removeEventListener("pageshow", checkElapsed);
      window.removeEventListener("myshell-auto-lock-changed", onSettingChanged);
    };
  }, [vault]);

  // ── MCP ↔ GUI bridge: listen for commands from the MCP server ──────────
  // The MCP server (separate process) can tell the GUI to:
  //   - "open_connection": open/focus a terminal tab (fire-and-forget)
  //   - "exec_in_tab": run a command in a visible terminal tab and capture
  //     the output back (blocking — the IPC thread waits for mcpExecResult)
  //
  // For exec_in_tab we use a sentinel mechanism: send the command followed by
  // `echo __MCP_DONE_<uuid>__:$?`, then watch the ssh_output stream until the
  // sentinel appears. Everything between the command echo and the sentinel is
  // the command's stdout; the number after the colon is the exit code.
  useEffect(() => {
    const unlisten = listen<{
      action: string;
      connection_id: string;
      tab_type?: string;
      focus_existing?: boolean;
      request_id?: string;
      command?: string;
      timeout?: number;
    }>("mcp-gui-command", (event) => {
      const { action, connection_id } = event.payload;
      if (!connection_id) return;

      const config = connections.find((c) => c.id === connection_id);
      if (!config) {
        console.warn(`[mcp-gui] connection not found: ${connection_id}`);
        // If this is an exec_in_tab, report the error back.
        if (action === "exec_in_tab" && event.payload.request_id) {
          mcpExecResult(event.payload.request_id, { ok: false, error: "未找到连接" });
        }
        return;
      }

      // ── exec_in_tab: run command in a visible tab + capture output ──
      if (action === "exec_in_tab" && event.payload.request_id && event.payload.command) {
        const requestId = event.payload.request_id;
        const command = event.payload.command;
        const timeoutMs = (event.payload.timeout || 30) * 1000;

        // ── Command mutex lock ──
        // The interactive PTY executes commands sequentially. If a previous
        // command is still running (long task, pip install, sleep, etc.),
        // injecting this command's bytes into the same PTY corrupts both.
        // Reject the second command and tell the AI to use nohup for long
        // tasks. This is the root-cause fix for the "session hangs after a
        // long command" issue — without it, overlapping commands cross their
        // sentinels and the session becomes unusable.
        if (mcpExecLocksRef.current.has(connection_id)) {
          mcpExecResult(requestId, {
            ok: false,
            error:
              "上一条命令仍在该服务器的终端中执行。交互式终端一次只能跑一条命令。" +
              "如需运行耗时命令，请用 nohup 后台执行（如 `nohup pip install -e . > /tmp/log 2>&1 &`），" +
              "然后轮询日志文件查看结果。",
          });
          return;
        }
        mcpExecLocksRef.current.add(connection_id);

        // Release the per-connection exec lock. Called on EVERY completion
        // path (success, idle-fallback, hard-timeout, connection error,
        // user-cancel) so the next exec can proceed. Wrapping mcpExecResult
        // here guarantees we never forget to release on a new exit path.
        const finishExec = (result: { ok: boolean; stdout?: string; exit_code?: number; error?: string }) => {
          mcpExecLocksRef.current.delete(connection_id);
          mcpExecResult(requestId, result);
        };

        // Find an existing terminal tab for this connection (regardless of
        // status). We then branch on the tab's status:
        //   connected    → run the command immediately (fast path, <1s)
        //   disconnected → reconnect IN PLACE (reuse the tab, preserve history)
        //                  then run the command
        //   absent       → open a fresh tab + connect, then run the command
        const existingTab = tabsRef.current.find(
          (t) => t.connectionId === connection_id && t.type === "terminal"
        );

        const runExec = async (sessionId: string) => {
          // ── Command confirmation (GUI-side, nicer than OS MessageBoxW) ──
          // When running in show_in_gui mode, check the command rules HERE
          // (in the GUI) rather than in the MCP process. This lets us show a
          // beautiful React dialog instead of a raw Windows MessageBoxW.
          try {
            const rules = await getCommandRules();
            // Simple client-side check: if the command matches a blacklist
            // regex and no whitelist regex matches, show confirmation.
            const needsConfirm = checkCommandNeedsConfirmation(command, rules);
            if (needsConfirm) {
              const connectionName = config.name || connection_id;
              const confirmed = await showMcpConfirm(command, connectionName, rules);
              if (!confirmed) {
                finishExec({
                  ok: false,
                  error: "❌ 用户取消了高危操作：ssh_exec",
                });
                return;
              }
            }
          } catch (e) {
            console.warn("[mcp-gui] failed to check command rules:", e);
            // On error, err on the side of caution — but don't block the
            // command (the MCP side already checked rules in headless fallback).
          }

          // Generate a unique sentinel. The shell will echo it with the exit
          // code appended, marking where the command's output ends.
          const sentinel = `__MCP_DONE_${Math.random().toString(36).slice(2, 14)}__`;

          // IMPORTANT: subscribe BEFORE sending the command. If we subscribed
          // after, fast commands' output would already have been emitted (and
          // rendered by TerminalPanel) before our handler attaches, leaving
          // outputBuf with only the sentinel line → stdout came back empty.
          let outputBuf = "";
          let done = false;
          let timedOut = false;
          // Track the last time we received ANY data. Used by the idle-timeout
          // fallback below — if output stops arriving for a while after the
          // sentinel *command echo* has been seen, we conclude the command has
          // finished even if the sentinel *result line* never reached us (it
          // can get stuck behind PTY/SSH buffering on large outputs).
          let lastDataAt = Date.now();
          let sawSentinelEcho = false;

          // Helper: finalize output extraction. Shared by the sentinel-match
          // path and the idle-timeout fallback so both produce identical
          // stdout cleaning. exitCode is null in the fallback case (unknown).
          const finalizeOutput = (exitCode: number | null) => {
            done = true;
            unlistenOutput();
            let stdout = outputBuf;
            // If the sentinel result line is present, truncate at it.
            const sentinelRe = new RegExp(
              sentinel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + ":(\\d+)"
            );
            const sentinelMatch = stdout.match(sentinelRe);
            if (sentinelMatch && sentinelMatch.index !== undefined) {
              stdout = stdout.slice(0, sentinelMatch.index);
            }

            // Normalize CRLF → LF early so all subsequent slicing is uniform.
            stdout = stdout.replace(/\r\n/g, "\n").replace(/\r/g, "\n");

            // Drop a trailing prompt line if one remains after stripping the
            // sentinel echo (interactive shells print a fresh `[user@host ~]$ `
            // before we send the sentinel). A prompt line ends with `$` or `#`
            // and contains no real output. Trim trailing empty lines first.
            stdout = stdout.replace(/\n+$/, "");
            const lastNl = stdout.lastIndexOf("\n");
            const lastLine = stdout.slice(lastNl + 1);
            if (lastLine && /[#$]\s*$/.test(lastLine)) {
              stdout = stdout.slice(0, lastNl);
            }

            // Strip the command echo at the beginning. The PTY echoes the
            // command text we sent (`command + "\n"`), possibly prefixed by a
            // PS1 prompt like `[user@host ~]$ `. Locate the command text itself
            // in the buffer — robust to a leading prompt on the same line,
            // which the old `command.startsWith(lines[0].trim())` check could
            // not handle (echoed line is `[host]$ whoami`, not `whoami`).
            let start = stdout.indexOf(command);
            if (start >= 0) {
              // Skip past the echoed command text to the end of its line.
              start += command.length;
              const lineEnd = stdout.indexOf("\n", start);
              stdout = lineEnd >= 0 ? stdout.slice(lineEnd + 1) : "";
            } else {
              // Fallback: no command echo found (some PTYs/shells don't echo).
              const lines = stdout.split("\n");
              if (lines.length > 0 && command.startsWith(lines[0].trim())) {
                lines.shift();
              }
              stdout = lines.join("\n");
            }
            // Strip the sentinel helper-command echoes. After the user's
            // command, we sent ONE helper line: `echo __MCP_DONE_xxx__:$?`.
            // The PTY echoes it back (with a PS1 prompt prefix, ANSI escapes,
            // etc.). Find the FIRST occurrence of `echo __MCP_DONE_` after the
            // command output and truncate everything from that line onward —
            // the real stdout is strictly between the command echo and the
            // sentinel command echo.
            //
            // CRITICAL: we use an ANSI-stripped COPY only to LOCATE the
            // helper line, then truncate the ORIGINAL stdout (which still has
            // color codes) via stripFromAnsiPosition. The old code replaced
            // stdout with the ANSI-stripped copy, which discarded all colors —
            // the root cause of "AI output has no color" complaints.
            //
            // The OSC regex is upgraded from [0-9]; (single digit) to \d+;
            // to handle multi-digit OSC params (e.g. OSC 133 shell-integration
            // sequences if they appear).
            const ansiCleaned = stdout
              .replace(/\x1b\]\d+;.*?(?:\x07|\x1b\\)/g, "")
              .replace(/\x1b\[[0-9;]*[a-zA-Z]/g, "");
            const helperIdx = ansiCleaned.search(/echo __MCP_DONE_/);
            if (helperIdx >= 0) {
              // Truncate at the start of the line containing the sentinel
              // command echo, so we don't leave a dangling prompt prefix.
              const lineStart = ansiCleaned.lastIndexOf("\n", helperIdx);
              const cutVisible = lineStart >= 0 ? lineStart : helperIdx;
              // Map visible-char position back to the ANSI-preserving original.
              stdout = stripFromAnsiPosition(stdout, cutVisible);
            }
            // NOTE: we deliberately do NOT strip ANSI from stdout here —
            // preserving color codes so the AI sees the same colors the user
            // sees in the terminal (red errors, green success, blue dirs).
            stdout = stdout.replace(/^\n+/, "").replace(/\n+$/, "");

            finishExec({
              ok: true,
              stdout,
              exit_code: exitCode ?? 0,
            });
          };

          const unlistenOutput = await onSshOutput(sessionId, (data) => {
            if (done) return;
            lastDataAt = Date.now();
            // Decode bytes to string (terminal output is UTF-8 / ASCII).
            outputBuf += new TextDecoder("utf-8", { fatal: false }).decode(data);

            // Cap the buffer at 4MB (aligned with the headless path's MAX
            // limit). Without this, a chatty command (cat huge.log) can grow
            // outputBuf unbounded and OOM the renderer. We keep the TAIL
            // (most recent output) because the sentinel always appears at the
            // end — truncating the head only loses early stdout, which is
            // acceptable for pathological output sizes.
            const MAX_OUTPUT = 4 * 1024 * 1024;
            if (outputBuf.length > MAX_OUTPUT) {
              outputBuf = outputBuf.slice(-MAX_OUTPUT);
            }

            // Detect the sentinel *command echo* (`echo __MCP_DONE_xxx__:$?`).
            // Once seen, we know the command has been submitted and the result
            // line should follow shortly. This arms the idle-timeout fallback.
            if (!sawSentinelEcho && outputBuf.includes(`echo ${sentinel}`)) {
              sawSentinelEcho = true;
            }

            // The PTY echoes everything we send. The output stream looks like:
            //   <command text echoed back>          ← strip (line 1, may wrap)
            //   <command's actual stdout/stderr>     ← KEEP
            //   echo __MCP_DONE_xxx__:$?  echoed     ← strip
            //   __MCP_DONE_xxx__:0                   ← sentinel + exit code
            //
            // We detect the sentinel line via regex, then extract the output
            // between the command echo and the sentinel echo.

            // Check for the sentinel result line: sentinel:N
            // Scan the TAIL (last 16KB) — the sentinel is always near the end,
            // but on large/chatty outputs the tail batch can exceed the old
            // 2KB window (prompt + ANSI codes + buffered chunks). 16KB covers
            // realistic worst cases without measurable cost vs 2KB.
            const sentinelRe = new RegExp(
              sentinel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + ":(\\d+)"
            );
            const tailSearch = outputBuf.slice(-16 * 1024);
            const tailMatch = tailSearch.match(sentinelRe);
            if (!tailMatch) return;
            // Map the tail match back to a full-buffer index.
            const matchIndex = outputBuf.length - tailSearch.length + (tailMatch.index ?? 0);
            const exitCode = parseInt(tailMatch[1], 10);
            // Truncate the buffer at the sentinel so finalizeOutput's
            // sentinel-truncation logic fires cleanly.
            outputBuf = outputBuf.slice(0, matchIndex);
            finalizeOutput(exitCode);
          });

          // Now that the listener is attached, send the command + sentinel.
          // Send user's command first (visible in terminal), then one
          // sentinel line to capture the exit code. The sentinel line
          // (`echo __MCP_DONE_...`) IS visible in the terminal, but
          // TerminalPanel's render-layer filter strips any line containing
          // `__MCP_DONE_` before writing to xterm, so the user never sees
          // it. We do NOT use `stty -echo` tricks — echo timing is
          // unreliable across shells/PTYs.
          await sshSend(sessionId, command + "\n");
          await new Promise((r) => setTimeout(r, 80));
          await sshSend(sessionId, `echo ${sentinel}:$?\n`);

          // ── Three-tier completion strategy ──────────────────────────────
          // Tier 1 (happy path): sentinel result line `__MCP_DONE_xxx__:N`
          //   arrives → finalizeOutput with the real exit code. Handled in the
          //   onSshOutput callback above.
          //
          // Tier 2 (idle fallback): the sentinel COMMAND echo
          //   (`echo __MCP_DONE_xxx__:$?`) has been seen, the command itself
          //   must have finished — but the sentinel RESULT line is stuck
          //   behind PTY/SSH buffering on large outputs. If no new data
          //   arrives for IDLE_TIMEOUT_MS, conclude the command is done and
          //   return what we have (exit code unknown → 0). This is what fixes
          //   the `docker logs --tail 30` 30s-timeout bug: the command had
          //   long finished, but the sentinel result never arrived in time.
          //
          // Tier 3 (hard timeout): the user-supplied `timeout` elapses with
          //   continued chatty output (tail -f, interactive command, hang).
          //   Return an error + partial stdout.
          const IDLE_TIMEOUT_MS = 5000;
          const idleCheck = setInterval(() => {
            if (done || timedOut) {
              clearInterval(idleCheck);
              return;
            }
            // Only fire the idle fallback AFTER we've seen the sentinel
            // command echo — otherwise we'd prematurely cut off a command
            // that's still producing output (slow `find /`, etc.).
            if (sawSentinelEcho && Date.now() - lastDataAt >= IDLE_TIMEOUT_MS) {
              clearInterval(idleCheck);
              finalizeOutput(null);
            }
          }, 1000);

          // Hard timeout: if the command genuinely hangs (e.g. waiting for
          // stdin, or a long task the AI didn't background), return an error
          // + partial stdout. CRITICAL: send Ctrl+C (\x03) to the PTY
          // afterward so the running process is interrupted and the prompt
          // returns — otherwise the next exec on this session inherits a
          // still-running command and the session hangs. The bytes are sent
          // BEFORE we report the timeout so the interrupt takes effect even
          // as we return.
          setTimeout(() => {
            if (!done && !timedOut) {
              timedOut = true;
              clearInterval(idleCheck);
              unlistenOutput();
              // Interrupt any still-running process in the PTY so the session
              // is left in a clean prompt state. Best-effort: ignore errors
              // (the session may already be gone).
              sshSend(sessionId, "\x03").catch(() => {});
              finishExec({
                ok: false,
                error: `命令超时（${event.payload.timeout || 30}秒未完成）。已自动发送 Ctrl+C 中断残留进程；如需长时间运行的命令，请用 nohup 后台执行并轮询日志。`,
                stdout: outputBuf,
              });
            }
          }, timeoutMs);

          // If done already (fast command), clean up the listener.
          // We check periodically; a cleaner approach would use a Promise.
          const cleanupCheck = setInterval(() => {
            if (done || timedOut) {
              clearInterval(cleanupCheck);
              unlistenOutput();
            }
          }, 500);
        };

        if (existingTab && existingTab.status === "connected" && existingTab.sessionId) {
          // ── Fast path: tab already connected — run immediately. ──
          setActiveTabId(existingTab.id);
          runExec(existingTab.sessionId).catch((e) => {
            finishExec({ ok: false, error: `执行失败: ${e}` });
          });
        } else if (existingTab && (existingTab.status === "disconnected" || existingTab.status === "error")) {
          // ── Reconnect path: session timed out / died. Reconnect IN PLACE
          //    (reuse the same tab, preserve terminal history via snapshot)
          //    then run the command. This avoids opening a duplicate tab on
          //    every ssh_exec after a server-side TMOUT disconnect. ──
          setActiveTabId(existingTab.id);
          reconnectOne(existingTab.id).then(async (ok) => {
            if (!ok) {
              finishExec({
                ok: false,
                error: `会话已断开且重连失败：${existingTab.errorMessage || "请检查网络和保险库是否解锁"}`,
              });
              return;
            }
            // reconnectOne updated the tab's sessionId; read the fresh value.
            const reconnected = tabsRef.current.find(
              (t) => t.id === existingTab.id && t.status === "connected" && t.sessionId
            );
            if (reconnected?.sessionId) {
              runExec(reconnected.sessionId).catch((e) => {
                finishExec({ ok: false, error: `执行失败: ${e}` });
              });
            } else {
              finishExec({ ok: false, error: "重连后未找到会话" });
            }
          });
        } else {
          // ── Fresh tab path: no tab exists yet. Open one + connect. ──
          handleConnect(config);
          // Watch tabs state for the new connected tab.
          let attempts = 0;
          const maxAttempts = Math.floor(timeoutMs / 500);
          const checkInterval = setInterval(() => {
            attempts++;
            // Check for error state FIRST — if handleConnect failed (e.g.
            // vault locked, auth refused), the tab flips to "error" almost
            // immediately. Without this check we'd poll uselessly until the
            // full timeout, wasting ~30s before reporting.
            const errTab = tabsRef.current.find(
              (t) => t.connectionId === connection_id && t.type === "terminal" && t.status === "error"
            );
            if (errTab) {
              clearInterval(checkInterval);
              finishExec({
                ok: false,
                error: `连接失败：${errTab.errorMessage || "请检查保险库是否已解锁"}`,
              });
              return;
            }
            const tab = tabsRef.current.find(
              (t) => t.connectionId === connection_id && t.type === "terminal" && t.status === "connected" && t.sessionId
            );
            if (tab && tab.sessionId) {
              clearInterval(checkInterval);
              setActiveTabId(tab.id);
              runExec(tab.sessionId).catch((e) => {
                finishExec({ ok: false, error: `执行失败: ${e}` });
              });
            } else if (attempts >= maxAttempts) {
              clearInterval(checkInterval);
              finishExec({ ok: false, error: "连接超时，无法建立终端会话" });
            }
          }, 500);
        }
        return;
      }

      // ── open_connection: open/focus a tab (fire-and-forget) ──
      if (action !== "open_connection") return;

      const tab_type = event.payload.tab_type || "auto";
      const focus_existing = event.payload.focus_existing ?? true;

      const wantType: "terminal" | "sftp" | "auto" =
        tab_type === "sftp" ? "sftp" : tab_type === "terminal" ? "terminal" : "auto";

      if (focus_existing) {
        const existing = tabs.find((t) => {
          if (t.connectionId !== connection_id) return false;
          if (wantType === "sftp") return t.type === "sftp";
          if (wantType === "terminal") return t.type === "terminal";
          return true;
        });
        if (existing) {
          setActiveTabId(existing.id);
          return;
        }
      }

      if (wantType === "sftp" && (config.conn_type === "ssh" || config.conn_type === "sftp" || !config.conn_type)) {
        handleConnect({ ...config, conn_type: "sftp" });
      } else {
        handleConnect(config);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [connections, tabs]);

  // Broadcast group: a Set of tab IDs whose terminal sessions should mirror
  // each other's keystrokes. Toggled per-tab from the TabBar 📡 button.
  // Only SSH terminal tabs are eligible — SFTP/FTP tabs can't accept shell
  // input, so they're filtered out at target-collection time.
  const [broadcastIds, setBroadcastIds] = useState<Set<string>>(new Set());

  // Session-scoped "don't remind me again" for the duplicate-connection
  // broadcast prompt. Lives in a ref (NOT localStorage) so it resets on every
  // app restart — the user's choice only sticks for the current session. Once
  // true, adding any further same-connection tab to the broadcast group goes
  // through without prompting.
  const broadcastDupDismissedRef = useRef(false);
  // Remembers the user's last "不再提醒" checkbox choice across prompts within
  // the session. Starts true (default-checked) but if the user ever unchecks
  // it, every later prompt reopens already unchecked — matching their stated
  // preference. Session-scoped (ref, not storage) so it resets on restart.
  const broadcastDupDontRemindPrefRef = useRef(true);
  // Pending duplicate prompt: the tabId the user is trying to add, plus how
  // many same-connection tabs are already in the group (for the dialog text),
  // and the initial checkbox state (the user's last choice).
  const [broadcastDupPrompt, setBroadcastDupPrompt] = useState<{
    tabId: string;
    connectionName: string;
    existingCount: number;
    initialDontRemind: boolean;
  } | null>(null);

  /** Actually add/remove a tab in the broadcast set. Split out from
   * toggleBroadcast so the duplicate-confirm path can call it after the user
   * has accepted. */
  function setBroadcastMembership(tabId: string, inGroup: boolean) {
    setBroadcastIds((prev) => {
      const next = new Set(prev);
      if (inGroup) next.add(tabId);
      else next.delete(tabId);
      return next;
    });
  }

  function toggleBroadcast(tabId: string) {
    const tab = tabs.find((t) => t.id === tabId);
    if (!tab) return;
    // Toggling OFF or a tab with no connection → no duplicate concern, just
    // flip membership.
    if (broadcastIds.has(tabId) || !tab.connectionId) {
      setBroadcastMembership(tabId, !broadcastIds.has(tabId));
      return;
    }
    // Count same-connection tabs already in the group. If none, no duplicate.
    const dupCount = tabs.filter(
      (t) => broadcastIds.has(t.id) && t.connectionId === tab.connectionId
    ).length;
    if (dupCount === 0) {
      setBroadcastMembership(tabId, true);
      return;
    }
    // Duplicate detected. If the user already silenced this prompt for the
    // session, proceed without asking.
    if (broadcastDupDismissedRef.current) {
      setBroadcastMembership(tabId, true);
      return;
    }
    // Otherwise surface the prompt. The dialog's confirm/cancel callbacks
    // decide whether to actually add.
    const connName =
      connections.find((c) => c.id === tab.connectionId)?.name || tab.name;
    setBroadcastDupPrompt({
      tabId,
      connectionName: connName,
      existingCount: dupCount,
      initialDontRemind: broadcastDupDontRemindPrefRef.current,
    });
  }

  /** Compute the broadcast target sessionIds for a given tab:
   *  - If the tab is in the broadcast group: returns all SSH-terminal
   *    sessions that are also in the group (could include this tab itself).
   *  - Otherwise: empty array — TerminalPanel falls back to single-target.
   * The list is recomputed on every render so toggling membership is
   * reflected immediately; TerminalPanel reads it via a live ref so the
   * onData handler doesn't need rebinding.
   *
   * Note: we no longer de-dup by connectionId here. Two tabs of the same
   * connection are allowed to both receive broadcast — the duplicate case is
   * handled as a one-time prompt at toggle time (see toggleBroadcast), so by
   * the time a tab is in the group the user has agreed it should receive
   * keystrokes. This supports the jump-box pattern (one connection, several
   * tabs each SSH'd onward to different hosts). */
  function getBroadcastTargets(tab: Tab): string[] {
    if (!tab.sessionId || !broadcastIds.has(tab.id)) return [];
    const targets: string[] = [];
    for (const t of tabs) {
      if (
        !broadcastIds.has(t.id) ||
        !t.sessionId ||
        t.type !== "terminal" ||
        t.connType !== "ssh" ||
        t.status !== "connected"
      ) {
        continue;
      }
      targets.push(t.sessionId);
    }
    return targets;
  }

  async function reload() {
    try {
      const [conns, dirs] = await Promise.all([getConnections(), listFolders()]);
      setConnections(conns);
      setFolders(dirs);
    } catch (e) {
      console.error("Failed to load:", e);
    }
  }

  async function handleConnect(config: ConnectionConfig) {
    const connType = config.conn_type ?? "ssh";
    const display = connType === "local" ? config.name : `${config.username}@${config.host}`;

    // The tab id is a STABLE identifier created once here. It never changes
    // across reconnects — only `sessionId` changes when the underlying SSH
    // connection is rebuilt. This keeps the React key (and thus the xterm
    // instance) stable across reconnects, preserving terminal scrollback
    // history. The temp- prefix is kept for clarity during the connecting
    // phase; the id itself stays the same after connect succeeds.
    const newTabId = `tab-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const newTab: Tab = {
      id: newTabId,
      name: display,
      type: connType === "ftp" ? "ftp" : connType === "sftp" ? "sftp" : "terminal",
      connType,
      connectionId: config.id,
      status: "connecting",
      config: config,
    };

    setTabs((prev) => [...prev, newTab]);
    setActiveTabId(newTabId);

    try {
      if (connType === "ftp") {
        const ftpId = await ftpConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === newTabId
              ? {
                  ...t,
                  sessionId: ftpId,
                  ftpSessionId: ftpId,
                  status: "connected",
                  errorMessage: undefined,
                }
              : t
          )
        );
      } else if (connType === "local") {
        const sessionId = await localConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === newTabId
              ? {
                  ...t,
                  sessionId,
                  status: "connected",
                  errorMessage: undefined,
                }
              : t
          )
        );
      } else {
        const sessionId = await sshConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === newTabId
              ? {
                  ...t,
                  sessionId,
                  status: "connected",
                  errorMessage: undefined,
                }
              : t
          )
        );
      }
    } catch (e) {
      const errorMessage = String(e);
      // Host-key mismatch (server reinstall / regenerated keys) surfaces as a
      // specific message from ssh.rs — flag it so the error screen can offer
      // an inline "reset key & reconnect" instead of the old opaque error.
      const hostKeyMismatch = errorMessage.includes("主机密钥已变更");
      // Update tab to show error state
      setTabs((prev) =>
        prev.map((t) =>
          t.id === newTabId
            ? {
                ...t,
                status: "error",
                errorMessage: errorMessage,
                hostKeyMismatch,
              }
            : t
        )
      );
    }
  }

  /** Reconnect a single tab by its id. Returns true on success. Extracted from
   * handleReconnect so the broadcast-cascade path can reuse it without
   * re-triggering the cascade logic recursively. */
  async function reconnectOne(tabId: string): Promise<boolean> {
    const tab = tabs.find((t) => t.id === tabId);
    if (!tab || !tab.config) return false;

    // Capture the old terminal's text buffer before tearing it down, so the
    // new xterm instance can restore scrollback history after reconnect. We
    // read it via the terminal registry (onTerminalReady registered the
    // xterm instance by its old sessionId). Only terminal tabs have a
    // captured xterm; SFTP/FTP tabs skip this.
    let snapshot: string | undefined;
    if (tab.type === "terminal" && tab.sessionId) {
      const oldTerm = terminalRegistryRef.current.get(tab.sessionId);
      if (oldTerm) {
        try {
          const buf = oldTerm.buffer.active;
          const lines: string[] = [];
          for (let i = 0; i < buf.length; i++) {
            const line = buf.getLine(i);
            if (line) lines.push(line.translateToString(true));
          }
          // Limit to last 5000 lines to avoid pathological memory use.
          snapshot = lines.slice(-5000).join("\r\n");
        } catch {
          // Buffer read failed — proceed without snapshot.
        }
      }
    }

    // Disconnect the old session first (best-effort — it may already be dead).
    // We do NOT change the tab id: it stays stable so the React key (and the
    // xterm instance) is preserved across reconnects. Only `sessionId` is
    // replaced. TerminalPanel detects the sessionId change and rebinds its
    // output/close subscriptions WITHOUT destroying the xterm terminal.
    if (tab.sessionId) {
      try {
        if (tab.connType === "ftp" && tab.ftpSessionId) {
          await ftpDisconnect(tab.ftpSessionId);
        } else if (tab.connType === "local") {
          await localDisconnect(tab.sessionId);
        } else {
          await sshDisconnect(tab.sessionId);
        }
      } catch {
        // Best-effort: the old session may already be gone. Ignore.
      }
    }

    // Stash the snapshot on the tab so the new TerminalPanel can restore it.
    if (snapshot) {
      setTabs((prev) =>
        prev.map((t) =>
          t.id === tabId ? { ...t, reconnectSnapshot: snapshot } : t
        )
      );
    }

    // Update status to connecting
    setTabs((prev) =>
      prev.map((t) =>
        t.id === tabId
          ? { ...t, status: "connecting", errorMessage: undefined }
          : t
      )
    );

    try {
      const config = tab.config;
      const connType = config.conn_type ?? "ssh";

      if (connType === "ftp") {
        const ftpId = await ftpConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  sessionId: ftpId,
                  ftpSessionId: ftpId,
                  status: "connected",
                  errorMessage: undefined,
                }
              : t
          )
        );
      } else if (connType === "local") {
        const sessionId = await localConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  sessionId,
                  status: "connected",
                  errorMessage: undefined,
                }
              : t
          )
        );
      } else {
        const sessionId = await sshConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  sessionId,
                  status: "connected",
                  errorMessage: undefined,
                }
              : t
          )
        );
      }
      // No broadcast-group id migration needed anymore: tab.id is stable now,
      // so it never falls out of broadcastIds on reconnect.
      return true;
    } catch (e) {
      const errorMessage = String(e);
      setTabs((prev) =>
        prev.map((t) =>
          t.id === tabId
            ? {
                ...t,
                status: "error",
                errorMessage,
                hostKeyMismatch: errorMessage.includes("主机密钥已变更"),
              }
            : t
        )
      );
      return false;
    }
  }

  async function handleReconnect(tabId: string) {
    await reconnectOne(tabId);
    // Broadcast cascade: if the tab we just reconnected is in the broadcast
    // group, reconnect every OTHER member of the same group that's also down
    // (disconnected/error). This lets a user revive a whole jump-box fleet
    // with a single click instead of reconnecting each tab one by one.
    // Members already connected/connecting are left alone.
    if (broadcastIds.has(tabId)) {
      const downSiblings = tabs.filter(
        (t) =>
          t.id !== tabId &&
          broadcastIds.has(t.id) &&
          t.config &&
          (t.status === "disconnected" || t.status === "error")
      );
      // Reconnect in parallel — they're independent sessions.
      await Promise.all(downSiblings.map((t) => reconnectOne(t.id)));
    }
  }

  /** Host-key mismatch recovery: forget the stored fingerprint for this host,
   * then reconnect (which re-runs trust-on-first-use and accepts the new key).
   * Triggered by the error screen's reconnect button when hostKeyMismatch. */
  async function handleResetHostKeyAndReconnect(tabId: string) {
    const tab = tabs.find((t) => t.id === tabId);
    if (!tab?.config) return;
    try {
      await resetKnownHost(tab.config.host, tab.config.port);
    } catch (e) {
      window.alert(`重置主机密钥失败: ${e}`);
      return;
    }
    await handleReconnect(tabId);
  }

  /** Batch-close every tab that's currently offline (disconnected or error).
   * Used by the "一键删除掉线会话" button in both dropdown panels. Reuses
   * handleCloseTab per-id so each disconnect + broadcast cleanup + active-tab
   * fallback runs identically to a manual close. */
  async function handleCloseDisconnected() {
    const down = tabs.filter(
      (t) => t.status === "disconnected" || t.status === "error"
    );
    await Promise.all(down.map((t) => handleCloseTab(t.id)));
  }

  async function handleCloseTab(tabId: string) {
    const tab = tabs.find((t) => t.id === tabId);
    if (tab?.sessionId) {
      try {
        if (tab.connType === "ftp" && tab.ftpSessionId) {
          if (tab.status === "connected") await ftpDisconnect(tab.ftpSessionId);
        } else if (tab.connType === "local") {
          if (tab.status === "connected") await localDisconnect(tab.sessionId);
        } else {
          // SSH/SFTP: always disconnect on tab close. ssh::disconnect is
          // idempotent (returns Ok if the session is already gone), and the
          // session now survives a shell-channel close (status becomes
          // "disconnected") so SFTP keeps working for SFTP-only accounts.
          // Skipping this call would leak the backend session + its TCP
          // connection until app exit.
          await sshDisconnect(tab.sessionId);
        }
      } catch (e) {
        console.error("Disconnect error:", e);
      }
    }
    setTabs((prev) => prev.filter((t) => t.id !== tabId));
    // Also drop the closed tab from the broadcast group so stale sessionIds
    // don't end up in the next broadcast fan-out.
    setBroadcastIds((prev) => {
      if (!prev.has(tabId)) return prev;
      const next = new Set(prev);
      next.delete(tabId);
      return next;
    });
    if (activeTabId === tabId) {
      const remaining = tabs.filter((t) => t.id !== tabId);
      setActiveTabId(remaining.length > 0 ? remaining[remaining.length - 1].id : null);
    }
  }

  function handleEdit(config: ConnectionConfig) {
    setEditConfig(config);
    setInitialConnType(undefined);
    setInitialFolderPath(undefined);
    setShowDialog(true);
  }

  function handleAddNew(initialType?: ConnType, initialFolderPath?: string) {
    setEditConfig(null);
    setInitialConnType(initialType);
    setInitialFolderPath(initialFolderPath);
    setShowDialog(true);
  }

  const activeTab = tabs.find((t) => t.id === activeTabId);

  // Vault gate — render the setup/unlock screen first, blocking the main
  // UI until the master key is loaded. Each onSuccess flips vault to
  // "checking" so the effect above re-queries status (and runs reload).
  if (vault === "checking") {
    return (
      <div
        style={{
          position: "fixed",
          inset: 0,
          background: "var(--bg-base)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "var(--text-muted)",
          fontSize: 13,
        }}
      >
        加载中…
      </div>
    );
  }
  if (vault === "setup") {
    return <MasterPasswordGate mode="setup" onSuccess={() => setVault("checking")} />;
  }
  if (vault === "unlock") {
    return <MasterPasswordGate mode="unlock" onSuccess={() => setVault("checking")} />;
  }

  return (
    <div style={{ display: "flex", height: "100vh", overflow: "hidden" }}>
      <Sidebar
        connections={connections}
        folders={folders}
        onConnect={handleConnect}
        onEdit={handleEdit}
        onDelete={async (id) => {
          // Soft-delete (move to recycle bin). The confirm gate lives in
          // Sidebar.handleDeleteConnection (closer to the UI / naming), so by
          // the time we reach here the user has already confirmed.
          await deleteConnection(id);
          reload();
        }}
        onAddNew={handleAddNew}
        onRefresh={reload}
        onOpenSettings={() => setShowSettings(true)}
        onOpenQuickCommands={() => {
          setQcInitialConnectionId(null);
          setShowQuickCommands(true);
        }}
        collapsed={sidebarCollapsed}
        onToggleCollapsed={() => setSidebarCollapsed((v) => !v)}
        width={sidebarWidth}
        onWidthChange={(w) => {
          setSidebarWidth(w);
          localStorage.setItem("myshell.sidebarWidth", String(w));
        }}
        version={appVersion}
        updateAvailable={!!updateInfo?.has_update}
        onOpenAbout={() => setAbout({ open: true, mode: "about" })}
        onOpenFeedback={() => setShowFeedback(true)}
      />
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        <TabBar
          tabs={tabs}
          activeTabId={activeTabId}
          onSelect={setActiveTabId}
          onClose={handleCloseTab}
          onReconnect={handleReconnect}
          broadcastIds={broadcastIds}
          onToggleBroadcast={toggleBroadcast}
          onCloseDisconnected={handleCloseDisconnected}
        />
        <div
          style={{
            flex: 1,
            display: "flex",
            flexDirection: "column",
            overflow: "hidden",
            position: "relative",
          }}
        >
          {tabs.length === 0 ? (
            <WelcomeScreen />
          ) : (
            tabs.map((tab) => {
              if (!tab.sessionId && tab.status !== "error" && tab.status !== "connecting") return null;
              const isActive = tab.id === activeTabId;
              // Use position:absolute + visibility:hidden instead of
              // display:none so every tab's container keeps a real size
              // at all times. xterm's renderer breaks when its container
              // collapses to 0x0 (RenderService throws "Cannot read
              // properties of undefined (reading 'dimensions')"), and the
              // FitAddon would otherwise read 0 cols/rows during the
              // hidden transition — which then propagates through the
              // broadcast sync as COLUMNS=0 and turns `ls` output into
              // one-file-per-line. Stacking tabs absolutely means the
              // ResizeObserver always sees the parent's full geometry.
              return (
                <div
                  key={tab.id}
                  style={{
                    position: "absolute",
                    inset: 0,
                    visibility: isActive ? "visible" : "hidden",
                    display: "flex",
                    flexDirection: "column",
                    overflow: "hidden",
                  }}
                >
                  {tab.status === "error" ? (
                    <ErrorState
                      message={tab.errorMessage || "连接失败"}
                      onReconnect={() => handleReconnect(tab.id)}
                      onClose={() => handleCloseTab(tab.id)}
                      hostKeyMismatch={tab.hostKeyMismatch}
                      onResetHostKeyAndReconnect={() => handleResetHostKeyAndReconnect(tab.id)}
                    />
                  ) : tab.status === "connecting" ? (
                    <ConnectingState />
                  ) : tab.type === "terminal" ? (
                    <TerminalPanel
                      tabId={tab.id}
                      sessionId={tab.sessionId!}
                      connectionId={tab.connectionId || ""}
                      connType={tab.connType}
                      fontOverride={connections.find((c) => c.id === (tab.connectionId || ""))?.terminal_font}
                      rendererBackend={rendererBackend}
                      broadcastTargets={getBroadcastTargets(tab)}
                      onTerminalReady={handleTerminalReady}
                      onTerminalGone={handleTerminalGone}
                      onOpenAi={() => setShowAiPanel((prev) => !prev)}
                      active={isActive}
                      status={tab.status}
                      onReconnect={() => handleReconnect(tab.id)}
                      connectionName={tab.name}
                      reconnectSnapshot={tab.reconnectSnapshot}
                      onSnapshotConsumed={() => clearReconnectSnapshot(tab.id)}
                      onOpenQuickCommandsManage={() => {
                        setQcInitialConnectionId(tab.connectionId || null);
                        setShowQuickCommands(true);
                      }}
                      onDisconnected={() => {
                        setTabs((prev) =>
                          prev.map((t) =>
                            t.id === tab.id ? { ...t, status: "disconnected" as const } : t
                          )
                        );
                      }}
                    />
                  ) : tab.type === "sftp" || tab.type === "ftp" ? (
                    <SftpPanel
                      sessionId={tab.sessionId!}
                      source={tab.type === "ftp" ? "ftp" : "ssh"}
                      fullHeight
                      status={tab.status}
                      onReconnect={() => handleReconnect(tab.id)}
                      onDisconnected={() => {
                        setTabs((prev) =>
                          prev.map((t) =>
                            t.id === tab.id ? { ...t, status: "disconnected" as const } : t
                          )
                        );
                      }}
                    />
                  ) : (
                    <TerminalPanel
                      tabId={tab.id}
                      sessionId={tab.sessionId!}
                      connectionId={tab.connectionId || ""}
                      connType={tab.connType}
                      fontOverride={connections.find((c) => c.id === (tab.connectionId || ""))?.terminal_font}
                      rendererBackend={rendererBackend}
                      onTerminalReady={handleTerminalReady}
                      onTerminalGone={handleTerminalGone}
                      onOpenAi={() => setShowAiPanel((prev) => !prev)}
                      active={isActive}
                      status={tab.status}
                      onReconnect={() => handleReconnect(tab.id)}
                      reconnectSnapshot={tab.reconnectSnapshot}
                      onSnapshotConsumed={() => clearReconnectSnapshot(tab.id)}
                      onDisconnected={() => {
                        setTabs((prev) =>
                          prev.map((t) =>
                            t.id === tab.id ? { ...t, status: "disconnected" as const } : t
                          )
                        );
                      }}
                    />
                  )}
                </div>
              );
            })
          )}
        </div>
        {activeTab &&
          activeTab.sessionId &&
          activeTab.connType === "ssh" &&
          activeTab.type === "terminal" &&
          activeTab.status === "connected" && (
            <ServerInfoPanel
              sessionId={activeTab.sessionId}
              active={activeTab.id === activeTabId}
            />
          )}
      </div>
      {showAiPanel && (
        <AiPanel
          activeConnType={activeTab?.connType}
          activeConnectionName={activeTab?.config?.name ?? activeTab?.connectionId}
          activeSessionId={activeTab?.sessionId}
          getTerminal={getTerminal}
          width={aiPanelWidth}
          onWidthChange={(w) => {
            setAiPanelWidth(w);
            localStorage.setItem("myshell.aiPanelWidth", String(w));
          }}
          onClose={() => setShowAiPanel(false)}
        />
      )}
      {showDialog && (
        <ConnectionDialog
          config={editConfig}
          initialConnType={initialConnType}
          initialFolderPath={initialFolderPath}
          folders={folders}
          onClose={() => {
            setShowDialog(false);
            setEditConfig(null);
            setInitialConnType(undefined);
            setInitialFolderPath(undefined);
          }}
          onSave={() => {
            setShowDialog(false);
            setEditConfig(null);
            setInitialConnType(undefined);
            setInitialFolderPath(undefined);
            reload();
          }}
        />
      )}
      {showSettings && (
        <SettingsPanel
          onClose={() => setShowSettings(false)}
          onRefresh={reload}
          connectionCount={connections.length}
          onOpenQuickCommands={() => {
            setQcInitialConnectionId(null);
            setShowSettings(false);
            setShowQuickCommands(true);
          }}
        />
      )}
      {showQuickCommands && (
        <QuickCommandsPanel
          onClose={() => setShowQuickCommands(false)}
          connections={connections}
          initialConnectionId={qcInitialConnectionId}
          activeConnectionId={activeTab?.connectionId ?? null}
        />
      )}
      {about.open && (
        <AboutDialog
          mode={about.mode}
          version={appVersion}
          updateInfo={updateInfo}
          checking={updateChecking}
          onClose={closeAbout}
          onCheckUpdates={checkNow}
          onDownload={(url) => {
            void openExternalUrl(url);
          }}
        />
      )}
      {showFeedback && (
        <FeedbackDialog
          version={appVersion}
          onClose={() => setShowFeedback(false)}
        />
      )}
      {statsPrompt && (
        <StatsConsentDialog
          version={statsPrompt.version}
          onAgree={() => {
            setStatsConsent(true);
            // NOTE: do NOT markVersionHandled here. reportVersion marks the
            // version stamp only AFTER a successful fetch — so if the send
            // fails (network/CSP), the next launch re-enters checkReportNeeded,
            // sees hasConsent=true (just set), and silently retries WITHOUT
            // re-prompting the user. Marking here would deadlock: user agreed
            // but the event was never delivered, and we'd never retry.
            void reportVersion(statsPrompt.version, navigator.platform);
            setStatsPrompt(null);
          }}
          onDecline={() => {
            setStatsConsent(false);
            markVersionHandled(statsPrompt.version);
            setStatsPrompt(null);
          }}
        />
      )}
      {vault === "ready" && updateInfo?.has_update && (
        <UpdateNotification
          updateInfo={updateInfo}
        />
      )}

      {/* MCP exec_in_tab command confirmation — shown when AI runs a dangerous
          command in show_in_gui mode. Uses the same ConfirmDialog as other
          destructive prompts for visual consistency. */}
      {mcpConfirm && (
        <ConfirmDialog
          title="🤖 AI 请求执行高危命令"
          message={
            <>
              <div style={{ marginBottom: 8 }}>
                AI agent 通过 MCP 请求在服务器{" "}
                <strong style={{ color: "var(--text-primary)" }}>
                  [{mcpConfirm.connectionName}]
                </strong>{" "}
                上执行以下命令：
              </div>
              <div
                style={{
                  background: "var(--bg-base)",
                  border: "1px solid var(--border-default)",
                  borderRadius: "var(--radius-sm)",
                  padding: "8px 10px",
                  fontFamily: "monospace",
                  fontSize: 12,
                  color: "var(--text-primary)",
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-all",
                  maxHeight: 120,
                  overflow: "auto",
                }}
              >
                {highlightDangerous(mcpConfirm.command, mcpConfirm.rules)}
              </div>
              <div style={{ marginTop: 8, color: "var(--text-muted)" }}>
                点击「确认执行」允许，点击「取消」拒绝。取消后 AI 会收到错误消息。
              </div>
            </>
          }
          confirmLabel="确认执行"
          cancelLabel="取消"
          danger={true}
          onConfirm={() => {
            setMcpConfirm(null);
            mcpConfirmResolver.current?.(true);
            mcpConfirmResolver.current = null;
          }}
          onCancel={() => {
            setMcpConfirm(null);
            mcpConfirmResolver.current?.(false);
            mcpConfirmResolver.current = null;
          }}
        />
      )}

      {broadcastDupPrompt && (
        <BroadcastDupDialog
          connectionName={broadcastDupPrompt.connectionName}
          existingCount={broadcastDupPrompt.existingCount}
          initialDontRemind={broadcastDupPrompt.initialDontRemind}
          onConfirm={(dontRemindAgain) => {
            // Remember the user's checkbox choice for the next prompt this
            // session, then fold it into the session dismiss flag: checked ⇒
            // all future duplicate adds skip the prompt; unchecked ⇒ only this
            // add is allowed through, the next one will prompt again.
            broadcastDupDontRemindPrefRef.current = dontRemindAgain;
            if (dontRemindAgain) broadcastDupDismissedRef.current = true;
            setBroadcastMembership(broadcastDupPrompt.tabId, true);
            setBroadcastDupPrompt(null);
          }}
          onCancel={(lastDontRemind) => {
            // Even on cancel we keep the user's checkbox preference so the
            // next prompt reopens in the same state they left it.
            broadcastDupDontRemindPrefRef.current = lastDontRemind;
            setBroadcastDupPrompt(null);
          }}
        />
      )}
    </div>
  );
}

function WelcomeScreen() {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        height: "100%",
        color: "var(--text-muted)",
        gap: 16,
      }}
    >
      <BrandLogo size={72} glow />
      <div style={{ fontSize: 20, fontWeight: 600, color: "var(--text-secondary)" }}>MyShell</div>
      <div style={{ fontSize: 13 }}>点击左侧连接列表开始新会话</div>
    </div>
  );
}

function ConnectingState() {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        height: "100%",
        color: "var(--text-muted)",
        gap: 12,
      }}
    >
      <div
        style={{
          fontSize: 24,
          animation: "spin 1s linear infinite",
        }}
      >
        ⏳
      </div>
      <div style={{ fontSize: 14 }}>正在连接...</div>
    </div>
  );
}

function ErrorState({
  message,
  onReconnect,
  onClose,
  hostKeyMismatch,
  onResetHostKeyAndReconnect,
}: {
  message: string;
  onReconnect: () => void;
  onClose: () => void;
  /** True when the connect failed because the server's host key changed.
   * Swaps the reconnect button to a "reset key & reconnect" action. */
  hostKeyMismatch?: boolean;
  onResetHostKeyAndReconnect?: () => void;
}) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        height: "100%",
        padding: 32,
      }}
    >
      <div
        style={{
          width: 80,
          height: 80,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "var(--error-muted)",
          borderRadius: "var(--radius-xl)",
          fontSize: 36,
          marginBottom: 20,
        }}
      >
        ❌
      </div>
      <div
        style={{
          fontSize: 18,
          fontWeight: 600,
          color: "var(--text-primary)",
          marginBottom: 8,
        }}
      >
        连接失败
      </div>
      <div
        style={{
          fontSize: 13,
          color: "var(--text-tertiary)",
          textAlign: "center",
          maxWidth: 400,
          marginBottom: 24,
          lineHeight: 1.6,
        }}
      >
        请确认下ip端口等是否填写错误!
        <br />
        {message}
      </div>
      <div style={{ display: "flex", gap: 12 }}>
        <button
          onClick={onClose}
          style={{
            padding: "10px 24px",
            background: "transparent",
            color: "var(--text-secondary)",
            border: "1px solid var(--border-default)",
            borderRadius: "var(--radius-md)",
            fontSize: 13,
            cursor: "pointer",
            transition: "all var(--duration-fast) var(--ease-in-out)",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "var(--bg-surface-hover)";
            e.currentTarget.style.borderColor = "var(--border-emphasis)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "transparent";
            e.currentTarget.style.borderColor = "var(--border-default)";
          }}
        >
          关闭
        </button>
        <button
          onClick={hostKeyMismatch ? onResetHostKeyAndReconnect : onReconnect}
          style={{
            padding: "10px 24px",
            background: "var(--accent-primary)",
            color: "white",
            border: "none",
            borderRadius: "var(--radius-md)",
            fontSize: 13,
            fontWeight: 600,
            cursor: "pointer",
            transition: "all var(--duration-fast) var(--ease-in-out)",
            boxShadow: "var(--shadow-glow)",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "var(--accent-primary-hover)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "var(--accent-primary)";
          }}
        >
          {hostKeyMismatch ? "重置密钥并重连" : "重新连接"}
        </button>
      </div>
    </div>
  );
}
