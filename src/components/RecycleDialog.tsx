import { useCallback, useEffect, useState } from "react";
import { ConnIcon } from "./ConnIcon";
import type { DeletedConnection, ConnType } from "../api";
import {
  getDeletedConnections,
  restoreConnection,
  purgeConnection,
  purgeAllDeletedConnections,
} from "../api";

interface Props {
  /** Called after any change (restore/purge) so the caller refreshes the live
   * connection list in the sidebar. */
  onChanged: () => void;
  onClose: () => void;
}

/**
 * Recycle bin for soft-deleted connections. Lists every deleted connection
 * (newest-deleted first), each with a relative "x 分钟前" timestamp, and lets
 * the user restore it (back to the sidebar) or permanently purge it. A
 * "清空回收站" button empties everything.
 *
 * The backend enforces a 30-row cap (oldest overflow is hard-purged on each
 * delete), surfaced as a hint at the bottom. Connections deleted via folder
 * cascade land here too and are individually restoreable.
 *
 * Loaded fresh each time the dialog opens (no stale state across opens). The
 * list re-fetches after each restore/purge so counts stay accurate.
 */
export function RecycleDialog({ onChanged, onClose }: Props) {
  const [items, setItems] = useState<DeletedConnection[]>([]);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      const list = await getDeletedConnections();
      setItems(list);
    } catch (e) {
      console.error("[recycle] load failed", e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  async function handleRestore(id: string) {
    setBusyId(id);
    try {
      await restoreConnection(id);
      onChanged();
      await reload();
    } catch (e) {
      window.alert(`找回失败: ${e}`);
    } finally {
      setBusyId(null);
    }
  }

  async function handlePurgeOne(id: string, name: string) {
    if (!window.confirm(`彻底删除「${name}」？此操作不可恢复。`)) return;
    setBusyId(id);
    try {
      await purgeConnection(id);
      onChanged();
      await reload();
    } catch (e) {
      window.alert(`删除失败: ${e}`);
    } finally {
      setBusyId(null);
    }
  }

  async function handlePurgeAll() {
    if (items.length === 0) return;
    if (!window.confirm(`确定清空回收站？将彻底删除 ${items.length} 个连接，此操作不可恢复。`))
      return;
    setBusyId("__all__");
    try {
      await purgeAllDeletedConnections();
      onChanged();
      await reload();
    } catch (e) {
      window.alert(`清空失败: ${e}`);
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--bg-overlay)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 2100,
        backdropFilter: "blur(8px)",
      }}
      onClick={onClose}
    >
      <div
        className="animate-scale-in"
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "var(--bg-elevated)",
          border: "1px solid var(--border-emphasis)",
          borderRadius: "var(--radius-xl)",
          width: 480,
          maxWidth: "92vw",
          maxHeight: "80vh",
          display: "flex",
          flexDirection: "column",
          boxShadow: "var(--shadow-xl)",
          overflow: "hidden",
        }}
      >
        {/* Header */}
        <div
          style={{
            padding: "16px 20px",
            borderBottom: "1px solid var(--border-subtle)",
            display: "flex",
            alignItems: "center",
            gap: 10,
          }}
        >
          <span style={{ fontSize: 18 }}>🗑️</span>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text-primary)" }}>
              找回连接
            </div>
            <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 2 }}>
              {loading ? "加载中…" : `${items.length} 个已删除的连接`}
            </div>
          </div>
          {items.length > 0 && (
            <button
              onClick={handlePurgeAll}
              disabled={busyId === "__all__"}
              style={{
                padding: "7px 14px",
                fontSize: 12,
                color: "var(--error)",
                background: "transparent",
                border: "1px solid var(--error)",
                borderRadius: "var(--radius-md)",
                cursor: busyId === "__all__" ? "wait" : "pointer",
                opacity: busyId === "__all__" ? 0.6 : 1,
                transition: "all var(--duration-fast) var(--ease-in-out)",
              }}
              onMouseEnter={(e) => {
                if (busyId !== "__all__") {
                  e.currentTarget.style.background = "var(--error)";
                  e.currentTarget.style.color = "#fff";
                }
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "transparent";
                e.currentTarget.style.color = "var(--error)";
              }}
            >
              {busyId === "__all__" ? "清空中…" : "清空回收站"}
            </button>
          )}
          <button
            onClick={onClose}
            style={{
              background: "transparent",
              border: "none",
              color: "var(--text-secondary)",
              fontSize: 18,
              cursor: "pointer",
              padding: "2px 6px",
              lineHeight: 1,
            }}
            title="关闭"
          >
            ×
          </button>
        </div>

        {/* List */}
        <div style={{ flex: 1, overflowY: "auto", padding: 8 }}>
          {!loading && items.length === 0 ? (
            <div
              style={{
                padding: "48px 20px",
                textAlign: "center",
                color: "var(--text-muted)",
                fontSize: 13,
                lineHeight: 1.7,
              }}
            >
              <div style={{ fontSize: 36, opacity: 0.3, marginBottom: 14 }}>🗑️</div>
              回收站为空
              <div style={{ marginTop: 6, fontSize: 11, color: "var(--text-tertiary)" }}>
                删除的连接会暂存在这里
              </div>
            </div>
          ) : (
            items.map((item) => {
              const connType = (item.conn_type as ConnType) || "ssh";
              const isBusy = busyId === item.id;
              return (
                <div
                  key={item.id}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "9px 10px",
                    borderRadius: "var(--radius-md)",
                    transition: "background var(--duration-fast) var(--ease-in-out)",
                  }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background = "var(--bg-surface-hover)";
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = "transparent";
                  }}
                >
                  <ConnIcon connType={connType} size={16} style={{ opacity: 0.7, flexShrink: 0 }} />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div
                      title={item.name}
                      style={{
                        fontSize: 13,
                        color: "var(--text-primary)",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {item.name}
                    </div>
                    <div style={{ fontSize: 11, color: "var(--text-muted)" }}>
                      {item.host ? `${item.host}:${item.port}` : "—"}
                      <span style={{ margin: "0 6px", opacity: 0.5 }}>·</span>
                      {relativeTime(item.deletedAt)}
                    </div>
                  </div>
                  <button
                    onClick={() => handleRestore(item.id)}
                    disabled={isBusy}
                    title="找回该连接"
                    style={{
                      flexShrink: 0,
                      padding: "5px 12px",
                      fontSize: 11,
                      fontWeight: 600,
                      color: "#fff",
                      background: "var(--accent-primary)",
                      border: "none",
                      borderRadius: "var(--radius-md)",
                      cursor: isBusy ? "wait" : "pointer",
                      opacity: isBusy ? 0.6 : 1,
                      transition: "background var(--duration-fast) var(--ease-in-out)",
                    }}
                    onMouseEnter={(e) => {
                      if (!isBusy) e.currentTarget.style.background = "var(--accent-primary-hover)";
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.background = "var(--accent-primary)";
                    }}
                  >
                    {isBusy ? "…" : "找回"}
                  </button>
                  <button
                    onClick={() => handlePurgeOne(item.id, item.name)}
                    disabled={isBusy}
                    title="彻底删除（不可恢复）"
                    style={{
                      flexShrink: 0,
                      padding: "5px 10px",
                      fontSize: 11,
                      color: "var(--text-muted)",
                      background: "transparent",
                      border: "1px solid var(--border-default)",
                      borderRadius: "var(--radius-md)",
                      cursor: isBusy ? "wait" : "pointer",
                      opacity: isBusy ? 0.5 : 1,
                      transition: "all var(--duration-fast) var(--ease-in-out)",
                    }}
                    onMouseEnter={(e) => {
                      if (!isBusy) {
                        e.currentTarget.style.color = "var(--error)";
                        e.currentTarget.style.borderColor = "var(--error)";
                      }
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.color = "var(--text-muted)";
                      e.currentTarget.style.borderColor = "var(--border-default)";
                    }}
                  >
                    删除
                  </button>
                </div>
              );
            })
          )}
        </div>

        {/* Footer hint */}
        <div
          style={{
            padding: "10px 20px",
            borderTop: "1px solid var(--border-subtle)",
            fontSize: 11,
            color: "var(--text-muted)",
            textAlign: "center",
            background: "var(--bg-surface)",
          }}
        >
          最多保留 30 条记录，超出将自动彻底删除最早的连接
        </div>
      </div>
    </div>
  );
}

/** Format an ISO timestamp as a Chinese relative-time string ("3 分钟前"). */
function relativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const diff = Date.now() - then;
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return "刚刚";
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min} 分钟前`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr} 小时前`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day} 天前`;
  const mon = Math.floor(day / 30);
  if (mon < 12) return `${mon} 个月前`;
  return `${Math.floor(mon / 12)} 年前`;
}
