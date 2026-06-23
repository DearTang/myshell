import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ThemeProvider } from "./hooks/useTheme";
import { ColorSchemeProvider } from "./hooks/useColorScheme";
import "./styles/global.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <ColorSchemeProvider>
        <App />
      </ColorSchemeProvider>
    </ThemeProvider>
  </React.StrictMode>,
);

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
