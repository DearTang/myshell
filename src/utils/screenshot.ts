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
 *
 * Colors come from the selected screenshot style (设置 → 截图样式, default
 * PuTTY), NOT from the live terminal palette: a PNG is an export artifact
 * meant for docs/tickets, so it should stay stable while the user repaints
 * their terminal. Only cell geometry (cols/rows, char size) and the font
 * stack come from the live terminal — a screenshot must line up with what
 * the user actually saw.
 */
import type { Terminal } from "@xterm/xterm";
import { resolveScreenshotStyle } from "@/store/appearance";

/** xterm `ITheme` key for each of the 16 ANSI palette indices. */
const PALETTE_KEYS = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
] as const;

// PuTTY default 16-color scheme — FALLBACK palette for the 16-color "indexed"
// form, used only when neither the live terminal theme nor the selected
// screenshot style defines the slot. Matches the app's default screenshot
// style (settings → 截图样式 → PuTTY), so a capture on a theme that omitted
// a slot still comes out in PuTTY colors rather than a third scheme.
const PALETTE_16: string[] = [
  // 0-7 (normal)
  "#000000", // 0 black
  "#bb0000", // 1 red
  "#00bb00", // 2 green
  "#bbbb00", // 3 yellow
  "#0000bb", // 4 blue
  "#bb00bb", // 5 magenta
  "#00bbbb", // 6 cyan
  "#bbbbbb", // 7 white
  // 8-15 (bright)
  "#555555", // 8 bright black
  "#ff5555", // 9 bright red
  "#55ff55", // 10 bright green
  "#ffff55", // 11 bright yellow
  "#5555ff", // 12 bright blue
  "#ff55ff", // 13 bright magenta
  "#55ffff", // 14 bright cyan
  "#ffffff", // 15 bright white
];

const DEFAULT_BG = "#000000";
const DEFAULT_FG = "#bbbbbb";

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

    // ── 截图样式（设置 → 截图样式）──
    // The PNG is an EXPORT artifact, so it follows its own palette instead of
    // whatever the user picked for the live terminal: a user on Tokyo Night
    // still wants PuTTY-style PNGs when reporting a bug. `theme` (the live
    // xterm theme) stays the base for anything the screenshot style doesn't
    // define, so a partial style can't leave slots unset.
    const shotStyle = resolveScreenshotStyle();
    const shotTheme: Record<string, string | undefined> = { ...theme };
    for (const key of ["background", "foreground", "cursor", "cursorAccent"] as const) {
      if (shotStyle.terminal[key]) shotTheme[key] = shotStyle.terminal[key];
    }
    for (const key of PALETTE_KEYS) {
      if (shotStyle.terminal[key]) shotTheme[key] = shotStyle.terminal[key];
    }
    // 字体：截图样式自带字体（PuTTY = Courier New）优先，读不到时才回落终端自己的
    const shotFont = shotStyle.fontFamily || fontFamily;

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
    // The screenshot style's background wins outright when it is opaque
    // (PuTTY is pure #000000 — the whole point of the preset), so the PNG
    // never depends on what glass/wallpaper happens to be behind the terminal
    // on screen. Only when the style leaves the background transparent do we
    // walk up the DOM for the first non-transparent computed background, and
    // fall back to the style's own (or the built-in) background if every
    // layer is transparent.
    const bgColor = isOpaqueColor(shotTheme.background)
      ? shotTheme.background!
      : resolveEffectiveBackground(rootEl, shotTheme.background || DEFAULT_BG);
    ctx.fillStyle = bgColor;
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    ctx.font = `${Math.round(fontSize)}px ${shotFont}`;
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
        ctx.fillStyle = runFg || shotTheme.foreground || DEFAULT_FG;
        ctx.font = `${runBold ? "bold " : ""}${runItalic ? "italic " : ""}${Math.round(fontSize)}px ${shotFont}`;
        ctx.fillText(runChars, x, y);

        if (runUnderline) {
          ctx.strokeStyle = runFg || shotTheme.foreground || DEFAULT_FG;
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
        const fgColor = resolveColor(cell.getFgColor(), cell.getFgColorMode(), isBold, shotTheme, true, isInverse);
        const bgColor = resolveColor(cell.getBgColor(), cell.getBgColorMode(), false, shotTheme, false, isInverse);

        // Compute effective colors after inverse swap.
        let effFg = fgColor;
        let effBg = bgColor; // null = default bg
        if (isInverse) {
          effFg = bgColor || shotTheme.foreground || DEFAULT_FG;
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
  const fontFamily: string = opts.fontFamily || "Courier New, Consolas, monospace";
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
 * ancestor chain is transparent, return `fallback` (the caller passes the
 * selected screenshot style's background) so the PNG still has a defined
 * bottom instead of an unset alpha channel.
 */
function resolveEffectiveBackground(rootEl: HTMLElement, fallback: string): string {
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
  return fallback;
}

/** Whether a CSS color string is fully opaque. Accepts `#rrggbb`, `#rrggbbaa`
 *  (alpha ff), `rgb()`, and named/hex colors. `rgba(...,0)` and
 *  `rgba(0,0,0,0)` — the value MyShell's live terminal uses to let the glass
 *  layer through — are transparent, so a screenshot style that wants a solid
 *  background can always override it. */
function isOpaqueColor(color: string | undefined | null): boolean {
  if (!color) return false;
  const c = color.trim().toLowerCase();
  if (c === "transparent") return false;
  if (/^#[0-9a-f]{3}$/.test(c)) return true;
  if (/^#[0-9a-f]{6}$/.test(c)) return true;
  const rgba = /^#([0-9a-f]{8})$/.exec(c);
  if (rgba) return rgba[1].slice(6) === "ff";
  const m = /^rgba?\(\s*[\d.]+\s*,\s*[\d.]+\s*,\s*[\d.]+\s*(?:,\s*([\d.]+)\s*)?\)$/.exec(c);
  if (m) return m[1] === undefined || parseFloat(m[1]) >= 1;
  // 命名色（black/white/...）默认不透明；无法识别的串交给 canvas 自己兜底
  return !c.startsWith("rgba");
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

// xterm.js 5.x color-mode bits (common/buffer/Constants.ts). The values this
// file used to assume (0/1/2) were xterm 4.x's; since 5.x the mode carries
// flag bits in the high nibble, so a palette cell now reports 0x1000000 and
// NOT 1 — with the old constants every indexed/RGB cell fell through to the
// default-foreground branch and screenshots lost all their color.
const CM_DEFAULT = 0;
const CM_P16 = 0x1000000;
const CM_P256 = 0x2000000;
const CM_RGB = 0x3000000;
const CM_MASK = 0x3000000;

/**
 * Resolve an xterm cell color to a CSS color string.
 *
 * xterm cells store color as (color, mode) where mode is one of:
 *   CM_DEFAULT = 0          (use theme foreground/background)
 *   CM_P16     = 0x1000000  (16-color ANSI palette)
 *   CM_P256    = 0x2000000  (256-color palette: 16 + 216 cube + 24 grayscale)
 *   CM_RGB     = 0x3000000  (direct 24-bit RGB)
 * `color` is the payload (palette index, or 0xRRGGBB for RGB).
 */
function resolveColor(
  color: number,
  mode: number,
  isBold: boolean,
  theme: any,
  isForeground: boolean,
  isInverse: boolean,
): string | null {
  // CM_DEFAULT = default fg/bg from theme.
  if ((mode & CM_MASK) === CM_DEFAULT) {
    if (isForeground) {
      return normalizeColor(theme?.foreground) || DEFAULT_FG;
    }
    return null; // default bg — we've already painted the background
  }
  // CM_RGB — direct 24-bit. xterm packs it as 0xRRGGBB.
  if ((mode & CM_MASK) === CM_RGB) {
    return `#${(color & 0xffffff).toString(16).padStart(6, "0")}`;
  }
  // CM_P16 / CM_P256 — palette index 0-255.
  if ((mode & CM_MASK) === CM_P16 || (mode & CM_MASK) === CM_P256) {
    if (color < 16) {
      // 0-7 normal, 8-15 bright. Bold text "brightens" colors 0-7 to 8-15
      // (the SGR "bold also means bright" behavior).
      //
      // `theme` is the merged screenshot theme: the live xterm theme with the
      // selected screenshot style's palette (and fg/bg) layered on top, so
      // colors follow the screenshot style rather than the on-screen palette.
      // PALETTE_16 (PuTTY) is the last resort for a theme that omits a slot.
      if (color < 8) {
        const key = PALETTE_KEYS[color];
        const fromTheme = normalizeColor(theme?.[key]);
        if (fromTheme) {
          return isBold && isForeground ? normalizeColor(theme?.[PALETTE_KEYS[color + 8]]) || fromTheme : fromTheme;
        }
      } else {
        const fromTheme = normalizeColor(theme?.[PALETTE_KEYS[color]]);
        if (fromTheme) return fromTheme;
      }
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
