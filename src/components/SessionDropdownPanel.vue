<!-- 通用"锚定下拉面板"：列出全部会话标签，锚定到顶栏的「当前会话」触发钮下方。
     每行显示连接图标 + 完整标签名（绝不截断——本面板存在的意义就是标签条
     放不下名字）+ 状态点 + 行尾操作（关闭 / 退出广播），以及行内次操作（广播切换）。
     点击行切换到该标签并关闭面板；点外与 Esc 均关闭。
     2026-10-10 起不再有独立的「广播」面板：广播组是全部标签页的子集，
     单独一个面板与「当前会话」列的是同一批行，改为在本面板内用标题行的
     批量动作 + 行内 📡 开关管理广播，标题行右侧保留广播计数徽标。
     本文件还导出两个辅助：readAnchorRect（锚点矩形读取）与
     DropdownTrigger（触发按钮，样式由使用方 AppTopBar 的 scoped CSS 提供：
     父作用域 id 只会落到子组件根节点，故内部 span 走 :deep()）。 -->
<script lang="ts">
import { defineComponent, h, ref } from "vue";
import { MyButton, MyIcon, MyListRow, MyStatusDot, type MyTone } from "myui";
import type { Tab } from "@/api";

/** 面板/触发钮可用的框架图标（myui 注册表名，取代旧 emoji）。
 *  存字符串名而非组件：MyIcon 的 icon/name 入参都接受注册表名，
 *  模板与 render 函数可共用同一份映射，不必再引 @element-plus/icons-vue。 */
export const DROPDOWN_ICONS = {
  Document: "Document",
  Promotion: "Promotion",
} as const;
export type DropdownIconName = keyof typeof DROPDOWN_ICONS;

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

/** 下拉触发钮（图标 + 文案 + 计数徽标 + 广播徽标）。父组件通过组件 ref（`$el`）
 *  拿按钮 DOM 作为定位锚点。
 *
 *  2026-10-10：「当前会话」与「广播」合并为**同一个**触发钮（两个面板列的是同一批
 *  标签页，只是筛选口径不同，用户视角确实重叠），广播状态改为挂在计数右侧的
 *  📡 徽标，面板内提供批量加入/退出广播与逐行切换。
 *
 *  同时把这里的裸 `<button>` 收口到 `MyButton`——阶段 170 的裸 button 盘点只扫了
 *  模板里的 `<button`，本组件走 `h()` 渲染路径被漏掉了。用 `variant="ghost"`
 *  （EP text 按钮，透明底无边框）贴合顶栏原有的无框观感，其余盒模型仍由
 *  AppTopBar 的 scoped CSS 覆盖。 */
export const DropdownTrigger = defineComponent({
  name: "DropdownTrigger",
  props: {
    iconName: { type: String, required: true },
    label: { type: String, required: true },
    count: { type: Number, required: true },
    /** 广播组成员数；0 时不渲染 📡 徽标。 */
    broadcastCount: { type: Number, default: 0 },
    active: { type: Boolean, default: false },
    title: { type: String, required: true },
    accent: { type: Boolean, default: false },
  },
  emits: { click: () => true },
  setup(props, { emit }) {
    const hover = ref(false);
    return () =>
      h(
        MyButton,
        {
          variant: props.accent ? "ghost-primary" : "ghost",
          size: "small",
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
        {
          default: () => [
            h(MyIcon, { size: 14, name: props.iconName, class: "trigger-icon", decorative: true }),
            h("span", { class: "trigger-label" }, props.label),
            // 两枚计数徽标都带图标，构成一对：「N 个终端会话」与「其中 N 个在广播组」。
            // 此前只有广播那枚有图标（📡 纸飞机），会话那枚是光秃秃的数字，两枚
            // 形态不一致、读起来对不上号。会话数用 Terminal 图标 —— 它数的正是终端
            // 会话，且与 📡（发往）构成「发往这 N 个」的语义对照。
            h(
              "span",
              { class: ["trigger-count", { "is-hot": props.active || props.accent }] },
              [h(MyIcon, { size: 10, name: "Terminal", decorative: true }), String(props.count)],
            ),
            props.broadcastCount > 0
              ? h(
                  "span",
                  {
                    class: "trigger-bcast",
                    title: `${props.broadcastCount} 个标签页在广播组中`,
                  },
                  [
                    h(MyIcon, { size: 10, name: "Promotion", decorative: true }),
                    String(props.broadcastCount),
                  ],
                )
              : null,
          ],
        },
      );
  },
});
</script>

<script setup lang="ts">
import { computed, onMounted, onUnmounted } from "vue";
import ConnIcon from "./ConnIcon.vue";

defineOptions({ name: "SessionDropdownPanel" });

/** 状态 → 语义色调 + 文案。颜色与光晕由 MyStatusDot 的 tone/glow 承担，
 *  故这里不再存 CSS 颜色值——原实现直接内联 `background: var(--error)`，
 *  绕过了 MyShell 的令牌层（33 套配色预设换不掉它）。 */
const STATUS_DOT: Record<
  NonNullable<Tab["status"]>,
  { tone: MyTone; title: string; pulse?: boolean; glow?: boolean }
> = {
  connecting: { tone: "warning", title: "连接中", pulse: true, glow: true },
  connected: { tone: "success", title: "已连接" },
  disconnected: { tone: "danger", title: "已断开" },
  error: { tone: "danger", title: "连接失败", glow: true },
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
    /** 广播组成员数——标题行渲染 📡 徽标（0 时不渲染）。合并「广播」面板后
     *  由调用方直接喂 store 的 broadcastCount。 */
    broadcastCount?: number;
  }>(),
  { disconnectedCount: 0, broadcastCount: 0 },
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
  dot: { tone: MyTone; title: string; pulse?: boolean; glow?: boolean };
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
  <!-- 必须 Teleport 到 body：面板挂在 AppTopBar 里，而顶栏带
       `backdrop-filter`（毛玻璃）+ `overflow: hidden` —— **这两个属性会让顶栏
       成为其后代的 position:fixed 包含块并裁剪之**，面板会被裁到 44px 顶栏
       高度之外，表现是「点当前会话/广播毫无反应」（遮罩同样被裁，连点外关闭
       也失效）。Teleport 后脱离该包含块，scoped 的 data-v 属性仍保留，
       样式照常生效。 -->
  <Teleport to="body">
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
      <span class="ddp-header-icon"><MyIcon :size="14" :icon="DROPDOWN_ICONS[iconName]" /></span>
      <MyButton
        v-if="onCloseDisconnected && disconnectedCount > 0"
        class="ddp-btn"
        variant="danger"
        outline
        size="small"
        :title="`批量关闭 ${disconnectedCount} 个掉线的会话`"
        @click.stop="onCloseDisconnected()"
      >
        清理掉线 × {{ disconnectedCount }}
      </MyButton>
      <MyButton
        v-for="(action, i) in batchActions"
        :key="i"
        class="ddp-btn"
        variant="secondary"
        outline
        size="small"
        :title="action.title || action.label"
        @click.stop="action.onClick()"
      >
        {{ action.label }}
      </MyButton>
      <span class="ddp-count">{{ rows.length }}</span>
      <span
        v-if="broadcastCount > 0"
        class="ddp-bcast"
        :title="`${broadcastCount} 个标签页在广播组中，广播命令会发往这 ${broadcastCount} 个会话`"
      >
        <MyIcon :size="11" name="Promotion" />
        {{ broadcastCount }}
      </span>
    </div>

    <!-- 列表 -->
    <div class="ddp-list">
      <div v-if="rows.length === 0" class="ddp-empty">{{ emptyHint || "暂无会话" }}</div>
      <template v-else>
        <MyListRow
          v-for="row in renderRows"
          :key="row.tab.id"
          class="ddp-row"
          clickable
          :selected="row.tab.id === activeTabId"
          :title="row.tab.name"
          @activate="handleRowClick(row)"
        >
          <template #lead>
            <ConnIcon class="ddp-row-icon" :type="row.tab.connType || 'ssh'" :size="15" />
          </template>

          <span class="ddp-row-name">{{ row.tab.name }}</span>
          <!-- 状态点 -->
          <MyStatusDot
            class="ddp-row-dot"
            :tone="row.dot.tone"
            :pulse="row.dot.pulse"
            :glow="row.dot.glow"
            :aria-label="row.dot.title"
          />

          <template #actions>
            <!-- 行内次操作（如广播切换）与行尾主操作（关闭/退出）。
                 两者原本各写一句 @click.stop 防冒泡到行；MyListRow 的
                 closestInteractiveControl 只认原生交互控件与 ARIA widget 角色，
                 这两个是 <span>，故按 myui 文档用 data-list-row-stop 显式豁免。 -->
            <span
              v-if="row.secondary"
              class="ddp-row-secondary"
              :class="{ 'is-on': row.secondary.active }"
              data-list-row-stop
              :title="row.secondary.title"
              @click="onRowSecondaryAction?.(row.tab)"
            >
              <MyIcon v-if="row.secondary.iconName" :size="12" :icon="DROPDOWN_ICONS[row.secondary.iconName]" />
              <template v-else>{{ row.secondary.label }}</template>
            </span>
            <span
              v-if="row.onAction && row.actionLabel"
              class="ddp-row-action"
              data-list-row-stop
              :title="row.actionTitle"
              @click="row.onAction(row.tab)"
            >
              {{ row.actionLabel }}
            </span>
          </template>
        </MyListRow>
      </template>
      </div>
    </div>
  </Teleport>
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
/* 合并「广播」面板后批量动作最多可达 4 枚（全部重连 / 全部广播 / 退出全部广播 /
   清理掉线），320px 宽放不下时换行而不是溢出裁切。 */
.ddp-header {
  padding: 11px 14px;
  border-bottom: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px 8px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
}

.ddp-header-icon {
  font-size: 14px;
}

/* 批量操作钮改用 MyButton + outline：描边、配色、hover 反色全部交给
   myui（危险那枚是 danger 变体，hover 自动实心化，与迁移前一致）。
   这里只留标题行里的外距与不换行。 */
.ddp-btn {
  margin-right: 4px;
  white-space: nowrap;
}

.ddp-count {
  font-size: 11px;
  color: var(--text-muted);
  background: var(--bg-surface);
  padding: 1px 8px;
  border-radius: var(--radius-full);
}

/* 广播计数徽标：与 .ddp-count 同一形态，但着成功色 —— 面板标题行原本没有
   任何语义色，这里用它标出"组内有 N 个会话会收到广播命令"。 */
.ddp-bcast {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 11px;
  font-weight: 600;
  color: var(--success);
  background: var(--success-muted);
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

/* 行盒（flex / gap / padding / 圆角 / cursor / 过渡 / focus 环）由 MyListRow
   提供，此处只保留两处 MyShell 的刻意偏离：
   ① 悬停底色用 --bg-surface-hover 而非库的 --bg-elevated —— 后者在暗色 Carbon
      下明显更暗，悬停反馈几乎消失；
   ② 行内操作恒显（库默认 hover / 聚焦 / 选中时才浮现），关闭/退出是常驻操作，
      不该藏起来。 */
.ddp-row:hover:not(.is-selected) {
  background: var(--bg-surface-hover);
}

.ddp-row :deep(.my-list-row__actions) {
  opacity: 1;
}

.ddp-row-icon {
  opacity: 0.85;
  flex-shrink: 0;
}

/* 活动行图标着主色（覆盖 ConnIcon 的内联类型色，需 !important） */
.ddp-row.is-selected .ddp-row-icon {
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

.ddp-row.is-selected .ddp-row-name {
  color: var(--accent-primary);
  font-weight: 600;
}

/* 状态圆点：直径与发光由 MyStatusDot 的 size / glow / pulse 承担，
   这里只把它按会话列表的密度压小一号并禁止收缩。 */
.ddp-row-dot {
  flex-shrink: 0;
  transform: scale(0.875);
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
