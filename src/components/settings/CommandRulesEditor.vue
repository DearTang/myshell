<!-- 命令确认规则编辑器 — 控制 ssh_exec 哪些命令弹人工确认的黑/白名单正则
     （从旧 mcp 分区拆出）。active 语义沿用旧版：每次切入 MCP 类目重新拉取。 -->
<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { Search } from "@element-plus/icons-vue";
import { MyButton, MyCheckbox, MyInput } from "myui";
import { getCommandRules, setCommandRules, type CommandRules } from "@/api";

defineOptions({ name: "CommandRulesEditor" });

const props = defineProps<{ active: boolean }>();

// ── 命令确认规则（白名单/黑名单正则）──
const rulesBlacklist = ref<string[]>([]);
const rulesWhitelist = ref<string[]>([]);
const rulesConfirmUnknown = ref(false);
const rulesShowInGui = ref(true);
const rulesSaving = ref(false);
// 列表式规则编辑器的搜索 + 添加输入状态。
const blacklistSearch = ref("");
const blacklistInput = ref("");
const whitelistSearch = ref("");
const whitelistInput = ref("");

function load(): void {
  getCommandRules()
    .then((r) => {
      rulesBlacklist.value = r.blacklist;
      rulesWhitelist.value = r.whitelist;
      rulesConfirmUnknown.value = r.confirm_unknown;
      rulesShowInGui.value = r.show_in_gui;
    })
    .catch(() => {});
}

onMounted(load);

watch(
  () => props.active,
  (active) => {
    if (active) load();
  },
);

/** 把编辑器内容解析为 CommandRules 并持久化。 */
async function saveCommandRules(): Promise<void> {
  rulesSaving.value = true;
  try {
    const rules: CommandRules = {
      blacklist: rulesBlacklist.value,
      whitelist: rulesWhitelist.value,
      confirm_unknown: rulesConfirmUnknown.value,
      show_in_gui: rulesShowInGui.value,
    };
    await setCommandRules(rules);
    // Tell the confirm dialog to re-read. It used to fetch the rules once at
    // mount, so a rule added here was not reflected in the highlighting for
    // the rest of the session.
    window.dispatchEvent(new Event("myshell-command-rules-changed"));
  } catch (e) {
    console.error("saveCommandRules failed:", e);
  } finally {
    rulesSaving.value = false;
  }
}

/** 清空编辑（仅本地编辑态；点"保存规则"前不落盘）。 */
function resetRuleEdits(): void {
  rulesBlacklist.value = [];
  rulesWhitelist.value = [];
  rulesConfirmUnknown.value = false;
}

interface RuleListVM {
  key: "blacklist" | "whitelist";
  title: string;
  tone: "error" | "success";
  maxHeight: number;
}

const ruleLists: RuleListVM[] = [
  {
    key: "blacklist",
    title: "黑名单（命中则确认，除非白名单豁免）",
    tone: "error",
    maxHeight: 200,
  },
  {
    key: "whitelist",
    title: "白名单豁免（命中则免确认，优先级高于黑名单）",
    tone: "success",
    maxHeight: 150,
  },
];

function rulesOf(key: RuleListVM["key"]): string[] {
  return key === "blacklist" ? rulesBlacklist.value : rulesWhitelist.value;
}

function setRules(key: RuleListVM["key"], rules: string[]): void {
  if (key === "blacklist") rulesBlacklist.value = rules;
  else rulesWhitelist.value = rules;
}

function searchOf(key: RuleListVM["key"]): string {
  return key === "blacklist" ? blacklistSearch.value : whitelistSearch.value;
}

function setSearch(key: RuleListVM["key"], v: string): void {
  if (key === "blacklist") blacklistSearch.value = v;
  else whitelistSearch.value = v;
}

function inputOf(key: RuleListVM["key"]): string {
  return key === "blacklist" ? blacklistInput.value : whitelistInput.value;
}

function setInput(key: RuleListVM["key"], v: string): void {
  if (key === "blacklist") blacklistInput.value = v;
  else whitelistInput.value = v;
}

function addRule(key: RuleListVM["key"]): void {
  const v = inputOf(key).trim();
  if (v && !rulesOf(key).includes(v)) {
    setRules(key, [...rulesOf(key), v].sort());
  }
  setInput(key, "");
}

function addRuleOnEnter(key: RuleListVM["key"], e: KeyboardEvent): void {
  if (e.key === "Enter" && inputOf(key).trim()) addRule(key);
}

function removeRule(key: RuleListVM["key"], rule: string): void {
  setRules(
    key,
    rulesOf(key).filter((r) => r !== rule),
  );
}

function filteredRules(key: RuleListVM["key"]): string[] {
  const search = searchOf(key).toLowerCase();
  return rulesOf(key).filter((r) => !search || r.toLowerCase().includes(search));
}
</script>

<template>
  <div class="rules-block">
    <div class="block-head">
      <div class="block-title">命令确认规则</div>
      <div class="block-actions">
        <MyButton variant="secondary" size="small" @click="resetRuleEdits">清空编辑</MyButton>
        <MyButton variant="primary" size="small" :disabled="rulesSaving" @click="saveCommandRules">
          {{ rulesSaving ? "保存中..." : "保存规则" }}
        </MyButton>
      </div>
    </div>

    <div class="rules-intro">
      控制 <code>ssh_exec</code> 执行哪些命令时弹人工确认对话框。每行一条正则表达式（大小写不敏感）。
    </div>

    <!-- confirm_unknown 开关 -->
    <MyCheckbox
      :model-value="rulesConfirmUnknown"
      label="未匹配任何规则的命令也需要确认（严格模式，默认关闭）"
      class="rules-check"
      @update:model-value="rulesConfirmUnknown = Boolean($event)"
    />

    <!-- show_in_gui 开关 -->
    <MyCheckbox
      :model-value="rulesShowInGui"
      label="ssh_exec 命令在界面终端同步展示（关闭则后台静默执行，默认开启）"
      class="rules-check"
      @update:model-value="rulesShowInGui = Boolean($event)"
    />

    <template v-for="list in ruleLists" :key="list.key">
      <div class="rule-block">
        <div class="rule-title" :class="list.tone">{{ list.title }}</div>
        <div class="rule-input-row">
          <input
            class="rule-input"
            :value="inputOf(list.key)"
            @input="setInput(list.key, ($event.target as HTMLInputElement).value)"
            @keydown="addRuleOnEnter(list.key, $event)"
            placeholder="输入正则后回车添加…"
          />
          <MyButton variant="secondary" size="small" @click="addRule(list.key)">添加</MyButton>
        </div>
        <MyInput
          v-if="rulesOf(list.key).length > 3"
          class="rule-search"
          size="small"
          :model-value="searchOf(list.key)"
          placeholder="搜索…"
          @update:model-value="setSearch(list.key, $event)"
        >
          <template #prefix>
            <el-icon :size="12"><Search /></el-icon>
          </template>
        </MyInput>
        <div class="rule-listbox" :style="{ maxHeight: list.maxHeight + 'px' }">
          <div
            v-for="(rule, i) in filteredRules(list.key)"
            :key="rule"
            class="rule-row"
            :class="{ divided: i < rulesOf(list.key).length - 1 }"
          >
            <span class="rule-text" :class="list.tone">{{ rule }}</span>
            <button class="rule-remove" type="button" title="删除" @click="removeRule(list.key, rule)">✕</button>
          </div>
          <div v-if="rulesOf(list.key).length === 0" class="rule-empty">暂无规则</div>
        </div>
        <div class="rule-count">共 {{ rulesOf(list.key).length }} 条，自动按字母排序</div>
      </div>
    </template>

    <div class="verdict-hint">
      判定顺序：① 命令替换 / 写重定向 / 管道到 shell → 始终确认（不可配置）；
      ② 黑名单命中且白名单未命中 → 确认；③ 黑名单未命中 → 放行（除非开启严格模式）。
      空配置文件自动使用内置默认规则。
    </div>
  </div>
</template>

<style scoped>
.rules-block {
  margin-bottom: 12px;
}

.block-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}

.block-title {
  font-size: 12px;
  font-weight: 500;
}

.block-actions {
  display: flex;
  gap: 6px;
}

.rules-intro {
  font-size: 11px;
  color: var(--text-muted);
  margin-bottom: 8px;
}

.rules-intro code {
  font-family: monospace;
}

.rules-check {
  display: flex;
  margin-bottom: 10px;
  font-size: 11px;
}

.rule-block {
  margin-bottom: 8px;
}

.rule-title {
  font-size: 12px;
  font-weight: 500;
  margin-bottom: 4px;
}

.rule-title.error {
  color: var(--error);
}

.rule-title.success {
  color: var(--success);
}

.rule-input-row {
  display: flex;
  gap: 4px;
  margin-bottom: 4px;
}

.rule-input {
  flex: 1;
  font-size: 13px;
  font-family: monospace;
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  padding: 5px 8px;
  color: var(--text-primary);
  outline: none;
}

/* 规则搜索框：MyInput（small + 前缀搜索图标），只保留布局约束。 */
.rule-search {
  width: 100%;
  margin-bottom: 4px;
}

.rule-listbox {
  overflow-y: auto;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--bg-input);
}

.rule-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
}

.rule-row.divided {
  border-bottom: 1px solid var(--border-default);
}

.rule-text {
  font-size: 13px;
  font-family: monospace;
  flex: 1;
  word-break: break-all;
}

.rule-text.error {
  color: var(--error);
}

.rule-text.success {
  color: var(--success);
}

.rule-remove {
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 14px;
  padding: 0 4px;
  flex-shrink: 0;
}

.rule-empty {
  padding: 8px;
  font-size: 12px;
  color: var(--text-muted);
  text-align: center;
}

.rule-count {
  font-size: 10px;
  color: var(--text-muted);
  margin-top: 2px;
}

.verdict-hint {
  font-size: 10px;
  color: var(--text-muted);
  margin-top: 4px;
  line-height: 1.6;
}
</style>
