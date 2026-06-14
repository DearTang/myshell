import type { Tab } from "../api";

interface Props {
  tabs: Tab[];
  activeTabId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  /** Tab IDs that are members of the broadcast group. The 📡 button on each
   * tab reflects membership; clicking toggles it. */
  broadcastIds: Set<string>;
  onToggleBroadcast: (id: string) => void;
}

export function TabBar({
  tabs,
  activeTabId,
  onSelect,
  onClose,
  broadcastIds,
  onToggleBroadcast,
}: Props) {
  const broadcastCount = broadcastIds.size;
  return (
    <div
      style={{
        height: 36,
        background: "var(--bg-panel)",
        borderBottom: "1px solid var(--border)",
        display: "flex",
        alignItems: "center",
        overflow: "hidden",
      }}
    >
      <div style={{ display: "flex", flex: 1, overflow: "auto" }}>
        {tabs.map((tab) => {
          const isBroadcast = broadcastIds.has(tab.id);
          // Broadcast only makes sense on SSH terminals — show the toggle
          // disabled-looking for SFTP/FTP tabs.
          const canBroadcast = tab.type === "terminal" && tab.connType === "ssh";
          return (
            <div
              key={tab.id}
              onClick={() => onSelect(tab.id)}
              style={{
                height: "100%",
                display: "flex",
                alignItems: "center",
                gap: 6,
                padding: "0 12px",
                fontSize: 12,
                cursor: "pointer",
                borderRight: "1px solid var(--border)",
                background: tab.id === activeTabId ? "var(--bg-dark)" : "transparent",
                color: tab.id === activeTabId ? "var(--text-primary)" : "var(--text-secondary)",
                whiteSpace: "nowrap",
                minWidth: 0,
                position: "relative",
              }}
            >
              <span
                style={{
                  fontSize: 13,
                  opacity: 0.85,
                }}
              >
                {tab.connType === "ftp" ? "📤" : tab.connType === "sftp" ? "📁" : "🖥"}
              </span>
              <span
                style={{
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  color: isBroadcast ? "var(--success)" : undefined,
                }}
              >
                {tab.name}
              </span>
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
                    opacity: 1,
                    cursor: "pointer",
                    padding: "0 2px",
                    color: isBroadcast ? "var(--success)" : "var(--error)",
                    transition: "opacity 0.15s, color 0.15s",
                  }}
                >
                  📡
                </span>
              )}
              <span
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(tab.id);
                }}
                style={{
                  marginLeft: 4,
                  fontSize: 14,
                  opacity: 0.4,
                  cursor: "pointer",
                  lineHeight: 1,
                  padding: "0 2px",
                }}
                onMouseEnter={(e) => (e.currentTarget.style.opacity = "1")}
                onMouseLeave={(e) => (e.currentTarget.style.opacity = "0.4")}
              >
                ×
              </span>
            </div>
          );
        })}
      </div>
      {broadcastCount > 0 && (
        <div
          title={`${broadcastCount} 个 tab 在广播组中：输入将同步到所有成员`}
          style={{
            padding: "0 12px",
            height: "100%",
            display: "flex",
            alignItems: "center",
            gap: 6,
            borderLeft: "1px solid var(--border)",
            background: "rgba(137,180,250,0.12)",
            color: "var(--accent)",
            fontSize: 11,
            fontWeight: 600,
          }}
        >
          <span style={{ fontSize: 13 }}>📡</span>
          <span>广播 × {broadcastCount}</span>
        </div>
      )}
    </div>
  );
}
