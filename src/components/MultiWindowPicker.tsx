import { useState, useEffect } from "react";
import type { Tab } from "../api";

interface Props {
  tabs: Tab[];
  onConfirm: (tabIds: string[]) => void;
  onClose: () => void;
}

export function MultiWindowPicker({ tabs, onConfirm, onClose }: Props) {
  // Candidate sessions: only connected SSH terminal tabs.
  const candidates = tabs.filter(
    (t) => t.type === "terminal" && t.status === "connected" && t.connType === "ssh"
  );

  const [selected, setSelected] = useState<Set<string>>(new Set(candidates.map((t) => t.id)));

  // Keep selection in sync if tabs change while the picker is open.
  useEffect(() => {
    setSelected(new Set(candidates.map((t) => t.id)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const toggle = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.5)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 10000,
        backdropFilter: "blur(2px)",
      }}
      onClick={onClose}
    >
      <div
        style={{
          background: "var(--bg-base)",
          border: "1px solid var(--border-default)",
          borderRadius: "var(--radius-xl)",
          minWidth: 360,
          maxWidth: 480,
          maxHeight: "70vh",
          display: "flex",
          flexDirection: "column",
          boxShadow: "0 12px 48px rgba(0,0,0,0.4)",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div
          style={{
            padding: "16px 20px",
            borderBottom: "1px solid var(--border-subtle)",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <div style={{ fontSize: 14, fontWeight: 600, color: "var(--text-primary)" }}>
            🪟 多窗口会话选择
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span style={{ fontSize: 12, color: "var(--text-tertiary)" }}>
              已选 {selected.size} / {candidates.length}
            </span>
            <button
              onClick={() => {
                if (selected.size === candidates.length) {
                  setSelected(new Set());
                } else {
                  setSelected(new Set(candidates.map((t) => t.id)));
                }
              }}
              style={{
                padding: "2px 10px",
                background: "var(--bg-input)",
                color: "var(--text-secondary)",
                border: "1px solid var(--border-default)",
                borderRadius: "var(--radius-sm)",
                fontSize: 11,
                cursor: "pointer",
              }}
            >
              {selected.size === candidates.length ? "取消全选" : "全选"}
            </button>
          </div>
        </div>

        {/* List */}
        <div style={{ flex: 1, overflowY: "auto", padding: "8px 0" }}>
          {candidates.length === 0 ? (
            <div
              style={{
                padding: "32px 20px",
                textAlign: "center",
                color: "var(--text-tertiary)",
                fontSize: 13,
                lineHeight: 1.6,
              }}
            >
              没有已连接的 SSH 终端会话。
              <br />
              请先连接至少一个会话。
            </div>
          ) : (
            candidates.map((tab) => {
              const checked = selected.has(tab.id);
              return (
                <label
                  key={tab.id}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "10px 20px",
                    cursor: "pointer",
                    transition: "background var(--duration-fast) var(--ease-in-out)",
                    background: checked ? "var(--accent-primary-muted)" : "transparent",
                  }}
                  onMouseEnter={(e) => {
                    if (!checked)
                      e.currentTarget.style.background = "var(--bg-surface-hover)";
                  }}
                  onMouseLeave={(e) => {
                    if (!checked) e.currentTarget.style.background = "transparent";
                  }}
                >
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={() => toggle(tab.id)}
                    style={{ accentColor: "var(--accent-primary)", width: 16, height: 16 }}
                  />
                  <span style={{ fontSize: 13, color: "var(--text-primary)" }}>{tab.name}</span>
                </label>
              );
            })
          )}
        </div>

        {/* Footer */}
        <div
          style={{
            padding: "12px 20px",
            borderTop: "1px solid var(--border-subtle)",
            display: "flex",
            justifyContent: "flex-end",
            gap: 10,
            flexShrink: 0,
          }}
        >
          <button
            onClick={onClose}
            style={{
              padding: "8px 20px",
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
            onClick={() => onConfirm(Array.from(selected))}
            disabled={selected.size === 0}
            style={{
              padding: "8px 24px",
              background: "var(--accent-primary)",
              color: "white",
              border: "none",
              borderRadius: "var(--radius-md)",
              fontSize: 13,
              fontWeight: 600,
              cursor: selected.size === 0 ? "not-allowed" : "pointer",
              opacity: selected.size === 0 ? 0.5 : 1,
              transition: "all var(--duration-fast) var(--ease-in-out)",
            }}
            onMouseEnter={(e) => {
              if (selected.size > 0) {
                e.currentTarget.style.background = "var(--accent-primary-hover)";
                e.currentTarget.style.transform = "translateY(-1px)";
              }
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "var(--accent-primary)";
              e.currentTarget.style.transform = "translateY(0)";
            }}
          >
            开始多窗口
          </button>
        </div>
      </div>
    </div>
  );
}
