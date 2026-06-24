import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ReactNode } from "react";
import { BrandLogo } from "./BrandLogo";
import type { UpdateInfo } from "../api";
// `?raw` bundles the file as a string at build time (see vite-env.d.ts).
// Offline-safe and pinned to the installed version — no network needed to
// show what changed. CHANGELOG.md lives at the repo root, two levels up
// from src/components/.
import changelog from "../../CHANGELOG.md?raw";

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
  // release page. Either is an https link the Rust opener will accept.
  const downloadUrl =
    updateInfo?.download_url || updateInfo?.release_url || "";

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
                  alignItems: "center",
                  gap: 12,
                  padding: "12px 14px",
                  background: "var(--success-muted)",
                  border: "1px solid var(--success)",
                  borderRadius: "var(--radius-md)",
                }}
              >
                <span style={{ fontSize: 16 }}>✨</span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
                    发现新版本 v{updateInfo!.latest_version}
                  </div>
                  <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginTop: 2 }}>
                    点击右侧按钮前往下载
                  </div>
                </div>
                <button
                  onClick={() => downloadUrl && onDownload(downloadUrl)}
                  style={btnPrimary}
                >
                  去下载
                </button>
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
            {changelog}
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
