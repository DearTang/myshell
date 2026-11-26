<!-- 顶部栏（自包含，无 props）：品牌区 + 标签条 + 会话/广播下拉面板 +
     壳层动作按钮 + 窗口控制。全部状态经 store（sessions/ui/connections）直接读写。
     移植自旧 TabBar.tsx（功能结构）+ unified-ui-vue 模板 AppTopBar
     （毛玻璃观感）。高度填满壳层 grid 第一行 var(--ui-topbar-h)。
     v2.15.4 起窗口无边框（decorations:false），最小化/最大化/关闭按钮
     内嵌本栏最右（omp desktop 式一行顶栏），空白区域可拖动窗口。 -->
<script setup lang="ts">
import { MyIcon, MyIconButton, MyStatusDot, type MyTone } from "myui";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { ui } from "@/store/ui";
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

const props = defineProps<{
  /** 监控面板是否打开（顶栏按钮的激活态）。 */
  monitorOpen: boolean;
}>();

const emit = defineEmits<{
  "toggle-monitor": [];
}>();

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

/* canBroadcast 已随标签上的广播钮一起删除（操作走「当前会话」下拉）。 */

const broadcastCount = computed(() => sessions.broadcastIds.size);

const downTabCount = computed(
  () => sessions.tabs.filter((t) => t.status === "disconnected" || t.status === "error").length,
);

/** 标签的主连接标识：`用户名@host` 或 `用户名@host:port`。
 *
 * 取不到 config 的标签（本地终端 / 早期会话）返回空串，模板据此决定是否渲染
 * 这一截，避免出现 "undefined@undefined"。
 *
 * 端口 22（SSH 默认）省略 —— 默认端口不增加信息量，却让每个标签都长一截。
 *
 * 为什么用 `用户@host` 而不是「名称 + host」两截：多开会场景下真正区分标签的
 * 是「连哪台机器、用哪个账号」，名称可以重复（两台都叫 prod），用户@host 不会。
 * 名称移到 title 里悬停可看全，不占条上宽度。 */
function tabHost(tab: Tab): string {
  const user = tab.config?.username?.trim();
  const host = tab.config?.host?.trim();
  if (!host) return "";
  const port = tab.config?.port;
  const hostPort = port && port !== 22 ? `${host}:${port}` : host;
  return user ? `${user}@${hostPort}` : hostPort;
}

/** 标签悬停提示：名称 + 完整连接标识。
 *
 * 条上只显示 `用户@host`（省宽度），名称与端口进 title —— 悬停即可看全，
 * 这是「条上精简、悬停完整」的拆法：条上永远不因为信息多而换行或被截断，
 * 完整信息也不丢失。 */
function tabTitle(tab: Tab): string {
  const id = tabHost(tab);
  const base = id ? `${tab.name} — ${id}` : tab.name;
  const badge = taskBadge(tab);
  // 角标也进 title：条上被截断时，悬停仍能看到阶段。
  return badge ? `${base} — MCP 任务：${badge}` : base;
}

/** 该标签对应的 MCP 后台任务角标文案（无则空串）。
 *
 * 键是 connectionId（跨重连稳定），不是 tab.id —— 同一台机器的多个标签页
 * 共享同一个任务状态，任务本来就是按连接跑的。 */
function taskBadge(tab: Tab): string {
  return (tab.connectionId && sessions.taskBadges[tab.connectionId]) || "";
}

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

// ── 标签溢出检测 ──
// 标签条 overflow:hidden 等分收缩，标签数 × min-width 超过条宽时边缘标签
// 被裁 —— tabs-overflow（…）钮在此时出现，收纳被裁标签的查看/切换入口。
// tabsScrollRef 上同时挂 ResizeObserver 与 tabs 变化的 nextTick 重算：
// 窗口拖宽/拖窄、开关标签都会即时翻转溢出态（有一帧延迟，可接受）。
const tabsScrollRef = ref<HTMLDivElement | null>();
const tabsOverflow = ref(false);

function recomputeTabsOverflow(): void {
  const el = tabsScrollRef.value;
  if (!el) return;
  // 减去溢出钮自身宽度：它出现后要让出的空间不算进“是否还有标签被裁”。
  tabsOverflow.value = el.scrollWidth > el.clientWidth + 1;
}

let tabsRo: ResizeObserver | null = null;

watch(
  () => sessions.tabs.length,
  () => void nextTick(recomputeTabsOverflow),
);

onMounted(() => {
  tabsRo = new ResizeObserver(() => recomputeTabsOverflow());
  if (tabsScrollRef.value) tabsRo.observe(tabsScrollRef.value);
  recomputeTabsOverflow();
});

onUnmounted(() => {
  tabsRo?.disconnect();
  tabsRo = null;
});

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

function openQuickCommands(): void {
  ui.qcInitialConnectionId = null;
  ui.showQuickCommands = true;
}

</script>

<template>
  <!-- data-tauri-drag-region：元素自身空白处可拖动窗口（子元素不受影响）；
       标签条空白区同样可拖（Chrome/omp 式）。双击空白 = 最大化/还原。 -->
  <header class="app-topbar" data-tauri-drag-region>
    <!-- 左：标签条直接占满左侧。品牌展示（>_ 图标 + MyShell 文字）已删除
         —— 侧栏折叠后这一块会滑到窗口最左，与侧栏顶排的图标排重复占位，
         且产品名在侧栏顶排 / 欢迎页 / 关于对话框已有多处展示，顶栏这处属冗余。
         此处保留空的 .topbar-left 是为了：标签条（.tab-strip）是 flex:1 的
         中间项，删掉左容器会让 flex 分配变化，标签条将贴到窗口最左。 -->
    <div class="topbar-left" />

    <!-- 中：标签条（浏览器式等分收缩） -->
    <div class="tab-strip">
      <div ref="tabsScrollRef" class="tabs-scroll" data-tauri-drag-region>
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

          <!-- 主标识：服务器名称（用户指定：条上只要名称，完整 用户@host
               信息进 title 悬停看全）。名称可重复 —— 多开同一台机器时靠
               图标/状态点与 title 区分，这是「信息密度让位可用宽度」的取舍。 -->
          <span
            class="tab-name"
            :class="{ 'is-error': (tab.status || 'connected') === 'error', 'in-broadcast': sessions.broadcastIds.has(tab.id), 'has-task': !!taskBadge(tab) }"
            :title="tabTitle(tab)"
          >
            {{ tab.name }}
            <!-- MCP 后台任务角标：ssh_run 在该连接上跑着（阶段由 MCP 经 IPC 推送）。
                 只读展示，不抢焦点、不改终端内容 —— 让用户知道「有活儿在跑」，
                 而不冒充一个真的终端流。 -->
            <span v-if="taskBadge(tab)" class="tab-task-badge">{{ taskBadge(tab) }}</span>
          </span>

          <!-- 状态指示（始终显示） -->
          <MyStatusDot
            class="tab-status"
            :tone="statusOf(tab).tone"
            :pulse="statusOf(tab).pulse"
            :aria-label="statusOf(tab).label"
          />

          <!-- 悬停关闭（用户复调：多标签场景下逐个去下拉关闭太绕）。
               悬停/活动标签显示，平时不占视觉；广播切换仍走下拉（低频）。 -->
          <MyIconButton
            title="关闭标签"
            class="tab-close"
            label="关闭标签"
            size="12"
            name="Close"
            @click.stop="closeTab(tab.id)"
          />
        </div>

        <!-- 溢出收纳：标签条放不下（有标签被裁）时出现，点击打开同一个
             「当前会话」面板查看/切换/关闭全部标签。 -->
        <button
          v-if="tabsOverflow"
          class="tabs-overflow"
          title="更多标签页…"
          @click="toggleSessionsPanel"
        >
          …
        </button>
      </div>
    </div>

    <!-- 右：壳层动作钮。「当前会话」触发钮从标签条移到这一组的最左：
         顶栏图标按要求**靠右成一排**（ompDesktop 的 topbar-right 同款做法），
         标签条独占左侧剩余空间，标签再多也不会把动作钮挤出视口。 -->
    <div class="topbar-actions">
      <!-- 「当前会话」触发钮（常驻：标签再多也能一键看全量列表）——
           2026-10-10 起同时承载广播状态：有广播成员时整钮转强调色，
           并在计数右侧挂一枚 📡 N 徽标。广播的增删在面板内完成。 -->
      <DropdownTrigger
        ref="sessionsTriggerRef"
        label=""
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
      <!-- 会话放行指示器已挪至「快捷命令」左侧（见下），此处不再重复。 -->
      <MyIconButton
        title="多窗口"
        class="icon-btn"
        label="多窗口"
        size="16"
        name="Grid"
        @click="ui.showMultiWindowPicker = true"
      />
      <MyIconButton
        title="监控面板"
        class="icon-btn"
        label="监控面板"
        size="16"
        name="Monitor"
        :active="monitorOpen"
        @click="emit('toggle-monitor')"
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
  /* App.vue 壳层未给 .shell-topbar 设 grid-area，由本组件自己声明。
     壳层第一行是 "side topbar topbar"：左列归侧栏顶排，本栏占第 2~3 列，
     两者等高（--chrome-top-height）拼成窗口最上面连续的一整排。 */
  grid-area: topbar;
  display: flex;
  align-items: stretch;
  height: 100%;
  min-width: 0;
  overflow: hidden;
  background: var(--surface-translucent);
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  /* 与侧栏（.side）完全同配方：侧栏右缘用的是 --glass-border + --glass-highlight，
     顶栏此前用 --border-subtle 且无高光，两块拼成同一整排时右缘出现
     双重边框 + 高光台阶（截图红框处的接缝）。 */
  border-bottom: 1px solid var(--glass-border);
  box-shadow: var(--glass-highlight);
}

/* ─── 左：占位 ───
   品牌展示（>_ 图标 + MyShell 文字）已删除，此处只留一个零宽占位容器：
   标签条是 flex:1 的中间项，若把左容器整个删掉，flex 分配会让标签条贴到
   窗口最左缘。保留空容器 + flex-shrink:0 + 零内边距，视觉上与删除等价，
   布局上维持「侧栏顶排 → 标签条」的既有节奏。 */
.topbar-left {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  padding: 0;
}

/* 顶栏动作钮：外观（悬停/按下/禁用/焦点环）由 MyIconButton 承担，
   这里只固定尺寸与静止态文字色，并把悬停改成强调色系——
   库的默认悬停是中性 bg-elevated，本项目一律「悬停变主色」。
   静止态用 --text-primary 而非 --text-secondary：浅色模式下 secondary
   （oklch 0.45）压在 0.985 玻璃上约 4.3:1，图标发虚（用户实报
   「浅色模式图标对比度不够」）；primary 浅色 0.2、暗色 0.92 都清晰。 */
.icon-btn {
  width: 32px;
  height: 32px;
  flex-shrink: 0;
  color: var(--text-primary);
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
  /* 浏览器式：标签等分收缩塞满条宽，不出现横向滚动条（用户指定）。
     极端多标签时每个压到 min-width 出省略号，悬停 title 看全。 */
  overflow: hidden;
}
.tab-item {
  position: relative;
  height: 100%;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 8px 0 14px;
  font-size: var(--fs-xs);
  cursor: pointer;
  border-right: 1px solid var(--border-subtle);
  background: transparent;
  color: var(--text-primary);
  /* 对比度：非活动标签原用 --text-secondary（暗色 oklch 0.68），压在玻璃
     顶栏上发虚（用户实报「看不清/颜色不对」）。全亮则活动/非活动无层次
     —— 折中：primary 基色 + 非活动态 0.72 不透明度，活动态回到 1。 */
  opacity: 0.72;
  white-space: nowrap;
  /* 浏览器式等分收缩（用户指定，替代 2026-11-12 的「不收缩+滚动」方案）：
     flex:1 1 0 让所有标签均分条宽；max-width 15em 保证标签少时不单个
     占满整条；min-width 7em 保证「图标 + 4-5 个字 + 状态点」可读
     （2026-11-13 用户实拍 20 标签全挤成 "ia..."）—— 再多放不下就裁切，
     由条尾 … 溢出钮收纳。 */
  flex: 1 1 0;
  max-width: 15em;
  min-width: 7em;
  transition:
    background var(--duration-fast) var(--ease-in-out),
    opacity var(--duration-fast) var(--ease-in-out);
}

.tab-item:hover:not(.is-active) {
  background: var(--bg-surface-hover);
}

.tab-item.is-active {
  background: var(--bg-base);
  color: var(--text-primary);
  opacity: 1;
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

/* 主标识：服务器名称（UI 字体 —— 内容是名称不是地址，等宽字体那层
   「连接标识」的视觉区分随 用户@host 一起移除了）。tab 内只有文本随
   宽窄伸缩（图标/状态点/关闭钮定宽），min-width:1.5em 保证极端多标签
   时至少露出 1-2 个字符 + 省略号，完整信息悬停 title 看。 */
.tab-name {
  flex: 1 1 auto;
  min-width: 1.5em;
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

/* MCP 后台任务角标：紧贴名称的小胶囊。用 --info 色系与「错误/广播」区分开，
   且不加动画 —— 阶段变化本身已经是信息，呼吸动画会让整条标签条显得忙。 */
.tab-task-badge {
  display: inline-block;
  margin-left: 0.4em;
  padding: 0 0.4em;
  border-radius: var(--radius-sm, 4px);
  background: color-mix(in srgb, var(--info, #3b82f6) 22%, transparent);
  color: var(--info, #3b82f6);
  font-size: 0.85em;
  line-height: 1.4;
  vertical-align: baseline;
  white-space: nowrap;
}

.tab-name.has-task {
  /* 有任务时允许名称让出空间给角标，但保持省略号行为 */
  min-width: 3em;
}

/* 状态点：尺寸与呼吸动画由 MyStatusDot 承担，这里只收紧密度。 */
.tab-status {
  flex-shrink: 0;
  transform: scale(0.9);
}

/* 悬停关闭钮（2026-11-13 复调恢复：多标签下逐个去下拉关太绕）。Chrome 式
   —— 悬停/活动标签显示，平时透明不占视觉。
   **绝对定位**是关键（2026-11-13 用户实拍：流内布局时 MyIconButton 默认
   36px 宽放不进 84px 标签，× 溢出画到右侧相邻标签上 —— 悬停 tab1 的 ×
   出现在 tab2、滑过去又串到 tab3，“套娃”且点不中）。定位在标签右侧内
   缘，盖住状态点（悬停时状态点让位），永不跳出本标签范围。 */
.tab-close {
  position: absolute;
  right: 4px;
  top: 50%;
  transform: translateY(-50%);
  width: 18px !important;
  height: 18px !important;
  min-width: 0 !important;
  padding: 0 !important;
  opacity: 0;
  transition: opacity var(--duration-fast) var(--ease-in-out);
}

.tab-item:hover .tab-close,
.tab-item.is-active .tab-close {
  opacity: 1;
}

/* 悬停时状态点让位给 ×：两者都靠右，避免叠在一起 */
.tab-item:hover .tab-status,
.tab-item.is-active .tab-status {
  opacity: 0;
}

.tab-close:hover {
  opacity: 1;
  background: var(--error-muted);
  color: var(--error);
}

/* 溢出收纳钮（…）：标签条有标签被裁时出现，点击打开「当前会话」面板。
   定宽不参与等分；与标签同密度。 */
.tabs-overflow {
  flex: 0 0 auto;
  height: 100%;
  padding: 0 10px;
  border: none;
  border-left: 1px solid var(--border-subtle);
  background: transparent;
  color: var(--text-primary);
  font-size: var(--fs-sm);
  cursor: pointer;
}

.tabs-overflow:hover {
  background: var(--bg-surface-hover);
  color: var(--accent-primary);
}

/* ─── DropdownTrigger（本组件的 <button class="el-button dropdown-trigger">
      继承本组件作用域 id；内部由 SessionDropdownPanel 渲染的元素不带本组件
      作用域 id，需 :deep 命中） ─── */
.dropdown-trigger {
  height: 100%;
  padding: 0 14px;
  border: none;
  border-left: 1px solid var(--border-default);
  border-radius: 0;
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--fs-2xs);
  font-weight: 600;
  white-space: nowrap;
  flex-shrink: 0;
  box-shadow: none;
  transition:
    background var(--duration-fast) var(--ease-in-out),
    color var(--duration-fast) var(--ease-in-out);
}

/* flex 布局落在 SessionDropdownPanel 自包的 .trigger-row 上（原因见该组件注释）。
   此前这里写的是 `:deep(.el-button__content)`，而该类名在 EP 2.14.5 里不存在，
   规则从未命中。:deep 现在只用于命中**我们自己的**类，不穿透 EP 内部结构。 */
.dropdown-trigger :deep(.trigger-row) {
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
  font-size: var(--fs-md);
}

/* 两枚计数徽标形态必须一致：都是「图标 + 数字」的实心/淡底胶囊、同样的
   padding / line-height / gap。2026-10-10 之前会话那枚只有数字、广播那枚带 📡，
   并排时一胖一瘦读起来对不上号。 */
.dropdown-trigger :deep(.trigger-count) {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 3px;
  font-size: var(--fs-2xs);
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
  font-size: var(--fs-2xs);
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

/* 窗口控制样式在 WindowControls.vue（顶栏与门禁页共用）。 */

</style>
