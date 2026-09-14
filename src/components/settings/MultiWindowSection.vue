<!-- 多窗口 — 网格行列数设置（旧 multiWindow 分区） -->
<script setup lang="ts">
import { ref } from "vue";
import { MySection, MySelect, type SelectOption } from "myui";

defineOptions({ name: "MultiWindowSection" });

const rows = ref(localStorage.getItem("myshell-multiwindow-rows") ?? "2");
const cols = ref(localStorage.getItem("myshell-multiwindow-cols") ?? "3");

const ROW_OPTIONS: SelectOption[] = [
  { label: "1 行", value: "1" },
  { label: "2 行（默认）", value: "2" },
  { label: "3 行", value: "3" },
  { label: "4 行", value: "4" },
];

const COL_OPTIONS: SelectOption[] = [
  { label: "1 列", value: "1" },
  { label: "2 列", value: "2" },
  { label: "3 列（默认）", value: "3" },
  { label: "4 列", value: "4" },
  { label: "5 列", value: "5" },
  { label: "6 列", value: "6" },
];

function onRowsChange(v: unknown): void {
  rows.value = String(v);
  localStorage.setItem("myshell-multiwindow-rows", rows.value);
  window.dispatchEvent(new Event("myshell-multiwindow-changed"));
}

function onColsChange(v: unknown): void {
  cols.value = String(v);
  localStorage.setItem("myshell-multiwindow-cols", cols.value);
  window.dispatchEvent(new Event("myshell-multiwindow-changed"));
}
</script>

<template>
  <MySection
    title="多窗口网格"
    description="设置多窗口模式的默认网格行列数。进入多窗口时自动按此布局排列，并最大化窗口。"
  >
    <div class="grid-prefs">
      <div class="field">
        <label class="field-label">行数</label>
        <MySelect
          :model-value="rows"
          :options="ROW_OPTIONS"
          class="select-narrow"
          @update:model-value="onRowsChange"
        />
      </div>
      <div class="field">
        <label class="field-label">列数</label>
        <MySelect
          :model-value="cols"
          :options="COL_OPTIONS"
          class="select-narrow"
          @update:model-value="onColsChange"
        />
      </div>
    </div>
    <div class="grid-hint">
      默认 2 行 × 3 列，最多同时展示 6 个窗口。超出部分可向下滚动查看。修改后下次进入多窗口生效。
    </div>
  </MySection>
</template>

<style scoped>
.grid-prefs {
  display: flex;
  gap: 16px;
  align-items: flex-end;
}

.field {
  margin-bottom: 0;
}

.field-label {
  display: block;
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 6px;
  font-weight: 500;
}

.select-narrow {
  max-width: 240px;
}

.grid-hint {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-top: 10px;
  line-height: 1.5;
}
</style>
