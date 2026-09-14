// 外观域：终端配色预设（33 套）+ 自定义预设 + 背景图。
// 逻辑从旧 useColorScheme.tsx 移植为响应式单例，localStorage key 原样保留。
import { reactive, watch } from "vue";
import {
  type BackgroundImageConfig,
  type ColorPalette,
  resolvePalette,
  DEFAULT_PALETTE_ID,
  DEFAULT_BG_IMAGE,
  STORAGE_KEY_COLOR,
  STORAGE_KEY_CUSTOM,
  STORAGE_KEY_BG,
} from "../themes";
import { ui } from "./ui";

// 与预设系统对齐的 UI 变量清单（预设可覆盖的令牌范围）
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

function readStored<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function writeStored<T>(key: string, value: T): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* 容量满等场景静默忽略 */
  }
}

interface AppearanceState {
  paletteId: string;
  customPalette: ColorPalette | null;
  bgImage: BackgroundImageConfig;
}

function migrateStoredPalette(stored: string): string {
  // 旧默认值迁移到 Carbon
  return stored === "catppuccin-mocha" ? DEFAULT_PALETTE_ID : stored;
}

export const appearance = reactive<AppearanceState>({
  paletteId: migrateStoredPalette(readStored<string>(STORAGE_KEY_COLOR, DEFAULT_PALETTE_ID)),
  customPalette: readStored<ColorPalette | null>(STORAGE_KEY_CUSTOM, null),
  bgImage: readStored<BackgroundImageConfig>(STORAGE_KEY_BG, DEFAULT_BG_IMAGE),
});

let appliedKeys: string[] = [];

/** 把当前预设的 UI 变量覆盖到文档根（预设切换 / 深浅切换后都要重跑）。 */
export function applyColorPreset(): void {
  const root = document.documentElement;
  // 先清掉上一次的覆盖，避免预设切换后残留
  for (const key of appliedKeys) root.style.removeProperty(key);
  appliedKeys = [];

  const palette = resolvePalette(appearance.paletteId, appearance.customPalette);
  const variant = ui.isDark ? palette.dark : palette.light;
  // 预设内的所有 UI 变量都应用（含非标准自定义变量，与旧实现一致）
  for (const [key, value] of Object.entries(variant.ui)) {
    if (value != null) {
      root.style.setProperty(key, value);
      appliedKeys.push(key);
    }
  }
}

/** 把背景图配置写入根节点（终端工作区背景使用）。 */
export function applyBgImage(): void {
  const root = document.documentElement.style;
  if (appearance.bgImage.dataUrl) {
    root.setProperty("--app-bg-image", `url(${appearance.bgImage.dataUrl})`);
    root.setProperty("--app-bg-opacity", String(appearance.bgImage.opacity));
  } else {
    root.removeProperty("--app-bg-image");
    root.removeProperty("--app-bg-opacity");
  }
}

export function setPaletteId(id: string): void {
  appearance.paletteId = migrateStoredPalette(id);
  writeStored(STORAGE_KEY_COLOR, appearance.paletteId);
  applyColorPreset();
}

export function setCustomPalette(palette: ColorPalette): void {
  appearance.customPalette = palette;
  appearance.paletteId = "custom";
  writeStored(STORAGE_KEY_CUSTOM, palette);
  writeStored(STORAGE_KEY_COLOR, "custom");
  applyColorPreset();
}

export function clearCustomPalette(): void {
  appearance.customPalette = null;
  writeStored(STORAGE_KEY_CUSTOM, null);
  applyColorPreset();
}

export function setBgImage(config: BackgroundImageConfig): void {
  appearance.bgImage = config;
  writeStored(STORAGE_KEY_BG, config);
  applyBgImage();
}

export function getActivePalette(): ColorPalette {
  return resolvePalette(appearance.paletteId, appearance.customPalette);
}

// 深浅主题切换时重应用预设（两个变体的 ui 覆盖不同）
watch(
  () => ui.isDark,
  () => applyColorPreset(),
);

// 初始化（main.ts import 本模块即生效；applyTheme 后 watcher 会再跑一次）
applyColorPreset();
applyBgImage();
