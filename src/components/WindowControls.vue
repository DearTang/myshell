<!-- 窗口控制按钮（无边框窗口自绘 ─ □ ✕）。
     顶栏与保险库门禁页共用：门禁页是全屏覆盖层（z-index 5000），顶栏尚未渲染，
     若不共用，用户在解锁/设置密码阶段既不能最小化也不能关闭窗口。
     Windows 规范：46px 宽、全高、无圆角、悬停高亮；关闭钮悬停用系统关闭红
     （非主题令牌——用户对 ─ □ ✕ 的红有肌肉记忆，主题色会认不出）。 -->
<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
  <div class="window-controls" :class="`is-${tone}`">
    <button type="button" class="wc-btn" title="最小化" aria-label="最小化" @click="minimizeWin">
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0 5h10" stroke="currentColor" stroke-width="1" />
      </svg>
    </button>
    <button
      type="button"
      class="wc-btn"
      :title="isMaximized ? '向下还原' : '最大化'"
      :aria-label="isMaximized ? '向下还原' : '最大化'"
      @click="toggleMaximize"
    >
      <!-- 最大化：空心方框；还原：双层方框（Windows 规范字形） -->
      <svg v-if="!isMaximized" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1" />
      </svg>
      <svg v-else width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <rect x="0.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1" />
        <path d="M2.5 2.5V0.5h7v7h-2" fill="none" stroke="currentColor" stroke-width="1" />
      </svg>
    </button>
    <button type="button" class="wc-btn wc-close" title="关闭" aria-label="关闭" @click="closeWin">
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0 0l10 10M10 0L0 10" stroke="currentColor" stroke-width="1" />
      </svg>
    </button>
  </div>
</template>

<style scoped>
.window-controls {
  display: flex;
  align-items: stretch;
  height: 100%;
  flex-shrink: 0;
}

.wc-btn {
  width: 46px;
  height: 100%;
  display: grid;
  place-items: center;
  padding: 0;
  border: none;
  border-radius: 0;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  user-select: none;
  -webkit-user-select: none;
  font-family: inherit;
  transition:
    background var(--duration-fast) var(--ease-in-out),
    color var(--duration-fast) var(--ease-in-out);
}

.wc-btn:hover {
  background: var(--bg-surface-hover);
  color: var(--text-primary);
}

.wc-btn:active {
  background: var(--bg-surface-active);
}

.wc-close:hover {
  background: #e81123;
  color: #fff;
}

.wc-close:active {
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
  height: 44px;
}

.window-controls.is-overlay .wc-btn {
  color: var(--text-tertiary);
}

.window-controls.is-overlay .wc-btn:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-primary);
}

.window-controls.is-overlay .wc-close:hover {
  background: #e81123;
  color: #fff;
}
</style>
