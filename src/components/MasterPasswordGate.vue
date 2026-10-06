<!-- 保险库门禁：setup 首次设置主密码 / unlock 解锁。
     从 src-legacy/components/MasterPasswordGate.tsx 原样移植（内联样式改 scoped CSS + 令牌，文案逐字保留）。
     成功后 emit("success")，由壳层 refreshVaultStatus() 推进 vault 状态。
     防误关：全屏门禁无任何关闭路径。密码输入用 el-input（MyInput 无 password/show-password 支持，
     见迁移报告）；主密码框带明文切换，回车提交语义与旧版一致（unlock 主框回车可提交，
     setup 仅在"再次输入"框回车提交）。 -->
<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { Lock, Warning } from "@element-plus/icons-vue";
import type { InputInstance } from "element-plus";
import { MyButton } from "myui";
import { setupVault, unlockVault, getLockoutInfo } from "@/api";
import type { LockoutInfo } from "@/api";
import { BROWSER_PREVIEW_NOTICE, isTauri } from "@/utils/environment";

defineOptions({ name: "MasterPasswordGate" });

const props = defineProps<{ mode: "setup" | "unlock" }>();
const emit = defineEmits<{ success: [] }>();

const MIN_LEN = 6;

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

const pass = ref("");
const confirm = ref("");
const busy = ref(false);
const err = ref<string | null>(null);
const lockoutInfo = ref<LockoutInfo | null>(null);
const passInput = ref<InputInstance | null>(null);

const isSetup = computed(() => props.mode === "setup");
const hint = computed(() => strengthHint(pass.value));
const tooShort = computed(() => pass.value.length > 0 && pass.value.length < MIN_LEN);
const mismatch = computed(() => isSetup.value && confirm.value.length > 0 && confirm.value !== pass.value);
const canSubmit = computed(
  () => pass.value.length >= MIN_LEN && (!isSetup.value || confirm.value === pass.value) && !busy.value,
);

onMounted(() => {
  passInput.value?.focus();
});

watch(
  () => props.mode,
  (mode) => {
    if (mode === "unlock") {
      getLockoutInfo()
        .then((info) => (lockoutInfo.value = info))
        .catch(() => {});
    }
  },
  { immediate: true },
);

async function submit(): Promise<void> {
  if (!canSubmit.value) return;
  // 浏览器直开时没有 Tauri IPC 桥，给出可读提示而不是原始 TypeError
  if (!isTauri()) {
    err.value = BROWSER_PREVIEW_NOTICE;
    return;
  }
  busy.value = true;
  err.value = null;
  try {
    if (isSetup.value) {
      await setupVault(pass.value);
    } else {
      await unlockVault(pass.value);
    }
    pass.value = "";
    confirm.value = "";
    emit("success");
  } catch (e) {
    err.value = String(e);
    busy.value = false;
    pass.value = "";
    if (isSetup.value) confirm.value = "";
    if (!isSetup.value) {
      getLockoutInfo()
        .then((info) => (lockoutInfo.value = info))
        .catch(() => {});
    }
    requestAnimationFrame(() => passInput.value?.focus());
  }
}

/** 旧版语义：主密码框仅在 unlock 模式回车提交；setup 模式回车留给"再次输入"框。 */
function onPassEnter(): void {
  if (!isSetup.value) void submit();
}

function formatTime(timestamp: number | null): string {
  if (!timestamp) return "";
  const date = new Date(timestamp * 1000);
  return date.toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}
</script>

<template>
  <div class="gate" data-tauri-drag-region>
    <!-- Background Gradient Mesh -->
    <div class="gate-bg"></div>

    <!-- Main Card -->
    <div class="card animate-scale-in">
      <!-- Header：品牌 Logo 方块（参照统一框架登录页样式） -->
      <div class="header">
        <div class="brand-tile" aria-hidden="true">
          <svg width="30" height="30" viewBox="0 0 64 64" fill="none">
            <!-- chevron `>` — vertex on the left, opens right -->
            <path
              d="M35 19 L16 32 L35 45"
              stroke="currentColor"
              stroke-width="6.5"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
            <!-- cursor `_` -->
            <rect x="37" y="42" width="13" height="5" rx="2.5" fill="currentColor" />
          </svg>
        </div>
        <div class="title">{{ isSetup ? "设置登录密码" : "解锁 MyShell" }}</div>
        <div class="subtitle">
          <template v-if="isSetup">
            设置登录密码用于保护您的连接数据
            <div class="setup-warning">遗忘后将无法找回，所有数据彻底丢失</div>
          </template>
          <template v-else>
            输入登录密码解锁应用
            <div v-if="lockoutInfo && lockoutInfo.dailyFailures > 0" class="lockout-hint">
              今日已错 {{ lockoutInfo.dailyFailures }} 次<span
                v-if="lockoutInfo.lastFailureTime"
                class="lockout-time"
              >
                · 上次错误：{{ formatTime(lockoutInfo.lastFailureTime) }}</span
              >
            </div>
          </template>
        </div>
      </div>

      <!-- Password Input -->
      <div class="field">
        <label class="field-label">登录密码</label>
        <el-input
          ref="passInput"
          v-model="pass"
          type="password"
          show-password
          :class="['pass-input', { 'is-invalid': tooShort }]"
          placeholder="至少 6 个字符"
          @keydown.enter="onPassEnter"
        >
          <template #prefix>
            <el-icon :size="14"><Lock /></el-icon>
          </template>
        </el-input>
        <!-- Strength Indicator - Only show in setup mode -->
        <div v-if="isSetup && pass.length > 0" class="strength">
          <div class="strength-track">
            <div class="strength-fill" :style="{ width: hint.width, background: hint.color }"></div>
          </div>
          <div class="strength-meta" :style="{ color: hint.color }">
            <span>强度：{{ hint.label }}</span>
            <span class="char-count">{{ pass.length }} 字符</span>
          </div>
        </div>
      </div>

      <!-- Confirm Input (Setup Only) -->
      <div v-if="isSetup" class="field">
        <label class="field-label">再次输入</label>
        <el-input
          v-model="confirm"
          type="password"
          :class="['pass-input', { 'is-invalid': mismatch }]"
          placeholder="确认登录密码"
          @keydown.enter="submit()"
        >
          <template #prefix>
            <el-icon :size="14"><Lock /></el-icon>
          </template>
        </el-input>
        <div v-if="mismatch" class="mismatch">两次输入不一致</div>
      </div>

      <!-- Error Message -->
      <div v-if="err" class="err-box animate-fade-up">
        <span class="err-icon">
          <el-icon :size="14"><Warning /></el-icon>
        </span>
        {{ err }}
      </div>

      <!-- Submit Button -->
      <MyButton
        class="submit-btn"
        variant="primary"
        :disabled="!canSubmit"
        :loading="busy"
        @click="submit()"
      >
        {{ busy ? "处理中..." : isSetup ? "设置并加密所有数据" : "解锁" }}
      </MyButton>

      <!-- Footer Hint -->
      <div class="footer-hint">密码错误锁定机制：3 次错误锁定 5 分钟，每日最多 30 次</div>
    </div>
  </div>
</template>

<style scoped>
/* 旧版 global.css 的 animate-scale-in / animate-fade-up 在新版基样式中不存在，本地补齐 */
@keyframes scale-in {
  from {
    opacity: 0;
    transform: scale(0.96);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

.animate-scale-in {
  animation: scale-in var(--duration-normal) var(--ease-out-back);
}

@keyframes fade-up {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

.animate-fade-up {
  animation: fade-up var(--duration-slow) var(--ease-out-expo);
}

.gate {
  position: fixed;
  inset: 0;
  background: var(--bg-base);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 5000;
}

.gate-bg {
  position: absolute;
  inset: 0;
  background:
    radial-gradient(ellipse at 30% 20%, var(--accent-primary-muted) 0%, transparent 50%),
    radial-gradient(ellipse at 70% 80%, var(--accent-secondary-muted) 0%, transparent 50%),
    var(--bg-base);
  opacity: 0.6;
}

.card {
  position: relative;
  width: 420px;
  padding: 32px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-emphasis);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-xl);
}

.header {
  text-align: center;
  margin-bottom: 28px;
}

.brand-tile {
  width: 64px;
  height: 64px;
  margin: 0 auto 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 16px;
  background: linear-gradient(135deg, var(--accent-primary) 0%, var(--accent-secondary) 100%);
  color: #ffffff;
  box-shadow: var(--shadow-glow);
}

.title {
  font-size: 20px;
  font-weight: 600;
  color: var(--text-primary);
  letter-spacing: -0.02em;
}

.subtitle {
  font-size: 13px;
  color: var(--text-tertiary);
  margin-top: 8px;
  line-height: 1.6;
}

.setup-warning {
  margin-top: 8px;
  padding: 10px 12px;
  background: var(--warning-muted);
  border: 1px solid var(--warning);
  border-radius: var(--radius-md);
  color: var(--warning);
  font-size: 12px;
}

.lockout-hint {
  margin-top: 10px;
  padding: 8px 12px;
  background: var(--warning-muted);
  border-radius: var(--radius-md);
  font-size: 12px;
  color: var(--warning);
}

.lockout-time {
  opacity: 0.8;
}

.field {
  margin-bottom: 16px;
}

.field-label {
  display: block;
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 6px;
  font-weight: 500;
}

/* el-input：复刻旧版输入框观感（monospace、错误描边、聚焦光晕）。
   MyInput 无 password/show-password 能力，故此处直接用 el-input。
   宽度显式 100% + border-box，确保两个输入框严格等宽对齐。 */
.pass-input {
  width: 100%;
}

.pass-input :deep(.el-input__wrapper) {
  background: var(--bg-input);
  border-radius: var(--radius-md);
  box-shadow: 0 0 0 1px var(--border-default) inset;
  transition:
    box-shadow var(--duration-fast) var(--ease-in-out);
}

.pass-input :deep(.el-input__wrapper.is-focus) {
  box-shadow:
    0 0 0 1px var(--accent-primary) inset,
    0 0 0 3px var(--accent-primary-muted);
}

.pass-input.is-invalid :deep(.el-input__wrapper) {
  box-shadow: 0 0 0 1px var(--error) inset;
}

.pass-input :deep(.el-input__inner) {
  height: 40px;
  font-family: ui-monospace, monospace;
  font-size: 14px;
  color: var(--text-primary);
}

.pass-input :deep(.el-input__suffix) {
  color: var(--text-tertiary);
}

.strength {
  margin-top: 10px;
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

.strength-meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: 6px;
  font-size: 11px;
}

.char-count {
  color: var(--text-muted);
}

.mismatch {
  margin-top: 6px;
  font-size: 11px;
  color: var(--error);
}

.err-box {
  margin-bottom: 16px;
  padding: 10px 14px;
  background: var(--error-muted);
  border: 1px solid var(--error);
  border-radius: var(--radius-md);
  font-size: 12px;
  color: var(--error);
  display: flex;
  align-items: center;
  gap: 8px;
}

.err-icon {
  font-size: 14px;
}

.submit-btn {
  width: 100%;
  height: 44px;
  font-size: 14px;
  font-weight: 600;
}

.footer-hint {
  margin-top: 20px;
  text-align: center;
  font-size: 11px;
  color: var(--text-muted);
}
</style>
