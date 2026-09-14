<!-- AI 供应商的模型管理块 — 列表 / 从接口获取 / 手动添加（从旧 AiSection 拆出）。
     models 为受控属性，增删通过 update:models 回传父级；接口获取与手动添加
     的输入状态为本地状态。 -->
<script setup lang="ts">
import { ref } from "vue";
import { MyButton, MyInput, toast } from "myui";
import {
  fetchProviderModels,
  fetchModelsForSupplier,
  type AiProvider,
  type SupplierModel,
} from "@/api";

defineOptions({ name: "AiModelManager" });

const props = defineProps<{
  models: SupplierModel[];
  provider: AiProvider;
  baseUrl: string;
  apiKey: string;
  hasKey: boolean;
  supplierId: number | null;
  creating: boolean;
}>();

const emit = defineEmits<{
  "update:models": [models: SupplierModel[]];
}>();

// 右侧"从接口获取模型"状态。
const fetchingModels = ref(false);
const fetchedModels = ref<string[]>([]);
// 手动添加模型缓冲。
const showAddModel = ref(false);
const newModelId = ref("");
const newModelLabel = ref("");

function showToast(kind: "ok" | "err", text: string): void {
  toast(text, { type: kind === "ok" ? "success" : "error", duration: 4000 });
}

function setModels(models: SupplierModel[]): void {
  emit("update:models", models);
}

// 从接口获取模型（openai_compatible / ollama）。
async function handleFetchModels(): Promise<void> {
  if (!props.baseUrl) {
    showToast("err", "请先填写 Base URL");
    return;
  }
  if (!props.apiKey && !props.hasKey) {
    showToast("err", "请先填写 API Key");
    return;
  }
  fetchingModels.value = true;
  fetchedModels.value = [];
  try {
    let models: string[];
    if (props.creating || !props.supplierId) {
      // 新供应商尚未保存：直接用表单值。
      models = await fetchProviderModels(props.provider, props.baseUrl, props.apiKey);
    } else {
      // 已有供应商：服务端解密 key，但用户输入的新 key 优先。
      models = await fetchModelsForSupplier(props.supplierId, props.apiKey || undefined);
    }
    fetchedModels.value = models;
    if (models.length === 0) {
      showToast("err", "接口返回空模型列表");
    }
  } catch (e) {
    showToast("err", `获取模型失败: ${e}`);
  } finally {
    fetchingModels.value = false;
  }
}

function addFetchedModel(mid: string): void {
  const alreadyAdded = props.models.some((m) => m.modelId === mid);
  if (!alreadyAdded) {
    setModels([
      ...props.models,
      { id: 0, supplierId: props.supplierId ?? 0, modelId: mid, label: undefined, sortOrder: props.models.length },
    ]);
  }
}

function addManualModel(): void {
  if (!newModelId.value.trim()) return;
  setModels([
    ...props.models,
    {
      id: 0,
      supplierId: props.supplierId ?? 0,
      modelId: newModelId.value.trim(),
      label: newModelLabel.value.trim() || undefined,
      sortOrder: props.models.length,
    },
  ]);
  newModelId.value = "";
  newModelLabel.value = "";
  showAddModel.value = false;
}

function removeModel(idx: number): void {
  if (idx === 0) return; // 主模型不可删除
  setModels(props.models.filter((_, i) => i !== idx));
}

/** 供应商切换 / 存储重载时清空"从接口获取"与手动添加表单的瞬态
 *  （对应旧版编辑缓冲同步 effect 里的 setFetchedModels([]) /
 *  setShowAddModel(false)）。 */
function resetTransient(): void {
  fetchedModels.value = [];
  showAddModel.value = false;
}

defineExpose({ resetTransient });
</script>

<template>
  <div class="models-block">
    <div class="models-title">模型列表</div>
    <div class="models-list">
      <div v-if="models.length === 0" class="models-empty">暂无模型，请添加或从接口获取</div>
      <div v-for="(model, idx) in models" :key="model.id || idx" class="model-row">
        <span class="model-name">
          {{ model.label ? `${model.label} (${model.modelId})` : model.modelId }}
        </span>
        <span v-if="idx === 0" class="model-primary">主</span>
        <button
          class="model-remove"
          :disabled="idx === 0"
          :title="idx === 0 ? '主模型不可删除' : '删除'"
          type="button"
          @click="removeModel(idx)"
        >
          ×
        </button>
      </div>
    </div>

    <!-- 从接口获取模型 -->
    <div v-if="provider === 'openai_compatible' || provider === 'ollama'" class="fetch-row">
      <MyButton
        variant="secondary"
        size="small"
        :disabled="fetchingModels || !baseUrl"
        @click="handleFetchModels"
      >
        {{ fetchingModels ? "获取中…" : "从接口获取模型" }}
      </MyButton>
    </div>

    <!-- 已获取模型 — 点击添加 -->
    <div v-if="fetchedModels.length > 0" class="fetched-block">
      <div class="fetched-hint">点击添加到列表：</div>
      <div class="fetched-chips">
        <button
          v-for="mid in fetchedModels"
          :key="mid"
          class="fetched-chip"
          :class="{ added: models.some((m) => m.modelId === mid) }"
          :disabled="models.some((m) => m.modelId === mid)"
          type="button"
          @click="addFetchedModel(mid)"
        >
          {{ models.some((m) => m.modelId === mid) ? "✓ " : "+ " }}{{ mid }}
        </button>
      </div>
    </div>

    <!-- 手动添加模型 -->
    <div v-if="showAddModel" class="manual-add">
      <div class="manual-field">
        <label class="manual-label">模型 ID</label>
        <MyInput v-model="newModelId" placeholder="如 glm-4-plus" />
      </div>
      <div class="manual-field">
        <label class="manual-label">显示名（可选）</label>
        <MyInput v-model="newModelLabel" placeholder="如 GLM-4 Plus" />
      </div>
      <MyButton
        variant="primary"
        size="small"
        class="manual-add-btn"
        :disabled="!newModelId.trim()"
        @click="addManualModel"
      >
        添加
      </MyButton>
      <MyButton variant="secondary" size="small" class="manual-add-btn" @click="showAddModel = false">
        取消
      </MyButton>
    </div>
    <MyButton
      v-else
      variant="ghost"
      size="small"
      class="manual-add-toggle"
      @click="showAddModel = true"
    >
      ＋ 手动添加模型
    </MyButton>
  </div>
</template>

<style scoped>
.models-block {
  border-top: 1px solid var(--border-subtle);
  padding-top: 12px;
}

.models-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 8px;
}

.models-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 8px;
}

.models-empty {
  font-size: 11px;
  color: var(--text-muted);
  padding: 4px 0;
}

.model-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px;
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
}

.model-name {
  flex: 1;
  font-size: 11px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.model-primary {
  font-size: 9px;
  color: var(--text-muted);
}

.model-remove {
  background: transparent;
  border: none;
  color: var(--error);
  font-size: 14px;
  cursor: pointer;
  padding: 0 4px;
}

.model-remove:disabled {
  color: var(--text-muted);
  cursor: default;
}

.fetch-row {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 8px;
}

.fetched-block {
  margin-bottom: 8px;
}

.fetched-hint {
  font-size: 11px;
  color: var(--text-muted);
  margin-bottom: 4px;
}

.fetched-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  max-height: 100px;
  overflow-y: auto;
}

.fetched-chip {
  background: var(--accent-primary-muted);
  border: 1px solid var(--accent-primary);
  border-radius: var(--radius-full);
  padding: 3px 10px;
  font-size: 11px;
  color: var(--text-primary);
  cursor: pointer;
}

.fetched-chip.added {
  background: var(--bg-input);
  border-color: var(--border-default);
  color: var(--text-muted);
  cursor: default;
  opacity: 0.5;
}

.manual-add {
  display: flex;
  gap: 6px;
  align-items: flex-end;
  margin-bottom: 4px;
}

.manual-field {
  flex: 1;
}

.manual-label {
  display: block;
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 6px;
  font-weight: 500;
}

.manual-add-btn {
  margin-bottom: 12px;
}

.manual-add-toggle {
  width: 100%;
  border: 1px dashed var(--border-default);
}
</style>
