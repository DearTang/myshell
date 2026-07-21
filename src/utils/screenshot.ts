/**
 * Terminal screenshot utility.
 *
 * Captures the visible terminal viewport as a PNG data URL by READING THE
 * XTERM BUFFER DIRECTLY and re-rendering each cell onto a fresh canvas.
 *
 * Why not use the "obvious" approaches:
 *
 * 1. **html2canvas** — blank output, because xterm's canvas/webgl renderer
 *    paints pixels via WebGL/2D-context and html2canvas only re-renders
 *    DOM/CSS, not canvas bitmap content.
 *
 * 2. **canvas.toDataURL() on xterm's internal canvas** — blank or "black
 *    block" output, because (a) the WebGL renderer marks its canvas as
 *    write-only (preserveDrawingBuffer: false), so readback returns black;
 *    (b) the canvas renderer shifts its buffer via CSS but the bitmap we read
 *    back may still be empty depending on WebView2 GPU driver state.
 *
 * 3. **Reading the buffer (this file)** — works in 100% of cases because it
 *    doesn't touch the renderer's canvas at all. We walk every visible cell
 *    via `term.buffer.active.getLine(i).getCell(j)`, read its char, fg color,
 *    bg color, and attribute flags, and paint each cell onto a 2D canvas
 *    with the terminal's theme font + palette. This is exactly what xterm's
 *    own canvas renderer does internally — we're just doing it on demand.
 *
 * The output is a faithful image of the visible viewport (no scrollback
 * above the fold), excluding the surrounding CommandBar/tab-strip (because
 * we only draw the buffer, not the DOM tree).
 */
import type { Terminal } from "@xterm/xterm";

// Catppuccin Mocha — the app's hardcoded terminal theme. Used as the default
// palette when a cell uses the 16-color "indexed" form rather than a direct
// RGB. Must match styles/global.css and TerminalPanel's forceVisibleCursor.
// Source: https://github.com/catppuccin/catppuccin
const PALETTE_16: string[] = [
  // 0-7 (normal)
  "#45475a", // 0 black
  "#f38ba8", // 1 red
  "#a6e3a1", // 2 green
  "#f9e2af", // 3 yellow
  "#89b4fa", // 4 blue
  "#f5c2e7", // 5 magenta
  "#94e2d5", // 6 cyan
  "#bac2de", // 7 white
  // 8-15 (bright)
  "#585b70", // 8 bright black
  "#f38ba8", // 9 bright red
  "#a6e3a1", // 10 bright green
  "#f9e2af", // 11 bright yellow
  "#89b4fa", // 12 bright blue
  "#f5c2e7", // 13 bright magenta
  "#94e2d5", // 14 bright cyan
  "#a6adc8", // 15 bright white
];

const DEFAULT_BG = "#1e1e2e";
const DEFAULT_FG = "#cdd6f4";

export async function captureTerminalToDataUrl(term: Terminal): Promise<string | null> {
  try {
    // Give the renderer one frame to flush any pending paint, so the buffer
    // reflects what the user actually sees on screen.
    await new Promise((r) => requestAnimationFrame(r));

    const rootEl = term.element;
    if (!rootEl) {
      console.error("[screenshot] term.element is null");
      return null;
    }

    const buffer = term.buffer.active;
    const cols = term.cols;
    const rows = term.rows;
    const baseRow = buffer.baseY; // top of scrollback
    const viewportTop = buffer.viewportY; // first visible line (may equal baseY)

    if (cols === 0 || rows === 0) {
      console.error("[screenshot] terminal has zero cols/rows");
      return null;
    }

    // Read theme + cell measurements from the live DOM so our output matches
    // what the user sees (font family, size, char dimensions).
    const { fontFamily, fontSize, lineHeight, charWidth, charHeight, theme } =
      readMetrics(term);

    // Output bitmap size (CSS pixels — toDataURL doesn't care about dpr).
    // Add a tiny padding so the last row's descenders aren't clipped.
    const padding = Math.round(fontSize * 0.4);
    const width = cols * charWidth + padding * 2;
    const height = rows * charHeight + padding * 2;

    const canvas = document.createElement("canvas");
    canvas.width = Math.ceil(width);
    canvas.height = Math.ceil(height);
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      console.error("[screenshot] 2d context unavailable");
      return null;
    }

    // Paint background.
    //
    // xterm's theme.background may be 'rgba(0,0,0,0)' (fully transparent) when
    // the app lets an outer CSS background show through (MyShell's background-
    // image feature does exactly this). Painting transparent here means the
    // PNG's alpha channel is 0, which image viewers render as a black/white
    // checkerboard — the "黑白块" bug.
    //
    // Resolve the effective visible background by walking up the DOM from the
    // terminal root and taking the first NON-transparent computed background.
    // Falls back to Catppuccin Mocha base if every layer is transparent.
    const bgColor = resolveEffectiveBackground(rootEl);
    ctx.fillStyle = bgColor;
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    ctx.font = `${Math.round(fontSize)}px ${fontFamily}`;
    ctx.textBaseline = "top";
    ctx.textAlign = "left";

    // Walk each visible row. Visible rows are [viewportTop, viewportTop+rows).
    // (When the user hasn't scrolled back, viewportTop === baseRow === 0
    // relative addressing — buffer.getLine takes absolute scrollback index.)
    for (let row = 0; row < rows; row++) {
      const line = buffer.getLine(viewportTop + row);
      if (!line) continue;

      // Build the cell-run: consecutive cells with the same style share one
      // fillText call. This is both a perf win and avoids sub-pixel jitter
      // from rendering each cell separately.
      let runChars = "";
      let runStartCol = 0;
      let runFg: string | null = null;
      let runBg: string | null = null;
      let runBold = false;
      let runItalic = false;
      let runUnderline = false;

      const flush = (endCol: number) => {
        if (runChars.length === 0) return;
        const x = padding + runStartCol * charWidth;
        const y = padding + row * charHeight;

        // Background fill (only if non-default).
        if (runBg) {
          ctx.fillStyle = runBg;
          ctx.fillRect(x, y, (endCol - runStartCol) * charWidth, charHeight);
        }

        // Text.
        ctx.fillStyle = runFg || DEFAULT_FG;
        ctx.font = `${runBold ? "bold " : ""}${runItalic ? "italic " : ""}${Math.round(fontSize)}px ${fontFamily}`;
        ctx.fillText(runChars, x, y);

        if (runUnderline) {
          ctx.strokeStyle = runFg || DEFAULT_FG;
          ctx.lineWidth = Math.max(1, fontSize / 12);
          ctx.beginPath();
          ctx.moveTo(x, y + charHeight - 1);
          ctx.lineTo(x + (endCol - runStartCol) * charWidth, y + charHeight - 1);
          ctx.stroke();
        }

        // Reset run.
        runChars = "";
        runStartCol = endCol;
      };

      for (let col = 0; col < cols; col++) {
        const cell = line.getCell(col);
        if (!cell) continue;

        const ch = cell.getChars() || " ";
        // xterm's attribute accessors return number (0/1), not boolean.
        // Coerce explicitly to keep the comparisons below well-typed.
        const isBold = !!cell.isBold();
        const isItalic = !!cell.isItalic();
        const isUnderline = !!cell.isUnderline();
        const isInverse = !!cell.isInverse();
        const fgColor = resolveColor(cell.getFgColor(), cell.getFgColorMode(), isBold, theme, true, isInverse);
        const bgColor = resolveColor(cell.getBgColor(), cell.getBgColorMode(), false, theme, false, isInverse);

        // Compute effective colors after inverse swap.
        let effFg = fgColor;
        let effBg = bgColor; // null = default bg
        if (isInverse) {
          effFg = bgColor || DEFAULT_FG;
          effBg = fgColor; // may be null, in which case no bg fill (uses default already painted)
        }

        // Bold also affects width in many fonts — group separately when bold
        // state changes.
        const styleChange =
          (effFg || null) !== (runFg || null) ||
          (effBg || null) !== (runBg || null) ||
          isBold !== runBold ||
          isItalic !== runItalic ||
          isUnderline !== runUnderline;

        if (styleChange && col > runStartCol) {
          flush(col);
        }
        if (runChars.length === 0) {
          runFg = effFg;
          runBg = effBg;
          runBold = isBold;
          runItalic = isItalic;
          runUnderline = isUnderline;
          // If this is the first cell of a new run, re-anchor to current col.
          runStartCol = col;
        }
        runChars += ch;
      }
      flush(cols);
    }

    console.info(`[screenshot] captured ${cols}x${rows} cells → ${canvas.width}x${canvas.height}px`);
    return canvas.toDataURL("image/png");
  } catch (e) {
    console.error("[screenshot] capture failed:", e);
    return null;
  }
}

/** Read font + cell-size metrics from the live terminal DOM. */
function readMetrics(term: Terminal): {
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  charWidth: number;
  charHeight: number;
  theme: any;
} {
  // xterm exposes the active options via term.options.
  const opts = (term as any).options || {};
  const fontFamily: string = opts.fontFamily || "Cascadia Code, Consolas, monospace";
  const fontSize: number = opts.fontSize || 14;
  const lineHeight: number = opts.lineHeight || 1.0;

  // Measure cell size empirically by drawing a reference glyph. xterm's
  // renderer itself does exactly this and stores _renderService.dimensions.
  // Try to read it; if unavailable, fall back to measurement.
  let charWidth = 0;
  let charHeight = 0;
  const dims = (term as any)._renderService?.dimensions;
  if (dims && typeof dims.cssCellWidth === "number") {
    charWidth = dims.cssCellWidth;
    charHeight = dims.cssCellHeight;
  }
  if (!charWidth || !charHeight) {
    // Measure with a hidden canvas.
    const m = document.createElement("canvas").getContext("2d");
    if (m) {
      m.font = `${fontSize}px ${fontFamily}`;
      // Use a wide-ish glyph so the width is accurate for CJK too.
      const metrics = m.measureText("M");
      charWidth = metrics.width || fontSize * 0.6;
      charHeight = (metrics.fontBoundingBoxAscent || fontSize * 0.8) +
                   (metrics.fontBoundingBoxDescent || fontSize * 0.2);
      charHeight *= lineHeight;
    }
  }
  if (!charWidth) charWidth = fontSize * 0.6;
  if (!charHeight) charHeight = fontSize * lineHeight * 1.2;

  return {
    fontFamily,
    fontSize,
    lineHeight,
    charWidth,
    charHeight,
    theme: opts.theme,
  };
}

/**
 * Resolve the effective (visible) background color of the terminal by walking
 * up the DOM tree from the terminal root, returning the first non-transparent
 * computed background-color.
 *
 * Why: MyShell's terminal sometimes sets xterm's theme.background to fully
 * transparent so an outer background-image or panel color shows through.
 * Using that transparent value directly for our screenshot's fillRect leaves
 * the PNG's alpha channel at 0 — image viewers then show a checkerboard
 * ("黑白块" bug). This function finds what color is ACTUALLY visible behind
 * the terminal text.
 *
 * Walk order: .xterm → parents (the terminal viewport wrapper, the panel
 * background, ...) until we hit a non-transparent rgba/rgb. If the entire
 * ancestor chain is transparent, return Catppuccin Mocha base as the final
 * fallback (matches what MyShell looks like with no custom background).
 */
function resolveEffectiveBackground(rootEl: HTMLElement): string {
  let node: HTMLElement | null = rootEl;
  let depth = 0;
  while (node && depth < 8) {
    const bg = getComputedStyle(node).backgroundColor;
    const parsed = parseRgba(bg);
    if (parsed && parsed.a > 0) {
      // Found an opaque (or semi-opaque) layer. Return it as-is so any
      // intentional translucency blends correctly against the deeper layer.
      // But if a > 0 but very low (semi-transparent), keep walking to find a
      // better base — the final composite will look wrong on a flat PNG
      // anyway. Use threshold 0.5: anything more opaque than that is the
      // visible background.
      if (parsed.a >= 0.5) {
        return bg;
      }
    }
    node = node.parentElement;
    depth++;
  }
  return DEFAULT_BG;
}

/** Parse a CSS `rgb()` / `rgba()` string into {r,g,b,a}. Null if unparseable. */
function parseRgba(s: string): { r: number; g: number; b: number; a: number } | null {
  const m = s.match(/rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)/i);
  if (!m) return null;
  const a = m[4] !== undefined ? parseFloat(m[4]) : 1;
  return { r: parseFloat(m[1]), g: parseFloat(m[2]), b: parseFloat(m[3]), a };
}

/**
 * Normalize an xterm theme color value to a CSS color string.
 *
 * xterm theme values may be:
 *   - string: "#1e1e2e", "#1e1e2eff" (with alpha), "rgb(30, 30, 46)" — passed through.
 *   - number: 0xRRGGBBAA or 0xRRGGBB — converted to "#rrggbb[aa]".
 *   - undefined/null: returns null (caller falls back to default).
 *
 * Returns null if the input can't be turned into a valid color, so the caller
 * can apply its own default rather than passing garbage to fillStyle (which
 * silently falls back to the previous fillStyle — a common source of "white
 * background" bugs).
 */
function normalizeColor(c: string | number | undefined | null): string | null {
  if (c == null) return null;
  if (typeof c === "string") {
    const s = c.trim();
    if (s === "") return null;
    // Accept any CSS color string (#rgb, #rrggbb, #rrggbbaa, rgb(), rgba(),
    // named colors). Browsers validate at paint time; we just check it's
    // non-empty here.
    return s;
  }
  if (typeof c === "number") {
    // Pack as hex. xterm uses 0xRRGGBB (24-bit) for theme entries — alpha
    // defaults to opaque.
    const hex = (c >>> 0).toString(16).padStart(6, "0");
    return `#${hex.slice(0, 6)}`;
  }
  return null;
}

/**
 * Resolve an xterm cell color to a CSS color string.
 *
 * xterm cells store color as (color, mode) where mode is one of:
 *   0 = default (use theme foreground/background)
 *   1 = indexed palette (16-color + 216 cube + 24 grayscale = 256 total)
 *   2 = direct RGB (24-bit)
 */
function resolveColor(
  color: number,
  mode: number,
  isBold: boolean,
  theme: any,
  isForeground: boolean,
  isInverse: boolean,
): string | null {
  // mode 0 = default
  if (mode === 0) {
    // Default fg/bg come from theme.
    if (isForeground) {
      return normalizeColor(theme?.foreground) || DEFAULT_FG;
    }
    return null; // default bg — we've already painted the background
  }
  // mode 2 = RGB direct (24-bit). xterm packs it as 0xRRGGBB.
  if (mode === 2) {
    return `#${(color & 0xffffff).toString(16).padStart(6, "0")}`;
  }
  // mode 1 = palette index 0-255.
  if (mode === 1) {
    if (color < 16) {
      // 0-7 normal, 8-15 bright. Bold text "brightens" colors 0-7 to 8-15
      // (the SGR "bold also means bright" behavior).
      if (isBold && isForeground && color < 8) {
        return PALETTE_16[color + 8];
      }
      return PALETTE_16[color];
    }
    if (color >= 16 && color < 232) {
      // 216-cube: 6x6x6 RGB, each channel in {0,1,2,3,4,5} → {0,95,135,175,215,255}.
      const i = color - 16;
      const r = Math.floor(i / 36) % 6;
      const g = Math.floor(i / 6) % 6;
      const b = i % 6;
      const toByte = (v: number) => (v === 0 ? 0 : 95 + (v - 1) * 40);
      return `#${toByte(r).toString(16).padStart(2, "0")}${toByte(g).toString(16).padStart(2, "0")}${toByte(b).toString(16).padStart(2, "0")}`;
    }
    // 232-255: grayscale ramp, 24 steps from #080808 to #eeeeee.
    const v = Math.round(8 + (color - 232) * 10);
    const hex = v.toString(16).padStart(2, "0");
    return `#${hex}${hex}${hex}`;
  }
  return null;
}
