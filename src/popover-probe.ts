// 几何探针：在纯浏览器里复现 CommandBar 的 MyPopover 浮层，量出 popper 与面板
// 的真实 rect，定位「浮层溢出窗口右缘」的成因。与 CommandBar 使用完全相同的
// CSS 层叠（EP base → EP 暗色变量 → myui 令牌 → myui element-theme → app.css）。
import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "myui/styles";
import "myui/element-theme";
import "./styles/app.css";

import { createApp, defineComponent, h, nextTick, onMounted, ref } from "vue";
import { MyButton, MyIcon, MyPopover } from "myui";
import { applyTheme, ui } from "./store/ui";

applyTheme();

// CommandBar 里 .float-panel / .history-panel / .cb-popper 的原样拷贝。
const PROBE_CSS = `
.probe-shell { position: fixed; inset: 0; display: flex; flex-direction: column; }
.probe-work { flex: 1; }
.probe-bar {
  height: 44px; display: flex; align-items: center; gap: 8px;
  padding: 0 12px; border-top: 1px solid var(--border-subtle);
  background: var(--bg-surface); justify-content: flex-end;
}
.float-panel {
  max-height: 60vh;
  background: var(--bg-surface);
  border: 1px solid var(--border-emphasis);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.cb-popper.el-popover {
  padding: 0;
  border: none;
  background: transparent;
  box-shadow: none;
  overflow: visible;
}
.history-panel {
  width: 100%;
  max-height: min(60vh, calc(100vh - 96px));
}
.float-header { padding: 8px 12px; border-bottom: 1px solid var(--border-default); }
.float-list { flex: 1; overflow-y: auto; padding: 4px 0; }
.history-item { display: flex; align-items: center; padding: 6px 12px; gap: 8px; }
.cmd-text { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
`;

const HISTORY = Array.from({ length: 12 }, (_, i) => ({
  id: String(i),
  command: `git commit -m "some fairly long commit message number ${i} to force wrapping"`,
}));

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
    const open = ref(true);
    const rows = ref<string[]>([]);

    function measure(tag: string) {
      const popper = document.querySelector(".el-popper");
      const panel = document.querySelector(".history-panel");
      const trigger = document.querySelector(".probe-trigger");
      const out: string[] = [];
      out.push(`--- ${tag}`);
      out.push(`viewport  w=${window.innerWidth} h=${window.innerHeight}`);
      out.push(`body clientW=${document.body.clientWidth} scrollW=${document.body.scrollWidth}`);
      out.push(`trigger   ${JSON.stringify(r(trigger))}`);
      out.push(`popper    ${JSON.stringify(r(popper))}`);
      out.push(`panel     ${JSON.stringify(r(panel))}`);
      if (popper) {
        const cs = getComputedStyle(popper);
        out.push(
          `popper computed: position=${cs.position} display=${cs.display} width=${cs.width} minWidth=${cs.minWidth} maxWidth=${cs.maxWidth} padding=${cs.padding} overflow=${cs.overflow}`,
        );
        out.push(`popper inline style: ${popper.getAttribute("style")}`);
        out.push(`popper class: ${popper.className}`);
        out.push(`popper parent: <${popper.parentElement?.tagName}> class=${popper.parentElement?.className}`);
        const off = (popper as HTMLElement).offsetParent;
        out.push(`popper offsetParent: ${off ? `<${off.tagName}> class=${off.className}` : "null (=> initial containing block)"}`);
      }
      if (panel) {
        const cs = getComputedStyle(panel);
        out.push(`panel computed: width=${cs.width} maxWidth=${cs.maxWidth} position=${cs.position}`);
      }
      const pRect = r(popper);
      const vw = window.innerWidth;
      if (pRect && pRect.right > vw) {
        out.push(`>>> POPPER OVERFLOWS RIGHT by ${(pRect.right - vw).toFixed(1)}px`);
      }
      const pnRect = r(panel);
      if (pnRect && pnRect.right > vw) {
        out.push(`>>> PANEL OVERFLOWS RIGHT by ${(pnRect.right - vw).toFixed(1)}px`);
      }
      rows.value = [...rows.value, ...out, ""];
      const el = document.getElementById("out");
      if (el) el.textContent = rows.value.join("\n");
    }

    onMounted(async () => {
      await nextTick();
      await new Promise((res) => setTimeout(res, 300));
      measure("after mount (open=true)");
      // 触发一次重新定位（模拟窗口尺寸/内容变化后 EP 的 resize observer 更新）
      window.dispatchEvent(new Event("resize"));
      await new Promise((res) => setTimeout(res, 300));
      measure("after resize");
    });

    return () =>
      h("div", { class: "probe-shell" }, [
        h("main", { class: "probe-work" }, "WORK AREA"),
        h(
          "div",
          { class: "probe-bar" },
          [
            h(
              MyButton,
              { class: "probe-trigger", variant: "secondary", size: "small" },
              { default: () => [h(MyIcon, { size: 14, name: "Clock" }), "历史"] },
            ),
          ],
        ),
        h(
          MyPopover,
          {
            modelValue: open.value,
            "onUpdate:modelValue": (v: boolean) => (open.value = v),
            placement: "top-end",
            trigger: "click",
            popperClass: "cb-popper",
            // 候选修复：把宽度交给 popper 外壳，panel 只占 100%
            popperStyle: { width: "min(480px, calc(100vw - 32px))" },
          },
          {
            default: () => [
              h(
                MyButton,
                { class: "probe-trigger", variant: "secondary", size: "small" },
                { default: () => [h(MyIcon, { size: 14, name: "Clock" }), "历史"] },
              ),
            ],
            content: () => [
              h("div", { class: "float-panel history-panel" }, [
                h("div", { class: "float-header" }, "历史命令"),
                h(
                  "div",
                  { class: "float-list" },
                  HISTORY.map((it) =>
                    h("div", { class: "history-item" }, [
                      h("span", { class: "cmd-text" }, it.command),
                    ]),
                  ),
                ),
              ]),
            ],
          },
        ),
      ]);
  },
});

const style = document.createElement("style");
style.textContent = PROBE_CSS;
document.head.appendChild(style);

document.documentElement.classList.add("dark");
createApp(Probe).mount("#app");