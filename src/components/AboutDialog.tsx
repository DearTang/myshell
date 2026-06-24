import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ReactNode } from "react";
import { useState, useEffect } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { BrandLogo } from "./BrandLogo";
import {
  downloadUpdate,
  installUpdate,
  onUpdateDownloadProgress,
  type UpdateInfo,
} from "../api";
// `?raw` bundles the file as a string at build time (see vite-env.d.ts).
// Offline-safe and pinned to the installed version — no network needed to
// show what changed. CHANGELOG.md lives at the repo root, two levels up
// from src/components/.
import changelog from "../../CHANGELOG.md?raw";

// CHANGELOG.md carries a top-of-file guidance HTML comment (plus keepachangelog
// notes) that must NOT appear in the in-app rendered view — react-markdown can
// surface raw HTML-comment text. Strip every `<!-- … -->` (multiline-safe) and
// collapse the resulting blank runs before handing the string to ReactMarkdown.
const cleanChangelog = changelog
  .replace(/<!--[\s\S]*?-->/g, "")
  .replace(/\n{3,}/g, "\n\n")
  .trim();

type AboutMode = "whatsnew" | "about";

interface Props {
  mode: AboutMode;
  version: string;
  updateInfo: UpdateInfo | null;
  /** True while a forced check is in flight (disables the check button). */
  checking: boolean;
  onClose: () => void;
  /** Force a fresh update check (bypasses the 24h throttle). */
  onCheckUpdates: () => void;
  /** Open a download/release URL in the default browser. */
  onDownload: (url: string) => void;
}

/**
 * Shared modal for two surfaces:
 *
 * - `whatsnew` — shown automatically on first launch after an upgrade (App
 *   compares the running version against `myshell.knownVersion`). Headline
 *   + scrollable changelog + single "知道了" button.
 * - `about` — opened from the sidebar version footer. Branding, version,
 *   "检查更新" button, an update banner when one is available, the
 *   changelog, and a close button.
 *
 * Styling mirrors the existing dialog pattern (overlay + `.animate-scale-in`
 * + inline styles + CSS vars from styles/global.css). The changelog is
 * rendered via react-markdown + remark-gfm (same deps AiPanel uses); inline
 * component overrides give headings/lists/links a clean look without a
 * dedicated global CSS class.
 */
export function AboutDialog({
  mode,
  version,
  updateInfo,
  checking,
  onClose,
  onCheckUpdates,
  onDownload,
}: Props) {
  const isWhatsNew = mode === "whatsnew";
  const hasUpdate = !!updateInfo?.has_update;

  // Download destination: prefer the first asset's URL, fall back to the
  // release page.
  const downloadUrl =
    updateInfo?.download_url || updateInfo?.release_url || "";

  // Auto-download+install state (mirrors UpdateNotification pattern).
  type AboutUpdatePhase = "idle" | "downloading" | "ready" | "failed";
  const [aboutPhase, setAboutPhase] = useState<AboutUpdatePhase>("idle");
  const [aboutProgress, setAboutProgress] = useState({ downloaded: 0, total: 0 });
  const [aboutError, setAboutError] = useState("");
  const [aboutPath, setAboutPath] = useState("");

  useEffect(() => {
    if (aboutPhase !== "downloading") return;
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;
    onUpdateDownloadProgress((p) => {
      setAboutProgress({ downloaded: p.downloaded, total: p.total });
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => { cancelled = true; unlisten?.(); };
  }, [aboutPhase]);

  const aboutPct =
    aboutProgress.total > 0
      ? Math.min(100, Math.round((aboutProgress.downloaded / aboutProgress.total) * 100))
      : null;

  const handleAboutUpdate = async () => {
    setAboutPhase("downloading");
    setAboutProgress({ downloaded: 0, total: 0 });
    setAboutError("");
    try {
      const path = await downloadUpdate(downloadUrl);
      setAboutPath(path);
      setAboutPhase("ready");
    } catch (e) {
      setAboutError(String(e));
      setAboutPhase("failed");
    }
  };

  const handleAboutInstall = async () => {
    if (!aboutPath) {
      setAboutError("安装包路径丢失");
      setAboutPhase("failed");
      return;
    }
    try {
      await installUpdate(aboutPath);
    } catch (e) {
      setAboutError(String(e));
      setAboutPhase("failed");
    }
  };

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
      // Clicking the backdrop dismisses (matches the lightweight "what's new"
      // / toast nature; the only editable state here is transient).
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="animate-scale-in"
        style={{
          background: "var(--bg-elevated)",
          border: "1px solid var(--border-emphasis)",
          borderRadius: "var(--radius-xl)",
          width: 480,
          maxWidth: "92vw",
          maxHeight: "86vh",
          display: "flex",
          flexDirection: "column",
          boxShadow: "var(--shadow-xl)",
          overflow: "hidden",
        }}
      >
        {/* Header */}
        <div style={{ padding: "20px 24px 16px" }}>
          {isWhatsNew ? (
            <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
              <span style={{ fontSize: 28 }}>🆕</span>
              <div>
                <div style={{ fontSize: 16, fontWeight: 600, color: "var(--text-primary)" }}>
                  MyShell 已更新到 v{version}
                </div>
                <div style={{ fontSize: 12, color: "var(--text-tertiary)", marginTop: 2 }}>
                  以下是本次更新内容
                </div>
              </div>
            </div>
          ) : (
            <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
              <BrandLogo size={44} glow />
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 18, fontWeight: 600, color: "var(--text-primary)" }}>
                  MyShell
                </div>
                <div style={{ fontSize: 12, color: "var(--text-tertiary)", marginTop: 2 }}>
                  版本 v{version}
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Update banner (about mode only) */}
        {!isWhatsNew && (
          <div style={{ padding: "0 24px 14px" }}>
            {hasUpdate ? (
              <div
                style={{
                  display: "flex",
                  flexDirection: "column",
                  gap: 10,
                  padding: "12px 14px",
                  background: "var(--success-muted)",
                  border: "1px solid var(--success)",
                  borderRadius: "var(--radius-md)",
                }}
              >
                <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                  <span style={{ fontSize: 16 }}>✨</span>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
                      发现新版本 {updateInfo!.latest_version.startsWith("v") ? updateInfo!.latest_version : `v${updateInfo!.latest_version}`}
                    </div>
                    <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginTop: 2 }}>
                      {aboutPhase === "idle" && "可自动下载安装，也可前往网页下载"}
                      {aboutPhase === "downloading" && (aboutPct !== null ? `正在下载… ${aboutPct}%` : "正在下载…")}
                      {aboutPhase === "ready" && "下载完成，可安装"}
                      {aboutPhase === "failed" && (aboutError || "下载失败")}
                    </div>
                  </div>
                </div>
                {/* Progress bar */}
                {aboutPhase === "downloading" && (
                  <div style={{ height: 3, background: "var(--bg-surface)", borderRadius: "var(--radius-full)", overflow: "hidden" }}>
                    <div style={{ height: "100%", width: aboutPct !== null ? `${aboutPct}%` : "30%", background: "var(--accent-primary)", borderRadius: "var(--radius-full)", transition: "width 150ms cubic-bezier(0.32, 0.72, 0, 1)" }} />
                  </div>
                )}
                {/* Action buttons */}
                <div style={{ display: "flex", gap: 8 }}>
                  {aboutPhase === "idle" && (
                    <>
                      <button onClick={handleAboutUpdate} style={btnPrimary}>更新</button>
                      <button onClick={() => downloadUrl && onDownload(downloadUrl)} style={btnGhost}>网页下载</button>
                    </>
                  )}
                  {aboutPhase === "downloading" && (
                    <div style={{ flex: 1, textAlign: "center", fontSize: 12, color: "var(--text-muted)", padding: "6px 0" }}>请稍候…</div>
                  )}
                  {aboutPhase === "ready" && (
                    <button onClick={handleAboutInstall} style={{ ...btnPrimary, flex: 1 }}>安装并重启</button>
                  )}
                  {aboutPhase === "failed" && (
                    <>
                      <button onClick={() => downloadUrl && onDownload(downloadUrl)} style={btnPrimary}>浏览器下载</button>
                      <button onClick={handleAboutUpdate} style={btnGhost}>重试</button>
                    </>
                  )}
                </div>
              </div>
            ) : (
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <button
                  onClick={onCheckUpdates}
                  disabled={checking}
                  style={checking ? { ...btnGhost, opacity: 0.6, cursor: "default" } : btnGhost}
                >
                  {checking ? "检查中…" : "检查更新"}
                </button>
                <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
                  {updateInfo?.error
                    ? "上次检查失败，可重试"
                    : updateInfo && !hasUpdate
                    ? "当前已是最新版本"
                    : ""}
                </span>
              </div>
            )}
          </div>
        )}

        {/* Changelog (scrollable) */}
        <div
          style={{
            flex: 1,
            overflowY: "auto",
            padding: "4px 24px 16px",
            borderTop: "1px solid var(--border-subtle)",
            fontSize: 13,
            color: "var(--text-secondary)",
            lineHeight: 1.6,
          }}
        >
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            components={markdownComponents}
          >
            {cleanChangelog}
          </ReactMarkdown>
        </div>

        {/* Footer */}
        <div
          style={{
            padding: "12px 24px",
            borderTop: "1px solid var(--border-subtle)",
            display: "flex",
            justifyContent: "flex-end",
            background: "var(--bg-surface)",
          }}
        >
          <button onClick={onClose} style={btnPrimary}>
            {isWhatsNew ? "知道了" : "关闭"}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Inline ReactMarkdown component map — renders a tidy changelog without a
 * dedicated global CSS class. Headings/paragraphs/lists/links/tables/code
 * get minimal styling on top of the surrounding dialog.
 */
const markdownComponents = {
  h1: ({ children }: { children?: ReactNode }) => (
    <h2 style={{ margin: "14px 0 8px", fontSize: 16, color: "var(--text-primary)" }}>
      {children}
    </h2>
  ),
  h2: ({ children }: { children?: ReactNode }) => (
    <h2 style={{ margin: "14px 0 8px", fontSize: 15, color: "var(--text-primary)" }}>
      {children}
    </h2>
  ),
  h3: ({ children }: { children?: ReactNode }) => (
    <h3 style={{ margin: "10px 0 6px", fontSize: 13, color: "var(--text-primary)" }}>
      {children}
    </h3>
  ),
  p: ({ children }: { children?: ReactNode }) => (
    <p style={{ margin: "6px 0" }}>{children}</p>
  ),
  ul: ({ children }: { children?: ReactNode }) => (
    <ul style={{ margin: "6px 0", paddingLeft: 20 }}>{children}</ul>
  ),
  ol: ({ children }: { children?: ReactNode }) => (
    <ol style={{ margin: "6px 0", paddingLeft: 20 }}>{children}</ol>
  ),
  li: ({ children }: { children?: ReactNode }) => (
    <li style={{ margin: "3px 0" }}>{children}</li>
  ),
  a: ({ children, href }: { children?: ReactNode; href?: string }) => (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      style={{ color: "var(--accent-primary)", textDecoration: "none" }}
    >
      {children}
    </a>
  ),
  hr: () => (
    <hr style={{ border: "none", borderTop: "1px solid var(--border-subtle)", margin: "14px 0" }} />
  ),
  code: ({ children }: { children?: ReactNode }) => (
    <code
      style={{
        fontFamily: "monospace",
        fontSize: 12,
        background: "var(--bg-surface)",
        padding: "1px 5px",
        borderRadius: 4,
      }}
    >
      {children}
    </code>
  ),
  blockquote: ({ children }: { children?: ReactNode }) => (
    <blockquote
      style={{
        margin: "8px 0",
        padding: "8px 12px",
        borderLeft: "3px solid var(--border-emphasis)",
        background: "var(--bg-surface)",
        borderRadius: "var(--radius-sm)",
        color: "var(--text-tertiary)",
        fontSize: 12,
      }}
    >
      {children}
    </blockquote>
  ),
};

// Shared button styles (inline, matching the rest of the app).
const btnPrimary: React.CSSProperties = {
  padding: "8px 18px",
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
  padding: "8px 16px",
  background: "transparent",
  color: "var(--text-secondary)",
  border: "1px solid var(--border-default)",
  borderRadius: "var(--radius-md)",
  fontSize: 13,
  cursor: "pointer",
  transition: "all var(--duration-fast) var(--ease-in-out)",
};
