// 外观主题（外观设置的命名快照）：把当前所有外观调整固化成一套主题，
// 点击一键整套切换（对齐 ompDesktop 的 appearanceThemes）。
//
// 存在的理由：外观设置分散在 8+ 个控件里（深浅、主题色、两档实心度、字号、
// 动效、背景模式/壁纸/强度、终端配色），用户调出一套满意的组合后没有任何
// 办法留住它——换个场景（白天写代码、晚上读日志）就得全部重拖一遍。
//
// 明确**不在**快照里的：
// - customPalette：是用户资产（自定义终端配色），不是外观值。删一套主题
//   就连带删掉用户资产是不可接受的损失。
// - 终端字体 / 渲染后端：功能配置而非外观调校（GPU 后端甚至要重启生效）。
// - 侧栏视图等布局项：不住在外观分区。
//
// 本模块是纯函数：不 import 任何 store，读写都经由调用方传入的结构化参数。

import type { BackgroundImageConfig } from "@/themes";

/** 一套外观主题的完整快照（字段与 store 中的外观值同名同义）。 */
export interface AppearanceValues {
  isDark: boolean;
  /** 终端配色预设 id（可指向 custom，customPalette 本身不随快照走）。 */
  paletteId: string;
  /** 主题色相 0-360。 */
  accentHue: number;
  /** 界面实心度 0-1。 */
  uiSolidity: number;
  /** 弹窗实心度 0-1。 */
  popupSolidity: number;
  /** 界面正文字号（px，12-18）。 */
  uiFontPx: number;
  motionEnabled: boolean;
  background: BackgroundImageConfig;
}

/** 用户保存的一套外观主题。 */
export interface AppearanceTheme {
  id: string;
  label: string;
  values: AppearanceValues;
}

/** 读取外观值所需的两个 store 的最小结构（真实 store 天然满足）。 */
export interface AppearanceSources {
  appearance: {
    paletteId: string;
    accentHue: number;
    uiSolidity: number;
    popupSolidity: number;
    bgImage: BackgroundImageConfig;
  };
  ui: {
    themeChoice: "dark" | "light";
    uiFontPx: number;
    motionEnabled: boolean;
  };
}

/** 写回外观值所需的 setter 集（真实 store 的函数子集）。 */
export interface AppearanceAppliers {
  setThemeChoice(choice: "dark" | "light"): void;
  setPaletteId(id: string): void;
  setAccentHue(hue: number): void;
  setSolidity(next: { ui?: number; popup?: number }): void;
  setUiFontPx(px: number): void;
  setMotionEnabled(enabled: boolean): void;
  setBgImage(config: BackgroundImageConfig): void;
}

/** 最多保存几套（更多就得滚动，反而难挑）。 */
export const APPEARANCE_THEMES_MAX = 12;
/** 主题名上限。 */
export const APPEARANCE_THEME_NAME_MAX = 40;
/**
 * 主题总预算（字节，UTF-16 长度近似）。
 *
 * 唯一能撑大快照的字段是壁纸 URL：asset 路径只有几十字节，但历史数据里
 * 可能还留着 data URL（迁移失败时不清设置）。localStorage 总额约 5MB，
 * 整套外观主题只该占到一个零头。
 */
export const APPEARANCE_THEMES_MAX_BYTES = 256 * 1024;

/* ---------- 归一化（持久化数据是不可信输入，读写两侧都夹） ---------- */

function clampNumber(value: unknown, fallback: number, lo: number, hi: number): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
  return Math.min(hi, Math.max(lo, value));
}

function normalizeBackground(raw: unknown): BackgroundImageConfig {
  const src = (raw ?? {}) as Partial<BackgroundImageConfig>;
  const mode: BackgroundImageConfig["mode"] =
    src.mode === "grid" || src.mode === "image" || src.mode === "none" || src.mode === "glow"
      ? src.mode
      : "glow";
  return {
    mode,
    image: mode === "image" && typeof src.image === "string" ? src.image : "",
    opacity: clampNumber(src.opacity, 0.22, 0.05, 0.8),
  };
}

/** 归一一条快照：所有字段夹回合法区间，类型不对回落设计默认值
 *  （暗色 / 当前预设不强制 / 青绿 180 / 0 / 0.55 / 14px / 动效开 / glow）。
 *  uiSolidity 默认 0 与 store 一致（阶段 212 把界面实心度默认归零对齐
 *  外部参考实现）；这里若留旧的 0.25，一条缺该字段的旧快照会被恢复成
 *  「比默认更实」，与「恢复默认外观」的语义相悖。 */
export function normalizeAppearanceValues(raw: unknown): AppearanceValues {
  const source = (raw ?? {}) as Partial<AppearanceValues>;
  return {
    isDark: typeof source.isDark === "boolean" ? source.isDark : true,
    paletteId: typeof source.paletteId === "string" ? source.paletteId : "carbon",
    accentHue: clampNumber(source.accentHue, 180, 0, 360),
    uiSolidity: clampNumber(source.uiSolidity, 0, 0, 1),
    popupSolidity: clampNumber(source.popupSolidity, 0.55, 0, 1),
    uiFontPx: clampNumber(source.uiFontPx, 14, 12, 18),
    motionEnabled: typeof source.motionEnabled === "boolean" ? source.motionEnabled : true,
    background: normalizeBackground(source.background),
  };
}

/** 从两个 store 读当前外观值。 */
export function readAppearanceValues(sources: AppearanceSources): AppearanceValues {
  return normalizeAppearanceValues({
    isDark: sources.ui.themeChoice === "dark",
    paletteId: sources.appearance.paletteId,
    accentHue: sources.appearance.accentHue,
    uiSolidity: sources.appearance.uiSolidity,
    popupSolidity: sources.appearance.popupSolidity,
    uiFontPx: sources.ui.uiFontPx,
    motionEnabled: sources.ui.motionEnabled,
    background: sources.appearance.bgImage,
  });
}

/** 写回 store（逐字段 setter，store 内部各自持久化 + 应用到 DOM）。 */
export function applyAppearanceValues(
  appliers: AppearanceAppliers,
  values: AppearanceValues,
): void {
  const next = normalizeAppearanceValues(values);
  appliers.setThemeChoice(next.isDark ? "dark" : "light");
  appliers.setPaletteId(next.paletteId);
  appliers.setAccentHue(next.accentHue);
  appliers.setSolidity({ ui: next.uiSolidity, popup: next.popupSolidity });
  appliers.setUiFontPx(next.uiFontPx);
  appliers.setMotionEnabled(next.motionEnabled);
  appliers.setBgImage(next.background);
}

/**
 * 两条快照是否一致（判断「当前外观是否正贴着某套主题」）。
 *
 * 比的是归一后的值：滑杆步进与浮点存储可能差在末位，直接比原始值
 * 会永远显示「已改动」。
 */
export function appearanceValuesEqual(a: AppearanceValues, b: AppearanceValues): boolean {
  const left = normalizeAppearanceValues(a);
  const right = normalizeAppearanceValues(b);
  return (
    left.isDark === right.isDark &&
    left.paletteId === right.paletteId &&
    left.accentHue === right.accentHue &&
    left.uiSolidity === right.uiSolidity &&
    left.popupSolidity === right.popupSolidity &&
    left.uiFontPx === right.uiFontPx &&
    left.motionEnabled === right.motionEnabled &&
    left.background.mode === right.background.mode &&
    left.background.image === right.background.image &&
    Math.abs(left.background.opacity - right.background.opacity) < 0.001
  );
}

/** 主题列表占用的字节数（UTF-16 近似；只做粗预算）。 */
export function appearanceThemesBytes(themes: readonly AppearanceTheme[]): number {
  let total = 0;
  for (const theme of themes) {
    total += theme.label.length * 2 + theme.values.background.image.length * 2 + 256;
  }
  return total;
}

/** 加上一条后是否仍在预算内。 */
export function appearanceThemesWithinBudget(
  themes: readonly AppearanceTheme[],
  incoming: AppearanceTheme,
): boolean {
  return appearanceThemesBytes([...themes, incoming]) <= APPEARANCE_THEMES_MAX_BYTES;
}

/** 生成主题 id（时间戳 + 随机后缀，同毫秒建两条也不撞）。 */
export function newAppearanceThemeId(): string {
  return `theme-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
}

/** 校验主题名：空名与超长名在写入前拦掉（prompt 可能被直接回车）。 */
export function validAppearanceThemeName(name: string): boolean {
  const trimmed = name.trim();
  return trimmed.length > 0 && trimmed.length <= APPEARANCE_THEME_NAME_MAX;
}

/** 是否与现有主题重名（大小写无关；重名让「更新的是哪一套」不可判定）。 */
export function appearanceThemeNameTaken(
  name: string,
  themes: readonly AppearanceTheme[],
  exceptId?: string,
): boolean {
  const target = name.trim().toLowerCase();
  if (!target) return false;
  return themes.some((theme) => theme.id !== exceptId && theme.label.trim().toLowerCase() === target);
}
