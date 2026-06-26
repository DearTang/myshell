import type { ReactNode } from "react";

interface Props {
  title: string;
  /** Body text. Accepts a string (plain) or a ReactNode so callers can embed
   * emphasized spans — e.g. a red bold connection count inside a folder-delete
   * prompt when the folder actually contains connections. */
  message: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  /** When true the confirm button renders in the error color (destructive
   * actions like delete). Defaults to true — most confirms in this app gate
   * destructive ops. */
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * A generic confirmation modal — the React replacement for `window.confirm`.
 *
 * Why this exists: `window.confirm` / `window.alert` are silently swallowed by
 * some Tauri WebView configurations (they render nothing or no-op), so
 * delete/confirm prompts that "worked in the browser" never appear for the
 * user. This component renders a real DOM modal that always shows, styled to
 * match BroadcastDupDialog / ConnectionDialog (overlay + animate-scale-in +
 * the project's CSS custom properties).
 *
 * Usage: render `{confirmState && <ConfirmDialog ... />}` and drive it with a
 * state object holding whatever payload the action needs.
 */
export function ConfirmDialog({
  title,
  message,
  confirmLabel = "确定",
  cancelLabel = "取消",
  danger = true,
  onConfirm,
  onCancel,
}: Props) {
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
      onClick={onCancel}
    >
      <div
        className="animate-scale-in"
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "var(--bg-elevated)",
          border: "1px solid var(--border-emphasis)",
          borderRadius: "var(--radius-xl)",
          width: 400,
          maxWidth: "90vw",
          boxShadow: "var(--shadow-xl)",
          overflow: "hidden",
        }}
      >
        <div style={{ padding: "20px 24px" }}>
          <div style={{ display: "flex", alignItems: "flex-start", gap: 12 }}>
            <span style={{ fontSize: 22, lineHeight: 1.2 }}>
              {danger ? "⚠️" : "❓"}
            </span>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text-primary)", marginBottom: 8 }}>
                {title}
              </div>
              <div style={{ fontSize: 12.5, color: "var(--text-secondary)", lineHeight: 1.7, whiteSpace: "pre-wrap" }}>
                {message}
              </div>
            </div>
          </div>
        </div>

        <div
          style={{
            padding: "13px 24px",
            borderTop: "1px solid var(--border-subtle)",
            display: "flex",
            justifyContent: "flex-end",
            gap: 10,
            background: "var(--bg-surface)",
          }}
        >
          <button
            onClick={onCancel}
            style={{
              padding: "9px 18px",
              background: "transparent",
              color: "var(--text-secondary)",
              border: "1px solid var(--border-default)",
              borderRadius: "var(--radius-md)",
              fontSize: 13,
              cursor: "pointer",
              transition: "all var(--duration-fast) var(--ease-in-out)",
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = "var(--bg-surface-hover)";
              e.currentTarget.style.borderColor = "var(--border-emphasis)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "transparent";
              e.currentTarget.style.borderColor = "var(--border-default)";
            }}
          >
            {cancelLabel}
          </button>
          <button
            onClick={onConfirm}
            autoFocus
            style={{
              padding: "9px 22px",
              background: danger ? "var(--error)" : "var(--accent-primary)",
              color: "white",
              border: "none",
              borderRadius: "var(--radius-md)",
              fontSize: 13,
              fontWeight: 600,
              cursor: "pointer",
              transition: "all var(--duration-fast) var(--ease-in-out)",
              boxShadow: danger ? "none" : "var(--shadow-glow)",
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = danger
                ? "var(--error-hover, var(--error))"
                : "var(--accent-primary-hover)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = danger ? "var(--error)" : "var(--accent-primary)";
            }}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Convenience type for the state object that drives a `<ConfirmDialog />`.
 * Store one of these in component state, set it to prompt the user, and clear
 * it (null) on confirm/cancel. The `payload` field carries whatever the
 * action needs (e.g. an id to delete). */
export interface ConfirmState<T = unknown> {
  title: string;
  message: ReactNode;
  confirmLabel?: string;
  danger?: boolean;
  payload: T;
}
