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
