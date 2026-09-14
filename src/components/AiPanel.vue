<!-- AI 助手面板（右栏）：多供应商/多模型选择 + 流式对话 + 服务器/本机巡检。
     自 src-legacy/components/AiPanel.tsx 原样移植：
     - 终端读取/粘贴不再经 props.getTerminal，改用 store/sessions 的 getTerminal；
     - markdown 由 react-markdown+remark-gfm 换为 markdown-it（html:false 保持防 XSS 语义）；
     - 宽度拖拽 emit width-change，由壳层（ui.aiPanelWidth）持久化。 -->
<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import { ChatDotRound, Link, Search } from "@element-plus/icons-vue";
import type { Terminal } from "@xterm/xterm";
import type { UnlistenFn } from "@tauri-apps/api/event";
import MarkdownIt from "markdown-it";
import {
  aiChat,
  aiInspectHealthLocal,
  aiInspectHealthSsh,
  getActiveAiSelection,
  listAiModels,
  onAiDone,
  onAiError,
  onAiToken,
  setActiveAiModel,
  type AiContext,
  type AiModel,
  type ChatMessage,
  type ConnType,
  type SupplierModel,
} from "@/api";
import { getTerminal } from "@/store/sessions";

defineOptions({ name: "AiPanel" });

interface Props {
  /** Active tab context. connType drives the shell hint; sessionId + getTerminal
   * let the panel read terminal output/selection and paste commands. */
  activeConnType?: ConnType;
  activeConnectionName?: string;
  activeSessionId?: string;
  width: number;
}

const props = defineProps<Props>();
const emit = defineEmits<{ close: []; "width-change": [w: number] }>();

interface Msg {
  id: string;
  role: "user" | "assistant";
  content: string;
  /** Context snapshot attached at send time — shown above the user message
   * as a muted snippet so the user sees what was sent to the AI. */
  selection?: string;
  error?: boolean;
  streaming?: boolean;
}

function newId(): string {
  try {
    return crypto.randomUUID();
  } catch {
    return Math.random().toString(36).slice(2) + Date.now().toString(36);
  }
}

function shellHintFor(connType?: ConnType): string {
  return connType === "local" ? "powershell" : "bash";
}

/** Provider display label for the model switcher. */
function providerLabel(provider: string): string {
  switch (provider) {
    case "claude":
      return "Claude";
    case "openai":
      return "OpenAI";
    case "ollama":
      return "Ollama";
    case "openai_compatible":
      return "OpenAI兼容";
    case "anthropic_compatible":
      return "Anthropic兼容";
    default:
      return provider;
  }
}

/** Read the last `n` non-empty lines from the normal buffer (skips blank
 * trailing rows). In an alternate buffer (vim/claude TUI) scrollback is empty,
 * so this just returns the current screen — fine for "what's on screen". */
function readRecentLines(term: Terminal, n: number): string {
  const buf = term.buffer.active;
  const total = buf.length;
  const lines: string[] = [];
  for (let i = Math.max(0, total - n); i < total; i++) {
    const line = buf.getLine(i);
    if (line) {
      const s = line.translateToString(true);
      if (s.trim()) lines.push(s);
    }
  }
  return lines.slice(-n).join("\n");
}

/** Cap auto-attached recent terminal output so we never flood the system
 * prompt. Keeps the most recent `max` characters and stamps a truncation note
 * at the top when something was dropped. User selections are NOT capped — the
 * user explicitly chose that text, so we send it verbatim. */
const RECENT_OUTPUT_MAX = 5000;
function capRecentOutput(s: string): string {
  if (s.length <= RECENT_OUTPUT_MAX) return s;
  const dropped = s.length - RECENT_OUTPUT_MAX;
  return (
    `…（已省略较早的 ${dropped} 个字符，仅保留最近输出）\n` +
    s.slice(s.length - RECENT_OUTPUT_MAX)
  );
}

// ── markdown 渲染（markdown-it，语义对齐旧版 react-markdown + remark-gfm） ──
// 安全语义（保留旧版）：html:false —— 模型输出里的原始 HTML 一律转义为纯文本，
// 不进入 DOM；markdown-it 内置 validateLink 亦会拦截 javascript:/vbscript: 等危险
// 协议链接（旧版 react-markdown 的 defaultUrlTransform 同样拦截）。因此 v-html 是安全的。
const md = new MarkdownIt({ html: false, linkify: true, breaks: false, typographer: false });

// 旧版 quirk（保留）：react-markdown 里只有带 language-xxx 的围栏代码块才走"块级代码"
// 分支；无语言标注的围栏/缩进代码块会退化为内联 code 样式。这里用空 info 判定复现。
function renderInlineCode(content: string): string {
  return `<code class="ai-code-inline">${md.utils.escapeHtml(content)}</code>`;
}

md.renderer.rules.code_block = (tokens, idx) => renderInlineCode(tokens[idx].content);

md.renderer.rules.fence = (tokens, idx, _options, env) => {
  const token = tokens[idx];
  const info = token.info.trim();
  if (!info) return renderInlineCode(token.content);
  // 旧版仅给带语言标注的块渲染"插入终端 / 复制"操作按钮
  const pasteBtn = env.canPaste
    ? `<button type="button" class="ai-code-btn" data-action="paste" title="把命令插入当前终端（需自行按回车执行）">插入终端</button>`
    : "";
  return (
    `<div class="ai-code-block"><div class="ai-code-actions">${pasteBtn}` +
    `<button type="button" class="ai-code-btn" data-action="copy">复制</button></div>` +
    `<pre class="ai-code-pre"><code>${md.utils.escapeHtml(token.content)}</code></pre></div>`
  );
};

// ── 多模型状态（选择器只展示已启用的供应商） ──
const aiModels = ref<AiModel[]>([]);
const aiActiveId = ref<number | null>(null);
// The specific model string chosen within the active supplier (null = the
// supplier's primary). Mirrors ai_settings.active_model_string so the label
// and dropdown highlight match what the backend actually uses.
const aiActiveModelString = ref<string | null>(null);
const modelPickerOpen = ref(false);

function refreshAiModels(): void {
  listAiModels()
    .then((v) => {
      aiModels.value = v;
    })
    .catch(() => {});
  getActiveAiSelection()
    .then((sel) => {
      aiActiveId.value = sel.id ?? null;
      aiActiveModelString.value = sel.modelString ?? null;
    })
    .catch(() => {});
}
refreshAiModels();
// Re-fetch every time the picker opens — models added/removed in 设置 must
// be selectable immediately, without restarting the app.
watch(modelPickerOpen, (open) => {
  if (open) refreshAiModels();
});

const enabledModels = computed(() => aiModels.value.filter((m) => m.isEnabled && !m.isPreset));
const activeModel = computed(
  () => enabledModels.value.find((m) => m.id === aiActiveId.value) ?? enabledModels.value[0],
);
// Label the exact model in use: the chosen string when it belongs to the
// active supplier, otherwise the supplier's primary modelId.
const activeModelIdLabel = computed(() =>
  activeModel.value && activeModel.value.id === aiActiveId.value
    ? aiActiveModelString.value ?? activeModel.value.modelId
    : activeModel.value?.modelId,
);
const pickerLabel = computed(() =>
  activeModel.value
    ? `${activeModel.value.name}${activeModelIdLabel.value ? ` / ${activeModelIdLabel.value}` : ""}`
    : "选择模型",
);

const messages = ref<Msg[]>([]);
// Vue 的 ref 读取是实时的：异步回调里读 messages.value 即最新会话历史
// （对应旧版用 messagesRef 镜像规避 React setState 异步的写法）。
const input = ref("");
const streaming = ref(false);
const attachedSelection = ref<string | null>(null);
const scrollRef = ref<HTMLDivElement | null>(null);
// 当前进行中的流式请求：reqId + 事件退订列表（非响应式，同旧版 useRef）。
let activeReq: { reqId: string; unlistens: UnlistenFn[] } | null = null;

function cancelSubscriptions(): void {
  activeReq?.unlistens.forEach((u) => u());
  activeReq = null;
}

// Cancel any in-flight subscription on unmount.
onUnmounted(() => {
  cancelSubscriptions();
});

// Auto-scroll to bottom as tokens arrive.
watch(messages, () => {
  const el = scrollRef.value;
  if (el) el.scrollTop = el.scrollHeight;
}, { flush: "post" });

const activeTerm = computed(() =>
  props.activeSessionId ? getTerminal(props.activeSessionId) : undefined,
);

// Pull the active terminal's selection (if any) or the last ~40 lines as
// context for the next request. Returned as an AiContext for the backend
// to fold into the system prompt.
function collectContext(): AiContext {
  const ctx: AiContext = { shellHint: shellHintFor(props.activeConnType), connType: props.activeConnType };
  if (attachedSelection.value) {
    ctx.selection = attachedSelection.value;
    return ctx;
  }
  const term = activeTerm.value;
  if (term) {
    const sel = term.getSelection();
    if (sel && sel.trim()) ctx.selection = sel;
    else {
      const out = readRecentLines(term, 40);
      if (out) ctx.terminalOutput = capRecentOutput(out);
    }
  }
  return ctx;
}

/** Snapshot the selection/context that will be sent alongside this message.
 * Used to render a preview inside the user bubble so the user can see
 * exactly what context the AI received. */
function snapshotSelection(): string | undefined {
  if (attachedSelection.value) return attachedSelection.value;
  const term = activeTerm.value;
  if (term) {
    const sel = term.getSelection();
    if (sel && sel.trim()) return sel;
    const out = readRecentLines(term, 40);
    if (out) return capRecentOutput(out);
  }
  return undefined;
}

function pasteToTerminal(text: string): void {
  // term.paste() drives xterm's onData → the same path as the user typing,
  // so the command shows up in the terminal for the user to review and
  // press Enter (we deliberately don't auto-run AI output).
  activeTerm.value?.paste(text);
}

/** 订阅某个 reqId 的 token/done/error 三事件，返回退订列表。 */
async function subscribeStream(
  reqId: string,
  finish: () => void,
): Promise<UnlistenFn[]> {
  return Promise.all([
    onAiToken(reqId, (tok) => {
      messages.value = messages.value.map((m) =>
        m.id === reqId ? { ...m, content: m.content + tok } : m,
      );
    }),
    onAiDone(reqId, finish),
    onAiError(reqId, (err) => {
      messages.value = messages.value.map((m) =>
        m.id === reqId ? { ...m, content: err, error: true, streaming: false } : m,
      );
      finish();
    }),
  ]);
}

function cancelActive(): void {
  cancelSubscriptions();
  streaming.value = false;
  messages.value = messages.value.map((m) => (m.streaming ? { ...m, streaming: false } : m));
}

function send(): void {
  const text = input.value.trim();
  if (!text || streaming.value) return;
  const reqId = newId();
  const sel = snapshotSelection();
  const userMsg: Msg = { id: `${reqId}-u`, role: "user", content: text, selection: sel };
  const aiMsg: Msg = { id: reqId, role: "assistant", content: "", streaming: true };
  messages.value = [...messages.value, userMsg, aiMsg];
  input.value = "";
  attachedSelection.value = null;
  streaming.value = true;

  cancelSubscriptions();

  const finish = (): void => {
    streaming.value = false;
    cancelSubscriptions();
    messages.value = messages.value.map((m) => (m.id === reqId ? { ...m, streaming: false } : m));
  };

  void subscribeStream(reqId, finish).then((subs) => {
    activeReq = { reqId, unlistens: subs };

    const context = collectContext();
    // Build the FULL conversation history to send to the backend. Previously
    // this only sent `[payload]` (the latest user turn), so the AI had zero
    // memory of earlier turns — the reported "no context continuity" bug.
    // We read the live messages (not a stale copy) and filter out
    // failed/incomplete turns: an errored assistant reply or a still-streaming
    // bubble carries no usable answer to build on. 与 React 版逐字一致：本轮
    // user 消息已在上方追加进列表，因此历史末尾会再补一条相同输入（重复一次），
    // 这是旧版既有线上行为，移植时不做"修复"。
    const history: ChatMessage[] = messages.value
      .filter((m) => !m.error && !m.streaming && m.content.trim().length > 0)
      .map((m) => ({ role: m.role, content: m.content }));
    history.push({ role: "user", content: text });
    aiChat(reqId, history, null, context).catch((e) => {
      messages.value = messages.value.map((m) =>
        m.id === reqId ? { ...m, content: `请求失败: ${e}`, error: true, streaming: false } : m,
      );
      finish();
    });
  });
}

// Health inspection: the backend runs a preset read-only script (SSH over
// exec_once, local via the OS shell) and streams an AI health report over
// the same ai_token/ai_done/ai_error events as a normal chat. SSH needs a
// sessionId; local inspects the user's own machine. sftp/ftp tabs have no
// shell, so the button only shows for ssh/local.
function runInspection(): void {
  if (streaming.value) return;
  const reqId = newId();
  const isSsh = props.activeConnType === "ssh" && !!props.activeSessionId;
  const userMsg: Msg = {
    id: `${reqId}-u`,
    role: "user",
    content: isSsh ? "正在采集服务器指标并生成健康报告…" : "正在采集本机指标并生成健康报告…",
  };
  const aiMsg: Msg = { id: reqId, role: "assistant", content: "", streaming: true };
  messages.value = [...messages.value, userMsg, aiMsg];
  attachedSelection.value = null;
  streaming.value = true;

  cancelSubscriptions();
  const finish = (): void => {
    streaming.value = false;
    cancelSubscriptions();
    messages.value = messages.value.map((m) => (m.id === reqId ? { ...m, streaming: false } : m));
  };
  void subscribeStream(reqId, finish).then((subs) => {
    activeReq = { reqId, unlistens: subs };

    const kickoff = isSsh
      ? aiInspectHealthSsh(props.activeSessionId!, reqId)
      : aiInspectHealthLocal(reqId);
    kickoff.catch((e) => {
      messages.value = messages.value.map((m) =>
        m.id === reqId
          ? { ...m, content: `巡检失败: ${e}`, error: true, streaming: false }
          : m,
      );
      finish();
    });
  });
}

function attachSelection(): void {
  const term = activeTerm.value;
  if (!term) return;
  const sel = term.getSelection();
  if (sel && sel.trim()) attachedSelection.value = sel;
}

function onKeyDown(e: KeyboardEvent): void {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    send();
  }
}

function onResizeStart(e: MouseEvent): void {
  e.preventDefault();
  const startX = e.clientX;
  const startW = props.width;
  const onMove = (ev: MouseEvent): void => {
    const w = Math.min(720, Math.max(300, startW - (ev.clientX - startX)));
    emit("width-change", w);
  };
  const onUp = (): void => {
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
  };
  document.addEventListener("mousemove", onMove);
  document.addEventListener("mouseup", onUp);
}

function copy(text: string): void {
  navigator.clipboard?.writeText(text).catch(() => {});
}

const ctxLabel = computed(
  () => props.activeConnectionName ?? (props.activeConnType ? `${props.activeConnType} 终端` : "未连接"),
);

// markdown 代码块操作按钮走事件委托（v-html 内容无法直接绑 Vue 事件）：
// 复制/插入的文本 = 代码内容去掉末尾一个换行（与旧版一致）。
function onMdActionClick(e: MouseEvent): void {
  const target = e.target as HTMLElement | null;
  const btn = target?.closest("button.ai-code-btn");
  if (!(btn instanceof HTMLElement)) return;
  const wrap = btn.closest(".ai-code-block");
  const codeEl = wrap?.querySelector("code");
  if (!wrap || !codeEl) return;
  const text = (codeEl.textContent ?? "").replace(/\n$/, "");
  if (btn.dataset.action === "paste") pasteToTerminal(text);
  else copy(text);
}

// 用户气泡附带的上下文快照：双击展开/收起（按 msg.id 记录，初始收起，同旧版局部 state）。
const expandedSels = ref(new Set<string>());
function toggleSelExpand(id: string): void {
  const next = new Set(expandedSels.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  expandedSels.value = next;
}

function mdHtml(content: string): string {
  // canPaste 参与渲染依赖收集：终端挂载/切换后重渲染，代码块按钮随 canPaste 增减
  // （与旧版 MessageBubble 每次渲染读取 canPaste 一致）。
  return md.render(content, { canPaste: !!activeTerm.value });
}

// ── 模型选择器（两步：供应商 → 模型） ──
const selectedSupplierIdx = ref(0);
// Clamp index when the list changes.
const safeIdx = computed(() => Math.min(selectedSupplierIdx.value, Math.max(0, enabledModels.value.length - 1)));
const pickerSupplier = computed(() => enabledModels.value[safeIdx.value]);
const pickerModelList = computed(() =>
  pickerSupplier.value?.models?.length ? pickerSupplier.value.models : null,
);
// Highlight the exact chosen model; without a chosen string the
// supplier's primary is the active one.
function modelRowActive(sm: SupplierModel): boolean {
  const supplier = pickerSupplier.value;
  if (!supplier) return false;
  return supplier.id === aiActiveId.value && sm.modelId === (aiActiveModelString.value || supplier.modelId);
}

function onSelectModel(supplierId: number, modelString: string): void {
  setActiveAiModel(supplierId, modelString)
    .then(() => {
      aiActiveId.value = supplierId;
      aiActiveModelString.value = modelString;
    })
    .catch(() => {});
  modelPickerOpen.value = false;
}
</script>

<template>
  <div class="ai-panel" :style="{ width: `${width}px` }">
    <div class="resize-handle" @mousedown="onResizeStart" />

    <!-- Header -->
    <header class="panel-header">
      <div class="header-titles">
        <span class="panel-title">
          <el-icon class="title-icon" :size="14"><ChatDotRound /></el-icon>
          AI 助手
        </span>
        <span class="ctx-label" :title="ctxLabel">上下文：{{ ctxLabel }}</span>
      </div>
      <button
        v-if="activeConnType === 'ssh' || activeConnType === 'local'"
        type="button"
        class="inspect-btn"
        :disabled="streaming"
        title="采集服务器/本机指标，让 AI 出健康报告"
        @click="runInspection"
      >
        <el-icon :size="12"><Search /></el-icon>
        巡检
      </button>
      <button type="button" class="close-btn" title="关闭" @click="emit('close')">×</button>
    </header>

    <!-- Messages -->
    <div ref="scrollRef" class="messages" @click="onMdActionClick">
      <div v-if="messages.length === 0" class="empty-hint">
        向 AI 描述你想做的事，比如：<br />· “生成一个查找最大文件的命令”<br />· “解释 awk -F: {print $1} /etc/passwd”<br />· 选中终端里的报错，点下方“附带选区”让 AI 排查
      </div>
      <div
        v-for="m in messages"
        :key="m.id"
        class="bubble-row"
        :class="{ 'from-user': m.role === 'user' }"
      >
        <div class="bubble" :class="{ 'is-user': m.role === 'user', 'is-error': m.error === true }">
          <div
            v-if="m.role === 'user' && m.selection"
            class="sel-snap"
            :class="{ expanded: expandedSels.has(m.id) }"
            :title="expandedSels.has(m.id) ? '双击收起为单行' : '双击展开完整内容'"
            @dblclick="toggleSelExpand(m.id)"
          >{{ expandedSels.has(m.id) ? m.selection : m.selection.replace(/\s+/g, " ").trim() }}</div>
          <div v-if="m.role === 'user'" class="bubble-text">{{ m.content }}</div>
          <span v-else-if="m.content === '' && m.streaming" class="thinking">思考中…</span>
          <!-- markdown-it html:false：模型输出的原始 HTML 一律转义，防 XSS（保留旧版安全语义） -->
          <!-- eslint-disable-next-line vue/no-v-html -->
          <div v-else class="ai-markdown" v-html="mdHtml(m.content)" />
        </div>
      </div>
    </div>

    <!-- Input -->
    <div class="input-bar">
      <div v-if="attachedSelection" class="attach-chip">
        <span class="attach-text">
          <el-icon class="attach-icon" :size="12"><Link /></el-icon>
          已附带选区：{{ attachedSelection.slice(0, 40) }}{{ attachedSelection.length > 40 ? "…" : "" }}
        </span>
        <button type="button" class="attach-remove" @click="attachedSelection = null">×</button>
      </div>
      <div class="input-wrap">
        <textarea
          v-model="input"
          class="input-textarea"
          rows="4"
          title="Enter 发送
Shift+Enter 换行"
          placeholder="描述你想做的事…
Enter 发送 ● Shift+Enter 换行"
          @keydown="onKeyDown"
        />
        <button v-if="streaming" type="button" class="stop-btn" title="停止生成" @click="cancelActive">■</button>
      </div>
      <div class="input-toolbar">
        <button
          type="button"
          class="attach-btn"
          :disabled="!activeTerm"
          :title="activeTerm ? '把终端里选中的文本作为上下文' : '当前无活动终端'"
          @click="attachSelection"
        >
          <el-icon :size="12"><Link /></el-icon>
          附带选区
        </button>
        <span class="toolbar-hint">{{ activeTerm ? "将自动附带最近输出" : "AI 命令请确认后再执行" }}</span>
      </div>
      <!-- Two-step model picker — supplier → model. -->
      <div class="model-picker">
        <button
          type="button"
          class="picker-toggle"
          title="点击切换 AI 模型（先选供应商，再选模型）"
          @click="modelPickerOpen = !modelPickerOpen"
        >
          <span class="picker-label">{{ pickerLabel }}</span>
          <span v-if="activeModel" class="provider-chip">{{ providerLabel(activeModel.provider) }}</span>
          <span class="picker-arrow">{{ modelPickerOpen ? "▲" : "▼" }}</span>
        </button>
        <div v-if="modelPickerOpen" class="picker-dropdown">
          <!-- Left: supplier column -->
          <div class="supplier-col">
            <div
              v-for="(m, idx) in enabledModels"
              :key="m.id"
              class="supplier-row"
              :class="{ chosen: idx === safeIdx, active: m.id === aiActiveId }"
              @click="selectedSupplierIdx = idx"
            >
              <span class="supplier-name">{{ m.name }}</span>
              <span v-if="!m.hasKey" class="need-key">需</span>
            </div>
          </div>
          <!-- Right: model column -->
          <div class="model-col">
            <div v-if="!pickerSupplier" class="model-empty">请先选择供应商</div>
            <div v-else-if="!pickerModelList || pickerModelList.length === 0" class="model-empty">该供应商暂无额外模型</div>
            <template v-else>
              <div
                v-for="sm in pickerModelList"
                :key="sm.id || sm.modelId"
                class="model-row"
                :class="{ active: modelRowActive(sm) }"
                :title="sm.modelId"
                @click="onSelectModel(pickerSupplier.id, sm.modelId)"
              >
                <span class="model-name">{{ sm.label ?? sm.modelId }}</span>
                <span v-if="sm.label" class="model-id">{{ sm.modelId }}</span>
                <span v-if="modelRowActive(sm)" class="model-check">✓</span>
              </div>
            </template>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 旧版对应组件为全内联样式，此处逐条转为 scoped CSS；色值一律走令牌。 */

.ai-panel {
  flex-shrink: 0;
  height: 100%;
  display: flex;
  flex-direction: column;
  /* 旧版即引用未定义令牌（--bg-sidebar 在两版主题里均未定义 → 解析为透明）。
     逐字保留以维持行为一致；若壳层日后补充该令牌则自动生效。 */
  background: var(--bg-sidebar);
  position: relative;
}

/* 面板内按钮基础态：对应旧版 global.css 的 button 重置（无边框/手型） */
.ai-panel button {
  border: none;
  cursor: pointer;
  font-family: inherit;
  font-size: inherit;
}

.resize-handle {
  position: absolute;
  left: -3px;
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: col-resize;
  z-index: 5;
}

/* ── Header ── */
.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border-default);
  flex-shrink: 0;
}

.header-titles {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.panel-title {
  font-weight: 600;
  color: var(--text-primary);
  font-size: 13px;
  display: inline-flex;
  align-items: center;
  gap: 5px;
}

.panel-title .title-icon {
  color: var(--accent-primary);
}

.ctx-label {
  font-size: 11px;
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.inspect-btn {
  background: var(--bg-surface);
  color: var(--text-secondary);
  border: 1px solid var(--border-default);
  border-radius: 6px;
  padding: 4px 10px;
  font-size: 12px;
  cursor: pointer;
  margin-right: 4px;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.close-btn {
  background: transparent;
  color: var(--text-secondary);
  font-size: 18px;
  line-height: 1;
  padding: 4px 8px;
  border-radius: 6px;
}

/* ── Messages ── */
.messages {
  flex: 1;
  overflow-y: auto;
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.empty-hint {
  color: var(--text-tertiary);
  font-size: 13px;
  line-height: 1.7;
}

.bubble-row {
  display: flex;
  justify-content: flex-start;
}

.bubble-row.from-user {
  justify-content: flex-end;
}

.bubble {
  max-width: 92%;
  padding: 10px 12px;
  border-radius: 10px;
  background: var(--bg-surface);
  border: 1px solid var(--border-subtle);
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.6;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.bubble.is-user {
  padding: 8px 12px;
  background: var(--accent-primary-muted);
}

.bubble.is-error {
  background: var(--error-muted);
  border: 1px solid color-mix(in srgb, var(--error) 40%, transparent);
  color: var(--error);
}

.sel-snap {
  font-size: 11px;
  line-height: 1.5;
  color: var(--text-tertiary);
  background: var(--bg-base);
  border: 1px solid var(--border-subtle);
  border-radius: 6px;
  padding: 6px 8px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: "Cascadia Code", Consolas, monospace;
  cursor: default;
  user-select: none;
}

.sel-snap.expanded {
  white-space: pre-wrap;
  overflow: auto;
  word-break: break-word;
  max-height: 240px;
}

.bubble-text {
  white-space: pre-wrap;
  word-break: break-word;
}

.thinking {
  color: var(--text-muted);
}

/* markdown 正文：旧版依赖 global.css 的 `* { margin:0; padding:0 }` 全局重置，
   新壳层没有该重置，这里在正文作用域内补齐等价规则（保持排版零变化）。 */
.ai-markdown :deep(*) {
  margin: 0;
  padding: 0;
}

.ai-markdown :deep(code.ai-code-inline) {
  background: var(--bg-surface-active);
  padding: 1px 5px;
  border-radius: 4px;
  font-family: "Cascadia Code", Consolas, monospace;
  font-size: 12px;
}

.ai-markdown :deep(.ai-code-block) {
  position: relative;
  margin: 8px 0;
  border-radius: 8px;
  overflow: hidden;
  border: 1px solid var(--border-default);
}

.ai-markdown :deep(.ai-code-actions) {
  position: absolute;
  top: 6px;
  right: 6px;
  z-index: 2;
  display: flex;
  gap: 4px;
}

.ai-markdown :deep(.ai-code-btn) {
  background: var(--bg-surface-active);
  color: var(--text-secondary);
  border: none;
  border-radius: 5px;
  padding: 3px 8px;
  font-size: 11px;
  cursor: pointer;
  font-family: inherit;
}

.ai-markdown :deep(.ai-code-pre) {
  margin: 0;
  padding: 10px 12px;
  overflow-x: auto;
  background: var(--bg-base);
}

.ai-markdown :deep(.ai-code-pre code) {
  font-family: "Cascadia Code", Consolas, monospace;
  font-size: 12px;
}

/* ── Input ── */
.input-bar {
  padding: 12px;
  border-top: 1px solid var(--border-default);
  flex-shrink: 0;
}

.attach-chip {
  font-size: 11px;
  color: var(--text-secondary);
  background: var(--accent-primary-muted);
  border: 1px solid var(--border-accent);
  border-radius: 6px;
  padding: 4px 8px;
  margin-bottom: 6px;
  display: flex;
  align-items: center;
  gap: 6px;
}

.attach-text {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.attach-text .attach-icon {
  flex-shrink: 0;
}

.attach-remove {
  background: transparent;
  border: none;
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 13px;
}

.input-wrap {
  position: relative;
}

.input-textarea {
  width: 100%;
  resize: vertical;
  background: var(--bg-input);
  color: var(--text-primary);
  border: 1px solid var(--border-default);
  border-radius: 8px;
  padding: 8px 10px;
  font-size: 13px;
  line-height: 1.5;
  font-family: inherit;
  outline: none;
  min-height: 48px;
  max-height: 300px;
  box-sizing: border-box;
  display: block;
}

.stop-btn {
  position: absolute;
  right: 8px;
  bottom: 8px;
  width: 28px;
  height: 28px;
  background: var(--error-muted);
  border: 1px solid var(--error);
  border-radius: 6px;
  color: var(--error);
  font-size: 12px;
  line-height: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition: all 0.2s cubic-bezier(0.32, 0.72, 0, 1);
}

.input-toolbar {
  display: flex;
  justify-content: space-between;
  margin-top: 6px;
}

.attach-btn {
  background: transparent;
  border: none;
  color: var(--text-secondary);
  font-size: 11px;
  cursor: pointer;
  padding: 0;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.attach-btn:disabled {
  color: var(--text-muted);
  cursor: default;
}

.toolbar-hint {
  font-size: 11px;
  color: var(--text-muted);
}

/* ── 模型选择器 ── */
.model-picker {
  position: relative;
  margin-top: 6px;
  border-top: 1px solid var(--border-subtle);
  padding-top: 6px;
}

.picker-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 5px 8px;
  cursor: pointer;
  text-align: left;
}

.picker-label {
  flex: 1;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.provider-chip {
  font-size: 10px;
  padding: 1px 5px;
  border-radius: var(--radius-full);
  background: var(--bg-surface);
  color: var(--text-tertiary);
  border: 1px solid var(--border-default);
}

.picker-arrow {
  font-size: 10px;
  color: var(--text-tertiary);
}

.picker-dropdown {
  position: absolute;
  bottom: 100%;
  left: 0;
  right: 0;
  margin-bottom: 4px;
  max-height: 280px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-md);
  z-index: 10;
  display: flex;
  overflow: hidden;
}

.supplier-col {
  width: 140px;
  flex-shrink: 0;
  overflow-y: auto;
  border-right: 1px solid var(--border-subtle);
  background: var(--bg-surface);
}

.supplier-row {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px 8px;
  cursor: pointer;
  background: transparent;
  border-bottom: 1px solid var(--border-subtle);
}

.supplier-row.active {
  background: var(--bg-surface-hover);
}

.supplier-row.chosen {
  background: var(--accent-primary-muted);
}

.supplier-name {
  flex: 1;
  font-size: 11px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.supplier-row.chosen .supplier-name {
  color: var(--accent-primary);
  font-weight: 600;
}

.need-key {
  font-size: 9px;
  color: var(--warning, #fab005);
}

.model-col {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
}

.model-empty {
  padding: 12px;
  font-size: 11px;
  color: var(--text-muted);
}

.model-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  cursor: pointer;
  background: transparent;
  border-bottom: 1px solid var(--border-subtle);
}

.model-row.active {
  background: var(--accent-primary-muted);
}

.model-name {
  flex: 1;
  font-size: 11px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.model-row.active .model-name {
  color: var(--accent-primary);
  font-weight: 600;
}

.model-id {
  font-size: 9px;
  color: var(--text-muted);
  max-width: 80px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.model-check {
  font-size: 10px;
  color: var(--accent-primary);
}
</style>
