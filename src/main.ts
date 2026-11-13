// 应用入口：样式分层与 Provider 装配，参照 unified-ui-vue 模板。
// 样式顺序：EP 基础 → EP 暗色变量 → MyUI 令牌/组件观感 → EP 变量桥 → MyShell 壳层。
import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "myui/styles";
import "myui/element-theme";
import "./styles/app.css";

import { createApp } from "vue";
import { ElLoading } from "element-plus";
import { setupMyUII18n } from "myui";
import { i18n } from "./i18n";
import { applyTheme, ui } from "./store/ui";
// 引入即生效：注册配色预设/背景图的初始应用与 isDark watcher
import "@/store/appearance";
import { migrateLegacyWallpaper } from "@/store/appearance";
import { writeFrontendLog } from "@/api";
import App from "./App.vue";

setupMyUII18n(i18n);
applyTheme();
// 旧版壁纸（base64 data URL）一次性迁入自管目录；失败下次启动再试
void migrateLegacyWallpaper();

// myui 库内部的 v-loading 指令由消费方注册
createApp(App).use(i18n).use(ElLoading).mount("#app");

// 保持 locale 响应（预留多语言切换）
void ui;

// ── 全局前端错误捕获（勿删） ─────────────────────────────────────────────────
// WebView2 的 console 不落盘，用户也不会开 devtools。把未捕获错误与未处理
// rejection 转给后端 write_frontend_log，与 Rust 日志同文件同时序，前端渲染
// 崩溃才能和后端 SSH/PTY 事件对上时间线。
//
// 这块在 React→Vue 迁移（2c2ce3d）时整块丢失，直到复查 8-07 的日志才发现：
// 那次 xterm 的 syncScrollArea race 全靠它兜住。writeFrontendLog 自身
// fire-and-forget 且吞错，日志失败绝不会变成新的未捕获错误。
if (typeof window !== "undefined") {
  // 未捕获的同步错误 + 资源加载错误。
  window.addEventListener("error", (event) => {
    const msg =
      event.error instanceof Error
        ? `${event.error.message}${event.error.stack ? ` | ${event.error.stack.replace(/\s+/g, " ").trim()}` : ""}`
        : String(event.message || "unknown error");
    writeFrontendLog("error", `uncaught: ${msg} @ ${event.filename}:${event.lineno}`);

    // 抑制已知的 xterm.js race：Viewport.syncScrollArea 读 renderer.dimensions
    // 时 Canvas/WebGL 渲染器还没初始化完（SSH 数据在 term.open() 后约 100ms
    // 内到达就会撞上）。不 preventDefault 的话 WebView2 会把它当致命错误，
    // 直接崩掉渲染进程 —— 整个 UI 冻结，只能杀进程。
    // 2026-08-07 实测日志：单日 9 次，全部发生在新连接建立后的首帧窗口。
    if (msg.includes("reading 'dimensions'") && event.filename.includes("xterm")) {
      event.preventDefault();
    }
  });

  // 未处理的 promise rejection（async/await 失败的主路径）。
  window.addEventListener("unhandledrejection", (event) => {
    const reason = event.reason;
    const text =
      reason instanceof Error
        ? `${reason.message}${reason.stack ? ` | ${reason.stack.replace(/\s+/g, " ").trim()}` : ""}`
        : typeof reason === "string"
          ? reason
          : safeStringify(reason);
    writeFrontendLog("error", `unhandledrejection: ${text}`);
  });
}

/** JSON.stringify 不会因循环引用 / 不可序列化值抛错。 */
function safeStringify(v: unknown): string {
  try {
    return JSON.stringify(v) ?? String(v);
  } catch {
    return String(v);
  }
}

// ── 首帧就绪信号（窗口显隐握手） ──────────────────────────────────────────────
// 主窗口以 visible:false 启动（避免白屏闪烁），后端在收到本信号后才显示窗口；
// 若信号丢失，后端的 4s 兜底会兜住（但用户会白等 4s）。Vue 迁移时本信号曾遗漏，
// 导致每次启动都走兜底路径 —— 勿删。
// 等两帧 rAF 再发，确保浏览器真的完成了一次绘制（不只是排好队）。
// try/catch + 动态 import：纯浏览器（npm run dev 直接开页面）下必须静默跳过。
(() => {
  try {
    import("@tauri-apps/api/event")
      .then(({ emit }) => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            void emit("dom-ready").catch(() => {
              /* 后端有 4s 兜底，此处失败可忽略 */
            });
          });
        });
      })
      .catch(() => {
        /* 非 Tauri 环境（纯浏览器）。空操作。 */
      });
  } catch {
    /* 空操作。 */
  }
})();
