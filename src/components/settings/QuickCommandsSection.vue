<!-- 快捷命令 — 管理入口 + 行间延迟（旧 quickCommands 分区）。
     行间延迟控制多行快捷命令逐行发送的间隔，避免下一条命令在交互提示
     （sudo/mysql 密码）就绪前被送出。三档模式 + ##delay:N 行内指令。 -->
<script setup lang="ts">
import { ref } from "vue";
import { MyButton, MySection, MySelect, type SelectOption } from "myui";

defineOptions({ name: "QuickCommandsSection" });

const emit = defineEmits<{
  "open-manage": [];
}>();

const MODE_KEY = "myshell-quick-command-mode";
const MS_KEY = "myshell-quick-command-line-delay-ms";

// 生效模式：显式 key 优先，否则从旧的 delay-ms 值迁移（>0 ⇒ fixed）。
function readMode(): string {
  return (
    localStorage.getItem(MODE_KEY) ||
    (Number(localStorage.getItem(MS_KEY)) > 0 ? "fixed" : "off")
  );
}

const qcMode = ref(readMode());
const qcMs = ref(localStorage.getItem(MS_KEY) || "300");

const MODE_OPTIONS: SelectOption[] = [
  { label: "关闭（一次性发出）", value: "off" },
  { label: "固定延迟", value: "fixed" },
  { label: "智能等待（推荐）", value: "idle" },
];

const MS_OPTIONS: SelectOption[] = [
  { label: "100 毫秒", value: "100" },
  { label: "300 毫秒", value: "300" },
  { label: "500 毫秒（推荐）", value: "500" },
  { label: "1 秒", value: "1000" },
  { label: "2 秒", value: "2000" },
];

function onModeChange(v: unknown): void {
  qcMode.value = String(v);
  localStorage.setItem(MODE_KEY, qcMode.value);
}

function onMsChange(v: unknown): void {
  qcMs.value = String(v);
  localStorage.setItem(MS_KEY, qcMs.value);
}
</script>

<template>
  <MySection title="快捷命令">
    <div class="manage-row">
      <div class="manage-text">
        <div class="manage-title">全局与服务器专属快捷命令</div>
        <div class="manage-desc">定义可复用的命令片段，支持多行按顺序执行，终端中一键运行</div>
      </div>
      <MyButton variant="primary" class="manage-btn" @click="emit('open-manage')">管理</MyButton>
    </div>
  </MySection>

  <div class="divider" />

  <!-- 行间延迟 -->
  <MySection title="行间延迟">
    <div class="delay-intro">
      多行快捷命令逐行发送时的间隔策略。<b>智能等待</b>监听终端输出，等上一行输出静止后再发下一行（推荐，自动适配命令速度，能处理密码提示）；<b>固定延迟</b>每行固定等待；<b>关闭</b>一次性全部发出。
      <br />
      需在某处精确等待时，可在命令里单独写一行 <code>##delay:800</code>（毫秒）或
      <code>##delay:1s</code>（秒），作为最小等待下限（与模式叠加生效）。
    </div>
    <div class="field">
      <label class="field-label">等待模式</label>
      <MySelect
        :model-value="qcMode"
        :options="MODE_OPTIONS"
        class="select-narrow"
        @update:model-value="onModeChange"
      />
    </div>
    <div v-if="qcMode !== 'off'" class="field">
      <label class="field-label">{{ qcMode === "idle" ? "静止判定时长" : "固定延迟时长" }}</label>
      <MySelect
        :model-value="qcMs"
        :options="MS_OPTIONS"
        class="select-narrow"
        @update:model-value="onMsChange"
      />
    </div>
    <div v-if="qcMode === 'idle'" class="idle-hint">
      基于"输出静止"判定：对断续输出的命令（如 apt install 有较长停顿）可能略早发送下一条；可调大静止时长，或在该处用
      ##delay:N 兜底。
    </div>
  </MySection>
</template>

<style scoped>
.manage-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.manage-text {
  flex: 1;
  min-width: 0;
}

.manage-title {
  font-size: 13px;
  color: var(--text-primary);
}

.manage-desc {
  font-size: 12px;
  color: var(--text-muted);
  margin-top: 2px;
}

.manage-btn {
  white-space: nowrap;
}

.divider {
  height: 1px;
  background: var(--border-subtle);
  margin: 20px 0;
}

.field {
  margin-bottom: 12px;
}

.field-label {
  display: block;
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 6px;
  font-weight: 500;
}

.select-narrow {
  max-width: 280px;
}

.idle-hint {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 4px;
  line-height: 1.5;
}

.delay-intro {
  font-size: 12px;
  color: var(--text-muted);
  margin-bottom: 14px;
  line-height: 1.6;
}

.delay-intro code {
  background: var(--bg-input);
  padding: 1px 5px;
  border-radius: 3px;
  font-family: "Cascadia Code", "Fira Code", monospace;
  font-size: 12px;
}
</style>
