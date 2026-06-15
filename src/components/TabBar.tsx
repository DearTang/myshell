import type { Tab } from "../api";

interface Props {
  tabs: Tab[];
  activeTabId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onReconnect: (id: string) => void;
  broadcastIds: Set<string>;
  onToggleBroadcast: (id: string) => void;
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
}: Props) {
  const broadcastCount = broadcastIds.size;

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
              <span
                style={{
                  fontSize: 14,
                  opacity: 0.85,
                  color: tab.connType === "ftp"
                    ? "var(--warning)"
                    : tab.connType === "sftp"
                      ? "var(--success)"
                      : "var(--accent-primary)",
                }}
              >
                {tab.connType === "ftp" ? "📤" : tab.connType === "sftp" ? "📁" : "🖥️"}
              </span>

              {/* Tab Name */}
              <span
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

      {/* Broadcast Indicator */}
      {broadcastCount > 0 && (
        <div
          title={`${broadcastCount} 个 tab 在广播组中：输入将同步到所有成员`}
          className="animate-slide-in-right"
          style={{
            padding: "0 14px",
            height: "100%",
            display: "flex",
            alignItems: "center",
            gap: 8,
            borderLeft: "1px solid var(--border-default)",
            background: "var(--accent-primary-muted)",
            color: "var(--accent-primary)",
            fontSize: 11,
            fontWeight: 600,
          }}
        >
          <span style={{ fontSize: 14 }}>📡</span>
          <span>广播 × {broadcastCount}</span>
        </div>
      )}
    </div>
  );
}
