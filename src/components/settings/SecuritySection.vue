<!-- 安全 — 管理员权限 / 自动锁定 / 隐私 / 修改登录密码（旧 security 分区） -->
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { CircleCheck, CircleClose } from "@element-plus/icons-vue";
import { MyButton, MyInput, MySection, MySelect, MyToggle, type SelectOption, confirmDialog } from "myui";
import {
  changeMasterPassword,
  getAppSettings,
  setAppSettings,
  isElevated,
  restartAsAdmin,
} from "@/api";

defineOptions({ name: "SecuritySection" });

const MIN_LEN = 6;

// ── 管理员权限 ──
// null = 尚未检测。驱动管理员状态徽标 + 重启按钮。
const elevated = ref<boolean | null>(null);
const restartBusy = ref(false);

onMounted(() => {
  isElevated()
    .then((v) => (elevated.value = v))
    .catch(() => (elevated.value = false));
  getAppSettings()
    .then((s) => (disableCommandHistory.value = s.disable_command_history))
    .catch(() => {});
});

async function handleRestartAdmin(): Promise<void> {
  const ok = await confirmDialog({
    message: "将以管理员身份重启 MyShell，当前所有终端会话将被关闭。是否继续？",
    title: "以管理员重启",
    type: "warning",
  });
  if (!ok) return;
  restartBusy.value = true;
  try {
    await restartAsAdmin();
    // 成功后本进程退出、提权实例接管 — 故意不复位 restartBusy
    // （这个 UI 即将消失）。
  } catch (e) {
    restartBusy.value = false;
    const msg = String(e);
    // UAC 取消时静默；其余一律上报。
    if (!msg.includes("取消")) alert(`以管理员重启失败: ${msg}`);
  }
}

// ── 自动锁定 ──
const autoLockMinutes = ref(localStorage.getItem("myshell-auto-lock-minutes") ?? "30");

const AUTO_LOCK_OPTIONS: SelectOption[] = [
  { label: "10 分钟", value: "10" },
  { label: "30 分钟（默认）", value: "30" },
  { label: "1 小时", value: "60" },
  { label: "不启用", value: "0" },
];

function onAutoLockChange(v: unknown): void {
  const value = String(v);
  autoLockMinutes.value = value;
  localStorage.setItem("myshell-auto-lock-minutes", value);
  window.dispatchEvent(new Event("myshell-auto-lock-changed"));
}

// ── 隐私：后端强制的命令历史开关（settings.json）──
const disableCommandHistory = ref(false);

async function onDisableHistoryChange(next: string | number | boolean): Promise<void> {
  const value = Boolean(next);
  disableCommandHistory.value = value;
  try {
    await setAppSettings({
      disable_command_history: value,
    });
  } catch {
    disableCommandHistory.value = !value;
  }
}

// ── 修改登录密码 ──
const oldPass = ref("");
const newPass = ref("");
const confirmPass = ref("");
const passwordBusy = ref(false);
const passwordErr = ref<string | null>(null);
const passwordSuccess = ref(false);

function strengthHint(p: string): { label: string; color: string; width: string } {
  let classes = 0;
  if (/[a-z]/.test(p)) classes++;
  if (/[A-Z]/.test(p)) classes++;
  if (/[0-9]/.test(p)) classes++;
  if (/[^A-Za-z0-9]/.test(p)) classes++;
  if (p.length < MIN_LEN) return { label: "至少 6 个字符", color: "var(--error)", width: "0%" };
  if (p.length < 8) return { label: "简单", color: "var(--warning)", width: "25%" };
  if (classes <= 2) return { label: "弱", color: "var(--error)", width: "50%" };
  if (classes === 3) return { label: "中等", color: "var(--warning)", width: "75%" };
  return { label: "强", color: "var(--success)", width: "100%" };
}

const hint = computed(() => strengthHint(newPass.value));
const mismatch = computed(() => confirmPass.value.length > 0 && confirmPass.value !== newPass.value);
const canChangePassword = computed(
  () =>
    oldPass.value.length >= MIN_LEN &&
    newPass.value.length >= MIN_LEN &&
    confirmPass.value === newPass.value &&
    !passwordBusy.value,
);

async function handleChangePassword(): Promise<void> {
  if (!canChangePassword.value) return;
  passwordBusy.value = true;
  passwordErr.value = null;
  passwordSuccess.value = false;
  try {
    await changeMasterPassword(oldPass.value, newPass.value);
    passwordSuccess.value = true;
    oldPass.value = "";
    newPass.value = "";
    confirmPass.value = "";
  } catch (e) {
    passwordErr.value = String(e);
  } finally {
    passwordBusy.value = false;
  }
}
</script>

<template>
  <!-- 管理员权限 -->
  <MySection
    title="管理员权限"
    description="本地终端以 MyShell 自身的权限运行 shell。需要管理员权限执行命令（如安装软件、修改系统配置）时，以管理员身份重启 MyShell，之后所有本地连接即获得管理员权限。"
  >
    <div class="elevated-row">
      <span class="elevated-chip" :class="{ yes: elevated }">
        {{ elevated === null ? "检测中…" : elevated ? "✓ 已是管理员" : "当前：普通用户" }}
      </span>
      <MyButton
        v-if="elevated === false"
        variant="primary"
        :disabled="restartBusy"
        @click="handleRestartAdmin"
      >
        {{ restartBusy ? "启动中…" : "以管理员重启" }}
      </MyButton>
    </div>
    <div v-if="elevated === false" class="elevated-warn">
      重启将关闭当前所有终端会话；Windows 会弹出 UAC 确认，点击「是」后以管理员启动新实例。
    </div>
  </MySection>

  <div class="divider" />

  <!-- 自动锁定 -->
  <MySection
    title="自动锁定"
    description="无操作超过指定时间后自动锁定应用，需重新输入密码解锁。「不启用」则首次解锁后不再自动锁定（重启后恢复）。"
  >
    <div class="field">
      <label class="field-label">空闲锁定时长</label>
      <MySelect
        :model-value="autoLockMinutes"
        :options="AUTO_LOCK_OPTIONS"
        class="select-narrow"
        @update:model-value="onAutoLockChange"
      />
    </div>
  </MySection>

  <div class="divider" />

  <!-- 隐私 — 后端强制的命令历史开关 -->
  <MySection
    title="隐私"
    description="命令历史已始终加密存储（随保险库解锁）。开启下方开关后，后端将不再记录任何命令历史（已保存的历史保留，可按连接清空）。"
  >
    <div class="privacy-row">
      <div>
        <div class="privacy-title">不记录命令历史</div>
        <div class="privacy-desc">适合执行含临时令牌、内联密码等敏感命令的场景。立即生效。</div>
      </div>
      <MyToggle
        :model-value="disableCommandHistory"
        @update:model-value="onDisableHistoryChange"
      />
    </div>
  </MySection>

  <div class="divider" />

  <!-- 修改登录密码 -->
  <MySection title="修改登录密码" description="修改密码后需要使用新密码解锁应用">
    <div class="field">
      <label class="field-label">原密码</label>
      <MyInput
        v-model="oldPass"
        type="password"
        placeholder="输入原密码"
      />
    </div>

    <div class="field">
      <label class="field-label">新密码</label>
      <MyInput
        v-model="newPass"
        type="password"
        placeholder="至少 6 个字符"
      />
      <div v-if="newPass.length > 0" class="strength">
        <div class="strength-track">
          <div
            class="strength-fill"
            :style="{ width: hint.width, background: hint.color }"
          />
        </div>
        <div class="strength-label" :style="{ color: hint.color }">强度：{{ hint.label }}</div>
      </div>
    </div>

    <div class="field">
      <label class="field-label">确认新密码</label>
      <MyInput
        v-model="confirmPass"
        type="password"
        placeholder="再次输入新密码"
        :error="mismatch ? '两次输入不一致' : ''"
      />
    </div>

    <div v-if="passwordErr" class="msg-box error">
      <el-icon :size="14"><CircleClose /></el-icon>
      {{ passwordErr }}
    </div>
    <div v-if="passwordSuccess" class="msg-box success">
      <el-icon :size="14"><CircleCheck /></el-icon>
      密码修改成功
    </div>

    <MyButton
      variant="primary"
      class="change-btn"
      :disabled="!canChangePassword"
      @click="handleChangePassword"
    >
      {{ passwordBusy ? "处理中..." : "修改密码" }}
    </MyButton>
  </MySection>
</template>

<style scoped>
.elevated-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.elevated-chip {
  font-size: 12px;
  font-weight: 600;
  padding: 5px 12px;
  border-radius: var(--radius-full);
  background: var(--bg-surface);
  color: var(--text-tertiary);
  border: 1px solid var(--border-default);
}

.elevated-chip.yes {
  background: var(--bg-surface-hover);
  color: var(--success);
  border-color: var(--success);
}

.elevated-warn {
  margin-top: 10px;
  padding: 8px 12px;
  font-size: 11px;
  color: var(--warning);
  background: var(--warning-muted);
  border: 1px solid var(--warning);
  border-radius: var(--radius-md);
  line-height: 1.5;
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
  max-width: 240px;
}

.privacy-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.privacy-title {
  font-size: 13px;
  color: var(--text-primary);
}

.privacy-desc {
  font-size: 12px;
  color: var(--text-muted);
  margin-top: 2px;
}

.strength {
  margin-top: 8px;
}

.strength-track {
  height: 4px;
  background: var(--bg-surface);
  border-radius: var(--radius-full);
  overflow: hidden;
}

.strength-fill {
  height: 100%;
  border-radius: var(--radius-full);
  transition: width var(--duration-normal) var(--ease-out-expo);
}

.strength-label {
  margin-top: 4px;
  font-size: 11px;
}

.msg-box {
  margin-bottom: 12px;
  padding: 10px 14px;
  border-radius: var(--radius-md);
  font-size: 12px;
  display: flex;
  align-items: center;
  gap: 8px;
}

.msg-box.error {
  background: var(--error-muted);
  border: 1px solid var(--error);
  color: var(--error);
}

.msg-box.success {
  background: var(--success-muted);
  border: 1px solid var(--success);
  color: var(--success);
}

.change-btn {
  margin-top: 8px;
  width: 100%;
}
</style>
