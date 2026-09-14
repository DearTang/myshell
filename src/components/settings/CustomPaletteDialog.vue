<!-- 自定义主题对话框 — 从旧 SettingsPanel.tsx 的 CustomPaletteDialog 移植。
     颜色选择器恒定产出合法 #RRGGBB（MyColorField 未开 showAlpha，避免 rgba
     产出与下方 HEX 校验/十六进制+alpha 后缀的调色板方案冲突），但旁边的
     自由文本输入可以填任何内容；在此校验避免把非法 CSS 颜色烘进保存的主题。 -->
<script setup lang="ts">
import { computed } from "vue";
import { MyButton, MyColorField, MyDialog, MyInput } from "myui";

defineOptions({ name: "CustomPaletteDialog" });

const props = defineProps<{
  accent: string;
  bg: string;
  accentValid: boolean;
  bgValid: boolean;
}>();

const emit = defineEmits<{
  "update:accent": [value: string];
  "update:bg": [value: string];
  save: [];
  close: [];
}>();

const canSave = computed(() => props.accentValid && props.bgValid);
</script>

<template>
  <MyDialog
    :model-value="true"
    title="自定义主题"
    :width="400"
    :dismissable="false"
    hide-footer
    @cancel="emit('close')"
  >
    <div class="dialog-intro">选择主题色和终端背景色，保存为自定义主题</div>

    <div class="field">
      <label class="field-label">主题色<span class="req">*</span></label>
      <div class="color-row">
        <!-- 与旧原生 input[type=color] 一致：非法文本时选色器回退展示默认色。 -->
        <MyColorField
          class="color-picker"
          :model-value="accentValid ? accent : '#6366f1'"
          bare
          @update:model-value="emit('update:accent', $event ?? '')"
        />
        <MyInput
          :model-value="accent"
          :error="accentValid ? '' : '请输入合法的颜色值，如 #6366f1 或 #f1f'"
          placeholder="#6366f1"
          class="color-text"
          @update:model-value="emit('update:accent', $event)"
        />
      </div>
    </div>

    <div class="field">
      <label class="field-label">终端背景色<span class="req">*</span></label>
      <div class="color-row">
        <MyColorField
          class="color-picker"
          :model-value="bgValid ? bg : '#1e1e2e'"
          bare
          @update:model-value="emit('update:bg', $event ?? '')"
        />
        <MyInput
          :model-value="bg"
          :error="bgValid ? '' : '请输入合法的颜色值，如 #1e1e2e 或 #222'"
          placeholder="#1e1e2e"
          class="color-text"
          @update:model-value="emit('update:bg', $event)"
        />
      </div>
    </div>

    <!-- 预览 -->
    <div class="preview" :style="{ background: bg, borderColor: accent }">
      <div class="preview-label">预览</div>
      <div class="preview-button" :style="{ background: accent }">主题按钮</div>
    </div>

    <div class="dialog-actions">
      <MyButton variant="secondary" @click="emit('close')">取消</MyButton>
      <MyButton
        variant="primary"
        :disabled="!canSave"
        :title="canSave ? undefined : '请输入合法的主题色和背景色'"
        @click="emit('save')"
      >
        保存
      </MyButton>
    </div>
  </MyDialog>
</template>

<style scoped>
.dialog-intro {
  font-size: 12px;
  color: var(--text-muted);
  margin-bottom: 14px;
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

.req {
  color: var(--error);
  margin-left: 3px;
  font-weight: 700;
}

.color-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

/* 选色器：MyColorField 裸态（不渲染自带 label，沿用上方 field-label 布局），
   触发块尺寸对齐旧原生 input[type=color] 的 36px 观感。 */
.color-picker {
  flex-shrink: 0;
}

.color-picker :deep(.el-color-picker__trigger) {
  width: 36px;
  height: 36px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border-default);
}

.color-text {
  flex: 1;
}

.preview {
  margin-top: 16px;
  padding: 16px;
  border: 2px solid;
  border-radius: var(--radius-lg);
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.preview-label {
  font-size: 11px;
  color: #888;
  text-align: center;
}

.preview-button {
  padding: 6px 12px;
  border-radius: var(--radius-md);
  font-size: 12px;
  font-weight: 600;
  color: #fff;
  text-align: center;
}

.dialog-actions {
  display: flex;
  gap: 10px;
  margin-top: 20px;
  justify-content: flex-end;
}
</style>
