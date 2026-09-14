// 终端字体偏好。从旧 useTerminalFont.ts 移植。
// 存储"主字体族"单个名字（空串 = 默认 Nerd Font 优先栈）。
import { ref } from "vue";
import { STORAGE_KEY_TERMINAL_FONT, TERMINAL_FONT_DEFAULT_STACK } from "../themes";

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
