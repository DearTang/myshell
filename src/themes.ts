// ===========================================================================
// Color Palette System — Preset definitions for terminal + global UI theming
// ===========================================================================

// --------------- Types ---------------

/** Full xterm.js ITheme shape for terminal colors */
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

/**
 * Partial map of CSS custom-property name → hex value.
 * Only the variables that differ from the default Catppuccin Mocha
 * dark / Catppuccin Latte light base are included.
 */
export type UIVariables = Partial<Record<string, string>>;

/** One variant (dark or light) of a color palette */
export interface ColorPaletteVariant {
  terminal: TerminalTheme;
  ui: UIVariables;
}

/** A complete named palette with dark + light variants */
export interface ColorPalette {
  id: string;
  name: string;
  dark: ColorPaletteVariant;
  light: ColorPaletteVariant;
}

/** Background image configuration persisted to localStorage */
export interface BackgroundImageConfig {
  dataUrl: string | null;
  opacity: number; // 0–1
}

// --------------- localStorage keys ---------------

export const STORAGE_KEY_COLOR = "myshell-color-scheme";
export const STORAGE_KEY_CUSTOM = "myshell-custom-color-scheme";
export const STORAGE_KEY_BG = "myshell-bg-image";

export const DEFAULT_PALETTE_ID = "catppuccin-mocha";
export const DEFAULT_BG_IMAGE: BackgroundImageConfig = {
  dataUrl: null,
  opacity: 0.85,
};

// --------------- Shared helpers ---------------

function hex(hexStr: string): string {
  return hexStr;
}

// ===========================================================================
// PRESET 1: Catppuccin Mocha (default — mauve/rosewater accents)
// ===========================================================================

const catppuccinMochaDark: ColorPaletteVariant = {
  terminal: {
    background: hex("#1e1e2e"),
    foreground: hex("#cdd6f4"),
    cursor: hex("#f5e0dc"),
    cursorAccent: hex("#1e1e2e"),
    selectionBackground: hex("#585b7055"),
    black: hex("#45475a"),
    red: hex("#f38ba8"),
    green: hex("#a6e3a1"),
    yellow: hex("#f9e2af"),
    blue: hex("#89b4fa"),
    magenta: hex("#f5c2e7"),
    cyan: hex("#94e2d5"),
    white: hex("#bac2de"),
    brightBlack: hex("#585b70"),
    brightRed: hex("#f38ba8"),
    brightGreen: hex("#a6e3a1"),
    brightYellow: hex("#f9e2af"),
    brightBlue: hex("#89b4fa"),
    brightMagenta: hex("#f5c2e7"),
    brightCyan: hex("#94e2d5"),
    brightWhite: hex("#a6adc8"),
  },
  ui: {}, // default — no overrides needed
};

const catppuccinMochaLight: ColorPaletteVariant = {
  terminal: {
    background: hex("#eff1f5"),
    foreground: hex("#4c4f69"),
    cursor: hex("#dc8a78"),
    cursorAccent: hex("#eff1f5"),
    selectionBackground: hex("#acb0be55"),
    black: hex("#5c5f77"),
    red: hex("#d20f39"),
    green: hex("#40a02b"),
    yellow: hex("#df8e1d"),
    blue: hex("#1e66f5"),
    magenta: hex("#ea76cb"),
    cyan: hex("#179299"),
    white: hex("#acb0be"),
    brightBlack: hex("#6c6f85"),
    brightRed: hex("#d20f39"),
    brightGreen: hex("#40a02b"),
    brightYellow: hex("#df8e1d"),
    brightBlue: hex("#1e66f5"),
    brightMagenta: hex("#ea76cb"),
    brightCyan: hex("#179299"),
    brightWhite: hex("#bcc0cc"),
  },
  ui: {},
};

// ===========================================================================
// PRESET 2: Dracula — purple/cyan/pink on dark gray
// ===========================================================================

const draculaDark: ColorPaletteVariant = {
  terminal: {
    background: hex("#282a36"),
    foreground: hex("#f8f8f2"),
    cursor: hex("#f8f8f2"),
    cursorAccent: hex("#282a36"),
    selectionBackground: hex("#44475a"),
    black: hex("#21222c"),
    red: hex("#ff5555"),
    green: hex("#50fa7b"),
    yellow: hex("#f1fa8c"),
    blue: hex("#bd93f9"),
    magenta: hex("#ff79c6"),
    cyan: hex("#8be9fd"),
    white: hex("#f8f8f2"),
    brightBlack: hex("#6272a4"),
    brightRed: hex("#ff6e6e"),
    brightGreen: hex("#69ff94"),
    brightYellow: hex("#ffffa5"),
    brightBlue: hex("#d6acff"),
    brightMagenta: hex("#ff92df"),
    brightCyan: hex("#a4ffff"),
    brightWhite: hex("#ffffff"),
  },
  ui: {
    "--bg-base": "#1e1f29",
    "--bg-elevated": "#282a36",
    "--bg-surface": "#343746",
    "--bg-surface-hover": "#3d3f53",
    "--bg-surface-active": "#44475a",
    "--bg-input": "#2a2c3d",
    "--bg-input-hover": "#34364b",
    "--accent-primary": "#bd93f9",
    "--accent-primary-hover": "#d6acff",
    "--accent-primary-muted": "rgba(189, 147, 249, 0.15)",
    "--accent-secondary": "#8be9fd",
    "--accent-secondary-muted": "rgba(139, 233, 253, 0.15)",
    "--success": "#50fa7b",
    "--warning": "#f1fa8c",
    "--error": "#ff5555",
    "--info": "#8be9fd",
    "--border-accent": "rgba(189, 147, 249, 0.4)",
    "--shadow-glow": "0 0 20px rgba(189, 147, 249, 0.25)",
    "--glass-bg": "rgba(40, 42, 54, 0.85)",
    "--glass-border": "rgba(255, 255, 255, 0.08)",
  },
};

const draculaLight: ColorPaletteVariant = {
  terminal: {
    background: hex("#f8f8f2"),
    foreground: hex("#282a36"),
    cursor: hex("#282a36"),
    cursorAccent: hex("#f8f8f2"),
    selectionBackground: hex("#44475a44"),
    black: hex("#f8f8f2"),
    red: hex("#ff5555"),
    green: hex("#50fa7b"),
    yellow: hex("#bd9300"),
    blue: hex("#bd93f9"),
    magenta: hex("#ff79c6"),
    cyan: hex("#18918e"),
    white: hex("#282a36"),
    brightBlack: hex("#bfbfbf"),
    brightRed: hex("#e04040"),
    brightGreen: hex("#40c960"),
    brightYellow: hex("#9e7a00"),
    brightBlue: hex("#9e6ff5"),
    brightMagenta: hex("#e05ab0"),
    brightCyan: hex("#1d8a88"),
    brightWhite: hex("#44475a"),
  },
  ui: {
    "--bg-base": "#f8f8f2",
    "--bg-elevated": "#ffffff",
    "--bg-surface": "#f0f0ec",
    "--bg-surface-hover": "#e6e6e0",
    "--accent-primary": "#bd93f9",
    "--accent-primary-hover": "#9e6ff5",
    "--accent-secondary": "#18918e",
  },
};

// ===========================================================================
// PRESET 3: Nord — frosty blue-gray, cool and calm
// ===========================================================================

const nordDark: ColorPaletteVariant = {
  terminal: {
    background: hex("#2e3440"),
    foreground: hex("#d8dee9"),
    cursor: hex("#d8dee9"),
    cursorAccent: hex("#2e3440"),
    selectionBackground: hex("#4c566a66"),
    black: hex("#3b4252"),
    red: hex("#bf616a"),
    green: hex("#a3be8c"),
    yellow: hex("#ebcb8b"),
    blue: hex("#81a1c1"),
    magenta: hex("#b48ead"),
    cyan: hex("#88c0d0"),
    white: hex("#e5e9f0"),
    brightBlack: hex("#4c566a"),
    brightRed: hex("#bf616a"),
    brightGreen: hex("#a3be8c"),
    brightYellow: hex("#ebcb8b"),
    brightBlue: hex("#81a1c1"),
    brightMagenta: hex("#b48ead"),
    brightCyan: hex("#8fbcbb"),
    brightWhite: hex("#eceff4"),
  },
  ui: {
    "--bg-base": "#242933",
    "--bg-elevated": "#2e3440",
    "--bg-surface": "#3b4252",
    "--bg-surface-hover": "#434c5e",
    "--bg-surface-active": "#4c566a",
    "--bg-input": "#353b4a",
    "--bg-input-hover": "#3f4758",
    "--accent-primary": "#88c0d0",
    "--accent-primary-hover": "#8fbcbb",
    "--accent-primary-muted": "rgba(136, 192, 208, 0.15)",
    "--accent-secondary": "#81a1c1",
    "--accent-secondary-muted": "rgba(129, 161, 193, 0.15)",
    "--success": "#a3be8c",
    "--warning": "#ebcb8b",
    "--error": "#bf616a",
    "--info": "#88c0d0",
    "--border-accent": "rgba(136, 192, 208, 0.4)",
    "--shadow-glow": "0 0 20px rgba(136, 192, 208, 0.2)",
    "--glass-bg": "rgba(46, 52, 64, 0.85)",
    "--glass-border": "rgba(255, 255, 255, 0.06)",
  },
};

const nordLight: ColorPaletteVariant = {
  terminal: {
    background: hex("#eceff4"),
    foreground: hex("#2e3440"),
    cursor: hex("#2e3440"),
    cursorAccent: hex("#eceff4"),
    selectionBackground: hex("#d8dee955"),
    black: hex("#3b4252"),
    red: hex("#bf616a"),
    green: hex("#a3be8c"),
    yellow: hex("#ebcb8b"),
    blue: hex("#81a1c1"),
    magenta: hex("#b48ead"),
    cyan: hex("#88c0d0"),
    white: hex("#e5e9f0"),
    brightBlack: hex("#4c566a"),
    brightRed: hex("#bf616a"),
    brightGreen: hex("#a3be8c"),
    brightYellow: hex("#ebcb8b"),
    brightBlue: hex("#81a1c1"),
    brightMagenta: hex("#b48ead"),
    brightCyan: hex("#8fbcbb"),
    brightWhite: hex("#eceff4"),
  },
  ui: {
    "--bg-base": "#eceff4",
    "--bg-elevated": "#ffffff",
    "--bg-surface": "#e5e9f0",
    "--bg-surface-hover": "#d8dee9",
    "--accent-primary": "#5e81ac",
    "--accent-primary-hover": "#81a1c1",
    "--accent-secondary": "#88c0d0",
  },
};

// ===========================================================================
// PRESET 4: Solarized Dark — warm dark with yellow/blue accents
// ===========================================================================

const solarizedDark: ColorPaletteVariant = {
  terminal: {
    background: hex("#002b36"),
    foreground: hex("#839496"),
    cursor: hex("#839496"),
    cursorAccent: hex("#002b36"),
    selectionBackground: hex("#586e7566"),
    black: hex("#073642"),
    red: hex("#dc322f"),
    green: hex("#859900"),
    yellow: hex("#b58900"),
    blue: hex("#268bd2"),
    magenta: hex("#d33682"),
    cyan: hex("#2aa198"),
    white: hex("#eee8d5"),
    brightBlack: hex("#586e75"),
    brightRed: hex("#dc322f"),
    brightGreen: hex("#859900"),
    brightYellow: hex("#b58900"),
    brightBlue: hex("#268bd2"),
    brightMagenta: hex("#d33682"),
    brightCyan: hex("#2aa198"),
    brightWhite: hex("#fdf6e3"),
  },
  ui: {
    "--bg-base": "#001f27",
    "--bg-elevated": "#002b36",
    "--bg-surface": "#073642",
    "--bg-surface-hover": "#0d4959",
    "--bg-surface-active": "#11576b",
    "--bg-input": "#05323f",
    "--bg-input-hover": "#0a3f4d",
    "--accent-primary": "#268bd2",
    "--accent-primary-hover": "#6caddf",
    "--accent-primary-muted": "rgba(38, 139, 210, 0.15)",
    "--accent-secondary": "#2aa198",
    "--accent-secondary-muted": "rgba(42, 161, 152, 0.15)",
    "--success": "#859900",
    "--warning": "#b58900",
    "--error": "#dc322f",
    "--info": "#268bd2",
    "--border-accent": "rgba(38, 139, 210, 0.4)",
    "--shadow-glow": "0 0 20px rgba(38, 139, 210, 0.2)",
    "--glass-bg": "rgba(0, 43, 54, 0.85)",
    "--glass-border": "rgba(255, 255, 255, 0.06)",
  },
};

const solarizedLight: ColorPaletteVariant = {
  terminal: {
    background: hex("#fdf6e3"),
    foreground: hex("#586e75"),
    cursor: hex("#586e75"),
    cursorAccent: hex("#fdf6e3"),
    selectionBackground: hex("#eee8d555"),
    black: hex("#002b36"),
    red: hex("#dc322f"),
    green: hex("#859900"),
    yellow: hex("#b58900"),
    blue: hex("#268bd2"),
    magenta: hex("#d33682"),
    cyan: hex("#2aa198"),
    white: hex("#eee8d5"),
    brightBlack: hex("#93a1a1"),
    brightRed: hex("#dc322f"),
    brightGreen: hex("#859900"),
    brightYellow: hex("#b58900"),
    brightBlue: hex("#268bd2"),
    brightMagenta: hex("#d33682"),
    brightCyan: hex("#2aa198"),
    brightWhite: hex("#fdf6e3"),
  },
  ui: {
    "--bg-base": "#fdf6e3",
    "--bg-elevated": "#ffffff",
    "--bg-surface": "#eee8d5",
    "--bg-surface-hover": "#e0d9c3",
    "--accent-primary": "#268bd2",
    "--accent-primary-hover": "#1d6faa",
    "--accent-secondary": "#2aa198",
  },
};

// ===========================================================================
// PRESET 5: Gruvbox Dark — retro warm earthy tones
// ===========================================================================

const gruvboxDarkDark: ColorPaletteVariant = {
  terminal: {
    background: hex("#282828"),
    foreground: hex("#ebdbb2"),
    cursor: hex("#ebdbb2"),
    cursorAccent: hex("#282828"),
    selectionBackground: hex("#665c5466"),
    black: hex("#3c3836"),
    red: hex("#cc241d"),
    green: hex("#98971a"),
    yellow: hex("#d79921"),
    blue: hex("#458588"),
    magenta: hex("#b16286"),
    cyan: hex("#689d6a"),
    white: hex("#a89984"),
    brightBlack: hex("#7c6f64"),
    brightRed: hex("#fb4934"),
    brightGreen: hex("#b8bb26"),
    brightYellow: hex("#fabd2f"),
    brightBlue: hex("#83a598"),
    brightMagenta: hex("#d3869b"),
    brightCyan: hex("#8ec07c"),
    brightWhite: hex("#ebdbb2"),
  },
  ui: {
    "--bg-base": "#1d2021",
    "--bg-elevated": "#282828",
    "--bg-surface": "#3c3836",
    "--bg-surface-hover": "#504945",
    "--bg-surface-active": "#665c54",
    "--bg-input": "#32302f",
    "--bg-input-hover": "#3c3836",
    "--accent-primary": "#d79921",
    "--accent-primary-hover": "#fabd2f",
    "--accent-primary-muted": "rgba(215, 153, 33, 0.15)",
    "--accent-secondary": "#83a598",
    "--accent-secondary-muted": "rgba(131, 165, 152, 0.15)",
    "--success": "#98971a",
    "--warning": "#d79921",
    "--error": "#cc241d",
    "--info": "#458588",
    "--border-accent": "rgba(215, 153, 33, 0.4)",
    "--shadow-glow": "0 0 20px rgba(215, 153, 33, 0.25)",
    "--glass-bg": "rgba(40, 40, 40, 0.85)",
    "--glass-border": "rgba(255, 255, 255, 0.06)",
  },
};

const gruvboxDarkLight: ColorPaletteVariant = {
  terminal: {
    background: hex("#fbf1c7"),
    foreground: hex("#3c3836"),
    cursor: hex("#3c3836"),
    cursorAccent: hex("#fbf1c7"),
    selectionBackground: hex("#d5c4a155"),
    black: hex("#fbf1c7"),
    red: hex("#cc241d"),
    green: hex("#98971a"),
    yellow: hex("#d79921"),
    blue: hex("#458588"),
    magenta: hex("#b16286"),
    cyan: hex("#689d6a"),
    white: hex("#7c6f64"),
    brightBlack: hex("#a89984"),
    brightRed: hex("#cc241d"),
    brightGreen: hex("#98971a"),
    brightYellow: hex("#d79921"),
    brightBlue: hex("#458588"),
    brightMagenta: hex("#b16286"),
    brightCyan: hex("#689d6a"),
    brightWhite: hex("#282828"),
  },
  ui: {
    "--bg-base": "#fbf1c7",
    "--bg-elevated": "#ffffff",
    "--bg-surface": "#f2e5bc",
    "--bg-surface-hover": "#ebdbb2",
    "--accent-primary": "#b57614",
    "--accent-primary-hover": "#d79921",
    "--accent-secondary": "#458588",
  },
};

// ===========================================================================
// PRESET 6: One Dark — Atom editor classic
// ===========================================================================

const oneDarkDark: ColorPaletteVariant = {
  terminal: {
    background: hex("#282c34"),
    foreground: hex("#abb2bf"),
    cursor: hex("#528bff"),
    cursorAccent: hex("#282c34"),
    selectionBackground: hex("#3e4451"),
    black: hex("#282c34"),
    red: hex("#e06c75"),
    green: hex("#98c379"),
    yellow: hex("#e5c07b"),
    blue: hex("#61afef"),
    magenta: hex("#c678dd"),
    cyan: hex("#56b6c2"),
    white: hex("#abb2bf"),
    brightBlack: hex("#5c6370"),
    brightRed: hex("#e06c75"),
    brightGreen: hex("#98c379"),
    brightYellow: hex("#e5c07b"),
    brightBlue: hex("#61afef"),
    brightMagenta: hex("#c678dd"),
    brightCyan: hex("#56b6c2"),
    brightWhite: hex("#ffffff"),
  },
  ui: {
    "--bg-base": "#21252b",
    "--bg-elevated": "#282c34",
    "--bg-surface": "#313640",
    "--bg-surface-hover": "#3b404b",
    "--bg-surface-active": "#444a57",
    "--bg-input": "#2c313a",
    "--bg-input-hover": "#353b47",
    "--accent-primary": "#61afef",
    "--accent-primary-hover": "#8cc8f4",
    "--accent-primary-muted": "rgba(97, 175, 239, 0.15)",
    "--accent-secondary": "#56b6c2",
    "--accent-secondary-muted": "rgba(86, 182, 194, 0.15)",
    "--success": "#98c379",
    "--warning": "#e5c07b",
    "--error": "#e06c75",
    "--info": "#61afef",
    "--border-accent": "rgba(97, 175, 239, 0.4)",
    "--shadow-glow": "0 0 20px rgba(97, 175, 239, 0.25)",
    "--glass-bg": "rgba(40, 44, 52, 0.85)",
    "--glass-border": "rgba(255, 255, 255, 0.08)",
  },
};

const oneDarkLight: ColorPaletteVariant = {
  terminal: {
    background: hex("#fafafa"),
    foreground: hex("#383a42"),
    cursor: hex("#526fff"),
    cursorAccent: hex("#fafafa"),
    selectionBackground: hex("#e5e5e6"),
    black: hex("#fafafa"),
    red: hex("#e45649"),
    green: hex("#50a14f"),
    yellow: hex("#c18401"),
    blue: hex("#4078f2"),
    magenta: hex("#a626a4"),
    cyan: hex("#0184bc"),
    white: hex("#a0a1a7"),
    brightBlack: hex("#a0a1a7"),
    brightRed: hex("#e45649"),
    brightGreen: hex("#50a14f"),
    brightYellow: hex("#c18401"),
    brightBlue: hex("#4078f2"),
    brightMagenta: hex("#a626a4"),
    brightCyan: hex("#0184bc"),
    brightWhite: hex("#383a42"),
  },
  ui: {
    "--bg-base": "#fafafa",
    "--bg-elevated": "#ffffff",
    "--bg-surface": "#f0f0f1",
    "--bg-surface-hover": "#e5e5e6",
    "--accent-primary": "#4078f2",
    "--accent-primary-hover": "#526fff",
    "--accent-secondary": "#0184bc",
  },
};

// ===========================================================================
// PRESET 7: Monokai — classic Sublime Text colors
// ===========================================================================

const monokaiDark: ColorPaletteVariant = {
  terminal: {
    background: hex("#272822"),
    foreground: hex("#f8f8f2"),
    cursor: hex("#f8f8f0"),
    cursorAccent: hex("#272822"),
    selectionBackground: hex("#49483e"),
    black: hex("#272822"),
    red: hex("#f92672"),
    green: hex("#a6e22e"),
    yellow: hex("#f4bf75"),
    blue: hex("#66d9ef"),
    magenta: hex("#ae81ff"),
    cyan: hex("#a1efe4"),
    white: hex("#f8f8f2"),
    brightBlack: hex("#75715e"),
    brightRed: hex("#f92672"),
    brightGreen: hex("#a6e22e"),
    brightYellow: hex("#f4bf75"),
    brightBlue: hex("#66d9ef"),
    brightMagenta: hex("#ae81ff"),
    brightCyan: hex("#a1efe4"),
    brightWhite: hex("#f9f8f5"),
  },
  ui: {
    "--bg-base": "#1e1f1a",
    "--bg-elevated": "#272822",
    "--bg-surface": "#34352e",
    "--bg-surface-hover": "#3e3f36",
    "--bg-surface-active": "#49483e",
    "--bg-input": "#2d2e27",
    "--bg-input-hover": "#373830",
    "--accent-primary": "#a6e22e",
    "--accent-primary-hover": "#c4f05a",
    "--accent-primary-muted": "rgba(166, 226, 46, 0.15)",
    "--accent-secondary": "#66d9ef",
    "--accent-secondary-muted": "rgba(102, 217, 239, 0.15)",
    "--success": "#a6e22e",
    "--warning": "#f4bf75",
    "--error": "#f92672",
    "--info": "#66d9ef",
    "--border-accent": "rgba(166, 226, 46, 0.4)",
    "--shadow-glow": "0 0 20px rgba(166, 226, 46, 0.2)",
    "--glass-bg": "rgba(39, 40, 34, 0.85)",
    "--glass-border": "rgba(255, 255, 255, 0.08)",
  },
};

const monokaiLight: ColorPaletteVariant = {
  terminal: {
    background: hex("#f9f8f5"),
    foreground: hex("#272822"),
    cursor: hex("#272822"),
    cursorAccent: hex("#f9f8f5"),
    selectionBackground: hex("#e6e5e055"),
    black: hex("#272822"),
    red: hex("#f92672"),
    green: hex("#82b414"),
    yellow: hex("#c48100"),
    blue: hex("#33b1cc"),
    magenta: hex("#8c4fdb"),
    cyan: hex("#55b4a0"),
    white: hex("#ccccc7"),
    brightBlack: hex("#75715e"),
    brightRed: hex("#f92672"),
    brightGreen: hex("#a6e22e"),
    brightYellow: hex("#e6a332"),
    brightBlue: hex("#66d9ef"),
    brightMagenta: hex("#ae81ff"),
    brightCyan: hex("#a1efe4"),
    brightWhite: hex("#f9f8f5"),
  },
  ui: {
    "--bg-base": "#f9f8f5",
    "--bg-elevated": "#ffffff",
    "--bg-surface": "#f0efe9",
    "--bg-surface-hover": "#e6e5e0",
    "--accent-primary": "#82b414",
    "--accent-primary-hover": "#a6e22e",
    "--accent-secondary": "#33b1cc",
  },
};

// ===========================================================================
// PRESET 8: Tokyo Night — deep navy with neon accents
// ===========================================================================

const tokyoNightDark: ColorPaletteVariant = {
  terminal: {
    background: hex("#1a1b26"),
    foreground: hex("#a9b1d6"),
    cursor: hex("#c0caf5"),
    cursorAccent: hex("#1a1b26"),
    selectionBackground: hex("#33467c"),
    black: hex("#32344a"),
    red: hex("#f7768e"),
    green: hex("#9ece6a"),
    yellow: hex("#e0af68"),
    blue: hex("#7aa2f7"),
    magenta: hex("#ad8ee7"),
    cyan: hex("#449dab"),
    white: hex("#787c99"),
    brightBlack: hex("#444b6a"),
    brightRed: hex("#f7768e"),
    brightGreen: hex("#9ece6a"),
    brightYellow: hex("#e0af68"),
    brightBlue: hex("#7aa2f7"),
    brightMagenta: hex("#bb9af7"),
    brightCyan: hex("#7dcfff"),
    brightWhite: hex("#c0caf5"),
  },
  ui: {
    "--bg-base": "#13141d",
    "--bg-elevated": "#1a1b26",
    "--bg-surface": "#24283b",
    "--bg-surface-hover": "#2f334d",
    "--bg-surface-active": "#3b4261",
    "--bg-input": "#1e2030",
    "--bg-input-hover": "#292e42",
    "--accent-primary": "#7aa2f7",
    "--accent-primary-hover": "#a9bcf7",
    "--accent-primary-muted": "rgba(122, 162, 247, 0.15)",
    "--accent-secondary": "#7dcfff",
    "--accent-secondary-muted": "rgba(125, 207, 255, 0.15)",
    "--success": "#9ece6a",
    "--warning": "#e0af68",
    "--error": "#f7768e",
    "--info": "#7aa2f7",
    "--border-accent": "rgba(122, 162, 247, 0.4)",
    "--shadow-glow": "0 0 20px rgba(122, 162, 247, 0.3)",
    "--glass-bg": "rgba(26, 27, 38, 0.85)",
    "--glass-border": "rgba(255, 255, 255, 0.06)",
  },
};

const tokyoNightLight: ColorPaletteVariant = {
  terminal: {
    background: hex("#d5d6db"),
    foreground: hex("#343b58"),
    cursor: hex("#343b58"),
    cursorAccent: hex("#d5d6db"),
    selectionBackground: hex("#b7b9c055"),
    black: hex("#d5d6db"),
    red: hex("#8c4351"),
    green: hex("#485e30"),
    yellow: hex("#8f5e15"),
    blue: hex("#34548a"),
    magenta: hex("#5a4a78"),
    cyan: hex("#0f4b6e"),
    white: hex("#343b58"),
    brightBlack: hex("#9699a3"),
    brightRed: hex("#8c4351"),
    brightGreen: hex("#485e30"),
    brightYellow: hex("#8f5e15"),
    brightBlue: hex("#34548a"),
    brightMagenta: hex("#5a4a78"),
    brightCyan: hex("#0f4b6e"),
    brightWhite: hex("#1a1b26"),
  },
  ui: {
    "--bg-base": "#d5d6db",
    "--bg-elevated": "#e9eaed",
    "--bg-surface": "#cbccd1",
    "--bg-surface-hover": "#c0c1c8",
    "--accent-primary": "#34548a",
    "--accent-primary-hover": "#436caf",
    "--accent-secondary": "#0f4b6e",
  },
};

// ===========================================================================
// PRESET 9: Everforest — green-tinted dark, nature-inspired
// ===========================================================================

const everforestDark: ColorPaletteVariant = {
  terminal: {
    background: hex("#2d353b"),
    foreground: hex("#d3c6aa"),
    cursor: hex("#d3c6aa"),
    cursorAccent: hex("#2d353b"),
    selectionBackground: hex("#47525866"),
    black: hex("#475258"),
    red: hex("#e67e80"),
    green: hex("#a7c080"),
    yellow: hex("#dbbc7f"),
    blue: hex("#7fbbb3"),
    magenta: hex("#d699b6"),
    cyan: hex("#83c092"),
    white: hex("#d3c6aa"),
    brightBlack: hex("#7a8478"),
    brightRed: hex("#e67e80"),
    brightGreen: hex("#a7c080"),
    brightYellow: hex("#dbbc7f"),
    brightBlue: hex("#7fbbb3"),
    brightMagenta: hex("#d699b6"),
    brightCyan: hex("#83c092"),
    brightWhite: hex("#e5dfc5"),
  },
  ui: {
    "--bg-base": "#232a2e",
    "--bg-elevated": "#2d353b",
    "--bg-surface": "#343f44",
    "--bg-surface-hover": "#3d484d",
    "--bg-surface-active": "#475258",
    "--bg-input": "#303b40",
    "--bg-input-hover": "#3a454a",
    "--accent-primary": "#a7c080",
    "--accent-primary-hover": "#c1d9a0",
    "--accent-primary-muted": "rgba(167, 192, 128, 0.15)",
    "--accent-secondary": "#7fbbb3",
    "--accent-secondary-muted": "rgba(127, 187, 179, 0.15)",
    "--success": "#a7c080",
    "--warning": "#dbbc7f",
    "--error": "#e67e80",
    "--info": "#7fbbb3",
    "--border-accent": "rgba(167, 192, 128, 0.4)",
    "--shadow-glow": "0 0 20px rgba(167, 192, 128, 0.2)",
    "--glass-bg": "rgba(45, 53, 59, 0.85)",
    "--glass-border": "rgba(255, 255, 255, 0.06)",
  },
};

const everforestLight: ColorPaletteVariant = {
  terminal: {
    background: hex("#fdf6e3"),
    foreground: hex("#5c6a72"),
    cursor: hex("#5c6a72"),
    cursorAccent: hex("#fdf6e3"),
    selectionBackground: hex("#e0dcc755"),
    black: hex("#fdf6e3"),
    red: hex("#f85552"),
    green: hex("#8da101"),
    yellow: hex("#dfa000"),
    blue: hex("#3a94c5"),
    magenta: hex("#df69ba"),
    cyan: hex("#35a77c"),
    white: hex("#5c6a72"),
    brightBlack: hex("#939f91"),
    brightRed: hex("#f85552"),
    brightGreen: hex("#8da101"),
    brightYellow: hex("#dfa000"),
    brightBlue: hex("#3a94c5"),
    brightMagenta: hex("#df69ba"),
    brightCyan: hex("#35a77c"),
    brightWhite: hex("#2d353b"),
  },
  ui: {
    "--bg-base": "#fdf6e3",
    "--bg-elevated": "#ffffff",
    "--bg-surface": "#f4efd9",
    "--bg-surface-hover": "#e8e2ca",
    "--accent-primary": "#8da101",
    "--accent-primary-hover": "#a7c080",
    "--accent-secondary": "#3a94c5",
  },
};

// ===========================================================================
// PRESET 10: Catppuccin Latte — warm light base palette
// ===========================================================================

const catppuccinLatteDark: ColorPaletteVariant = {
  terminal: {
    background: hex("#1e1e2e"),
    foreground: hex("#cdd6f4"),
    cursor: hex("#f5e0dc"),
    cursorAccent: hex("#1e1e2e"),
    selectionBackground: hex("#585b7055"),
    black: hex("#45475a"),
    red: hex("#f38ba8"),
    green: hex("#a6e3a1"),
    yellow: hex("#f9e2af"),
    blue: hex("#89b4fa"),
    magenta: hex("#f5c2e7"),
    cyan: hex("#94e2d5"),
    white: hex("#bac2de"),
    brightBlack: hex("#585b70"),
    brightRed: hex("#f38ba8"),
    brightGreen: hex("#a6e3a1"),
    brightYellow: hex("#f9e2af"),
    brightBlue: hex("#89b4fa"),
    brightMagenta: hex("#f5c2e7"),
    brightCyan: hex("#94e2d5"),
    brightWhite: hex("#a6adc8"),
  },
  ui: {},
};

const catppuccinLatteLight: ColorPaletteVariant = {
  terminal: {
    background: hex("#eff1f5"),
    foreground: hex("#4c4f69"),
    cursor: hex("#dc8a78"),
    cursorAccent: hex("#eff1f5"),
    selectionBackground: hex("#acb0be55"),
    black: hex("#5c5f77"),
    red: hex("#d20f39"),
    green: hex("#40a02b"),
    yellow: hex("#df8e1d"),
    blue: hex("#1e66f5"),
    magenta: hex("#ea76cb"),
    cyan: hex("#179299"),
    white: hex("#acb0be"),
    brightBlack: hex("#6c6f85"),
    brightRed: hex("#d20f39"),
    brightGreen: hex("#40a02b"),
    brightYellow: hex("#df8e1d"),
    brightBlue: hex("#1e66f5"),
    brightMagenta: hex("#ea76cb"),
    brightCyan: hex("#179299"),
    brightWhite: hex("#bcc0cc"),
  },
  ui: {},
};

// ===========================================================================
// Preset registry
// ===========================================================================

export const PRESETS: ColorPalette[] = [
  { id: "catppuccin-mocha", name: "Catppuccin Mocha", dark: catppuccinMochaDark, light: catppuccinMochaLight },
  { id: "catppuccin-latte", name: "Catppuccin Latte", dark: catppuccinLatteDark, light: catppuccinLatteLight },
  { id: "dracula", name: "Dracula", dark: draculaDark, light: draculaLight },
  { id: "nord", name: "Nord", dark: nordDark, light: nordLight },
  { id: "solarized-dark", name: "Solarized Dark", dark: solarizedDark, light: solarizedLight },
  { id: "gruvbox-dark", name: "Gruvbox Dark", dark: gruvboxDarkDark, light: gruvboxDarkLight },
  { id: "one-dark", name: "One Dark", dark: oneDarkDark, light: oneDarkLight },
  { id: "monokai", name: "Monokai", dark: monokaiDark, light: monokaiLight },
  { id: "tokyo-night", name: "Tokyo Night", dark: tokyoNightDark, light: tokyoNightLight },
  { id: "everforest", name: "Everforest", dark: everforestDark, light: everforestLight },
];

// --------------- Utilities ---------------

export function getPalette(id: string): ColorPalette | undefined {
  return PRESETS.find((p) => p.id === id);
}

/**
 * Resolve a palette by ID, checking the preset registry first then falling
 * back to the user's custom palette (if any). Returns the default Catppuccin
 * Mocha palette as the ultimate fallback.
 */
export function resolvePalette(
  id: string,
  custom: ColorPalette | null
): ColorPalette {
  if (custom && custom.id === id) return custom;
  const preset = getPalette(id);
  return preset ?? getPalette(DEFAULT_PALETTE_ID)!;
}
