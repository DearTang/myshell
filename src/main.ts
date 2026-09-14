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
