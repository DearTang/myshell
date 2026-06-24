import { useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  downloadUpdate,
  installUpdate,
  onUpdateDownloadProgress,
  openExternalUrl,
  type UpdateInfo,
} from "../api";

interface Props {
  updateInfo: UpdateInfo;
}

type Phase = "prompt" | "downloading" | "ready" | "failed";

/**
 * Centered modal update dialog.
 *
 * Shown once after login when the background check detects a newer version.
 * Two actions:
 *   - "更新" → auto-download the installer → progress bar → "安装并重启"
 *   - "当前版本忽略" → remember this version in localStorage; never prompt
 *     again for the same version. A newer version will re-trigger the dialog.
 *
 * Trust model: HTTPS (Gitee) + explicit user consent. On download/install
 * failure, falls back to a browser download button.
 */
export function UpdateNotification({ updateInfo }: Props) {
  const latest = updateInfo.latest_version;
  const downloadUrl = updateInfo.download_url || updateInfo.release_url || "";

  // Per-version ignore: once set, this version is permanently skipped until a
  // different (newer) version appears.
  const IGNORE_KEY = "myshell.ignoredUpdateVersion";
  const [ignored, setIgnored] = useState<boolean>(() => {
    try {
      return localStorage.getItem(IGNORE_KEY) === latest;
    } catch {
      return false;
    }
  });

  // If a new version replaces the ignored one, re-show the dialog.
  useEffect(() => {
    try {
      if (localStorage.getItem(IGNORE_KEY) !== latest) setIgnored(false);
    } catch {
      // ignore
    }
  }, [latest]);

  const [phase, setPhase] = useState<Phase>("prompt");
  const [progress, setProgress] = useState<{ downloaded: number; total: number }>({
    downloaded: 0,
    total: 0,
  });
  const [error, setError] = useState<string>("");
  const [downloadedPath, setDownloadedPath] = useState<string>("");

  // Subscribe to progress events while downloading.
  useEffect(() => {
    if (phase !== "downloading") return;
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;
    onUpdateDownloadProgress((p) => {
      setProgress({ downloaded: p.downloaded, total: p.total });
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [phase]);

  if (ignored) return null;

  const handleUpdate = async () => {
    setPhase("downloading");
    setProgress({ downloaded: 0, total: 0 });
    setError("");
    try {
      const path = await downloadUpdate(downloadUrl);
      setDownloadedPath(path);
      setPhase("ready");
    } catch (e) {
      setError(String(e));
      setPhase("failed");
    }
  };

  const handleInstall = async () => {
    if (!downloadedPath) {
      setError("安装包路径丢失");
      setPhase("failed");
      return;
    }
    try {
      // Does not return — the app exits inside installUpdate.
      await installUpdate(downloadedPath);
    } catch (e) {
      setError(String(e));
      setPhase("failed");
    }
  };

  const handleIgnore = () => {
    setIgnored(true);
    try {
      localStorage.setItem(IGNORE_KEY, latest);
    } catch {
      // best-effort
    }
  };

  const pct =
    progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : null;

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--bg-overlay)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 1500,
        backdropFilter: "blur(8px)",
      }}
    >
      <div
        className="animate-scale-in"
        style={{
          background: "var(--bg-elevated)",
          border: "1px solid var(--border-emphasis)",
          borderRadius: "var(--radius-xl)",
          width: 380,
          maxWidth: "90vw",
          boxShadow: "var(--shadow-xl)",
          overflow: "hidden",
        }}
      >
        {/* Header */}
        <div style={{ padding: "20px 24px 16px", textAlign: "center" }}>
          <div style={{ fontSize: 28, marginBottom: 10 }}>✨</div>
          <div style={{ fontSize: 16, fontWeight: 600, color: "var(--text-primary)" }}>
            发现新版本 v{latest}
          </div>
          <div style={{ fontSize: 12, color: "var(--text-tertiary)", marginTop: 4 }}>
            {phase === "prompt" && "是否更新？"}
            {phase === "downloading" && (pct !== null ? `下载中 ${pct}%` : "下载中…")}
            {phase === "ready" && "下载完成，可安装"}
            {phase === "failed" && (error || "下载失败")}
          </div>
        </div>

        {/* Progress bar */}
        {phase === "downloading" && (
          <div
            style={{
              height: 4,
              background: "var(--bg-surface)",
              margin: "0 24px 16px",
              borderRadius: "var(--radius-full)",
              overflow: "hidden",
            }}
          >
            <div
              style={{
                height: "100%",
                width: pct !== null ? `${pct}%` : "30%",
                background: "var(--accent-primary)",
                borderRadius: "var(--radius-full)",
                transition: "width 200ms linear",
              }}
            />
          </div>
        )}

        {/* Actions */}
        <div style={{ padding: "0 24px 20px", display: "flex", gap: 10 }}>
          {phase === "prompt" && (
            <>
              <button onClick={handleIgnore} style={btnGhost}>
                当前版本忽略
              </button>
              <button onClick={handleUpdate} style={btnPrimary}>
                更新
              </button>
            </>
          )}
          {phase === "downloading" && (
            <div style={{ flex: 1, textAlign: "center", fontSize: 12, color: "var(--text-muted)" }}>
              请稍候…
            </div>
          )}
          {phase === "ready" && (
            <button onClick={handleInstall} style={{ ...btnPrimary, flex: 1 }}>
              安装并重启
            </button>
          )}
          {phase === "failed" && (
            <>
              <button
                onClick={() => downloadUrl && openExternalUrl(downloadUrl)}
                style={btnPrimary}
              >
                浏览器下载
              </button>
              <button onClick={handleUpdate} style={btnGhost}>
                重试
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

const btnPrimary: React.CSSProperties = {
  flex: 1,
  padding: "10px 20px",
  background: "var(--accent-primary)",
  color: "#ffffff",
  border: "none",
  borderRadius: "var(--radius-md)",
  fontSize: 13,
  fontWeight: 600,
  cursor: "pointer",
  transition: "background var(--duration-fast) var(--ease-in-out)",
};

const btnGhost: React.CSSProperties = {
  padding: "10px 20px",
  background: "transparent",
  color: "var(--text-secondary)",
  border: "1px solid var(--border-default)",
  borderRadius: "var(--radius-md)",
  fontSize: 13,
  cursor: "pointer",
  transition: "all var(--duration-fast) var(--ease-in-out)",
};
