// 探针：挂载真实的 settings/SettingsDrawer，检查右上角关闭钮的 DOM 是否
// 真的渲染出了 myui 的图标字形（此前 MyIconButton 漏 import，Vue 把它当
// 未知原生元素 <myiconbutton> 渲染，class 生效成空盒子、字形却是空的）。
import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "myui/styles";
import "myui/element-theme";
import "./styles/app.css";

import { createApp, defineComponent, h, nextTick, onMounted, ref } from "vue";
import { applyTheme } from "./store/ui";
import SettingsDrawer from "./components/SettingsDrawer.vue";

applyTheme();

const Probe = defineComponent({
  setup() {
    const rows = ref<string[]>([]);

    function log(s: string) {
      rows.value.push(s);
      const el = document.getElementById("out");
      if (el) el.textContent = rows.value.join("\n");
    }

    onMounted(async () => {
      await nextTick();
      await new Promise((res) => setTimeout(res, 600));
      const btn = document.querySelector(".close-btn");
      if (!btn) {
        log("❌ 没找到 .close-btn");
        return;
      }
      log(`tagName        = ${btn.tagName}`);
      log(`className      = ${btn.className}`);
      log(`is myui button = ${btn.classList.contains("my-icon-button")}`);
      log(`aria-label     = ${btn.getAttribute("aria-label")}`);
      const icon = btn.querySelector(".my-icon");
      log(`.my-icon found = ${!!icon}`);
      if (icon) {
        const cs = getComputedStyle(icon);
        const r = icon.getBoundingClientRect();
        log(
          `  my-icon rect   = ${r.width.toFixed(1)} x ${r.height.toFixed(1)} @ ${r.left.toFixed(1)},${r.top.toFixed(1)}`,
        );
        log(
          `  my-icon color  = ${cs.color}  size=${cs.width}  --my-icon-size=${cs.getPropertyValue("--my-icon-size").trim()}`,
        );
        const svg = icon.querySelector("svg");
        log(`  svg found      = ${!!svg}`);
        const svgRect = svg ? svg.getBoundingClientRect() : null;
        if (svg) {
          const scs = getComputedStyle(svg);
          log(`  svg rect       = ${svgRect!.width.toFixed(1)} x ${svgRect!.height.toFixed(1)}`);
          log(`  svg fill       = ${scs.fill}`);
          log(`  svg path count = ${svg.querySelectorAll("path").length}`);
        }
        const btnRect = btn.getBoundingClientRect();
        log(`button rect     = ${btnRect.width.toFixed(1)} x ${btnRect.height.toFixed(1)}`);
        const ok =
          btn.tagName === "BUTTON" &&
          btn.classList.contains("my-icon-button") &&
          !!svg &&
          r.width > 4 &&
          !!svgRect &&
          svgRect.width > 4;
        log(ok ? "\n✅ 关闭钮字形已正常渲染" : "\n❌ 关闭钮字形仍未渲染");
      }
    });

    return () => h("div", null, [h(SettingsDrawer as never, { connectionCount: 0 })]);
  },
});

document.documentElement.classList.add("dark");
createApp(Probe).mount("#app");