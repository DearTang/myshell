<!-- 密码验证对话框 — 从 src-legacy/components/PasswordVerifyDialog.tsx 原样移植
     （文案逐字保留）。编辑连接时查看已存明文密码前的门禁，挂在 ConnectionDialog 内。
     成功后把输入的主密码回传给父组件（success 事件）；父组件负责原子揭示。
     旧版样式沿用：本地遮罩（点遮罩不关闭，与旧版一致），z-index 3000 压过编辑框。 -->
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import type { ComponentPublicInstance } from "vue";
import { Lock } from "@element-plus/icons-vue";
import { MyButton, MyInput } from "myui";
import { verifyPassword } from "@/api";

defineOptions({ name: "PasswordVerifyDialog" });

const emit = defineEmits<{
  /** 内联预检通过后回传明文主密码。
   *  后端在解密时会权威地再验一次（原子揭示）——本对话框的检查只是 UX
   *  （内联错误、防呆），不是安全边界。 */
  success: [masterPassword: string];
  close: [];
}>();

const MIN_LEN = 6;

const pass = ref("");
const busy = ref(false);
const err = ref<string | null>(null);
// MyInput 根节点（.my-field），内层原生 input 挂载后手动聚焦（对齐旧 useEffect）
const inputRef = ref<ComponentPublicInstance | null>(null);

onMounted(() => {
  const el = inputRef.value?.$el as HTMLElement | undefined;
  el?.querySelector("input")?.focus();
});

const canSubmit = computed(() => pass.value.length >= MIN_LEN && !busy.value);

async function submit(): Promise<void> {
  if (!canSubmit.value) return;
  busy.value = true;
  err.value = null;
  try {
    const valid = await verifyPassword(pass.value);
    if (valid) {
      const typed = pass.value;
      pass.value = "";
      emit("success", typed);
    } else {
      err.value = "密码错误";
      pass.value = "";
    }
  } catch (e) {
    err.value = String(e);
    // Clear the master password on a wrong attempt too. `verify_password`
    // never returns Ok(false) — a wrong password arrives as an Err — so the
    // `else` branch above is dead code and the only clearing paths were the
    // success and dead ones. The plaintext master password was therefore left
    // in the ref (and in the mounted input) after every failure, ready to be
    // resubmitted with Enter.
    pass.value = "";
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="overlay">
    <div class="panel" @click.stop>
      <div class="title">
        <el-icon class="title-icon" :size="14"><Lock /></el-icon>
        验证密码
      </div>
      <div class="subtitle">请输入登录密码以查看明文密码</div>

      <label class="field-label">登录密码</label>
      <MyInput
        ref="inputRef"
        v-model="pass"
        :clearable="false"
        type="password"
        placeholder="输入登录密码"
        @keydown.enter="submit"
      />

      <div v-if="err" class="error-box">{{ err }}</div>

      <div class="actions">
        <MyButton variant="secondary" size="small" @click="emit('close')">取消</MyButton>
        <MyButton variant="primary" size="small" :disabled="!canSubmit" @click="submit">
          {{ busy ? "验证中…" : "确认" }}
        </MyButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: var(--bg-overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 3000;
}

.panel {
  background: var(--bg-elevated);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 20px;
  width: 360px;
  box-shadow: var(--shadow-xl);
}

.title {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 6px;
  color: var(--text-primary);
  display: flex;
  align-items: center;
  gap: 5px;
}

.title-icon {
  color: var(--text-secondary);
}

.subtitle {
  font-size: 12px;
  color: var(--text-muted);
  margin-bottom: 14px;
}

.field-label {
  display: block;
  font-size: 11px;
  color: var(--text-muted);
  margin-bottom: 4px;
}

.error-box {
  margin-top: 10px;
  padding: 8px 10px;
  background: var(--error-muted);
  border: 1px solid var(--error);
  border-radius: var(--radius-sm);
  font-size: 12px;
  color: var(--error);
}

.actions {
  display: flex;
  gap: 8px;
  margin-top: 16px;
  justify-content: flex-end;
}
</style>
