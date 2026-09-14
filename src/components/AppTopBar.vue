<!-- 顶部栏（自包含，无 props）：品牌区 + 标签条 + 会话/广播下拉面板 +
     壳层动作按钮。全部状态经 store（sessions/ui/connections）直接读写。
     移植自旧 TabBar.tsx（功能结构）+ unified-ui-vue 模板 AppTopBar
     （毛玻璃观感）。高度填满壳层 grid 第一行 var(--ui-topbar-h)。 -->
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { Promotion } from "@element-plus/icons-vue";
import {
  ChatDotRound,
  Expand,
  Fold,
  Grid,
  Lightning,
  Setting,
} from "@element-plus/icons-vue";
import { persistUi, ui } from "@/store/ui";
import {
  broadcastAll,
  closeDisconnected,
  closeTab,
  exitAllBroadcast,
  reconnectAll,
  sessions,
  setActiveTab,
  toggleBroadcast,
} from "@/store/sessions";
import type { Tab } from "@/api";
import ConnIcon from "./ConnIcon.vue";
import SessionDropdownPanel from "./SessionDropdownPanel.vue";
import {
  DropdownTrigger,
  readAnchorRect,
  type BatchAction,
  type DropdownRow,
  type RowSecondaryActionInfo,
} from "./SessionDropdownPanel.vue";

defineOptions({ name: "AppTopBar" });

type TabStatus = NonNullable<Tab["status"]>;

// 状态指示器的颜色与图标（与旧 TabBar 一致）
const STATUS_CONFIG: Record<TabStatus, { color: string; icon: string; label: string; fontSize: number }> = {
  connecting: { color: "var(--warning)", icon: "⏳", label: "连接中", fontSize: 14 },
  connected: { color: "var(--success)", icon: "●", label: "已连接", fontSize: 14 },
  disconnected: { color: "var(--error)", icon: "●", label: "已断开", fontSize: 14 },
  error: { color: "var(--error)", icon: "✕", label: "连接失败", fontSize: 12 },
};

function statusOf(tab: Tab): (typeof STATUS_CONFIG)[TabStatus] {
  return STATUS_CONFIG[tab.status || "connected"];
}

function canBroadcast(tab: Tab): boolean {
  return tab.type === "terminal" && tab.connType === "ssh" && tab.status === "connected";
}

const broadcastCount = computed(() => sessions.broadcastIds.size);

// 广播组内的标签（已连接的 SSH 终端），按标签条顺序，供广播下拉用
const broadcastTabs = computed(() => sessions.tabs.filter((t) => sessions.broadcastIds.has(t.id)));

const downTabCount = computed(
  () => sessions.tabs.filter((t) => t.status === "disconnected" || t.status === "error").length,
);

// ── 下拉面板开关 ──
// 当前打开的下拉面板："sessions"（全部标签）/"broadcast"（广播组），或 null。
// 同时只开一个；再点同一触发钮即关闭。
type OpenPanel = "sessions" | "broadcast" | null;
const openPanel = ref<OpenPanel>(null);
const anchor = ref<{ right: number; top: number } | null>(null);

// 触发钮 DOM（组件根 button），作为面板定位锚点
const sessionsTriggerRef = ref<{ $el: HTMLButtonElement } | null>(null);
const broadcastTriggerRef = ref<{ $el: HTMLButtonElement } | null>(null);

// 广播组清空时自动关闭广播面板（触发钮消失，面板会变孤儿）。标签数量变化
// 不触发关闭：用户在会话面板里连续关标签时期望面板保持打开。
watch(broadcastCount, (count) => {
  if (openPanel.value === "broadcast" && count === 0) {
    openPanel.value = null;
  }
});

function togglePanel(which: Exclude<OpenPanel, null>, triggerEl: HTMLElement | null): void {
  if (openPanel.value === which) {
    openPanel.value = null;
    return;
  }
  const rect = readAnchorRect(triggerEl);
  if (rect) anchor.value = rect;
  openPanel.value = which;
}

function toggleSessionsPanel(): void {
  togglePanel("sessions", sessionsTriggerRef.value?.$el ?? null);
}

function toggleBroadcastPanel(): void {
  togglePanel("broadcast", broadcastTriggerRef.value?.$el ?? null);
}

// 窗口 resize / 任意容器 scroll（capture 覆盖标签条内滚动）时重算锚点，
// 面板跟随触发钮。
function recomputeAnchor(): void {
  if (!openPanel.value) return;
  const inst = openPanel.value === "sessions" ? sessionsTriggerRef.value : broadcastTriggerRef.value;
  const rect = readAnchorRect(inst?.$el ?? null);
  if (rect) anchor.value = rect;
}

onMounted(() => {
  window.addEventListener("resize", recomputeAnchor);
  window.addEventListener("scroll", recomputeAnchor, true);
});

onUnmounted(() => {
  window.removeEventListener("resize", recomputeAnchor);
  window.removeEventListener("scroll", recomputeAnchor, true);
});

// ── 面板数据 ──

// 会话面板行：主操作 = 关闭标签
const sessionRows = computed<DropdownRow[]>(() =>
  sessions.tabs.map((t) => ({
    tab: t,
    actionLabel: "✕",
    actionTitle: "关闭标签页",
    onAction: (tab) => void closeTab(tab.id),
  })),
);

const sessionBatchActions = computed<BatchAction[]>(() => {
  const actions: BatchAction[] = [];
  if (sessions.tabs.some((t) => t.status === "disconnected" || t.status === "error")) {
    actions.push({ label: "全部重连", title: "重连所有掉线的会话", onClick: () => void reconnectAll() });
  }
  if (sessions.tabs.some((t) => t.type === "terminal" && t.connType === "ssh" && t.status === "connected")) {
    actions.push({ label: "全部广播", title: "将所有已连接的 SSH 会话加入广播组", onClick: () => broadcastAll() });
  }
  return actions;
});

// 会话面板行内次操作：广播切换（仅已连接的 SSH 终端显示）
function rowSecondaryAction(tab: Tab): RowSecondaryActionInfo | undefined {
  if (tab.type !== "terminal" || tab.connType !== "ssh" || tab.status !== "connected") return undefined;
  return {
    iconName: "Promotion",
    title: sessions.broadcastIds.has(tab.id) ? "退出广播" : "加入广播",
    active: sessions.broadcastIds.has(tab.id),
  };
}

// 广播面板行：主操作 = 退出广播组
const broadcastRows = computed<DropdownRow[]>(() =>
  broadcastTabs.value.map((t) => ({
    tab: t,
    actionLabel: "退出",
    actionTitle: "退出广播组",
    onAction: (tab) => toggleBroadcast(tab.id),
  })),
);

const broadcastBatchActions = computed<BatchAction[]>(() => [
  { label: "退出全部广播", title: "移除所有广播组成员", onClick: () => exitAllBroadcast() },
]);

const broadcastDownCount = computed(
  () => broadcastTabs.value.filter((t) => t.status === "disconnected" || t.status === "error").length,
);

// ── 壳层动作 ──

function toggleSidebar(): void {
  ui.sidebarCollapsed = !ui.sidebarCollapsed;
  persistUi();
}

function openQuickCommands(): void {
  ui.qcInitialConnectionId = null;
  ui.showQuickCommands = true;
}
</script>

<template>
  <header class="app-topbar">
    <!-- 左：侧栏折叠 + 品牌区 -->
    <div class="topbar-left">
      <button
        type="button"
        class="icon-btn"
        :title="ui.sidebarCollapsed ? '展开侧栏' : '收起侧栏'"
        :aria-expanded="!ui.sidebarCollapsed"
        @click="toggleSidebar"
      >
        <el-icon :size="17"><Expand v-if="ui.sidebarCollapsed" /><Fold v-else /></el-icon>
      </button>
      <div class="brand">
        <!-- 内联矢量品牌标记 >_（与应用图标同款），随主题令牌变色 -->
        <svg class="brand-mark" viewBox="0 0 64 64" fill="none" role="img" aria-label="MyShell">
          <defs>
            <linearGradient id="app-topbar-brand-grad" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" style="stop-color: var(--accent-primary-hover)" />
              <stop offset="0.5" style="stop-color: var(--accent-primary)" />
              <stop offset="1" style="stop-color: var(--accent-secondary)" />
            </linearGradient>
          </defs>
          <!-- 折号 `>`：顶点在左，开口朝右 -->
          <path
            d="M35 19 L16 32 L35 45"
            stroke="url(#app-topbar-brand-grad)"
            stroke-width="6.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
          <!-- 光标 `_` -->
          <rect x="37" y="42" width="13" height="5" rx="2.5" style="fill: var(--accent-secondary)" />
        </svg>
        <span class="brand-name">MyShell</span>
      </div>
    </div>

    <!-- 中：标签条（横向滚动）+ 右侧下拉触发钮 -->
    <div class="tab-strip">
      <div class="tabs-scroll">
        <div
          v-for="tab in sessions.tabs"
          :key="tab.id"
          class="tab-item"
          :class="{ 'is-active': tab.id === sessions.activeTabId }"
          @click="setActiveTab(tab.id)"
        >
          <!-- 活动指示条 -->
          <span
            v-if="tab.id === sessions.activeTabId"
            class="tab-active-bar"
            :class="`is-${tab.status || 'connected'}`"
          />

          <!-- 连接类型图标（随标签文字着色） -->
          <ConnIcon class="tab-icon" :type="tab.connType || 'ssh'" :size="14" :name="tab.name" />

          <!-- 标签名（广播组优先于错误态着色，与旧版三元表达式一致） -->
          <span
            class="tab-name"
            :class="{ 'is-error': (tab.status || 'connected') === 'error', 'in-broadcast': sessions.broadcastIds.has(tab.id) }"
            :title="tab.name"
          >
            {{ tab.name }}
          </span>

          <!-- 状态指示（始终显示） -->
          <span
            class="tab-status"
            :title="statusOf(tab).label"
            :style="{ color: statusOf(tab).color, fontSize: `${statusOf(tab).fontSize}px` }"
          >
            <span :class="{ spinning: (tab.status || 'connected') === 'connecting' }">{{ statusOf(tab).icon }}</span>
          </span>

          <!-- 广播切换 -->
          <span
            v-if="canBroadcast(tab)"
            class="tab-broadcast"
            :class="{ 'in-group': sessions.broadcastIds.has(tab.id) }"
            :title="
              sessions.broadcastIds.has(tab.id)
                ? `广播组中（共 ${broadcastCount} 个 tab）— 点击退出`
                : '加入广播'
            "
            @click.stop="toggleBroadcast(tab.id)"
          >
        <el-icon :size="12"><Promotion /></el-icon>
          </span>

          <!-- 关闭钮 -->
          <span class="tab-close" @click.stop="closeTab(tab.id)">✕</span>
        </div>
      </div>

      <!-- "当前会话"触发钮——常驻：标签再多也能一键看全量列表 -->
      <DropdownTrigger
        ref="sessionsTriggerRef"
        icon-name="Document"
        label="当前会话"
        :count="sessions.tabs.length"
        :active="openPanel === 'sessions'"
        title="查看/切换/关闭所有标签页"
        @click="toggleSessionsPanel"
      />

      <!-- "广播"触发钮——取代旧的静态徽标：可点击打开广播组面板 -->
      <DropdownTrigger
        v-if="broadcastCount > 0"
        ref="broadcastTriggerRef"
        icon-name="Promotion"
        label="广播"
        :count="broadcastCount"
        :active="openPanel === 'broadcast'"
        :title="`${broadcastCount} 个标签页在广播组中，点击查看/管理`"
        accent
        @click="toggleBroadcastPanel"
      />
    </div>

    <!-- 右：壳层动作钮 -->
    <div class="topbar-actions">
      <button type="button" class="icon-btn" title="多窗口" @click="ui.showMultiWindowPicker = true">
        <el-icon :size="16"><Grid /></el-icon>
      </button>
      <button type="button" class="icon-btn" title="AI 助手" @click="ui.showAiPanel = !ui.showAiPanel">
        <el-icon :size="16"><ChatDotRound /></el-icon>
      </button>
      <button type="button" class="icon-btn" title="快捷命令" @click="openQuickCommands">
        <el-icon :size="16"><Lightning /></el-icon>
      </button>
      <button type="button" class="icon-btn" title="设置" @click="ui.showSettings = true">
        <el-icon :size="16"><Setting /></el-icon>
      </button>
    </div>

    <!-- 下拉面板（锚定于各自触发钮） -->
    <SessionDropdownPanel
      v-if="openPanel === 'sessions' && anchor"
      title="当前会话"
      icon-name="Document"
      :anchor-rect="anchor"
      :active-tab-id="sessions.activeTabId"
      :on-select="(t) => setActiveTab(t.id)"
      :on-close="() => (openPanel = null)"
      empty-hint="暂无打开的标签页"
      :on-close-disconnected="closeDisconnected"
      :disconnected-count="downTabCount"
      :rows="sessionRows"
      :batch-actions="sessionBatchActions"
      :row-secondary-action="rowSecondaryAction"
      :on-row-secondary-action="(t) => toggleBroadcast(t.id)"
    />
    <SessionDropdownPanel
      v-if="openPanel === 'broadcast' && anchor"
      title="广播组成员"
      icon-name="Promotion"
      :anchor-rect="anchor"
      :active-tab-id="sessions.activeTabId"
      :on-select="(t) => setActiveTab(t.id)"
      :on-close="() => (openPanel = null)"
      empty-hint="广播组为空"
      :on-close-disconnected="closeDisconnected"
      :disconnected-count="broadcastDownCount"
      :rows="broadcastRows"
      :batch-actions="broadcastBatchActions"
    />
  </header>
</template>

<style scoped>
.app-topbar {
  /* App.vue 壳层未给 .shell-topbar 设 grid-area，由本组件自己声明，
     横贯第一行三列并填满 var(--ui-topbar-h) */
  grid-area: topbar;
  display: flex;
  align-items: stretch;
  height: 100%;
  min-width: 0;
  overflow: hidden;
  background: var(--surface-translucent);
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 1));
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 1));
  border-bottom: 1px solid var(--border-subtle);
}

/* ─── 左：折叠钮 + 品牌 ─── */
.topbar-left {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 4px 0 8px;
  flex-shrink: 0;
}

.icon-btn {
  width: 32px;
  height: 32px;
  display: inline-grid;
  place-items: center;
  padding: 0;
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  flex-shrink: 0;
}

.icon-btn:hover {
  color: var(--text-primary);
  background: var(--surface-translucent-hover);
  border-color: var(--border-subtle);
}

.brand {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 6px;
}

.brand-mark {
  width: 24px;
  height: 24px;
  display: block;
}

.brand-name {
  color: var(--text-primary);
  font-size: 14px;
  font-weight: 600;
  letter-spacing: 0.02em;
  white-space: nowrap;
}

/* ─── 中：标签条 ─── */
.tab-strip {
  display: flex;
  flex: 1;
  min-width: 0;
  height: 100%;
  overflow: hidden;
}

.tabs-scroll {
  display: flex;
  flex: 1;
  min-width: 0;
  height: 100%;
  overflow: auto;
}

.tab-item {
  position: relative;
  height: 100%;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 14px;
  font-size: 12px;
  cursor: pointer;
  border-right: 1px solid var(--border-subtle);
  background: transparent;
  color: var(--text-secondary);
  white-space: nowrap;
  min-width: 0;
  transition:
    background var(--duration-fast) var(--ease-in-out),
    color var(--duration-fast) var(--ease-in-out);
}

.tab-item:hover:not(.is-active) {
  background: var(--bg-surface-hover);
}

.tab-item.is-active {
  background: var(--bg-base);
  color: var(--text-primary);
}

.tab-active-bar {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  height: 2px;
  background: var(--accent-primary);
  border-radius: 2px 2px 0 0;
}

.tab-active-bar.is-error {
  background: var(--error);
}

.tab-active-bar.is-disconnected {
  background: var(--text-muted);
}

/* 类型图标随标签文字着色（覆盖 ConnIcon 内联类型色，需 !important） */
.tab-icon {
  opacity: 0.85;
  color: inherit !important;
  flex-shrink: 0;
}

.tab-name {
  overflow: hidden;
  text-overflow: ellipsis;
  font-weight: 400;
}

.tab-item.is-active .tab-name {
  font-weight: 500;
}

/* 着色优先级与旧版一致：广播组（绿）优先于错误（红） */
.tab-name.is-error {
  color: var(--error);
}

.tab-name.in-broadcast {
  color: var(--success);
}

.tab-status {
  display: flex;
  align-items: center;
  gap: 2px;
}

.tab-status .spinning {
  display: inline-block;
  animation: spin 1s linear infinite;
}

.tab-broadcast {
  font-size: 12px;
  opacity: 0.4;
  cursor: pointer;
  padding: 2px 4px;
  color: var(--text-muted);
  transition: all var(--duration-fast) var(--ease-in-out);
  border-radius: var(--radius-sm);
  flex-shrink: 0;
}

.tab-broadcast.in-group {
  opacity: 1;
  color: var(--success);
}

.tab-broadcast:not(.in-group):hover {
  opacity: 1;
  background: var(--bg-surface-active);
}

.tab-close {
  margin-left: 2px;
  font-size: 14px;
  opacity: 0.35;
  cursor: pointer;
  line-height: 1;
  padding: 2px 4px;
  border-radius: var(--radius-sm);
  transition: all var(--duration-fast) var(--ease-in-out);
  flex-shrink: 0;
}

.tab-close:hover {
  opacity: 1;
  background: var(--error-muted);
  color: var(--error);
}

/* ─── DropdownTrigger（组件根 button 继承本组件作用域 id；
      内部 span 需 :deep） ─── */
.dropdown-trigger {
  height: 100%;
  padding: 0 14px;
  display: flex;
  align-items: center;
  gap: 8px;
  border: none;
  border-left: 1px solid var(--border-default);
  background: transparent;
  color: var(--text-secondary);
  font-size: 11px;
  font-weight: 600;
  cursor: pointer;
  white-space: nowrap;
  flex-shrink: 0;
  font-family: inherit;
  transition:
    background var(--duration-fast) var(--ease-in-out),
    color var(--duration-fast) var(--ease-in-out);
}

.dropdown-trigger.is-hover:not(.is-active):not(.is-accent) {
  background: var(--bg-surface-hover);
  color: var(--text-primary);
}

.dropdown-trigger.is-active,
.dropdown-trigger.is-accent {
  background: var(--accent-primary-muted);
  color: var(--accent-primary);
}

.dropdown-trigger :deep(.trigger-icon) {
  font-size: 14px;
}

.dropdown-trigger :deep(.trigger-count) {
  font-size: 11px;
  background: var(--bg-surface-active);
  color: var(--text-secondary);
  padding: 0 7px;
  border-radius: var(--radius-full);
  min-width: 20px;
  text-align: center;
  line-height: 16px;
}

.dropdown-trigger.is-active :deep(.trigger-count),
.dropdown-trigger.is-accent :deep(.trigger-count) {
  /* 旧版为白字；白不是令牌，统一用反色文本令牌 */
  background: var(--accent-primary);
  color: var(--text-inverse);
}

/* ─── 右：壳层动作 ─── */
.topbar-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  height: 100%;
  padding: 0 8px;
  flex-shrink: 0;
  border-left: 1px solid var(--border-subtle);
}
</style>
