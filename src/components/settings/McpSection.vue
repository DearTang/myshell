<!-- MCP 支持 — AI 工具集成（旧 mcp 分区）：服务开关 / 附件目录 / 命令确认规则 / 工具配置。
     active 属性语义沿用旧版：每次切到本类目都重新拉取工具/目录/规则（规则编辑器
     拆分为 CommandRulesEditor，自行响应 active）。 -->
<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import type { Component } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { Camera, ChatDotRound, Lightning, Tools } from "@element-plus/icons-vue";
import { MyButton, MySection, MyToggle, toast } from "myui";
import {
  mcpDetectTools,
  mcpWriteConfig,
  mcpRemoveConfig,
  getAttachmentDir,
  setAttachmentDir,
  showInFolder,
  type AiToolInfo,
} from "@/api";
import CommandRulesEditor from "./CommandRulesEditor.vue";

defineOptions({ name: "McpSection" });

const props = defineProps<{ active: boolean }>();

// ── MCP 服务状态 ──
const mcpEnabled = ref(false);
const mcpTools = ref<AiToolInfo[]>([]);
const mcpConfiguring = ref(false);
// ── 附件目录（截图自动保存的位置）──
const attachmentDir = ref<string | null>(null);
const attachmentDirPicking = ref(false);
const attachmentDirAcknowledged = ref(false);

function loadAll(): void {
  // 检测已安装的工具
  mcpDetectTools()
    .then((tools) => {
      mcpTools.value = tools;
      // 任一工具已配置则开关反映之
      mcpEnabled.value = tools.some((t) => t.configured);
    })
    .catch(() => {});
  // 读取附件目录
  getAttachmentDir()
    .then((dir) => {
      attachmentDir.value = dir;
    })
    .catch(() => {});
  // 读取"用户已知晓附件目录提示"旗标
  try {
    const ack = localStorage.getItem("myshell-attachment-dir-acknowledged");
    if (ack === "1") attachmentDirAcknowledged.value = true;
  } catch {
    /* best-effort */
  }
}

onMounted(loadAll);

watch(
  () => props.active,
  (active) => {
    if (active) loadAll();
  },
);

/** 通过系统目录选择器选目录并持久化。 */
async function pickAttachmentDir(): Promise<void> {
  attachmentDirPicking.value = true;
  try {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      const canonical = await setAttachmentDir(selected);
      attachmentDir.value = canonical;
      try {
        localStorage.setItem("myshell-attachment-dir-acknowledged", "1");
      } catch {
        /* best-effort */
      }
      attachmentDirAcknowledged.value = true;
    }
  } catch (e) {
    console.error("pickAttachmentDir failed:", e);
  } finally {
    attachmentDirPicking.value = false;
  }
}

async function onMcpEnabledChange(next: string | number | boolean): Promise<void> {
  mcpEnabled.value = Boolean(next);
  if (!mcpEnabled.value) {
    // 禁用：移除所有工具的配置
    const tools = await mcpDetectTools();
    for (const t of tools) {
      if (t.configured) {
        await mcpRemoveConfig(t.id).catch(() => {});
      }
    }
    mcpTools.value = await mcpDetectTools();
    toast("已禁用 MCP 支持", { type: "success", duration: 4000 });
  } else {
    toast("已启用 MCP 支持，请配置下方密码", { type: "success", duration: 4000 });
  }
}

// ── AI 工具配置 ──
async function configureAll(): Promise<void> {
  mcpConfiguring.value = true;
  try {
    const tools = await mcpDetectTools();
    let configured = 0;
    let skipped = 0;
    for (const tool of tools) {
      if (!tool.installed) continue;
      if (tool.configured) {
        skipped++;
        continue;
      }
      const written = await mcpWriteConfig(tool.id);
      if (written) configured++;
    }
    mcpTools.value = await mcpDetectTools();
    toast(`配置完成：新增 ${configured} 个，跳过 ${skipped} 个已配置`, { type: "success", duration: 4000 });
  } catch (e) {
    toast(`配置失败: ${e}`, { type: "error", duration: 4000 });
  } finally {
    mcpConfiguring.value = false;
  }
}

async function configureTool(tool: AiToolInfo): Promise<void> {
  try {
    const written = await mcpWriteConfig(tool.id);
    mcpTools.value = await mcpDetectTools();
    toast(written ? `已为 ${tool.name} 配置 MCP` : "已存在，跳过", { type: "success", duration: 4000 });
  } catch (e) {
    toast(`配置失败: ${e}`, { type: "error", duration: 4000 });
  }
}

async function removeToolConfig(tool: AiToolInfo): Promise<void> {
  try {
    await mcpRemoveConfig(tool.id);
    mcpTools.value = await mcpDetectTools();
    toast(`已从 ${tool.name} 移除 MCP 配置`, { type: "success", duration: 4000 });
  } catch (e) {
    toast(`移除失败: ${e}`, { type: "error", duration: 4000 });
  }
}

function toolIcon(id: string): Component {
  return id === "claude" ? ChatDotRound : id === "opencode" ? Lightning : Tools;
}
</script>

<template>
  <MySection title="MCP 服务">
    <div class="mcp-head">
      <div class="mcp-head-text">
        <div class="mcp-title">启用 MCP 支持</div>
        <div class="mcp-subtitle">允许 AI 工具（Claude / Opencode / Zcode）通过 MyShell 操作远程服务器</div>
      </div>
      <MyToggle :model-value="mcpEnabled" @update:model-value="onMcpEnabledChange" />
    </div>

    <!-- v-show 保持挂载：开关来回切换不丢编辑态（与旧版根组件状态一致） -->
    <div v-show="mcpEnabled">
      <!-- 附件目录 — 截图自动保存的位置。首次打开 MCP 设置且未配置时，
           显示警告横幅指向选择器。一旦知晓，之后即使取消设置也不再打扰。 -->
      <div class="block">
        <div class="block-head">
          <div class="block-title">附件目录</div>
          <div class="block-actions">
            <MyButton v-if="attachmentDir" variant="secondary" size="small" @click="showInFolder(attachmentDir)">
              打开目录
            </MyButton>
            <MyButton variant="primary" size="small" :disabled="attachmentDirPicking" @click="pickAttachmentDir">
              {{ attachmentDirPicking ? "选择中..." : attachmentDir ? "更改目录" : "选择目录" }}
            </MyButton>
          </div>
        </div>

        <div v-if="!attachmentDir && !attachmentDirAcknowledged" class="warning-banner">
          尚未配置附件目录。终端截图功能（CommandBar 的
          <el-icon class="inline-icon" :size="12"><Camera /></el-icon>
          按钮）会保存到这里，请先选择一个目录。
        </div>

        <div v-if="attachmentDir" class="dir-path">{{ attachmentDir }}</div>
        <div v-else class="dir-missing">未配置 — 截图按钮点击后会提示先来此配置</div>
        <div class="dir-hint">终端截图文件名为「截图_&lt;连接名&gt;_&lt;时间戳&gt;.png」，自动保存到此目录。</div>
      </div>

      <!-- 命令确认规则 — 控制哪些 ssh_exec 命令跳过系统对话框的白名单/黑名单正则。 -->
      <CommandRulesEditor :active="active" />

      <!-- AI 工具配置 -->
      <div class="block">
        <div class="block-head">
          <div class="block-title">已安装的 AI 工具</div>
          <MyButton variant="secondary" size="small" :disabled="mcpConfiguring" @click="configureAll">
            {{ mcpConfiguring ? "配置中…" : "一键配置全部" }}
          </MyButton>
        </div>

        <div class="tools-list">
          <div v-for="tool in mcpTools" :key="tool.id" class="tool-row">
            <div class="tool-info">
              <span class="tool-icon">
                <el-icon :size="14"><component :is="toolIcon(tool.id)" /></el-icon>
              </span>
              <div>
                <div class="tool-name">{{ tool.name }}</div>
                <div class="tool-status">
                  <template v-if="tool.installed">
                    <span v-if="tool.configured" class="ok">✓ 已配置 MCP</span>
                    <span v-else>已安装，未配置</span>
                  </template>
                  <span v-else>未检测到</span>
                </div>
              </div>
            </div>
            <div class="tool-actions">
              <MyButton
                v-if="tool.installed && !tool.configured"
                variant="primary"
                size="small"
                @click="configureTool(tool)"
              >
                配置
              </MyButton>
              <MyButton
                v-if="tool.configured"
                variant="danger"
                size="small"
                @click="removeToolConfig(tool)"
              >
                移除
              </MyButton>
            </div>
          </div>
        </div>

        <div class="other-tools">
          <strong>其他 AI 工具？</strong>只需在 MCP 配置文件中添加如下 server：<br />
          <code>{{'{"command": "…/myshell-mcp.exe"}'}}</code><br />
          Claude Desktop → .claude/mcp.json | Cursor → .cursor/mcp.json | 其他工具可参考其 MCP 配置文档。
        </div>
      </div>
    </div>
  </MySection>
</template>

<style scoped>
.mcp-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}

.mcp-title {
  font-size: 13px;
  font-weight: 500;
}

.mcp-subtitle {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 2px;
}

.block {
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

.warning-banner {
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  background: var(--warning-muted);
  border: 1px solid var(--warning);
  font-size: 11px;
  color: var(--text-secondary);
  margin-bottom: 8px;
}

.dir-path {
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  font-size: 11px;
  color: var(--text-secondary);
  font-family: monospace;
  word-break: break-all;
}

.dir-missing {
  font-size: 11px;
  color: var(--text-muted);
}

.dir-hint {
  font-size: 10px;
  color: var(--text-muted);
  margin-top: 6px;
}

.tools-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.tool-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
}

.tool-info {
  display: flex;
  align-items: center;
  gap: 8px;
}

.tool-icon {
  font-size: 14px;
  display: inline-flex;
  align-items: center;
}

/* 正文内嵌的小图标（警告横幅里的截图按钮指代），随文字颜色走 currentColor。 */
.inline-icon {
  vertical-align: -2px;
}

.tool-name {
  font-size: 12px;
  font-weight: 500;
}

.tool-status {
  font-size: 10px;
  color: var(--text-muted);
}

.tool-status .ok {
  color: var(--success);
}

.tool-actions {
  display: flex;
  gap: 6px;
}

.other-tools {
  margin-top: 10px;
  padding: 10px;
  border-radius: var(--radius-sm);
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  font-size: 11px;
  color: var(--text-muted);
  line-height: 1.5;
}

.other-tools code {
  font-size: 10px;
  background: var(--bg-base);
  padding: 2px 4px;
  border-radius: 3px;
}
</style>
