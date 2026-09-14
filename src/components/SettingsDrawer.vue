<!-- 设置中心根组件 — 全屏覆盖面板：左类目导航 + 右内容区（旧 SettingsPanel 形态）。
     首次访问某类目时才挂载对应分区（visited 懒加载），挂载后保持存活（v-show），
     使各分区的编辑状态跨类目切换保留 — 与旧版把全部分区状态放在根组件一致。
     例外：McpSection 每次切入都重新拉取（旧版 useEffect 依赖 activeCategory 语义）。
     左侧类目导航用 myui MyNav（el-menu 封装），图标用注册表内置名。 -->
<script setup lang="ts">
import { reactive, ref } from "vue";
import { MyNav, type NavGroup } from "myui";
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
  { id: "ai", label: "AI 助手", icon: "ChatDotRound" },
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

// 类目点击切换逻辑不变（MyNav select 的 key 即类目 id）。
function onNavSelect(key: string): void {
  if (isCategoryId(key)) setActive(key);
}
</script>

<template>
  <div class="settings-overlay">
    <div class="settings-panel">
      <!-- 头部 -->
      <div class="panel-header">
        <div>
          <div class="panel-title">设置</div>
          <div class="panel-subtitle">管理应用配置与安全设置</div>
        </div>
        <button class="close-btn" type="button" @click="emit('close')">✕</button>
      </div>

      <!-- 主内容区 -->
      <div class="panel-body">
        <!-- 左侧导航（MyNav：activeKey 绑当前类目，select 派发类目 id） -->
        <nav class="panel-nav">
          <MyNav
            :groups="navGroups"
            :active-key="activeCategory"
            aria-label="设置分类"
            @select="onNavSelect"
          />
        </nav>

        <!-- 右侧内容 -->
        <div class="panel-content">
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
</template>

<style scoped>
.settings-overlay {
  position: fixed;
  inset: 0;
  background: var(--bg-overlay);
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
  background: var(--bg-elevated);
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

.panel-header {
  padding: 20px 24px;
  border-bottom: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
}

.panel-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.panel-subtitle {
  font-size: 12px;
  color: var(--text-tertiary);
  margin-top: 2px;
}

.close-btn {
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  color: var(--text-tertiary);
  font-size: 18px;
  cursor: pointer;
  border-radius: var(--radius-md);
  transition: all var(--duration-fast) var(--ease-in-out);
}

.close-btn:hover {
  background: var(--bg-surface-hover);
  color: var(--text-primary);
}

.panel-body {
  flex: 1;
  display: flex;
  overflow: hidden;
  min-height: 0;
}

.panel-nav {
  width: 200px;
  border-right: 1px solid var(--border-subtle);
  background: var(--bg-surface);
  padding: 16px 0;
  overflow-y: auto;
  flex-shrink: 0;
}

/* MyNav（el-menu 封装）：透明底融入面板；条目高度对齐旧版按钮观感。 */
.panel-nav :deep(.my-nav__menu) {
  --el-menu-item-height: 38px;
  width: 100%;
  background: transparent;
  padding: 0;
}

.panel-content {
  flex: 1;
  overflow-y: auto;
  padding: 20px 24px;
  min-width: 0;
}

.category-pane {
  min-height: 480px;
}
</style>
