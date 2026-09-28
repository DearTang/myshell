<script lang="ts">
// 字体选择器：基于 myui 0.12.0 的 MyCombobox（el-autocomplete 封装）。
// 模块级字体缓存：设置中心与连接对话框共享一次 list_system_fonts 往返
// （与旧 FontField.tsx 的 module-scope cache 语义一致）。
import { listSystemFonts } from "@/api";

let cachedFonts: string[] | null = null;
let fetchPromise: Promise<string[]> | null = null;

function loadFonts(): Promise<string[]> {
  if (cachedFonts) return Promise.resolve(cachedFonts);
  if (!fetchPromise) {
    fetchPromise = listSystemFonts()
      .then((f) => {
        cachedFonts = f;
        return f;
      })
      .catch(() => {
        cachedFonts = [];
        return [];
      });
  }
  return fetchPromise;
}

/** 渲染上限 — 海量字体的机器上保持 DOM 轻量。 */
export const MAX_RESULTS = 200;

export interface FontItem {
  value: string;
  [key: string]: unknown;
}
</script>

<script setup lang="ts">
import { ref, watch } from "vue";
import { MyCombobox } from "myui";

defineOptions({ name: "FontField" });

const props = defineProps<{
  modelValue: string;
  placeholder?: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
}>();

const fonts = ref<string[]>(cachedFonts ?? []);
let loaded = cachedFonts !== null;

void loadFonts().then((f) => {
  fonts.value = f;
  loaded = true;
});

// 活动查询串（驱动下拉项的命中高亮），与 modelValue 分离：
// 选中字体后清空 → 重新展开时列表不带选中名的高亮（旧版语义）。
const queryText = ref("");

// 字段被外部清空时（如对话框重开）重置过滤条件。
watch(
  () => props.modelValue,
  (value) => {
    if (value === "") queryText.value = "";
  },
);

function tokenize(query: string): string[] {
  return query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);
}

/** 模糊过滤：每个空格分隔的关键字都必须出现在字体名里（不区分大小写、
 * 任意顺序）——"nerd mono" 能命中 "JetBrainsMono Nerd Font Mono"。
 * 空关键字显示完整（原始排序）列表。 */
function filterFonts(query: string): FontItem[] {
  const list = fonts.value;
  const tk = tokenize(query);
  const src =
    tk.length === 0 ? list : list.filter((f) => tk.every((t) => f.toLowerCase().includes(t)));
  return src.slice(0, MAX_RESULTS).map((f) => ({ value: f }));
}

let capVisible = ref(false);

// MyCombobox（el-autocomplete）建议源。特例：query 与当前值一致（重新聚焦
// 已选字体）时展示完整列表——旧版"重新聚焦仍显示全量"的语义。
function fetchSuggestions(query: string, cb: (items: FontItem[]) => void): void {
  const run = (): void => {
    const effective = query === props.modelValue && props.modelValue !== "" ? "" : query;
    const items = filterFonts(effective);
    capVisible.value = tokenize(effective).length > 0 && items.length >= MAX_RESULTS;
    cb(items);
  };
  if (!loaded) {
    void loadFonts().then(() => {
      loaded = true;
      run();
    });
    return;
  }
  run();
}

/** 按 token 把字体名切分为命中/未命中片段，命中的片段加粗变色以保持
 * 模糊命中可读。无内容可高亮（空查询 / 无交集）时返回整名。 */
function highlightParts(name: string): { text: string; hit: boolean }[] {
  const tk = tokenize(queryText.value);
  if (tk.length === 0) return [{ text: name, hit: false }];
  const lower = name.toLowerCase();
  const ranges: Array<[number, number]> = [];
  for (const t of tk) {
    let from = 0;
    while (from <= lower.length) {
      const idx = lower.indexOf(t, from);
      if (idx === -1) break;
      ranges.push([idx, idx + t.length]);
      from = idx + t.length;
    }
  }
  if (ranges.length === 0) return [{ text: name, hit: false }];

  ranges.sort((a, b) => a[0] - b[0]);
  const merged: Array<[number, number]> = [];
  for (const r of ranges) {
    const last = merged[merged.length - 1];
    if (last && r[0] <= last[1]) {
      last[1] = Math.max(last[1], r[1]);
    } else {
      merged.push([r[0], r[1]]);
    }
  }

  const parts: { text: string; hit: boolean }[] = [];
  let pos = 0;
  for (const r of merged) {
    if (r[0] > pos) parts.push({ text: name.slice(pos, r[0]), hit: false });
    parts.push({ text: name.slice(r[0], r[1]), hit: true });
    pos = r[1];
  }
  if (pos < name.length) parts.push({ text: name.slice(pos), hit: false });
  return parts;
}

// 键入即回传（自由文本），并记录查询串供高亮用——与旧版 onInput 一致。
function onInput(value: string): void {
  queryText.value = value;
  emit("update:modelValue", value);
}

// 选中建议项：回传字体名并清空查询串（下拉高亮复位）。
function onSelect(item: FontItem): void {
  queryText.value = "";
  emit("update:modelValue", item.value);
}

// 清除按钮：回传空串（与旧版外层清空路径一致）。
function onClear(): void {
  queryText.value = "";
  emit("update:modelValue", "");
}
</script>

<template>
  <MyCombobox
    class="font-field"
    :model-value="modelValue"
    :fetch-suggestions="fetchSuggestions"
    :placeholder="placeholder"
    :trigger-on-focus="true"
    :fit-input-width="true"
    :highlight-first-item="false"
    clearable
    bare
    @input="onInput"
    @select="onSelect"
    @clear="onClear"
  >
    <template #default="{ item }">
      <template v-for="(part, pi) in highlightParts(String(item.value))" :key="pi">
        <span v-if="part.hit" class="hit">{{ part.text }}</span>
        <template v-else>{{ part.text }}</template>
      </template>
    </template>
    <template #footer>
      <div v-if="capVisible" class="dropdown-cap">
        仅显示前 {{ MAX_RESULTS }} 条，输入更多关键字可缩小范围
      </div>
    </template>
  </MyCombobox>
</template>

<style scoped>
.font-field {
  width: 100%;
}

.font-field :deep(.el-input__inner) {
  font-family: inherit;
}

.font-field :deep(.el-autocomplete-suggestion) {
  background: var(--bg-elevated);
}

.hit {
  color: var(--text-primary);
  font-weight: 700;
}

.dropdown-cap {
  padding: 6px 10px;
  font-size: 11px;
  color: var(--text-muted);
  border-top: 1px solid var(--border-subtle);
  margin-top: 2px;
}
</style>
