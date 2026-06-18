import { useCallback, useState } from "react";
import {
  STORAGE_KEY_TERMINAL_FONT,
  TERMINAL_FONT_DEFAULT_STACK,
} from "../themes";

/**
 * Persists the user's chosen terminal font FAMILY (a single primary family,
 * e.g. "CaskaydiaCove Nerd Font") in localStorage, mirroring the theme/
 * palette hooks. Empty string = use the default Nerd-Font-first stack.
 */

function readStoredFont(): string {
  try {
    return localStorage.getItem(STORAGE_KEY_TERMINAL_FONT) ?? "";
  } catch {
    return "";
  }
}

/** Strip characters that would break out of the quoted family name. */
function cleanFontName(raw: string): string {
  return raw.replace(/['"]/g, "").trim();
}

/**
 * Build an xterm.js font-family string: the given primary family FIRST (so its
 * glyphs win, including Nerd Font icons), then the default Nerd-Font-first
 * fallback chain. Empty/undefined primary → just the default stack.
 *
 * Shared by the global setting (via useTerminalFont) and per-connection
 * overrides — TerminalPanel resolves `override ?? global` using this.
 */
export function resolveFontStack(primary?: string): string {
  const cleaned = cleanFontName(primary ?? "");
  return cleaned
    ? `'${cleaned}', ${TERMINAL_FONT_DEFAULT_STACK}`
    : TERMINAL_FONT_DEFAULT_STACK;
}

export function useTerminalFont(): {
  /** Raw stored primary family ("" = default). Bind to the settings input. */
  primaryFont: string;
  /** Persist a new primary family. */
  setPrimaryFont: (font: string) => void;
  /** Resolved CSS font-family string for xterm.js. */
  fontFamily: string;
} {
  const [primaryFont, setPrimaryFontState] = useState<string>(readStoredFont);

  const setPrimaryFont = useCallback((font: string) => {
    const cleaned = cleanFontName(font);
    setPrimaryFontState(cleaned);
    try {
      localStorage.setItem(STORAGE_KEY_TERMINAL_FONT, cleaned);
    } catch {
      // localStorage full or unavailable — keep in-memory state only.
    }
  }, []);

  return {
    primaryFont,
    setPrimaryFont,
    fontFamily: resolveFontStack(primaryFont),
  };
}
