<!-- 关于 / 新版本说明 双面对话框。
     从 src-legacy/components/AboutDialog.tsx 原样移植（文案与行为语义零变化）：
     - whatsnew：升级后首次启动自动弹出（App 对比 myshell.knownVersion），标题 + 可滚动
     changelog + 单个"知道了"按钮；关闭即确认版本（壳层 closeAbout 负责）。
     - about：侧栏版本号入口打开，品牌标识 + 版本 + 检查更新 + 更新横幅（含应用内
     下载/安装流程，与 UpdateNotification 同构）+ changelog + 关闭按钮。
     markdown 由 react-markdown+remark-gfm 换为 markdown-it（html:false 保持防 XSS 语义，
     与 AiPanel.vue 同款配置）；changelog 为构建期内联的 CHANGELOG.md（离线安全）。
     BrandLogo 尚无 Vue 版，品牌 SVG 内联于此（渐变 id 固定，组件单实例安全）。
     壳：MyDialog（旧版点遮罩关闭 → dismissable；@cancel → emit('close')）。 -->
<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { Bell, MagicStick } from "@element-plus/icons-vue";
import MarkdownIt from "markdown-it";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { MyButton, MyDialog } from "myui";
import {
  downloadUpdate,
  installUpdate,
  onUpdateDownloadProgress,
  type UpdateInfo,
} from "@/api";
// `?raw` bundles the file as a string at build time (see vite-env.d.ts).
// Offline-safe and pinned to the installed version — no network needed to
// show what changed. CHANGELOG.md lives at the repo root, two levels up
// from src/components/.
import changelog from "../../CHANGELOG.md?raw";

defineOptions({ name: "AboutDialog" });

const props = defineProps<{
  mode: "whatsnew" | "about";
  version: string;
  updateInfo: UpdateInfo | null;
  /** True while a forced check is in flight (disables the check button). */
  checking: boolean;
}>();

const emit = defineEmits<{
  close: [];
  /** Force a fresh update check (bypasses the 24h throttle). */
  "check-updates": [];
  /** Open a download/release URL in the default browser. */
  download: [url: string];
}>();

// CHANGELOG.md carries a top-of-file guidance HTML comment (plus keepachangelog
// notes) that must NOT appear in the in-app rendered view. Strip every
// `<!-- … -->` (multiline-safe) and collapse the resulting blank runs before
// handing the string to markdown-it.
const cleanChangelog = changelog
  .replace(/<!--[\s\S]*?-->/g, "")
  .replace(/\n{3,}/g, "\n\n")
  .trim();

// ── markdown 渲染（markdown-it，语义对齐旧版 react-markdown + remark-gfm） ──
// 安全语义（保留旧版）：html:false —— 原始 HTML 一律转义，不进入 DOM；
// markdown-it 内置 validateLink 亦会拦截 javascript:/vbscript: 等危险协议链接。
// 因此 v-html 是安全的。表格/删除线内置支持，linkify 对齐 GFM 自动链接。
const md = new MarkdownIt({ html: false, linkify: true, breaks: false, typographer: false });

// 旧版 a 组件渲染 target=_blank rel=noreferrer —— 用 renderer 规则复现。
const prevLinkOpen =
  md.renderer.rules.link_open ??
  ((tokens, idx, options, _env, self) => self.renderToken(tokens, idx, options));
md.renderer.rules.link_open = (tokens, idx, options, env, self) => {
  tokens[idx].attrSet("target", "_blank");
  tokens[idx].attrSet("rel", "noreferrer");
  return prevLinkOpen(tokens, idx, options, env, self);
};

const renderedChangelog = md.render(cleanChangelog);

const isWhatsNew = computed(() => props.mode === "whatsnew");
const hasUpdate = computed(() => !!props.updateInfo?.has_update);

// Download destination: prefer the first asset's URL, fall back to the
// release page.
const downloadUrl = computed(() => props.updateInfo?.download_url || props.updateInfo?.release_url || "");

// Linux/macOS: no built-in installer-launch pipeline; show a single
// "open download page" button instead of the download+install flow.
const isBrowserMode = computed(() => props.updateInfo?.update_strategy === "browser");

// latest_version comes from the Gitee tag which already includes "v" (e.g.
// "v1.6.1"). Display it as-is to avoid a double "v" prefix.
const latestDisplay = computed(() => {
  const latest = props.updateInfo?.latest_version ?? "";
  return latest.startsWith("v") ? latest : `v${latest}`;
});

// Auto-download+install state (mirrors UpdateNotification pattern).
type AboutUpdatePhase = "idle" | "downloading" | "ready" | "failed";
const aboutPhase = ref<AboutUpdatePhase>("idle");
const aboutProgress = ref({ downloaded: 0, total: 0 });
const aboutError = ref("");
const aboutPath = ref("");

// 旧版 useEffect([aboutPhase])：进入 downloading 才订阅下载进度事件；
// 离开/重进时退订旧订阅（token 防竞态：迟到的订阅 Promise 自行退订）。
let progressSub: UnlistenFn | null = null;
let progressToken = 0;

watch(aboutPhase, (phase) => {
  const token = ++progressToken;
  progressSub?.();
  progressSub = null;
  if (phase !== "downloading") return;
  void onUpdateDownloadProgress((p) => {
    aboutProgress.value = { downloaded: p.downloaded, total: p.total };
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

const aboutPct = computed(() =>
  aboutProgress.value.total > 0
    ? Math.min(100, Math.round((aboutProgress.value.downloaded / aboutProgress.value.total) * 100))
    : null,
);

async function handleAboutUpdate(): Promise<void> {
  aboutPhase.value = "downloading";
  aboutProgress.value = { downloaded: 0, total: 0 };
  aboutError.value = "";
  try {
    const path = await downloadUpdate(downloadUrl.value);
    aboutPath.value = path;
    aboutPhase.value = "ready";
  } catch (e) {
    aboutError.value = String(e);
    aboutPhase.value = "failed";
  }
}

async function handleAboutInstall(): Promise<void> {
  if (!aboutPath.value) {
    aboutError.value = "安装包路径丢失";
    aboutPhase.value = "failed";
    return;
  }
  try {
    await installUpdate(aboutPath.value);
  } catch (e) {
    aboutError.value = String(e);
    aboutPhase.value = "failed";
  }
}

function onDownloadClick(): void {
  if (downloadUrl.value) emit("download", downloadUrl.value);
}
</script>

<template>
  <!-- Clicking the backdrop dismisses (matches the lightweight "what's new" /
       toast nature; the only editable state here is transient).
       MyDialog 壳：dismissable（遮罩 / Esc / 右上角 X）→ cancel → close。 -->
  <MyDialog
    :model-value="true"
    title="MyShell"
    :width="480"
    align-center
    dismissable
    class="about-dialog"
    @cancel="emit('close')"
  >
    <template #header>
      <!-- Header -->
      <div v-if="isWhatsNew" class="whatsnew-head">
        <span class="whatsnew-icon">
          <el-icon :size="28"><MagicStick /></el-icon>
        </span>
        <div>
          <div class="whatsnew-title">MyShell 已更新到 v{{ version }}</div>
          <div class="whatsnew-sub">以下是本次更新内容</div>
        </div>
      </div>
      <div v-else class="about-head">
        <!-- BrandLogo 内联 SVG（品牌 ">_" 标志，accent 变量自适应明暗主题） -->
        <svg
          class="brand-logo"
          width="44"
          height="44"
          viewBox="0 0 64 64"
          fill="none"
          role="img"
          aria-label="MyShell"
        >
          <defs>
            <linearGradient id="aboutBrandGrad" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" style="stop-color: var(--accent-primary-hover)" />
              <stop offset="0.5" style="stop-color: var(--accent-primary)" />
              <stop offset="1" style="stop-color: var(--accent-secondary)" />
            </linearGradient>
          </defs>
          <!-- chevron `>` — vertex on the left, opens right -->
          <path
            d="M35 19 L16 32 L35 45"
            stroke="url(#aboutBrandGrad)"
            stroke-width="6.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
          <!-- cursor `_` -->
          <rect x="37" y="42" width="13" height="5" rx="2.5" style="fill: var(--accent-secondary)" />
        </svg>
        <div class="about-info">
          <div class="about-title">MyShell</div>
          <div class="about-sub">版本 v{{ version }}</div>
        </div>
      </div>
    </template>

    <!-- Update banner (about mode only) -->
    <div v-if="!isWhatsNew" class="banner-wrap">
        <div v-if="hasUpdate" class="update-banner">
          <div class="banner-row">
            <span class="banner-icon">
              <el-icon :size="16"><Bell /></el-icon>
            </span>
            <div class="banner-info">
              <div class="banner-title">发现新版本 {{ latestDisplay }}</div>
              <div class="banner-sub">
                <template v-if="aboutPhase === 'idle'">
                  {{
                    isBrowserMode
                      ? "当前系统暂不支持应用内自动更新，请前往下载页手动下载安装"
                      : "可自动下载安装，也可前往网页下载"
                  }}
                </template>
                <template v-else-if="aboutPhase === 'downloading'">
                  {{ aboutPct !== null ? `正在下载… ${aboutPct}%` : "正在下载…" }}
                </template>
                <template v-else-if="aboutPhase === 'ready'">下载完成，可安装</template>
                <template v-else>{{ aboutError || "下载失败" }}</template>
              </div>
            </div>
          </div>
          <!-- Progress bar -->
          <div v-if="aboutPhase === 'downloading'" class="banner-progress">
            <div
              class="banner-progress-fill"
              :style="{ width: aboutPct !== null ? `${aboutPct}%` : '0%' }"
            ></div>
          </div>
          <!-- Action buttons -->
          <div class="banner-actions">
            <template v-if="aboutPhase === 'idle'">
              <template v-if="isBrowserMode">
                <MyButton class="grow" variant="primary" @click="onDownloadClick">打开下载页</MyButton>
              </template>
              <template v-else>
                <MyButton variant="primary" @click="handleAboutUpdate">更新</MyButton>
                <MyButton variant="ghost" @click="onDownloadClick">网页下载</MyButton>
              </template>
            </template>
            <div v-else-if="aboutPhase === 'downloading'" class="banner-waiting">请稍候…</div>
            <template v-else-if="aboutPhase === 'ready'">
              <MyButton class="grow" variant="primary" @click="handleAboutInstall">安装并重启</MyButton>
            </template>
            <template v-else>
              <MyButton variant="primary" @click="onDownloadClick">浏览器下载</MyButton>
              <MyButton variant="ghost" @click="handleAboutUpdate">重试</MyButton>
            </template>
          </div>
        </div>
        <div v-else class="check-row">
          <MyButton variant="ghost" :disabled="checking" @click="emit('check-updates')">
            {{ checking ? "检查中…" : "检查更新" }}
          </MyButton>
          <span class="check-hint">
            {{
              updateInfo?.error
                ? "上次检查失败，可重试"
                : updateInfo && !hasUpdate
                  ? "当前已是最新版本"
                  : ""
            }}
          </span>
        </div>
      </div>

      <!-- Changelog (scrollable) -->
      <div class="changelog">
        <!-- markdown-it html:false：CHANGELOG 内的原始 HTML 一律转义，防 XSS（保留旧版安全语义） -->
        <!-- eslint-disable-next-line vue/no-v-html -->
        <div class="markdown-body" v-html="renderedChangelog"></div>
      </div>

      <!-- Footer -->
      <template #footer>
        <div class="dlg-footer">
          <MyButton variant="primary" @click="emit('close')">
            {{ isWhatsNew ? "知道了" : "关闭" }}
          </MyButton>
        </div>
      </template>
  </MyDialog>
</template>

<style scoped>
/* ── MyDialog 壳适配（class 经 attrs 透传到 el-dialog 根元素，故用 :global） ──
   旧版结构：面板限高 + banner 固定 + changelog 内部滚动。等价改为：弹窗限高，
   body 固定不滚（flex 列），changelog 自身滚动。 */
:global(.about-dialog.el-dialog) {
  display: flex;
  flex-direction: column;
  max-height: 86vh;
}

:global(.about-dialog .el-dialog__body) {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.whatsnew-head {
  display: flex;
  align-items: center;
  gap: 12px;
}

.whatsnew-icon {
  font-size: 28px;
  display: inline-flex;
}

.whatsnew-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.whatsnew-sub {
  font-size: 12px;
  color: var(--text-tertiary);
  margin-top: 2px;
}

.about-head {
  display: flex;
  align-items: center;
  gap: 16px;
}

.brand-logo {
  display: block;
  filter: drop-shadow(0 0 10px var(--accent-primary-muted));
  flex-shrink: 0;
}

.about-info {
  flex: 1;
}

.about-title {
  font-size: 18px;
  font-weight: 600;
  color: var(--text-primary);
}

.about-sub {
  font-size: 12px;
  color: var(--text-tertiary);
  margin-top: 2px;
}

.banner-wrap {
  padding: 0 24px 14px;
}

.update-banner {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 14px;
  background: var(--success-muted);
  border: 1px solid var(--success);
  border-radius: var(--radius-md);
}

.banner-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.banner-icon {
  font-size: 16px;
}

.banner-info {
  flex: 1;
  min-width: 0;
}

.banner-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.banner-sub {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-top: 2px;
}

.banner-progress {
  height: 3px;
  background: var(--bg-surface);
  border-radius: var(--radius-full);
  overflow: hidden;
}

.banner-progress-fill {
  height: 100%;
  background: var(--accent-primary);
  border-radius: var(--radius-full);
  transition: width 150ms cubic-bezier(0.32, 0.72, 0, 1);
}

.banner-actions {
  display: flex;
  gap: 8px;
}

.banner-actions .grow {
  flex: 1;
}

.banner-waiting {
  flex: 1;
  text-align: center;
  font-size: 12px;
  color: var(--text-muted);
  padding: 6px 0;
}

.check-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.check-hint {
  font-size: 11px;
  color: var(--text-muted);
}

.changelog {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  border-top: 1px solid var(--border-subtle);
  /* body 为 overflow:hidden 的 flex 列（负边距会被剪裁），故不做全宽拉伸：
     分隔线随 body 内容盒（缩进 16px），内容保持旧版 24px 缩进 */
  padding: 4px 24px 16px;
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.6;
}

/* 旧版 ReactMarkdown 内联组件映射 → 深度选择器复现同款观感 */
.markdown-body :deep(h1) {
  margin: 14px 0 8px;
  font-size: 16px;
  color: var(--text-primary);
}

.markdown-body :deep(h2) {
  margin: 14px 0 8px;
  font-size: 15px;
  color: var(--text-primary);
}

.markdown-body :deep(h3) {
  margin: 10px 0 6px;
  font-size: 13px;
  color: var(--text-primary);
}

.markdown-body :deep(h4),
.markdown-body :deep(h5),
.markdown-body :deep(h6) {
  margin: 10px 0 6px;
  font-size: 13px;
  color: var(--text-primary);
}

.markdown-body :deep(p) {
  margin: 6px 0;
}

.markdown-body :deep(ul),
.markdown-body :deep(ol) {
  margin: 6px 0;
  padding-left: 20px;
}

.markdown-body :deep(li) {
  margin: 3px 0;
}

.markdown-body :deep(a) {
  color: var(--accent-primary);
  text-decoration: none;
}

.markdown-body :deep(hr) {
  border: none;
  border-top: 1px solid var(--border-subtle);
  margin: 14px 0;
}

.markdown-body :deep(code) {
  font-family: monospace;
  font-size: 12px;
  background: var(--bg-surface);
  padding: 1px 5px;
  border-radius: 4px;
}

.markdown-body :deep(blockquote) {
  margin: 8px 0;
  padding: 8px 12px;
  border-left: 3px solid var(--border-emphasis);
  background: var(--bg-surface);
  border-radius: var(--radius-sm);
  color: var(--text-tertiary);
  font-size: 12px;
}

.markdown-body :deep(table) {
  border-collapse: collapse;
  margin: 8px 0;
  font-size: 12px;
}

.markdown-body :deep(th),
.markdown-body :deep(td) {
  border: 1px solid var(--border-default);
  padding: 4px 8px;
  text-align: left;
}

/* 让 #footer 分隔线紧贴 body 结束处（去掉 EP footer 自带的 padding-top） */
:global(.about-dialog .el-dialog__footer) {
  padding-top: 0;
}

.dlg-footer {
  border-top: 1px solid var(--border-subtle);
  /* 左右/下负边距横贯弹窗全宽（抵消 .el-dialog 根元素内边距） */
  margin: 0 calc(var(--el-dialog-padding-primary) * -1)
    calc(var(--el-dialog-padding-primary) * -1);
  padding: 12px 24px 16px;
  display: flex;
  justify-content: flex-end;
}
</style>
