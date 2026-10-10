<!-- 窗口控制三联钮 ─ □ ✕。
     顶栏与保险库门禁页共用：门禁页是全屏覆盖层（z-index 5000），顶栏尚未渲染，
     若不共用，用户在解锁/设置密码阶段既不能最小化也不能关闭窗口。

     2026-10-09：改用 myui 的 MyWindowControls（v0.29.0 新增）。此前本文件
     自绘三个 <button> + 内联 SVG，是因为 myui 既无 caption 按钮条组件、
     图标注册表也没有 Maximize / Restore / Minus 三枚字形（EP 的 FullScreen
     是四角外扩角括号，语义是"进全屏"，用作最大化按钮属语义错配）。
     现两者都已补齐，图标与 Windows 规范字形由库提供。

     MyWindowControls 只 emit 事件、不碰窗口系统——本文件仍自己接
     getCurrentWindow()，myui 保持零桌面运行时依赖。 -->
<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { MyWindowControls } from "myui";
import type { UnlistenFn } from "@tauri-apps/api/event";

defineOptions({ name: "WindowControls" });

/** 门禁页背景是深色渐变，按钮默认色需比顶栏更亮一档才看得清。 */
withDefaults(defineProps<{ tone?: "topbar" | "overlay" }>(), { tone: "topbar" });

const win = getCurrentWindow();
const isMaximized = ref(false);
let unlistenResized: UnlistenFn | null = null;

function minimizeWin(): void {
  void win.minimize().catch(() => {
    /* best-effort */
  });
}

function toggleMaximize(): void {
  void win.toggleMaximize().catch(() => {
    /* best-effort */
  });
}

function closeWin(): void {
  void win.close().catch(() => {
    /* best-effort */
  });
}

onMounted(() => {
  void win
    .isMaximized()
    .then((m) => {
      isMaximized.value = m;
    })
    .catch(() => {
      /* 权限缺失时按钮退化为普通图标 */
    });
  void win
    .onResized(async () => {
      isMaximized.value = await win.isMaximized().catch(() => isMaximized.value);
    })
    .then((fn) => {
      unlistenResized = fn;
    })
    .catch(() => {
      /* best-effort */
    });
});

onUnmounted(() => {
  unlistenResized?.();
  unlistenResized = null;
});
</script>

<template>
  <MyWindowControls
    class="window-controls"
    :class="`is-${tone}`"
    :maximized="isMaximized"
    @minimize="minimizeWin"
    @toggle-maximize="toggleMaximize"
    @close="closeWin"
  />
</template>

<style scoped>
/* ── MyWindowControls 之上叠两处 MyShell 的刻意偏离 ──

   1) 按钮全高：库默认 46×32（贴合常见标题栏），本项目顶栏 44px、
      门禁页覆盖层 44px，按钮需 `height: 100%` 才与栏等高。
   2) 关闭钮悬停用系统红 `#e81123` 而非库默认的 `var(--danger)`——
      用户对 ─ □ ✕ 的红有肌肉记忆，主题色会认不出。这是早于 myui
      接入就存在的刻意选择，保留。

   两者都要下钻到 `.my-window-controls__btn`：MyWindowControls 是
   `inheritAttrs:false` + 根节点 mergeProps，所以根上的 .window-controls
   直接生效，子按钮是库内部结构，只能 :deep()。若 myui 日后改按钮类名，
   这两条会静默失效。 */
.window-controls {
  height: 100%;
}

.window-controls :deep(.my-window-controls__btn) {
  height: 100%;
  border-radius: 0;
  font-family: inherit;
  user-select: none;
  -webkit-user-select: none;
}

.window-controls :deep(.my-window-controls__btn.is-close:hover) {
  background: #e81123;
  color: #fff;
}

.window-controls :deep(.my-window-controls__btn.is-close:active) {
  background: #f1707a;
  color: #fff;
}

/* 浮层（门禁页）形态：按钮贴在覆盖层右上角，底色是深色渐变而非顶栏毛玻璃，
   因此默认色提亮一档，并给一层极淡的底以便与渐变背景分离。 */
.window-controls.is-overlay {
  position: absolute;
  top: 0;
  right: 0;
  z-index: 1;
}

.window-controls.is-overlay :deep(.my-window-controls__btn) {
  color: var(--text-tertiary);
}

.window-controls.is-overlay :deep(.my-window-controls__btn:hover) {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-primary);
}
</style>
