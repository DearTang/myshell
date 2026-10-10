<!-- 顶部栏（自包含，无 props）：品牌区 + 标签条 + 会话/广播下拉面板 +
     壳层动作按钮 + 窗口控制。全部状态经 store（sessions/ui/connections）直接读写。
     移植自旧 TabBar.tsx（功能结构）+ unified-ui-vue 模板 AppTopBar
     （毛玻璃观感）。高度填满壳层 grid 第一行 var(--ui-topbar-h)。
     v2.15.4 起窗口无边框（decorations:false），最小化/最大化/关闭按钮
     内嵌本栏最右（omp desktop 式一行顶栏），空白区域可拖动窗口。 -->
<script setup lang="ts">
import { MyButton, MyIcon, MyIconButton, MyStatusDot, type MyTone } from "myui";
import { computed, onMounted, onUnmounted, ref } from "vue";
import { persistUi, setMcpSessionAllowed, ui } from "@/store/ui";
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
import WindowControls from "./WindowControls.vue";
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
/** 标签状态 → 语义色调 + 文案。颜色与呼吸动画由 MyStatusDot 的 tone/pulse 承担。
 *
 *  2026-10-10：此前这里存的是 `color: "var(--error)"` 之类的 CSS 颜色值，
 *  模板再内联 `background`/`boxShadow` —— **绕过了令牌层**，33 套配色预设切换时
 *  状态点颜色不变。改成语义 tone 后由 myui 经 `--success/--warning/--danger`
 *  取色，预设能生效。 */
const STATUS_CONFIG: Record<TabStatus, { tone: MyTone; label: string; pulse?: boolean }> = {
  connecting: { tone: "warning", label: "连接中", pulse: true },
  connected: { tone: "success", label: "已连接" },
  disconnected: { tone: "neutral", label: "已断开" },
  error: { tone: "danger", label: "连接失败" },
};

function statusOf(tab: Tab): (typeof STATUS_CONFIG)[TabStatus] {
  return STATUS_CONFIG[tab.status || "connected"];
}

function canBroadcast(tab: Tab): boolean {
  return tab.type === "terminal" && tab.connType === "ssh" && tab.status === "connected";
}

const broadcastCount = computed(() => sessions.broadcastIds.size);

const downTabCount = computed(
  () => sessions.tabs.filter((t) => t.status === "disconnected" || t.status === "error").length,
);

// ── 下拉面板开关 ──
// 2026-10-10：「当前会话」与「广播」两个触发钮合并为一个（两者列的都是标签页，
// 只是口径不同，视图上确实重叠）。广播状态改为触发钮上的 📡 徽标 + 面板内的
// 批量动作 / 行内开关。openPanel 因此退化成布尔量。
const openPanel = ref(false);
const anchor = ref<{ right: number; top: number } | null>(null);

// 触发钮 DOM（组件根 button），作为面板定位锚点
const sessionsTriggerRef = ref<{ $el: HTMLButtonElement } | null>(null);

function toggleSessionsPanel(): void {
  if (openPanel.value) {
    openPanel.value = false;
    return;
  }
  const rect = readAnchorRect(sessionsTriggerRef.value?.$el ?? null);
  if (rect) anchor.value = rect;
  openPanel.value = true;
}

// 窗口 resize / 任意容器 scroll（capture 覆盖标签条内滚动）时重算锚点，
// 面板跟随触发钮。
function recomputeAnchor(): void {
  if (!openPanel.value) return;
  const rect = readAnchorRect(sessionsTriggerRef.value?.$el ?? null);
  if (rect) anchor.value = rect;
}

// 窗口控制按钮已抽到 WindowControls.vue（与保险库门禁页共用）。

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

// 面板标题行的批量操作。合并「广播」面板后，广播的进出都收在这里：
// 「全部广播」把已连接的 SSH 终端全塞进组，「退出全部广播」一键清空。
const sessionBatchActions = computed<BatchAction[]>(() => {
  const actions: BatchAction[] = [];
  if (sessions.tabs.some((t) => t.status === "disconnected" || t.status === "error")) {
    actions.push({ label: "全部重连", title: "重连所有掉线的会话", onClick: () => void reconnectAll() });
  }
  if (
    sessions.tabs.some((t) => t.type === "terminal" && t.connType === "ssh" && t.status === "connected")
  ) {
    actions.push({
      label: "全部广播",
      title: "将所有已连接的 SSH 会话加入广播组",
      onClick: () => broadcastAll(),
    });
  }
  if (broadcastCount.value > 0) {
    actions.push({
      label: "退出全部广播",
      title: "移除所有广播组成员",
      onClick: () => exitAllBroadcast(),
    });
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

// ── 壳层动作 ──

function toggleSidebar(): void {
  ui.sidebarCollapsed = !ui.sidebarCollapsed;
  persistUi();
}

function openQuickCommands(): void {
  ui.qcInitialConnectionId = null;
  ui.showQuickCommands = true;
}

/** 撤销会话放行：仅清除 GUI 侧标志。MCP 服务器仍持有自己的授权标志，
 *  但它每次 exec 请求都带 session_allowed，GUI 是权威判定方——被撤销后
 *  GUI 忽略该字段、照常弹窗，并在响应里回带 session_revoked 让 MCP
 *  同步清除标志。因此无需反向连接 MCP（它由 AI 客户端以 stdio 启动）。 */
function revokeSessionAllowed(): void {
  setMcpSessionAllowed(false);
}
</script>

<template>
  <!-- data-tauri-drag-region：元素自身空白处可拖动窗口（子元素不受影响）；
       标签条空白区同样可拖（Chrome/omp 式）。双击空白 = 最大化/还原。 -->
  <header class="app-topbar" data-tauri-drag-region>
    <!-- 左：侧栏折叠 + 品牌区 -->
    <div class="topbar-left">
      <MyIconButton
        :title="ui.sidebarCollapsed ? '展开侧栏' : '收起侧栏'"
        class="icon-btn"
        :label="ui.sidebarCollapsed ? '展开侧栏' : '收起侧栏'"
        size="17"
        :name="ui.sidebarCollapsed ? 'Expand' : 'Fold'"
        :aria-expanded="!ui.sidebarCollapsed"
        @click="toggleSidebar"
      />
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
      <div class="tabs-scroll" data-tauri-drag-region>
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
          <MyStatusDot
            class="tab-status"
            :tone="statusOf(tab).tone"
            :pulse="statusOf(tab).pulse"
            :aria-label="statusOf(tab).label"
          />

          <!-- 广播切换 -->
          <MyIconButton
            :title="
              sessions.broadcastIds.has(tab.id)
                ? `广播组中（共 ${broadcastCount} 个 tab）— 点击退出`
                : '加入广播'
            "
            v-if="canBroadcast(tab)"
            class="tab-broadcast"
            :label="
              sessions.broadcastIds.has(tab.id)
                ? `广播组中（共 ${broadcastCount} 个 tab）— 点击退出`
                : '加入广播'
            "
            :active="sessions.broadcastIds.has(tab.id)"
            size="12"
            name="Promotion"
            @click.stop="toggleBroadcast(tab.id)"
          />

          <!-- 关闭钮 -->
          <MyIconButton
            title="关闭标签"
            class="tab-close"
            label="关闭标签"
            size="12"
            name="Close"
            @click.stop="closeTab(tab.id)"
          />
        </div>
      </div>

      <!-- 「当前会话」触发钮（常驻：标签再多也能一键看全量列表）——
           2026-10-10 起同时承载广播状态：有广播成员时整钮转强调色，
           并在计数右侧挂一枚 📡 N 徽标。广播的增删在面板内完成。 -->
      <DropdownTrigger
        ref="sessionsTriggerRef"
        icon-name="Document"
        label="当前会话"
        :count="sessions.tabs.length"
        :broadcast-count="broadcastCount"
        :active="openPanel"
        :accent="broadcastCount > 0"
        :title="
          broadcastCount > 0
            ? `查看/切换/关闭所有标签页（共 ${sessions.tabs.length} 个，其中 ${broadcastCount} 个在广播组中）`
            : '查看/切换/关闭所有标签页'
        "
        @click="toggleSessionsPanel"
      />
    </div>

    <!-- 右：壳层动作钮 -->
    <div class="topbar-actions">
      <!-- 会话放行指示器：MCP 高危命令「本轮会话均允许」授权期间常驻显示，
           点击即可撤销（撤销后下一次高危命令重新弹窗确认） -->
      <MyButton
        v-if="ui.mcpSessionAllowed"
        variant="ghost-success"
        size="small"
        class="session-allowed-chip"
        title="本轮会话已放行全部高危命令（点击撤销，撤销后重新弹窗确认）"
        @click="revokeSessionAllowed"
      >
        <MyIcon :size="13" name="Unlock" />
        <span>已放行高危</span>
      </MyButton>
      <MyIconButton
        title="多窗口"
        class="icon-btn"
        label="多窗口"
        size="16"
        name="Grid"
        @click="ui.showMultiWindowPicker = true"
      />
      <MyIconButton
        title="AI 助手"
        class="icon-btn"
        label="AI 助手"
        size="16"
        name="Robot"
        @click="ui.showAiPanel = !ui.showAiPanel"
      />
      <MyIconButton
        title="快捷命令"
        class="icon-btn"
        label="快捷命令"
        size="16"
        name="Lightning"
        @click="openQuickCommands"
      />
      <MyIconButton
        title="设置"
        class="icon-btn"
        label="设置"
        size="16"
        name="Setting"
        @click="ui.showSettings = true"
      />
    </div>

    <!-- 右缘：窗口控制（无边框窗口自绘按钮，与顶栏同行，贴窗口右上角）。
         与保险库门禁页共用同一组件，避免两处逻辑漂移。 -->
    <WindowControls />

    <!-- 下拉面板（锚定到「当前会话」触发钮；广播组的进出也在此面板内完成） -->
    <SessionDropdownPanel
      v-if="openPanel && anchor"
      title="当前会话"
      icon-name="Document"
      :anchor-rect="anchor"
      :active-tab-id="sessions.activeTabId"
      :on-select="(t) => setActiveTab(t.id)"
      :on-close="() => (openPanel = false)"
      empty-hint="暂无打开的标签页"
      :on-close-disconnected="closeDisconnected"
      :disconnected-count="downTabCount"
      :rows="sessionRows"
      :batch-actions="sessionBatchActions"
      :row-secondary-action="rowSecondaryAction"
      :on-row-secondary-action="(t) => toggleBroadcast(t.id)"
      :broadcast-count="broadcastCount"
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
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
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

/* 顶栏动作钮：外观（悬停/按下/禁用/焦点环）由 MyIconButton 承担，
   这里只固定尺寸与静止态次级文字色，并把悬停改成强调色系——
   库的默认悬停是中性 bg-elevated，本项目一律「悬停变主色」。 */
.icon-btn {
  width: 32px;
  height: 32px;
  flex-shrink: 0;
  color: var(--text-secondary);
}

.icon-btn:hover {
  color: var(--accent-primary);
  background: var(--accent-primary-muted);
  border-color: var(--border-accent);
}

.icon-btn:active {
  background: var(--bg-surface-active);
  color: var(--accent-primary-hover);
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

/* 状态点：尺寸与呼吸动画由 MyStatusDot 承担，这里只收紧密度。 */
.tab-status {
  flex-shrink: 0;
  transform: scale(0.9);
}

/* 广播开关与关闭钮：外观由 MyIconButton 承担（含 active 态与 aria-pressed），
   这里只保留标签条密度与「在组内着成功色」的原有意图。 */
.tab-broadcast {
  flex-shrink: 0;
  opacity: 0.55;
}

.tab-broadcast:hover {
  opacity: 1;
}

.tab-close {
  flex-shrink: 0;
  opacity: 0;
  transition: opacity var(--duration-fast) var(--ease-in-out);
}

.tab-item:hover .tab-close,
.tab-item.is-active .tab-close {
  opacity: 1;
}

.tab-close:hover {
  opacity: 1;
  background: var(--error-muted);
  color: var(--error);
}

/* ─── DropdownTrigger（MyButton 的根 <button class="el-button dropdown-trigger">
      继承本组件作用域 id；其内部的 .el-button__content 与各 span 需 :deep） ─── */
.dropdown-trigger {
  height: 100%;
  padding: 0 14px;
  border: none;
  border-left: 1px solid var(--border-default);
  border-radius: 0;
  background: transparent;
  color: var(--text-secondary);
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
  flex-shrink: 0;
  box-shadow: none;
  transition:
    background var(--duration-fast) var(--ease-in-out),
    color var(--duration-fast) var(--ease-in-out);
}

/* MyButton 把插槽内容包进 .el-button__content，原先挂在根节点上的
   flex 布局必须下推到这一层，否则图标/文案/计数会退回 EP 的行内排版。 */
.dropdown-trigger :deep(.el-button__content) {
  display: flex;
  align-items: center;
  gap: 8px;
}

/* 状态色。⚠️ 这里必须每条都自带 :hover 变体：EP 的
   `.el-button.is-text:not(.is-disabled):hover` 权重是 (0,4,0)，高于任何
   不带 :hover 的状态规则（本组件作用域属性只贡献 1 个类权重）。少了 :hover
   变体，「面板已打开时鼠标停在触发钮上」会被 EP 的 --el-fill-color-light
   灰底压过去，强调色闪一下就没了。带 :hover 后同为 (0,4,0)，靠源码顺序取胜
   （组件 CSS 在 EP 之后打包）。 */
.dropdown-trigger.is-hover:not(.is-active):not(.is-accent) {
  background: var(--bg-surface-hover);
  color: var(--text-primary);
}

.dropdown-trigger.is-active,
.dropdown-trigger.is-active:hover,
.dropdown-trigger.is-accent,
.dropdown-trigger.is-accent:hover {
  background: var(--accent-primary-muted);
  color: var(--accent-primary);
}

.dropdown-trigger :deep(.trigger-icon) {
  font-size: 14px;
}

/* 两枚计数徽标形态必须一致：都是「图标 + 数字」的实心/淡底胶囊、同样的
   padding / line-height / gap。2026-10-10 之前会话那枚只有数字、广播那枚带 📡，
   并排时一胖一瘦读起来对不上号。 */
.dropdown-trigger :deep(.trigger-count) {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 3px;
  font-size: 11px;
  background: var(--bg-surface-active);
  color: var(--text-secondary);
  padding: 0 7px;
  border-radius: var(--radius-full);
  min-width: 20px;
  line-height: 16px;
}

.dropdown-trigger.is-active :deep(.trigger-count),
.dropdown-trigger.is-accent :deep(.trigger-count) {
  /* 旧版为白字；白不是令牌，统一用反色文本令牌 */
  background: var(--accent-primary);
  color: var(--text-inverse);
}

/* 广播徽标：合并「广播」触发钮后，组内会话数以实心成功色胶囊挂在计数右侧。
   刻意用实心 —— 它要在一枚同样是胶囊的总数旁边被一眼区分开，描边态在强调色
   按钮底上会和 .trigger-count 糊在一起。 */
.dropdown-trigger :deep(.trigger-bcast) {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-inverse);
  background: var(--success);
  padding: 0 7px;
  border-radius: var(--radius-full);
  line-height: 16px;
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

/* 会话放行指示器：警示色胶囊，点击撤销 */
.session-allowed-chip {
  display: flex;
  align-items: center;
  gap: 4px;
  height: 22px;
  margin-right: 4px;
  padding: 0 8px;
  background: var(--warning-muted);
  border: 1px solid var(--warning);
  border-radius: var(--radius-full, 999px);
  color: var(--warning);
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
  cursor: pointer;
}

.session-allowed-chip:hover {
  background: var(--warning);
  color: var(--text-inverse);
}

/* 窗口控制样式在 WindowControls.vue（顶栏与门禁页共用）。 */

</style>
