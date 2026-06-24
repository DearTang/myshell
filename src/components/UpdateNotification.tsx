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
 *   - "忽略" → remember this version in localStorage; never prompt
 *     again for the same version. A newer version will re-trigger the dialog.
 */
export function UpdateNotification({ updateInfo }: Props) {
  const latest = updateInfo.latest_version;
  const downloadUrl = updateInfo.download_url || updateInfo.release_url || "";
  // latest_version comes from the Gitee tag which already includes "v" (e.g.
  // "v1.6.1"). Display it as-is to avoid a double "v" prefix.
  const latestDisplay = latest.startsWith("v") ? latest : `v${latest}`;

  const IGNORE_KEY = "myshell.ignoredUpdateVersion";
  const [ignored, setIgnored] = useState<boolean>(() => {
    try {
      return localStorage.getItem(IGNORE_KEY) === latest;
    } catch {
      return false;
    }
  });

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
    <div style={styles.overlay}>
      <div className="animate-scale-in" style={styles.card}>
        {/* ── Outer bezel (glass shell) ── */}
        <div style={styles.shell}>
          {/* ── Inner core ── */}
          <div style={styles.core}>
            {/* Header */}
            <div style={styles.header}>
              <div style={styles.iconCircle}>
                <span style={{ fontSize: 22 }}>✨</span>
              </div>
              <div style={styles.title}>MyShell 有新版本可用</div>
              <div style={styles.version}>{latestDisplay}</div>
            </div>

            {/* Subtitle / status */}
            <div style={styles.subtitle}>
              {phase === "prompt" && "新版本已就绪，是否立即更新？"}
              {phase === "downloading" &&
                (pct !== null ? `正在下载… ${pct}%` : "正在下载…")}
              {phase === "ready" && "下载完成，点击安装并重启应用"}
              {phase === "failed" && (error || "下载出现问题")}
            </div>

            {/* Progress bar */}
            {phase === "downloading" && (
              <div style={styles.progressTrack}>
                <div
                  style={{
                    ...styles.progressFill,
                    width: pct !== null ? `${pct}%` : "30%",
                  }}
                />
              </div>
            )}

            {/* Actions */}
            <div style={styles.actions}>
              {phase === "prompt" && (
                <>
                  <button
                    onClick={handleIgnore}
                    style={styles.btnGhost}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.background =
                        "var(--bg-surface-hover)";
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.background = "transparent";
                    }}
                  >
                    忽略
                  </button>
                  <button
                    onClick={handleUpdate}
                    style={styles.btnPrimary}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.background =
                        "var(--accent-primary-hover)";
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.background =
                        "var(--accent-primary)";
                    }}
                  >
                    更新
                  </button>
                </>
              )}
              {phase === "downloading" && (
                <div style={styles.downloadingText}>请稍候，下载完成后将自动提示…</div>
              )}
              {phase === "ready" && (
                <button
                  onClick={handleInstall}
                  style={{ ...styles.btnPrimary, width: "100%" }}
                  onMouseEnter={(e) => {
                    e.currentTarget.style.background =
                      "var(--accent-primary-hover)";
                  }}
                  onMouseLeave={(e) => {
                    e.currentTarget.style.background = "var(--accent-primary)";
                  }}
                >
                  安装并重启
                </button>
              )}
              {phase === "failed" && (
                <>
                  <button
                    onClick={() => downloadUrl && openExternalUrl(downloadUrl)}
                    style={styles.btnPrimary}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.background =
                        "var(--accent-primary-hover)";
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.background =
                        "var(--accent-primary)";
                    }}
                  >
                    浏览器下载
                  </button>
                  <button
                    onClick={handleUpdate}
                    style={styles.btnGhost}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.background =
                        "var(--bg-surface-hover)";
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.background = "transparent";
                    }}
                  >
                    重试
                  </button>
                </>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

// ─── Styles (Double-Bezel architecture + inline CSS vars) ────────────────

const styles: Record<string, React.CSSProperties> = {
  overlay: {
    position: "fixed",
    inset: 0,
    background: "var(--bg-overlay)",
    backdropFilter: "blur(12px)",
    WebkitBackdropFilter: "blur(12px)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    zIndex: 1500,
  },
  card: {
    width: 400,
    maxWidth: "90vw",
  },
  // Outer glass shell
  shell: {
    background: "var(--glass-bg, rgba(255,255,255,0.04))",
    border: "1px solid var(--border-subtle)",
    borderRadius: "var(--radius-xl)",
    padding: 4,
    boxShadow: "var(--shadow-xl)",
  },
  // Inner core
  core: {
    background: "var(--bg-elevated)",
    borderRadius: "calc(var(--radius-xl) - 4px)",
    overflow: "hidden",
    boxShadow: "inset 0 1px 0 rgba(255,255,255,0.05)",
  },
  header: {
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    padding: "28px 24px 0",
    gap: 10,
  },
  iconCircle: {
    width: 48,
    height: 48,
    borderRadius: "var(--radius-full)",
    background: "var(--accent-primary-muted, rgba(88,166,255,0.12))",
    border: "1px solid var(--border-subtle)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    color: "var(--accent-primary)",
    boxShadow: "inset 0 1px 0 rgba(255,255,255,0.08)",
  },
  title: {
    fontSize: 17,
    fontWeight: 600,
    color: "var(--text-primary)",
    letterSpacing: "-0.01em",
  },
  version: {
    fontSize: 13,
    fontWeight: 500,
    color: "var(--accent-primary)",
    background: "var(--accent-primary-muted, rgba(88,166,255,0.12))",
    padding: "3px 12px",
    borderRadius: "var(--radius-full)",
    letterSpacing: "0.02em",
  },
  subtitle: {
    textAlign: "center" as const,
    fontSize: 13,
    color: "var(--text-tertiary)",
    lineHeight: 1.5,
    padding: "14px 24px 0",
  },
  progressTrack: {
    height: 3,
    background: "var(--bg-surface)",
    margin: "14px 24px 0",
    borderRadius: "var(--radius-full)",
    overflow: "hidden",
  },
  progressFill: {
    height: "100%",
    background: "var(--accent-primary)",
    borderRadius: "var(--radius-full)",
    transition: "width 150ms cubic-bezier(0.32, 0.72, 0, 1)",
  },
  actions: {
    display: "flex",
    gap: 10,
    padding: "20px 24px 24px",
  },
  downloadingText: {
    flex: 1,
    textAlign: "center" as const,
    fontSize: 12,
    color: "var(--text-muted)",
    padding: "8px 0",
  },
  btnPrimary: {
    flex: 1,
    padding: "10px 20px",
    background: "var(--accent-primary)",
    color: "#ffffff",
    border: "none",
    borderRadius: "var(--radius-md)",
    fontSize: 13,
    fontWeight: 600,
    cursor: "pointer",
    transition: "background 200ms cubic-bezier(0.32, 0.72, 0, 1)",
    lineHeight: 1.4,
  },
  btnGhost: {
    flex: 1,
    padding: "10px 20px",
    background: "transparent",
    color: "var(--text-secondary)",
    border: "1px solid var(--border-default)",
    borderRadius: "var(--radius-md)",
    fontSize: 13,
    fontWeight: 500,
    cursor: "pointer",
    transition: "background 200ms cubic-bezier(0.32, 0.72, 0, 1)",
    lineHeight: 1.4,
  },
};
