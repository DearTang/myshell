import { useState } from "react";

interface Props {
  /** Name of the connection already in the group, for context in the prompt. */
  connectionName: string;
  /** How many tabs of this connection are already broadcasting. */
  existingCount: number;
  /** Initial checkbox state — the user's last choice this session, so the
   * prompt reopens in the same state they left it (defaults to true on first
   * ever prompt). */
  initialDontRemind?: boolean;
  onConfirm: (dontRemindAgain: boolean) => void;
  /** Passes back the current checkbox state on cancel so the caller can
   * remember the user's preference for the next prompt. */
  onCancel: (lastDontRemind: boolean) => void;
}

/**
 * Confirms adding a tab to the broadcast group when another tab of the SAME
 * connection is already a member.
 *
 * Two tabs sharing one connection normally means double-execution on one
 * server — but the legit jump-box case (one connection → multiple tabs, each
 * SSH'd onward to a different target) needs the duplicate. Rather than decide
 * for the user, we ask once and let them choose. The "本次会话不再提醒"
 * checkbox (checked by default) silences this prompt for the rest of the app
 * session — it resets on restart because the flag lives in a ref, not storage.
 * The checkbox also remembers the user's last choice across prompts: if they
 * uncheck it once, later prompts reopen unchecked.
 */
export function BroadcastDupDialog({
  connectionName,
  existingCount,
  initialDontRemind = true,
  onConfirm,
  onCancel,
}: Props) {
  // Seeded from the user's last choice (session-scoped) so the checkbox state
  // persists between prompts; defaults to checked on the first ever prompt.
  const [dontRemind, setDontRemind] = useState(initialDontRemind);

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--bg-overlay)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 1100,
        backdropFilter: "blur(8px)",
      }}
      onClick={() => onCancel(dontRemind)}
    >
      <div
        className="animate-scale-in"
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "var(--bg-elevated)",
          border: "1px solid var(--border-emphasis)",
          borderRadius: "var(--radius-xl)",
          width: 420,
          maxWidth: "90vw",
          boxShadow: "var(--shadow-xl)",
          overflow: "hidden",
        }}
      >
        <div style={{ padding: "20px 24px" }}>
          <div style={{ display: "flex", alignItems: "flex-start", gap: 12 }}>
            <span style={{ fontSize: 24, lineHeight: 1 }}>⚠️</span>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text-primary)", marginBottom: 8 }}>
                该连接已在广播组中
              </div>
              <div style={{ fontSize: 12.5, color: "var(--text-secondary)", lineHeight: 1.7 }}>
                广播组里已有 <b style={{ color: "var(--text-primary)" }}>{existingCount}</b> 个来自连接
                「<b style={{ color: "var(--text-primary)" }}>{connectionName}</b>」的标签页。
                继续加入后，广播命令会在该连接上执行多次。
              </div>
            </div>
          </div>

          <label
            style={{
              display: "flex",
              alignItems: "center",
              gap: 8,
              marginTop: 18,
              padding: "10px 12px",
              background: "var(--bg-surface)",
              border: "1px solid var(--border-default)",
              borderRadius: "var(--radius-md)",
              cursor: "pointer",
              userSelect: "none",
            }}
          >
            <input
              type="checkbox"
              checked={dontRemind}
              onChange={(e) => setDontRemind(e.target.checked)}
              style={{ width: 15, height: 15, cursor: "pointer", accentColor: "var(--accent-primary)" }}
            />
            <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>
              本次会话不再提醒（重启应用后重置）
            </span>
          </label>
        </div>

        <div
          style={{
            padding: "14px 24px",
            borderTop: "1px solid var(--border-subtle)",
            display: "flex",
            justifyContent: "flex-end",
            gap: 10,
            background: "var(--bg-surface)",
          }}
        >
          <button
            onClick={() => onCancel(dontRemind)}
            style={{
              padding: "9px 18px",
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
            取消
          </button>
          <button
            onClick={() => onConfirm(dontRemind)}
            autoFocus
            style={{
              padding: "9px 22px",
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
            确认加入
          </button>
        </div>
      </div>
    </div>
  );
}
