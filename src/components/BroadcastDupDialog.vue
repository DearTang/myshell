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
import { MyCheckbox, MyDialog, MyIcon } from "myui";
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
          <MyIcon :size="24" color="var(--warning)" name="Warning" />
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

      <!-- 场景说明：只说「会重复」容易让人以为这是错误行为并一律取消。
           同一连接开多个标签页本身是合法的（跳板机一连接多目标），说清
           「同一设备可重复加入、各标签页独立收命令」，用户才能自己判断。 -->
      <div class="hint">
        <MyIcon class="hint-icon" :size="14" name="InfoFilled" />
        <span>
          同一设备可以多次加入广播组，每个标签页会独立收到同一条广播命令。
          如果这是「一台连接转多个目标」（如跳板机转发的不同内网机器），继续加入正是你要的。
        </span>
      </div>

      <div class="checkbox-row">
        <!-- 文案走默认插槽而非 label 属性：MyCheckbox 会把 label 交给外层
             MyFieldShell 渲染成 `<label class="my-field__label">`（**表单字段标签，
             位于控件上方**），于是变成「上面一行文字、下面一行孤零零的方框」，
             看起来像还有内容没渲染出来。行内文案必须放进默认插槽，
             由 ElCheckbox 渲染进 .el-checkbox__label（与 ConnectionDialog 的
             TMOUT 开关同一写法）。 -->
        <MyCheckbox v-model="dontRemind" class="dont-remind">
          本次会话不再提醒（重启应用后重置）
        </MyCheckbox>
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

/* 场景说明条：InfoFilled 图标 + 正文，比正文再低一档的次级色，
   与 .desc 的警告语气区分开（这里是"中性解释"，不是又一条风险）。 */
.hint {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  margin-top: 12px;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  font-size: 12px;
  line-height: 1.7;
  color: var(--text-secondary);
}

.hint-icon {
  flex-shrink: 0;
  margin-top: 2px;
  color: var(--text-muted);
}

.checkbox-row {
  display: flex;
  align-items: center;
  margin-top: 12px;
  padding: 10px 12px;
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  user-select: none;
}

/* class 不会落在外层 .my-field 上 —— MyCheckbox 虽然 `inheritAttrs: false`，
   但它把 $attrs merge 到了 ElCheckbox 的根 <label class="el-checkbox"> 上。
   故选择器直接命中 el-checkbox 本身，压掉 EP 默认的 32px 行高（单行文案时
   会让这条带子撑成 52px）。 */
.dont-remind {
  height: auto;
}

.dont-remind :deep(.el-checkbox__label) {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-secondary);
  white-space: normal;
}
</style>
