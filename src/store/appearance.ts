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
  STORAGE_KEY_SOLIDITY,
  STORAGE_KEY_POPUP_SOLIDITY,
} from "../themes";
import { ui } from "./ui";

// 预设可覆盖的 UI 变量范围由 themes.ts 的 UIVariables 类型约束；
// 应用侧不维护第二份清单 —— 旧版这里有一份 UI_VAR_KEYS 常量，从未被读取，
// 且会随令牌增删漂移（例如 --glass-bg 已改为实心度插值后它仍列着该项）。

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
  /** 界面实心度 0-1（0 = 基线玻璃，1 = 完全不透明）。 */
  uiSolidity: number;
  /** 弹窗/浮层实心度 0-1，默认比界面更实（弹层压在文字上）。 */
  popupSolidity: number;
}

function migrateStoredPalette(stored: string): string {
  // 旧默认值迁移到 Carbon
  return stored === "catppuccin-mocha" ? DEFAULT_PALETTE_ID : stored;
}

/** 实心度收敛：localStorage 里的旧值/外部改动都可能带出界，读写两侧都过一遍。
 *  fallback 必填 —— 界面与弹窗的默认值不同（0.25 / 0.55），不能共用一个兜底。 */
function clampSolidity(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(1, Math.max(0, value))
    : fallback;
}

export const appearance = reactive<AppearanceState>({
  paletteId: migrateStoredPalette(readStored<string>(STORAGE_KEY_COLOR, DEFAULT_PALETTE_ID)),
  customPalette: readStored<ColorPalette | null>(STORAGE_KEY_CUSTOM, null),
  bgImage: readStored<BackgroundImageConfig>(STORAGE_KEY_BG, DEFAULT_BG_IMAGE),
  uiSolidity: clampSolidity(readStored<number>(STORAGE_KEY_SOLIDITY, 0.25), 0.25),
  popupSolidity: clampSolidity(readStored<number>(STORAGE_KEY_POPUP_SOLIDITY, 0.55), 0.55),
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

/** 把背景图配置写入根节点（body 级壁纸层使用，全窗透出）。 */
export function applyBgImage(): void {
  const root = document.documentElement.style;
  const html = document.documentElement;
  if (appearance.bgImage.dataUrl) {
    // 用 JSON.stringify 把 URL 包成带引号的 CSS 字符串：data URL 里可能出现
    // `#` / `)` / 空白（SVG 的 `data:image/svg+xml,...` 就有），裸写进
    // `url(...)` 会被 CSS 解析器判为非法 —— setProperty 静默失败，
    // 属性根本没写进去（实测 --app-bg-opacity 成功、--app-bg-image 丢失）。
    // 引号形式对 base64 与原样 URL 都安全。
    root.setProperty("--app-bg-image", `url(${JSON.stringify(appearance.bgImage.dataUrl)})`);
    root.setProperty("--app-bg-opacity", String(appearance.bgImage.opacity));
    // 只有真设了壁纸才上可读性遮罩（app.css 的 html[data-wallpaper="1"]）
    html.dataset.wallpaper = "1";
  } else {
    root.removeProperty("--app-bg-image");
    root.removeProperty("--app-bg-opacity");
    delete html.dataset.wallpaper;
  }
}

/** 把界面/弹窗实心度写入 html 内联样式。
 *  内联优先于任何选择器 —— 写进 :root 会被 myui 的 `:root, html.dark`
 *  在深色模式下压过（ompDesktop 升级 myui 时踩过这个坑）。 */
export function applySolidity(): void {
  const html = document.documentElement;
  html.style.setProperty("--ui-solidity", String(clampSolidity(appearance.uiSolidity, 0.25)));
  html.style.setProperty(
    "--ui-popup-solidity",
    String(clampSolidity(appearance.popupSolidity, 0.55)),
  );
}

export function setSolidity(next: { ui?: number; popup?: number }): void {
  if (next.ui !== undefined) {
    appearance.uiSolidity = clampSolidity(next.ui, 0.25);
    writeStored(STORAGE_KEY_SOLIDITY, appearance.uiSolidity);
  }
  if (next.popup !== undefined) {
    appearance.popupSolidity = clampSolidity(next.popup, 0.55);
    writeStored(STORAGE_KEY_POPUP_SOLIDITY, appearance.popupSolidity);
  }
  applySolidity();
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
applySolidity();
