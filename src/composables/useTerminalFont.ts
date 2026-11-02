// 终端字体偏好。从旧 useTerminalFont.ts 移植。
// 存储"主字体族"单个名字（空串 = 默认 Nerd Font 优先栈）+ 字号（px）。
import { ref } from "vue";
import {
  STORAGE_KEY_TERMINAL_FONT,
  STORAGE_KEY_TERMINAL_FONT_SIZE,
  TERMINAL_FONT_DEFAULT_STACK,
  TERMINAL_FONT_SIZE_DEFAULT,
  TERMINAL_FONT_SIZE_MAX,
  TERMINAL_FONT_SIZE_MIN,
} from "../themes";

function readStoredFont(): string {
  try {
    return localStorage.getItem(STORAGE_KEY_TERMINAL_FONT) ?? "";
  } catch {
    return "";
  }
}

/** 去掉会逃逸出引号的字符。 */
function cleanFontName(raw: string): string {
  return raw.replace(/['"]/g, "").trim();
}

/** 组装 xterm.js 的 font-family：主字体在前（Nerd Font 图标优先命中），
 *  后接默认回退链。主字体为空 → 仅默认栈。 */
export function resolveFontStack(primary?: string): string {
  const cleaned = cleanFontName(primary ?? "");
  return cleaned ? `'${cleaned}', ${TERMINAL_FONT_DEFAULT_STACK}` : TERMINAL_FONT_DEFAULT_STACK;
}

export const primaryFont = ref<string>(readStoredFont());

export function setPrimaryFont(font: string): void {
  const cleaned = cleanFontName(font);
  primaryFont.value = cleaned;
  try {
    localStorage.setItem(STORAGE_KEY_TERMINAL_FONT, cleaned);
  } catch {
    /* 仅内存态 */
  }
}

/** 解析后的 CSS font-family（供 xterm.js 与预览用）。 */
export function resolvedFontFamily(): string {
  return resolveFontStack(primaryFont.value);
}

// ── 终端字号（px）──
// 独立于界面字号：界面字号缩放的是壳层（顶栏/侧栏/设置），终端字号是终端
// 缓冲区自己的度量，两者的合理取值区间不同（终端 10-22，界面 12-18）。
// Ctrl+= / Ctrl+- 在终端焦点下改这个，否则改界面字号。
function readStoredFontSize(): number {
  try {
    const n = Number(localStorage.getItem(STORAGE_KEY_TERMINAL_FONT_SIZE));
    if (Number.isFinite(n) && n > 0) {
      return Math.min(TERMINAL_FONT_SIZE_MAX, Math.max(TERMINAL_FONT_SIZE_MIN, Math.round(n)));
    }
  } catch {
    /* 用默认值 */
  }
  return TERMINAL_FONT_SIZE_DEFAULT;
}

export const terminalFontSize = ref<number>(readStoredFontSize());

export function setTerminalFontSize(px: number): void {
  const next = Math.min(TERMINAL_FONT_SIZE_MAX, Math.max(TERMINAL_FONT_SIZE_MIN, Math.round(px)));
  terminalFontSize.value = next;
  try {
    localStorage.setItem(STORAGE_KEY_TERMINAL_FONT_SIZE, String(next));
  } catch {
    /* 仅内存态 */
  }
}
