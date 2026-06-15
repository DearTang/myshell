import { useState, useEffect, useCallback } from "react";
import {
  listCommandHistory,
  setCommandHistoryPinned,
  deleteCommandHistory,
  clearCommandHistory,
  addCommandHistory,
  sshSend,
} from "../api";
import type { CommandHistoryItem } from "../api";

interface Props {
  sessionId: string;
  connectionId: string;
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
}

export function CommandBar({ sessionId, connectionId, broadcastTargets = [], onRegisterRefresh, status, onReconnect }: Props) {
  const [input, setInput] = useState("");
  const [history, setHistory] = useState<CommandHistoryItem[]>([]);
  const [panelOpen, setPanelOpen] = useState(false);
  const [loading, setLoading] = useState(false);

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
      destinations.map((sid) => sshSend(sid, cmd + "\r"))
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

  /** 点击历史项：填入输入框，不自动执行 */
  function handleSelectItem(cmd: string) {
    setInput(cmd);
    setPanelOpen(false);
    // 聚焦到输入框方便用户修改后回车执行
    const inputEl = document.querySelector<HTMLInputElement>('[data-cmd-input]');
    inputEl?.focus();
  }

  return (
    <div
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

      {/* History button */}
      <button
        onClick={() => setPanelOpen((v) => !v)}
        title="历史命令"
        style={{
          background: panelOpen ? "var(--accent-primary)" : "var(--bg-input)",
          color: panelOpen ? "white" : "var(--text-secondary)",
          border: "1px solid var(--border-default)",
          borderRadius: "var(--radius-md)",
          padding: "6px 12px",
          fontSize: 12,
          cursor: "pointer",
          display: "flex",
          alignItems: "center",
          gap: 4,
          transition: "all var(--duration-fast) var(--ease-in-out)",
        }}
      >
        <span>📜</span>
        <span>历史</span>
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
          <span>重连</span>
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
                    {item.pinned ? "📌" : "📍"}
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
    </div>
  );
}
