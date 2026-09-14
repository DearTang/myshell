<!-- 广播重复连接提醒（自包含，数据来自 sessions store）。
     从 src-legacy/components/BroadcastDupDialog.tsx 原样移植（文案与决策语义逐字保留）：
     同一连接已有一个标签页在广播组内时，再加入第二个标签页需要确认——两个共享同一
     连接的终端通常意味着命令会在同一台服务器上执行两次，但跳板机场景（一连接多标签、
     各自 SSH 转发到不同目标）确实需要重复。不替用户做决定，问一次由用户选择。
     "本次会话不再提醒"勾选框（默认勾选）在本次应用会话内静默后续提醒——标志活在
     ref 而非存储里，重启后重置。勾选框还会记住用户上次的选择：取消过一次，后续
     提醒就以未勾选状态打开。
     确认后直接把 tabId 加进 sessions.broadcastIds（旧版 setBroadcastMembership）；
     壳：MyDialog（默认 footer：确认=确认加入 / 取消=取消；旧版「点遮罩 = 取消」用
     close-on-click-modal 保持，Esc/右上角关闭旧版没有 → 维持禁用）。 -->
<script setup lang="ts">
import { ref, watch } from "vue";
import { Warning } from "@element-plus/icons-vue";
import { MyCheckbox, MyDialog } from "myui";
import { broadcastDup, broadcastDupDismissed, broadcastDupDontRemindPref, sessions } from "@/store/sessions";

defineOptions({ name: "BroadcastDupDialog" });

// Seeded from the user's last choice (session-scoped) so the checkbox state
// persists between prompts; defaults to checked on the first ever prompt.
const dontRemind = ref(true);

// 组件常驻（壳层无条件挂载）：每次弹出新 prompt 时按旧版 useState(initialDontRemind)
// 语义重新播种勾选框状态。
watch(
  () => broadcastDup.prompt,
  (p) => {
    if (p) dontRemind.value = p.initialDontRemind;
  },
);

function onConfirm(): void {
  const prompt = broadcastDup.prompt;
  if (!prompt) return;
  // Remember the user's checkbox choice for the next prompt this
  // session, then fold it into the session dismiss flag: checked ⇒
  // all future duplicate adds skip the prompt; unchecked ⇒ only this
  // add is allowed through, the next one will prompt again.
  broadcastDupDontRemindPref.value = dontRemind.value;
  if (dontRemind.value) broadcastDupDismissed.value = true;
  sessions.broadcastIds.add(prompt.tabId);
  broadcastDup.prompt = null;
}

function onCancel(): void {
  // Even on cancel we keep the user's checkbox preference so the
  // next prompt reopens in the same state they left it.
  broadcastDupDontRemindPref.value = dontRemind.value;
  broadcastDup.prompt = null;
}
</script>

<template>
  <!-- 组件常驻挂载（壳层无条件渲染），model-value 由 prompt 派生。
       内容须 v-if 守卫：关闭过渡动画期间 prompt 已为 null，不能解引用其字段。 -->
  <MyDialog
    :model-value="broadcastDup.prompt !== null"
    title="该连接已在广播组中"
    size="sm"
    align-center
    confirm-text="确认加入"
    cancel-text="取消"
    close-on-click-modal
    @confirm="onConfirm"
    @cancel="onCancel"
  >
    <template #header>
      <div class="head-row">
        <span class="warn-icon">
          <el-icon :size="24" color="var(--warning)"><Warning /></el-icon>
        </span>
        <span class="title">该连接已在广播组中</span>
      </div>
    </template>

    <template v-if="broadcastDup.prompt">
      <div class="desc">
        广播组里已有 <b class="strong">{{ broadcastDup.prompt.existingCount }}</b> 个来自连接
        「<b class="strong">{{ broadcastDup.prompt.connectionName }}</b
        >」的标签页。
        继续加入后，广播命令会在该连接上执行多次。
      </div>

      <div class="checkbox-row">
        <MyCheckbox v-model="dontRemind" label="本次会话不再提醒（重启应用后重置）" />
      </div>
    </template>
  </MyDialog>
</template>

<style scoped>
.head-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.warn-icon {
  font-size: 24px;
  line-height: 1;
}

.title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
}

.desc {
  font-size: 12.5px;
  color: var(--text-secondary);
  line-height: 1.7;
}

.strong {
  color: var(--text-primary);
}

.checkbox-row {
  display: flex;
  align-items: center;
  margin-top: 18px;
  padding: 10px 12px;
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  user-select: none;
}

.checkbox-row :deep(.el-checkbox__label) {
  font-size: 12px;
  color: var(--text-secondary);
}
</style>
