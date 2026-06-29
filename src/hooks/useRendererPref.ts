import { useCallback, useState } from "react";
import {
  getGpuAccelerationDisabled,
  setGpuAccelerationDisabled,
} from "../api";

/**
 * Terminal renderer backend selection + WebView2 GPU toggle.
 *
 * Background — the "cursor invisible / selection highlight invisible" reports
 * trace to xterm.js's RENDERER layer, not to the WebView-vs-Electron choice:
 *
 *  • WebGL renderer paints the cursor on a SEPARATE transparent 2D-canvas
 *    overlay sitting on top of the WebGL canvas (xtermjs/xterm.js#2614). On
 *    some GPU/driver + WebView2 combinations that overlay fails to composite,
 *    so the cursor (and selection) never appears, no matter what color it is.
 *  • The DOM renderer (xterm 5.x default) only shows the cursor while focused
 *    and has its own known cursor bugs (xtermjs/xterm.js#3271).
 *  • The Canvas renderer paints cursor + selection directly onto the same
 *    canvas as the text — no fragile overlay layer — so it's the most robust
 *    against this whole class of bug. That's the new default.
 *
 * Exposing the choice (and a hard GPU-off escape hatch) lets a user on a
 * misbehaving GPU recover without waiting for a release.
 *
 * Two independent prefs:
 *  • rendererBackend: "auto" | "dom" | "canvas" | "webgl"
 *      - auto   → canvas by default; WebGL only when a background image needs
 *                 transparency (it's the only renderer that composites cleanly
 *                 over a transparent terminal). [kept for TerminalPanel logic]
 *      - dom/canvas/webgl → forced, regardless of background image.
 *  • gpuDisabled: boolean. Persisted to a Rust-managed flag file (NOT just
 *      localStorage) so the Rust side can read it on the NEXT launch before
 *      WebView2 is created, and seed the
 *      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--disable-gpu env var. GPU change
 *      therefore takes effect only after restart — we surface that in the UI.
 */

export type RendererBackend = "auto" | "dom" | "canvas" | "webgl";

export const STORAGE_KEY_RENDERER = "myshell-renderer-backend";
export const DEFAULT_RENDERER: RendererBackend = "auto";

const VALID_BACKENDS: ReadonlySet<string> = new Set([
  "auto",
  "dom",
  "canvas",
  "webgl",
]);

function readRenderer(): RendererBackend {
  try {
    const v = localStorage.getItem(STORAGE_KEY_RENDERER);
    if (v && VALID_BACKENDS.has(v)) return v as RendererBackend;
  } catch {
    // localStorage unavailable — fall through to default.
  }
  return DEFAULT_RENDERER;
}

/**
 * Resolve the effective renderer for a terminal, factoring in the user's
 * backend pref and whether a background image is active. Pure helper — shared
 * by TerminalPanel (per-tab mount) so every terminal picks consistently.
 *
 *  • "webgl" forced    → webgl (user knows their GPU)
 *  • "dom"/"canvas"    → that backend, even with a bg image (user override)
 *  • "auto" + bg image → webgl (only renderer that composites transparent)
 *  • "auto" otherwise  → canvas (the robust default for cursor/selection)
 */
export function resolveRenderer(
  pref: RendererBackend,
  hasBgImage: boolean
): "dom" | "canvas" | "webgl" {
  if (pref === "webgl") return "webgl";
  if (pref === "dom") return "dom";
  if (pref === "canvas") return "canvas";
  // auto
  return hasBgImage ? "webgl" : "canvas";
}

export function useRendererPref(): {
  rendererBackend: RendererBackend;
  setRendererBackend: (b: RendererBackend) => void;
} {
  const [rendererBackend, setRendererBackendState] =
    useState<RendererBackend>(readRenderer);

  const setRendererBackend = useCallback((b: RendererBackend) => {
    setRendererBackendState(b);
    try {
      localStorage.setItem(STORAGE_KEY_RENDERER, b);
    } catch {
      // localStorage full/unavailable — keep in-memory state only.
    }
  }, []);

  return { rendererBackend, setRendererBackend };
}

// ── GPU acceleration toggle (Rust-backed) ──

export interface GpuPref {
  /** Current persisted GPU-off state. True = --disable-gpu on next launch. */
  gpuDisabled: boolean;
  /** Persist the GPU-off flag. Returns the value written. Promise rejection
   * is non-fatal — the Rust command failing just means the flag file couldn't
   * be (re)written; the in-memory state still flips so the UI stays honest. */
  setGpuDisabled: (disabled: boolean) => Promise<void>;
}

/**
 * Read the initial GPU-disabled flag from the Rust side. Used once at App
 * mount to seed the toggle. Defaults to false on any error (the safe,
 * GPU-on default that matches the historical behavior).
 */
export async function readGpuDisabled(): Promise<boolean> {
  try {
    return await getGpuAccelerationDisabled();
  } catch {
    return false;
  }
}

export function useGpuPref(initialDisabled: boolean): GpuPref {
  const [gpuDisabled, setGpuDisabledState] = useState<boolean>(initialDisabled);

  const setGpuDisabled = useCallback(async (disabled: boolean) => {
    setGpuDisabledState(disabled);
    try {
      await setGpuAccelerationDisabled(disabled);
    } catch (e) {
      // Surface to console for diagnostics; the UI state has already flipped
      // so the user sees their intent even if persistence failed.
      console.error("[useRendererPref] setGpuAccelerationDisabled failed:", e);
    }
  }, []);

  return { gpuDisabled, setGpuDisabled };
}
