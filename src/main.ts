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
import App from "./App.vue";

setupMyUII18n(i18n);
applyTheme();

// myui 库内部的 v-loading 指令由消费方注册
createApp(App).use(i18n).use(ElLoading).mount("#app");

// 保持 locale 响应（预留多语言切换）
void ui;

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
