import { useEffect, useMemo, useRef, useState } from "react";
import type { ConnectionConfig, ConnType } from "../api";
import {
  saveFolder,
  deleteFolder,
  renameFolder,
  copyConnection,
} from "../api";

interface Props {
  connections: ConnectionConfig[];
  folders: string[];
  onConnect: (config: ConnectionConfig) => void;
  onEdit: (config: ConnectionConfig) => void;
  onDelete: (id: string) => void;
  onAddNew: (initialType?: ConnType) => void;
  onRefresh: () => void;
  onOpenSettings: () => void;
  /** When true, the sidebar collapses to a narrow strip showing only an
   * "expand" button. Clicking it flips back to the full 220px panel. */
  collapsed?: boolean;
  onToggleCollapsed?: () => void;
}

interface FolderNode {
  path: string;
  name: string;
  depth: number;
  children: FolderNode[];
  conns: ConnectionConfig[];
}

const ROOT: FolderNode = {
  path: "/",
  name: "root",
  depth: 0,
  children: [],
  conns: [],
};

/** Build a tree from connections' group_path + the explicit folders list.
 * Empty folders (no children + no conns) are still rendered so users can
 * pre-create structure. */
function buildTree(conns: ConnectionConfig[], folders: string[]): FolderNode {
  const root: FolderNode = { ...ROOT, children: [], conns: [] };
  const nodeByPath = new Map<string, FolderNode>([["/", root]]);

  const ensureNode = (path: string): FolderNode => {
    if (nodeByPath.has(path)) return nodeByPath.get(path)!;
    const segments = path.split("/").filter(Boolean);
    const name = segments[segments.length - 1];
    const parentPath = "/" + segments.slice(0, -1).join("/");
    const parent = ensureNode(parentPath || "/");
    const node: FolderNode = {
      path,
      name,
      depth: segments.length,
      children: [],
      conns: [],
    };
    parent.children.push(node);
    nodeByPath.set(path, node);
    return node;
  };

  // Explicit folders first (creates empty branches).
  for (const f of folders) ensureNode(f);

  // Then attach connections.
  for (const c of conns) {
    const path = c.group_path || "/";
    ensureNode(path).conns.push(c);
  }

  // Stable sort within each level: folders first (alphabetical), then root-level conns.
  const sortRec = (n: FolderNode) => {
    n.children.sort((a, b) => a.name.localeCompare(b.name, "zh"));
    n.conns.sort((a, b) => a.name.localeCompare(b.name, "zh"));
    n.children.forEach(sortRec);
  };
  sortRec(root);
  return root;
}

const CONN_ICONS: Record<ConnType, string> = {
  ssh: "🖥",
  sftp: "📁",
  ftp: "📤",
};

export function Sidebar({
  connections,
  folders,
  onConnect,
  onEdit,
  onDelete,
  onAddNew,
  onRefresh,
  onOpenSettings,
  collapsed = false,
  onToggleCollapsed,
}: Props) {
  // Default: all folders collapsed. Set is empty initially.
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [menu, setMenu] = useState<
    | { x: number; y: number; kind: "blank" | "folder"; folderPath?: string }
    | null
  >(null);

  const tree = useMemo(() => buildTree(connections, folders), [connections, folders]);

  async function handleCopy(id: string) {
    try {
      await copyConnection(id);
      onRefresh();
    } catch (e) {
      window.alert(`复制失败: ${e}`);
    }
  }

  function toggle(path: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  async function handleAddFolder(parentPath: string) {
    const name = window.prompt(parentPath === "/" ? "新建文件夹名称" : `在 ${parentPath} 下新建`);
    if (!name) return;
    const child = `${parentPath === "/" ? "" : parentPath}/${name.trim()}`;
    try {
      await saveFolder(child);
      onRefresh();
    } catch (e) {
      window.alert(`新建失败: ${e}`);
    }
  }

  async function handleRenameFolder(oldPath: string) {
    const next = window.prompt("重命名文件夹", oldPath);
    if (!next || next === oldPath) return;
    try {
      await renameFolder(oldPath, next.trim());
      onRefresh();
    } catch (e) {
      window.alert(`重命名失败: ${e}`);
    }
  }

  async function handleDeleteFolder(path: string) {
    if (!window.confirm(`确定删除文件夹 ${path}？（仅空文件夹可删除）`)) return;
    try {
      await deleteFolder(path);
      onRefresh();
    } catch (e) {
      window.alert(`删除失败: ${e}`);
    }
  }

  // Flatten for rendering — DFS in display order.
  const rows: React.ReactNode[] = [];
  const walk = (node: FolderNode) => {
    if (node.path !== "/") {
      const isOpen = expanded.has(node.path);
      rows.push(
        <FolderRow
          key={`f:${node.path}`}
          node={node}
          isOpen={isOpen}
          onToggle={() => toggle(node.path)}
          onContext={(e) => {
            e.preventDefault();
            setMenu({ x: e.clientX, y: e.clientY, kind: "folder", folderPath: node.path });
          }}
        />
      );
      if (!isOpen) return;
    }
    for (const c of node.conns) {
      rows.push(
        <ConnRow
          key={`c:${c.id}`}
          conn={c}
          depth={node.depth}
          onConnect={() => onConnect(c)}
          onContext={(e) => {
            e.preventDefault();
            setMenu({
              x: e.clientX,
              y: e.clientY,
              kind: "blank",
            });
            // store conn id via closure
            (e.currentTarget as HTMLElement).dataset.connId = c.id;
          }}
          onEdit={() => onEdit(c)}
          onCopy={() => handleCopy(c.id)}
          onDelete={() => onDelete(c.id)}
        />
      );
    }
    for (const child of node.children) walk(child);
  };
  walk(tree);

  // Floating pill button anchored to the sidebar's right edge, vertically
  // centered. Same component in both states — only the icon direction
  // differs. Half-protrudes into the main area so it reads as a clearly
  // grabbable handle rather than another toolbar button.
  const toggleBtn = onToggleCollapsed ? (
    <button
      onClick={(e) => {
        e.stopPropagation();
        onToggleCollapsed();
      }}
      title={collapsed ? "展开侧栏" : "收起侧栏"}
      style={{
        position: "absolute",
        right: -11,
        top: "50%",
        transform: "translateY(-50%)",
        width: 22,
        height: 48,
        borderRadius: 10,
        background: "var(--bg-panel)",
        border: "1px solid var(--border)",
        color: "var(--text-muted)",
        cursor: "pointer",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        fontSize: 12,
        zIndex: 100,
        padding: 0,
        boxShadow: "2px 1px 8px rgba(0,0,0,0.3)",
        transition: "background 0.15s, color 0.15s, transform 0.15s ease-out",
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = "var(--accent)";
        e.currentTarget.style.color = "var(--bg-panel)";
        e.currentTarget.style.transform = "translateY(-50%) scale(1.08)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "var(--bg-panel)";
        e.currentTarget.style.color = "var(--text-muted)";
        e.currentTarget.style.transform = "translateY(-50%)";
      }}
    >
      {collapsed ? "▶" : "◀"}
    </button>
  ) : null;

  if (collapsed) {
    return (
      <div
        style={{
          position: "relative",
          width: 30,
          minWidth: 30,
          background: "var(--bg-sidebar)",
          borderRight: "1px solid var(--border)",
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          padding: "10px 0",
        }}
      >
        <span style={{ fontSize: 14, opacity: 0.5 }}>⚡</span>
        <span
          style={{
            fontSize: 10,
            writingMode: "vertical-rl",
            marginTop: 14,
            letterSpacing: 2,
            color: "var(--text-muted)",
            opacity: 0.7,
          }}
        >
          连接管理
        </span>
        {toggleBtn}
      </div>
    );
  }

  return (
    <div
      style={{
        position: "relative",
        width: 220,
        minWidth: 220,
        background: "var(--bg-sidebar)",
        borderRight: "1px solid var(--border)",
        display: "flex",
        flexDirection: "column",
        overflow: "visible",
      }}
      onClick={() => setMenu(null)}
      onContextMenu={(e) => {
        // Background right-click → new top-level folder.
        if (e.target === e.currentTarget) {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY, kind: "blank" });
        }
      }}
    >
      {/* Header — title + compact icon toolbar.
          New-connection is the primary action; settings is secondary. */}
      <div
        style={{
          padding: "8px 10px",
          borderBottom: "1px solid var(--border)",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 4,
        }}
      >
        <span style={{ fontSize: 12, fontWeight: 600, color: "var(--text-secondary)" }}>
          连接管理
        </span>
        <div style={{ display: "flex", gap: 2, alignItems: "center" }}>
          <IconBtn title="设置" onClick={onOpenSettings}>
            ⚙️
          </IconBtn>
          <button
            onClick={() => onAddNew()}
            title="新建连接"
            style={{
              width: 26,
              height: 26,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              background: "var(--accent)",
              color: "var(--bg-panel)",
              border: "none",
              borderRadius: 5,
              fontSize: 15,
              lineHeight: 1,
              cursor: "pointer",
              fontWeight: 600,
              transition: "filter 0.12s",
            }}
            onMouseEnter={(e) => (e.currentTarget.style.filter = "brightness(1.15)")}
            onMouseLeave={(e) => (e.currentTarget.style.filter = "none")}
          >
            +
          </button>
        </div>
      </div>

      {/* Connection List */}
      <div
        style={{ flex: 1, overflowY: "auto", padding: "4px 0" }}
        onContextMenu={(e) => {
          if (e.target === e.currentTarget) {
            e.preventDefault();
            setMenu({ x: e.clientX, y: e.clientY, kind: "blank" });
          }
        }}
      >
        {rows}
        {connections.length === 0 && folders.length === 0 && (
          <div
            style={{
              padding: 24,
              textAlign: "center",
              color: "var(--text-muted)",
              fontSize: 12,
            }}
          >
            暂无连接，点击"新建"添加
          </div>
        )}
      </div>

      {/* Context Menu */}
      {menu && (
        <div
          style={{
            position: "fixed",
            left: menu.x,
            top: menu.y,
            background: "var(--bg-input)",
            border: "1px solid var(--border)",
            borderRadius: 6,
            padding: 4,
            zIndex: 1000,
            minWidth: 140,
            boxShadow: "0 4px 12px rgba(0,0,0,0.4)",
          }}
          onClick={(e) => e.stopPropagation()}
        >
          {menu.kind === "blank" ? (
            <>
              <MenuItem
                label="新建 SSH 连接"
                onClick={() => {
                  onAddNew("ssh");
                  setMenu(null);
                }}
              />
              <MenuItem
                label="新建 SFTP 连接"
                onClick={() => {
                  onAddNew("sftp");
                  setMenu(null);
                }}
              />
              <MenuItem
                label="新建 FTP 连接"
                onClick={() => {
                  onAddNew("ftp");
                  setMenu(null);
                }}
              />
              <MenuItem
                label="新建文件夹"
                onClick={() => {
                  handleAddFolder("/");
                  setMenu(null);
                }}
              />
            </>
          ) : (
            <>
              <MenuItem
                label="新建子文件夹"
                onClick={() => {
                  handleAddFolder(menu.folderPath!);
                  setMenu(null);
                }}
              />
              <MenuItem
                label="重命名"
                onClick={() => {
                  handleRenameFolder(menu.folderPath!);
                  setMenu(null);
                }}
              />
              <MenuItem
                label="删除"
                danger
                onClick={() => {
                  handleDeleteFolder(menu.folderPath!);
                  setMenu(null);
                }}
              />
            </>
          )}
        </div>
      )}
      {toggleBtn}
    </div>
  );
}

function FolderRow({
  node,
  isOpen,
  onToggle,
  onContext,
}: {
  node: FolderNode;
  isOpen: boolean;
  onToggle: () => void;
  onContext: (e: React.MouseEvent) => void;
}) {
  return (
    <div
      onClick={onToggle}
      onContextMenu={onContext}
      style={{
        padding: "5px 14px",
        paddingLeft: 14 + node.depth * 12,
        cursor: "pointer",
        display: "flex",
        alignItems: "center",
        gap: 6,
        fontSize: 12,
        color: "var(--text-secondary)",
        userSelect: "none",
        transition: "background 0.12s",
      }}
      onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-input)")}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      <span style={{ fontSize: 9, width: 9 }}>{isOpen ? "▼" : "▶"}</span>
      <span style={{ fontSize: 13 }}>{isOpen ? "📂" : "📁"}</span>
      <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        {node.name}
      </span>
      {node.conns.length > 0 && (
        <span style={{ fontSize: 10, color: "var(--text-muted)" }}>{node.conns.length}</span>
      )}
    </div>
  );
}

function ConnRow({
  conn,
  depth,
  onConnect,
  onContext,
  onEdit,
  onCopy,
  onDelete,
}: {
  conn: ConnectionConfig;
  depth: number;
  onConnect: () => void;
  onContext: (e: React.MouseEvent) => void;
  onEdit: () => void;
  onCopy: () => void;
  onDelete: () => void;
}) {
  const [open, setOpen] = useState(false);
  // Anchor coordinates (viewport-relative) for the dropdown — captured from
  // the ⋯ button's bounding rect at click time so the menu pops next to it.
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);
  const icon = CONN_ICONS[(conn.conn_type as ConnType) || "ssh"];

  function openMenu(e: React.MouseEvent<HTMLSpanElement>) {
    e.stopPropagation();
    const r = e.currentTarget.getBoundingClientRect();
    // Place the menu flush with the button's right edge and just below it.
    // If it would overflow the viewport, the consumer can scroll/clip —
    // we don't bother flip-adjusting since the dropdown is small.
    setAnchor({ x: r.right, y: r.bottom + 2 });
    setOpen(true);
  }

  return (
    <>
      <div
        onDoubleClick={onConnect}
        onContextMenu={onContext}
        style={{
          padding: "6px 14px 6px 26 + depth * 12",
          paddingLeft: 26 + depth * 12,
          cursor: "pointer",
          display: "flex",
          alignItems: "center",
          gap: 8,
          fontSize: 13,
          color: "var(--text-primary)",
          transition: "background 0.12s",
        }}
        onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-input)")}
        onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
      >
        <span style={{ fontSize: 14 }}>{icon}</span>
        <span
          style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
        >
          {conn.name}
        </span>
        <span
          onClick={openMenu}
          title="更多操作"
          style={{ fontSize: 14, opacity: 0.4, cursor: "pointer", padding: "0 4px" }}
          onMouseEnter={(e) => (e.currentTarget.style.opacity = "1")}
          onMouseLeave={(e) => (e.currentTarget.style.opacity = "0.4")}
        >
          ⋯
        </span>
      </div>
      {open && anchor && (
        <>
          {/* Click-away layer: any click outside the dropdown closes it. */}
          <div
            style={{ position: "fixed", inset: 0, zIndex: 999 }}
            onClick={(e) => {
              e.stopPropagation();
              setOpen(false);
            }}
          />
          <div
            style={{
              position: "fixed",
              left: anchor.x,
              top: anchor.y,
              background: "var(--bg-input)",
              border: "1px solid var(--border)",
              borderRadius: 6,
              padding: 4,
              zIndex: 1000,
              minWidth: 100,
              boxShadow: "0 4px 12px rgba(0,0,0,0.4)",
              // If the menu would overflow the right viewport edge, flip to
              // open leftward from the same anchor point.
              transform:
                anchor.x + 120 > window.innerWidth ? "translateX(-100%)" : undefined,
              transformOrigin: "top right",
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <MenuItem
              label="连接"
              onClick={() => {
                onConnect();
                setOpen(false);
              }}
            />
            <MenuItem
              label="编辑"
              onClick={() => {
                onEdit();
                setOpen(false);
              }}
            />
            <MenuItem
              label="复制"
              onClick={() => {
                onCopy();
                setOpen(false);
              }}
            />
            <MenuItem
              label="删除"
              danger
              onClick={() => {
                onDelete();
                setOpen(false);
              }}
            />
          </div>
        </>
      )}
    </>
  );
}

function IconBtn({
  title,
  onClick,
  children,
}: {
  title: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      title={title}
      onClick={onClick}
      style={{
        width: 26,
        height: 26,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "transparent",
        color: "var(--text-muted)",
        border: "none",
        borderRadius: 5,
        fontSize: 13,
        cursor: "pointer",
        transition: "background 0.12s, color 0.12s",
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = "var(--bg-input)";
        e.currentTarget.style.color = "var(--text-primary)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "transparent";
        e.currentTarget.style.color = "var(--text-muted)";
      }}
    >
      {children}
    </button>
  );
}

function MenuItem({
  label,
  onClick,
  danger,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <div
      onClick={onClick}
      style={{
        padding: "6px 12px",
        fontSize: 12,
        cursor: "pointer",
        color: danger ? "var(--error)" : "var(--text-primary)",
        borderRadius: 4,
      }}
      onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-dark)")}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      {label}
    </div>
  );
}
