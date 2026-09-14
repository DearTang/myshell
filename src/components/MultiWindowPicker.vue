<!-- 多窗口会话选择器：从已连接的 SSH 终端 tab 里勾选要进入网格的会话。
     从 src-legacy/components/MultiWindowPicker.tsx 原样移植（会话列表改从 sessions store 读取）。
     超容量提示逻辑原在旧 App.tsx（mwOverflowPrompt）——现收进本组件：读取 localStorage 的
     myshell-multiwindow-cols/rows 计算容量（旧 getMultiWindowGridConfig 逻辑），超出时经
     myui confirmDialog 确认，用户确认后 emit 全部 ids。 -->
<script setup lang="ts">
import { computed, ref } from "vue";
import { Grid } from "@element-plus/icons-vue";
import { confirmDialog, MyButton, MyCheckbox } from "myui";
import type { Tab } from "@/api";
import { sessions } from "@/store/sessions";

defineOptions({ name: "MultiWindowPicker" });

const emit = defineEmits<{ close: []; confirm: [ids: string[]] }>();

/** Read multi-window grid rows/cols from localStorage (defaults: 2 rows x 3 cols). */
function getMultiWindowGridConfig(): { rows: number; cols: number } {
  const r = parseInt(localStorage.getItem("myshell-multiwindow-rows") ?? "2", 10);
  const col = parseInt(localStorage.getItem("myshell-multiwindow-cols") ?? "3", 10);
  return {
    rows: isNaN(r) || r < 1 ? 2 : Math.min(r, 6),
    cols: isNaN(col) || col < 1 ? 3 : Math.min(col, 6),
  };
}

/** Max visible windows = rows * cols. */
function getMultiWindowCapacity(): number {
  const { rows, cols } = getMultiWindowGridConfig();
  return rows * cols;
}

// Candidate sessions: only connected SSH terminal tabs.
const candidates = computed<Tab[]>(() =>
  sessions.tabs.filter(
    (t) => t.type === "terminal" && t.status === "connected" && t.connType === "ssh",
  ),
);

// Keep selection in sync if tabs change while the picker is open.
// （旧版仅在挂载时以候选集初始化一次，之后不再跟随 tabs 变化）
const selected = ref<Set<string>>(new Set(candidates.value.map((t) => t.id)));

function toggle(id: string): void {
  const next = new Set(selected.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  selected.value = next;
}

function toggleAll(): void {
  if (selected.value.size === candidates.value.length) {
    selected.value = new Set();
  } else {
    selected.value = new Set(candidates.value.map((t) => t.id));
  }
}

async function onConfirmClick(): Promise<void> {
  const ids = Array.from(selected.value);
  const cap = getMultiWindowCapacity();
  if (ids.length > cap) {
    const ok = await confirmDialog({
      title: "窗口数量超出网格容量",
      message: `当前网格最多展示 ${cap} 个窗口，你选了 ${ids.length} 个。多出的窗口会排在下方，需滚动查看。是否继续？`,
      type: "warning",
      confirmButtonText: "继续",
      cancelButtonText: "取消",
    });
    if (!ok) return;
  }
  emit("confirm", ids);
}
</script>

<template>
  <div class="overlay" @click="emit('close')">
    <div class="panel" @click.stop>
      <!-- Header -->
      <div class="header">
        <div class="header-title">
          <el-icon class="header-icon" :size="14"><Grid /></el-icon>
          多窗口会话选择
        </div>
        <div class="header-right">
          <span class="count-text">已选 {{ selected.size }} / {{ candidates.length }}</span>
          <button type="button" class="select-all-btn" @click="toggleAll">
            {{ selected.size === candidates.length ? "取消全选" : "全选" }}
          </button>
        </div>
      </div>

      <!-- List -->
      <div class="list">
        <div v-if="candidates.length === 0" class="empty-state">
          没有已连接的 SSH 终端会话。
          <br />
          请先连接至少一个会话。
        </div>
        <template v-else>
          <div
            v-for="tab in candidates"
            :key="tab.id"
            class="row"
            :class="{ 'is-checked': selected.has(tab.id) }"
            @click="toggle(tab.id)"
          >
            <MyCheckbox
              :model-value="selected.has(tab.id)"
              @update:model-value="toggle(tab.id)"
              @click.stop
            />
            <span class="row-name">{{ tab.name }}</span>
          </div>
        </template>
      </div>

      <!-- Footer -->
      <div class="footer">
        <MyButton variant="secondary" @click="emit('close')">取消</MyButton>
        <MyButton variant="primary" :disabled="selected.size === 0" @click="onConfirmClick">
          开始多窗口
        </MyButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: var(--bg-overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10000;
  backdrop-filter: blur(2px);
}

.panel {
  background: var(--bg-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-xl);
  min-width: 360px;
  max-width: 480px;
  max-height: 70vh;
  display: flex;
  flex-direction: column;
  box-shadow: var(--shadow-xl);
}

.header {
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.header-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  display: flex;
  align-items: center;
  gap: 6px;
}

.header-title .header-icon {
  color: var(--text-secondary);
}

.header-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

.count-text {
  font-size: 12px;
  color: var(--text-tertiary);
}

.select-all-btn {
  padding: 2px 10px;
  background: var(--bg-input);
  color: var(--text-secondary);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  font-size: 11px;
  cursor: pointer;
}

.select-all-btn:hover {
  background: var(--bg-surface-hover);
  border-color: var(--border-emphasis);
}

.list {
  flex: 1;
  overflow-y: auto;
  padding: 8px 0;
}

.empty-state {
  padding: 32px 20px;
  text-align: center;
  color: var(--text-tertiary);
  font-size: 13px;
  line-height: 1.6;
}

.row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 20px;
  cursor: pointer;
  transition: background var(--duration-fast) var(--ease-in-out);
}

.row:hover {
  background: var(--bg-surface-hover);
}

.row.is-checked {
  background: var(--accent-primary-muted);
}

.row-name {
  font-size: 13px;
  color: var(--text-primary);
}

.footer {
  padding: 12px 20px;
  border-top: 1px solid var(--border-subtle);
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  flex-shrink: 0;
}

</style>
