// 探针：直接挂载真实的 settings/AppearanceSection，量「配色卡片」的排版几何，
// 验证「色板在上、名称在下、色板不顶出卡片上沿」这条修复。
// 与主应用使用完全相同的 CSS 层叠顺序。
import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "myui/styles";
import "myui/element-theme";
import "./styles/app.css";

import { createApp, defineComponent, h, nextTick, onMounted, ref } from "vue";
import { applyTheme } from "./store/ui";
import AppearanceSection from "./components/settings/AppearanceSection.vue";

applyTheme();

function r(el: Element | null) {
  if (!el) return null;
  const b = el.getBoundingClientRect();
  return {
    left: +b.left.toFixed(1),
    right: +b.right.toFixed(1),
    top: +b.top.toFixed(1),
    bottom: +b.bottom.toFixed(1),
    width: +b.width.toFixed(1),
    height: +b.height.toFixed(1),
  };
}

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
      await new Promise((res) => setTimeout(res, 400));
      const cards = Array.from(document.querySelectorAll(".preset-card"));
      log(`preset-card count = ${cards.length}`);
      cards.slice(0, 3).forEach((card, i) => {
        const dot = card.querySelector(".preset-dot");
        const name = card.querySelector(".preset-name");
        const body = card.querySelector(".preset-body");
        const cr = r(card);
        const dr = r(dot);
        const nr = r(name);
        log(`\n[card ${i}]`);
        log(`  card  ${JSON.stringify(cr)}`);
        log(`  body  ${JSON.stringify(r(body))}`);
        log(`  dot   ${JSON.stringify(dr)}`);
        log(`  name  ${JSON.stringify(nr)}`);
        if (cr && dr && nr) {
          const overflowTop = cr.top - dr.top;
          const overflowBottom = dr.bottom - cr.bottom;
          const stacked = dr.bottom <= nr.top + 1;
          log(`  -> 色板顶出卡片上沿 ${overflowTop.toFixed(1)}px  ${overflowTop > 0 ? "❌ FAIL" : "✅ OK"}`);
          log(`  -> 色板越出卡片下沿 ${overflowBottom.toFixed(1)}px  ${overflowBottom > 0 ? "❌ FAIL" : "✅ OK"}`);
          log(`  -> 色板在名称上方   ${stacked ? "✅ OK" : "❌ FAIL (仍然并排)"}`);
        }
        if (body) {
          const cs = getComputedStyle(body);
          log(`  body computed: display=${cs.display} flexDirection=${cs.flexDirection} alignItems=${cs.alignItems}`);
        }
      });
    });

    return () =>
      h("div", { style: "padding:16px;min-height:100%" }, [
        h(AppearanceSection as never, {}),
      ]);
  },
});

document.documentElement.classList.add("dark");
createApp(Probe).mount("#app");