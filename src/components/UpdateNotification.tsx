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
  /** Open the About dialog (e.g. to read the changelog). */
  onOpenAbout?: () => void;
}

type Phase = "prompt" | "downloading" | "ready" | "failed";

/**
 * Bottom-left update notification card.
 *
 * Shown when the background check finds a newer version. Phases, driven by the
 * user tapping "立即更新":
 *   - prompt      → "✨ 发现新版本 vX.X.X" + green dot + 立即更新
 *   - downloading → progress bar (subscribes to update_download_progress)
 *   - ready       → "下载完成" + 立即安装并重启 (launches the NSIS installer;
 *                   the app exits so the installer can replace files)
 *   - failed      → "下载失败" + 浏览器下载 (opens the release page) + 重试
 *
 * Dismissible (×); dismissal is remembered per `latest_version` in localStorage
 * so the card only re-shows when an even-newer version appears.
 *
 * Auto download + launch-installer (no in-app silent install, no signature
 * verification): trust model is HTTPS + explicit user consent. Falls back to a
 * browser download on any download/install error, per spec.
 */
export function UpdateNotification({ updateInfo, onOpenAbout }: Props) {
  const latest = updateInfo.latest_version;
  const downloadUrl = updateInfo.download_url || updateInfo.release_url || "";

  // Dismissal is per-version: once dismissed for `latest`, stay hidden until a
  // different (newer) version shows up.
  const DISMISS_KEY = "myshell.dismissedUpdateVersion";
  const [dismissed, setDismissed] = useState<boolean>(() => {
    try {
      return localStorage.getItem(DISMISS_KEY) === latest;
    } catch {
      return false;
    }
  });

  // Re-show if a newer-than-dismissed version appears.
  useEffect(() => {
    try {
      if (localStorage.getItem(DISMISS_KEY) !== latest) setDismissed(false);
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

  // Subscribe to progress events only while downloading.
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

  if (dismissed) return null;

  const handleDownload = async () => {
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

  const handleDismiss = () => {
    setDismissed(true);
    try {
      localStorage.setItem(DISMISS_KEY, latest);
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
      className="animate-scale-in"
      style={{
        position: "fixed",
        left: 16,
        bottom: 16,
        zIndex: 1500,
        width: 300,
        maxWidth: "calc(100vw - 32px)",
        background: "var(--bg-elevated)",
        border: "1px solid var(--border-emphasis)",
        borderRadius: "var(--radius-lg)",
        boxShadow: "var(--shadow-xl)",
        overflow: "hidden",
      }}
    >
      {/* Header row: green dot + headline + dismiss */}
      <div style={{ display: "flex", alignItems: "flex-start", gap: 10, padding: "12px 14px 10px" }}>
        <span
          style={{
            marginTop: 5,
            width: 8,
            height: 8,
            borderRadius: "var(--radius-full)",
            background: "var(--accent-secondary)",
            boxShadow: "0 0 8px var(--accent-secondary)",
            flexShrink: 0,
          }}
        />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
            发现新版本 v{latest}
          </div>
          <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginTop: 2 }}>
            {phase === "prompt" && "点击立即更新（自动下载并安装）"}
            {phase === "downloading" && (pct !== null ? `下载中 ${pct}%` : "下载中…")}
            {phase === "ready" && "下载完成，可安装"}
            {phase === "failed" && (error || "下载失败")}
          </div>
        </div>
        <button
          onClick={handleDismiss}
          title="忽略"
          aria-label="忽略"
          style={{
            background: "transparent",
            border: "none",
            color: "var(--text-muted)",
            cursor: "pointer",
            fontSize: 14,
            lineHeight: 1,
            padding: 2,
            flexShrink: 0,
          }}
        >
          ✕
        </button>
      </div>

      {/* Progress bar (downloading phase) */}
      {phase === "downloading" && (
        <div
          style={{
            height: 4,
            background: "var(--bg-surface)",
            margin: "0 14px 10px",
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

      {/* Action row */}
      <div style={{ display: "flex", gap: 8, padding: "0 14px 12px" }}>
        {phase === "prompt" && (
          <>
            <button onClick={handleDownload} style={btnPrimary}>
              立即更新
            </button>
            {onOpenAbout && (
              <button onClick={onOpenAbout} style={btnGhost}>
                更新内容
              </button>
            )}
          </>
        )}
        {phase === "downloading" && (
          <button style={btnGhost} disabled>
            下载中…
          </button>
        )}
        {phase === "ready" && (
          <button onClick={handleInstall} style={btnPrimary}>
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
            <button onClick={handleDownload} style={btnGhost}>
              重试
            </button>
          </>
        )}
      </div>
    </div>
  );
}

const btnPrimary: React.CSSProperties = {
  flex: 1,
  padding: "8px 12px",
  background: "var(--accent-primary)",
  color: "#ffffff",
  border: "none",
  borderRadius: "var(--radius-md)",
  fontSize: 12,
  fontWeight: 600,
  cursor: "pointer",
  transition: "background var(--duration-fast) var(--ease-in-out)",
};

const btnGhost: React.CSSProperties = {
  padding: "8px 12px",
  background: "transparent",
  color: "var(--text-secondary)",
  border: "1px solid var(--border-default)",
  borderRadius: "var(--radius-md)",
  fontSize: 12,
  cursor: "pointer",
  transition: "all var(--duration-fast) var(--ease-in-out)",
};
