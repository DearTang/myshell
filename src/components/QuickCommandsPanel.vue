<!-- 快捷命令管理面板：全局 / 按服务器两个作用域的命令片段 CRUD + 排序。
     从 src-legacy/components/QuickCommandsPanel.tsx 原样移植（注释与文案逐字保留）。
     连接列表从 connectionsStore 读取（旧版由 props 传入）。
     旧版的 window.confirm / alert 按迁移约定替换为 myui confirmDialog / toast（文案不变）。
     壳：MyDialog（showClose 保留旧版右上角 ✕，遮罩/Esc 关闭旧版没有 → 维持禁用；
     hide-footer —— 关闭动作只有右上角 ✕，与旧版一致）。
     作用域下拉：MySelect 的 groups prop（v0.10.0）渲染「当前服务器/所有服务器」两个分组；
     「全局命令」顶项经默认插槽 el-option 保留（groups 渲染后仍渲染默认插槽，顺序不变）。 -->
<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { Delete, EditPen } from "@element-plus/icons-vue";
import { MyButton, MyDialog, MySelect, confirmDialog, toast, type SelectGroup } from "myui";
import {
  listQuickCommands,
  addQuickCommand,
  updateQuickCommand,
  updateQuickCommandOrder,
  deleteQuickCommand,
} from "@/api";
import type { QuickCommandItem } from "@/api";
import { connectionsStore } from "@/store/connections";

defineOptions({ name: "QuickCommandsPanel" });

const props = defineProps<{
  /** Preset scope on open: null = global, a connection id = that server. */
  initialConnectionId: string | null;
  /** The currently-active terminal tab's connection (highlighted in the
   * scope dropdown). May differ from initialConnectionId. */
  activeConnectionId: string | null;
}>();

const emit = defineEmits<{ close: [] }>();

const GLOBAL_VALUE = "__global__";

function scopeToValue(scope: string | null): string {
  return scope ?? GLOBAL_VALUE;
}

function valueToScope(value: string): string | null {
  return value === GLOBAL_VALUE ? null : value;
}

const scope = ref<string | null>(props.initialConnectionId ?? null);
const items = ref<QuickCommandItem[]>([]);
const loading = ref(false);
// Editor state: null = list mode; object = add/edit form open.
const editing = ref<{ id: number | null; label: string; command: string } | null>(null);
const saving = ref(false);
const helpOpen = ref(false);

async function reload(): Promise<void> {
  loading.value = true;
  try {
    const list = await listQuickCommands(scope.value);
    items.value = list;
  } catch {
    // Silently ignore — the list just stays stale.
  } finally {
    loading.value = false;
  }
}

watch(scope, () => {
  void reload();
});

onMounted(() => {
  void reload();
});

// If the panel is scoped to a connection that no longer exists (deleted),
// fall back to global so the user isn't stuck on an empty stale scope.
watch(
  [() => connectionsStore.connections, scope],
  ([conns, sc]) => {
    if (sc && !conns.some((c) => c.id === sc)) {
      scope.value = null;
    }
  },
  // 旧版 useEffect 挂载时也执行一次（打开时即校验初始作用域）
  { immediate: true },
);

function handleScopeChange(value: unknown): void {
  // MySelect 的 update:modelValue 载荷类型是 unknown；本下拉只有字符串值
  scope.value = valueToScope(value as string);
  editing.value = null;
}

function startAdd(): void {
  helpOpen.value = false; // 旧版编辑器子组件卸载重建，帮助态随之复位
  editing.value = { id: null, label: "", command: "" };
}

function startEdit(item: QuickCommandItem): void {
  helpOpen.value = false;
  editing.value = { id: item.id, label: item.label, command: item.command };
}

async function handleSave(): Promise<void> {
  if (!editing.value) return;
  const label = editing.value.label.trim();
  const command = editing.value.command.trim();
  if (!label || !command) return;
  saving.value = true;
  try {
    if (editing.value.id === null) {
      await addQuickCommand(scope.value, label, command);
    } else {
      await updateQuickCommand(editing.value.id, label, command);
    }
    editing.value = null;
    await reload();
  } catch (e) {
    toast(`保存失败: ${String(e)}`, { type: "error" });
  } finally {
    saving.value = false;
  }
}

async function handleDelete(id: number): Promise<void> {
  const ok = await confirmDialog({
    message: "删除该快捷命令？",
    title: "删除快捷命令",
    type: "warning",
  });
  if (!ok) return;
  // Guarded, unlike the sibling `handleSave`. Unguarded, a failure left the
  // row on screen with no explanation (reload never ran) and surfaced as an
  // unhandled rejection — the user is then likely to click 删除 again.
  try {
    await deleteQuickCommand(id);
    await reload();
  } catch (e) {
    toast(`删除失败: ${String(e)}`, { type: "error" });
    await reload();
  }
}

/** Swap sort_order with the adjacent item (items are pre-sorted by it). */
async function handleMove(index: number, direction: -1 | 1): Promise<void> {
  const current = items.value[index];
  const target = items.value[index + direction];
  if (!current || !target) return;
  // Two independent UPDATEs: if the second fails, both rows keep the same
  // sort_order and `list_quick_commands` then falls back to `id ASC` as the
  // tiebreaker — permanently scrambled, and a later move cannot repair it.
  // Always reload so the UI reflects the real persisted order either way.
  try {
    await updateQuickCommandOrder(current.id, target.sortOrder);
    await updateQuickCommandOrder(target.id, current.sortOrder);
  } catch (e) {
    toast(`调整顺序失败: ${String(e)}`, { type: "error" });
  } finally {
    await reload();
  }
}

const scopeName = (): string =>
  scope.value === null
    ? "全局命令"
    : connectionsStore.connections.find((c) => c.id === scope.value)?.name ?? "未知服务器";

// 作用域分组（MySelect groups prop）。「当前服务器」分组仅在有活动连接时出现
// （与旧版 el-option-group 的 v-if 一致）；「全局命令」顶项走模板默认插槽。
const scopeGroups = computed<SelectGroup[]>(() => {
  const groups: SelectGroup[] = [];
  const active = props.activeConnectionId
    ? connectionsStore.connections.find((c) => c.id === props.activeConnectionId)
    : undefined;
  if (active) {
    groups.push({
      label: "当前服务器",
      options: [{ value: active.id, label: `${active.name} (${active.host})` }],
    });
  }
  groups.push({
    label: "所有服务器",
    options: connectionsStore.connections.map((c) => ({
      value: c.id,
      label: `${c.name} (${c.host})`,
    })),
  });
  return groups;
});

const canSave = computed(
  () =>
    (editing.value?.label.trim().length ?? 0) > 0 &&
    (editing.value?.command.trim().length ?? 0) > 0 &&
    !saving.value,
);

function lineCount(command: string): number {
  return command.split(/\r?\n/).filter((l) => l.trim()).length;
}
</script>

<template>
  <MyDialog
    :model-value="true"
    title="快捷命令管理"
    size="lg"
    align-center
    show-close
    hide-footer
    class="qc-dialog"
    @cancel="emit('close')"
  >
    <template #header>
      <div class="dlg-header">
        <div>
          <div class="header-title">快捷命令管理</div>
          <div class="header-sub">定义可复用的命令片段，支持多行按顺序执行</div>
        </div>
      </div>
    </template>

    <!-- Scope selector -->
    <div class="scope-row">
      <span class="scope-label">作用域:</span>
      <MySelect
        :model-value="scopeToValue(scope)"
        class="scope-select"
        :groups="scopeGroups"
        @update:model-value="handleScopeChange"
      >
        <el-option :value="GLOBAL_VALUE" label="全局命令（所有服务器可用）" />
      </MySelect>
    </div>

      <!-- Body: list or editor -->
      <div class="body">
        <template v-if="editing">
          <!-- ── Editor form（旧版 EditorForm 子组件内联） ── -->
          <div class="editor">
            <div class="editor-title">
              {{ editing.id === null ? "新增快捷命令" : "编辑快捷命令" }}
              <button
                type="button"
                class="help-btn"
                :class="{ 'is-open': helpOpen }"
                title="命令编写规则"
                @click="helpOpen = !helpOpen"
              >
                ?
              </button>
            </div>
            <div v-if="helpOpen" class="help-box">
              <div>• 每行一条命令，按顺序逐行发送到终端</div>
              <div>
                • 以 <code class="help-code">#</code> 开头的行和空行会被跳过（注释）
              </div>
              <div>
                • <code class="help-code">##delay:500</code> — 两行之间等待 500 毫秒（如等待密码提示）
              </div>
              <div>
                • <code class="help-code">##delay:1s</code> / <code class="help-code">##delay:0.5s</code> — 也支持秒
              </div>
              <div>• 「设置 → 快捷命令」开启「智能等待」可自动等上一行输出静止后再发下一行（推荐）</div>
            </div>
            <div class="editor-field">
              <label class="editor-label">
                名称<span class="required-star">*</span>
              </label>
              <input
                v-model="editing.label"
                class="editor-input"
                :class="{ 'field-error': !editing.label.trim() }"
                placeholder="如：重启 nginx"
                autofocus
              />
            </div>
            <div class="editor-field">
              <label class="editor-label">
                命令
                <span class="required-star">*</span>
                <span class="label-hint">每行一条，点标题旁 ? 查看写法</span>
              </label>
              <textarea
                v-model="editing.command"
                class="editor-textarea"
                :class="{ 'field-error': !editing.command.trim() }"
                :placeholder="'sudo systemctl restart nginx\n# 清理 7 天前的日志\nfind /var/log -mtime +7 -delete\n\n# 需要等待交互提示时，用 ##delay:N 插入延迟：\n# mysql -u root -p\n# ##delay:800\n# mypassword'"
                rows="8"
              ></textarea>
            </div>
            <div class="editor-actions">
              <MyButton variant="secondary" @click="editing = null">取消</MyButton>
              <MyButton
                variant="primary"
                :disabled="!canSave"
                :title="canSave ? undefined : '请填写名称和命令（标 * 的必填项）'"
                @click="handleSave"
              >
                {{ saving ? "保存中..." : "保存" }}
              </MyButton>
            </div>
          </div>
        </template>
        <template v-else>
          <button type="button" class="add-btn" @click="startAdd">
            + 新增快捷命令
          </button>

          <div v-if="loading" class="body-hint">加载中...</div>
          <div v-else-if="items.length === 0" class="body-hint empty">
            「{{ scopeName() }}」暂无快捷命令
          </div>
          <div v-else class="command-list">
            <div v-for="(item, index) in items" :key="item.id" class="command-row">
              <div class="command-head">
                <span class="command-label" :title="item.label">{{ item.label }}</span>
                <span class="command-lines">{{ lineCount(item.command) }} 行</span>
                <button
                  type="button"
                  class="row-btn"
                  title="上移"
                  :disabled="!(index > 0)"
                  @click="handleMove(index, -1)"
                >
                  ↑
                </button>
                <button
                  type="button"
                  class="row-btn"
                  title="下移"
                  :disabled="!(index < items.length - 1)"
                  @click="handleMove(index, 1)"
                >
                  ↓
                </button>
                <button type="button" class="row-btn" title="编辑" @click="startEdit(item)">
                  <el-icon :size="14"><EditPen /></el-icon>
                </button>
                <button type="button" class="row-btn danger" title="删除" @click="handleDelete(item.id)">
                  <el-icon :size="14"><Delete /></el-icon>
                </button>
              </div>
              <pre class="command-text">{{ item.command }}</pre>
            </div>
          </div>
        </template>
      </div>
  </MyDialog>
</template>

<style scoped>
/* ── MyDialog 壳适配（class 经 attrs 透传到 el-dialog 根元素，故用 :global） ──
   旧版 .body 是滚动区域（面板 max-height 88vh）；等价改为弹窗限高 + body 滚动。 */
:global(.qc-dialog.el-dialog) {
  display: flex;
  flex-direction: column;
  max-height: 88vh;
}

:global(.qc-dialog .el-dialog__body) {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

.dlg-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--border-default);
  /* 左右负边距横贯弹窗全宽（抵消 .el-dialog 根元素 16px 内边距），内容保持 20px 缩进 */
  margin: 0 calc(var(--el-dialog-padding-primary) * -1);
  padding: 0 20px;
}

.header-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.header-sub {
  font-size: 12px;
  color: var(--text-muted);
  margin-top: 2px;
}

.scope-row {
  padding: 14px 20px;
  border-bottom: 1px solid var(--border-default);
  display: flex;
  align-items: center;
  gap: 10px;
}

.scope-label {
  font-size: 12px;
  color: var(--text-secondary);
}

.scope-select {
  flex: 1;
}

.body {
  flex: 1;
  overflow-y: auto;
  padding: 12px 20px 20px;
}

.body-hint {
  padding: 24px;
  text-align: center;
  color: var(--text-muted);
  font-size: 13px;
}

.body-hint.empty {
  padding: 32px;
}

.add-btn {
  width: 100%;
  padding: 10px;
  background: var(--accent-primary-muted);
  color: var(--accent-primary);
  border: 1px dashed var(--border-accent);
  border-radius: var(--radius-md);
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  margin-bottom: 12px;
}

.command-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.command-row {
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 10px 12px;
}

.command-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}

.command-label {
  flex: 1;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.command-lines {
  font-size: 11px;
  color: var(--text-muted);
}

.row-btn {
  background: transparent;
  border: none;
  color: var(--text-muted);
  font-size: 14px;
  cursor: pointer;
  opacity: 0.8;
  padding: 2px 4px;
}

.row-btn:hover:not(:disabled) {
  opacity: 1;
}

.row-btn:disabled {
  cursor: not-allowed;
  opacity: 0.3;
}

.row-btn.danger {
  color: var(--error);
}

.command-text {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary);
  font-family: "Cascadia Code", "Fira Code", "JetBrains Mono", monospace;
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 120px;
  overflow-y: auto;
}

/* ── 编辑器表单 ── */
.editor {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.editor-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  display: flex;
  align-items: center;
  gap: 8px;
}

.help-btn {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  border: 1px solid var(--border-default);
  background: var(--bg-input);
  color: var(--text-muted);
  font-size: 11px;
  font-weight: 700;
  line-height: 1;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  flex-shrink: 0;
}

.help-btn.is-open {
  background: var(--accent-primary);
  color: var(--text-inverse);
}

.help-box {
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 10px 12px;
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.7;
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.help-code {
  background: var(--bg-surface);
  padding: 1px 5px;
  border-radius: var(--radius-sm);
  font-family: "Cascadia Code", "Fira Code", monospace;
  font-size: 12px;
  color: var(--text-primary);
}

.editor-field {
  display: flex;
  flex-direction: column;
}

.editor-label {
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 4px;
}

.required-star {
  color: var(--error);
  margin-left: 3px;
  font-weight: 700;
}

.label-hint {
  color: var(--text-muted);
  font-weight: 400;
  margin-left: 6px;
  font-size: 11px;
}

.editor-input,
.editor-textarea {
  width: 100%;
  background: var(--bg-input);
  color: var(--text-primary);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 8px 10px;
  font-size: 13px;
  outline: none;
}

/* 旧版 onFocus 逻辑：仅在有内容时高亮（等价的纯 CSS 写法） */
.editor-input:not(:placeholder-shown):focus,
.editor-textarea:not(:placeholder-shown):focus {
  border-color: var(--accent-primary);
  box-shadow: 0 0 0 3px var(--accent-primary-muted);
}

/* 旧版 global.css 的 field-error：必填项为空时红框，修正后消失 */
.field-error {
  border-color: var(--error) !important;
  box-shadow: 0 0 0 3px var(--error-muted) !important;
}

.editor-textarea {
  font-family: "Cascadia Code", "Fira Code", "JetBrains Mono", monospace;
  resize: vertical;
  line-height: 1.5;
}

.editor-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
