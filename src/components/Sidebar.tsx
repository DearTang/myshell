import { useEffect, useMemo, useRef, useState } from "react";
import type { ConnectionConfig, ConnType } from "../api";
import {
  saveFolder,
  deleteFolder,
  renameFolder,
  copyConnection,
} from "../api";
import { useTheme } from "../hooks/useTheme";
import { BrandLogo } from "./BrandLogo";

interface Props {
  connections: ConnectionConfig[];
  folders: string[];
  onConnect: (config: ConnectionConfig) => void;
  onEdit: (config: ConnectionConfig) => void;
  onDelete: (id: string) => void;
  onAddNew: (initialType?: ConnType, initialFolderPath?: string) => void;
  onRefresh: () => void;
  onOpenSettings: () => void;
  onOpenQuickCommands: () => void;
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

  for (const f of folders) ensureNode(f);
  for (const c of conns) {
    const path = c.group_path || "/";
    ensureNode(path).conns.push(c);
  }

  const sortRec = (n: FolderNode) => {
    n.children.sort((a, b) => a.name.localeCompare(b.name, "zh"));
    n.conns.sort((a, b) => a.name.localeCompare(b.name, "zh"));
    n.children.forEach(sortRec);
  };
  sortRec(root);
  return root;
}

// 使用标准 emoji 图标，确保跨平台兼容
const CONN_ICONS: Record<ConnType, string> = {
  ssh: "🖥️",
  sftp: "📁",
  ftp: "📤",
  local: "💻",
};

const styles = {
  container: {
    position: "relative" as const,
    width: 240,
    minWidth: 240,
    background: "var(--bg-elevated)",
    borderRight: "1px solid var(--border-default)",
    display: "flex",
    flexDirection: "column" as const,
    overflow: "visible" as const,
  },
  collapsed: {
    position: "relative" as const,
    width: 44,
    minWidth: 44,
    background: "var(--bg-elevated)",
    borderRight: "1px solid var(--border-default)",
    display: "flex",
    flexDirection: "column" as const,
    alignItems: "center" as const,
    padding: "12px 0",
  },
  header: {
    padding: "14px 16px",
    borderBottom: "1px solid var(--border-subtle)",
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: 8,
  },
  headerTitle: {
    fontSize: 11,
    fontWeight: 600,
    color: "var(--text-secondary)",
    textTransform: "uppercase" as const,
    letterSpacing: "0.08em",
  },
  headerActions: {
    display: "flex",
    gap: 4,
    alignItems: "center",
  },
  listContainer: {
    flex: 1,
    overflowY: "auto" as const,
    padding: "8px 0",
  },
  emptyState: {
    padding: 32,
    textAlign: "center" as const,
    color: "var(--text-muted)",
    fontSize: 12,
  },
  contextMenu: {
    position: "fixed" as const,
    background: "var(--bg-surface)",
    border: "1px solid var(--border-emphasis)",
    borderRadius: "var(--radius-lg)",
    padding: 6,
    zIndex: 1000,
    minWidth: 160,
    boxShadow: "var(--shadow-xl)",
    backdropFilter: "blur(var(--glass-blur))",
  },
  menuItem: {
    padding: "8px 14px",
    fontSize: 12,
    cursor: "pointer",
    borderRadius: "var(--radius-md)",
    display: "flex",
    alignItems: "center",
    gap: 10,
    transition: "background var(--duration-fast) var(--ease-in-out)",
  },
  menuItemDanger: {
    color: "var(--error)",
  },
  toggleBtn: {
    position: "absolute" as const,
    right: -13,
    top: "50%",
    width: 26,
    height: 26,
    transform: "translateY(-50%)",
    borderRadius: "var(--radius-full)",
    background: "var(--bg-surface)",
    border: "1px solid var(--border-emphasis)",
    color: "var(--text-tertiary)",
    cursor: "pointer",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    zIndex: 100,
    padding: 0,
    boxShadow: "var(--shadow-sm), inset 0 1px 0 rgba(255,255,255,0.06)",
    transition:
      "background var(--duration-normal) var(--ease-out-expo), color var(--duration-normal) var(--ease-out-expo), border-color var(--duration-normal) var(--ease-out-expo), box-shadow var(--duration-normal) var(--ease-out-expo), transform var(--duration-normal) var(--ease-out-expo)",
  },
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
  onOpenQuickCommands,
  collapsed = false,
  onToggleCollapsed,
}: Props) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [menu, setMenu] = useState<
    { x: number; y: number; kind: "blank" | "folder"; folderPath?: string } | null
  >(null);
  const [searchQuery, setSearchQuery] = useState("");
  const { theme, toggleTheme } = useTheme();

  // Filter connections based on search query
  const filteredConnections = useMemo(() => {
    if (!searchQuery.trim()) return connections;
    const query = searchQuery.toLowerCase();
    return connections.filter((conn) => {
      const nameMatch = conn.name.toLowerCase().includes(query);
      const hostMatch = conn.host.toLowerCase().includes(query);
      return nameMatch || hostMatch;
    });
  }, [connections, searchQuery]);

  // Build tree for normal view (with folders)
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
            setMenu({ x: e.clientX, y: e.clientY, kind: "blank" });
          }}
          onEdit={() => onEdit(c)}
          onCopy={() => handleCopy(c.id)}
          onDelete={() => onDelete(c.id)}
        />
      );
    }
    for (const child of node.children) walk(child);
  };

  // Build flat list for search results
  const searchResults: React.ReactNode[] = useMemo(() => {
    if (!searchQuery.trim()) return [];
    return filteredConnections.map((conn) => (
      <ConnRow
        key={`search:${conn.id}`}
        conn={conn}
        depth={0}
        showGroupPath
        onConnect={() => onConnect(conn)}
        onContext={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY, kind: "blank" });
        }}
        onEdit={() => onEdit(conn)}
        onCopy={() => handleCopy(conn.id)}
        onDelete={() => onDelete(conn.id)}
      />
    ));
  }, [filteredConnections, searchQuery, onConnect, onEdit, handleCopy, onDelete]);

  // Use search results if searching, otherwise use tree
  if (!searchQuery.trim()) {
    walk(tree);
  }

  const toggleBtn = onToggleCollapsed ? (
    <button
      onClick={(e) => {
        e.stopPropagation();
        onToggleCollapsed();
      }}
      title={collapsed ? "展开侧栏" : "收起侧栏"}
      aria-label={collapsed ? "展开侧栏" : "收起侧栏"}
      style={styles.toggleBtn}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = "var(--accent-primary-muted)";
        e.currentTarget.style.borderColor = "var(--border-accent)";
        e.currentTarget.style.color = "var(--accent-primary)";
        e.currentTarget.style.transform = "translateY(-50%) scale(1.1)";
        e.currentTarget.style.boxShadow =
          "var(--shadow-md), inset 0 1px 0 rgba(255,255,255,0.08)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "var(--bg-surface)";
        e.currentTarget.style.borderColor = "var(--border-emphasis)";
        e.currentTarget.style.color = "var(--text-tertiary)";
        e.currentTarget.style.transform = "translateY(-50%) scale(1)";
        e.currentTarget.style.boxShadow =
          "var(--shadow-sm), inset 0 1px 0 rgba(255,255,255,0.06)";
      }}
    >
      <ChevronIcon size={12} rotated={collapsed} />
    </button>
  ) : null;

  if (collapsed) {
    return (
      <div style={styles.collapsed}>
        <BrandLogo size={26} />
        <span
          style={{
            fontSize: 10,
            writingMode: "vertical-rl",
            marginTop: 16,
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
      style={styles.container}
      onClick={() => setMenu(null)}
      onContextMenu={(e) => {
        if (e.target === e.currentTarget) {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY, kind: "blank" });
        }
      }}
    >
      <div style={styles.header}>
        <span style={styles.headerTitle}>连接管理</span>
        <div style={styles.headerActions}>
          <button
            onClick={toggleTheme}
            title={theme === "dark" ? "切换到亮色模式" : "切换到暗色模式"}
            style={{
              width: 28,
              height: 28,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              background: "transparent",
              color: "var(--text-tertiary)",
              border: "none",
              borderRadius: "var(--radius-md)",
              fontSize: 14,
              cursor: "pointer",
              transition: "all var(--duration-fast) var(--ease-in-out)",
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = "var(--bg-surface-hover)";
              e.currentTarget.style.color = "var(--text-primary)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "transparent";
              e.currentTarget.style.color = "var(--text-tertiary)";
            }}
          >
            {theme === "dark" ? "☀️" : "🌙"}
          </button>
          <IconBtn title="快捷命令" onClick={onOpenQuickCommands}>
            🧩
          </IconBtn>
          <IconBtn title="设置" onClick={onOpenSettings}>
            ⚙️
          </IconBtn>
          <button
            onClick={() => onAddNew()}
            title="新建连接"
            aria-label="新建连接"
            style={{
              width: 28,
              height: 28,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              background: "var(--accent-primary-muted)",
              color: "var(--accent-primary)",
              border: "1px solid transparent",
              borderRadius: "var(--radius-md)",
              cursor: "pointer",
              transition:
                "background var(--duration-normal) var(--ease-out-expo), color var(--duration-normal) var(--ease-out-expo), border-color var(--duration-normal) var(--ease-out-expo), box-shadow var(--duration-normal) var(--ease-out-expo), transform var(--duration-fast) var(--ease-out-expo)",
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = "var(--accent-primary)";
              e.currentTarget.style.color = "#ffffff";
              e.currentTarget.style.borderColor = "var(--accent-primary-hover)";
              e.currentTarget.style.boxShadow = "var(--shadow-glow)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "var(--accent-primary-muted)";
              e.currentTarget.style.color = "var(--accent-primary)";
              e.currentTarget.style.borderColor = "transparent";
              e.currentTarget.style.boxShadow = "none";
              e.currentTarget.style.transform = "scale(1)";
            }}
            onMouseDown={(e) => {
              e.currentTarget.style.transform = "scale(0.88)";
            }}
            onMouseUp={(e) => {
              e.currentTarget.style.transform = "scale(1)";
            }}
          >
            <PlusIcon size={16} />
          </button>
        </div>
      </div>

      {/* Search Box */}
      <div style={{
        padding: "8px 12px",
        borderBottom: "1px solid var(--border-subtle)",
      }}>
        <div style={{
          position: "relative",
          display: "flex",
          alignItems: "center",
        }}>
          <span style={{
            position: "absolute",
            left: 10,
            fontSize: 14,
            color: "var(--text-muted)",
            pointerEvents: "none",
          }}>
            🔍
          </span>
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="搜索连接名称或地址..."
            style={{
              width: "100%",
              background: "var(--bg-input)",
              border: "1px solid var(--border-default)",
              borderRadius: "var(--radius-md)",
              padding: "8px 10px 8px 32px",
              color: "var(--text-primary)",
              fontSize: 12,
              outline: "none",
              transition: "all var(--duration-fast) var(--ease-in-out)",
            }}
            onFocus={(e) => {
              e.currentTarget.style.borderColor = "var(--accent-primary)";
              e.currentTarget.style.boxShadow = "0 0 0 3px var(--accent-primary-muted)";
            }}
            onBlur={(e) => {
              e.currentTarget.style.borderColor = "var(--border-default)";
              e.currentTarget.style.boxShadow = "none";
            }}
          />
          {searchQuery && (
            <button
              onClick={() => setSearchQuery("")}
              title="清除搜索"
              style={{
                position: "absolute",
                right: 8,
                background: "transparent",
                border: "none",
                color: "var(--text-muted)",
                fontSize: 14,
                cursor: "pointer",
                padding: "2px 4px",
                borderRadius: "var(--radius-sm)",
                transition: "all var(--duration-fast) var(--ease-in-out)",
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.color = "var(--text-primary)";
                e.currentTarget.style.background = "var(--bg-surface-hover)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.color = "var(--text-muted)";
                e.currentTarget.style.background = "transparent";
              }}
            >
              ✕
            </button>
          )}
        </div>
        {searchQuery && (
          <div style={{
            marginTop: 6,
            fontSize: 11,
            color: "var(--text-muted)",
            textAlign: "center",
          }}>
            找到 {filteredConnections.length} 个连接
          </div>
        )}
      </div>

      <div
        style={styles.listContainer}
        onContextMenu={(e) => {
          if (e.target === e.currentTarget) {
            e.preventDefault();
            setMenu({ x: e.clientX, y: e.clientY, kind: "blank" });
          }
        }}
      >
        {searchQuery.trim() ? searchResults : rows}
        {filteredConnections.length === 0 && searchQuery && (
          <div style={styles.emptyState}>
            <div style={{ fontSize: 32, opacity: 0.3, marginBottom: 12 }}>🔍</div>
            <div>未找到匹配的连接</div>
            <div style={{ marginTop: 4, color: "var(--text-muted)" }}>
              尝试其他关键词
            </div>
          </div>
        )}
        {connections.length === 0 && folders.length === 0 && !searchQuery && (
          <div style={styles.emptyState}>
            <div style={{ fontSize: 32, opacity: 0.3, marginBottom: 12 }}>🖥️</div>
            <div>暂无连接</div>
            <div style={{ marginTop: 4, color: "var(--text-muted)" }}>
              点击 + 新建
            </div>
          </div>
        )}
      </div>

      {menu && (
        <div
          style={{
            ...styles.contextMenu,
            left: menu.x,
            top: menu.y,
          }}
          onClick={(e) => e.stopPropagation()}
        >
          {menu.kind === "blank" ? (
            <>
              <MenuItem icon="🔌" label="新建连接" onClick={() => { onAddNew("ssh"); setMenu(null); }} />
              <MenuItem icon="📂" label="新建文件夹" onClick={() => { handleAddFolder("/"); setMenu(null); }} />
            </>
          ) : (
            <>
              <MenuItem icon="🔌" label="新建连接" onClick={() => { onAddNew("ssh", menu.folderPath); setMenu(null); }} />
              <MenuItem icon="📂" label="新建子文件夹" onClick={() => { handleAddFolder(menu.folderPath!); setMenu(null); }} />
              <MenuItem icon="✏️" label="重命名" onClick={() => { handleRenameFolder(menu.folderPath!); setMenu(null); }} />
              <MenuItem icon="🗑️" label="删除" danger onClick={() => { handleDeleteFolder(menu.folderPath!); setMenu(null); }} />
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
        padding: "7px 16px",
        paddingLeft: 16 + node.depth * 14,
        cursor: "pointer",
        display: "flex",
        alignItems: "center",
        gap: 8,
        fontSize: 12,
        color: "var(--text-secondary)",
        userSelect: "none",
        transition: "background var(--duration-fast) var(--ease-in-out)",
        borderRadius: "0 var(--radius-md) var(--radius-md) 0",
        marginRight: 8,
      }}
      onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-surface-hover)")}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      <span style={{
        fontSize: 8,
        opacity: 0.6,
        transition: "transform var(--duration-normal) var(--ease-out-expo)",
        display: "inline-block",
        transform: isOpen ? "rotate(90deg)" : "rotate(0deg)",
      }}>
        ›
      </span>
      <span style={{ fontSize: 14, opacity: isOpen ? 1 : 0.7 }}>
        {isOpen ? "📂" : "📁"}
      </span>
      <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        {node.name}
      </span>
      {node.conns.length > 0 && (
        <span style={{
          fontSize: 10,
          color: "var(--text-muted)",
          background: "var(--bg-surface)",
          padding: "2px 6px",
          borderRadius: "var(--radius-full)",
        }}>
          {node.conns.length}
        </span>
      )}
    </div>
  );
}

function ConnRow({
  conn,
  depth,
  showGroupPath,
  onConnect,
  onContext,
  onEdit,
  onCopy,
  onDelete,
}: {
  conn: ConnectionConfig;
  depth: number;
  showGroupPath?: boolean;
  onConnect: () => void;
  onContext: (e: React.MouseEvent) => void;
  onEdit: () => void;
  onCopy: () => void;
  onDelete: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);
  const icon = CONN_ICONS[(conn.conn_type as ConnType) || "ssh"];

  function openMenu(e: React.MouseEvent<HTMLSpanElement>) {
    e.stopPropagation();
    const r = e.currentTarget.getBoundingClientRect();
    setAnchor({ x: r.right, y: r.bottom + 4 });
    setOpen(true);
  }

  // Format group path for display
  const groupDisplay = showGroupPath && conn.group_path && conn.group_path !== "/"
    ? conn.group_path.startsWith("/") ? conn.group_path.slice(1) : conn.group_path
    : null;

  return (
    <>
      <div
        onDoubleClick={onConnect}
        onContextMenu={onContext}
        style={{
          padding: "8px 16px 8px 24",
          paddingLeft: 24 + depth * 14,
          cursor: "pointer",
          display: "flex",
          alignItems: "center",
          gap: 10,
          fontSize: 13,
          color: "var(--text-primary)",
          transition: "background var(--duration-fast) var(--ease-in-out)",
          borderRadius: "0 var(--radius-md) var(--radius-md) 0",
          marginRight: 8,
        }}
        onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-surface-hover)")}
        onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
      >
        <span style={{
          fontSize: 16,
          opacity: 0.85,
        }}>
          {icon}
        </span>
        <div style={{ flex: 1, overflow: "hidden", display: "flex", flexDirection: "column", gap: 2 }}>
          <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {conn.name}
          </span>
          {groupDisplay && (
            <span style={{
              fontSize: 10,
              color: "var(--text-muted)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}>
              📂 {groupDisplay}
            </span>
          )}
          {showGroupPath && (
            <span style={{
              fontSize: 10,
              color: "var(--text-tertiary)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}>
              {conn.host}:{conn.port}
            </span>
          )}
        </div>
        <span
          onClick={openMenu}
          title="更多操作"
          style={{
            fontSize: 12,
            opacity: 0.35,
            cursor: "pointer",
            padding: "2px 6px",
            borderRadius: "var(--radius-sm)",
            transition: "all var(--duration-fast) var(--ease-in-out)",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.opacity = "1";
            e.currentTarget.style.background = "var(--bg-surface-active)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.opacity = "0.35";
            e.currentTarget.style.background = "transparent";
          }}
        >
          ⋯
        </span>
      </div>
      {open && anchor && (
        <>
          <div
            style={{ position: "fixed", inset: 0, zIndex: 999 }}
            onClick={(e) => {
              e.stopPropagation();
              setOpen(false);
            }}
          />
          <div
            style={{
              ...styles.contextMenu,
              left: anchor.x,
              top: anchor.y,
              transform: anchor.x + 140 > window.innerWidth ? "translateX(-100%)" : undefined,
              transformOrigin: "top right",
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <MenuItem icon="🔌" label="连接" onClick={() => { onConnect(); setOpen(false); }} />
            <MenuItem icon="✏️" label="编辑" onClick={() => { onEdit(); setOpen(false); }} />
            <MenuItem icon="📋" label="复制" onClick={() => { onCopy(); setOpen(false); }} />
            <MenuItem icon="🗑️" label="删除" danger onClick={() => { onDelete(); setOpen(false); }} />
          </div>
        </>
      )}
    </>
  );
}

function PlusIcon({ size = 16 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
      style={{ display: "block" }}
    >
      <path
        d="M12 5v14M5 12h14"
        stroke="currentColor"
        strokeWidth="2.2"
        strokeLinecap="round"
      />
    </svg>
  );
}

function ChevronIcon({ size = 14, rotated }: { size?: number; rotated: boolean }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
      style={{
        display: "block",
        transform: rotated ? "rotate(180deg)" : "rotate(0deg)",
        transition: "transform var(--duration-normal) var(--ease-out-expo)",
      }}
    >
      {/* Left-pointing chevron; rotates 180° to point right when collapsed */}
      <path
        d="M15 6l-6 6 6 6"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
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
        width: 28,
        height: 28,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "transparent",
        color: "var(--text-tertiary)",
        border: "none",
        borderRadius: "var(--radius-md)",
        fontSize: 14,
        cursor: "pointer",
        transition: "all var(--duration-fast) var(--ease-in-out)",
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = "var(--bg-surface-hover)";
        e.currentTarget.style.color = "var(--text-primary)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "transparent";
        e.currentTarget.style.color = "var(--text-tertiary)";
      }}
    >
      {children}
    </button>
  );
}

function MenuItem({
  icon,
  label,
  onClick,
  danger,
}: {
  icon?: string;
  label: string;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <div
      onClick={onClick}
      style={{
        ...styles.menuItem,
        color: danger ? "var(--error)" : "var(--text-primary)",
      }}
      onMouseEnter={(e) => (e.currentTarget.style.background = danger ? "var(--error-muted)" : "var(--bg-surface-hover)")}
      onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
    >
      {icon && <span style={{ fontSize: 14, opacity: 0.8 }}>{icon}</span>}
      {label}
    </div>
  );
}
