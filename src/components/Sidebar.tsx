import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import type { ConnectionConfig, ConnType } from "../api";
import {
  saveFolder,
  deleteFolder,
  renameFolder,
  copyConnection,
  renameConnection,
  resetKnownHost,
} from "../api";
import { useTheme } from "../hooks/useTheme";
import { useConnectionDrag } from "../hooks/useConnectionDrag";
import { BrandLogo } from "./BrandLogo";
import { ConnIcon } from "./ConnIcon";
import { ConfirmDialog, type ConfirmState } from "./ConfirmDialog";

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
  /** Expanded sidebar width in px. The resize handle on the right edge drives
   * this; persisted by the parent so it survives reloads. Ignored when
   * collapsed (the collapsed rail is a fixed 44px). */
  width?: number;
  onWidthChange?: (w: number) => void;
  /** Running app version, shown in the expanded-mode footer. */
  version?: string;
  /** When true, an accent dot is shown next to the version (update available). */
  updateAvailable?: boolean;
  /** Open the About / what's-new dialog. */
  onOpenAbout?: () => void;
  /** Open the feedback dialog. */
  onOpenFeedback?: () => void;
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

// 连接类型图标来自 src/assets/iconfont (iconfont.cn)。
// Sidebar / TabBar / ConnectionDialog 统一用 ConnIcon 渲染，保证跨平台一致。

const styles = {
  container: {
    position: "relative" as const,
    // width / minWidth are set inline from the `width` prop so the sidebar is
    // resizable; the static 240 default lives there too.
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
  width,
  onWidthChange,
  version,
  updateAvailable,
  onOpenAbout,
  onOpenFeedback,
}: Props) {
  // Resizable sidebar width. Defaults to 240 when the parent doesn't manage it;
  // clamped to [200, 560] in onResizeStart so it can't get unusably narrow or
  // eat the whole window. Long connection names become fully visible once the
  // user drags the handle wider.
  const SIDEBAR_DEFAULT_WIDTH = 240;
  const sidebarWidth = width ?? SIDEBAR_DEFAULT_WIDTH;

  const onResizeStart = (e: ReactMouseEvent) => {
    if (!onWidthChange) return;
    e.preventDefault();
    const startX = e.clientX;
    const startW = sidebarWidth;
    const onMove = (ev: globalThis.MouseEvent) => {
      const w = Math.min(560, Math.max(200, startW + (ev.clientX - startX)));
      onWidthChange(w);
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  };

  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [menu, setMenu] = useState<
    { x: number; y: number; kind: "blank" | "folder"; folderPath?: string } | null
  >(null);
  const [searchQuery, setSearchQuery] = useState("");
  const { theme, toggleTheme } = useTheme();

  // Confirm gates for destructive actions. `window.confirm` is silently
  // swallowed by some Tauri WebViews, so we drive a real React modal
  // (ConfirmDialog) from these states instead. Each holds the action's
  // payload (connection / folder path) and is cleared on confirm or cancel.
  const [deleteConnConfirm, setDeleteConnConfirm] = useState<ConfirmState<ConnectionConfig> | null>(null);
  const [deleteFolderConfirm, setDeleteFolderConfirm] = useState<
    | (ConfirmState<{ path: string }> & { connCount: number; subFolderCount: number })
    | null
  >(null);
  // Reset stored host key (recover from a legitimate server-side host-key
  // change). Not destructive to credentials — only forgets the trust anchor.
  const [resetHostKeyConfirm, setResetHostKeyConfirm] = useState<ConfirmState<ConnectionConfig> | null>(null);

  // Long-press-drag-to-folder. While dragState is active the list switches to
  // a compact folders-only view so the user doesn't have to scroll past dozens
  // of connections to reach the target folder. onMoved auto-expands the target
  // (so the moved connection is immediately visible) and triggers a refresh.
  const { dragState, beginDrag } = useConnectionDrag({
    onMoved: (targetFolderPath) => {
      setExpanded((prev) => {
        const next = new Set(prev);
        next.add(targetFolderPath);
        return next;
      });
      onRefresh();
    },
    onMoveError: (connId, err) => {
      window.alert(`移动失败: ${err}`);
      console.error("[move_connection] failed for", connId, err);
    },
  });
  const isDragging = dragState !== null;

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

  /** Rename a connection via the lightweight rename command (name is a plaintext
   * column, no full re-save needed). Prompted from the connection's right-click
   * menu. */
  async function handleRenameConnection(id: string, currentName: string) {
    const next = window.prompt("重命名连接", currentName);
    if (!next || next.trim() === currentName) return;
    try {
      await renameConnection(id, next.trim());
      onRefresh();
    } catch (e) {
      window.alert(`重命名失败: ${e}`);
    }
  }

  /** Open the delete-connection confirm modal. The actual delete + recycle-bin
   * logic lives in App (onDelete); we only gate it here via a React modal
   * (window.confirm is unreliable in Tauri WebViews) so the prompt always
   * shows regardless of entry point (⋯ menu, etc.). */
  function handleDeleteConnection(conn: ConnectionConfig) {
    setDeleteConnConfirm({
      title: "删除连接",
      message: `确定删除连接「${conn.name}」？\n删除后可在「设置 → 找回连接」中恢复。`,
      confirmLabel: "删除",
      danger: true,
      payload: conn,
    });
  }

  /** Actually run the connection delete after the user confirmed. */
  function confirmDeleteConnection() {
    if (!deleteConnConfirm) return;
    onDelete(deleteConnConfirm.payload.id);
    setDeleteConnConfirm(null);
  }

  /** Ask to forget the stored host key for a connection (SSH/SFTP only). The
   * next connect then re-runs trust-on-first-use. Recovery for a server whose
   * host key legitimately changed (reinstall / regenerated host keys). */
  function handleResetHostKey(conn: ConnectionConfig) {
    setResetHostKeyConfirm({
      title: "重置主机密钥信任",
      message: `确定清除「${conn.name}」(${conn.host}:${conn.port}) 已保存的主机密钥指纹？\n下次连接会重新信任服务器当前的主机密钥。仅在服务器重装或更换了密钥时使用。`,
      confirmLabel: "重置并重连信任",
      danger: false,
      payload: conn,
    });
  }

  /** Run the host-key reset after the user confirmed. */
  async function confirmResetHostKey() {
    if (!resetHostKeyConfirm) return;
    const conn = resetHostKeyConfirm.payload;
    setResetHostKeyConfirm(null);
    try {
      await resetKnownHost(conn.host, conn.port);
      window.alert(`已清除「${conn.name}」的主机密钥记录，下次连接将重新信任。`);
    } catch (e) {
      window.alert(`重置失败: ${e}`);
    }
  }

  async function handleDeleteFolder(path: string) {
    // Count connections + sub-folders under this path (recursively, so nested
    // children are caught too) to show a precise warning. window.confirm is
    // swallowed by some Tauri WebViews, so we open a React modal instead.
    // Match rule mirrors the backend like_prefix_pattern: a row belongs under
    // `path` if its group/path equals `path` or starts with `path + "/"`.
    const prefix = path.endsWith("/") ? path : path + "/";
    const isUnder = (p: string) => p === path || p.startsWith(prefix);
    const connCount = connections.filter((c) => isUnder(c.group_path || "/")).length;
    const subFolders = folders.filter((f) => f !== path && isUnder(f));

    // Red bold inline span — used to emphasize the connection count inside
    // the message when the folder actually contains connections (the real
    // risk). Per spec: only emphasize when there ARE connections; an empty
    // folder or one with only sub-folders doesn't need the alarm.
    const hl = (text: string) => (
      <span style={{ color: "var(--error)", fontWeight: 700 }}>{text}</span>
    );

    let message: React.ReactNode;
    if (connCount > 0) {
      // Connections present → emphasize the count + "包含连接" so the user
      // can't miss that real connections will be moved to the recycle bin.
      const connPart = hl(`${connCount} 个连接`);
      const folderPart =
        subFolders.length > 0 ? <>、{hl(`${subFolders.length} 个子文件夹`)}</> : null;
      message = (
        <>
          文件夹「{path}」{hl("下包含 ")}
          {connPart}
          {folderPart}
          。删除后这些子文件夹将一并移除，其中的连接会移入回收站（可在「设置 → 找回连接」恢复）。
          <br />
          确定删除？
        </>
      );
    } else if (subFolders.length > 0) {
      // Only sub-folders, no connections → no emphasis needed.
      message = (
        <>
          文件夹「{path}」下包含 {subFolders.length} 个子文件夹。
          <br />
          确定删除？
        </>
      );
    } else {
      message = (
        <>
          确定删除空文件夹「{path}」？
        </>
      );
    }

    setDeleteFolderConfirm({
      title: connCount > 0 || subFolders.length > 0 ? "删除文件夹" : "删除空文件夹",
      message,
      confirmLabel: "删除",
      danger: true,
      payload: { path },
      connCount,
      subFolderCount: subFolders.length,
    });
  }

  /** Actually run the folder delete after the user confirmed. */
  async function confirmDeleteFolder() {
    if (!deleteFolderConfirm) return;
    const path = deleteFolderConfirm.payload.path;
    setDeleteFolderConfirm(null);
    try {
      // One backend call soft-deletes child connections (into the recycle bin)
      // and physically drops this folder + all descendants, transaction-safe.
      await deleteFolder(path);
      onRefresh();
    } catch (e) {
      window.alert(`删除失败: ${e}`);
    }
  }

  const rows: React.ReactNode[] = [];
  const walk = (node: FolderNode) => {
    if (node.path !== "/") {
      const isOpen = isDragging ? true : expanded.has(node.path);
      rows.push(
        <FolderRow
          key={`f:${node.path}`}
          node={node}
          isOpen={isOpen}
          isDropTarget={dragState?.hoverFolderPath === node.path}
          onToggle={() => toggle(node.path)}
          onContext={(e) => {
            e.preventDefault();
            if (isDragging) return;
            setMenu({ x: e.clientX, y: e.clientY, kind: "folder", folderPath: node.path });
          }}
        />
      );
      // While dragging, render folders only (expanded) so the user can reach
      // any target folder without scrolling past the connections inside each.
      if (isDragging) {
        for (const child of node.children) walk(child);
        return;
      }
      if (!isOpen) return;
    }
    for (const c of node.conns) {
      rows.push(
        <ConnRow
          key={`c:${c.id}`}
          conn={c}
          depth={node.depth}
          draggingConnId={dragState?.connId ?? null}
          onPointerDown={(e) => beginDrag(c, e)}
          onConnect={() => onConnect(c)}
          onContext={(e) => {
            // Suppress the browser context menu on connections, but don't open
            // our own menu either — connection actions (rename/edit/copy/delete)
            // live in the ⋯ button per spec. Right-click is reserved so it
            // doesn't duplicate the ⋯ menu.
            e.preventDefault();
          }}
          onEdit={() => onEdit(c)}
          onCopy={() => handleCopy(c.id)}
          onDelete={() => handleDeleteConnection(c)}
          onResetHostKey={() => handleResetHostKey(c)}
          onRename={() => handleRenameConnection(c.id, c.name)}
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
        }}
        onEdit={() => onEdit(conn)}
        onCopy={() => handleCopy(conn.id)}
        onDelete={() => handleDeleteConnection(conn)}
        onResetHostKey={() => handleResetHostKey(conn)}
        onRename={() => handleRenameConnection(conn.id, conn.name)}
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
      style={{ ...styles.container, width: sidebarWidth, minWidth: sidebarWidth }}
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
        {isDragging && <DragHint
          hoverFolderPath={dragState?.hoverFolderPath ?? null}
          sourceConnName={dragState?.connName ?? ""}
        />}
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
            <ConnIcon connType="ssh" size={32} style={{ opacity: 0.3, marginBottom: 12, color: "var(--text-secondary)" }} />
            <div>暂无连接</div>
            <div style={{ marginTop: 4, color: "var(--text-muted)" }}>
              点击 + 新建
            </div>
          </div>
        )}
      </div>

      {/* Version footer — only in expanded mode (the collapsed rail is
          44px wide and already drops the header/search for density). Shows
          the running version; a small accent dot appears when a newer
          release was detected. Click opens the About / what's-new dialog.
          The feedback icon (💬) sits at the far right — clicking it opens the
          feedback dialog. It stops propagation so the version-row click
          (About) isn't triggered. */}
      {version && (
        <div
          onClick={() => onOpenAbout?.()}
          title={updateAvailable ? "发现新版本，点击查看" : "关于 MyShell"}
          style={{
            flexShrink: 0,
            borderTop: "1px solid var(--border-subtle)",
            padding: "9px 16px",
            display: "flex",
            alignItems: "center",
            gap: 8,
            cursor: "pointer",
            userSelect: "none",
            transition: "background var(--duration-fast) var(--ease-in-out)",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "var(--bg-surface-hover)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "transparent";
          }}
        >
          <span style={{ fontSize: 11, color: "var(--text-tertiary)" }}>
            MyShell
          </span>
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
            v{version}
          </span>
          {updateAvailable && (
            <span
              title="发现新版本"
              style={{
                width: 7,
                height: 7,
                borderRadius: "var(--radius-full)",
                background: "var(--accent-secondary)",
                boxShadow: "0 0 6px var(--accent-secondary)",
                marginLeft: "auto",
              }}
            />
          )}
          {onOpenFeedback && (
            <span
              onClick={(e) => {
                e.stopPropagation();
                onOpenFeedback();
              }}
              title="反馈与建议"
              style={{
                fontSize: 13,
                lineHeight: 1,
                cursor: "pointer",
                // If no update dot claimed the right edge, push the icon right.
                marginLeft: updateAvailable ? undefined : "auto",
                padding: "0 2px",
                opacity: 0.6,
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.opacity = "1";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.opacity = "0.6";
              }}
            >
              💬
            </span>
          )}
        </div>
      )}

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
      {deleteConnConfirm && (
        <ConfirmDialog
          title={deleteConnConfirm.title}
          message={deleteConnConfirm.message}
          confirmLabel={deleteConnConfirm.confirmLabel}
          danger={deleteConnConfirm.danger}
          onConfirm={confirmDeleteConnection}
          onCancel={() => setDeleteConnConfirm(null)}
        />
      )}
      {resetHostKeyConfirm && (
        <ConfirmDialog
          title={resetHostKeyConfirm.title}
          message={resetHostKeyConfirm.message}
          confirmLabel={resetHostKeyConfirm.confirmLabel}
          danger={resetHostKeyConfirm.danger}
          onConfirm={confirmResetHostKey}
          onCancel={() => setResetHostKeyConfirm(null)}
        />
      )}
      {deleteFolderConfirm && (
        <ConfirmDialog
          title={deleteFolderConfirm.title}
          message={deleteFolderConfirm.message}
          confirmLabel={deleteFolderConfirm.confirmLabel}
          danger={deleteFolderConfirm.danger}
          onConfirm={confirmDeleteFolder}
          onCancel={() => setDeleteFolderConfirm(null)}
        />
      )}
      {/* Right-edge resize handle — drag horizontally to widen the sidebar so
          long connection names become visible. Mirrors AiPanel's resizer. Only
          rendered when the parent actually manages width. */}
      {onWidthChange && (
        <div
          onMouseDown={onResizeStart}
          title="拖动调整侧栏宽度"
          style={{
            position: "absolute",
            right: 0,
            top: 0,
            bottom: 0,
            width: 5,
            cursor: "col-resize",
            zIndex: 6,
            // A faint hit area; brighten on hover so the affordance is discoverable.
            background: "transparent",
            transition: "background var(--duration-fast) var(--ease-in-out)",
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "var(--accent-primary-muted)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "transparent";
          }}
        />
      )}
      {toggleBtn}
    </div>
  );
}

const FolderRow = memo(function FolderRow({
  node,
  isOpen,
  isDropTarget,
  onToggle,
  onContext,
}: {
  node: FolderNode;
  isOpen: boolean;
  isDropTarget?: boolean;
  onToggle: () => void;
  onContext: (e: React.MouseEvent) => void;
}) {
  return (
    <div
      data-folder-path={node.path}
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
        // Drop target highlight — only set when isDropTarget is true; left
        // as `transparent` otherwise so the existing onMouseEnter/Leave hover
        // background still works (it writes e.currentTarget.style.background).
        background: isDropTarget ? "var(--accent-primary-muted)" : "transparent",
        boxShadow: isDropTarget ? "inset 0 0 0 1px var(--border-accent)" : "none",
        transition: "background var(--duration-fast) var(--ease-out-expo)",
        borderRadius: "0 var(--radius-md) var(--radius-md) 0",
        marginRight: 8,
      }}
      onMouseEnter={(e) => {
        if (!isDropTarget) e.currentTarget.style.background = "var(--bg-surface-hover)";
      }}
      onMouseLeave={(e) => {
        if (!isDropTarget) e.currentTarget.style.background = "transparent";
      }}
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
      <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={node.name}>
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
});

const ConnRow = memo(function ConnRow({
  conn,
  depth,
  showGroupPath,
  draggingConnId,
  onPointerDown,
  onConnect,
  onContext,
  onEdit,
  onCopy,
  onDelete,
  onResetHostKey,
  onRename,
}: {
  conn: ConnectionConfig;
  depth: number;
  showGroupPath?: boolean;
  draggingConnId?: string | null;
  onPointerDown?: (e: React.PointerEvent) => void;
  onConnect: () => void;
  onContext: (e: React.MouseEvent) => void;
  onEdit: () => void;
  onCopy: () => void;
  onDelete: () => void;
  onResetHostKey: () => void;
  onRename: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);
  const connType = (conn.conn_type as ConnType) || "ssh";

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

  // True while THIS row is the one being dragged. We set pointerEvents:none so
  // document.elementFromPoint sees the folder beneath it instead of this row,
  // and dim+raise it as the "you're dragging this" affordance.
  const isThisDragging = draggingConnId === conn.id;

  return (
    <>
      <div
        onPointerDown={onPointerDown}
        onDoubleClick={onConnect}
        onContextMenu={onContext}
        style={{
          padding: "8px 16px 8px 24",
          paddingLeft: 24 + depth * 14,
          cursor: isThisDragging ? "grabbing" : "grab",
          display: "flex",
          alignItems: "center",
          gap: 10,
          fontSize: 13,
          color: "var(--text-primary)",
          opacity: isThisDragging ? 0.4 : 1,
          pointerEvents: isThisDragging ? "none" : "auto",
          transform: isThisDragging ? "scale(1.02)" : "none",
          boxShadow: isThisDragging ? "var(--shadow-glow)" : "none",
          transition: "background var(--duration-fast) var(--ease-in-out)",
          borderRadius: "0 var(--radius-md) var(--radius-md) 0",
          marginRight: 8,
        }}
        onMouseEnter={(e) => {
          if (!isThisDragging) e.currentTarget.style.background = "var(--bg-surface-hover)";
        }}
        onMouseLeave={(e) => {
          if (!isThisDragging) e.currentTarget.style.background = "transparent";
        }}
      >
        <ConnIcon connType={connType} size={16} style={{ opacity: 0.85 }} />
        <div style={{ flex: 1, overflow: "hidden", display: "flex", flexDirection: "column", gap: 2 }}
          title={`${conn.name}${conn.host ? `\n${conn.host}${conn.port ? `:${conn.port}` : ""}` : ""}`}
        >
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
          onPointerDown={(e) => e.stopPropagation()}
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
            <MenuItem icon="🏷️" label="重命名" onClick={() => { onRename(); setOpen(false); }} />
            <MenuItem icon="📋" label="复制" onClick={() => { onCopy(); setOpen(false); }} />
            {(connType === "ssh" || connType === "sftp") && (
              <MenuItem icon="🔑" label="重置主机密钥信任" onClick={() => { onResetHostKey(); setOpen(false); }} />
            )}
            <MenuItem icon="🗑️" label="删除" danger onClick={() => { onDelete(); setOpen(false); }} />
          </div>
        </>
      )}
    </>
  );
});

// Banner shown at the top of the connection list while a long-press drag is
// active. Tells the user (a) that dragging moves the connection into a folder,
// and (b) the current drop target, so they're never guessing what the gesture
// does or where they'll land.
function DragHint({
  hoverFolderPath,
  sourceConnName,
}: {
  hoverFolderPath: string | null;
  sourceConnName: string;
}) {
  const trimmed = hoverFolderPath && hoverFolderPath !== "/"
    ? hoverFolderPath.startsWith("/")
      ? hoverFolderPath.slice(1)
      : hoverFolderPath
    : null;
  return (
    <div
      style={{
        margin: "4px 8px 8px 8px",
        padding: "8px 12px",
        background: "var(--accent-primary-muted)",
        border: "1px solid var(--border-accent)",
        borderRadius: "var(--radius-md)",
        fontSize: 11,
        lineHeight: 1.5,
        color: "var(--text-primary)",
        display: "flex",
        flexDirection: "column",
        gap: 2,
        animation: "fadeIn var(--duration-normal) var(--ease-out-expo)",
      }}
    >
      <span style={{ fontWeight: 600, color: "var(--accent-primary)" }}>
        正在移动「{sourceConnName}」
      </span>
      <span style={{ color: "var(--text-secondary)" }}>
        拖到文件夹上松开即可移动到该文件夹；ESC 或松开在空白处取消
      </span>
      {trimmed ? (
        <span style={{ color: "var(--text-tertiary)" }}>
          当前目标：<span style={{ color: "var(--accent-secondary)" }}>📁 {trimmed}</span>
        </span>
      ) : (
        <span style={{ color: "var(--text-muted)" }}>
          当前目标：移到文件夹上以选择
        </span>
      )}
    </div>
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
