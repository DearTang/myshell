import React from "react";
import { writeFrontendLog } from "../api";

/**
 * Global React error boundary.
 *
 * Catches errors thrown during render / lifecycle / constructors of ANY
 * descendant, which would otherwise unmount the whole app to a blank window
 * (the classic "white screen" with zero diagnostics). On a caught error it:
 *   1. Forwards the error to the Rust backend log (tagged `[frontend]`) so it
 *      lands in the same daily log file as SSH/PTY events — a render crash can
 *      then be correlated with the backend event that preceded it.
 *   2. Renders a minimal fallback instead of a blank screen, with a "reload"
 *      button so the user can recover without force-killing the app.
 *
 * This complements (not replaces) the window-level `error` /
 * `unhandledrejection` listeners installed in main.tsx — those catch errors
 * in async callbacks and event handlers that this boundary can't reach.
 */
interface State {
  error: Error | null;
}

export class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  State
> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo): void {
    // Join the component stack into one line so it stays a single log entry.
    const stack = info.componentStack ? ` | stack:${info.componentStack.replace(/\s+/g, " ").trim()}` : "";
    writeFrontendLog("error", `render crash: ${error.message}${stack}`);
  }

  render(): React.ReactNode {
    if (this.state.error) {
      return (
        <div
          style={{
            height: "100vh",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            gap: 12,
            background: "var(--bg-base, #0d1117)",
            color: "var(--text-primary, #e6edf3)",
            fontFamily: "system-ui, sans-serif",
            padding: 24,
            textAlign: "center",
          }}
        >
          <div style={{ fontSize: 15, fontWeight: 600 }}>
            界面渲染出错
          </div>
          <div
            style={{
              fontSize: 12,
              color: "var(--text-secondary, #8b949e)",
              maxWidth: 420,
              lineHeight: 1.6,
            }}
          >
            已记录到日志。重新加载通常可以恢复；若持续出现，请在设置中提供日志反馈。
          </div>
          <button
            onClick={() => window.location.reload()}
            style={{
              marginTop: 8,
              padding: "8px 18px",
              background: "var(--accent-primary, #58a6ff)",
              color: "#fff",
              border: "none",
              borderRadius: 8,
              fontSize: 13,
              fontWeight: 600,
              cursor: "pointer",
            }}
          >
            重新加载
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
