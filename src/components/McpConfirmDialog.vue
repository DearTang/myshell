<!-- MCP 危险命令确认对话框（GUI 对话框形态，自包含）。
     旧版在 src-legacy/App.tsx 中用 ConfirmDialog + highlightDangerous 内联实现；
     Vue 版数据改来自 store/ui 的 ui.mcpConfirm（{ command, connectionName, reasons }），
     点击确认/取消调 resolveMcpConfirm(true/false)——store 里的 Promise resolver 把异步
     exec 流程与用户的按钮点击连接起来（语义同旧版 mcpConfirmResolver ref）。
     安全文案（标题/危害说明/拒绝后果说明）逐字保留，不得改动。

     窗口置顶（旧版行为完整移植）：对话框打开期间窗口置顶、取消最小化、聚焦并任务栏
     闪烁——用户此刻通常在另一个应用（AI agent 的编辑器/终端）里工作，后台弹窗意味着
     人工审查会无限期静默等待。解决后恢复普通（非置顶）窗口行为。

     危险片段高亮：splitDangerSegments（黑名单正则 + $()、反引号、写重定向字面模式），
     规则经 getCommandRules 拉取一次缓存；拉取失败时不高亮、仅展示原文。 -->
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { Warning } from "@element-plus/icons-vue";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { MyButton, MyDialog } from "myui";
import { resolveMcpConfirm, ui } from "@/store/ui";
import { getCommandRules } from "@/api";
import type { CommandRules } from "@/api";
import { splitDangerSegments } from "@/utils/ansi";
import type { DangerSegment } from "@/utils/ansi";

defineOptions({ name: "McpConfirmDialog" });

// 命令确认规则缓存（黑名单正则用于高亮）。挂载时拉一次；失败置 null → 不高亮仅展示原文。
const rules = ref<CommandRules | null>(null);

onMounted(() => {
  getCommandRules()
    .then((r) => (rules.value = r))
    .catch(() => (rules.value = null));
});

/** 把命令串按危险片段切分（匹配黑名单正则 + 危险字面模式）；
 *  规则不可用时退化为单段原文（danger=false，不渲染红色）。 */
const segments = computed<DangerSegment[]>(() => {
  const confirm = ui.mcpConfirm;
  if (!confirm) return [];
  if (!rules.value) return [{ text: confirm.command, danger: false }];
  return splitDangerSegments(confirm.command, rules.value.blacklist);
});

// ── 窗口置顶 / 拉回前台（旧版 showMcpConfirm / resolveMcpConfirm 的窗口操作） ──
const win = getCurrentWindow();

watch(
  () => ui.mcpConfirm,
  (cur) => {
    if (cur) {
      win.setAlwaysOnTop(true).catch(() => {});
      win.unminimize().catch(() => {});
      win.show().catch(() => {});
      win.setFocus().catch(() => {});
      win.requestUserAttention(UserAttentionType.Critical).catch(() => {});
    } else {
      win.setAlwaysOnTop(false).catch(() => {});
    }
  },
);

onUnmounted(() => {
  win.setAlwaysOnTop(false).catch(() => {});
});

function onConfirm(): void {
  resolveMcpConfirm("allow");
}

function onAllowSession(): void {
  resolveMcpConfirm("session");
}

function onCancel(): void {
  resolveMcpConfirm("deny");
}
</script>

<template>
  <MyDialog
    :model-value="ui.mcpConfirm !== null"
    title="AI 请求执行高危命令"
    :width="400"
    danger
    confirm-text="确认执行"
    cancel-text="取消"
    dismissable
    @confirm="onConfirm"
    @cancel="onCancel"
  >
    <div v-if="ui.mcpConfirm" class="mcp-body">
      <div class="intro">
        AI agent 通过 MCP 请求在服务器
        <strong class="conn-name">[{{ ui.mcpConfirm.connectionName }}]</strong>
        上执行以下命令：
      </div>
      <div class="cmd-box">
        <span
          v-for="(seg, i) in segments"
          :key="i"
          :class="{ 'seg-danger': seg.danger }"
        >{{ seg.text }}</span>
      </div>
      <div v-if="ui.mcpConfirm.reasons.length > 0" class="reasons-box">
        <div class="reasons-title">
          <el-icon class="reasons-icon" :size="12"><Warning /></el-icon>
          危害说明（命中的危险规则）
        </div>
        <ul class="reasons-list">
          <li v-for="(reason, i) in ui.mcpConfirm.reasons" :key="i">{{ reason }}</li>
        </ul>
      </div>
      <div class="note">
        点击「确认执行」仅允许本次，点击「本轮会话均允许」后本会话内所有高危命令
        将不再弹窗、直接执行；点击「取消」拒绝。取消后 AI
        会立即停止当前任务、向你说明任务进度，并等待你的指示。
      </div>
    </div>

    <!-- 自定义 footer：三按钮（取消 / 本轮会话均允许 / 确认执行）。
         MyDialog 默认 footer 仅两按钮，故整体接管。 -->
    <template #footer>
      <div class="mcp-footer">
        <MyButton variant="secondary" @click="onCancel">取消</MyButton>
        <MyButton variant="ghost-warning" @click="onAllowSession">本轮会话均允许</MyButton>
        <MyButton variant="danger" @click="onConfirm">确认执行</MyButton>
      </div>
    </template>
  </MyDialog>
</template>

<style scoped>
.mcp-body {
  font-size: 12.5px;
  color: var(--text-secondary);
  line-height: 1.7;
}

.intro {
  margin-bottom: 8px;
}

.conn-name {
  color: var(--text-primary);
}

.cmd-box {
  background: var(--bg-base);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  padding: 8px 10px;
  font-family: monospace;
  font-size: 12px;
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 120px;
  overflow: auto;
}

.seg-danger {
  color: var(--error);
  font-weight: 700;
  background: var(--error-muted);
  border-radius: 3px;
  padding: 0 2px;
}

.reasons-box {
  margin-top: 8px;
  background: var(--bg-base);
  border: 1px solid var(--error);
  border-radius: var(--radius-sm);
  padding: 8px 10px;
}

.reasons-title {
  font-size: 12px;
  font-weight: 700;
  color: var(--error);
  margin-bottom: 4px;
  display: flex;
  align-items: center;
  gap: 4px;
}

.reasons-list {
  margin: 0;
  padding-left: 18px;
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.7;
}

.note {
  margin-top: 8px;
  color: var(--text-muted);
}

/* 三按钮 footer：窄面板下「本轮会话均允许」文案较长，允许换行不挤压 */
.mcp-footer {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
</style>
