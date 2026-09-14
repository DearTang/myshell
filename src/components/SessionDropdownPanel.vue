<!-- 通用"锚定下拉面板"：列出会话标签。由顶栏"当前会话"入口（全部标签）和
     广播徽标（广播组成员）共用。面板经 anchorRect 右对齐定位到触发钮下方。
     每行显示连接图标 + 完整标签名（绝不截断——本面板存在的意义就是标签条
     放不下名字）+ 状态点 + 可选的行尾操作（关闭 / 退出广播），可选的行内
     次操作（广播切换）。点击行切换到该标签并关闭面板；点外与 Esc 均关闭。
     本文件还导出两个辅助：readAnchorRect（锚点矩形读取）与
     DropdownTrigger（触发按钮，样式由使用方 AppTopBar 的 scoped CSS 提供：
     父作用域 id 只会落到子组件根节点，故内部 span 走 :deep()）。 -->
<script lang="ts">
import { defineComponent, h, ref } from "vue";
import { ElIcon } from "element-plus";
import { Document, Promotion } from "@element-plus/icons-vue";
import type { Tab } from "@/api";

/** 面板/触发钮可用的框架图标（EP 单色图标，取代旧 emoji）。 */
export const DROPDOWN_ICONS = {
  Document,
  Promotion,
} as const;
export type DropdownIconName = keyof typeof DROPDOWN_ICONS;

/** 按名字渲染 EP 图标的辅助（render 函数用）。 */
function renderIcon(name: DropdownIconName, size = 14) {
  return h(ElIcon, { size }, () => h(DROPDOWN_ICONS[name]));
}

/** 每行的数据：tab + 行尾主操作（如"✕"关闭 / "退出"广播组）。 */
export interface DropdownRow {
  tab: Tab;
  /** 行尾主操作按钮文案。undefined ⇒ 不渲染行尾按钮。 */
  actionLabel?: string;
  actionTitle?: string;
  onAction?: (tab: Tab) => void;
}

/** 标题行的批量操作按钮（自包含：文案 + 回调）。 */
export interface BatchAction {
  label: string;
  title?: string;
  onClick: () => void;
}

/** 行内次操作信息（如广播切换）。iconName 与 label 二选一。 */
export interface RowSecondaryActionInfo {
  label?: string;
  iconName?: DropdownIconName;
  title: string;
  active: boolean;
}

/** 读取触发元素的锚点矩形（右缘 x + 底缘 y）用于面板定位；未挂载返回 null。 */
export function readAnchorRect(el: HTMLElement | null): { right: number; top: number } | null {
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { right: r.right, top: r.bottom };
}

/** 两个下拉触发钮（当前会话 / 广播）共用的切换按钮：图标 + 文案 + 计数徽标。
 * 父组件通过组件 ref（`$el`）拿按钮 DOM 作为定位锚点。 */
export const DropdownTrigger = defineComponent({
  name: "DropdownTrigger",
  props: {
    iconName: { type: String, required: true },
    label: { type: String, required: true },
    count: { type: Number, required: true },
    active: { type: Boolean, default: false },
    title: { type: String, required: true },
    accent: { type: Boolean, default: false },
  },
  emits: { click: () => true },
  setup(props, { emit }) {
    const hover = ref(false);
    return () =>
      h(
        "button",
        {
          type: "button",
          class: [
            "dropdown-trigger",
            { "is-active": props.active, "is-accent": props.accent, "is-hover": hover.value },
          ],
          title: props.title,
          onClick: () => emit("click"),
          onMouseenter: () => {
            hover.value = true;
          },
          onMouseleave: () => {
            hover.value = false;
          },
        },
        [
          h("span", { class: "trigger-icon" }, renderIcon(props.iconName as DropdownIconName)),
          h("span", { class: "trigger-label" }, props.label),
          h(
            "span",
            { class: ["trigger-count", { "is-hot": props.active || props.accent }] },
            String(props.count),
          ),
        ],
      );
  },
});
</script>

<script setup lang="ts">
import { computed, onMounted, onUnmounted } from "vue";
import ConnIcon from "./ConnIcon.vue";

defineOptions({ name: "SessionDropdownPanel" });

/** 状态 → 彩色圆点，与标签条内联指示器共享语义。 */
const STATUS_DOT: Record<NonNullable<Tab["status"]>, { color: string; title: string }> = {
  connecting: { color: "var(--warning)", title: "连接中" },
  connected: { color: "var(--success)", title: "已连接" },
  disconnected: { color: "var(--error)", title: "已断开" },
  error: { color: "var(--error)", title: "连接失败" },
};

const props = withDefaults(
  defineProps<{
    title: string;
    iconName: DropdownIconName;
    rows: DropdownRow[];
    activeTabId: string | null;
    /** 面板的屏幕锚点——面板定位到该矩形的下方并右对齐。 */
    anchorRect: { right: number; top: number };
    onSelect: (tab: Tab) => void;
    onClose: () => void;
    /** 无行时的空态提示。 */
    emptyHint?: string;
    /** 大于 0 时标题行渲染"清理掉线"按钮，批量关闭所有掉线（已断开/失败）
     * 标签。面板保持打开，用户能看到列表原地更新。 */
    onCloseDisconnected?: () => void;
    /** 本面板内掉线行数——控制按钮显隐并喂给按钮文案。 */
    disconnectedCount?: number;
    /** 标题行里的批量操作按钮。每项自包含（文案 + 回调），调用方可注入
     * 自定义批量操作（全部重连 / 全部广播 / 退出全部广播等）而无需改面板内部。 */
    batchActions?: BatchAction[];
    /** 行内次操作——主操作之后的第二个行尾按钮（会话列表里的广播切换 📡）。 */
    rowSecondaryAction?: (tab: Tab) => RowSecondaryActionInfo | undefined;
    /** 次操作点击回调。 */
    onRowSecondaryAction?: (tab: Tab) => void;
  }>(),
  { disconnectedCount: 0 },
);

const panelRef = ref<HTMLDivElement | null>(null);

// Esc 关闭；点外关闭（overlay 也兜底，但这里覆盖父组件没有渲染全屏
// 拦截层的情形）。
function onKey(e: KeyboardEvent): void {
  if (e.key === "Escape") props.onClose();
}
onMounted(() => document.addEventListener("keydown", onKey));
onUnmounted(() => document.removeEventListener("keydown", onKey));

const PANEL_WIDTH = 320;
const GAP = 6;
// 面板右缘对齐触发钮右缘（保持在打开它的按钮下方）；左缘钳到 8px 防止
// 溢出视口左侧。保持响应式：窗口 resize/scroll 时锚点会重算。
const panelLeft = computed(() => `${Math.max(8, props.anchorRect.right - PANEL_WIDTH)}px`);
const panelTop = computed(() => `${props.anchorRect.top + GAP}px`);

interface RenderRow extends DropdownRow {
  dot: { color: string; title: string };
  secondary: RowSecondaryActionInfo | null;
}

const renderRows = computed<RenderRow[]>(() =>
  props.rows.map((row) => ({
    ...row,
    dot: STATUS_DOT[row.tab.status || "connected"],
    secondary:
      props.rowSecondaryAction && props.onRowSecondaryAction
        ? props.rowSecondaryAction(row.tab) ?? null
        : null,
  })),
);

function handleRowClick(row: DropdownRow): void {
  props.onSelect(row.tab);
  props.onClose();
}
</script>

<template>
  <!-- 全屏点击拦截层——点击面板外任意处关闭（含再次点击触发钮，即切换关闭）。 -->
  <div class="ddp-overlay" @click="onClose()" />
  <div
    ref="panelRef"
    class="ddp-panel"
    :style="{ left: panelLeft, top: panelTop }"
    @click.stop
  >
    <!-- 标题行 -->
    <div class="ddp-header">
      <span class="ddp-header-icon"><el-icon :size="14"><component :is="DROPDOWN_ICONS[iconName]" /></el-icon></span>
      <button
        v-if="onCloseDisconnected && disconnectedCount > 0"
        type="button"
        class="ddp-btn ddp-btn-danger"
        :title="`批量关闭 ${disconnectedCount} 个掉线的会话`"
        @click.stop="onCloseDisconnected()"
      >
        清理掉线 × {{ disconnectedCount }}
      </button>
      <button
        v-for="(action, i) in batchActions"
        :key="i"
        type="button"
        class="ddp-btn"
        :title="action.title || action.label"
        @click.stop="action.onClick()"
      >
        {{ action.label }}
      </button>
      <span class="ddp-count">{{ rows.length }}</span>
    </div>

    <!-- 列表 -->
    <div class="ddp-list">
      <div v-if="rows.length === 0" class="ddp-empty">{{ emptyHint || "暂无会话" }}</div>
      <template v-else>
        <div
          v-for="row in renderRows"
          :key="row.tab.id"
          class="ddp-row"
          :class="{ 'is-active': row.tab.id === activeTabId }"
          :title="row.tab.name"
          @click="handleRowClick(row)"
        >
          <ConnIcon
            class="ddp-row-icon"
            :type="row.tab.connType || 'ssh'"
            :size="15"
          />
          <span class="ddp-row-name">{{ row.tab.name }}</span>
          <!-- 状态点 -->
          <span
            class="ddp-row-dot"
            :title="row.dot.title"
            :style="{
              background: row.dot.color,
              boxShadow: row.tab.status === 'connecting' ? `0 0 5px ${row.dot.color}` : 'none',
            }"
          />
          <!-- 行内次操作（如广播切换） -->
          <span
            v-if="row.secondary"
            class="ddp-row-secondary"
            :class="{ 'is-on': row.secondary.active }"
            :title="row.secondary.title"
            @click.stop="onRowSecondaryAction?.(row.tab)"
          >
            <el-icon v-if="row.secondary.iconName" :size="12">
              <component :is="DROPDOWN_ICONS[row.secondary.iconName]" />
            </el-icon>
            <template v-else>{{ row.secondary.label }}</template>
          </span>
          <span
            v-if="row.onAction && row.actionLabel"
            class="ddp-row-action"
            :title="row.actionTitle"
            @click.stop="row.onAction(row.tab)"
          >
            {{ row.actionLabel }}
          </span>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.ddp-overlay {
  position: fixed;
  inset: 0;
  z-index: 1099;
}

.ddp-panel {
  position: fixed;
  width: 320px;
  max-height: 420px;
  z-index: 1100;
  background: var(--bg-elevated);
  border: 1px solid var(--border-emphasis);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  backdrop-filter: blur(var(--glass-blur));
  display: flex;
  flex-direction: column;
  overflow: hidden;
  transform-origin: top right;
  animation: ddp-scale-in var(--duration-normal) var(--ease-out-back);
}

@keyframes ddp-scale-in {
  from {
    opacity: 0;
    transform: scale(0.96);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

/* ─── 标题行 ─── */
.ddp-header {
  padding: 11px 14px;
  border-bottom: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
}

.ddp-header-icon {
  font-size: 14px;
}

.ddp-btn {
  font-size: 11px;
  font-weight: 500;
  color: var(--text-secondary);
  background: transparent;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 3px 9px;
  cursor: pointer;
  margin-right: 4px;
  white-space: nowrap;
  transition: all var(--duration-fast) var(--ease-in-out);
  font-family: inherit;
}

.ddp-btn:hover {
  background: var(--bg-surface-hover);
  border-color: var(--border-emphasis);
}

.ddp-btn-danger {
  color: var(--error);
  border-color: var(--error);
}

.ddp-btn-danger:hover {
  /* 旧版为白字；白不是令牌，统一用反色文本令牌 */
  background: var(--error);
  color: var(--text-inverse);
}

.ddp-count {
  font-size: 11px;
  color: var(--text-muted);
  background: var(--bg-surface);
  padding: 1px 8px;
  border-radius: var(--radius-full);
}

/* ─── 列表 ─── */
.ddp-list {
  overflow-y: auto;
  padding: 6px;
}

.ddp-empty {
  padding: 24px 12px;
  text-align: center;
  font-size: 12px;
  color: var(--text-muted);
  line-height: 1.6;
}

.ddp-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  cursor: pointer;
  background: transparent;
  transition: background var(--duration-fast) var(--ease-in-out);
}

.ddp-row:hover:not(.is-active) {
  background: var(--bg-surface-hover);
}

.ddp-row.is-active {
  background: var(--accent-primary-muted);
}

.ddp-row-icon {
  opacity: 0.85;
  flex-shrink: 0;
}

/* 活动行图标着主色（覆盖 ConnIcon 的内联类型色，需 !important） */
.ddp-row.is-active .ddp-row-icon {
  color: var(--accent-primary) !important;
}

.ddp-row-name {
  flex: 1;
  min-width: 0;
  font-size: 12.5px;
  color: var(--text-primary);
  font-weight: 400;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ddp-row.is-active .ddp-row-name {
  color: var(--accent-primary);
  font-weight: 600;
}

.ddp-row-dot {
  width: 7px;
  height: 7px;
  border-radius: var(--radius-full);
  flex-shrink: 0;
}

.ddp-row-secondary {
  flex-shrink: 0;
  font-size: 12px;
  cursor: pointer;
  padding: 1px 4px;
  color: var(--text-muted);
  opacity: 0.5;
  transition: opacity var(--duration-fast) var(--ease-in-out);
}

.ddp-row-secondary.is-on {
  color: var(--success);
  opacity: 1;
}

.ddp-row-action {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--text-muted);
  cursor: pointer;
  padding: 2px 5px;
  border-radius: var(--radius-sm);
  line-height: 1;
  transition: all var(--duration-fast) var(--ease-in-out);
  min-width: 18px;
  text-align: center;
}

.ddp-row-action:hover {
  color: var(--error);
  background: var(--error-muted);
}
</style>
