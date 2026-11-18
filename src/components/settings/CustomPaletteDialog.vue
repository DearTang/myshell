<!-- 自定义主题对话框 — 从旧 SettingsPanel.tsx 的 CustomPaletteDialog 移植。
     颜色选择器恒定产出合法 #RRGGBB（MyColorField 未开 showAlpha，避免 rgba
     产出与下方 HEX 校验/十六进制+alpha 后缀的调色板方案冲突），但旁边的
     自由文本输入可以填任何内容；在此校验避免把非法 CSS 颜色烘进保存的主题。 -->
<script setup lang="ts">
import { computed } from "vue";
import { MyButton, MyColorField, MyDialog, MyInput } from "myui";

defineOptions({ name: "CustomPaletteDialog" });

const props = defineProps<{
  bg: string;
  bgValid: boolean;
}>();

const emit = defineEmits<{
  "update:bg": [value: string];
  save: [];
  close: [];
}>();

const canSave = computed(() => props.bgValid);
</script>

<template>
  <MyDialog
    :model-value="true"
    title="自定义终端配色"
    size="sm"
    :dismissable="false"
    hide-footer
    @cancel="emit('close')"
  >
    <div class="dialog-intro">
      自定义预设只覆盖终端背景色（其余 16 色沿用 Carbon 基线）。界面强调色请用「外观
      → 主题色」滑杆调整。
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

    <!-- 预览：光标色条复用预设卡片同款（Carbon 光标色），直观看到成品形态 -->
    <div class="preview" :style="{ background: bg }">
      <div class="preview-label">终端背景预览</div>
    </div>

    <div class="dialog-actions">
      <MyButton variant="secondary" @click="emit('close')">取消</MyButton>
      <MyButton
        variant="primary"
        :disabled="!canSave"
        :title="canSave ? undefined : '请输入合法的背景色'"
        @click="emit('save')"
      >
        保存
      </MyButton>
    </div>
  </MyDialog>
</template>


<style scoped>
.dialog-intro {
  font-size: var(--fs-xs);
  color: var(--text-muted);
  margin-bottom: 14px;
}

.field {
  margin-bottom: 12px;
}

.field-label {
  display: block;
  font-size: var(--fs-xs);
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
  font-size: var(--fs-2xs);
  color: #888;
  text-align: center;
}

.preview-button {
  padding: 6px 12px;
  border-radius: var(--radius-md);
  font-size: var(--fs-xs);
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
