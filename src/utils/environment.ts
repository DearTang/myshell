// 运行环境探测：Tauri IPC 桥只在桌面 WebView 里注入。
// 浏览器直开（localhost 预览）时调用 invoke 会抛
// "Cannot read properties of undefined (reading 'invoke')"，用这里的能力做优雅降级。
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 非 Tauri 环境下的统一提示文案。 */
export const BROWSER_PREVIEW_NOTICE =
  "当前是浏览器预览模式，无法访问系统后端。请通过 cargo tauri dev（或桌面应用）启动后再操作。";
