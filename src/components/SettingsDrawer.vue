<!-- 设置中心根组件 — 全屏覆盖面板：左类目导航 + 右内容区（旧 SettingsPanel 形态）。
     首次访问某类目时才挂载对应分区（visited 懒加载），挂载后保持存活（v-show），
     使各分区的编辑状态跨类目切换保留 — 与旧版把全部分区状态放在根组件一致。
     例外：McpSection 每次切入都重新拉取（旧版 useEffect 依赖 activeCategory 语义）。
     左侧类目导航用 myui MyNav（el-menu 封装），图标用注册表内置名。 -->
<script setup lang="ts">
import { computed, reactive, ref } from "vue";
import { MyIconButton, MyNav, type NavGroup } from "myui";
import AppearanceSection from "@/components/settings/AppearanceSection.vue";
import AiSection from "@/components/settings/AiSection.vue";
import McpSection from "@/components/settings/McpSection.vue";
import MultiWindowSection from "@/components/settings/MultiWindowSection.vue";
import TransferSection from "@/components/settings/TransferSection.vue";
import SecuritySection from "@/components/settings/SecuritySection.vue";
import DataSection from "@/components/settings/DataSection.vue";
import QuickCommandsSection from "@/components/settings/QuickCommandsSection.vue";

defineOptions({ name: "SettingsDrawer" });

interface Props {
  connectionCount: number;
}

defineProps<Props>();

const emit = defineEmits<{
  close: [];
  refresh: [];
  "open-quick-commands": [];
}>();

const categories = [
  { id: "appearance", label: "外观", icon: "Brush" },
  { id: "ai", label: "AI 助手", icon: "Robot" },
  { id: "mcp", label: "MCP 支持", icon: "Connection" },
  { id: "multiWindow", label: "多窗口", icon: "Grid" },
  { id: "transfer", label: "文件传输", icon: "Upload" },
  { id: "security", label: "安全", icon: "Lock" },
  { id: "data", label: "数据管理", icon: "Files" },
  { id: "quickCommands", label: "快捷命令", icon: "Lightning" },
] as const;

type CategoryId = (typeof categories)[number]["id"];

const activeCategory = ref<CategoryId>("appearance");
const visited = reactive(new Set<CategoryId>());
visited.add(activeCategory.value);

function setActive(id: CategoryId): void {
  activeCategory.value = id;
  visited.add(id);
}

// MyNav 数据：8 个类目同属一组（无分组标题，与旧版单列布局一致）。
const navGroups: NavGroup[] = [
  {
    key: "settings",
    items: categories.map((category) => ({
      key: category.id,
      label: category.label,
      icon: category.icon,
    })),
  },
];

const isCategoryId = (value: string): value is CategoryId =>
  categories.some((category) => category.id === value);

/** 内容卡标题取当前类目的 label（ompDesktop 的内容卡头就是这个语义）。 */
const activeLabel = computed(
  () => categories.find((category) => category.id === activeCategory.value)?.label ?? "设置",
);

// 类目点击切换逻辑不变（MyNav select 的 key 即类目 id）。
function onNavSelect(key: string): void {
  if (isCategoryId(key)) setActive(key);
}
</script>

<template>
  <div class="settings-overlay">
    <div class="settings-panel">
      <!-- 顶排：与外壳顶栏同高（--chrome-top-height），右侧留给窗口键 -->
      <div class="settings-topbar">
        <div class="topbar-left">
          <span class="topbar-title">设置</span>
          <span class="topbar-sub">管理应用配置与安全设置</span>
        </div>
        <MyIconButton
          title="关闭设置" class="close-btn" label="关闭设置" size="16" name="Close" @click="emit('close')" />
      </div>

      <!-- 主内容区：左分组菜单 + 右玻璃内容卡（对齐 ompDesktop settings-panel） -->
      <div class="settings-body">
        <!-- 左侧导航（MyNav：activeKey 绑当前类目，select 派发类目 id） -->
        <nav class="panel-nav" aria-label="设置分类">
          <MyNav
            :groups="navGroups"
            :active-key="activeCategory"
            aria-label="设置分类"
            @select="onNavSelect"
          />
        </nav>

        <!-- 右侧内容卡：标题在卡片头，正文限宽居中 -->
        <div class="settings-content">
          <header class="settings-content-head">
            <h2 class="settings-content-title">{{ activeLabel }}</h2>
          </header>
          <div class="settings-content-body">
            <div v-show="activeCategory === 'appearance'" class="category-pane">
              <AppearanceSection v-if="visited.has('appearance')" />
            </div>
            <div v-show="activeCategory === 'ai'" class="category-pane">
              <AiSection v-if="visited.has('ai')" />
            </div>
            <div v-show="activeCategory === 'mcp'" class="category-pane">
              <McpSection v-if="visited.has('mcp')" :active="activeCategory === 'mcp'" />
            </div>
            <div v-show="activeCategory === 'multiWindow'" class="category-pane">
              <MultiWindowSection v-if="visited.has('multiWindow')" />
            </div>
            <div v-show="activeCategory === 'transfer'" class="category-pane">
              <TransferSection v-if="visited.has('transfer')" />
            </div>
            <div v-show="activeCategory === 'security'" class="category-pane">
              <SecuritySection v-if="visited.has('security')" />
            </div>
            <div v-show="activeCategory === 'data'" class="category-pane">
              <DataSection
                v-if="visited.has('data')"
                :connection-count="connectionCount"
                @refresh="emit('refresh')"
              />
            </div>
            <div v-show="activeCategory === 'quickCommands'" class="category-pane">
              <QuickCommandsSection
                v-if="visited.has('quickCommands')"
                @open-manage="emit('open-quick-commands')"
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  backdrop-filter: blur(8px);
  z-index: 1000;
  animation: settings-fade-in var(--duration-normal) var(--ease-out-expo);
}

@keyframes settings-fade-in {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

.settings-panel {
  position: absolute;
  inset: 0;
  /* 面板底色不用 --bg-base（不透明实底会把壁纸整片盖掉，等于设置页里
     壁纸消失）。改走同族的玻璃底，实心度滑杆对它同样生效。 */
  background: var(--glass-bg);
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  overflow: hidden;
  display: flex;
  flex-direction: column;
  animation: settings-scale-in var(--duration-normal) var(--ease-out-expo);
}

@keyframes settings-scale-in {
  from {
    opacity: 0;
    transform: scale(0.98);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

/* 顶排：高度与外壳顶栏一致，右侧留出窗口控制键的宽度（无边框窗口自绘键
   在 App 层固定定位，设置页全屏覆盖时也在最上层）。 */
.settings-topbar {
  flex-shrink: 0;
  height: var(--chrome-top-height);
  box-sizing: border-box;
  padding: 0 12px 0 20px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  background: var(--surface-translucent);
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  border-bottom: 1px solid var(--border-subtle);
}

.topbar-left {
  display: flex;
  align-items: baseline;
  gap: 10px;
  min-width: 0;
}

.topbar-title {
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--text-primary);
}

.topbar-sub {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.close-btn {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: 1px solid var(--border-subtle);
  color: var(--text-secondary);
  font-size: var(--fs-lg);
  cursor: pointer;
  border-radius: var(--radius-md);
  transition: all var(--duration-fast) var(--ease-in-out);
}

.close-btn:hover {
  background: var(--bg-surface-hover);
  border-color: var(--border-default);
  color: var(--text-primary);
}

/* 主体：左菜单 + 右内容卡 */
.settings-body {
  flex: 1;
  display: flex;
  overflow: hidden;
  min-height: 0;
}

.panel-nav {
  width: 200px;
  flex-shrink: 0;
  border-right: 1px solid var(--border-subtle);
  background: var(--surface-translucent);
  padding: 12px 8px;
  overflow-y: auto;
}

/* MyNav（el-menu 封装）：透明底融入面板；条目高度对齐 ompDesktop 的 32px。 */
.panel-nav :deep(.my-nav__menu) {
  --el-menu-item-height: 32px;
  width: 100%;
  background: transparent;
  padding: 0;
}

/* 内容卡：圆角玻璃卡 + 极淡描边 + 高光内阴影（ompDesktop 的
   .settings-content 三件套）。卡片内正文限宽 940px 居中，长行不至于拉满
   整个窗口宽度。 */
.settings-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  margin: 12px;
  border: 1px solid var(--glass-border);
  border-radius: var(--radius-lg);
  background: var(--surface-translucent);
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.06), var(--shadow-sm);
  overflow: hidden;
}

.settings-content-head {
  flex-shrink: 0;
  padding: 12px 20px;
  border-bottom: 1px solid var(--border-subtle);
  background: var(--glass-bg);
}

.settings-content-title {
  margin: 0;
  font-size: var(--fs-xl);
  font-weight: 700;
  color: var(--text-primary);
}

.settings-content-body {
  flex: 1;
  overflow-y: auto;
  padding: 16px 20px 24px;
  max-width: 940px;
  width: 100%;
  margin: 0 auto;
  min-height: 0;
}

.category-pane {
  min-height: 100%;
}
</style>
