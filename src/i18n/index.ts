// vue-i18n 实例：MyShell 界面文案沿用旧版硬编码中文，这里只为承载
// myui 库组件的词条注入（setupMyUII18n）。en 语言包暂缺，fallback zh-CN。
import { createI18n } from "vue-i18n";

export const i18n = createI18n({
  legacy: false,
  locale: "zh-CN",
  fallbackLocale: "zh-CN",
});
