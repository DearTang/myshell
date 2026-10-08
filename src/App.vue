<!-- App Shell：五区布局 + 保险库门禁 + 全局浮层装配。
     布局参照 unified-ui-vue 模板；面板挂载关系与旧 App.tsx 一致：
     所有 tab 的面板绝对定位堆叠（visibility 切换），保住 xterm 容器真实尺寸。 -->
<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch } from "vue";
import { MyButton } from "myui";
import { ackWhatsnew, getAppVersion, getWhatsnewAck, openExternalUrl } from "./api";
import {
  checkReportNeeded,
  markVersionHandled,
  reportVersion,
  setStatsConsent,
} from "./lib/usageStats";
import { ui, closeConnectionDialog, connectionDialog } from "@/store/ui";
import { reloadConnections, connectionsStore } from "@/store/connections";
import {
  sessions,
  setActiveTab,
  reconnect as reconnectTab,
  resetHostKeyAndReconnect,
  closeTab as closeTabAction,
  enterMultiWindow,
  clearReconnectSnapshot,
  registerTerminal,
  unregisterTerminal,
  getBroadcastTargets,
} from "@/store/sessions";
import { refreshVaultStatus, vault } from "@/composables/useVault";
import { startMcpBridge } from "@/composables/useMcpBridge";
import {
  autoCheckUpdates,
  checkNow,
  updateChecking,
  updateInfo,
} from "@/composables/useUpdateCheck";
import { rendererBackend } from "@/composables/useRendererPref";
import AppTopBar from "@/components/AppTopBar.vue";
import AppSidebar from "@/components/AppSidebar.vue";
import WindowControls from "@/components/WindowControls.vue";
import TerminalPanel from "@/components/TerminalPanel.vue";
import SftpPanel from "@/components/SftpPanel.vue";
import MultiWindowGrid from "@/components/MultiWindowGrid.vue";
import ServerInfoPanel from "@/components/ServerInfoPanel.vue";
import AiPanel from "@/components/AiPanel.vue";
import MasterPasswordGate from "@/components/MasterPasswordGate.vue";
import ConnectionDialog from "@/components/ConnectionDialog.vue";
import SettingsDrawer from "@/components/SettingsDrawer.vue";
import QuickCommandsPanel from "@/components/QuickCommandsPanel.vue";
import AboutDialog from "@/components/AboutDialog.vue";
import FeedbackDialog from "@/components/FeedbackDialog.vue";
import RecycleDialog from "@/components/RecycleDialog.vue";
import StatsConsentDialog from "@/components/StatsConsentDialog.vue";
import BroadcastDupDialog from "@/components/BroadcastDupDialog.vue";
import McpConfirmDialog from "@/components/McpConfirmDialog.vue";
import MultiWindowPicker from "@/components/MultiWindowPicker.vue";
import UpdateNotification from "@/components/UpdateNotification.vue";
import { appearance } from "@/store/appearance";
import type { Tab } from "./api";

defineOptions({ name: "AppShell" });

let stopMcpBridge: (() => void) | null = null;

const activeTab = computed<Tab | undefined>(() => sessions.tabs.find((t) => t.id === sessions.activeTabId));
const hasTabs = computed(() => sessions.tabs.length > 0);
// 响应式：外观设置里更换背景图后壳层立即生效
const hasBgImage = computed(() => !!appearance.bgImage.dataUrl);

// ── 保险库就绪后的启动序列（版本/更新/统计/MCP 桥/连接列表） ──
async function onVaultReady(): Promise<void> {
  vault.value = "checking";
  await refreshVaultStatus();
}

onMounted(async () => {
  await refreshVaultStatus();
});

onUnmounted(() => {
  stopMcpBridge?.();
  stopMcpBridge = null;
});

// vault 翻到 ready 时执行一次性启动任务
watch(
  vault,
  (state) => {
    if (state !== "ready") return;
    void reloadConnections();
    void getAppVersion()
      .then(async (v) => {
        ui.appVersion = v;
        // 升级后首次启动弹更新日志。确认状态存 Rust 侧文件（whatsnew-ack），
        // 不用 localStorage：WebView2 的 localStorage LevelDB 日志被非正常退出
        // 写坏后，其后所有写入永远读不回（曾导致每次启动都弹，见 阶段 149）。
        let known: string | null = null;
        try {
          known = await getWhatsnewAck();
        } catch {
          known = null;
        }
        if (known === null) {
          // 全新安装（或状态文件丢失）：静默记录，不打扰新用户
          try {
            await ackWhatsnew(v);
          } catch {
            /* best-effort */
          }
        } else if (known !== v) {
          ui.about = { open: true, mode: "whatsnew" };
        }
        const { shouldReport, hasConsent } = checkReportNeeded(v);
        if (shouldReport) {
          if (hasConsent) {
            void reportVersion(v, navigator.platform);
          } else {
            ui.statsPrompt = { version: v };
          }
        }
      })
      .catch(() => {
        /* getVersion 不应失败；失败则跳过该特性 */
      });
    autoCheckUpdates();
    if (!stopMcpBridge) {
      void startMcpBridge().then((off) => {
        stopMcpBridge = off;
      });
    }
  },
  { immediate: true },
);

function closeAbout(): void {
  // whatsnew 关闭即确认版本，下次启动不再弹（Rust 侧文件持久化）
  if (ui.about.mode === "whatsnew" && ui.appVersion) {
    void ackWhatsnew(ui.appVersion).catch(() => {
      /* best-effort */
    });
  }
  ui.about = { open: false, mode: "about" };
}

function onStatsAgree(): void {
  if (ui.statsPrompt) {
    setStatsConsent(true);
    void reportVersion(ui.statsPrompt.version, navigator.platform);
    markVersionHandled(ui.statsPrompt.version);
  }
  ui.statsPrompt = null;
}

function onStatsDecline(): void {
  if (ui.statsPrompt) {
    setStatsConsent(false);
    markVersionHandled(ui.statsPrompt.version);
  }
  ui.statsPrompt = null;
}

function onAiWidthChange(w: number): void {
  ui.aiPanelWidth = w;
  localStorage.setItem("myshell.aiPanelWidth", String(w));
}
</script>

<template>
  <!-- 保险库门禁：checking 全屏加载；setup/unlock 走门禁组件。
       顶栏尚未渲染，两层都带 drag-region 保证无边框窗口在门禁阶段可拖动；
       窗口控制按钮也需在此阶段可见（否则无法最小化/关闭） -->
  <div v-if="vault === 'checking'" class="vault-splash" data-tauri-drag-region>
    <WindowControls tone="overlay" />
    加载中…
  </div>
  <MasterPasswordGate v-else-if="vault !== 'ready'" :mode="vault === 'setup' ? 'setup' : 'unlock'" @success="onVaultReady" />

  <div
    v-else
    class="app-shell"
    :class="{
      'sidebar-collapsed': ui.sidebarCollapsed,
      'no-ai': !ui.showAiPanel,
      'has-bg-image': hasBgImage,
    }"
  >
    <AppTopBar class="shell-topbar" />
    <AppSidebar class="shell-side" />

    <main class="shell-work">
      <!-- 多窗口网格 -->
      <MultiWindowGrid v-if="ui.multiWindowMode" />

      <!-- 欢迎页 -->
      <div v-else-if="!hasTabs" class="welcome-screen">
        <div class="welcome-logo">MyShell</div>
        <div class="welcome-hint">点击左侧连接列表开始新会话</div>
      </div>

      <!-- 标签页堆叠：保持所有 tab 面板挂载（xterm 尺寸不变量）。
           work-panes 占据状态栏以上的全部空间；tab-stack 的 inset:0 以它为
           定位基准，终端不再铺到底部监控条底下与之重叠 -->
      <template v-else>
        <div class="work-panes">
          <div
            v-for="tab in sessions.tabs"
            :key="tab.id"
            class="tab-stack"
            :class="{ 'is-hidden': tab.id !== sessions.activeTabId }"
          >
            <!-- 连接失败态 -->
            <div v-if="tab.status === 'error'" class="error-state">
              <div class="error-icon">❌</div>
              <div class="error-title">连接失败</div>
              <div class="error-message">
                请确认下ip端口等是否填写错误!
                <br />
                {{ tab.errorMessage }}
              </div>
              <div class="error-actions">
                <MyButton variant="secondary" @click="closeTabAction(tab.id)">关闭</MyButton>
                <MyButton variant="primary" @click="tab.hostKeyMismatch ? resetHostKeyAndReconnect(tab.id) : reconnectTab(tab.id)">
                  {{ tab.hostKeyMismatch ? "重置密钥并重连" : "重新连接" }}
                </MyButton>
              </div>
            </div>

            <!-- 连接中态 -->
            <div v-else-if="!tab.sessionId" class="connecting-state">
              <div class="connecting-icon">⏳</div>
              <div>正在连接...</div>
            </div>

            <!-- SFTP/FTP 面板 -->
            <SftpPanel
              v-else-if="tab.type !== 'terminal'"
              :session-id="tab.sessionId"
              :source="tab.type === 'ftp' ? 'ftp' : 'ssh'"
              full-height
              :status="tab.status"
              @reconnect="reconnectTab(tab.id)"
              @disconnected="tab.status = 'disconnected'"
            />

            <!-- 终端面板 -->
            <TerminalPanel
              v-else
              :tab-id="tab.id"
              :session-id="tab.sessionId"
              :connection-id="tab.connectionId ?? ''"
              :conn-type="tab.connType"
              :font-override="tab.config?.terminal_font"
              :renderer-backend="rendererBackend"
              :active="tab.id === sessions.activeTabId"
              :status="tab.status"
              :connection-name="tab.name"
              :reconnect-snapshot="tab.reconnectSnapshot"
              :broadcast-targets="getBroadcastTargets(tab)"
              @terminal-ready="registerTerminal"
              @terminal-gone="unregisterTerminal"
              @open-ai="ui.showAiPanel = !ui.showAiPanel"
              @open-multiwindow="ui.showMultiWindowPicker = true"
              @reconnect="reconnectTab(tab.id)"
              @snapshot-consumed="clearReconnectSnapshot(tab.id)"
              @disconnected="tab.status = 'disconnected'"
              @open-quick-commands-manage="
                (cid: string) => {
                  ui.qcInitialConnectionId = cid;
                  ui.showQuickCommands = true;
                }
              "
            />
          </div>
        </div>

        <!-- 服务器监控（仅活动 SSH 终端）：流式布局中的最后一行，
             位于 work-panes 之下、窗口最底部，与终端内容互不重叠 -->
        <ServerInfoPanel
          v-if="
            activeTab &&
            activeTab.sessionId &&
            activeTab.connType === 'ssh' &&
            activeTab.type === 'terminal' &&
            activeTab.status === 'connected'
          "
          :session-id="activeTab.sessionId"
          :active="true"
        />
      </template>
    </main>

    <!-- AI 助手（右栏） -->
    <AiPanel
      v-if="ui.showAiPanel"
      class="shell-ai"
      :active-conn-type="activeTab?.connType"
      :active-connection-name="activeTab?.config?.name ?? activeTab?.connectionId"
      :active-session-id="activeTab?.sessionId"
      :width="ui.aiPanelWidth"
      @close="ui.showAiPanel = false"
      @width-change="onAiWidthChange"
    />

    <!-- ── 浮层群 ── -->
    <ConnectionDialog
      v-if="connectionDialog.open"
      :config="connectionDialog.editConfig"
      :initial-conn-type="connectionDialog.initialConnType"
      :initial-folder-path="connectionDialog.initialFolderPath"
      :folders="connectionsStore.folders"
      @close="closeConnectionDialog()"
      @save="reloadConnections()"
    />
    <SettingsDrawer
      v-if="ui.showSettings"
      :connection-count="connectionsStore.connections.length"
      @close="ui.showSettings = false"
      @refresh="reloadConnections()"
      @open-quick-commands="
        () => {
          ui.showSettings = false;
          ui.qcInitialConnectionId = null;
          ui.showQuickCommands = true;
        }
      "
    />
    <QuickCommandsPanel
      v-if="ui.showQuickCommands"
      :initial-connection-id="ui.qcInitialConnectionId"
      :active-connection-id="activeTab?.connectionId ?? null"
      @close="ui.showQuickCommands = false"
    />
    <AboutDialog
      v-if="ui.about.open"
      :mode="ui.about.mode"
      :version="ui.appVersion"
      :update-info="updateInfo"
      :checking="updateChecking"
      @close="closeAbout"
      @check-updates="checkNow()"
      @download="(url: string) => void openExternalUrl(url)"
    />
    <FeedbackDialog v-if="ui.showFeedback" :version="ui.appVersion" @close="ui.showFeedback = false" />
    <RecycleDialog v-if="ui.showRecycle" @close="ui.showRecycle = false" />
    <StatsConsentDialog v-if="ui.statsPrompt" :version="ui.statsPrompt.version" @agree="onStatsAgree" @decline="onStatsDecline" />
    <BroadcastDupDialog />
    <McpConfirmDialog />
    <MultiWindowPicker v-if="ui.showMultiWindowPicker" @close="ui.showMultiWindowPicker = false" @confirm="enterMultiWindow($event)" />
    <UpdateNotification />
  </div>
</template>

<style scoped>
.vault-splash {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-base);
  color: var(--text-muted);
  font-size: 13px;
}

.welcome-screen {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 16px;
  color: var(--text-muted);
}

.welcome-logo {
  font-size: 20px;
  font-weight: 600;
  color: var(--text-secondary);
}

.welcome-hint {
  font-size: 13px;
}

.connecting-state {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-muted);
  font-size: 14px;
}

.connecting-icon {
  font-size: 24px;
  animation: spin 1s linear infinite;
}

.error-state {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 32px;
}

.error-icon {
  width: 80px;
  height: 80px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--error-muted);
  border-radius: var(--radius-xl);
  font-size: 36px;
  margin-bottom: 20px;
}

.error-title {
  font-size: 18px;
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 8px;
}

.error-message {
  font-size: 13px;
  color: var(--text-tertiary);
  text-align: center;
  max-width: 400px;
  margin-bottom: 24px;
  line-height: 1.6;
}

.error-actions {
  display: flex;
  gap: 12px;
}

.btn-secondary {
  padding: 10px 24px;
  background: transparent;
  color: var(--text-secondary);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  font-size: 13px;
  cursor: pointer;
}

.btn-secondary:hover {
  background: var(--bg-surface-hover);
  border-color: var(--border-emphasis);
}

.btn-primary {
  padding: 10px 24px;
  background: var(--accent-primary);
  color: var(--text-inverse);
  border: none;
  border-radius: var(--radius-md);
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  box-shadow: var(--shadow-glow);
}

.btn-primary:hover {
  background: var(--accent-primary-hover);
}
</style>
