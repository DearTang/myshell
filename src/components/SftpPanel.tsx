import { useState, useEffect, useCallback } from "react";
import {
  sftpListDir,
  sftpMkdir,
  sftpRemove,
  sftpRename,
  ftpListDir,
  ftpMkdir,
  ftpRemove,
  ftpRename,
} from "../api";
import type { FileEntry } from "../api";

interface Props {
  sessionId: string;
  /** "ssh" routes through the russh-sftp subsystem (default). "ftp" routes
   * through suppaftp — both share the same UI surface so the user can reuse
   * muscle memory across connection types. */
  source?: "ssh" | "ftp";
  /** When true, the panel stretches to fill its parent — used when an FTP/SFTP
   * connection opens in its own tab. Default (side panel) is fixed-width. */
  fullHeight?: boolean;
}

export function SftpPanel({ sessionId, source = "ssh", fullHeight = false }: Props) {
  // FTP servers don't understand "~" — use "/" as the natural root. SSH/SFTP
  // honors "~" for the home directory shortcut.
  const initialPath = source === "ftp" ? "/" : "~";
  const [currentPath, setCurrentPath] = useState(initialPath);
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<string[]>([initialPath]);
  const [historyIndex, setHistoryIndex] = useState(0);
  const [newFolderName, setNewFolderName] = useState("");
  const [showMkdir, setShowMkdir] = useState(false);

  const dispatch = {
    list: source === "ftp" ? ftpListDir : sftpListDir,
    mkdir: source === "ftp" ? ftpMkdir : sftpMkdir,
    remove:
      source === "ftp"
        ? (sid: string, path: string) => ftpRemove(sid, path, false)
        : sftpRemove,
    rmdir:
      source === "ftp"
        ? (sid: string, path: string) => ftpRemove(sid, path, true)
        : sftpRemove,
    rename: source === "ftp" ? ftpRename : sftpRename,
  };

  const loadDir = useCallback(
    async (path: string) => {
      setLoading(true);
      setError(null);
      try {
        const files = await dispatch.list(sessionId, path);
        setEntries(files);
        setCurrentPath(path);
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    },
    [sessionId, source]
  );

  useEffect(() => {
    loadDir(initialPath);
  }, [loadDir, initialPath]);

  function navigateTo(path: string) {
    const newHistory = history.slice(0, historyIndex + 1);
    newHistory.push(path);
    setHistory(newHistory);
    setHistoryIndex(newHistory.length - 1);
    loadDir(path);
  }

  function goBack() {
    if (historyIndex > 0) {
      const newIndex = historyIndex - 1;
      setHistoryIndex(newIndex);
      loadDir(history[newIndex]);
    }
  }

  function goForward() {
    if (historyIndex < history.length - 1) {
      const newIndex = historyIndex + 1;
      setHistoryIndex(newIndex);
      loadDir(history[newIndex]);
    }
  }

  function goUp() {
    const parts = currentPath.replace(/\/$/, "").split("/");
    if (parts.length > 1) {
      const parent = parts.slice(0, -1).join("/") || "/";
      navigateTo(parent);
    }
  }

  async function handleMkdir() {
    if (!newFolderName.trim()) return;
    const fullPath = currentPath.endsWith("/")
      ? currentPath + newFolderName
      : currentPath + "/" + newFolderName;
    try {
      await dispatch.mkdir(sessionId, fullPath);
      setShowMkdir(false);
      setNewFolderName("");
      loadDir(currentPath);
    } catch (e) {
      alert(`创建失败: ${e}`);
    }
  }

  async function handleDelete(entry: FileEntry) {
    if (!confirm(`确认删除 "${entry.name}"？`)) return;
    try {
      if (entry.is_dir) {
        await dispatch.rmdir(sessionId, entry.path);
      } else {
        await dispatch.remove(sessionId, entry.path);
      }
      loadDir(currentPath);
    } catch (e) {
      alert(`删除失败: ${e}`);
    }
  }

  async function handleRename(entry: FileEntry) {
    const newName = prompt("输入新名称", entry.name);
    if (!newName || newName === entry.name) return;
    const parentDir = currentPath.endsWith("/") ? currentPath : currentPath + "/";
    const newPath = parentDir + newName;
    try {
      await dispatch.rename(sessionId, entry.path, newPath);
      loadDir(currentPath);
    } catch (e) {
      alert(`重命名失败: ${e}`);
    }
  }

  function formatSize(bytes: number): string {
    if (bytes === 0) return "-";
    const units = ["B", "KB", "MB", "GB"];
    let i = 0;
    let size = bytes;
    while (size >= 1024 && i < units.length - 1) {
      size /= 1024;
      i++;
    }
    return `${size.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
  }

  return (
    <div
      style={{
        width: fullHeight ? "100%" : 320,
        minWidth: fullHeight ? 0 : 320,
        background: "var(--bg-sidebar)",
        borderLeft: fullHeight ? "none" : "1px solid var(--border)",
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
      }}
    >
      {/* Toolbar */}
      <div
        style={{
          padding: "8px 10px",
          borderBottom: "1px solid var(--border)",
          display: "flex",
          alignItems: "center",
          gap: 4,
        }}
      >
        <ToolBtn onClick={goBack} title="后退" disabled={historyIndex <= 0}>
          ←
        </ToolBtn>
        <ToolBtn onClick={goForward} title="前进" disabled={historyIndex >= history.length - 1}>
          →
        </ToolBtn>
        <ToolBtn onClick={goUp} title="上级目录">
          ↑
        </ToolBtn>
        <ToolBtn onClick={() => loadDir(currentPath)} title="刷新">
          ↻
        </ToolBtn>
        <ToolBtn onClick={() => setShowMkdir((v) => !v)} title="新建文件夹">
          📁+
        </ToolBtn>
      </div>

      {/* Path Bar */}
      <div style={{ padding: "6px 10px", borderBottom: "1px solid var(--border)" }}>
        <input
          value={currentPath}
          onChange={(e) => setCurrentPath(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") navigateTo(currentPath);
          }}
          style={{
            width: "100%",
            background: "var(--bg-input)",
            border: "1px solid var(--border)",
            borderRadius: 4,
            padding: "4px 8px",
            color: "var(--text-primary)",
            fontSize: 12,
          }}
        />
      </div>

      {/* Mkdir Input */}
      {showMkdir && (
        <div style={{ padding: "6px 10px", borderBottom: "1px solid var(--border)", display: "flex", gap: 4 }}>
          <input
            value={newFolderName}
            onChange={(e) => setNewFolderName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleMkdir();
              if (e.key === "Escape") setShowMkdir(false);
            }}
            placeholder="文件夹名称"
            autoFocus
            style={{
              flex: 1,
              background: "var(--bg-input)",
              border: "1px solid var(--border)",
              borderRadius: 4,
              padding: "3px 8px",
              color: "var(--text-primary)",
              fontSize: 12,
            }}
          />
          <button
            onClick={handleMkdir}
            style={{
              background: "var(--accent)",
              color: "var(--bg-panel)",
              border: "none",
              borderRadius: 4,
              padding: "3px 8px",
              fontSize: 11,
            }}
          >
            创建
          </button>
        </div>
      )}

      {/* File List */}
      <div style={{ flex: 1, overflowY: "auto" }}>
        {loading && (
          <div style={{ padding: 16, textAlign: "center", color: "var(--text-muted)", fontSize: 12 }}>
            加载中...
          </div>
        )}
        {error && (
          <div style={{ padding: 16, textAlign: "center", color: "var(--error)", fontSize: 12 }}>
            {error}
          </div>
        )}
        {!loading &&
          !error &&
          entries.map((entry) => (
            <div
              key={entry.path}
              onDoubleClick={() => {
                if (entry.is_dir) navigateTo(entry.path);
              }}
              style={{
                padding: "5px 10px",
                display: "flex",
                alignItems: "center",
                gap: 8,
                fontSize: 12,
                cursor: "pointer",
                color: "var(--text-primary)",
              }}
              onMouseEnter={(e) => (e.currentTarget.style.background = "var(--bg-input)")}
              onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
            >
              <span style={{ fontSize: 14, flexShrink: 0 }}>
                {entry.is_dir ? "📁" : "📄"}
              </span>
              <span
                style={{
                  flex: 1,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                }}
              >
                {entry.name}
              </span>
              <span style={{ color: "var(--text-muted)", fontSize: 11, flexShrink: 0 }}>
                {formatSize(entry.size)}
              </span>
              <span
                onClick={(e) => {
                  e.stopPropagation();
                  handleRename(entry);
                }}
                title="重命名"
                style={{ fontSize: 11, opacity: 0.4, cursor: "pointer", flexShrink: 0 }}
              >
                ✏
              </span>
              <span
                onClick={(e) => {
                  e.stopPropagation();
                  handleDelete(entry);
                }}
                title="删除"
                style={{ fontSize: 11, opacity: 0.4, cursor: "pointer", color: "var(--error)", flexShrink: 0 }}
              >
                ✕
              </span>
            </div>
          ))}
        {!loading && !error && entries.length === 0 && (
          <div style={{ padding: 16, textAlign: "center", color: "var(--text-muted)", fontSize: 12 }}>
            空目录
          </div>
        )}
      </div>
    </div>
  );
}

function ToolBtn({
  children,
  onClick,
  title,
  disabled,
}: {
  children: React.ReactNode;
  onClick: () => void;
  title: string;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      disabled={disabled}
      style={{
        background: "transparent",
        color: disabled ? "var(--text-muted)" : "var(--text-secondary)",
        padding: "3px 6px",
        fontSize: 13,
        borderRadius: 4,
        opacity: disabled ? 0.4 : 1,
        cursor: disabled ? "default" : "pointer",
      }}
    >
      {children}
    </button>
  );
}
