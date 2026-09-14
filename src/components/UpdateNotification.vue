<!-- 更新可用浮层卡片（自包含）。
     从 src-legacy/components/UpdateNotification.tsx 原样移植：
     updateInfo 改从 @/composables/useUpdateCheck 读取（旧版经 prop 传入，渲染条件
     updateInfo.has_update 收进本组件 visible），忽略/下载/安装/浏览器跳转逻辑照旧。
     位置按迁移指令与旧 App.tsx 设计注释（"show the bottom-left UpdateNotification card,
     dismissable, per-version"）实现为左下角浮层卡片，替代全屏居中遮罩；
     忽略后按版本号记忆（localStorage myshell.ignoredUpdateVersion），新版本重新弹出。
     两条路径：Windows 走应用内下载+安装（进度条→安装并重启）；
     Linux/macOS（update_strategy="browser"）只提供"打开下载页"。 -->
<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { Bell } from "@element-plus/icons-vue";
import { MyButton } from "myui";
import {
  downloadUpdate,
  installUpdate,
  onUpdateDownloadProgress,
  openExternalUrl,
} from "@/api";
import { updateInfo } from "@/composables/useUpdateCheck";

defineOptions({ name: "UpdateNotification" });

type Phase = "prompt" | "downloading" | "ready" | "failed";

const latest = computed(() => updateInfo.value?.latest_version ?? "");
// latest_version comes from the Gitee tag which already includes "v" (e.g.
// "v1.6.1"). Display it as-is to avoid a double "v" prefix.
const latestDisplay = computed(() => {
  const v = latest.value;
  return v.startsWith("v") ? v : `v${v}`;
});
// Linux/macOS path: no built-in installer-launch pipeline. Skip the
// download+install phases and offer a single button that opens the
// release page in the system browser (user downloads the .deb/.dmg
// manually). Windows keeps the original auto-download flow.
const isBrowserMode = computed(() => updateInfo.value?.update_strategy === "browser");

const downloadUrl = computed(() => updateInfo.value?.download_url || updateInfo.value?.release_url || "");

const IGNORE_KEY = "myshell.ignoredUpdateVersion";
// 旧版 useState 惰性初始化 → IIFE 求值一次（等价语义）
const ignored = ref<boolean>(
  (() => {
    try {
      return localStorage.getItem(IGNORE_KEY) === latest.value;
    } catch {
      return false;
    }
  })(),
);

watch(latest, (v) => {
  try {
    if (localStorage.getItem(IGNORE_KEY) !== v) ignored.value = false;
  } catch {
    // ignore
  }
});

const visible = computed(() => !!updateInfo.value?.has_update && !ignored.value);

const phase = ref<Phase>("prompt");
const progress = ref<{ downloaded: number; total: number }>({
  downloaded: 0,
  total: 0,
});
const error = ref<string>("");
const downloadedPath = ref<string>("");

// 旧版 useEffect([phase])：进入 downloading 才订阅下载进度事件（token 防竞态）。
let progressSub: UnlistenFn | null = null;
let progressToken = 0;

watch(phase, (p) => {
  const token = ++progressToken;
  progressSub?.();
  progressSub = null;
  if (p !== "downloading") return;
  void onUpdateDownloadProgress((evt) => {
    progress.value = { downloaded: evt.downloaded, total: evt.total };
  }).then((fn) => {
    if (token !== progressToken) fn();
    else progressSub = fn;
  });
});

onBeforeUnmount(() => {
  progressToken++;
  progressSub?.();
  progressSub = null;
});

const pct = computed(() =>
  progress.value.total > 0
    ? Math.min(100, Math.round((progress.value.downloaded / progress.value.total) * 100))
    : null,
);

async function handleUpdate(): Promise<void> {
  phase.value = "downloading";
  progress.value = { downloaded: 0, total: 0 };
  error.value = "";
  try {
    const path = await downloadUpdate(downloadUrl.value);
    downloadedPath.value = path;
    phase.value = "ready";
  } catch (e) {
    error.value = String(e);
    phase.value = "failed";
  }
}

async function handleInstall(): Promise<void> {
  if (!downloadedPath.value) {
    error.value = "安装包路径丢失";
    phase.value = "failed";
    return;
  }
  try {
    await installUpdate(downloadedPath.value);
  } catch (e) {
    error.value = String(e);
    phase.value = "failed";
  }
}

function handleIgnore(): void {
  ignored.value = true;
  try {
    localStorage.setItem(IGNORE_KEY, latest.value);
  } catch {
    // best-effort
  }
}

function openBrowser(): void {
  if (downloadUrl.value) void openExternalUrl(downloadUrl.value);
}
</script>

<template>
  <div v-if="visible" class="update-card animate-scale-in">
    <!-- Header -->
    <div class="header">
      <div class="icon-circle">
        <span class="icon">
          <el-icon :size="22"><Bell /></el-icon>
        </span>
      </div>
      <div class="title">MyShell 有新版本可用</div>
      <div class="version">{{ latestDisplay }}</div>
    </div>

    <!-- Subtitle / status -->
    <div class="subtitle">
      <template v-if="phase === 'prompt'">
        {{
          isBrowserMode
            ? "检测到新版本。当前系统暂不支持应用内自动更新，请前往下载页手动下载安装。"
            : "新版本已就绪，是否立即更新？"
        }}
      </template>
      <template v-else-if="phase === 'downloading'">
        {{ pct !== null ? `正在下载… ${pct}%` : "正在下载…" }}
      </template>
      <template v-else-if="phase === 'ready'">下载完成，点击安装并重启应用</template>
      <template v-else>{{ error || "下载出现问题" }}</template>
    </div>

    <!-- Progress bar -->
    <div v-if="phase === 'downloading'" class="progress-track">
      <div class="progress-fill" :style="{ width: pct !== null ? `${pct}%` : '0%' }"></div>
    </div>

    <!-- Actions -->
    <div class="actions">
      <template v-if="phase === 'prompt'">
        <MyButton variant="ghost" class="grow" @click="handleIgnore">忽略</MyButton>
        <MyButton v-if="isBrowserMode" variant="primary" class="grow" @click="openBrowser">
          打开下载页
        </MyButton>
        <MyButton v-else variant="primary" class="grow" @click="handleUpdate">更新</MyButton>
      </template>
      <div v-else-if="phase === 'downloading'" class="downloading-text">
        请稍候，下载完成后将自动提示…
      </div>
      <template v-else-if="phase === 'ready'">
        <MyButton variant="primary" class="grow full" @click="handleInstall">安装并重启</MyButton>
      </template>
      <template v-else>
        <MyButton variant="primary" class="grow" @click="openBrowser">浏览器下载</MyButton>
        <MyButton variant="ghost" class="grow" @click="handleUpdate">重试</MyButton>
      </template>
    </div>
  </div>
</template>

<style scoped>
/* 旧版 global.css 的 animate-scale-in 在新版基样式中不存在，本地补齐 */
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

/* 左下角浮层卡片（按旧 App.tsx 设计注释与迁移指令；旧实现为全屏遮罩，
   位置变化不影响交互语义：无遮罩、不阻塞主界面、按版本可忽略） */
.update-card {
  position: fixed;
  left: 16px;
  bottom: calc(var(--ui-statusbar-h) + 16px);
  z-index: 1500;
  width: 400px;
  max-width: 90vw;
  background: var(--bg-elevated);
  border: 1px solid var(--border-emphasis);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-xl);
  overflow: hidden;
}

.header {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 28px 24px 0;
  gap: 10px;
}

.icon-circle {
  width: 48px;
  height: 48px;
  border-radius: var(--radius-full);
  background: var(--accent-primary-muted);
  border: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--accent-primary);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
}

.icon {
  font-size: 22px;
}

.title {
  font-size: 17px;
  font-weight: 600;
  color: var(--text-primary);
  letter-spacing: -0.01em;
}

.version {
  font-size: 13px;
  font-weight: 500;
  color: var(--accent-primary);
  background: var(--accent-primary-muted);
  padding: 3px 12px;
  border-radius: var(--radius-full);
  letter-spacing: 0.02em;
}

.subtitle {
  text-align: center;
  font-size: 13px;
  color: var(--text-tertiary);
  line-height: 1.5;
  padding: 14px 24px 0;
}

.progress-track {
  height: 3px;
  background: var(--bg-surface);
  margin: 14px 24px 0;
  border-radius: var(--radius-full);
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: var(--accent-primary);
  border-radius: var(--radius-full);
  transition: width 150ms cubic-bezier(0.32, 0.72, 0, 1);
}

.actions {
  display: flex;
  gap: 10px;
  padding: 20px 24px 24px;
}

.actions .grow {
  flex: 1;
}

.actions .full {
  width: 100%;
}

.downloading-text {
  flex: 1;
  text-align: center;
  font-size: 12px;
  color: var(--text-muted);
  padding: 8px 0;
}
</style>
