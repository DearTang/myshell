<!-- AI 助手 — 供应商左列表 + 右详情编辑器（旧 ai 分区）。
     多模型存储是唯一事实来源；右侧面板编辑的是所选供应商的副本。
     左列表 / 模型管理块拆分为 AiSupplierList / AiModelManager。 -->
<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { MyButton, MyInput, MySelect, MySlider, MySection, type SelectOption, confirmDialog, toast } from "myui";
import {
  listAiModels,
  getActiveAiModelId,
  getAiSettings,
  saveAiModel,
  deleteAiModel,
  setActiveAiModel,
  toggleAiModelEnabled,
  aiTestSettings,
  type AiModel,
  type AiProvider,
} from "@/api";
import AiSupplierList from "./AiSupplierList.vue";
import AiModelManager from "./AiModelManager.vue";
defineOptions({ name: "AiSection" });

// ── 系统 AI 供应商预设（对应 ai.rs 的 init_ai_presets_cmd）──
// 不显示在侧栏 — 仅在新建供应商时作为模板，自动填充 provider + baseUrl。
interface AiPreset {
  name: string;
  provider: Exclude<AiProvider, "claude" | "openai" | "ollama">;
  baseUrl: string;
}
const AI_PRESETS: AiPreset[] = [
  { name: "GLM (智谱)", provider: "openai_compatible", baseUrl: "https://open.bigmodel.cn/api/paas/v4" },
  { name: "GLM Coding Plan (OpenAI)", provider: "openai_compatible", baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4" },
  { name: "GLM Coding Plan (Anthropic)", provider: "anthropic_compatible", baseUrl: "https://open.bigmodel.cn/api/anthropic" },
  { name: "MIMO", provider: "openai_compatible", baseUrl: "https://api.xiaomimimo.com/v1" },
  { name: "MiniMax M3", provider: "openai_compatible", baseUrl: "https://api.minimaxi.com/v1" },
  { name: "LongCat", provider: "openai_compatible", baseUrl: "https://api.longcat.chat/openai" },
  { name: "DeepSeek", provider: "openai_compatible", baseUrl: "https://api.deepseek.com/v1" },
  { name: "通义千问 (阿里云)", provider: "openai_compatible", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1" },
  { name: "混元 (腾讯云)", provider: "openai_compatible", baseUrl: "https://api.hunyuan.cloud.tencent.com/v1" },
  { name: "Ollama (本地)", provider: "openai_compatible", baseUrl: "http://localhost:11434/api" },
];
const PROVIDER_LABELS: Record<string, string> = {
  openai_compatible: "OpenAI 兼容",
  anthropic_compatible: "Anthropic 兼容",
  claude: "Claude 官方",
  openai: "OpenAI 官方",
  ollama: "Ollama 本地",
};

const PROVIDER_OPTIONS: SelectOption[] = (
  [
    ["openai_compatible", "OpenAI 兼容"],
    ["anthropic_compatible", "Anthropic 兼容"],
    ["claude", "Claude (Anthropic 官方)"],
    ["openai", "OpenAI (官方)"],
    ["ollama", "Ollama (本地)"],
  ] as [AiProvider, string][]
).map(([value, label]) => ({ value, label }));

const PRESET_OPTIONS: SelectOption[] = [
  { label: "— 自定义 —", value: -1 },
  ...AI_PRESETS.map((p, idx) => ({
    label: `${p.name}（${PROVIDER_LABELS[p.provider] ?? p.provider}）`,
    value: idx,
  })),
];

// ── 多模型配置（旧 useAiConfig.reload 语义：settings/models/activeId 三读）──
const aiModels = ref<AiModel[]>([]);
const aiActiveId = ref<number | null>(null);
const aiLoading = ref(true);

async function reloadAi(): Promise<void> {
  try {
    const [, m, aid] = await Promise.all([getAiSettings(), listAiModels(), getActiveAiModelId()]);
    aiModels.value = m;
    aiActiveId.value = aid;
  } catch {
    aiModels.value = [];
    aiActiveId.value = null;
  } finally {
    aiLoading.value = false;
  }
}

onMounted(() => {
  void reloadAi();
});

const selectedSupplierId = ref<number | null>(null);
// "创建模式"：为 true 时右侧显示新供应商表单。
const creating = ref(false);
const createPresetIdx = ref(-1); // -1 = 自定义
// 所选供应商的编辑缓冲（右侧面板）。
const editName = ref("");
const editProvider = ref<AiProvider>("openai_compatible");
const editBaseUrl = ref("");
const editProxy = ref("");
const editKey = ref("");
const editTemp = ref(0.7);
const editModels = ref<AiModel["models"]>([]);
const editSaving = ref(false);
const editTesting = ref(false);
// 当前编辑缓冲是否已通过测试（创建模式的门槛）。
const testedOk = ref(false);

const selectedSupplier = computed<AiModel | null>(
  () => aiModels.value.find((m) => m.id === selectedSupplierId.value) ?? null,
);
// 侧栏只显示非预设供应商。
const userSuppliers = computed(() => aiModels.value.filter((m) => !m.isPreset));

const modelManagerRef = ref<InstanceType<typeof AiModelManager> | null>(null);

// 所选供应商变化（或底层存储重载）时同步右侧编辑缓冲。
// 创建模式下按所选预设（自定义则留空）初始化。
watch([selectedSupplierId, aiModels, creating, createPresetIdx], () => {
  if (creating.value) {
    const preset = createPresetIdx.value >= 0 ? AI_PRESETS[createPresetIdx.value] : null;
    editName.value = preset ? preset.name : "";
    editProvider.value = preset?.provider ?? "openai_compatible";
    editBaseUrl.value = preset?.baseUrl ?? "";
    editProxy.value = "";
    editKey.value = "";
    editTemp.value = 0.7;
    editModels.value = [];
    modelManagerRef.value?.resetTransient();
    return;
  }
  if (selectedSupplier.value) {
    editName.value = selectedSupplier.value.name;
    editProvider.value = selectedSupplier.value.provider;
    editBaseUrl.value = selectedSupplier.value.baseUrl ?? "";
    editProxy.value = selectedSupplier.value.proxyUrl ?? "";
    editTemp.value = selectedSupplier.value.temperature;
    editModels.value = selectedSupplier.value.models;
    editKey.value = "";
    testedOk.value = false;
    modelManagerRef.value?.resetTransient();
  }
});

// 数据加载后自动选中第一个用户供应商（跳过预设）。
watch([aiLoading, userSuppliers, selectedSupplierId, creating], () => {
  if (!aiLoading.value && !creating.value && selectedSupplierId.value === null && userSuppliers.value.length > 0) {
    selectedSupplierId.value = userSuppliers.value[0].id;
  }
});

function showToast(kind: "ok" | "err", text: string): void {
  toast(text, { type: kind === "ok" ? "success" : "error", duration: 4000 });
}

async function handleSaveSupplier(): Promise<void> {
  // 创建模式门槛：至少一个模型 AND 先通过测试。
  if (creating.value) {
    if (editModels.value.length === 0) {
      showToast("err", "请先添加至少一个模型再创建供应商");
      return;
    }
    if (!testedOk.value) {
      showToast("err", "请先点击「测试」通过后再创建");
      return;
    }
  }
  editSaving.value = true;
  const wasCreating = creating.value;
  try {
    const id = await saveAiModel({
      // 无 id = 新建（创建模式）；有 id = 更新已有。
      ...(creating.value ? {} : { id: selectedSupplierId.value ?? undefined }),
      name: editName.value.trim() || "未命名",
      provider: editProvider.value,
      modelId: editModels.value[0]?.modelId ?? "default",
      baseUrl: editBaseUrl.value.trim() || undefined,
      apiKey: editKey.value || undefined,
      proxyUrl: editProxy.value.trim() || undefined,
      temperature: editTemp.value,
      models: editModels.value.map((m) => ({ modelId: m.modelId, label: m.label })),
    });
    editKey.value = "";
    await reloadAi();
    // 退出创建模式 + 选中新创建的供应商。
    creating.value = false;
    selectedSupplierId.value = id;
    showToast("ok", wasCreating ? "供应商已创建" : "供应商已保存");
  } catch (e) {
    showToast("err", `保存失败: ${e}`);
  } finally {
    editSaving.value = false;
  }
}

// 通过覆盖参数测试当前供应商配置 — 不先保存。
async function handleTestSupplier(): Promise<void> {
  editTesting.value = true;
  const run = async (allowVaultKeyToNewHost: boolean): Promise<string> =>
    aiTestSettings({
      supplierId: creating.value ? undefined : selectedSupplierId.value ?? undefined,
      provider: editProvider.value,
      model: editModels.value[0]?.modelId ?? "default",
      baseUrl: editBaseUrl.value.trim() || undefined,
      proxyUrl: editProxy.value.trim() || undefined,
      apiKey: editKey.value,
      temperature: editTemp.value,
      allowVaultKeyToNewHost,
    });
  try {
    let msg: string;
    try {
      msg = await run(false);
    } catch (e) {
      // 后端拒绝把「已保存的 API Key」发往新主机（见 ai.rs test_settings 的
      // 安全确认）。这里显式确认后才带 allow=true 重试——编辑已有供应商时
      // key 输入框本来就是空的，用户看不到自己正在发送什么。
      if (!String(e).includes("安全确认")) throw e;
      const confirmed = await confirmDialog({
        message:
          `${String(e)}\n\n` +
          "继续将把你已保存的 API Key 发送到上面这个地址。若该地址不是你预期的，请取消并检查。",
        title: "确认发送到新主机",
        type: "warning",
      });
      if (!confirmed) {
        showToast("err", "已取消测试：未向新主机发送 API Key");
        testedOk.value = false;
        return;
      }
      msg = await run(true);
    }
    showToast("ok", msg);
    testedOk.value = true;
  } catch (e) {
    showToast("err", `测试失败: ${e}`);
    testedOk.value = false;
  } finally {
    editTesting.value = false;
  }
}

async function handleDeleteSupplier(): Promise<void> {
  if (!selectedSupplier.value) return;
  const ok = await confirmDialog({
    message: `删除供应商「${selectedSupplier.value.name}」？`,
    title: "确认删除",
    type: "warning",
  });
  if (ok) {
    try {
      const target = selectedSupplier.value;
      const wasActive = aiActiveId.value === target.id;
      await deleteAiModel(target.id);
      await reloadAi();
      // 删除的是激活供应商时，自动选中第一个剩余的已启用用户供应商。
      if (wasActive) {
        const remaining = aiModels.value.filter(
          (m) => m.id !== target.id && m.isEnabled && !m.isPreset,
        );
        if (remaining.length > 0) {
          await setActiveAiModel(remaining[0].id);
        }
      }
      selectedSupplierId.value = null;
      showToast("ok", "供应商已删除");
    } catch (e) {
      showToast("err", `删除失败: ${e}`);
    }
  }
}

async function handleRename(m: AiModel, name: string): Promise<void> {
  try {
    await saveAiModel({
      id: m.id,
      name,
      provider: m.provider,
      modelId: m.modelId,
      baseUrl: m.baseUrl,
      temperature: m.temperature,
      models: m.models.map((x) => ({ modelId: x.modelId, label: x.label })),
    });
    await reloadAi();
  } catch {
    /* ignore */
  }
}

async function handleToggle(m: AiModel): Promise<void> {
  try {
    await toggleAiModelEnabled(m.id, !m.isEnabled);
    await reloadAi();
  } catch (e) {
    showToast("err", `操作失败: ${e}`);
  }
}

function startCreating(): void {
  creating.value = true;
  selectedSupplierId.value = null;
  createPresetIdx.value = -1;
}

function cancelCreating(): void {
  creating.value = false;
  selectedSupplierId.value = null;
}

const apiKeyLabel = computed(
  () => `API Key${!creating.value && selectedSupplier.value?.hasKey ? "（已保存，留空保持不变）" : ""}`,
);
const apiKeyPlaceholder = computed(() =>
  creating.value
    ? "粘贴 API key（必填，用于测试和对话）"
    : selectedSupplier.value?.hasKey
      ? "••••••（已保存）"
      : "粘贴 API key",
);
</script>

<template>
  <MySection
    title="AI 助手"
    description="配置 AI 供应商与模型，用于命令生成、输出诊断与服务器巡检。左侧选择供应商，右侧编辑详情。API key 经主密码库加密存储。"
  >
    <div class="ai-layout">
      <!-- ── 左：供应商列表 ── -->
      <AiSupplierList
        :suppliers="userSuppliers"
        :selected-id="selectedSupplierId"
        :loading="aiLoading"
        @select="selectedSupplierId = $event"
        @create="startCreating"
        @toggle="handleToggle"
        @rename="handleRename"
      />

      <!-- ── 右：供应商详情编辑器 ── -->
      <div class="editor-col">
        <div v-if="!selectedSupplier && !creating" class="editor-placeholder">
          选择左侧供应商或新建一个
        </div>
        <div v-else class="editor-form">
          <!-- 创建模式：预设选择器 -->
          <div v-if="creating" class="field">
            <label class="field-label">选择供应商</label>
            <MySelect
              :model-value="createPresetIdx"
              :options="PRESET_OPTIONS"
              @update:model-value="createPresetIdx = $event as number"
            />
          </div>
          <!-- 基本信息 -->
          <div class="grid-2">
            <div class="field">
              <label class="field-label">供应商名称</label>
              <MyInput v-model="editName" placeholder="如：GLM、腾讯云" />
            </div>
            <div class="field">
              <label class="field-label">协议 (Provider)</label>
              <MySelect
                :model-value="editProvider"
                :options="PROVIDER_OPTIONS"
                @update:model-value="editProvider = $event as AiProvider"
              />
            </div>
          </div>
          <div class="field">
            <label class="field-label">API Base URL</label>
            <MyInput v-model="editBaseUrl" placeholder="如 https://open.bigmodel.cn/api/paas/v4" />
          </div>
          <div class="field">
            <label class="field-label">网络代理（可选：http:// 或 socks5://）</label>
            <MyInput v-model="editProxy" placeholder="留空直连；如 http://127.0.0.1:7890" />
          </div>
          <div class="field">
            <label class="field-label">{{ apiKeyLabel }}</label>
            <MyInput
              v-model="editKey"
              type="password"
              :placeholder="apiKeyPlaceholder"
            />
          </div>
          <div class="field">
            <label class="field-label">Temperature（{{ editTemp }}）</label>
            <MySlider v-model="editTemp" :min="0" :max="1" :step="0.1" />
          </div>

          <!-- ── 模型管理 ── -->
          <AiModelManager
            ref="modelManagerRef"
            v-model:models="editModels"
            :provider="editProvider"
            :base-url="editBaseUrl"
            :api-key="editKey"
            :has-key="!!selectedSupplier?.hasKey"
            :supplier-id="selectedSupplierId"
            :creating="creating"
          />

          <!-- ── 操作 ── -->
          <div class="actions">
            <MyButton v-if="creating" variant="secondary" @click="cancelCreating">取消</MyButton>
            <MyButton
              variant="secondary"
              :disabled="editTesting || editSaving"
              @click="handleTestSupplier"
            >
              {{ editTesting ? "测试中…" : "测试" }}
            </MyButton>
            <MyButton
              variant="primary"
              :disabled="editSaving || editTesting"
              @click="handleSaveSupplier"
            >
              {{ editSaving ? "保存中…" : creating ? "创建" : "保存" }}
            </MyButton>
            <MyButton
              v-if="!creating && selectedSupplierId !== null"
              variant="danger"
              @click="handleDeleteSupplier"
            >
              删除
            </MyButton>
          </div>
        </div>
      </div>
    </div>
  </MySection>
</template>

<style scoped>
.ai-layout {
  display: flex;
  gap: 16px;
  min-height: 420px;
}

.editor-col {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
}

.editor-placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-muted);
  font-size: 13px;
}

.editor-form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.grid-2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.field {
  margin-bottom: 0;
}

.field-label {
  display: block;
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 6px;
  font-weight: 500;
}

.actions {
  display: flex;
  align-items: center;
  gap: 10px;
  border-top: 1px solid var(--border-subtle);
  padding-top: 12px;
}
</style>
