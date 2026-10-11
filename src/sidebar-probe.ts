// 探针：挂载真实的 AppTopBar + AppSidebar（带真实 .app-shell 网格），逐项核对：
//   ① 侧栏头部不再有收起/展开钮，收起/展开只保留顶栏最左一枚 ☰
//   ② ui.sidebarCollapsed = true 时侧栏**完全隐藏**（不再是 44px 窄条）
//   ③ 连接行「⋯」离侧栏右缘还剩多少
//   ④ 版本页脚高度是否与底栏其余两条（CommandBar / ServerInfoPanel，均 36px）对齐
import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "myui/styles";
import "myui/element-theme";
import "./styles/app.css";

import { createApp, defineComponent, h, nextTick, onMounted, ref } from "vue";
import { ElLoading } from "element-plus";
import { setupMyUII18n } from "myui";
import { i18n } from "./i18n";
import { applyTheme, ui } from "./store/ui";
// 连接是 App.vue 在挂载后拉取的，AppSidebar 自己不拉 —— 探针需补这一步
import { reloadConnections } from "./store/connections";
import AppTopBar from "./components/AppTopBar.vue";
import AppSidebar from "./components/AppSidebar.vue";

setupMyUII18n(i18n);
applyTheme();
// 版本号由 App.vue 拉取后写进 store；探针里直接置上，好让版本页脚渲染出来
ui.appVersion = "2.16.4";
ui.sidebarWidth = 264;

const rows = ref<string[]>([]);
function log(s: string) {
  rows.value.push(s);
  const el = document.getElementById("out");
  if (el) el.textContent = rows.value.join("\n");
}
function r(el: Element | null) {
  if (!el) return null;
  const b = el.getBoundingClientRect();
  return {
    left: +b.left.toFixed(1),
    right: +b.right.toFixed(1),
    top: +b.top.toFixed(1),
    bottom: +b.bottom.toFixed(1),
    w: +b.width.toFixed(1),
    h: +b.height.toFixed(1),
  };
}
const wait = (ms: number) => new Promise((res) => setTimeout(res, ms));

const Probe = defineComponent({
  setup() {
    onMounted(async () => {
      await reloadConnections();
      await nextTick();
      await wait(400);

      // ───────────── 展开态 ─────────────
      log("=== ① 收起/展开入口应只剩顶栏一枚 ===");
      const sideBtns = Array.from(document.querySelectorAll(".side-header button")).map(
        (b) => b.getAttribute("title") || b.getAttribute("aria-label") || "?",
      );
      log(`侧栏头部按钮 [${sideBtns.length}]: ${JSON.stringify(sideBtns)}`);
      const sideHasToggle = sideBtns.some((t) => t.includes("侧栏"));
      log(`  侧栏里有收起/展开钮 = ${sideHasToggle ? "❌ 还在" : "✅ 已移除"}`);

      const topBtns = Array.from(document.querySelectorAll(".topbar-left button")).map(
        (b) => `${b.getAttribute("title") || "?"} [${b.getAttribute("aria-expanded")}]`,
      );
      log(`顶栏左侧按钮 [${topBtns.length}]: ${JSON.stringify(topBtns)}`);
      const topToggle = Array.from(document.querySelectorAll(".topbar-left button")).find((b) =>
        /侧栏/.test(b.getAttribute("title") || ""),
      );
      log(`  顶栏有收起/展开钮 = ${topToggle ? "✅ 在" : "❌ 没了"}`);
      log(`  旧的骑跨浮钮 .toggle-btn = ${document.querySelector(".toggle-btn") ? "❌ 残留" : "✅ 无"}`);
      log(`  窄条模板 .side-collapsed = ${document.querySelector(".side-collapsed") ? "❌ 还在" : "✅ 已删"}`);

      log("\n=== ③ 连接行「⋯」离侧栏右缘 ===");
      const side = document.querySelector(".side");
      const more = document.querySelector(".conn-more");
      const sideR = r(side);
      log(`side ${JSON.stringify(sideR)}  .conn-more ${JSON.stringify(r(more))}`);
      if (sideR && more) {
        const mr = r(more)!;
        const gap = sideR.right - mr.right;
        log(`  盒右缘距侧栏右缘 = ${gap.toFixed(1)}px ${gap <= 3 ? "✅ 已贴边" : "❌ 仍有空档"}`);
      }

      log("\n=== ④ 版本页脚高度（底栏顶线对齐）===");
      const footer = document.querySelector(".footer");
      const fr = r(footer);
      log(
        `.footer ${JSON.stringify(fr)}  目标 36px ${fr && Math.abs(fr.h - 36) < 0.5 ? "✅" : "❌"}  border-top=${footer ? getComputedStyle(footer).borderTopWidth : "?"}`,
      );

      // ───────────── 隐藏态 ─────────────
      log("\n=== ② 隐藏态：侧栏应完全消失 ===");
      ui.sidebarCollapsed = true;
      await nextTick();
      await wait(500);
      const shellSide = document.querySelector(".shell-side");
      const shellSideR = r(shellSide);
      const work = document.querySelector(".shell-work");
      const workR = r(work);
      const cs = shellSide ? getComputedStyle(shellSide) : null;
      log(`shell 有 sidebar-collapsed 类 = ${document.querySelector(".app-shell.sidebar-collapsed") ? "✅" : "❌"}`);
      log(`.shell-side rect ${JSON.stringify(shellSideR)}  display=${cs?.display}`);
      log(
        `  侧栏占位宽度 = ${shellSideR ? shellSideR.w.toFixed(1) : "?"}px ${shellSideR && shellSideR.w <= 0.5 ? "✅ 完全收起" : "❌ 仍有占位"}`,
      );
      log(`.shell-work rect ${JSON.stringify(workR)}`);
      if (shellSideR && workR) {
        log(`  工作区左缘 = ${workR.left} ${workR.left <= 0.5 ? "✅ 已顶到窗口左缘" : "❌ 左侧仍有空白"}`);
      }
      const focusables = shellSide
        ? shellSide.querySelectorAll("button, a, input, [tabindex='0']").length
        : -1;
      log(`  侧栏内可聚焦元素数 = ${focusables} ${cs?.display === "none" ? "（display:none，不进 Tab 序）✅" : "❌ 仍可聚焦"}`);

      // 再切回来，确认可恢复
      log("\n=== ③ 再切回显示 ===");
      ui.sidebarCollapsed = false;
      await nextTick();
      await wait(500);
      const sideBack = r(document.querySelector(".side"));
      log(`.side ${JSON.stringify(sideBack)} ${sideBack && sideBack.w > 200 ? "✅ 已恢复" : "❌ 没恢复"}`);
      const nameBack = document.querySelector(".conn-name");
      log(`连接名仍在树上 = ${nameBack ? "✅（状态未丢）" : "❌"}`);
    });

    return () =>
      h("div", {
        class: ["app-shell", { "sidebar-collapsed": ui.sidebarCollapsed, "no-ai": true }],
      }, [
        h(AppTopBar as never, { class: "shell-topbar" }),
        h(AppSidebar as never, { class: "shell-side" }),
        h("main", { class: "shell-work" }, "工作区"),
      ]);
  },
});

document.documentElement.classList.add("dark");
createApp(Probe).use(i18n).use(ElLoading).mount("#app");