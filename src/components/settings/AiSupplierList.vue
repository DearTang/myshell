<!-- AI 供应商左列表 — 选择 / 双击重命名 / 启用开关（从旧 AiSection 拆出）。 -->
<script setup lang="ts">
import { ref } from "vue";
import { MyButton, MyToggle } from "myui";
import type { AiModel } from "@/api";

defineOptions({ name: "AiSupplierList" });

defineProps<{
  suppliers: AiModel[];
  selectedId: number | null;
  loading: boolean;
}>();

const emit = defineEmits<{
  select: [id: number];
  create: [];
  toggle: [m: AiModel];
  rename: [m: AiModel, name: string];
}>();

// 供应商名双击进入内联重命名。
const renamingId = ref<number | null>(null);
const renameBuf = ref("");

function startRename(m: AiModel): void {
  renamingId.value = m.id;
  renameBuf.value = m.name;
}

async function onRenameBlur(m: AiModel): Promise<void> {
  if (renameBuf.value.trim() && renameBuf.value !== m.name) {
    emit("rename", m, renameBuf.value.trim());
  }
  renamingId.value = null;
}

function onRenameKeydown(e: KeyboardEvent): void {
  if (e.key === "Enter") (e.target as HTMLInputElement).blur();
  if (e.key === "Escape") renamingId.value = null;
}
</script>

<template>
  <div class="supplier-col">
    <MyButton variant="primary" class="new-supplier-btn" @click="emit('create')">
      <span class="plus">＋</span> 新建供应商
    </MyButton>
    <div class="supplier-list">
      <div
        v-for="m in suppliers"
        :key="m.id"
        class="supplier-item"
        :class="{ selected: selectedId === m.id }"
        @click="emit('select', m.id)"
        @dblclick.stop="startRename(m)"
      >
        <!-- 行 1：名称 + 启用/禁用开关 -->
        <div class="supplier-row">
          <span class="supplier-name" :class="{ selected: selectedId === m.id }">
            <input
              v-if="renamingId === m.id"
              v-model="renameBuf"
              class="rename-input"
              autofocus
              @click.stop
              @blur="onRenameBlur(m)"
              @keydown="onRenameKeydown"
            />
            <template v-else>
              <span>{{ m.name }}</span>
              <span v-if="!m.isEnabled" class="disabled-badge">已禁用</span>
            </template>
          </span>
          <span class="supplier-toggle" @click.stop>
            <MyToggle
              :model-value="m.isEnabled"
              size="small"
              :title="m.isEnabled ? '点击禁用' : '点击启用'"
              role="switch"
              @update:model-value="emit('toggle', m)"
            />
          </span>
        </div>
      </div>
      <div v-if="suppliers.length === 0 && !loading" class="supplier-empty">
        暂无自定义供应商，点击上方按钮新建
      </div>
    </div>
  </div>
</template>

<style scoped>
.supplier-col {
  width: 240px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
}

.new-supplier-btn {
  width: 100%;
  margin-bottom: 8px;
}

.plus {
  font-size: 14px;
}

.supplier-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.supplier-item {
  padding: 8px 10px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border-default);
  background: var(--bg-surface);
  cursor: pointer;
}

.supplier-item.selected {
  border-color: var(--accent-primary);
  background: var(--accent-primary-muted);
}

.supplier-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.supplier-name {
  flex: 1;
  font-size: 12px;
  font-weight: 400;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.supplier-name.selected {
  font-weight: 600;
  color: var(--accent-primary);
}

.disabled-badge {
  font-size: 9px;
  margin-left: 4px;
  color: var(--text-muted);
}

.supplier-toggle {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
}

.rename-input {
  width: 100%;
  padding: 2px 4px;
  font-size: 12px;
  background: var(--bg-input);
  color: var(--text-primary);
  border: 1px solid var(--accent-primary);
  border-radius: var(--radius-sm);
  outline: none;
}

.supplier-empty {
  font-size: 11px;
  color: var(--text-muted);
  text-align: center;
  padding: 20px;
}
</style>
