import { useEffect, useRef, useState } from "react";
import type { Tab } from "../api";
import { ConnIcon } from "./ConnIcon";
import { SessionDropdownPanel, DropdownTrigger, readAnchorRect } from "./SessionDropdownPanel";

interface Props {
  tabs: Tab[];
  activeTabId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onReconnect: (id: string) => void;
  broadcastIds: Set<string>;
  onToggleBroadcast: (id: string) => void;
  /** Reconnect all disconnected/error tabs at once. */
  onReconnectAll?: () => void;
  /** Add all connected SSH terminal tabs to the broadcast group. */
  onBroadcastAll?: () => void;
  /** Remove all tabs from the broadcast group. */
  onExitAllBroadcast?: () => void;
  /** Batch-close every offline (disconnected/error) tab. Powers the
   * "清理掉线" button in the dropdown panels. */
  onCloseDisconnected: () => void;
}

// Status indicator colors and icons
const STATUS_CONFIG = {
  connecting: { color: "var(--warning)", icon: "⏳", label: "连接中", fontSize: 14 },
  connected: { color: "var(--success)", icon: "●", label: "已连接", fontSize: 14 },
  disconnected: { color: "var(--error)", icon: "●", label: "已断开", fontSize: 14 },
  error: { color: "var(--error)", icon: "✕", label: "连接失败", fontSize: 12 },
};

export function TabBar({
  tabs,
  activeTabId,
  onSelect,
  onClose,
  onReconnect,
  broadcastIds,
  onToggleBroadcast,
  onReconnectAll,
  onBroadcastAll,
  onExitAllBroadcast,
  onCloseDisconnected,
}: Props) {
  const broadcastCount = broadcastIds.size;

  // Which dropdown panel is open: "sessions" (all tabs), "broadcast"
  // (broadcast-group tabs), or null. Only one at a time; clicking the same
  // trigger again closes it.
  const [openPanel, setOpenPanel] = useState<"sessions" | "broadcast" | null>(null);
  const sessionsTriggerRef = useRef<HTMLButtonElement>(null);
  const broadcastTriggerRef = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<{ right: number; top: number } | null>(null);

  // Broadcast-group tabs (already connected SSH terminals), in bar order, for
  // the broadcast dropdown.
  const broadcastTabs = tabs.filter((t) => broadcastIds.has(t.id));

  // Auto-close the broadcast panel only when the group becomes empty (the
  // trigger button disappears, so the panel would be orphaned). We do NOT
  // close on every tab-count change: a user closing tabs from the sessions
  // panel expects it to stay open so they can close several in a row.
  useEffect(() => {
    if (openPanel === "broadcast" && broadcastCount === 0) {
      setOpenPanel(null);
    }
  }, [openPanel, broadcastCount]);

  function togglePanel(which: "sessions" | "broadcast", triggerEl: HTMLElement | null) {
    if (openPanel === which) {
      setOpenPanel(null);
      return;
    }
    const rect = readAnchorRect(triggerEl);
    if (rect) setAnchor(rect);
    setOpenPanel(which);
  }

  return (
    <div
      style={{
        height: 40,
        background: "var(--bg-elevated)",
        borderBottom: "1px solid var(--border-subtle)",
        display: "flex",
        alignItems: "center",
        overflow: "hidden",
      }}
    >
      <div style={{ display: "flex", flex: 1, overflow: "auto", height: "100%" }}>
        {tabs.map((tab) => {
          const isBroadcast = broadcastIds.has(tab.id);
          const canBroadcast = tab.type === "terminal" && tab.connType === "ssh" && tab.status === "connected";
          const isActive = tab.id === activeTabId;
          const status = tab.status || "connected";
          const statusConfig = STATUS_CONFIG[status];

          return (
            <div
              key={tab.id}
              onClick={() => onSelect(tab.id)}
              style={{
                height: "100%",
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "0 14px",
                fontSize: 12,
                cursor: "pointer",
                borderRight: "1px solid var(--border-subtle)",
                background: isActive
                  ? "var(--bg-base)"
                  : "transparent",
                color: isActive
                  ? "var(--text-primary)"
                  : "var(--text-secondary)",
                whiteSpace: "nowrap",
                minWidth: 0,
                position: "relative",
                transition: "background var(--duration-fast) var(--ease-in-out), color var(--duration-fast) var(--ease-in-out)",
              }}
              onMouseEnter={(e) => {
                if (!isActive) {
                  e.currentTarget.style.background = "var(--bg-surface-hover)";
                }
              }}
              onMouseLeave={(e) => {
                if (!isActive) {
                  e.currentTarget.style.background = "transparent";
                }
              }}
            >
              {/* Active Indicator */}
              {isActive && (
                <div
                  style={{
                    position: "absolute",
                    bottom: 0,
                    left: 0,
                    right: 0,
                    height: 2,
                    background: status === "error" ? "var(--error)" : status === "disconnected" ? "var(--text-muted)" : "var(--accent-primary)",
                    borderRadius: "2px 2px 0 0",
                  }}
                />
              )}

              {/* Connection Type Icon */}
              <ConnIcon
                connType={tab.connType || "ssh"}
                size={14}
                style={{ opacity: 0.85, color: "inherit" }}
              />

              {/* Tab Name */}
              <span
                title={tab.name}
                style={{
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  color: isBroadcast ? "var(--success)" : status === "error" ? "var(--error)" : undefined,
                  fontWeight: isActive ? 500 : 400,
                }}
              >
                {tab.name}
              </span>

              {/* Status Indicator - always show for visibility */}
              <span
                title={statusConfig.label}
                style={{
                  fontSize: statusConfig.fontSize,
                  color: statusConfig.color,
                  display: "flex",
                  alignItems: "center",
                  gap: 2,
                }}
              >
                {status === "connecting" && (
                  <span style={{ animation: "spin 1s linear infinite", fontSize: 14 }}>
                    {statusConfig.icon}
                  </span>
                )}
                {status !== "connecting" && statusConfig.icon}
              </span>

              {/* Broadcast Toggle */}
              {canBroadcast && (
                <span
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggleBroadcast(tab.id);
                  }}
                  title={
                    isBroadcast
                      ? `广播组中（共 ${broadcastCount} 个 tab）— 点击退出`
                      : "加入广播组"
                  }
                  style={{
                    fontSize: 12,
                    opacity: isBroadcast ? 1 : 0.4,
                    cursor: "pointer",
                    padding: "2px 4px",
                    color: isBroadcast ? "var(--success)" : "var(--text-muted)",
                    transition: "all var(--duration-fast) var(--ease-in-out)",
                    borderRadius: "var(--radius-sm)",
                  }}
                  onMouseEnter={(e) => {
                    if (!isBroadcast) {
                      e.currentTarget.style.opacity = "1";
                      e.currentTarget.style.background = "var(--bg-surface-active)";
                    }
                  }}
                  onMouseLeave={(e) => {
                    if (!isBroadcast) {
                      e.currentTarget.style.opacity = "0.4";
                      e.currentTarget.style.background = "transparent";
                    }
                  }}
                >
                  📡
                </span>
              )}

              {/* Close Button */}
              <span
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(tab.id);
                }}
                style={{
                  marginLeft: 2,
                  fontSize: 14,
                  opacity: 0.35,
                  cursor: "pointer",
                  lineHeight: 1,
                  padding: "2px 4px",
                  borderRadius: "var(--radius-sm)",
                  transition: "all var(--duration-fast) var(--ease-in-out)",
                }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.opacity = "1";
                  e.currentTarget.style.background = "var(--error-muted)";
                  e.currentTarget.style.color = "var(--error)";
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.opacity = "0.35";
                  e.currentTarget.style.background = "transparent";
                  e.currentTarget.style.color = "inherit";
                }}
              >
                ✕
              </span>
            </div>
          );
        })}
      </div>

      {/* Right-side dropdown triggers.
          - "当前会话": lists ALL tabs (full names, never clipped) with quick
            switch + close. Solves the "too many tabs, names cut off" problem.
          - "广播": lists broadcast-group tabs; each row can switch to it or
            leave the broadcast group. Only shown when a broadcast is active. */}

      {/* All-sessions trigger — always visible so the full tab list is one
          click away regardless of how many tabs overflow the bar. */}
      <DropdownTrigger
        icon="📑"
        label="当前会话"
        count={tabs.length}
        active={openPanel === "sessions"}
        title="查看/切换/关闭所有标签页"
        triggerRef={sessionsTriggerRef}
        onClick={() => togglePanel("sessions", sessionsTriggerRef.current)}
      />

      {/* Broadcast trigger — replaces the old static badge; now clickable to
          open the broadcast-group panel. */}
      {broadcastCount > 0 && (
        <DropdownTrigger
          icon="📡"
          label="广播"
          count={broadcastCount}
          active={openPanel === "broadcast"}
          title={`${broadcastCount} 个标签页在广播组中，点击查看/管理`}
          triggerRef={broadcastTriggerRef}
          onClick={() => togglePanel("broadcast", broadcastTriggerRef.current)}
          accent
        />
      )}

      {/* Dropdown panels */}
      {openPanel === "sessions" && anchor && (
        <SessionDropdownPanel
          title="当前会话"
          icon="📑"
          anchorRect={anchor}
          activeTabId={activeTabId}
          onSelect={(t) => onSelect(t.id)}
          onClose={() => setOpenPanel(null)}
          emptyHint="暂无打开的标签页"
          onCloseDisconnected={onCloseDisconnected}
          disconnectedCount={tabs.filter(
            (t) => t.status === "disconnected" || t.status === "error"
          ).length}
          rows={tabs.map((t) => ({
            tab: t,
            actionLabel: "✕",
            actionTitle: "关闭标签页",
            onAction: (tab) => onClose(tab.id),
          }))}
        batchActions={[
          ...(onReconnectAll && tabs.some((t) => t.status === "disconnected" || t.status === "error")
            ? [{ label: "全部重连", title: "重连所有掉线的会话", onClick: onReconnectAll }]
            : []),
          ...(onBroadcastAll && tabs.some((t) => t.type === "terminal" && t.connType === "ssh" && t.status === "connected")
            ? [{ label: "全部广播", title: "将所有已连接的 SSH 会话加入广播组", onClick: onBroadcastAll }]
            : []),
        ]}
        rowSecondaryAction={(tab) => {
          if (tab.type !== "terminal" || tab.connType !== "ssh" || tab.status !== "connected") return undefined;
          return { label: "📡", title: broadcastIds.has(tab.id) ? "退出广播" : "加入广播", active: broadcastIds.has(tab.id) };
        }}
        onRowSecondaryAction={(tab) => onToggleBroadcast(tab.id)}
        />
      )}
      {openPanel === "broadcast" && anchor && (
        <SessionDropdownPanel
          title="广播组成员"
          icon="📡"
          anchorRect={anchor}
          activeTabId={activeTabId}
          onSelect={(t) => onSelect(t.id)}
          onClose={() => setOpenPanel(null)}
          emptyHint="广播组为空"
          onCloseDisconnected={onCloseDisconnected}
          disconnectedCount={broadcastTabs.filter(
            (t) => t.status === "disconnected" || t.status === "error"
          ).length}
          rows={broadcastTabs.map((t) => ({
            tab: t,
            actionLabel: "退出",
            actionTitle: "退出广播组",
            onAction: (tab) => onToggleBroadcast(tab.id),
          }))}
        batchActions={onExitAllBroadcast ? [{ label: "退出全部广播", title: "移除所有广播组成员", onClick: onExitAllBroadcast }] : []}
        />
      )}
    </div>
  );
}
