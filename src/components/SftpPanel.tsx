import { useState, useEffect, useCallback, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  sftpListDir,
  sftpMkdir,
  sftpRemove,
  sftpRename,
  sftpUpload,
  sftpDownload,
  sftpCancelTransfer,
  onSftpTransferProgress,
  onSftpTransferDone,
  onSshClosed,
  ftpListDir,
  ftpMkdir,
  ftpRemove,
  ftpRename,
} from "../api";
import type { FileEntry, SftpTransferProgressPayload } from "../api";
import { getSftpDownloadConcurrency } from "../utils/transfer-settings";

interface Props {
  sessionId: string;
  /** "ssh" routes through the russh-sftp subsystem (default). "ftp" routes
   * through suppaftp — both share the same UI surface so the user can reuse
   * muscle memory across connection types. */
  source?: "ssh" | "ftp";
  /** When true, the panel stretches to fill its parent — used when an FTP/SFTP
   * connection opens in its own tab. Default (side panel) is fixed-width. */
  fullHeight?: boolean;
  /** Called when the SSH channel closes (server EOF / shell exit / network drop).
   * Wired to App.tsx to flip the tab's status dot from green to red. */
  onDisconnected?: () => void;
  /** Connection status from the parent tab — drives the reconnect overlay. */
  status?: "connecting" | "connected" | "disconnected" | "error";
  /** Called when the user clicks the reconnect button in the disconnected overlay. */
  onReconnect?: () => void;
}

/** In-flight or finished transfer shown in the overlay. */
interface TransferState {
  phase: "upload" | "download";
  currentFile: string;
  fileIndex: number;
  fileCount: number;
  bytesDone: number;
  bytesTotal: number;
  errors: string[];
  done: boolean;
  startTime: number;
  requestId: string;
}

function formatClock(epochMs: number): string {
  if (!epochMs) return "—";
  const d = new Date(epochMs);
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map((n) => String(n).padStart(2, "0"))
    .join(":");
}

function formatDuration(seconds: number): string {
  if (!isFinite(seconds) || seconds <= 0) return "—";
  if (seconds < 60) return `${Math.ceil(seconds)}秒`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = Math.ceil(seconds % 60);
    return `${m}分${s}秒`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}小时${m}分`;
}

export function SftpPanel({ sessionId, source = "ssh", fullHeight = false, onDisconnected, status, onReconnect }: Props) {
  // FTP servers don't understand "~" — use "/" as the natural root. SSH/SFTP
  // honors "~" for the home directory shortcut (resolved server-side).
  const initialPath = source === "ftp" ? "/" : "~";
  const [currentPath, setCurrentPath] = useState(initialPath);
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<string[]>([initialPath]);
  const [historyIndex, setHistoryIndex] = useState(0);
  const [newFolderName, setNewFolderName] = useState("");
  const [showMkdir, setShowMkdir] = useState(false);
  // Multi-select for batch download. Stores entry.path so it survives sorting.
  const [selected, setSelected] = useState<Set<string>>(new Set());
  // Active transfer overlay state (null = no overlay).
  const [transfer, setTransfer] = useState<TransferState | null>(null);
  // Unlisteners for the active transfer's events — cleared on overlay close.
  const transferUnlistenRef = useRef<Array<() => void>>([]);
  // 1s tick to keep elapsed/ETA live in the transfer overlay.
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!transfer || transfer.done) return;
    const id = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(id);
  }, [transfer?.done, transfer?.startTime]);

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

  // Clear selection when the directory changes — stale paths would be invalid.
  useEffect(() => {
    setSelected(new Set());
  }, [currentPath]);

  // Cleanup any active transfer listeners on unmount.
  useEffect(() => {
    return () => {
      transferUnlistenRef.current.forEach((u) => u());
      transferUnlistenRef.current = [];
    };
  }, []);

  // Subscribe to ssh_closed so the tab's status dot flips to red when the
  // underlying SSH channel dies (server EOF, shell exit, network drop).
  // Without this, SFTP tabs stay green forever because only TerminalPanel
  // subscribed to ssh_closed — SftpPanel rendered in its own tab never heard it.
  useEffect(() => {
    if (source !== "ssh") return;
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    onSshClosed(sessionId, () => {
      onDisconnected?.();
    }).then((u) => {
      if (cancelled) {
        // Component unmounted before the promise resolved — clean up immediately.
        u();
      } else {
        unlisten = u;
      }
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [sessionId, source, onDisconnected]);

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

  function toggleSelected(path: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  // Download accepts files AND folders — folders are expanded recursively
  // on the Rust side (subtree mirrors under <dest>/<folder-name>/...).
  const selectedFileEntries = entries.filter((e) => selected.has(e.path));

  /** Wire up progress/done listeners, run the transfer, then refresh if asked.
   * Shared by upload + download so the overlay lifecycle lives in one place. */
  async function runTransfer(
    phase: "upload" | "download",
    start: (requestId: string) => Promise<void>,
    refreshAfter: boolean
  ) {
    const requestId = crypto.randomUUID();
    setTransfer({
      phase,
      currentFile: "",
      fileIndex: 0,
      fileCount: 0,
      bytesDone: 0,
      bytesTotal: 0,
      errors: [],
      done: false,
      startTime: Date.now(),
      requestId,
    });
    // Subscribe BEFORE invoking so the earliest progress events land.
    const unP = await onSftpTransferProgress(requestId, (p: SftpTransferProgressPayload) => {
      setTransfer((t) =>
        t
          ? {
              ...t,
              currentFile: p.currentFile,
              fileIndex: p.fileIndex,
              fileCount: p.fileCount,
              bytesDone: p.bytesDone,
              bytesTotal: p.bytesTotal,
            }
          : t
      );
    });
    const unD = await onSftpTransferDone(requestId, (errors) => {
      setTransfer((t) => (t ? { ...t, done: true, errors } : t));
    });
    transferUnlistenRef.current = [unP, unD];
    try {
      await start(requestId);
      // Backend emits `done` right before returning Ok; if the listener hasn't
      // processed it yet (async), mark done here so the overlay finalizes.
      setTransfer((t) => (t && !t.done ? { ...t, done: true } : t));
    } catch (e) {
      setTransfer((t) =>
        t ? { ...t, done: true, errors: [...t.errors, String(e)] } : t
      );
    } finally {
      // Reset selection after transfer completes (download selects files
      // via checkboxes — stale checks after download are confusing).
      setSelected(new Set());
      if (refreshAfter) loadDir(currentPath);
    }
  }

  async function handleUpload() {
    const picked = await open({ multiple: true, title: "选择要上传的文件" });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    if (paths.length === 0) return;
    await runTransfer(
      "upload",
      (rid) => sftpUpload(sessionId, paths, currentPath, rid),
      true
    );
  }

  async function handleDownload() {
    if (selectedFileEntries.length === 0) {
      alert("请先勾选要下载的文件或文件夹");
      return;
    }
    const dest = await open({ directory: true, title: "选择保存位置" });
    if (!dest) return;
    const destDir = typeof dest === "string" ? dest : Array.isArray(dest) ? dest[0] : "";
    if (!destDir) return;
    const paths = selectedFileEntries.map((e) => e.path);
    await runTransfer(
      "download",
      (rid) => sftpDownload(sessionId, paths, destDir, rid, getSftpDownloadConcurrency()),
      false
    );
  }

  function closeTransfer() {
    transferUnlistenRef.current.forEach((u) => u());
    transferUnlistenRef.current = [];
    setTransfer(null);
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

  /** Derive a short type label from the file name extension. */
  function fileType(name: string, isDir: boolean): string {
    if (isDir) return "目录";
    const dot = name.lastIndexOf(".");
    if (dot <= 0 || dot === name.length - 1) return "文件";
    const ext = name.slice(dot + 1).toLowerCase();
    const map: Record<string, string> = {
      sh: "Shell", bash: "Shell", py: "Python", rb: "Ruby", pl: "Perl",
      js: "JS", ts: "TS", jsx: "JSX", tsx: "TSX", vue: "Vue", svelte: "Svelte",
      rs: "Rust", go: "Go", java: "Java", kt: "Kotlin", c: "C", cpp: "C++",
      h: "Header", cs: "C#", swift: "Swift", php: "PHP", lua: "Lua",
      html: "HTML", htm: "HTML", css: "CSS", scss: "SCSS", less: "Less",
      json: "JSON", yaml: "YAML", yml: "YAML", toml: "TOML", xml: "XML",
      md: "Markdown", txt: "文本", log: "日志", csv: "CSV", sql: "SQL",
      gz: "压缩", zip: "压缩", tar: "压缩", tgz: "压缩", "7z": "压缩", rar: "压缩",
      png: "图片", jpg: "图片", jpeg: "图片", gif: "图片", svg: "图片", webp: "图片", ico: "图片",
      mp3: "音频", wav: "音频", flac: "音频", aac: "音频", ogg: "音频",
      mp4: "视频", mkv: "视频", avi: "视频", mov: "视频", webm: "视频",
      pdf: "PDF", doc: "Word", docx: "Word", xls: "Excel", xlsx: "Excel",
      ppt: "PPT", pptx: "PPT",
      conf: "配置", cfg: "配置", ini: "配置", env: "配置", service: "配置",
      pem: "证书", key: "证书", crt: "证书", cert: "证书",
      dockerfile: "Docker", gitignore: "Git",
    };
    return map[ext] || ext.toUpperCase();
  }

  /** Format a Unix timestamp (seconds) to a readable local time string. */
  function formatTime(ts: number): string {
    if (!ts) return "-";
    const d = new Date(ts * 1000);
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }

  /** Truncate a permissions string (e.g. "drwxr-xr-x") to fit. */
  function truncatePerm(p: string): string {
    if (p.length <= 10) return p;
    return p.slice(0, 10);
  }

  const pctDone =
    transfer && transfer.bytesTotal > 0
      ? Math.min(100, Math.round((transfer.bytesDone / transfer.bytesTotal) * 100))
      : 0;

  return (
    <div
      style={{
        position: "relative",
        width: fullHeight ? "100%" : 360,
        minWidth: fullHeight ? 0 : 360,
        background: "var(--bg-elevated)",
        borderLeft: fullHeight ? "none" : "1px solid var(--border-default)",
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
      }}
    >
      {/* Toolbar */}
      <div
        style={{
          padding: "6px 8px",
          borderBottom: "1px solid var(--border-default)",
          display: "flex",
          alignItems: "center",
          gap: 2,
          flexWrap: "wrap",
        }}
      >
        <NavBtn onClick={goBack} title="后退" disabled={historyIndex <= 0}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M15 18l-6-6 6-6"/></svg>
        </NavBtn>
        <NavBtn onClick={goForward} title="前进" disabled={historyIndex >= history.length - 1}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M9 18l6-6-6-6"/></svg>
        </NavBtn>
        <NavBtn onClick={goUp} title="上级目录">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M18 15l-6-6-6 6"/></svg>
        </NavBtn>
        <NavBtn onClick={() => loadDir(currentPath)} title="刷新">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M23 4v6h-6"/><path d="M1 20v-6h6"/><path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15"/></svg>
        </NavBtn>
        <div style={{ width: 1, height: 20, background: "var(--border-default)", margin: "0 4px" }} />
        <NavBtn onClick={() => setShowMkdir((v) => !v)} title="新建文件夹">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M22 19a2 2 0 01-2 2H4a2 2 0 01-2-2V5a2 2 0 012-2h5l2 3h9a2 2 0 012 2z"/><line x1="12" y1="11" x2="12" y2="17"/><line x1="9" y1="14" x2="15" y2="14"/></svg>
        </NavBtn>
        {source === "ssh" && (
          <>
            <div style={{ width: 1, height: 20, background: "var(--border-default)", margin: "0 4px" }} />
            <NavBtn onClick={handleUpload} title="上传文件（可多选）">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
            </NavBtn>
            <NavBtn
              onClick={handleDownload}
              title={
                selectedFileEntries.length > 0
                  ? `下载 ${selectedFileEntries.length} 个选中文件`
                  : "下载选中文件（先勾选文件）"
              }
              disabled={selectedFileEntries.length === 0}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
              {selectedFileEntries.length > 0 && (
                <span style={{ marginLeft: 2, fontSize: 11 }}>{selectedFileEntries.length}</span>
              )}
            </NavBtn>
          </>
        )}
      </div>

      {/* Path Bar */}
      <div style={{ padding: "5px 8px", borderBottom: "1px solid var(--border-default)", display: "flex", alignItems: "center", gap: 6 }}>
        {/* Terminal/server prefix — reinforces "remote filesystem" vs sidebar's local tree */}
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="var(--text-muted)" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
          <polyline points="4 5 7 8 4 11"/><line x1="9" y1="11" x2="12" y2="11"/>
        </svg>
        <input
          value={currentPath}
          onChange={(e) => setCurrentPath(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") navigateTo(currentPath);
          }}
          style={{
            flex: 1,
            minWidth: 0,
            background: "var(--bg-input)",
            border: "1px solid var(--border-default)",
            borderRadius: 4,
            padding: "4px 8px",
            color: "var(--text-primary)",
            fontSize: 11,
            fontFamily: "'JetBrains Mono', 'Cascadia Code', 'Fira Code', ui-monospace, monospace",
            letterSpacing: "-0.01em",
          }}
        />
      </div>

      {/* Mkdir Input */}
      {showMkdir && (
        <div style={{ padding: "5px 8px", borderBottom: "1px solid var(--border-default)", display: "flex", gap: 4 }}>
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
              border: "1px solid var(--border-default)",
              borderRadius: 4,
              padding: "3px 8px",
              color: "var(--text-primary)",
              fontSize: 12,
            }}
          />
          <button
            onClick={handleMkdir}
            style={{
              background: "var(--accent-primary)",
              color: "var(--text-inverse)",
              border: "none",
              borderRadius: 4,
              padding: "3px 10px",
              fontSize: 11,
              cursor: "pointer",
            }}
          >
            创建
          </button>
        </div>
      )}

      {/* Column Headers */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          padding: "6px 10px 6px 12px",
          borderBottom: "1px solid var(--border-subtle)",
          fontSize: 10,
          fontWeight: 600,
          letterSpacing: "0.06em",
          textTransform: "uppercase",
          color: "var(--text-muted)",
          gap: 10,
          userSelect: "none",
          flexShrink: 0,
        }}
      >
        {source === "ssh" && <span style={{ width: 18, flexShrink: 0 }} />}
        <span style={{ width: 20, flexShrink: 0 }} />
        <span style={{ flex: 1, minWidth: 0 }}>名称</span>
        <span style={{ width: 64, textAlign: "right", flexShrink: 0 }}>大小</span>
        <span style={{ width: 50, textAlign: "center", flexShrink: 0 }}>类型</span>
        <span style={{ width: 110, textAlign: "right", flexShrink: 0 }}>修改时间</span>
        <span style={{ width: 76, textAlign: "right", flexShrink: 0 }}>权限</span>
        <span style={{ width: 36, flexShrink: 0 }} />
      </div>

      {/* File List */}
      <div style={{ flex: 1, overflowY: "auto" }}>
        {loading && (
          <div style={{ padding: 24, textAlign: "center", color: "var(--text-muted)", fontSize: 12 }}>
            加载中...
          </div>
        )}
        {error && (
          <div style={{ padding: 24, textAlign: "center", color: "var(--error)", fontSize: 12 }}>
            {error}
          </div>
        )}
        {!loading &&
          !error &&
          entries.map((entry) => {
            const isSelected = selected.has(entry.path);
            return (
            <div
              key={entry.path}
              onDoubleClick={() => {
                if (entry.is_dir) navigateTo(entry.path);
              }}
              style={{
                padding: "5px 10px 5px 12px",
                display: "flex",
                alignItems: "center",
                gap: 10,
                fontSize: 12,
                cursor: "pointer",
                color: "var(--text-primary)",
                background: isSelected ? "var(--accent-primary-muted)" : "transparent",
                transition: "background 120ms cubic-bezier(0.4, 0, 0.2, 1)",
              }}
              onMouseEnter={(e) => {
                if (!isSelected) e.currentTarget.style.background = "var(--bg-surface-hover)";
              }}
              onMouseLeave={(e) => {
                if (!isSelected) e.currentTarget.style.background = "transparent";
              }}
            >
              {/* Checkbox — files only (folders aren't transferable, files-only contract). */}
              {source === "ssh" && (
                <span style={{ width: 18, flexShrink: 0, display: "flex", alignItems: "center" }}>
                  {!entry.is_dir && (
                    <input
                      type="checkbox"
                      checked={isSelected}
                      onChange={() => toggleSelected(entry.path)}
                      onClick={(e) => e.stopPropagation()}
                      style={{ margin: 0, accentColor: "var(--accent-primary)" }}
                    />
                  )}
                </span>
              )}
              {/* Icon — matches TerminalPanel/SSH accent palette so SFTP and SSH
                  tabs read as one surface. Dirs use accent-primary (blue), files
                  use a neutral muted glyph. Both are 1px-stroke line icons to
                  keep the grid calm and legible at 12px. */}
              <span style={{ width: 20, flexShrink: 0, display: "flex", alignItems: "center", justifyContent: "center", color: entry.is_dir ? "var(--accent-primary)" : "var(--text-tertiary)" }}>
                {entry.is_dir ? (
                  <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
                    <path d="M1.5 4.5A1.5 1.5 0 013 3h3.2a1.5 1.5 0 011.06.44L8.28 4.5H13A1.5 1.5 0 0114.5 6v5.5A1.5 1.5 0 0113 13H3A1.5 1.5 0 011.5 11.5v-7z" fill="var(--accent-primary-muted)" stroke="currentColor" strokeWidth="1" strokeLinejoin="round"/>
                  </svg>
                ) : (
                  <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
                    <path d="M3.5 1.5A1.5 1.5 0 015 0h4.2L14 4.8v8.7A1.5 1.5 0 0112.5 15h-8A1.5 1.5 0 013 13.5v-12A1.5 1.5 0 014.5 0H5z" fill="rgba(139,148,158,0.06)" stroke="currentColor" strokeWidth="1" strokeLinejoin="round"/>
                    <path d="M9 0v4h4" stroke="currentColor" strokeWidth="1" strokeLinejoin="round"/>
                  </svg>
                )}
              </span>
              {/* Name — dirs get accent-primary color + medium weight to match the
                  SSH visual grammar (accent = navigable/interactive), files use
                  primary text. Both share the same body font as the rest of the
                  app — no serif/display mismatch. */}
              <span
                style={{
                  flex: 1,
                  minWidth: 0,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  color: entry.is_dir ? "var(--accent-primary)" : "var(--text-primary)",
                  fontWeight: entry.is_dir ? 500 : 400,
                  fontSize: 12,
                  letterSpacing: "0.01em",
                }}
              >
                {entry.name}
              </span>
              {/* Size — right-aligned, muted secondary. Directories show dash.
                  tabular-nums keeps the column visually aligned. */}
              <span
                style={{
                  width: 64,
                  textAlign: "right",
                  flexShrink: 0,
                  color: entry.is_dir ? "var(--text-muted)" : "var(--text-secondary)",
                  fontSize: 11,
                  fontVariantNumeric: "tabular-nums",
                }}
              >
                {entry.is_dir ? "—" : formatSize(entry.size)}
              </span>
              {/* Type — subtle text label (no badge). Calmer than the old pill;
                  keeps focus on the filename. */}
              <span
                style={{
                  width: 50,
                  textAlign: "center",
                  flexShrink: 0,
                  fontSize: 10,
                  letterSpacing: "0.02em",
                  color: entry.is_dir ? "var(--accent-secondary)" : "var(--text-tertiary)",
                }}
              >
                {fileType(entry.name, entry.is_dir)}
              </span>
              {/* Modified — monospace for alignment */}
              <span
                style={{
                  width: 110,
                  textAlign: "right",
                  flexShrink: 0,
                  color: "var(--text-secondary)",
                  fontSize: 10,
                  fontFamily: "'JetBrains Mono', 'Cascadia Code', 'Fira Code', ui-monospace, monospace",
                  fontVariantNumeric: "tabular-nums",
                  letterSpacing: "-0.01em",
                }}
              >
                {entry.modified ? formatTime(Number(entry.modified)) : "—"}
              </span>
              {/* Permissions — monospace, dimmer */}
              <span
                style={{
                  width: 76,
                  textAlign: "right",
                  flexShrink: 0,
                  color: "var(--text-muted)",
                  fontSize: 10,
                  fontFamily: "'JetBrains Mono', 'Cascadia Code', 'Fira Code', ui-monospace, monospace",
                  letterSpacing: "-0.02em",
                }}
              >
                {entry.permissions ? truncatePerm(entry.permissions) : "—"}
              </span>
              {/* Actions */}
              <span style={{ width: 36, flexShrink: 0, display: "flex", alignItems: "center", gap: 4, justifyContent: "flex-end" }}>
                <span
                  onClick={(e) => {
                    e.stopPropagation();
                    handleRename(entry);
                  }}
                  title="重命名"
                  style={{ fontSize: 12, color: "var(--text-muted)", cursor: "pointer", lineHeight: 1, opacity: 0.6 }}
                  onMouseEnter={(e) => { e.currentTarget.style.opacity = "1"; e.currentTarget.style.color = "var(--text-primary)"; }}
                  onMouseLeave={(e) => { e.currentTarget.style.opacity = "0.6"; e.currentTarget.style.color = "var(--text-muted)"; }}
                >
                  ✎
                </span>
                <span
                  onClick={(e) => {
                    e.stopPropagation();
                    handleDelete(entry);
                  }}
                  title="删除"
                  style={{ fontSize: 13, cursor: "pointer", color: "var(--text-muted)", lineHeight: 1, opacity: 0.6 }}
                  onMouseEnter={(e) => { e.currentTarget.style.opacity = "1"; e.currentTarget.style.color = "var(--error)"; }}
                  onMouseLeave={(e) => { e.currentTarget.style.opacity = "0.6"; e.currentTarget.style.color = "var(--text-muted)"; }}
                >
                  ✕
                </span>
              </span>
            </div>
            );
          })}
        {!loading && !error && entries.length === 0 && (
          <div style={{ padding: 24, textAlign: "center", color: "var(--text-muted)", fontSize: 12 }}>
            空目录
          </div>
        )}
      </div>

      {/* Transfer overlay — 4-row layout matching ZmodemProgressOverlay */}
      {transfer && (() => {
        const elapsed = transfer.startTime > 0 ? (Date.now() - transfer.startTime) / 1000 : 0;
        const speed = elapsed > 1 ? transfer.bytesDone / elapsed : 0;
        const remaining = speed > 0 && transfer.bytesTotal > 0
          ? (transfer.bytesTotal - transfer.bytesDone) / speed
          : Infinity;
        return (
        <div
          style={{
            position: "absolute",
            left: 0,
            right: 0,
            bottom: 0,
            background: "rgba(30, 30, 46, 0.96)",
            borderTop: "1px solid var(--border-emphasis)",
            padding: "8px 16px 10px",
            fontSize: 12,
            fontFamily: "'Cascadia Code', Consolas, monospace",
            zIndex: 10,
            display: "flex",
            flexDirection: "column",
            gap: 6,
          }}
        >
          {/* Row 1: direction + filename + cancel/close */}
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: 12 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
              <span style={{ color: "#89b4fa", fontSize: 16, lineHeight: 1 }}>
                {transfer.phase === "upload" ? "↑" : "↓"}
              </span>
              <span style={{ color: "#a6e3a1", fontWeight: 600, whiteSpace: "nowrap" }}>
                {transfer.done
                  ? transfer.phase === "upload" ? "上传完成" : "下载完成"
                  : transfer.phase === "upload" ? "SFTP 上传" : "SFTP 下载"}
              </span>
              {transfer.fileCount > 1 && (
                <span style={{ color: "#6c7086", whiteSpace: "nowrap" }}>
                  {Math.min(transfer.fileIndex + (transfer.done ? 0 : 1), transfer.fileCount)}/{transfer.fileCount}
                </span>
              )}
              <span style={{ color: "#bac2de", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {transfer.currentFile || "准备中…"}
              </span>
            </div>
            {transfer.done ? (
              <button
                onClick={closeTransfer}
                style={{
                  background: "var(--accent-primary)",
                  color: "var(--text-inverse)",
                  border: "none",
                  borderRadius: 4,
                  padding: "2px 12px",
                  fontSize: 12,
                  cursor: "pointer",
                  whiteSpace: "nowrap",
                }}
              >
                关闭
              </button>
            ) : (
              <button
                onClick={() => sftpCancelTransfer(transfer.requestId).catch(() => {})}
                style={{
                  background: "transparent",
                  color: "#f38ba8",
                  border: "1px solid #f38ba8",
                  borderRadius: 4,
                  padding: "2px 12px",
                  fontSize: 12,
                  cursor: "pointer",
                  whiteSpace: "nowrap",
                }}
              >
                取消
              </button>
            )}
          </div>

          {/* Row 2: progress bar */}
          <div style={{ height: 8, background: "#313244", borderRadius: 4, overflow: "hidden" }}>
            <div
              style={{
                width: `${pctDone}%`,
                height: "100%",
                background: "linear-gradient(90deg, #89b4fa, #b4befe)",
                transition: "width 0.3s ease",
              }}
            />
          </div>

          {/* Row 3: bytes + percent + speed */}
          <div style={{ display: "flex", justifyContent: "space-between", color: "#a6adc8" }}>
            <span>
              {formatSize(transfer.bytesDone)} / {formatSize(transfer.bytesTotal)}
              {transfer.bytesTotal > 0 && ` (${pctDone}%)`}
            </span>
            <span>{speed >= 1 ? `${formatSize(speed)}/s` : "—"}</span>
          </div>

          {/* Row 4: timing */}
          <div style={{ display: "flex", justifyContent: "space-between", color: "#6c7086", fontSize: 11 }}>
            <span>开始 {formatClock(transfer.startTime)}</span>
            <span>已用 {formatDuration(elapsed)}</span>
            <span>剩余 {transfer.done ? "—" : formatDuration(remaining)}</span>
          </div>

          {/* Errors (only when done) */}
          {transfer.done && transfer.errors.length > 0 && (
            <div style={{ color: "#f38ba8", maxHeight: 42, overflowY: "auto", lineHeight: 1.4 }}>
              {transfer.errors.slice(0, 3).map((er, i) => (
                <div key={i}>• {er}</div>
              ))}
              {transfer.errors.length > 3 && (
                <div>…等 {transfer.errors.length} 个错误</div>
              )}
            </div>
          )}
        </div>
        );
      })()}

      {/* Disconnected overlay — shown when the SSH channel dies.
          Covers the file list so the user can't interact with a dead session. */}
      {(status === "disconnected" || status === "error") && (
        <div
          style={{
            position: "absolute",
            inset: 0,
            background: "rgba(0,0,0,0.55)",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            gap: 12,
            zIndex: 15,
          }}
        >
          <span style={{ color: "var(--text-secondary)", fontSize: 13 }}>
            {status === "error" ? "连接失败" : "连接已断开"}
          </span>
          {onReconnect && (
            <button
              onClick={onReconnect}
              style={{
                background: "var(--accent-primary)",
                color: "var(--text-inverse)",
                border: "none",
                borderRadius: 6,
                padding: "6px 20px",
                fontSize: 12,
                fontWeight: 500,
                cursor: "pointer",
              }}
            >
              重新连接
            </button>
          )}
        </div>
      )}
    </div>
  );
}
function NavBtn({
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
        padding: "5px 6px",
        fontSize: 13,
        borderRadius: 4,
        opacity: disabled ? 0.35 : 1,
        cursor: disabled ? "default" : "pointer",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        border: "1px solid transparent",
        transition: "background 0.12s, color 0.12s, border-color 0.12s",
      }}
      onMouseEnter={(e) => {
        if (!disabled) {
          e.currentTarget.style.background = "var(--bg-surface-hover)";
          e.currentTarget.style.color = "var(--text-primary)";
          e.currentTarget.style.borderColor = "var(--border-default)";
        }
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "transparent";
        e.currentTarget.style.color = disabled ? "var(--text-muted)" : "var(--text-secondary)";
        e.currentTarget.style.borderColor = "transparent";
      }}
    >
      {children}
    </button>
  );
}
