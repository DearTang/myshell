<!-- 多窗口网格：把选中的会话平铺成网格，每格内嵌一个 TerminalPanel。
     从旧 App.tsx 的 MultiWindowGrid 函数组件原样移植（自包含，无 props）：
     工具条（标题 + 会话数 + 溢出提示 / 编辑会话 / 退出多窗口）+ 网格布局算法照旧
     （最少空格列数搜索）+ 每格标题条（移除按钮语义照旧）。
     TerminalPanel 的接线契约见 src/App.vue 中的绑定。 -->
<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Grid } from "@element-plus/icons-vue";
import { MyButton } from "myui";
import TerminalPanel from "@/components/TerminalPanel.vue";
import type { Tab } from "@/api";
import {
  sessions,
  getBroadcastTargets,
  registerTerminal,
  unregisterTerminal,
  reconnect,
  clearReconnectSnapshot,
  exitMultiWindow,
} from "@/store/sessions";
import { connectionsStore } from "@/store/connections";
import { ui } from "@/store/ui";
import { rendererBackend } from "@/composables/useRendererPref";

defineOptions({ name: "MultiWindowGrid" });

/** Read multi-window grid rows/cols from localStorage (defaults: 2 rows x 3 cols). */
function getMultiWindowGridConfig(): { rows: number; cols: number } {
  const r = parseInt(localStorage.getItem("myshell-multiwindow-rows") ?? "2", 10);
  const col = parseInt(localStorage.getItem("myshell-multiwindow-cols") ?? "3", 10);
  return {
    rows: isNaN(r) || r < 1 ? 2 : Math.min(r, 6),
    cols: isNaN(col) || col < 1 ? 3 : Math.min(col, 6),
  };
}

// 旧版每次渲染都重读 localStorage；本组件随多窗口模式挂载/卸载，这里读一次，
// 关闭设置抽屉时（可能改过行列配置）重读一次，等价旧版的渲染时重读。
const gridCfg = ref(getMultiWindowGridConfig());
watch(
  () => ui.showSettings,
  (open) => {
    if (!open) gridCfg.value = getMultiWindowGridConfig();
  },
);

const capacity = computed(() => gridCfg.value.rows * gridCfg.value.cols);
const overflow = computed(() => ui.multiWindowIds.length > capacity.value);

// Pick the most balanced grid layout for the current window count, bounded
// by the user's configured max rows/cols. We try every possible column count
// (1..maxCols) and choose the one with the FEWEST empty cells — so 4 windows
// in a 2×3 config → 2×2 (0 empty) instead of 1×3+1 (2 empty). Ties break
// toward more columns (wider cells) when it fills fewer rows.
const layout = computed<{ actualRows: number; actualCols: number }>(() => {
  const n = ui.multiWindowIds.length;
  let actualRows = 1;
  let actualCols = Math.min(n, gridCfg.value.cols);
  let bestEmpty = Infinity;
  for (let c = 1; c <= gridCfg.value.cols; c++) {
    const r = Math.ceil(n / c);
    if (r > gridCfg.value.rows) continue; // would need more rows than configured
    const empty = r * c - n;
    // Prefer fewer empty cells; on tie prefer fewer rows (taller cells).
    if (empty < bestEmpty || (empty === bestEmpty && r < actualRows)) {
      bestEmpty = empty;
      actualRows = r;
      actualCols = c;
    }
  }
  return { actualRows, actualCols };
});

// 网格内联样式（旧版 style 对象）：行高用 1fr 而非基于 100vh 的固定像素，
// 使其始终匹配容器真实尺寸（无论侧栏/AI 面板/最大化状态）。
const gridStyle = computed(() => ({
  gridTemplateColumns: `repeat(${layout.value.actualCols}, minmax(280px, 1fr))`,
  gridTemplateRows: `repeat(${layout.value.actualRows}, 1fr)`,
}));

// 旧版渲染时对每个 id 做 tabs.find，找不到或无 sessionId 的直接跳过不渲染。
const cells = computed<Tab[]>(() =>
  ui.multiWindowIds
    .map((tabId) => sessions.tabs.find((t) => t.id === tabId))
    .filter((t): t is Tab => !!t && !!t.sessionId),
);

function fontOverride(tab: Tab): string | undefined {
  return connectionsStore.connections.find((cn) => cn.id === (tab.connectionId || ""))?.terminal_font;
}

// 旧 onDisconnected：把对应 tab 置为掉线态。
function onDisconnected(tabId: string): void {
  const t = sessions.tabs.find((x) => x.id === tabId);
  if (t) t.status = "disconnected";
}

// 旧 onRemoveWindow：仅从 multiWindowIds 移除；剩余 ≤1 时自动退出多窗口
// （单窗口留在网格模式没有意义——退回单标签视图，窗口本身保留为标签页，
// 并尝试恢复窗口尺寸）。
async function onRemoveWindow(tabId: string): Promise<void> {
  const next = ui.multiWindowIds.filter((id) => id !== tabId);
  ui.multiWindowIds = next;
  if (next.length <= 1) {
    await exitMultiWindow();
  }
}

function onOpenQuickCommandsManage(connectionId: string): void {
  ui.qcInitialConnectionId = connectionId;
  ui.showQuickCommands = true;
}
</script>

<template>
  <div class="grid-root">
    <!-- Toolbar -->
    <div class="toolbar">
      <span class="toolbar-title">
        <el-icon class="toolbar-icon" :size="14"><Grid /></el-icon>
        多窗口模式（{{ ui.multiWindowIds.length }} 个会话{{ overflow ? `，仅展示前 ${capacity} 个，可滚动查看` : "" }}）
      </span>
      <div class="toolbar-actions">
        <MyButton variant="secondary" size="small" title="添加/移除会话" @click="ui.showMultiWindowPicker = true">
          编辑会话
        </MyButton>
        <MyButton variant="ghost" size="small" @click="exitMultiWindow">退出多窗口</MyButton>
      </div>
    </div>
    <!-- Grid -->
    <div class="grid" :style="gridStyle">
      <div v-for="tab in cells" :key="tab.id" class="cell">
        <!-- Window title bar -->
        <div class="cell-titlebar">
          <span class="cell-title" :title="tab.name">{{ tab.name }}</span>
          <button type="button" class="cell-remove" title="从多窗口移除" @click="onRemoveWindow(tab.id)">✕</button>
        </div>
        <!-- Terminal -->
        <div class="cell-terminal">
          <TerminalPanel
            :tab-id="tab.id"
            :session-id="tab.sessionId!"
            :connection-id="tab.connectionId || ''"
            :conn-type="tab.connType"
            :font-override="fontOverride(tab)"
            :renderer-backend="rendererBackend"
            :broadcast-targets="getBroadcastTargets(tab)"
            active
            :status="tab.status"
            :connection-name="tab.name"
            :reconnect-snapshot="tab.reconnectSnapshot"
            @terminal-ready="registerTerminal"
            @terminal-gone="unregisterTerminal"
            @open-ai="ui.showAiPanel = !ui.showAiPanel"
            @open-multiwindow="ui.showMultiWindowPicker = true"
            @reconnect="reconnect(tab.id)"
            @snapshot-consumed="clearReconnectSnapshot(tab.id)"
            @disconnected="onDisconnected(tab.id)"
            @open-quick-commands-manage="
              (cid: string) => {
                onOpenQuickCommandsManage(cid);
              }
            "
          />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.grid-root {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}

.toolbar {
  height: 36px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 12px;
  border-bottom: 1px solid var(--border-subtle);
  background: var(--bg-surface);
}

.toolbar-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.toolbar-title .toolbar-icon {
  color: var(--text-secondary);
}

.toolbar-actions {
  display: flex;
  gap: 8px;
  align-items: center;
}

.grid {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: grid;
  /* 每行高度由 :style 绑定注入（gridTemplateRows: repeat(N, 1fr)） */
  grid-auto-rows: 1fr;
  gap: 6px;
  padding: 6px;
  background: var(--bg-darker, var(--bg-base));
}

.cell {
  display: flex;
  flex-direction: column;
  background: var(--bg-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  overflow: hidden;
  min-width: 320px;
  min-height: 160px;
}

.cell-titlebar {
  height: 28px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 8px;
  background: var(--bg-surface);
  border-bottom: 1px solid var(--border-subtle);
}

.cell-title {
  font-size: 11px;
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cell-remove {
  background: transparent;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 14px;
  padding: 0 4px;
  line-height: 1;
}

.cell-terminal {
  flex: 1;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}
</style>
