import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ThemeProvider } from "./hooks/useTheme";
import { ColorSchemeProvider } from "./hooks/useColorScheme";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { writeFrontendLog } from "./api";
import "./styles/global.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <ThemeProvider>
        <ColorSchemeProvider>
          <App />
        </ColorSchemeProvider>
      </ThemeProvider>
    </ErrorBoundary>
  </React.StrictMode>,
);

// ── Global frontend error capture ──────────────────────────────────────────
//
// The WebView2 console isn't persisted on the user's machine, so to diagnose
// frontend-side anomalies (the "cursor invisible" class of report, an
// unhandled async rejection) we forward these to the Rust backend, which
// writes them into the SAME daily log file as the Rust output (tagged
// `[frontend]`). The backend trace and the frontend trace then share a
// timeline. writeFrontendLog is fire-and-forget and swallows its own errors,
// so a logging failure can never itself become an uncaught error.
//
// (React render crashes are caught separately by ErrorBoundary above; these
// listeners cover async callbacks + event handlers the boundary can't reach.)
if (typeof window !== "undefined") {
  // Uncaught synchronous errors + resource-load errors.
  window.addEventListener("error", (event) => {
    const msg = event.error instanceof Error
      ? `${event.error.message}${event.error.stack ? ` | ${event.error.stack.replace(/\s+/g, " ").trim()}` : ""}`
      : String(event.message || "unknown error");
    writeFrontendLog("error", `uncaught: ${msg} @ ${event.filename}:${event.lineno}`);

    // Suppress the known xterm.js race condition where Viewport.syncScrollArea
    // accesses renderer.dimensions before the Canvas/WebGL renderer has
    // finished initializing (happens when SSH data arrives within the first
    // ~100ms after term.open()). Without preventDefault, WebView2 treats this
    // as a fatal error and crashes the render process, killing the app.
    if (msg.includes("reading 'dimensions'") && event.filename.includes("xterm")) {
      event.preventDefault();
    }
  });
  // Unhandled promise rejections (the common case for failed async/await).
  window.addEventListener("unhandledrejection", (event) => {
    const reason = event.reason instanceof Error
      ? `${event.reason.message}${reasonStack(event.reason)}`
      : typeof event.reason === "string"
        ? event.reason
        : safeStringify(event.reason);
    writeFrontendLog("error", `unhandledrejection: ${reason}`);
  });
}

function reasonStack(e: Error): string {
  return e.stack ? ` | ${e.stack.replace(/\s+/g, " ").trim()}` : "";
}

/** JSON.stringify that can't throw on cyclic / non-serializable values. */
function safeStringify(v: unknown): string {
  try {
    return JSON.stringify(v);
  } catch {
    return String(v);
  }
}

// Tell the Rust backend the first frame is painted so it can reveal the
// window. The main window starts hidden (visible:false) to avoid a white
// flash before React mounts; emit after two animation frames so the browser
// has actually committed the paint (not just queued React's work).
// Wrapped in try/catch + feature checks so this never throws in a pure-web
// (non-Tauri) dev context like `npm run dev` opened in a browser.
(() => {
  try {
    // Dynamically import so web (non-Tauri) environments don't fail at module
    // load. `@tauri-apps/api` only works under the Tauri runtime.
    import("@tauri-apps/api/event")
      .then(({ emit }) => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            emit("dom-ready").catch(() => {
              /* Ignore — backend handles a 4s fallback. */
            });
          });
        });
      })
      .catch(() => {
        /* Non-Tauri context (plain browser). No-op. */
      });
  } catch {
    /* No-op. */
  }
})();
