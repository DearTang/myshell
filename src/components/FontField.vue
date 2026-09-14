<script lang="ts">
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
</script>

<script setup lang="ts">
import { computed, ref, watch } from "vue";

defineOptions({ name: "FontField" });

const props = defineProps<{
  modelValue: string;
  placeholder?: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: string];
}>();

const fonts = ref<string[]>(cachedFonts ?? []);
const loaded = ref<boolean>(cachedFonts !== null);
const open = ref(false);
// `query` 驱动过滤，与 modelValue 分离 — 重新聚焦已选字体时仍显示完整列表
// 而不是过滤到当前值。选中与外部清空时都会清掉。
const query = ref("");
const highlight = ref(0);
const listRef = ref<HTMLDivElement | null>(null);

void loadFonts().then((f) => {
  fonts.value = f;
  loaded.value = true;
});

// 字段被外部清空时（如对话框重开）重置过滤条件。
watch(
  () => props.modelValue,
  (value) => {
    if (value === "") query.value = "";
  },
);

const tokens = computed(() =>
  query.value
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean),
);

// 模糊过滤：每个空格分隔的关键字都必须出现在字体名里（不区分大小写、
// 任意顺序）——"nerd mono" 能命中 "JetBrainsMono Nerd Font Mono"。
// 空关键字显示完整（已排序）列表。
const filtered = computed(() => {
  if (tokens.value.length === 0) return fonts.value.slice(0, MAX_RESULTS);
  return fonts.value
    .filter((f) => tokens.value.every((t) => f.toLowerCase().includes(t)))
    .slice(0, MAX_RESULTS);
});

// 结果集变化时高亮回到顶部。
watch(filtered, () => {
  highlight.value = 0;
});

// 键盘导航时保持高亮项可见。
watch([highlight, open], () => {
  if (!open.value || !listRef.value) return;
  const el = listRef.value.children[highlight.value] as HTMLElement | undefined;
  el?.scrollIntoView({ block: "nearest" });
});

function choose(font: string): void {
  emit("update:modelValue", font);
  query.value = "";
  open.value = false;
}

function onInput(event: Event): void {
  const v = (event.target as HTMLInputElement).value;
  emit("update:modelValue", v);
  query.value = v;
}

function onKeyDown(event: KeyboardEvent): void {
  if (event.key === "ArrowDown") {
    if (!open.value) {
      open.value = true;
      return;
    }
    event.preventDefault();
    highlight.value = Math.min(highlight.value + 1, filtered.value.length - 1);
  } else if (event.key === "ArrowUp") {
    if (!open.value) return;
    event.preventDefault();
    highlight.value = Math.max(highlight.value - 1, 0);
  } else if (event.key === "Enter") {
    if (open.value && filtered.value[highlight.value]) {
      event.preventDefault();
      choose(filtered.value[highlight.value]);
    }
  } else if (event.key === "Escape") {
    if (open.value) {
      event.preventDefault();
      open.value = false;
    }
  }
}

/**
 * 按 token 把字体名切分为命中/未命中片段，命中的片段加粗变色以保持
 * 模糊命中可读。无内容可高亮（空查询 / 无交集）时返回整名。
 */
function highlightParts(name: string): { text: string; hit: boolean }[] {
  const tk = tokens.value;
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
</script>

<template>
  <div class="font-field">
    <input
      class="font-input"
      :value="modelValue"
      :placeholder="placeholder"
      autocomplete="off"
      spellcheck="false"
      @input="onInput"
      @focus="open = true"
      @blur="open = false"
      @click="open = true"
      @keydown="onKeyDown"
    />
    <!-- 下拉箭头 — 提示这是可选择的字段，不是纯自由文本 -->
    <span aria-hidden class="caret">▾</span>

    <div v-if="open" ref="listRef" class="dropdown">
      <div v-if="!loaded" class="dropdown-hint">加载字体列表…</div>
      <div v-else-if="filtered.length === 0" class="dropdown-hint">无匹配字体，可自定义输入</div>
      <template v-else>
        <div
          v-for="(f, i) in filtered"
          :key="f"
          class="dropdown-item"
          :class="{ active: i === highlight }"
          :title="f"
          @mousedown.prevent
          @mouseenter="highlight = i"
          @click="choose(f)"
        >
          <template v-for="(part, pi) in highlightParts(f)" :key="pi">
            <span v-if="part.hit" class="hit">{{ part.text }}</span>
            <template v-else>{{ part.text }}</template>
          </template>
        </div>
        <div v-if="tokens.length > 0 && filtered.length === MAX_RESULTS" class="dropdown-cap">
          仅显示前 {{ MAX_RESULTS }} 条，输入更多关键字可缩小范围
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.font-field {
  position: relative;
}

.font-input {
  width: 100%;
  /* 右侧留出下拉箭头字形的空间 */
  padding: 10px 30px 10px 12px;
  background: var(--bg-input);
  color: var(--text-primary);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  font-size: 13px;
  outline: none;
  font-family: inherit;
  transition:
    border-color var(--duration-fast) var(--ease-in-out),
    box-shadow var(--duration-fast) var(--ease-in-out);
}

.font-input:focus {
  border-color: var(--accent-primary);
  box-shadow: 0 0 0 3px var(--accent-primary-muted);
}

.caret {
  position: absolute;
  right: 10px;
  top: 50%;
  transform: translateY(-50%);
  font-size: 11px;
  color: var(--text-tertiary);
  pointer-events: none;
}

.dropdown {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  max-height: 240px;
  overflow-y: auto;
  background: var(--bg-elevated);
  border: 1px solid var(--border-emphasis);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-xl);
  z-index: 100;
  padding: 4px;
}

.dropdown-hint {
  padding: 10px 12px;
  font-size: 12px;
  color: var(--text-muted);
}

.dropdown-item {
  padding: 7px 10px;
  font-size: 13px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  background: transparent;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.dropdown-item.active {
  background: var(--accent-primary-muted);
  color: var(--accent-primary);
  font-weight: 600;
}

.dropdown-item .hit {
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
