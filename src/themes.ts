// ===========================================================================
// Color Palette System — Preset definitions for terminal + global UI theming
// ===========================================================================
//
// Terminal ANSI colors are tuned for:
// - ≥4.5:1 foreground-to-background contrast (WCAG AA)
// - Blue luminance ≥50 (no "disappearing blue" on dark backgrounds)
// - Bright variants +25-35% luminance vs normal variants
// - Complementary or analogous hue relationships within each palette
//
// UI CSS overrides are applied via document.documentElement.style.setProperty
// and tracked in useColorScheme.tsx for clean teardown on palette switch.
// ===========================================================================

// --------------- Types ---------------

export interface TerminalTheme {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent: string;
  selectionBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

export type UIVariables = Partial<Record<string, string>>;

export interface ColorPaletteVariant {
  terminal: TerminalTheme;
  ui: UIVariables;
}

export interface ColorPalette {
  id: string;
  name: string;
  dark: ColorPaletteVariant;
  light: ColorPaletteVariant;
}

export interface BackgroundImageConfig {
  dataUrl: string | null;
  opacity: number;
}

// --------------- localStorage keys ---------------

export const STORAGE_KEY_COLOR = "myshell-color-scheme";
export const STORAGE_KEY_CUSTOM = "myshell-custom-color-scheme";
export const STORAGE_KEY_BG = "myshell-bg-image";
export const STORAGE_KEY_TERMINAL_FONT = "myshell-terminal-font";

export const DEFAULT_PALETTE_ID = "carbon";
export const DEFAULT_BG_IMAGE: BackgroundImageConfig = { dataUrl: null, opacity: 0.85 };

/**
 * Default terminal font stack. Nerd Font families are listed FIRST so the
 * powerline arrows + icon glyphs emitted by prompt engines (Oh My Posh /
 * Starship / powerlevel10k) render correctly — those glyphs live in the
 * Nerd Font Private-Use Area, which base fonts (Cascadia Code, Consolas…)
 * lack, so without a Nerd Font they show as tofu / blank squares. Base
 * monospace fonts remain as a fallback for users without a Nerd Font.
 *
 * Used as the fallback chain; a user-chosen primary family (see
 * useTerminalFont) is prepended when set.
 */
export const TERMINAL_FONT_DEFAULT_STACK =
  "'CaskaydiaCove Nerd Font', 'Cascadia Code NF', 'MesloLGM Nerd Font', 'MesloLGM NF', 'JetBrainsMono Nerd Font', 'FiraCode Nerd Font', 'Hack Nerd Font', 'Cascadia Code', 'Fira Code', 'JetBrains Mono', Consolas, monospace";

// ===========================================================================
// TERMINAL COLOR PALETTES — Each preset redesigned for high contrast & harmony
// ===========================================================================
//
// Hue wheel mapping (for visual distinctiveness):
//   red:    0°-10°    green:  120°-140°   blue:   210°-240°   magenta: 290°-320°
//   yellow: 40°-55°   cyan:   170°-190°
//
// Luminance targets (perceptual, OKLCH-based approximation):
//   bg:    L=15-25    fg:     L=88-94     blue:    L=60-70
//   red:   L=50-58    green:  L=60-70     yellow:  L=70-82
//   cyan:  L=60-72    magenta:L=55-65     bright*: L=+20-35 vs normal
// ===========================================================================

// ── Preset 1: Carbon (default) — deep industrial charcoal + cool steel blue ──
// Zero warm bias: no pink, no rose, no mauve.  Pure tool-grade palette.
// Accent: steel-blue #58a6ff.  Terminal: high-contrast, GitHub-dark inspired.
const c1Dark: ColorPaletteVariant = {
  terminal: {
    background:        "#0d1117", // deep carbon — GitHub-dark
    foreground:         "#e6edf3", // cool white, ~11:1 contrast
    cursor:             "#58a6ff", // steel-blue cursor
    cursorAccent:       "#0d1117",
    selectionBackground:"#264f7844",
    black:              "#30363d", // visible dark grey
    red:                "#f85149", // signal red — sharp, no pink
    green:              "#3fb950", // natural green
    yellow:             "#d29922", // warm amber — the only warm tone
    blue:               "#58a6ff", // steel blue — bright, readable
    magenta:            "#bc8cff", // cool purple — not pink
    cyan:               "#39d2c0", // teal
    white:              "#c9d1d9",
    brightBlack:        "#6e7681",
    brightRed:          "#ff7b72",
    brightGreen:        "#56d364",
    brightYellow:       "#e3b341",
    brightBlue:         "#79c0ff",
    brightMagenta:      "#d2a8ff",
    brightCyan:         "#56d4dd",
    brightWhite:        "#f0f6fc",
  },
  ui: {
    "--bg-base": "#0d1117",
    "--bg-elevated": "#161b22",
    "--bg-surface": "#21262d",
    "--bg-surface-hover": "#292e36",
    "--bg-surface-active": "#323841",
    "--bg-input": "#1a1f26",
    "--bg-input-hover": "#242930",
    "--text-primary": "#e6edf3",
    "--text-secondary": "#8b949e",
    "--text-tertiary": "#6e7681",
    "--text-muted": "#484f58",
    "--accent-primary": "#58a6ff",
    "--accent-primary-hover": "#79c0ff",
    "--accent-primary-muted": "rgba(88,166,255,0.12)",
    "--accent-secondary": "#39d2c0",
    "--accent-secondary-muted": "rgba(57,210,192,0.12)",
    "--success": "#3fb950",
    "--success-muted": "rgba(63,185,80,0.12)",
    "--warning": "#d29922",
    "--warning-muted": "rgba(210,153,34,0.12)",
    "--error": "#f85149",
    "--error-muted": "rgba(248,81,73,0.12)",
    "--info": "#58a6ff",
    "--info-muted": "rgba(88,166,255,0.12)",
    "--border-default": "rgba(255,255,255,0.08)",
    "--border-subtle": "rgba(255,255,255,0.05)",
    "--border-emphasis": "rgba(255,255,255,0.14)",
    "--border-accent": "rgba(88,166,255,0.4)",
    "--shadow-xs": "0 1px 2px rgba(0,0,0,0.3)",
    "--shadow-sm": "0 2px 4px rgba(0,0,0,0.3)",
    "--shadow-md": "0 4px 12px rgba(0,0,0,0.4)",
    "--shadow-lg": "0 8px 24px rgba(0,0,0,0.5)",
    "--shadow-xl": "0 16px 48px rgba(0,0,0,0.6)",
    "--shadow-glow": "0 0 20px rgba(88,166,255,0.2)",
    "--glass-bg": "rgba(22,27,34,0.88)",
    "--glass-border": "rgba(255,255,255,0.08)",
    "--glass-blur": "12px",
  },
};

const c1Light: ColorPaletteVariant = {
  terminal: {
    background:        "#ffffff",
    foreground:         "#1f2328",
    cursor:             "#0969da",
    cursorAccent:       "#ffffff",
    selectionBackground:"#54aeff44",
    black:              "#afb8c1",
    red:                "#cf222e",
    green:              "#1a7f37",
    yellow:             "#9a6700",
    blue:               "#0969da",
    magenta:            "#8250df",
    cyan:               "#1b7c83",
    white:              "#656d76",
    brightBlack:        "#8c959f",
    brightRed:          "#e0454d",
    brightGreen:        "#26a641",
    brightYellow:       "#bf8700",
    brightBlue:         "#218bff",
    brightMagenta:      "#a475f9",
    brightCyan:         "#3192a0",
    brightWhite:        "#1f2328",
  },
  ui: {
    "--bg-base": "#ffffff",
    "--bg-elevated": "#f6f8fa",
    "--bg-surface": "#f0f2f5",
    "--bg-surface-hover": "#e8eaed",
    "--bg-surface-active": "#dfe1e5",
    "--bg-input": "#ffffff",
    "--bg-input-hover": "#f6f8fa",
    "--text-primary": "#1f2328",
    "--text-secondary": "#59636e",
    "--text-tertiary": "#8c959f",
    "--text-muted": "#afb8c1",
    "--accent-primary": "#0969da",
    "--accent-primary-hover": "#218bff",
    "--accent-primary-muted": "rgba(9,105,218,0.1)",
    "--accent-secondary": "#1b7c83",
    "--accent-secondary-muted": "rgba(27,124,131,0.1)",
    "--success": "#1a7f37",
    "--warning": "#9a6700",
    "--error": "#cf222e",
    "--info": "#0969da",
    "--border-default": "rgba(0,0,0,0.08)",
    "--border-subtle": "rgba(0,0,0,0.04)",
    "--border-emphasis": "rgba(0,0,0,0.15)",
    "--border-accent": "rgba(9,105,218,0.4)",
    "--shadow-glow": "0 0 20px rgba(9,105,218,0.15)",
    "--glass-bg": "rgba(255,255,255,0.88)",
    "--glass-border": "rgba(0,0,0,0.06)",
    "--glass-blur": "12px",
  },
};

// ── Preset 2: Dracula — purple/cyan dominant, green complementary ──
const c2Dark: ColorPaletteVariant = {
  terminal: {
    background:        "#282a36",
    foreground:         "#f8f8f2", // ~10:1 contrast
    cursor:             "#f8f8f2",
    cursorAccent:       "#282a36",
    selectionBackground:"#44475a",
    black:              "#383a4a",
    red:                "#ff6e6e", // coral-red — punchy but not aggressive
    green:              "#69f0a8", // vibrant mint (was #50fa7b — too neon)
    yellow:             "#f4d370", // warm gold
    blue:               "#c4a0ff", // brighter purple-blue (was #bd93f9)
    magenta:            "#ff92d0", // hot pink
    cyan:               "#8be9fd", // electric cyan
    white:              "#f8f8f2",
    brightBlack:        "#6272a4",
    brightRed:          "#ff9999",
    brightGreen:        "#8ef5be",
    brightYellow:       "#fbe599",
    brightBlue:         "#ddc4ff",
    brightMagenta:      "#ffb3e0",
    brightCyan:         "#b8f2ff",
    brightWhite:        "#ffffff",
  },
  ui: {
    "--bg-base": "#1e1f29", "--bg-elevated": "#282a36", "--bg-surface": "#343746",
    "--bg-surface-hover": "#3d3f53", "--bg-surface-active": "#44475a",
    "--bg-input": "#2a2c3d", "--bg-input-hover": "#34364b",
    "--accent-primary": "#c4a0ff", "--accent-primary-hover": "#ddc4ff",
    "--accent-primary-muted": "rgba(196,160,255,0.15)", "--accent-secondary": "#8be9fd",
    "--accent-secondary-muted": "rgba(139,233,253,0.15)",
    "--success": "#69f0a8", "--warning": "#f4d370", "--error": "#ff6e6e", "--info": "#8be9fd",
    "--border-accent": "rgba(196,160,255,0.4)", "--shadow-glow": "0 0 20px rgba(196,160,255,0.25)",
    "--glass-bg": "rgba(40,42,54,0.85)", "--glass-border": "rgba(255,255,255,0.08)",
  },
};

const c2Light: ColorPaletteVariant = {
  terminal: {
    background:        "#f8f8f2",
    foreground:         "#282a36",
    cursor:             "#282a36",
    cursorAccent:       "#f8f8f2",
    selectionBackground:"#44475a44",
    black:              "#f8f8f2",  red: "#e04040",  green: "#40c960",
    yellow:             "#bd9300",  blue: "#a86ef0",  magenta: "#e05ab0",
    cyan:               "#18918e",  white: "#282a36",
    brightBlack:        "#bfbfbf", brightRed: "#e86671", brightGreen: "#5ce088",
    brightYellow:       "#d4a520", brightBlue: "#c49af5", brightMagenta: "#ec7cc0",
    brightCyan:         "#3db8b0", brightWhite: "#44475a",
  },
  ui: {
    "--bg-base": "#f8f8f2", "--bg-elevated": "#ffffff", "--bg-surface": "#f0f0ec",
    "--bg-surface-hover": "#e6e6e0", "--accent-primary": "#a86ef0",
    "--accent-primary-hover": "#c49af5", "--accent-secondary": "#18918e",
  },
};

// ── Preset 3: Nord — frosty blue-grey palette, cool and calm ──
// Nord's biggest issue: blue #81a1c1 on #2e3440 → ~4.2:1 (just below AA).
// Fix: bump blue to #89b4d4 (+8L), brighten black, slightly warm the greys.
const c3Dark: ColorPaletteVariant = {
  terminal: {
    background:        "#2e3440",
    foreground:         "#e5e9f0", // boosted from #d8dee9 for ~9:1 contrast
    cursor:             "#e5e9f0",
    cursorAccent:       "#2e3440",
    selectionBackground:"#4c566a88",
    black:              "#434c5e", // was #3b4252 — now visible against bg
    red:                "#c26970", // muted rose
    green:              "#a3be8c", // sage green — distinctive
    yellow:             "#ebcb8b", // warm sand
    blue:               "#89b4d4", // brighter sky (was #81a1c1 — too dark)
    magenta:            "#b48ead", // mauve
    cyan:               "#8cc4c0", // brighter teal (was #88c0d0)
    white:              "#e5e9f0",
    brightBlack:        "#65738b", // +25L vs black
    brightRed:          "#d9888e",
    brightGreen:        "#bfd7a8",
    brightYellow:       "#f2dca8",
    brightBlue:         "#aacedd",
    brightMagenta:      "#caaec7",
    brightCyan:         "#aeddd9",
    brightWhite:        "#eceff4",
  },
  ui: {
    "--bg-base": "#242933", "--bg-elevated": "#2e3440", "--bg-surface": "#3b4252",
    "--bg-surface-hover": "#434c5e", "--bg-surface-active": "#4c566a",
    "--bg-input": "#353b4a", "--bg-input-hover": "#3f4758",
    "--accent-primary": "#8cc4c0", "--accent-primary-hover": "#aeddd9",
    "--accent-primary-muted": "rgba(140,196,192,0.15)", "--accent-secondary": "#89b4d4",
    "--accent-secondary-muted": "rgba(137,180,212,0.15)",
    "--success": "#a3be8c", "--warning": "#ebcb8b", "--error": "#c26970", "--info": "#8cc4c0",
    "--border-accent": "rgba(140,196,192,0.4)", "--shadow-glow": "0 0 20px rgba(140,196,192,0.2)",
    "--glass-bg": "rgba(46,52,64,0.85)", "--glass-border": "rgba(255,255,255,0.06)",
  },
};

const c3Light: ColorPaletteVariant = {
  terminal: {
    background:        "#eceff4",
    foreground:         "#2e3440",
    cursor:             "#2e3440",
    cursorAccent:       "#eceff4",
    selectionBackground:"#d8dee955",
    black:              "#4c566a",  red: "#c26970",  green: "#7d9a69",
    yellow:             "#c5933d",  blue: "#5a81b0",  magenta: "#9a6f95",
    cyan:               "#5a8d8a",  white: "#e5e9f0",
    brightBlack:        "#6c7a92", brightRed: "#d9888e", brightGreen: "#96b580",
    brightYellow:       "#daad58", brightBlue: "#7da3ce", brightMagenta: "#b58eb2",
    brightCyan:         "#7aaba8", brightWhite: "#eceff4",
  },
  ui: {
    "--bg-base": "#eceff4", "--bg-elevated": "#ffffff", "--bg-surface": "#e5e9f0",
    "--bg-surface-hover": "#d8dee9", "--accent-primary": "#5a81b0",
    "--accent-primary-hover": "#7da3ce", "--accent-secondary": "#5a8d8a",
  },
};

// ── Preset 4: Solarized Dark — warm dark + blue accent ──
// Known issue: blue #268bd2 on #002b36 → ~4.3:1.  Fix: blue → #4da0e0.
// Also: foreground #839496 → only ~5.5:1.  Fix: foreground → #9aacaf.
const c4Dark: ColorPaletteVariant = {
  terminal: {
    background:        "#002b36",
    foreground:         "#9aacaf", // was #839496 — now ~7:1 contrast
    cursor:             "#9aacaf",
    cursorAccent:       "#002b36",
    selectionBackground:"#586e7588",
    black:              "#073642",
    red:                "#dc322f", // punchy red — distinctive
    green:              "#859900", // olive-lime
    yellow:             "#c48d00", // warm amber (was #b58900 — darker)
    blue:               "#4da0e0", // brighter blue (was #268bd2 — too dark)
    magenta:            "#d33682", // magenta — good distinction
    cyan:               "#2aa198", // teal
    white:              "#eee8d5",
    brightBlack:        "#657b83",
    brightRed:          "#e85450",
    brightGreen:        "#9eb320",
    brightYellow:       "#e0aa20",
    brightBlue:         "#6dbdf0",
    brightMagenta:      "#e858a0",
    brightCyan:         "#40c0b8",
    brightWhite:        "#fdf6e3",
  },
  ui: {
    "--bg-base": "#001f27", "--bg-elevated": "#002b36", "--bg-surface": "#073642",
    "--bg-surface-hover": "#0d4959", "--bg-surface-active": "#11576b",
    "--bg-input": "#05323f", "--bg-input-hover": "#0a3f4d",
    "--accent-primary": "#4da0e0", "--accent-primary-hover": "#6dbdf0",
    "--accent-primary-muted": "rgba(77,160,224,0.15)", "--accent-secondary": "#2aa198",
    "--accent-secondary-muted": "rgba(42,161,152,0.15)",
    "--success": "#859900", "--warning": "#c48d00", "--error": "#dc322f", "--info": "#4da0e0",
    "--border-accent": "rgba(77,160,224,0.4)", "--shadow-glow": "0 0 20px rgba(77,160,224,0.2)",
    "--glass-bg": "rgba(0,43,54,0.85)", "--glass-border": "rgba(255,255,255,0.06)",
  },
};

const c4Light: ColorPaletteVariant = {
  terminal: {
    background:        "#fdf6e3",
    foreground:         "#586e75",
    cursor:             "#586e75",
    cursorAccent:       "#fdf6e3",
    selectionBackground:"#eee8d555",
    black:              "#073642",  red: "#dc322f",  green: "#6b7a00",
    yellow:             "#a06d00",  blue: "#287bb5",  magenta: "#b02460",
    cyan:               "#1e807a",  white: "#eee8d5",
    brightBlack:        "#8a9a9e", brightRed: "#e85450", brightGreen: "#8a9e1a",
    brightYellow:       "#c48d00", brightBlue: "#4da0e0", brightMagenta: "#d35890",
    brightCyan:         "#30a098", brightWhite: "#fdf6e3",
  },
  ui: {
    "--bg-base": "#fdf6e3", "--bg-elevated": "#ffffff", "--bg-surface": "#eee8d5",
    "--bg-surface-hover": "#e0d9c3", "--accent-primary": "#287bb5",
    "--accent-primary-hover": "#4da0e0", "--accent-secondary": "#1e807a",
  },
};

// ── Preset 5: Gruvbox Dark — retro warm earthy tones ──
// Issue: blue #458588 on #282828 → ~3.1:1 (way too dark).
// Fix: blue → #6ba8a8 (+20L).  Also boost green/yellow slightly.
const c5Dark: ColorPaletteVariant = {
  terminal: {
    background:        "#282828",
    foreground:         "#ebdbb2", // warm cream, ~8.5:1 contrast
    cursor:             "#ebdbb2",
    cursorAccent:       "#282828",
    selectionBackground:"#665c5488",
    black:              "#3c3836",
    red:                "#e04040", // warm red
    green:              "#98971a", // olive
    yellow:             "#d79921", // warm amber
    blue:               "#6ba8a8", // brighter teal-blue (was #458588 — invisible!)
    magenta:            "#b16286", // mauve
    cyan:               "#689d6a", // green-teal
    white:              "#d4be98", // warmer off-white (was #a89984 — too grey)
    brightBlack:        "#7c6f64",
    brightRed:          "#fb6a5c",
    brightGreen:        "#b8bb26",
    brightYellow:       "#fabd2f",
    brightBlue:         "#8cc8c8",
    brightMagenta:      "#d3869b",
    brightCyan:         "#8ec07c",
    brightWhite:        "#f2e5bc",
  },
  ui: {
    "--bg-base": "#1d2021", "--bg-elevated": "#282828", "--bg-surface": "#3c3836",
    "--bg-surface-hover": "#504945", "--bg-surface-active": "#665c54",
    "--bg-input": "#32302f", "--bg-input-hover": "#3c3836",
    "--accent-primary": "#d79921", "--accent-primary-hover": "#fabd2f",
    "--accent-primary-muted": "rgba(215,153,33,0.15)", "--accent-secondary": "#6ba8a8",
    "--accent-secondary-muted": "rgba(107,168,168,0.15)",
    "--success": "#98971a", "--warning": "#d79921", "--error": "#e04040", "--info": "#6ba8a8",
    "--border-accent": "rgba(215,153,33,0.4)", "--shadow-glow": "0 0 20px rgba(215,153,33,0.25)",
    "--glass-bg": "rgba(40,40,40,0.85)", "--glass-border": "rgba(255,255,255,0.06)",
  },
};

const c5Light: ColorPaletteVariant = {
  terminal: {
    background:        "#fbf1c7",
    foreground:         "#3c3836",
    cursor:             "#3c3836",
    cursorAccent:       "#fbf1c7",
    selectionBackground:"#d5c4a155",
    black:              "#fbf1c7",  red: "#cc241d",  green: "#79740e",
    yellow:             "#b57614",  blue: "#4d7a7a",  magenta: "#8f3f64",
    cyan:               "#427b58",  white: "#7c6f64",
    brightBlack:        "#a89984", brightRed: "#e04040", brightGreen: "#98971a",
    brightYellow:       "#d79921", brightBlue: "#6ba8a8", brightMagenta: "#b16286",
    brightCyan:         "#689d6a", brightWhite: "#282828",
  },
  ui: {
    "--bg-base": "#fbf1c7", "--bg-elevated": "#ffffff", "--bg-surface": "#f2e5bc",
    "--bg-surface-hover": "#ebdbb2", "--accent-primary": "#b57614",
    "--accent-primary-hover": "#d79921", "--accent-secondary": "#4d7a7a",
  },
};

// ── Preset 6: One Dark — balanced blue accent, Atom-inspired ──
const c6Dark: ColorPaletteVariant = {
  terminal: {
    background:        "#282c34",
    foreground:         "#d7dae0", // was #abb2bf — now ~8:1 contrast
    cursor:             "#61afef",
    cursorAccent:       "#282c34",
    selectionBackground:"#3e4451",
    black:              "#353a44",
    red:                "#e06c75", // coral-red
    green:              "#98c379", // forest green
    yellow:             "#e5c07b", // gold
    blue:               "#72b1f0", // brighter blue (was #61afef)
    magenta:            "#c678dd", // purple
    cyan:               "#56b6c2", // teal
    white:              "#d7dae0",
    brightBlack:        "#5c6370",
    brightRed:          "#e8878e",
    brightGreen:        "#b0d89a",
    brightYellow:       "#eed29a",
    brightBlue:         "#90c8f5",
    brightMagenta:      "#da95ec",
    brightCyan:         "#76cdd7",
    brightWhite:        "#ffffff",
  },
  ui: {
    "--bg-base": "#21252b", "--bg-elevated": "#282c34", "--bg-surface": "#313640",
    "--bg-surface-hover": "#3b404b", "--bg-surface-active": "#444a57",
    "--bg-input": "#2c313a", "--bg-input-hover": "#353b47",
    "--accent-primary": "#72b1f0", "--accent-primary-hover": "#90c8f5",
    "--accent-primary-muted": "rgba(114,177,240,0.15)", "--accent-secondary": "#56b6c2",
    "--accent-secondary-muted": "rgba(86,182,194,0.15)",
    "--success": "#98c379", "--warning": "#e5c07b", "--error": "#e06c75", "--info": "#72b1f0",
    "--border-accent": "rgba(114,177,240,0.4)", "--shadow-glow": "0 0 20px rgba(114,177,240,0.25)",
    "--glass-bg": "rgba(40,44,52,0.85)", "--glass-border": "rgba(255,255,255,0.08)",
  },
};

const c6Light: ColorPaletteVariant = {
  terminal: {
    background:        "#fafafa",
    foreground:         "#383a42",
    cursor:             "#4078f2",
    cursorAccent:       "#fafafa",
    selectionBackground:"#e5e5e6",
    black:              "#fafafa",  red: "#e45649",  green: "#50a14f",
    yellow:             "#c18401",  blue: "#4078f2",  magenta: "#a626a4",
    cyan:               "#0184bc",  white: "#a0a1a7",
    brightBlack:        "#a0a1a7", brightRed: "#e86671", brightGreen: "#6ebd68",
    brightYellow:       "#d99a20", brightBlue: "#6598f5", brightMagenta: "#c04ac0",
    brightCyan:         "#20a0d8", brightWhite: "#383a42",
  },
  ui: {
    "--bg-base": "#fafafa", "--bg-elevated": "#ffffff", "--bg-surface": "#f0f0f1",
    "--bg-surface-hover": "#e5e5e6", "--accent-primary": "#4078f2",
    "--accent-primary-hover": "#6598f5", "--accent-secondary": "#0184bc",
  },
};

// ── Preset 7: Monokai — classic Sublime Text, green/yellow pop ──
const c7Dark: ColorPaletteVariant = {
  terminal: {
    background:        "#272822",
    foreground:         "#f8f8f2", // ~10:1 contrast
    cursor:             "#f8f8f0",
    cursorAccent:       "#272822",
    selectionBackground:"#49483e",
    black:              "#3a3b32",
    red:                "#f92672", // hot pink-red
    green:              "#a6e22e", // neon lime
    yellow:             "#e6db74", // warm sunlight (was #f4bf75 — too orange)
    blue:               "#66d9ef", // electric cyan-blue
    magenta:            "#ae81ff", // purple
    cyan:               "#a1efe4", // mint
    white:              "#f8f8f2",
    brightBlack:        "#75715e",
    brightRed:          "#fa5090",
    brightGreen:        "#beee50",
    brightYellow:       "#f5ec98",
    brightBlue:         "#8ce4f5",
    brightMagenta:      "#c4a0ff",
    brightCyan:         "#bef5ed",
    brightWhite:        "#f9f8f5",
  },
  ui: {
    "--bg-base": "#1e1f1a", "--bg-elevated": "#272822", "--bg-surface": "#34352e",
    "--bg-surface-hover": "#3e3f36", "--bg-surface-active": "#49483e",
    "--bg-input": "#2d2e27", "--bg-input-hover": "#373830",
    "--accent-primary": "#a6e22e", "--accent-primary-hover": "#beee50",
    "--accent-primary-muted": "rgba(166,226,46,0.15)", "--accent-secondary": "#66d9ef",
    "--accent-secondary-muted": "rgba(102,217,239,0.15)",
    "--success": "#a6e22e", "--warning": "#e6db74", "--error": "#f92672", "--info": "#66d9ef",
    "--border-accent": "rgba(166,226,46,0.4)", "--shadow-glow": "0 0 20px rgba(166,226,46,0.2)",
    "--glass-bg": "rgba(39,40,34,0.85)", "--glass-border": "rgba(255,255,255,0.08)",
  },
};

const c7Light: ColorPaletteVariant = {
  terminal: {
    background:        "#f9f8f5",
    foreground:         "#272822",
    cursor:             "#272822",
    cursorAccent:       "#f9f8f5",
    selectionBackground:"#e6e5e055",
    black:              "#272822",  red: "#e62a5a",  green: "#80b010",
    yellow:             "#b8860b",  blue: "#2cacc8",  magenta: "#8c40d0",
    cyan:               "#48b098",  white: "#ccccc7",
    brightBlack:        "#75715e", brightRed: "#f05070", brightGreen: "#a6e22e",
    brightYellow:       "#d4a020", brightBlue: "#4cc8e0", brightMagenta: "#ae60f0",
    brightCyan:         "#68d0b8", brightWhite: "#f9f8f5",
  },
  ui: {
    "--bg-base": "#f9f8f5", "--bg-elevated": "#ffffff", "--bg-surface": "#f0efe9",
    "--bg-surface-hover": "#e6e5e0", "--accent-primary": "#80b010",
    "--accent-primary-hover": "#a6e22e", "--accent-secondary": "#2cacc8",
  },
};

// ── Preset 8: Tokyo Night — deep navy + neon cyan/magenta accents ──
// Issue: foreground #a9b1d6 on #1a1b26 → ~5.5:1.  Fix: fg → #c4cdf2.
const c8Dark: ColorPaletteVariant = {
  terminal: {
    background:        "#1a1b26",
    foreground:         "#c4cdf2", // was #a9b1d6 — now ~9:1 contrast
    cursor:             "#c0caf5",
    cursorAccent:       "#1a1b26",
    selectionBackground:"#33467c",
    black:              "#2f3148",
    red:                "#f7768e", // neon coral
    green:              "#9ece6a", // neon green
    yellow:             "#e0af68", // warm amber
    blue:               "#7aa2f7", // neon blue
    magenta:            "#bb9af7", // purple
    cyan:               "#7dcfff", // sky cyan
    white:              "#c4cdf2",
    brightBlack:        "#565a7e",
    brightRed:          "#f996a8",
    brightGreen:        "#b8e08a",
    brightYellow:       "#edc888",
    brightBlue:         "#9dbcf9",
    brightMagenta:      "#ceb8f9",
    brightCyan:         "#a0dcff",
    brightWhite:        "#dce0ff",
  },
  ui: {
    "--bg-base": "#13141d", "--bg-elevated": "#1a1b26", "--bg-surface": "#24283b",
    "--bg-surface-hover": "#2f334d", "--bg-surface-active": "#3b4261",
    "--bg-input": "#1e2030", "--bg-input-hover": "#292e42",
    "--accent-primary": "#7aa2f7", "--accent-primary-hover": "#9dbcf9",
    "--accent-primary-muted": "rgba(122,162,247,0.15)", "--accent-secondary": "#7dcfff",
    "--accent-secondary-muted": "rgba(125,207,255,0.15)",
    "--success": "#9ece6a", "--warning": "#e0af68", "--error": "#f7768e", "--info": "#7aa2f7",
    "--border-accent": "rgba(122,162,247,0.4)", "--shadow-glow": "0 0 20px rgba(122,162,247,0.3)",
    "--glass-bg": "rgba(26,27,38,0.85)", "--glass-border": "rgba(255,255,255,0.06)",
  },
};

const c8Light: ColorPaletteVariant = {
  terminal: {
    background:        "#d5d6db",
    foreground:         "#343b58",
    cursor:             "#343b58",
    cursorAccent:       "#d5d6db",
    selectionBackground:"#b7b9c055",
    black:              "#d5d6db",  red: "#8c4351",  green: "#485e30",
    yellow:             "#8f5e15",  blue: "#3a58c0",  magenta: "#5a4a78",
    cyan:               "#0f4b6e",  white: "#343b58",
    brightBlack:        "#9699a3", brightRed: "#a85968", brightGreen: "#5e7840",
    brightYellow:       "#a87528", brightBlue: "#5a74d8", brightMagenta: "#786098",
    brightCyan:         "#286888", brightWhite: "#1a1b26",
  },
  ui: {
    "--bg-base": "#d5d6db", "--bg-elevated": "#e9eaed", "--bg-surface": "#cbccd1",
    "--bg-surface-hover": "#c0c1c8", "--accent-primary": "#3a58c0",
    "--accent-primary-hover": "#5a74d8", "--accent-secondary": "#0f4b6e",
  },
};

// ── Preset 9: Everforest — nature-inspired, green-tinted ──
const c9Dark: ColorPaletteVariant = {
  terminal: {
    background:        "#2d353b",
    foreground:         "#e0dcc7", // warmer than #d3c6aa, ~8.5:1 contrast
    cursor:             "#e0dcc7",
    cursorAccent:       "#2d353b",
    selectionBackground:"#47525888",
    black:              "#3d494d",
    red:                "#e67e80", // rose
    green:              "#a7c080", // sage green
    yellow:             "#dbbc7f", // warm sand
    blue:               "#7fbbb3", // softer teal-blue
    magenta:            "#d699b6", // mauve
    cyan:               "#83c092", // brighter green-teal (was same as green)
    white:              "#e0dcc7",
    brightBlack:        "#6c7a74",
    brightRed:          "#ed9c9e",
    brightGreen:        "#c0d9a0",
    brightYellow:       "#e8ce9c",
    brightBlue:         "#9fd0ca",
    brightMagenta:      "#e0b5cd",
    brightCyan:         "#a4d8b0",
    brightWhite:        "#f0ecd8",
  },
  ui: {
    "--bg-base": "#232a2e", "--bg-elevated": "#2d353b", "--bg-surface": "#343f44",
    "--bg-surface-hover": "#3d484d", "--bg-surface-active": "#475258",
    "--bg-input": "#303b40", "--bg-input-hover": "#3a454a",
    "--accent-primary": "#a7c080", "--accent-primary-hover": "#c0d9a0",
    "--accent-primary-muted": "rgba(167,192,128,0.15)", "--accent-secondary": "#7fbbb3",
    "--accent-secondary-muted": "rgba(127,187,179,0.15)",
    "--success": "#a7c080", "--warning": "#dbbc7f", "--error": "#e67e80", "--info": "#7fbbb3",
    "--border-accent": "rgba(167,192,128,0.4)", "--shadow-glow": "0 0 20px rgba(167,192,128,0.2)",
    "--glass-bg": "rgba(45,53,59,0.85)", "--glass-border": "rgba(255,255,255,0.06)",
  },
};

const c9Light: ColorPaletteVariant = {
  terminal: {
    background:        "#f6f2e4",
    foreground:         "#5c6a72",
    cursor:             "#5c6a72",
    cursorAccent:       "#f6f2e4",
    selectionBackground:"#e0dcc755",
    black:              "#f6f2e4",  red: "#e84e4e",  green: "#7d9400",
    yellow:             "#c48900",  blue: "#3a94c5",  magenta: "#db58a8",
    cyan:               "#35a77c",  white: "#5c6a72",
    brightBlack:        "#939f91", brightRed: "#ee7070", brightGreen: "#98b020",
    brightYellow:       "#daa020", brightBlue: "#58b0e0", brightMagenta: "#e878c0",
    brightCyan:         "#50c098", brightWhite: "#2d353b",
  },
  ui: {
    "--bg-base": "#f6f2e4", "--bg-elevated": "#ffffff", "--bg-surface": "#ede7d5",
    "--bg-surface-hover": "#e0dac5", "--accent-primary": "#7d9400",
    "--accent-primary-hover": "#98b020", "--accent-secondary": "#3a94c5",
  },
};

// ── Preset 10: Catppuccin Latte — warm light base ──
const c10Dark: ColorPaletteVariant = {
  terminal: {
    background:        "#1e1e2e",
    foreground:         "#e0def4",
    cursor:             "#f2cdcd",
    cursorAccent:       "#1e1e2e",
    selectionBackground:"#585b7066",
    black:              "#45475a",
    red:                "#f38ba8",
    green:              "#a6e3a1",
    yellow:             "#f9e2af",
    blue:               "#96cdfb",
    magenta:            "#f5c2e7",
    cyan:               "#94e2d5",
    white:              "#c6cad8",
    brightBlack:        "#6c6f8a",
    brightRed:          "#f5a8b8",
    brightGreen:        "#c4f0b0",
    brightYellow:       "#fcecc4",
    brightBlue:         "#b7ddfd",
    brightMagenta:      "#f9daf1",
    brightCyan:         "#bcf0e8",
    brightWhite:        "#c6caf0",
  },
  ui: {},
};

const c10Light: ColorPaletteVariant = {
  terminal: {
    background:        "#eff1f5",
    foreground:         "#4c4f69",
    cursor:             "#dc8a78",
    cursorAccent:       "#eff1f5",
    selectionBackground:"#acb0be55",
    black:              "#5c5f77",  red: "#d20f39",  green: "#40a02b",
    yellow:             "#df8e1d",  blue: "#2e6bde",  magenta: "#ea76cb",
    cyan:               "#179299",  white: "#acb0be",
    brightBlack:        "#6c6f85", brightRed: "#e64553", brightGreen: "#5cb840",
    brightYellow:       "#e8a840", brightBlue: "#5b8cf0", brightMagenta: "#f292d6",
    brightCyan:         "#2db5a8", brightWhite: "#bcc0cc",
  },
  ui: {},
};

// ===========================================================================
// Preset registry
// ===========================================================================

export const PRESETS: ColorPalette[] = [
  { id: "carbon",            name: "Carbon",            dark: c1Dark,  light: c1Light },
  { id: "catppuccin-latte",  name: "Catppuccin Latte",  dark: c10Dark, light: c10Light },
  { id: "dracula",           name: "Dracula",           dark: c2Dark,  light: c2Light },
  { id: "nord",              name: "Nord",              dark: c3Dark,  light: c3Light },
  { id: "solarized-dark",    name: "Solarized Dark",    dark: c4Dark,  light: c4Light },
  { id: "gruvbox-dark",      name: "Gruvbox Dark",      dark: c5Dark,  light: c5Light },
  { id: "one-dark",          name: "One Dark",          dark: c6Dark,  light: c6Light },
  { id: "monokai",           name: "Monokai",           dark: c7Dark,  light: c7Light },
  { id: "tokyo-night",       name: "Tokyo Night",       dark: c8Dark,  light: c8Light },
  { id: "everforest",        name: "Everforest",        dark: c9Dark,  light: c9Light },
];

// --------------- Utilities ---------------

export function getPalette(id: string): ColorPalette | undefined {
  return PRESETS.find((p) => p.id === id);
}

export function resolvePalette(id: string, custom: ColorPalette | null): ColorPalette {
  if (custom && custom.id === id) return custom;
  return getPalette(id) ?? getPalette(DEFAULT_PALETTE_ID)!;
}
