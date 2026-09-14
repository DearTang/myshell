import {
  createContext,
  useContext,
  useEffect,
  useState,
  useRef,
  useCallback,
  type ReactNode,
} from "react";
import { useTheme } from "./useTheme";
import {
  type ColorPalette,
  type BackgroundImageConfig,
  resolvePalette,
  DEFAULT_PALETTE_ID,
  DEFAULT_BG_IMAGE,
  STORAGE_KEY_COLOR,
  STORAGE_KEY_CUSTOM,
  STORAGE_KEY_BG,
} from "../themes";

// --------------- Types ---------------

export interface ColorSchemeContextValue {
  /** Currently selected preset palette ID */
  paletteId: string;
  /** Switch to a preset by ID */
  setPaletteId: (id: string) => void;
  /** User-defined custom palette (null if none) */
  customPalette: ColorPalette | null;
  /** Save/update the custom palette */
  setCustomPalette: (palette: ColorPalette) => void;
  /** Delete the custom palette */
  clearCustomPalette: () => void;
  /** Resolve the active palette (custom overrides preset) */
  getActivePalette: () => ColorPalette;
  /** Background image configuration */
  bgImage: BackgroundImageConfig;
  /** Update background image settings */
  setBgImage: (config: BackgroundImageConfig) => void;
}

// --------------- Context ---------------

const ColorSchemeContext = createContext<ColorSchemeContextValue | null>(null);

// --------------- CSS variable keys to manage ---------------

const UI_VAR_KEYS = [
  "--bg-base",
  "--bg-elevated",
  "--bg-surface",
  "--bg-surface-hover",
  "--bg-surface-active",
  "--bg-input",
  "--bg-input-hover",
  "--accent-primary",
  "--accent-primary-hover",
  "--accent-primary-muted",
  "--accent-secondary",
  "--accent-secondary-muted",
  "--success",
  "--success-muted",
  "--warning",
  "--warning-muted",
  "--error",
  "--error-muted",
  "--info",
  "--info-muted",
  "--border-accent",
  "--shadow-glow",
  "--glass-bg",
  "--glass-border",
];

// --------------- Helper ---------------

function isBrowser(): boolean {
  return typeof window !== "undefined";
}

function readStored<T>(key: string, fallback: T): T {
  if (!isBrowser()) return fallback;
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function writeStored<T>(key: string, value: T): void {
  if (!isBrowser()) return;
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // localStorage full or unavailable — silently ignore
  }
}

// --------------- Provider ---------------

export function ColorSchemeProvider({ children }: { children: ReactNode }) {
  const { theme } = useTheme();

  const [paletteId, setPaletteIdRaw] = useState<string>(() => {
    const stored = readStored<string>(STORAGE_KEY_COLOR, DEFAULT_PALETTE_ID);
    // Migrate old default to new Carbon design
    if (stored === "catppuccin-mocha") return DEFAULT_PALETTE_ID;
    return stored;
  });

  const [customPalette, setCustomPaletteRaw] = useState<ColorPalette | null>(
    () => readStored<ColorPalette | null>(STORAGE_KEY_CUSTOM, null)
  );

  const [bgImage, setBgImageRaw] = useState<BackgroundImageConfig>(() =>
    readStored(STORAGE_KEY_BG, DEFAULT_BG_IMAGE)
  );

  // Track which CSS properties we've overridden so we can clean them up
  const appliedRef = useRef<Set<string>>(new Set());

  // Apply CSS variable overrides whenever palette or theme changes
  useEffect(() => {
    const root = document.documentElement;
    const palette = resolvePalette(paletteId, customPalette);
    const variant = theme === "dark" ? palette.dark : palette.light;

    // Remove previously-applied overrides
    appliedRef.current.forEach((key) => {
      root.style.removeProperty(key);
    });
    appliedRef.current.clear();

    // Apply new overrides
    const ui = variant.ui;
    for (const key of UI_VAR_KEYS) {
      if (key in ui) {
        const value = ui[key as keyof typeof ui]!;
        root.style.setProperty(key, value);
        appliedRef.current.add(key);
      }
    }
    // Also apply any custom (non-standard) properties the palette may define
    for (const [key, value] of Object.entries(ui)) {
      if (!UI_VAR_KEYS.includes(key) && value != null) {
        root.style.setProperty(key, value);
        appliedRef.current.add(key);
      }
    }
  }, [paletteId, customPalette, theme]);

  // ── State setters with persistence ──

  const setPaletteId = useCallback((id: string) => {
    setPaletteIdRaw(id);
    writeStored(STORAGE_KEY_COLOR, id);
  }, []);

  const setCustomPalette = useCallback((palette: ColorPalette) => {
    setCustomPaletteRaw(palette);
    writeStored(STORAGE_KEY_CUSTOM, palette);
  }, []);

  const clearCustomPalette = useCallback(() => {
    setCustomPaletteRaw(null);
    writeStored(STORAGE_KEY_CUSTOM, null);
  }, []);

  const setBgImage = useCallback((config: BackgroundImageConfig) => {
    setBgImageRaw(config);
    writeStored(STORAGE_KEY_BG, config);
  }, []);

  const getActivePalette = useCallback((): ColorPalette => {
    return resolvePalette(paletteId, customPalette);
  }, [paletteId, customPalette]);

  return (
    <ColorSchemeContext.Provider
      value={{
        paletteId,
        setPaletteId,
        customPalette,
        setCustomPalette,
        clearCustomPalette,
        getActivePalette,
        bgImage,
        setBgImage,
      }}
    >
      {children}
    </ColorSchemeContext.Provider>
  );
}

// --------------- Consumer hook ---------------

export function useColorScheme(): ColorSchemeContextValue {
  const context = useContext(ColorSchemeContext);
  if (!context) {
    throw new Error(
      "useColorScheme must be used within a ColorSchemeProvider"
    );
  }
  return context;
}
