import { useState, useEffect, useCallback } from "react";
import type { ReactNode } from "react";
import {
  listQuickCommands,
  addQuickCommand,
  updateQuickCommand,
  updateQuickCommandOrder,
  deleteQuickCommand,
} from "../api";
import type { ConnectionConfig, QuickCommandItem } from "../api";

// ============ Quick Commands Management Panel ============
//
// Single panel for managing both global and per-server quick commands. The
// scope selector at the top switches between "global" (null connectionId,
// available on every server) and any specific connection's per-server
// commands. Reuses the same `var(--xxx)` styling language as SettingsPanel.

interface Props {
  onClose: () => void;
  connections: ConnectionConfig[];
  /** Preset scope on open: null = global, a connection id = that server. */
  initialConnectionId: string | null;
  /** The currently-active terminal tab's connection (highlighted in the
   * scope dropdown). May differ from initialConnectionId. */
  activeConnectionId: string | null;
}

const GLOBAL_VALUE = "__global__";

function scopeToValue(scope: string | null): string {
  return scope ?? GLOBAL_VALUE;
}

function valueToScope(value: string): string | null {
  return value === GLOBAL_VALUE ? null : value;
}

export function QuickCommandsPanel({
  onClose,
  connections,
  initialConnectionId,
  activeConnectionId,
}: Props) {
  const [scope, setScope] = useState<string | null>(initialConnectionId ?? null);
  const [items, setItems] = useState<QuickCommandItem[]>([]);
  const [loading, setLoading] = useState(false);
  // Editor state: null = list mode; object = add/edit form open.
  const [editing, setEditing] = useState<{
    id: number | null;
    label: string;
    command: string;
  } | null>(null);
  const [saving, setSaving] = useState(false);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const list = await listQuickCommands(scope);
      setItems(list);
    } catch {
      // Silently ignore — the list just stays stale.
    } finally {
      setLoading(false);
    }
  }, [scope]);

  useEffect(() => {
    reload();
  }, [reload]);

  // If the panel is scoped to a connection that no longer exists (deleted),
  // fall back to global so the user isn't stuck on an empty stale scope.
  useEffect(() => {
    if (scope && !connections.some((c) => c.id === scope)) {
      setScope(null);
    }
  }, [connections, scope]);

  function handleScopeChange(value: string) {
    setScope(valueToScope(value));
    setEditing(null);
  }

  function startAdd() {
    setEditing({ id: null, label: "", command: "" });
  }

  function startEdit(item: QuickCommandItem) {
    setEditing({ id: item.id, label: item.label, command: item.command });
  }

  async function handleSave() {
    if (!editing) return;
    const label = editing.label.trim();
    const command = editing.command.trim();
    if (!label || !command) return;
    setSaving(true);
    try {
      if (editing.id === null) {
        await addQuickCommand(scope, label, command);
      } else {
        await updateQuickCommand(editing.id, label, command);
      }
      setEditing(null);
      await reload();
    } catch (e) {
      alert(`保存失败: ${String(e)}`);
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(id: number) {
    if (!window.confirm("删除该快捷命令？")) return;
    await deleteQuickCommand(id);
    await reload();
  }

  /** Swap sort_order with the adjacent item (items are pre-sorted by it). */
  async function handleMove(index: number, direction: -1 | 1) {
    const current = items[index];
    const target = items[index + direction];
    if (!current || !target) return;
    await updateQuickCommandOrder(current.id, target.sortOrder);
    await updateQuickCommandOrder(target.id, current.sortOrder);
    await reload();
  }

  const scopeName =
    scope === null
      ? "全局命令"
      : connections.find((c) => c.id === scope)?.name ?? "未知服务器";

  return (
    <div
      onClick={onClose}
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--bg-overlay)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 2000,
        backdropFilter: "blur(8px)",
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="animate-scale-in"
        style={{
          background: "var(--bg-elevated)",
          border: "1px solid var(--border-emphasis)",
          borderRadius: "var(--radius-xl)",
          width: 640,
          maxWidth: "92vw",
          maxHeight: "88vh",
          display: "flex",
          flexDirection: "column",
          boxShadow: "var(--shadow-xl)",
        }}
      >
        {/* Header */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            padding: "16px 20px",
            borderBottom: "1px solid var(--border-default)",
          }}
        >
          <div>
            <div style={{ fontSize: 16, fontWeight: 600, color: "var(--text-primary)" }}>
              快捷命令管理
            </div>
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 2 }}>
              定义可复用的命令片段，支持多行按顺序执行
            </div>
          </div>
          <button
            onClick={onClose}
            style={{
              background: "transparent",
              border: "none",
              color: "var(--text-muted)",
              fontSize: 18,
              cursor: "pointer",
              padding: "4px 8px",
            }}
          >
            ✕
          </button>
        </div>

        {/* Scope selector */}
        <div
          style={{
            padding: "14px 20px",
            borderBottom: "1px solid var(--border-default)",
            display: "flex",
            alignItems: "center",
            gap: 10,
          }}
        >
          <span style={{ fontSize: 12, color: "var(--text-secondary)" }}>作用域:</span>
          <select
            value={scopeToValue(scope)}
            onChange={(e) => handleScopeChange(e.target.value)}
            style={{
              flex: 1,
              background: "var(--bg-input)",
              color: "var(--text-primary)",
              border: "1px solid var(--border-default)",
              borderRadius: "var(--radius-md)",
              padding: "6px 10px",
              fontSize: 13,
              outline: "none",
            }}
          >
            <option value={GLOBAL_VALUE}>🌐 全局命令（所有服务器可用）</option>
            {activeConnectionId &&
              connections.find((c) => c.id === activeConnectionId) && (
                <optgroup label="当前服务器">
                  {connections
                    .filter((c) => c.id === activeConnectionId)
                    .map((c) => (
                      <option key={c.id} value={c.id}>
                        🖥️ {c.name} ({c.host})
                      </option>
                    ))}
                </optgroup>
              )}
            <optgroup label="所有服务器">
              {connections.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name} ({c.host})
                </option>
              ))}
            </optgroup>
          </select>
        </div>

        {/* Body: list or editor */}
        <div style={{ flex: 1, overflowY: "auto", padding: "12px 20px 20px" }}>
          {editing ? (
            <EditorForm
              label={editing.label}
              command={editing.command}
              isNew={editing.id === null}
              saving={saving}
              onLabel={(v) => setEditing({ ...editing, label: v })}
              onCommand={(v) => setEditing({ ...editing, command: v })}
              onSave={handleSave}
              onCancel={() => setEditing(null)}
            />
          ) : (
            <>
              <button
                onClick={startAdd}
                style={{
                  width: "100%",
                  padding: "10px",
                  background: "var(--accent-primary-muted)",
                  color: "var(--accent-primary)",
                  border: "1px dashed var(--border-accent)",
                  borderRadius: "var(--radius-md)",
                  fontSize: 13,
                  fontWeight: 600,
                  cursor: "pointer",
                  marginBottom: 12,
                }}
              >
                + 新增快捷命令
              </button>

              {loading ? (
                <div
                  style={{
                    padding: 24,
                    textAlign: "center",
                    color: "var(--text-muted)",
                    fontSize: 13,
                  }}
                >
                  加载中...
                </div>
              ) : items.length === 0 ? (
                <div
                  style={{
                    padding: 32,
                    textAlign: "center",
                    color: "var(--text-muted)",
                    fontSize: 13,
                  }}
                >
                  「{scopeName}」暂无快捷命令
                </div>
              ) : (
                <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                  {items.map((item, index) => (
                    <CommandRow
                      key={item.id}
                      item={item}
                      canMoveUp={index > 0}
                      canMoveDown={index < items.length - 1}
                      onEdit={() => startEdit(item)}
                      onDelete={() => handleDelete(item.id)}
                      onMoveUp={() => handleMove(index, -1)}
                      onMoveDown={() => handleMove(index, 1)}
                    />
                  ))}
                </div>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function CommandRow({
  item,
  canMoveUp,
  canMoveDown,
  onEdit,
  onDelete,
  onMoveUp,
  onMoveDown,
}: {
  item: QuickCommandItem;
  canMoveUp: boolean;
  canMoveDown: boolean;
  onEdit: () => void;
  onDelete: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
}) {
  const lineCount = item.command.split(/\r?\n/).filter((l) => l.trim()).length;
  return (
    <div
      style={{
        background: "var(--bg-surface)",
        border: "1px solid var(--border-default)",
        borderRadius: "var(--radius-md)",
        padding: "10px 12px",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 6 }}>
        <span
          style={{
            flex: 1,
            fontSize: 13,
            fontWeight: 600,
            color: "var(--text-primary)",
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {item.label}
        </span>
        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>{lineCount} 行</span>
        <RowBtn title="上移" disabled={!canMoveUp} onClick={onMoveUp}>
          ↑
        </RowBtn>
        <RowBtn title="下移" disabled={!canMoveDown} onClick={onMoveDown}>
          ↓
        </RowBtn>
        <RowBtn title="编辑" onClick={onEdit}>
          ✏️
        </RowBtn>
        <RowBtn title="删除" danger onClick={onDelete}>
          🗑️
        </RowBtn>
      </div>
      <pre
        style={{
          margin: 0,
          fontSize: 12,
          color: "var(--text-secondary)",
          fontFamily: "'Cascadia Code', 'Fira Code', 'JetBrains Mono', monospace",
          whiteSpace: "pre-wrap",
          wordBreak: "break-word",
          maxHeight: 120,
          overflowY: "auto",
        }}
      >
        {item.command}
      </pre>
    </div>
  );
}

function RowBtn({
  title,
  children,
  onClick,
  disabled,
  danger,
}: {
  title: string;
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
}) {
  return (
    <button
      title={title}
      onClick={onClick}
      disabled={disabled}
      style={{
        background: "transparent",
        border: "none",
        color: danger ? "var(--error)" : "var(--text-muted)",
        fontSize: 14,
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.3 : 0.8,
        padding: "2px 4px",
      }}
    >
      {children}
    </button>
  );
}

function EditorForm({
  label,
  command,
  isNew,
  saving,
  onLabel,
  onCommand,
  onSave,
  onCancel,
}: {
  label: string;
  command: string;
  isNew: boolean;
  saving: boolean;
  onLabel: (v: string) => void;
  onCommand: (v: string) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const canSave = label.trim().length > 0 && command.trim().length > 0 && !saving;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={{ fontSize: 14, fontWeight: 600, color: "var(--text-primary)" }}>
        {isNew ? "新增快捷命令" : "编辑快捷命令"}
      </div>
      <div>
        <label
          style={{
            display: "block",
            fontSize: 12,
            color: "var(--text-secondary)",
            marginBottom: 4,
          }}
        >
          名称
        </label>
        <input
          value={label}
          onChange={(e) => onLabel(e.target.value)}
          placeholder="如：重启 nginx"
          autoFocus
          style={{
            width: "100%",
            boxSizing: "border-box",
            background: "var(--bg-input)",
            color: "var(--text-primary)",
            border: "1px solid var(--border-default)",
            borderRadius: "var(--radius-md)",
            padding: "8px 10px",
            fontSize: 13,
            outline: "none",
          }}
        />
      </div>
      <div>
        <label
          style={{
            display: "block",
            fontSize: 12,
            color: "var(--text-secondary)",
            marginBottom: 4,
          }}
        >
          命令（每行一条，按顺序执行；以 # 开头的行和空行会被跳过）
        </label>
        <textarea
          value={command}
          onChange={(e) => onCommand(e.target.value)}
          placeholder={"sudo systemctl restart nginx\n# 清理 7 天前的日志\nfind /var/log -mtime +7 -delete"}
          rows={8}
          style={{
            width: "100%",
            boxSizing: "border-box",
            background: "var(--bg-input)",
            color: "var(--text-primary)",
            border: "1px solid var(--border-default)",
            borderRadius: "var(--radius-md)",
            padding: "8px 10px",
            fontSize: 13,
            fontFamily: "'Cascadia Code', 'Fira Code', 'JetBrains Mono', monospace",
            outline: "none",
            resize: "vertical",
            lineHeight: 1.5,
          }}
        />
      </div>
      <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
        <button
          onClick={onCancel}
          style={{
            padding: "8px 16px",
            background: "transparent",
            color: "var(--text-secondary)",
            border: "1px solid var(--border-default)",
            borderRadius: "var(--radius-md)",
            fontSize: 13,
            cursor: "pointer",
          }}
        >
          取消
        </button>
        <button
          onClick={onSave}
          disabled={!canSave}
          style={{
            padding: "8px 16px",
            background: canSave ? "var(--accent-primary)" : "var(--bg-surface-hover)",
            color: canSave ? "white" : "var(--text-muted)",
            border: "none",
            borderRadius: "var(--radius-md)",
            fontSize: 13,
            fontWeight: 600,
            cursor: canSave ? "pointer" : "not-allowed",
          }}
        >
          {saving ? "保存中..." : "保存"}
        </button>
      </div>
    </div>
  );
}
