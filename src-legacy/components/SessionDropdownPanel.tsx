import { useEffect, useRef, useState } from "react";
import type { Tab } from "../api";
import { ConnIcon } from "./ConnIcon";

/** Status → colored dot, shared with TabBar's inline indicator. */
const STATUS_DOT: Record<string, { color: string; title: string }> = {
  connecting: { color: "var(--warning)", title: "连接中" },
  connected: { color: "var(--success)", title: "已连接" },
  disconnected: { color: "var(--error)", title: "已断开" },
  error: { color: "var(--error)", title: "连接失败" },
};

interface Row {
  tab: Tab;
  /** Tail action button label per panel — e.g. "✕" (close) for the session
   * list, "退出" for the broadcast list. Undefined ⇒ no tail button. */
  actionLabel?: string;
  actionTitle?: string;
  onAction?: (tab: Tab) => void;
}

interface Props {
  title: string;
  icon: string;
  rows: Row[];
  activeTabId: string | null;
  /** Screen anchor for the dropdown — the panel is positioned under and
   * right-aligned to this rect. */
  anchorRect: { right: number; top: number };
  onSelect: (tab: Tab) => void;
  onClose: () => void;
  /** Empty-state hint when there are no rows. */
  emptyHint?: string;
  /** When > 0, renders a "一键删除掉线会话" button in the header that calls
   * this to batch-close every offline (disconnected/error) tab. The panel
   * itself stays open so the user sees the list update in place. */
  onCloseDisconnected?: () => void;
  /** How many rows in this panel are offline — gates the button's visibility
   * and feeds its label. */
  disconnectedCount?: number;
  /** Batch action buttons rendered in the header. Each entry is a self-
   * contained button (label + callback). Lets the caller add custom bulk
   * operations (reconnect all, broadcast all, exit all broadcast, etc.)
   * without changing the panel's internals. */
  batchActions?: Array<{
    label: string;
    title?: string;
    onClick: () => void;
  }>;
  /** Per-row secondary action — a second tail button after the main `onAction`.
   * Used for the broadcast toggle (📡) in the session list. */
  rowSecondaryAction?: (tab: Tab) => { label: string; title: string; active: boolean } | undefined;
  /** Callback for the secondary action click. */
  onRowSecondaryAction?: (tab: Tab) => void;
}

/**
 * A floating dropdown listing tab sessions. Shared by the "当前会话" entry
 * (all tabs) and the broadcast badge (broadcast-group tabs). Positioned
 * right-aligned under its trigger button via `anchorRect`.
 *
 * Each row shows the connection icon, the FULL tab name (never truncated —
 * this panel exists precisely because tab names get clipped in the bar),
 * a status dot, and an optional tail action (close / leave-broadcast).
 * Clicking a row switches to that tab and dismisses the panel.
 *
 * Click-outside and Esc both close it.
 */
export function SessionDropdownPanel({
  title,
  icon,
  rows,
  activeTabId,
  anchorRect,
  onSelect,
  onClose,
  emptyHint,
  onCloseDisconnected,
  disconnectedCount = 0,
  batchActions,
  rowSecondaryAction,
  onRowSecondaryAction,
}: Props) {
  const panelRef = useRef<HTMLDivElement>(null);

  // Close on Esc; close on click-outside (the overlay also catches it, but
  // this covers the case where the parent didn't render a full-screen guard).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  const PANEL_WIDTH = 320;
  const GAP = 6;
  // Right-align the panel to the trigger's right edge so it stays under the
  // button that opened it; clamp to the viewport so it can't overflow left.
  const left = Math.max(8, anchorRect.right - PANEL_WIDTH);
  const top = anchorRect.top + GAP;

  return (
    <>
      {/* Full-screen click guard — closes the panel when clicking anywhere
          outside it (including the trigger button again, which toggles off). */}
      <div
        onClick={onClose}
        style={{ position: "fixed", inset: 0, zIndex: 1099 }}
      />
      <div
        ref={panelRef}
        className="animate-scale-in"
        onClick={(e) => e.stopPropagation()}
        style={{
          position: "fixed",
          left,
          top,
          width: PANEL_WIDTH,
          maxHeight: 420,
          zIndex: 1100,
          background: "var(--bg-elevated)",
          border: "1px solid var(--border-emphasis)",
          borderRadius: "var(--radius-lg)",
          boxShadow: "var(--shadow-xl)",
          backdropFilter: "blur(var(--glass-blur))",
          display: "flex",
          flexDirection: "column",
          overflow: "hidden",
          transformOrigin: "top right",
        }}
      >
        {/* Header */}
        <div
          style={{
            padding: "11px 14px",
            borderBottom: "1px solid var(--border-subtle)",
            display: "flex",
            alignItems: "center",
            gap: 8,
            fontSize: 12,
            fontWeight: 600,
            color: "var(--text-secondary)",
          }}
        >
          <span style={{ fontSize: 14 }}>{icon}</span>
          {onCloseDisconnected && disconnectedCount > 0 && (
            <button
              onClick={(e) => {
                e.stopPropagation();
                onCloseDisconnected();
              }}
              title={`批量关闭 ${disconnectedCount} 个掉线的会话`}
              style={{
                fontSize: 11,
                fontWeight: 500,
                color: "var(--error)",
                background: "transparent",
                border: "1px solid var(--error)",
                borderRadius: "var(--radius-md)",
                padding: "3px 9px",
                cursor: "pointer",
                marginRight: 4,
                transition: "all var(--duration-fast) var(--ease-in-out)",
                whiteSpace: "nowrap",
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = "var(--error)";
                e.currentTarget.style.color = "#fff";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "transparent";
                e.currentTarget.style.color = "var(--error)";
              }}
            >
              清理掉线 × {disconnectedCount}
            </button>
          )}
          {batchActions?.map((action, i) => (
            <button
              key={i}
              onClick={(e) => {
                e.stopPropagation();
                action.onClick();
              }}
              title={action.title || action.label}
              style={{
                fontSize: 11,
                fontWeight: 500,
                color: "var(--text-secondary)",
                background: "transparent",
                border: "1px solid var(--border-default)",
                borderRadius: "var(--radius-md)",
                padding: "3px 9px",
                cursor: "pointer",
                marginRight: 4,
                whiteSpace: "nowrap",
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
              {action.label}
            </button>
          ))}
          <span
            style={{
              fontSize: 11,
              color: "var(--text-muted)",
              background: "var(--bg-surface)",
              padding: "1px 8px",
              borderRadius: "var(--radius-full)",
            }}
          >
            {rows.length}
          </span>
        </div>

        {/* List */}
        <div style={{ overflowY: "auto", padding: 6 }}>
          {rows.length === 0 ? (
            <div
              style={{
                padding: "24px 12px",
                textAlign: "center",
                fontSize: 12,
                color: "var(--text-muted)",
                lineHeight: 1.6,
              }}
            >
              {emptyHint || "暂无会话"}
            </div>
          ) : (
            rows.map(({ tab, actionLabel, actionTitle, onAction }) => {
              const isActive = tab.id === activeTabId;
              const dot = STATUS_DOT[tab.status || "connected"];
              return (
                <div
                  key={tab.id}
                  onClick={() => {
                    onSelect(tab);
                    onClose();
                  }}
                  title={tab.name}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "8px 10px",
                    borderRadius: "var(--radius-md)",
                    cursor: "pointer",
                    background: isActive ? "var(--accent-primary-muted)" : "transparent",
                    transition: "background var(--duration-fast) var(--ease-in-out)",
                  }}
                  onMouseEnter={(e) => {
                    if (!isActive) e.currentTarget.style.background = "var(--bg-surface-hover)";
                  }}
                  onMouseLeave={(e) => {
                    if (!isActive) e.currentTarget.style.background = "transparent";
                  }}
                >
                  <ConnIcon
                    connType={tab.connType || "ssh"}
                    size={15}
                    style={{ opacity: 0.85, flexShrink: 0, color: isActive ? "var(--accent-primary)" : undefined }}
                  />
                  <span
                    style={{
                      flex: 1,
                      minWidth: 0,
                      fontSize: 12.5,
                      color: isActive ? "var(--accent-primary)" : "var(--text-primary)",
                      fontWeight: isActive ? 600 : 400,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {tab.name}
                  </span>
                  {/* Status dot */}
                  <span
                    title={dot.title}
                    style={{
                      width: 7,
                      height: 7,
                      borderRadius: "var(--radius-full)",
                      background: dot.color,
                      flexShrink: 0,
                      boxShadow: tab.status === "connecting" ? `0 0 5px ${dot.color}` : "none",
                    }}
                  />
                  {/* Secondary per-row action (e.g. broadcast toggle) */}
                  {rowSecondaryAction && onRowSecondaryAction && (() => {
                    const sa = rowSecondaryAction(tab);
                    if (!sa) return null;
                    return (
                      <span
                        onClick={(e) => {
                          e.stopPropagation();
                          onRowSecondaryAction(tab);
                        }}
                        title={sa.title}
                        style={{
                          flexShrink: 0,
                          fontSize: 12,
                          cursor: "pointer",
                          padding: "1px 4px",
                          color: sa.active ? "var(--success)" : "var(--text-muted)",
                          opacity: sa.active ? 1 : 0.5,
                          transition: "opacity var(--duration-fast) var(--ease-in-out)",
                        }}
                      >
                        📡
                      </span>
                    );
                  })()}
                  {onAction && (
                    <span
                      onClick={(e) => {
                        e.stopPropagation();
                        onAction(tab);
                      }}
                      title={actionTitle}
                      style={{
                        flexShrink: 0,
                        fontSize: 11,
                        color: "var(--text-muted)",
                        cursor: "pointer",
                        padding: "2px 5px",
                        borderRadius: "var(--radius-sm)",
                        lineHeight: 1,
                        transition: "all var(--duration-fast) var(--ease-in-out)",
                        minWidth: 18,
                        textAlign: "center" as const,
                      }}
                      onMouseEnter={(e) => {
                        e.currentTarget.style.color = "var(--error)";
                        e.currentTarget.style.background = "var(--error-muted)";
                      }}
                      onMouseLeave={(e) => {
                        e.currentTarget.style.color = "var(--text-muted)";
                        e.currentTarget.style.background = "transparent";
                      }}
                    >
                      {actionLabel}
                    </span>
                  )}
                </div>
              );
            })
          )}
        </div>
      </div>
    </>
  );
}

/** Small helper: read a DOM element's anchor rect (right + top) for panel
 * positioning. Returns null if the element isn't mounted. */
export function readAnchorRect(el: HTMLElement | null): { right: number; top: number } | null {
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { right: r.right, top: r.bottom };
}

/** Reusable toggle button for the two dropdown triggers (当前会话 / 广播).
 * Renders the icon, label and count; the parent wires up onClick + tracks the
 * ref for anchoring. */
export function DropdownTrigger({
  icon,
  label,
  count,
  active,
  title,
  triggerRef,
  onClick,
  accent,
}: {
  icon: string;
  label: string;
  count: number;
  active: boolean;
  title: string;
  triggerRef: React.RefObject<HTMLButtonElement>;
  onClick: () => void;
  accent?: boolean;
}) {
  const [hover, setHover] = useState(false);
  const baseBg = active
    ? "var(--accent-primary-muted)"
    : accent
      ? "var(--accent-primary-muted)"
      : hover
        ? "var(--bg-surface-hover)"
        : "transparent";
  const color = active || accent ? "var(--accent-primary)" : hover ? "var(--text-primary)" : "var(--text-secondary)";
  return (
    <button
      ref={triggerRef}
      onClick={onClick}
      title={title}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        height: "100%",
        padding: "0 14px",
        display: "flex",
        alignItems: "center",
        gap: 8,
        borderLeft: "1px solid var(--border-default)",
        background: baseBg,
        color,
        fontSize: 11,
        fontWeight: 600,
        cursor: "pointer",
        transition: "background var(--duration-fast) var(--ease-in-out), color var(--duration-fast) var(--ease-in-out)",
        whiteSpace: "nowrap",
      }}
    >
      <span style={{ fontSize: 14 }}>{icon}</span>
      <span>{label}</span>
      <span
        style={{
          fontSize: 11,
          background: active || accent ? "var(--accent-primary)" : "var(--bg-surface-active)",
          color: active || accent ? "#fff" : "var(--text-secondary)",
          padding: "0 7px",
          borderRadius: "var(--radius-full)",
          minWidth: 20,
          textAlign: "center" as const,
          lineHeight: "16px",
        }}
      >
        {count}
      </span>
    </button>
  );
}
