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
  listFolders,
  sshConnect,
  sshSend,
  sshDisconnect,
  ftpConnect,
  ftpDisconnect,
  localConnect,
  localDisconnect,
  vaultStatus,
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

        // Find or open a terminal tab for this connection (terminal type).
        const existing = tabsRef.current.find(
          (t) => t.connectionId === connection_id && t.type === "terminal" && t.status === "connected"
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
                mcpExecResult(requestId, {
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

          // Send: the command, then the sentinel probe.
          // The `\n` ensures the command is submitted (Enter).
          await sshSend(sessionId, command + "\n");
          // Small delay so the command is processed before the sentinel.
          await new Promise((r) => setTimeout(r, 100));
          await sshSend(sessionId, `echo ${sentinel}:$?\n`);

          // Subscribe to ssh_output for this session and accumulate bytes.
          let outputBuf = "";
          let done = false;
          let timedOut = false;

          const unlistenOutput = await onSshOutput(sessionId, (data) => {
            if (done) return;
            // Decode bytes to string (terminal output is UTF-8 / ASCII).
            outputBuf += new TextDecoder("utf-8", { fatal: false }).decode(data);
            // Check for the sentinel line. The shell echoes it as:
            //   __MCP_DONE_xxxx__:0
            const re = new RegExp(sentinel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + ":(\\d+)");
            const match = outputBuf.match(re);
            if (match) {
              done = true;
              const exitCode = parseInt(match[1], 10);
              // Extract stdout: everything between the command echo and the
              // sentinel. We find the sentinel's position and work backwards.
              const sentinelIdx = outputBuf.indexOf(sentinel);
              // The sentinel line itself starts at the beginning of its line —
              // find the preceding newline. Everything before the command's
              // own echo (the first line after we sent it) is prior output.
              // Simplest heuristic: strip everything from the sentinel onwards,
              // and strip the command echo line (first line).
              let stdout = outputBuf.slice(0, sentinelIdx);
              // Remove the last newline before the sentinel (it's the end of
              // the sentinel echo line's predecessor).
              stdout = stdout.replace(/\n$/, "");
              // The command itself was echoed by the PTY as the first line(s).
              // Try to strip it: find the first newline after the command text.
              const cmdLineEnd = stdout.indexOf("\n");
              if (cmdLineEnd >= 0 && cmdLineEnd < command.length + 20) {
                stdout = stdout.slice(cmdLineEnd + 1);
              }

              mcpExecResult(requestId, { ok: true, stdout, exit_code: exitCode });
            }
          });

          // Timeout safety: if the sentinel never appears (interactive command,
          // hang, etc.), return what we have + error.
          setTimeout(() => {
            if (!done && !timedOut) {
              timedOut = true;
              unlistenOutput();
              mcpExecResult(requestId, {
                ok: false,
                error: `命令超时（${event.payload.timeout || 30}秒未完成）`,
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

        if (existing && existing.sessionId) {
          // Tab already open and connected — use it directly.
          setActiveTabId(existing.id);
          runExec(existing.sessionId).catch((e) => {
            mcpExecResult(requestId, { ok: false, error: `执行失败: ${e}` });
          });
        } else {
          // Need to open a new tab. handleConnect is async (establishes SSH),
          // but we don't get the sessionId back easily — we need to watch for
          // the new tab to appear with status "connected", then extract its
          // sessionId.
          handleConnect(config);
          // Watch tabs state for the new connected tab.
          let attempts = 0;
          const maxAttempts = Math.floor(timeoutMs / 500);
          const checkInterval = setInterval(() => {
            attempts++;
            const tab = tabsRef.current.find(
              (t) => t.connectionId === connection_id && t.type === "terminal" && t.status === "connected" && t.sessionId
            );
            if (tab && tab.sessionId) {
              clearInterval(checkInterval);
              setActiveTabId(tab.id);
              runExec(tab.sessionId).catch((e) => {
                mcpExecResult(requestId, { ok: false, error: `执行失败: ${e}` });
              });
            } else if (attempts >= maxAttempts) {
              clearInterval(checkInterval);
              mcpExecResult(requestId, { ok: false, error: "连接超时，无法建立终端会话" });
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

    // Create a temporary tab with "connecting" status
    const tempTabId = `temp-${Date.now()}`;
    const tempTab: Tab = {
      id: tempTabId,
      name: display,
      type: connType === "ftp" ? "ftp" : connType === "sftp" ? "sftp" : "terminal",
      connType,
      connectionId: config.id,
      status: "connecting",
      config: config,
    };

    setTabs((prev) => [...prev, tempTab]);
    setActiveTabId(tempTabId);

    try {
      if (connType === "ftp") {
        const ftpId = await ftpConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tempTabId
              ? {
                  id: ftpId,
                  name: display,
                  sessionId: ftpId,
                  type: "ftp",
                  connType: "ftp",
                  ftpSessionId: ftpId,
                  connectionId: config.id,
                  status: "connected",
                  config: config,
                }
              : t
          )
        );
        setActiveTabId(ftpId);
      } else if (connType === "local") {
        const sessionId = await localConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tempTabId
              ? {
                  id: sessionId,
                  name: display,
                  sessionId,
                  type: "terminal",
                  connType: "local",
                  connectionId: config.id,
                  status: "connected",
                  config: config,
                }
              : t
          )
        );
        setActiveTabId(sessionId);
      } else {
        const sessionId = await sshConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tempTabId
              ? {
                  id: sessionId,
                  name: display,
                  sessionId,
                  type: connType === "sftp" ? "sftp" : "terminal",
                  connType,
                  connectionId: config.id,
                  status: "connected",
                  config: config,
                }
              : t
          )
        );
        setActiveTabId(sessionId);
      }
    } catch (e) {
      const errorMessage = String(e);
      // Update tab to show error state
      setTabs((prev) =>
        prev.map((t) =>
          t.id === tempTabId
            ? {
                ...t,
                status: "error",
                errorMessage: errorMessage,
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
      let newId = tabId;

      if (connType === "ftp") {
        const ftpId = await ftpConnect(config);
        newId = ftpId;
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  id: ftpId,
                  sessionId: ftpId,
                  ftpSessionId: ftpId,
                  status: "connected",
                  errorMessage: undefined,
                }
              : t
          )
        );
        setActiveTabId(ftpId);
      } else if (connType === "local") {
        const sessionId = await localConnect(config);
        newId = sessionId;
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  id: sessionId,
                  sessionId,
                  status: "connected",
                  errorMessage: undefined,
                }
              : t
          )
        );
        setActiveTabId(sessionId);
      } else {
        const sessionId = await sshConnect(config);
        newId = sessionId;
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  id: sessionId,
                  sessionId,
                  status: "connected",
                  errorMessage: undefined,
                }
              : t
          )
        );
        setActiveTabId(sessionId);
      }
      // A reconnect mints a fresh session id, so the tab's id changes. The
      // broadcast group stores tab IDs — if we don't migrate, the old id stays
      // in broadcastIds while no tab carries it anymore, so the reconnecting
      // tab silently drops out of the group (and stops receiving broadcast
      // keystrokes). Swap the old id for the new one to keep membership.
      if (newId !== tabId) {
        setBroadcastIds((prev) => {
          if (!prev.has(tabId)) return prev;
          const next = new Set(prev);
          next.delete(tabId);
          next.add(newId);
          return next;
        });
      }
      return true;
    } catch (e) {
      const errorMessage = String(e);
      setTabs((prev) =>
        prev.map((t) =>
          t.id === tabId
            ? { ...t, status: "error", errorMessage }
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
                    />
                  ) : tab.status === "connecting" ? (
                    <ConnectingState />
                  ) : tab.type === "terminal" ? (
                    <TerminalPanel
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
            markVersionHandled(statsPrompt.version);
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
}: {
  message: string;
  onReconnect: () => void;
  onClose: () => void;
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
          onClick={onReconnect}
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
          重新连接
        </button>
      </div>
    </div>
  );
}
