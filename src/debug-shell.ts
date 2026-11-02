// 调试入口：复现 App.vue 五区壳层的顶栏 + 侧栏（折叠态）布局，
// 用于在纯浏览器里检查元素几何/遮挡（配套 /debug-shell.html，IPC 已 stub）。
import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "myui/styles";
import "myui/element-theme";
import "./styles/app.css";

import { createApp, h } from "vue";
import { setupMyUII18n } from "myui";
import { i18n } from "./i18n";
import { applyTheme, ui } from "./store/ui";
import "@/store/appearance";
import AppTopBar from "./components/AppTopBar.vue";
import AppSidebar from "./components/AppSidebar.vue";

// myui 的 MyWindowControls 等组件内部调 useI18n()；只 setup 不 install 会在
// setup 阶段抛 "Need to install with `app.use` function"，整个顶栏渲染失败。
// 正式入口 main.ts 有这两行，调试入口此前漏了。
setupMyUII18n(i18n);

// 强制折叠态：问题场景（圆形展开按钮骑跨窄条右缘）
ui.sidebarCollapsed = true;
applyTheme();

createApp({
  setup() {
    return () =>
      h("div", { class: "app-shell sidebar-collapsed no-ai" }, [
        h(AppTopBar, { class: "shell-topbar" }),
        h(AppSidebar, { class: "shell-side" }),
        h("main", { class: "shell-work" }, [
          h(
            "div",
            {
              style:
                "position:absolute;inset:0;display:flex;align-items:center;justify-content:center;color:var(--text-muted);font-size:var(--fs-sm);",
            },
            "WORK AREA（终端占位）",
          ),
        ]),
      ]);
  },
})
  .use(i18n)
  .mount("#app");
