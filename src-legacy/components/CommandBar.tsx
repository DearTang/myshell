import { useState, useEffect, useCallback, useRef } from "react";
import {
  listCommandHistory,
  setCommandHistoryPinned,
  deleteCommandHistory,
  clearCommandHistory,
  addCommandHistory,
  sshSend,
  localSend,
  listQuickCommandsForConnection,
  onSshOutput,
} from "../api";
import type { CommandHistoryItem, ConnType, QuickCommandExecItem } from "../api";

interface Props {
  sessionId: string;
  connectionId: string;
  /** Tab connection type — picks ssh_send vs local_send. Defaults to ssh. */
  connType?: ConnType;
  /** Broadcast target session IDs. If non-empty, commands from the input box
   * will be sent to all targets (including this session). */
  broadcastTargets?: string[];
  /** Callback to register our reload function with the parent's ref so
   * onData can trigger a history refresh after recording a new command. */
  onRegisterRefresh?: (fn: () => void) => void;
  /** Connection status: "connecting" | "connected" | "disconnected" | "error" */
  status?: "connecting" | "connected" | "disconnected" | "error";
  /** Callback to reconnect when status is disconnected/error */
  onReconnect?: () => void;
  /** Callback to open the quick-commands management panel. */
  onOpenQuickCommandsManage?: () => void;
  /** Open the docked AI assistant panel. */
  onOpenAi?: () => void;
  /** Open the multi-window session picker. */
  onOpenMultiWindow?: () => void;
  /** Capture a PNG screenshot of the terminal viewport (excludes this
   * CommandBar). Saves to the configured attachment directory. */
  onScreenshot?: () => void;
}

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

export function CommandBar({ sessionId, connectionId, connType, broadcastTargets = [], onRegisterRefresh, status, onReconnect, onOpenQuickCommandsManage, onOpenAi, onOpenMultiWindow, onScreenshot }: Props) {
  // Pick the send backend by connection type — local tabs must route to
  // local_send, not ssh_send.
  const sendFn = connType === "local" ? localSend : sshSend;
  const [input, setInput] = useState("");
  const [history, setHistory] = useState<CommandHistoryItem[]>([]);
  const [panelOpen, setPanelOpen] = useState(false);
  const [loading, setLoading] = useState(false);

  // Quick-commands panel state. Lists the union of global + this connection's
  // per-server commands, grouped by scope in the floating panel.
  const [quickPanelOpen, setQuickPanelOpen] = useState(false);
  const [quickCommands, setQuickCommands] = useState<QuickCommandExecItem[]>([]);
  const [quickLoading, setQuickLoading] = useState(false);

  // Root container ref so we can scope DOM queries (e.g. focusing the input
  // after selecting a history item) to THIS CommandBar — a global
  // document.querySelector would target whichever tab's CommandBar rendered
  // first, hijacking focus to the wrong tab.
  const containerRef = useRef<HTMLDivElement>(null);
  // Narrow-screen mode: hide button text labels when the bar is too narrow
  // (multi-window grid cells, small windows) to prevent input-box squeeze.
  const [showLabels, setShowLabels] = useState(true);
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      setShowLabels(el.clientWidth > 520);
    });
    ro.observe(el);
    setShowLabels(el.clientWidth > 520);
    return () => ro.disconnect();
  }, []);

  const reload = useCallback(async () => {
    if (!connectionId) return;
    setLoading(true);
    try {
      const items = await listCommandHistory(connectionId);
      setHistory(items);
    } catch {
      // Silently ignore — not critical.
    } finally {
      setLoading(false);
    }
  }, [connectionId]);

  // Load on mount and register refresh callback.
  useEffect(() => {
    reload();
    onRegisterRefresh?.(reload);
  }, [reload, onRegisterRefresh]);

  async function handleExecute(cmd: string) {
    if (!cmd.trim()) return;

    // Determine broadcast destinations
    const destinations = broadcastTargets.length > 0 ? broadcastTargets : [sessionId];

    // Send to all destinations (broadcast or single)
    await Promise.allSettled(
      destinations.map((sid) => sendFn(sid, cmd + "\r"))
    );

    // Record to history (only once, for this connection)
    if (connectionId) {
      addCommandHistory(connectionId, cmd)
        .then(() => reload())
        .catch(() => {});
    }
    setInput("");
  }

  async function handlePin(item: CommandHistoryItem) {
    await setCommandHistoryPinned(item.id, !item.pinned);
    await reload();
  }

  async function handleDelete(id: number) {
    await deleteCommandHistory(id);
    await reload();
  }

  async function handleClearUnpinned() {
    if (!connectionId) return;
    await clearCommandHistory(connectionId, false);
    await reload();
  }

  // ============ Quick Commands ============

  const reloadQuickCommands = useCallback(async () => {
    if (!connectionId) return;
    setQuickLoading(true);
    try {
      const items = await listQuickCommandsForConnection(connectionId);
      setQuickCommands(items);
    } catch {
      // Silently ignore — not critical.
    } finally {
      setQuickLoading(false);
    }
  }, [connectionId]);

  // Load on open (cheap; the panel is usually closed).
  useEffect(() => {
    if (quickPanelOpen) reloadQuickCommands();
  }, [quickPanelOpen, reloadQuickCommands]);

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
  async function handleExecuteQuickCommand(command: string) {
    if (status === "disconnected" || status === "error") return;
    const steps = parseQuickCommand(command);
    if (steps.length === 0) return;
    const { mode, ms: configMs } = readQuickCmdDelayConfig();
    const destinations = broadcastTargets.length > 0 ? broadcastTargets : [sessionId];
    const useIdle = mode === "idle";
    // Idle quiescence window — floor at 300 ms so a misconfigured 0 still waits
    // for output to actually settle.
    const quietMs = useIdle ? Math.max(configMs, 300) : 0;

    setQuickPanelOpen(false);

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
        unlisten = await onSshOutput(sessionId, () => {
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
            mode === "fixed" ? configMs : 0
          );
          if (floorMs > 0) await sleep(floorMs);
          if (useIdle) await waitForQuiescence();
        }
        // Arm the landing flag BEFORE sending so this line's echo flips it
        // true and arms the next gap's quiescence check.
        if (useIdle) landed = false;
        await Promise.allSettled(
          destinations.map((sid) => sendFn(sid, step.text + "\r"))
        );
        pendingDelay = 0;
        hadExplicitDelay = false;
        firstCmd = false;
      }
    } finally {
      unlisten?.();
    }
  }

  /** 点击历史项：填入输入框，不自动执行 */
  function handleSelectItem(cmd: string) {
    setInput(cmd);
    setPanelOpen(false);
    // 聚焦到输入框方便用户修改后回车执行。Scope to this CommandBar's
    // container so focus stays in the active tab when several are open.
    const inputEl = containerRef.current?.querySelector<HTMLInputElement>('[data-cmd-input]');
    inputEl?.focus();
  }

  return (
    <div
      ref={containerRef}
      style={{
        height: 36,
        minHeight: 36,
        background: "var(--bg-elevated)",
        borderTop: "1px solid var(--border-default)",
        display: "flex",
        alignItems: "center",
        padding: "0 12px",
        gap: 8,
        position: "relative",
      }}
    >
      {/* Command input */}
      <input
        data-cmd-input
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && input.trim()) {
            handleExecute(input);
          }
        }}
        placeholder="$ 输入命令..."
        style={{
          flex: 1,
          background: "var(--bg-input)",
          border: "1px solid var(--border-default)",
          borderRadius: "var(--radius-md)",
          padding: "6px 10px",
          color: "var(--text-primary)",
          fontSize: 13,
          outline: "none",
        }}
      />

      {/* Quick commands button */}
      <button
        onClick={() => {
          setPanelOpen(false);
          setQuickPanelOpen((v) => !v);
        }}
        title="快捷命令"
        style={{
          background: quickPanelOpen ? "var(--accent-primary)" : "var(--bg-input)",
          color: quickPanelOpen ? "white" : "var(--text-secondary)",
          border: "1px solid var(--border-default)",
          borderRadius: "var(--radius-md)",
          padding: "6px 12px",
          flexShrink: 0,
          whiteSpace: "nowrap",
          fontSize: 12,
          cursor: "pointer",
          display: "flex",
          alignItems: "center",
          gap: 4,
          transition: "all var(--duration-fast) var(--ease-in-out)",
        }}
      >
        <span>⌨</span>
        {showLabels && <span>快捷</span>}
      </button>

      {/* Screenshot button — captures the terminal viewport only (this
          CommandBar is excluded by the capture util). Saves to the
          attachment dir configured in Settings → MCP 支持. */}
      <button
        onClick={() => {
          setPanelOpen(false);
          setQuickPanelOpen(false);
          onScreenshot?.();
        }}
        title="截取当前终端（不含输入栏/工具栏）"
        style={{
          background: "var(--bg-input)",
          color: "var(--text-secondary)",
          border: "1px solid var(--border-default)",
          borderRadius: "var(--radius-md)",
          padding: "6px 12px",
          flexShrink: 0,
          whiteSpace: "nowrap",
          fontSize: 12,
          cursor: "pointer",
          display: "flex",
          alignItems: "center",
          gap: 4,
          transition: "all var(--duration-fast) var(--ease-in-out)",
        }}
      >
        <span>📷</span>
        {showLabels && <span>截图</span>}
      </button>

      {/* History button */}
      <button
        onClick={() => {
          setQuickPanelOpen(false);
          setPanelOpen((v) => !v);
        }}
        title="历史命令"
        style={{
          background: panelOpen ? "var(--accent-primary)" : "var(--bg-input)",
          color: panelOpen ? "white" : "var(--text-secondary)",
          border: "1px solid var(--border-default)",
          borderRadius: "var(--radius-md)",
          padding: "6px 12px",
          flexShrink: 0,
          whiteSpace: "nowrap",
          fontSize: 12,
          cursor: "pointer",
          display: "flex",
          alignItems: "center",
          gap: 4,
          transition: "all var(--duration-fast) var(--ease-in-out)",
        }}
      >
        <span>📜</span>
        {showLabels && <span>历史</span>}
      </button>

            {/* Multi-window picker button */}
      <button
        onClick={() => onOpenMultiWindow?.()}
        title="多窗口"
        style={{
          background: "var(--bg-input)",
          color: "var(--text-secondary)",
          border: "1px solid var(--border-default)",
          borderRadius: "var(--radius-md)",
          padding: "6px 12px",
          flexShrink: 0,
          whiteSpace: "nowrap",
          fontSize: 12,
          cursor: "pointer",
          display: "flex",
          alignItems: "center",
          gap: 4,
          transition: "all var(--duration-fast) var(--ease-in-out)",
        }}
      >
        <span>🪟</span>
        {showLabels && <span>多窗口</span>}
      </button>

      {/* AI assistant button — opens the docked right panel */}
      <button
        onClick={() => onOpenAi?.()}
        title="AI 助手"
        style={{
          background: "var(--bg-input)",
          color: "var(--text-secondary)",
          border: "1px solid var(--border-default)",
          borderRadius: "var(--radius-md)",
          padding: "6px 12px",
          flexShrink: 0,
          whiteSpace: "nowrap",
          fontSize: 12,
          cursor: "pointer",
          display: "flex",
          alignItems: "center",
          gap: 4,
          transition: "all var(--duration-fast) var(--ease-in-out)",
        }}
      >
        <span>🤖</span>
        {showLabels && <span>AI</span>}
      </button>

      {/* Reconnect button (only when disconnected/error) */}
      {(status === "disconnected" || status === "error") && onReconnect && (
        <button
          onClick={onReconnect}
          title="重新连接"
          style={{
            background: "var(--success-muted)",
            color: "var(--success)",
            border: "1px solid var(--success)",
            borderRadius: 6,
            padding: "6px 12px",
          flexShrink: 0,
          whiteSpace: "nowrap",
            fontSize: 12,
            cursor: "pointer",
            display: "flex",
            alignItems: "center",
            gap: 4,
            fontWeight: 600,
            transition: "all var(--duration-fast) var(--ease-in-out)",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "var(--success)";
            e.currentTarget.style.color = "white";
            e.currentTarget.style.transform = "scale(1.05)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "var(--success-muted)";
            e.currentTarget.style.color = "var(--success)";
            e.currentTarget.style.transform = "scale(1)";
          }}
        >
          <span>⚡</span>
          {showLabels && <span>重连</span>}
        </button>
      )}

      {/* Expanded history panel (floating overlay) */}
      {panelOpen && (
        <div
          style={{
            position: "absolute",
            bottom: 36,
            right: 8,
            width: 480,
            maxHeight: "60vh",
            background: "var(--bg-surface)",
            border: "1px solid var(--border-emphasis)",
            borderRadius: "var(--radius-lg)",
            boxShadow: "var(--shadow-xl)",
            display: "flex",
            flexDirection: "column",
            zIndex: 10,
          }}
        >
          {/* Header */}
          <div
            style={{
              padding: "8px 12px",
              borderBottom: "1px solid var(--border-default)",
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}
          >
            <span style={{ fontSize: 12, fontWeight: 600, color: "var(--text-secondary)" }}>
              历史命令 {loading && "(加载中...)"}
            </span>
            <button
              onClick={handleClearUnpinned}
              title="清空未钉住的历史"
              style={{
                background: "transparent",
                border: "none",
                color: "var(--text-muted)",
                fontSize: 11,
                cursor: "pointer",
              }}
            >
              清空
            </button>
          </div>

          {/* List */}
          <div
            style={{
              flex: 1,
              overflowY: "auto",
              padding: "4px 0",
            }}
          >
            {history.length === 0 ? (
              <div
                style={{
                  padding: 16,
                  textAlign: "center",
                  color: "var(--text-muted)",
                  fontSize: 12,
                }}
              >
                暂无历史记录
              </div>
            ) : (
              history.map((item) => (
                <div
                  key={item.id}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    padding: "6px 12px",
          flexShrink: 0,
          whiteSpace: "nowrap",
                    gap: 8,
                    cursor: "pointer",
                    transition: "background var(--duration-fast) var(--ease-in-out)",
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = "var(--bg-surface-hover)";
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = "transparent";
                  }}
                  onClick={() => handleSelectItem(item.command)}
                >
                  {/* Pin icon */}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handlePin(item);
                    }}
                    title={item.pinned ? "取消钉住" : "钉住"}
                    style={{
                      background: "transparent",
                      border: "none",
                      cursor: "pointer",
                      fontSize: 14,
                      opacity: item.pinned ? 1 : 0.4,
                      padding: 0,
                    }}
                  >
                    {/* 📌 in both states — pinned is fully lit, unpinned dimmed
                        via opacity. 📍 round-pushpin read as a balloon. */}
                    📌
                  </button>

                  {/* Command text */}
                  <span
                    style={{
                      flex: 1,
                      fontSize: 12,
                      color: "var(--text-primary)",
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {item.command}
                  </span>

                  {/* Delete button */}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleDelete(item.id);
                    }}
                    title="删除"
                    style={{
                      background: "transparent",
                      border: "none",
                      color: "var(--text-muted)",
                      fontSize: 12,
                      cursor: "pointer",
                      opacity: 0.6,
                    }}
                  >
                    ✕
                  </button>
                </div>
              ))
            )}
          </div>
        </div>
      )}

      {/* Expanded quick commands panel (floating overlay) */}
      {quickPanelOpen && (
        <div
          style={{
            position: "absolute",
            bottom: 36,
            right: 8,
            width: 420,
            maxHeight: "60vh",
            background: "var(--bg-surface)",
            border: "1px solid var(--border-emphasis)",
            borderRadius: "var(--radius-lg)",
            boxShadow: "var(--shadow-xl)",
            display: "flex",
            flexDirection: "column",
            zIndex: 10,
          }}
        >
          {/* Header */}
          <div
            style={{
              padding: "8px 12px",
              borderBottom: "1px solid var(--border-default)",
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}
          >
            <span style={{ fontSize: 12, fontWeight: 600, color: "var(--text-secondary)" }}>
              快捷命令 {quickLoading && "(加载中...)"}
            </span>
            {onOpenQuickCommandsManage && (
              <button
                onClick={() => {
                  setQuickPanelOpen(false);
                  onOpenQuickCommandsManage();
                }}
                title="管理快捷命令"
                style={{
                  background: "transparent",
                  border: "none",
                  color: "var(--accent-primary)",
                  fontSize: 11,
                  cursor: "pointer",
                }}
              >
                管理
              </button>
            )}
          </div>

          {/* Grouped list */}
          <div style={{ flex: 1, overflowY: "auto", padding: "4px 0" }}>
            {quickCommands.length === 0 ? (
              <div
                style={{
                  padding: 16,
                  textAlign: "center",
                  color: "var(--text-muted)",
                  fontSize: 12,
                }}
              >
                暂无快捷命令
              </div>
            ) : (
              <>
                <QuickCommandGroup
                  title="🌐 全局命令"
                  items={quickCommands.filter((q) => q.isGlobal)}
                  onExecute={handleExecuteQuickCommand}
                />
                <QuickCommandGroup
                  title="📌 本服务器专属"
                  items={quickCommands.filter((q) => !q.isGlobal)}
                  onExecute={handleExecuteQuickCommand}
                />
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function QuickCommandGroup({
  title,
  items,
  onExecute,
}: {
  title: string;
  items: QuickCommandExecItem[];
  onExecute: (command: string) => void;
}) {
  if (items.length === 0) return null;
  return (
    <div>
      <div
        style={{
          padding: "8px 12px 4px",
          fontSize: 11,
          fontWeight: 600,
          color: "var(--text-muted)",
        }}
      >
        {title}
      </div>
      {items.map((item) => (
        <div
          key={item.id}
          title={item.command}
          onClick={() => onExecute(item.command)}
          style={{
            display: "flex",
            alignItems: "center",
            padding: "7px 12px",
            gap: 8,
            cursor: "pointer",
            transition: "background var(--duration-fast) var(--ease-in-out)",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "var(--bg-surface-hover)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "transparent";
          }}
        >
          <span style={{ fontSize: 11, color: "var(--accent-primary)" }}>▶</span>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div
              style={{
                fontSize: 12,
                color: "var(--text-primary)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {item.label}
            </div>
            <div
              style={{
                fontSize: 11,
                color: "var(--text-muted)",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {item.command.split(/\r?\n/)[0]}
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}