// 外观域：终端配色预设 + 自定义预设 + 主题色相 + 背景图 + 实心度。
// 逻辑从旧 useColorScheme.tsx 移植为响应式单例，localStorage key 原样保留。
//
// 2026-11 重构（对齐 ompDesktop 模型）：UI 颜色不再随终端配色预设走 ——
// 预设只管 xterm 的 16 色；界面强调色族由 accentHue（--accent-hue）派生，
// 中性底在 app.css 的暗/亮主题块各定义一版。
import { reactive, watch } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import {
  type BackgroundImageConfig,
  type ColorPalette,
  resolvePalette,
  DEFAULT_PALETTE_ID,
  DEFAULT_BG_IMAGE,
  DEFAULT_SCREENSHOT_STYLE_ID,
  getScreenshotStyle,
  STORAGE_KEY_COLOR,
  STORAGE_KEY_CUSTOM,
  STORAGE_KEY_BG,
  STORAGE_KEY_SCREENSHOT_STYLE,
  STORAGE_KEY_SOLIDITY,
  STORAGE_KEY_POPUP_SOLIDITY,
} from "../themes";
import {
  APPEARANCE_THEMES_MAX,
  normalizeAppearanceValues,
  type AppearanceTheme,
} from "../utils/appearanceThemes";
import { currentWallpaper } from "../api";
import { ui } from "./ui";

/** 主题色相的持久化键（0-360 整数）。 */
const STORAGE_KEY_ACCENT_HUE = "myshell-accent-hue";
/** 默认主题色相：青绿（ompDesktop 同款）。 */
const DEFAULT_ACCENT_HUE = 180;

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
  /** 主题色相 0-360（默认青绿 180）。强调色族 / 光晕 / 玻璃色度由它派生。 */
  accentHue: number;
  /** 截图样式 id（默认 putty）。只管 📷 截图产出的 PNG，不影响真实终端。 */
  screenshotStyleId: string;
}

function migrateStoredPalette(stored: string): string {
  // 旧默认值迁移到 Carbon
  return stored === "catppuccin-mocha" ? DEFAULT_PALETTE_ID : stored;
}

/** 实心度收敛：localStorage 里的旧值/外部改动都可能带出界，读写两侧都过一遍。
 *  fallback 必填 —— 界面与弹窗的默认值不同（0 / 0.55），不能共用一个兜底。
 *  界面默认 0（2026-11-10 对齐 ompDesktop：透出玻璃 0.3+0.7×solidity 在
 *  0 时得 0.3 基线；此前 0.25 让侧栏/顶栏比 omp 厚 0.175）。 */
function clampSolidity(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(1, Math.max(0, value))
    : fallback;
}

/** 色相收敛：持久化数据不可信（外部改写 / 旧版本），读写两侧都夹回 0-360。 */
function clampHue(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return DEFAULT_ACCENT_HUE;
  return Math.min(360, Math.max(0, Math.round(value)));
}
/** 背景配置归一化：旧结构 `{ dataUrl, opacity }` → 四模式结构。
 *  localStorage 是不可信输入：mode 非法回落 glow、opacity 夹回 0.05-0.8、
 *  非 image 模式下不留图（切回来不会突然多出一张没人选过的壁纸）。 */
function normalizeBgConfig(raw: unknown): BackgroundImageConfig {
  const src = (raw ?? {}) as Partial<BackgroundImageConfig> & { dataUrl?: string | null };
  const mode: BackgroundImageConfig["mode"] =
    src.mode === "grid" || src.mode === "image" || src.mode === "none" || src.mode === "glow"
      ? src.mode
      : // 旧结构：有 dataUrl 视为 image 模式，否则回落默认 glow
        typeof src.dataUrl === "string" && src.dataUrl
        ? "image"
        : "glow";
  const image =
    typeof src.image === "string"
      ? src.image
      : typeof src.dataUrl === "string"
        ? src.dataUrl
        : "";
  const opacityRaw = typeof src.opacity === "number" ? src.opacity : DEFAULT_BG_IMAGE.opacity;
  // 旧版壁纸不透明度语义（0.85=很清楚）与新强度语义（0.22=淡）不同：
  // 旧值 >0.8 说明是旧语义，直接当 0.8（最清晰档）用。
  const opacity = Math.min(0.8, Math.max(0.05, opacityRaw > 0.8 ? 0.8 : opacityRaw));
  return { mode, image: mode === "image" ? image : "", opacity };
}

export const appearance = reactive<AppearanceState>({
  paletteId: migrateStoredPalette(readStored<string>(STORAGE_KEY_COLOR, DEFAULT_PALETTE_ID)),
  customPalette: readStored<ColorPalette | null>(STORAGE_KEY_CUSTOM, null),
  bgImage: normalizeBgConfig(readStored<unknown>(STORAGE_KEY_BG, DEFAULT_BG_IMAGE)),
  uiSolidity: clampSolidity(readStored<number>(STORAGE_KEY_SOLIDITY, 0), 0),
  popupSolidity: clampSolidity(readStored<number>(STORAGE_KEY_POPUP_SOLIDITY, 0.55), 0.55),
  accentHue: clampHue(readStored<number>(STORAGE_KEY_ACCENT_HUE, DEFAULT_ACCENT_HUE)),
  // 截图样式：非法 id 回落默认（localStorage 是不可信输入，同色相/实心度两侧夹紧）
  screenshotStyleId: getScreenshotStyle(
    readStored<string>(STORAGE_KEY_SCREENSHOT_STYLE, DEFAULT_SCREENSHOT_STYLE_ID),
  ).id,
});

/** 旧版预设把 UI 变量写成 html 内联覆盖；升级到 hue 派生模型后，启动时
 *  要把上一版本留在文档根上的那批内联变量清掉，否则它们会压过主题块的
 *  oklch 声明（内联样式优先级最高），表现为「升级后界面颜色还是旧预设」。
 *  只在启动时跑一次，没有增量状态需要维护。 */
const LEGACY_UI_VAR_KEYS = [
  "--bg-base",
  "--bg-elevated",
  "--bg-surface",
  "--bg-surface-hover",
  "--bg-surface-active",
  "--bg-input",
  "--bg-input-hover",
  "--text-primary",
  "--text-secondary",
  "--text-tertiary",
  "--text-muted",
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
  "--border-default",
  "--border-subtle",
  "--border-emphasis",
  "--border-accent",
  "--shadow-glow",
  "--glass-border",
  "--glass-blur",
];

/** 清理旧版预设留下的 html 内联 UI 变量（幂等；属性不存在时 remove 是 no-op）。 */
function clearLegacyPresetOverrides(): void {
  const root = document.documentElement;
  for (const key of LEGACY_UI_VAR_KEYS) root.style.removeProperty(key);
}

/** 终端配色预设的「应用」已无 DOM 副作用（预设只管 xterm 16 色，由
 *  TerminalPanel 在建终端时从 getActivePalette() 读取）。保留此函数供
 *  深浅切换 watcher 调用，承担一次性的旧覆盖清理。 */
export function applyColorPreset(): void {
  clearLegacyPresetOverrides();
}

/** 把主题色相写入 html 内联样式（--accent-hue）。
 *  内联优先级高于任何选择器，压过 app.css 的 :root 默认值（180）。 */
export function applyAccentHue(): void {
  document.documentElement.style.setProperty("--accent-hue", String(clampHue(appearance.accentHue)));
}

export function setAccentHue(hue: number): void {
  appearance.accentHue = clampHue(hue);
  writeStored(STORAGE_KEY_ACCENT_HUE, appearance.accentHue);
  applyAccentHue();
}

/** 背景 URL 消毒（对齐 ompDesktop）：只放行 data:image / asset 协议 /
 *  http(s)。localStorage 是任何脚本都能碰的，`url(javascript:...)` 之类
 *  照单全收等于给 CSS 塞不可信输入。 */
function sanitizeBackgroundImage(value: string): string {
  if (!value) return "";
  if (/^data:image\/(?:png|jpeg|webp|gif|bmp|avif);/i.test(value)) return value;
  if (/^(?:http:\/\/asset\.localhost|asset:\/\/|https:\/\/asset\.localhost)/i.test(value)) {
    return value;
  }
  try {
    const url = new URL(value, window.location.href);
    return ["http:", "https:"].includes(url.protocol) ? url.href : "";
  } catch {
    return "";
  }
}

/** 壁纸文件 → 可直接放进 CSS 的 asset URL。
 *
 *  文件名在 Rust 侧固定为 `wallpaper.<ext>`（换图是覆盖写），所以 URL 必须
 *  带 `?v=` 版本戳：同一个 URL 浏览器不会重新取图（Chromium 实测），不带戳
 *  换图后页面继续显示旧图。查询串不参与 asset 协议 scope 的路径判定。 */
export function wallpaperUrl(path: string): string {
  return `${convertFileSrc(path)}?v=${Date.now()}`;
}

/** asset 协议的壁纸文件可能已被替换/删除（实测：导入新壁纸后旧文件删掉，
 *  localStorage 里的旧文件名仍在，每次启动 WebView 请求一次 404，
 *  Tauri 侧报 `File does not exist at path`）。这里用 fetch 预检：
 *  失败就不写 --app-bg-image，退回无壁纸态。
 *  shortcut: 只处理 asset 协议；http(s) 壁纸不做预检（网络图常有 404 反跳），
 *  需要时再补。 */
async function verifyWallpaperReachable(url: string): Promise<boolean> {
  if (!/^(?:asset:\/\/|https?:\/\/asset\.localhost)/i.test(url)) return true;
  try {
    const res = await fetch(url, { method: "HEAD" });
    return res.ok;
  } catch {
    return false;
  }
}

/** 把背景配置写入根节点（body::before 背景层按 data-background-mode 绘制）。
 *
 *  四模式：glow（主题色光晕）/ grid（网格纹理）/ image（壁纸）/ none。
 *  image 模式：壁纸 opacity 固定 1（可见度交给壳层实心度，避免两套透明度
 *  互相竞争），强度滑杆转译为「清晰度」—— 越低越模糊（(0.8-清晰度)×4px），
 *  弱壁纸让位于内容、强壁纸保持锐利。只有 image 且有合法 URL 才进入
 *  透出模式（data-translucent：壳层玻璃 alpha 基线下调，壁纸透过整窗）。 */
export async function applyBgImage(): Promise<void> {
  const root = document.documentElement.style;
  const html = document.documentElement;
  const { mode, image, opacity } = appearance.bgImage;
  const safeUrl = mode === "image" ? sanitizeBackgroundImage(image) : "";

  html.dataset.backgroundMode = mode;
  const clarity = Math.min(0.8, Math.max(0.05, opacity));
  // 可见度归一化（0-1）：清晰度滑杆联动雾层稀释与透出玻璃基线
  // （app.css 消费），让「拉满清晰度」真的等于「背景更明显」，而不是
  // 只把 blur 从 3px 收到 0px —— 后者在玻璃+雾层压制下体感无变化。
  const visibility = (clarity - 0.05) / 0.75;
  root.setProperty("--app-bg-opacity", String(clarity));
  root.setProperty("--bg-visibility", visibility.toFixed(3));
  root.setProperty("--wallpaper-blur", `${((0.8 - clarity) * 4).toFixed(2)}px`);
  // 文件已被删/替换时（localStorage 里的旧文件名）不发请求：
  // 否则每次启动 WebView 请求一次 404，Tauri 侧刷一条 ERROR。
  let usableUrl = safeUrl && (await verifyWallpaperReachable(safeUrl)) ? safeUrl : "";
  // 自愈：localStorage 里的 URL 指向已不存在的文件时，从后端目录取当前壁纸。
  // 两种成因都实测过：旧版时间戳命名被清理；进程退出时 Chromium 没把最后
  // 一次写入刷进 LevelDB，重启读回旧值。image 为空 = 用户主动清除壁纸，
  // 此时不恢复（否则清除操作会在下次启动自己撤销）。
  if (!usableUrl && safeUrl) {
    try {
      const stored = await currentWallpaper();
      if (stored) {
        const fresh = wallpaperUrl(stored);
        if (await verifyWallpaperReachable(fresh)) {
          usableUrl = fresh;
          appearance.bgImage.image = fresh;
          writeStored(STORAGE_KEY_BG, appearance.bgImage);
        }
      }
    } catch {
      /* 自愈失败按无壁纸处理，不影响其它外观设置 */
    }
  }
  if (usableUrl) {
    // JSON.stringify 包引号：URL 里的 `#`/`)`/空白会让裸 url(...) 非法，
    // setProperty 静默失败（实测 --app-bg-image 丢失而 opacity 成功）。
    root.setProperty("--app-bg-image", `url(${JSON.stringify(usableUrl)})`);
    html.dataset.wallpaper = "1";
    html.dataset.translucent = "1";
  } else {
    root.removeProperty("--app-bg-image");
    delete html.dataset.wallpaper;
    delete html.dataset.translucent;
  }
}

/** 把界面/弹窗实心度写入 html 内联样式。
 *  内联优先于任何选择器 —— 写进 :root 会被 myui 的 `:root, html.dark`
 *  在深色模式下压过（ompDesktop 升级 myui 时踩过这个坑）。 */
export function applySolidity(): void {
  const html = document.documentElement;
  html.style.setProperty("--ui-solidity", String(clampSolidity(appearance.uiSolidity, 0)));
  html.style.setProperty(
    "--ui-popup-solidity",
    String(clampSolidity(appearance.popupSolidity, 0.55)),
  );
}

export function setSolidity(next: { ui?: number; popup?: number }): void {
  if (next.ui !== undefined) {
    appearance.uiSolidity = clampSolidity(next.ui, 0);
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
  void applyBgImage();
}

export function getActivePalette(): ColorPalette {
  return resolvePalette(appearance.paletteId, appearance.customPalette);
}

export function setScreenshotStyleId(id: string): void {
  // 归一化在 getScreenshotStyle 里（未知 id → putty），写进去的永远是合法值
  appearance.screenshotStyleId = getScreenshotStyle(id).id;
  writeStored(STORAGE_KEY_SCREENSHOT_STYLE, appearance.screenshotStyleId);
}

/** 截图实际使用的配色：PuTTY 样式直接返回；「跟随终端」按当前深浅主题取
 *  当前终端预设的对应变体（自定义预设同样覆盖，因为它就是当前预设）。
 *
 * `fontFamily` 为空串表示「不带自己的字体」：captureTerminalToDataUrl 里
 * `shotStyle.fontFamily || fontFamily` 会回落到终端实测的字体栈。返回非空值
 * （哪怕是 Courier New）会让 `||` 短路，把「跟随终端」的截图字体写死成
 * Courier New —— 而这个样式的语义就是只换颜色、字体沿用终端。 */
export function resolveScreenshotStyle(): {
  terminal: ColorPalette["dark"]["terminal"];
  fontFamily: string;
} {
  const style = getScreenshotStyle(appearance.screenshotStyleId);
  if (style.id !== "live") return { terminal: style.terminal, fontFamily: style.fontFamily };
  const active = getActivePalette();
  return {
    terminal: (ui.isDark ? active.dark : active.light).terminal,
    fontFamily: "",
  };
}

// 深浅主题切换时清一次旧覆盖（终端配色由 TerminalPanel 自取，无需重应用）
watch(
  () => ui.isDark,
  () => applyColorPreset(),
);

// 初始化（main.ts import 本模块即生效；applyTheme 后 watcher 会再跑一次）
applyColorPreset();
void applyBgImage();
applySolidity();
applyAccentHue();

/**
 * 旧版壁纸（base64 data URL 塞 localStorage）一次性迁入自管目录。
 *
 * 旧存法的问题不是能不能显示（能），而是 base64 动辄数 MB，撑爆
 * localStorage 配额后 setItem 静默失败，之后的任何外观改动都存不下来。
 * 迁移后 localStorage 只存 asset 路径（几十字节）。
 *
 * 失败（解码非法 / 体积超限 / 写盘失败）不清用户设置，下次启动再试 ——
 * 清了反而连「换一张」的入口都找不到。
 */
export async function migrateLegacyWallpaper(): Promise<void> {
  const { mode, image } = appearance.bgImage;
  if (mode !== "image" || !image.startsWith("data:")) return;
  try {
    const stored = await invoke<string>("import_wallpaper_data", { dataUrl: image });
    setBgImage({
      mode: "image",
      image: wallpaperUrl(stored),
      opacity: appearance.bgImage.opacity,
    });
  } catch (e) {
    console.warn("[appearance] 旧壁纸迁移失败（下次启动重试）:", e);
  }
}

// ── 外观主题快照（命名的一键切换；纯函数逻辑在 utils/appearanceThemes）──

const STORAGE_KEY_APPEARANCE_THEMES = "myshell-appearance-themes";

/** 归一化持久化列表：单条坏数据（外部改写 / 旧版本）剔掉而不是整列表报废。 */
function normalizeStoredThemes(raw: unknown): AppearanceTheme[] {
  if (!Array.isArray(raw)) return [];
  const out: AppearanceTheme[] = [];
  for (const item of raw) {
    const t = item as Partial<AppearanceTheme>;
    if (typeof t.id !== "string" || typeof t.label !== "string" || typeof t.values !== "object") {
      continue;
    }
    out.push({ id: t.id, label: t.label, values: normalizeAppearanceValues(t.values) });
  }
  return out.slice(0, APPEARANCE_THEMES_MAX);
}

export function getAppearanceThemes(): AppearanceTheme[] {
  return normalizeStoredThemes(readStored<unknown>(STORAGE_KEY_APPEARANCE_THEMES, []));
}

export function setAppearanceThemes(themes: AppearanceTheme[]): void {
  writeStored(STORAGE_KEY_APPEARANCE_THEMES, themes.slice(0, APPEARANCE_THEMES_MAX));
}
