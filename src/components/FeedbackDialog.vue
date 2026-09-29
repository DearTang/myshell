<!-- 反馈对话框：类型/描述/联系方式/截图附件/运行日志，提交时打包本地 zip + 复制剪贴板 + 唤起邮件客户端。
     从 src-legacy/components/FeedbackDialog.tsx 原样移植（注释与文案逐字保留）。
     壳：MyDialog。旧版「Esc 关闭 / 点遮罩关闭（提交中禁用）」由 close-on-press-escape /
     close-on-click-modal 的 !submitting 动态值承担（触发 cancel → handleClose）；右上角
     关闭按钮旧版没有 → showClose 维持禁用。 -->
<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { zipSync, strToU8 } from "fflate";
import {
  Camera,
  ChatDotRound,
  CircleCheck,
  CircleClose,
  FolderOpened,
  Link,
  Picture,
} from "@element-plus/icons-vue";
import { MyButton, MyCheckbox, MyDialog, MyInput, MySegmented } from "myui";
import {
  clearFeedbackDir,
  getFeedbackLog,
  openExternalUrl,
  revealPath,
  saveFeedbackZip,
  writeFrontendLog,
  type FeedbackLogInfo,
} from "@/api";

defineOptions({ name: "FeedbackDialog" });

const props = defineProps<{ version: string }>();
const emit = defineEmits<{ close: [] }>();

type FeedbackType = "bug" | "feature" | "other";

interface Attachment {
  name: string;
  dataUrl: string; // data:image/png;base64,…
  bytes: number;
}

const TYPE_LABELS: Record<FeedbackType, string> = {
  bug: "问题报告",
  feature: "功能建议",
  other: "其他",
};

const type = ref<FeedbackType>("bug");
const description = ref("");
const contact = ref("");
const attachments = ref<Attachment[]>([]);
const attachLog = ref(true);
const logInfo = ref<FeedbackLogInfo | null>(null);
const logLoading = ref(true);
const showLog = ref(false);

const submitting = ref(false);
const result = ref<
  | { kind: "success"; message: string; savedPath?: string }
  | { kind: "error"; message: string; savedPath?: string }
  | null
>(null);

// MySegmented 的值类型是 string|number|boolean，桥接到 FeedbackType。
const typeValue = ref<string | number | boolean>("bug");
function onTypeChange(v: string | number | boolean): void {
  typeValue.value = v;
  type.value = v as FeedbackType;
}

// Wrap onClose to clear the feedback dir when the dialog closes. This runs
// on every close path (cancel, ESC, "完成" button, overlay click) and
// prevents old zip packages from piling up on disk.
function handleClose(): void {
  void clearFeedbackDir().catch(() => {});
  emit("close");
}

// Load the (already-scrubbed) log on open.
let cancelled = false;
onMounted(() => {
  getFeedbackLog()
    .then((info) => {
      if (!cancelled) {
        logInfo.value = info;
        logLoading.value = false;
      }
    })
    .catch((e) => {
      if (!cancelled) {
        logInfo.value = {
          logDir: "",
          content: `(读取日志失败: ${String(e)})`,
          truncated: false,
        };
        logLoading.value = false;
      }
    });
});
onBeforeUnmount(() => {
  cancelled = true;
});

// ESC to close（由 MyDialog 的 close-on-press-escape="!submitting" 承担，
// 触发 cancel → handleClose；提交中两条关闭路径都随 !submitting 一并禁用）。

async function addImageFromFile(): Promise<void> {
  try {
    const selected = await open({
      multiple: true,
      filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] }],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    for (const p of paths) {
      await loadFileAsAttachment(p);
    }
  } catch {
    // dialog cancelled — ignore
  }
}

// Read a local file via the existing read_file_base64 command (returns a
// data URL). This reuses an audited command rather than a new one.
async function loadFileAsAttachment(path: string): Promise<void> {
  const { readFileBase64 } = await import("../api");
  const dataUrl = await readFileBase64(path);
  const base64 = dataUrl.split(",")[1] ?? "";
  // Approximate byte size from base64 length.
  const bytes = Math.floor((base64.length * 3) / 4);
  const name = path.replace(/\\/g, "/").split("/").pop() ?? "image";
  attachments.value = [...attachments.value, { name, dataUrl, bytes }];
}

async function captureScreen(): Promise<void> {
  try {
    // getDisplayMedia may not be available in all webview versions; the
    // catch degrades to "pick a file" silently.
    const mediaDevices = navigator.mediaDevices as MediaDevices & {
      getDisplayMedia?: (c: DisplayMediaStreamOptions) => Promise<MediaStream>;
    };
    if (!mediaDevices?.getDisplayMedia) {
      await addImageFromFile();
      return;
    }
    const stream = await mediaDevices.getDisplayMedia({ video: true });
    const track = stream.getVideoTracks()[0];
    // ImageCapture isn't universally typed; fall back to a video element grab.
    const video = document.createElement("video");
    video.srcObject = stream;
    await video.play();
    // Wait one frame for the video to render.
    await new Promise((r) => requestAnimationFrame(r));
    const canvas = document.createElement("canvas");
    canvas.width = video.videoWidth;
    canvas.height = video.videoHeight;
    const ctx = canvas.getContext("2d");
    ctx?.drawImage(video, 0, 0);
    track.stop();
    stream.getTracks().forEach((t) => t.stop());
    const dataUrl = canvas.toDataURL("image/png");
    const base64 = dataUrl.split(",")[1] ?? "";
    const bytes = Math.floor((base64.length * 3) / 4);
    const ts = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
    attachments.value = [
      ...attachments.value,
      { name: `screenshot-${ts}.png`, dataUrl, bytes },
    ];
  } catch {
    // User cancelled the screen picker, or getDisplayMedia unsupported.
    await addImageFromFile();
  }
}

function removeAttachment(idx: number): void {
  attachments.value = attachments.value.filter((_, i) => i !== idx);
}

/**
 * Build a local zip with: feedback.txt (type + description + env), the log
 * (if attached), and all images. Saved via the Rust save_feedback_zip
 * command into the feedback dir. This is the "open folder / manual send"
 * fallback that works regardless of the Web3Forms origin question.
 */
async function buildAndSaveZip(): Promise<string | null> {
  const ts = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
  const feedbackText = [
    `MyShell 反馈报告`,
    `时间: ${new Date().toLocaleString()}`,
    `类型: ${TYPE_LABELS[type.value]}`,
    `版本: v${props.version}`,
    `平台: ${navigator.platform}`,
    `联系方式: ${contact.value || "(未提供)"}`,
    ``,
    `──── 描述 ────`,
    description.value,
    ``,
  ].join("\n");

  const files: Record<string, Uint8Array> = {
    "feedback.txt": strToU8(feedbackText),
  };

  if (attachLog.value && logInfo.value?.content) {
    files["myshell.log"] = strToU8(logInfo.value.content);
  }

  for (let i = 0; i < attachments.value.length; i++) {
    const att = attachments.value[i];
    const base64 = att.dataUrl.split(",")[1] ?? "";
    // Decode base64 → binary. atob is available in the webview.
    const bin = atob(base64);
    const u8 = new Uint8Array(bin.length);
    for (let j = 0; j < bin.length; j++) u8[j] = bin.charCodeAt(j);
    const ext = att.name.split(".").pop() ?? "png";
    files[`images/${i + 1}.${ext}`] = u8;
  }

  const zipped = zipSync(files);
  const path = await saveFeedbackZip(`myshell-feedback-${ts}`, zipped);
  return path;
}

async function handleSubmit(): Promise<void> {
  if (!description.value.trim()) return;
  submitting.value = true;
  result.value = null;

  // Step 1: Always build the local zip — it's the reliable backup and the
  // thing the user attaches to the email (mailto can't auto-attach files).
  let savedPath: string | null = null;
  try {
    savedPath = await buildAndSaveZip();
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    console.warn("feedback zip save failed", e);
    writeFrontendLog("warn", `[feedback] 本地反馈包保存失败: ${msg}`);
  }

  // Step 2: Prepare the feedback content for the clipboard. We DON'T put
  // this in the mailto: body because many email clients (especially QQ Mail,
  // Outlook) silently drop the body when the URL is too long or the encoding
  // differs from what they expect. Instead we copy to clipboard and ask the
  // user to Ctrl+V — 100% reliable.
  const typeLabel = TYPE_LABELS[type.value];
  const subject = `【MYSHELL】${typeLabel} v${props.version}`;

  // Extract just the filename from savedPath for the clipboard note.
  const zipFileName = savedPath
    ? savedPath.replace(/[\\/]/g, "/").split("/").pop() ?? ""
    : "";

  const clipboardText = [
    `类型: ${typeLabel}`,
    `版本: v${props.version}`,
    `平台: ${navigator.platform}`,
    `联系方式: ${contact.value || "(未提供)"}`,
    ``,
    `描述:`,
    description.value,
  ].join("\n");

  // mailto: with subject only — short and reliable across all email clients.
  const mailtoUrl = `mailto:argustang@qq.com?subject=${encodeURIComponent(subject)}`;

  try {
    // Copy feedback content to clipboard first, then open the mail client.
    await navigator.clipboard.writeText(clipboardText);
    await openExternalUrl(mailtoUrl);
    writeFrontendLog("info", `[feedback] 已复制内容到剪贴板并唤起邮件客户端`);

    const noteParts = ["反馈内容已复制到剪贴板，请在邮件正文中按 Ctrl+V 粘贴。"];
    if (zipFileName) {
      noteParts.push(
        `然后点击下方按钮打开反馈包，将 "${zipFileName}" 拖入邮件作为附件后发送。`,
      );
    }
    result.value = {
      kind: "success",
      message: noteParts.join(""),
      savedPath: savedPath ?? undefined,
    };

    // Extra popup reminder — the result panel might be missed if the email
    // client window covers it. This ensures the user sees the instruction.
    const reminder = zipFileName
      ? `反馈内容已复制到剪贴板！\n\n请在邮件中：\n1. 正文区域按 Ctrl+V 粘贴\n2. 将 "${zipFileName}" 拖入邮件作为附件`
      : `反馈内容已复制到剪贴板！\n\n请在邮件正文区域按 Ctrl+V 粘贴。`;
    window.alert(reminder);
    // Intentionally NOT auto-opening the folder — explorer.exe and the
    // mailto handler race for window focus, and either one can steal focus
    // from the other depending on OS scheduling. The result screen has a
    // button to open the folder after the email client has opened.
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    writeFrontendLog("error", `[feedback] 唤起邮件客户端失败: ${msg}`);
    result.value = {
      kind: "error",
      message: `无法打开邮件客户端：${msg}。反馈包已保存在本地，你可以手动发送到 argustang@qq.com。`,
      savedPath: savedPath ?? undefined,
    };
  } finally {
    submitting.value = false;
  }
}

const canSubmit = (): boolean => description.value.trim().length > 0 && !submitting.value;

function openRevealFolder(): void {
  if (!result.value?.savedPath) return;
  const dir = result.value.savedPath.replace(/[\\/][^\\/]+$/, "");
  void revealPath(dir);
}

function openGiteeIssue(): void {
  void openExternalUrl("https://gitee.com/argustang/myshell/issues/new");
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

const logSizeText = (): string => {
  if (logLoading.value) return "（加载中…）";
  if (logInfo.value) {
    return `（${formatBytes(new Blob([logInfo.value.content]).size)}${logInfo.value.truncated ? "，已截断" : ""}）`;
  }
  return "";
};
</script>

<template>
  <!-- 提交中禁用遮罩/Esc 关闭（对齐旧版 !submitting 守卫） -->
  <MyDialog
    :model-value="true"
    title="提交反馈"
    size="lg"
    align-center
    class="feedback-dialog"
    :close-on-click-modal="!submitting"
    :close-on-press-escape="!submitting"
    @cancel="handleClose"
  >
    <template #header>
      <div class="dlg-header">
        <span class="header-icon">
          <el-icon :size="20"><ChatDotRound /></el-icon>
        </span>
        <div>
          <div class="header-title">提交反馈</div>
          <div class="header-sub">MyShell v{{ version }} · 帮助我们做得更好</div>
        </div>
      </div>
    </template>

    <template v-if="result">
        <!-- ── Result view ── -->
        <div class="scroll-body">
          <div class="result-wrap">
            <div class="result-icon">
              <el-icon
                :size="48"
                :color="result.kind === 'success' ? 'var(--success)' : 'var(--error)'"
              >
                <CircleCheck v-if="result.kind === 'success'" />
                <CircleClose v-else />
              </el-icon>
            </div>
            <div class="result-title" :class="result.kind === 'success' ? 'is-success' : 'is-error'">
              {{ result.kind === "success" ? "提交成功" : "提交失败" }}
            </div>
            <div v-if="result.kind === 'error'" class="result-error-box">
              {{ result.message }}
            </div>
            <div v-if="result.kind === 'success'" class="result-success-text">
              {{ result.message }}
            </div>
            <div v-if="result.savedPath" class="result-folder-row">
              <MyButton variant="secondary" :class="{ 'folder-btn-error': result.kind === 'error' }" @click="openRevealFolder">
                <el-icon :size="13"><FolderOpened /></el-icon>
                打开反馈包所在文件夹
              </MyButton>
            </div>
          </div>
        </div>
      </template>

      <template v-else>
        <!-- ── Form view ── -->
        <div class="scroll-body">
          <!-- Type -->
          <div class="form-block">
            <label class="field-label">反馈类型</label>
            <MySegmented
              :model-value="typeValue"
              block
              :options="[
                { label: TYPE_LABELS.bug, value: 'bug' },
                { label: TYPE_LABELS.feature, value: 'feature' },
                { label: TYPE_LABELS.other, value: 'other' },
              ]"
              @update:model-value="onTypeChange"
            />
          </div>

          <!-- Description -->
          <div class="form-block">
            <label class="field-label">
              描述 <span class="required-star">*</span>
            </label>
            <textarea
              v-model="description"
              class="desc-textarea"
              :placeholder="
                type === 'bug'
                  ? '发生了什么？你期望什么结果？复现步骤？（越详细越容易修复）'
                  : '你想看到什么功能？为什么需要它？'
              "
              autofocus
            ></textarea>
          </div>

          <!-- Contact -->
          <div class="form-block">
            <MyInput
              v-model="contact"
              label="联系方式（选填）"
              placeholder="邮箱 / QQ / 微信，方便我们追问细节"
            />
          </div>

          <!-- Images -->
          <div class="form-block">
            <label class="field-label">截图 / 图片（选填）</label>
            <div class="image-actions">
              <MyButton variant="secondary" size="small" @click="captureScreen">
                <el-icon :size="13"><Camera /></el-icon>
                截图
              </MyButton>
              <MyButton variant="secondary" size="small" @click="addImageFromFile">
                <el-icon :size="13"><Picture /></el-icon>
                选择图片
              </MyButton>
            </div>
            <div v-if="attachments.length > 0" class="attachment-list">
              <div v-for="(att, i) in attachments" :key="i" class="attachment-thumb">
                <img :src="att.dataUrl" :alt="att.name" />
                <button type="button" class="att-remove" title="移除" @click="removeAttachment(i)">✕</button>
                <!-- 图片上的角标：黑色半透明遮罩上必须用白字（任何主题下），无对应令牌 -->
                <div class="att-size">{{ formatBytes(att.bytes) }}</div>
              </div>
            </div>
            <div class="hint-text">
              截图仅打包进本地反馈包（zip），需你自己作为附件发送；MyShell 不会把它们上传到任何服务器。
            </div>
          </div>

          <!-- Log -->
          <div>
            <MyCheckbox v-model="attachLog" :label="`附带运行日志${logSizeText()}`" />
            <div class="hint-text log-hint">
              日志已自动脱敏（主机名/用户名/IP 会被掩码），可放心提交。
              <template v-if="logInfo?.truncated"> 仅包含最近的日志条目。</template>
            </div>
            <div v-if="attachLog && logInfo" class="log-actions">
              <MyButton variant="secondary" size="small" @click="showLog = !showLog">
                {{ showLog ? "收起日志" : "查看日志内容" }}
              </MyButton>
              <MyButton v-if="logInfo.logDir" variant="secondary" size="small" @click="revealPath(logInfo.logDir)">
                <el-icon :size="13"><FolderOpened /></el-icon>
                打开日志目录
              </MyButton>
            </div>
            <pre v-if="showLog && logInfo" class="log-pre">{{ logInfo.content || "(空)" }}</pre>
          </div>
        </div>
      </template>

      <!-- Footer -->
      <template #footer>
        <div class="dlg-footer">
          <template v-if="result">
            <MyButton variant="primary" @click="handleClose">完成</MyButton>
          </template>
          <template v-else>
            <a
              class="gitee-link"
              title="在 Gitee 上提交 Issue"
              @click.prevent="openGiteeIssue"
            >
              <el-icon :size="12"><Link /></el-icon>
              也可通过 Gitee Issue 提交
            </a>
            <MyButton variant="secondary" :disabled="submitting" @click="handleClose">取消</MyButton>
            <MyButton
              variant="primary"
              :disabled="!canSubmit()"
              @click="handleSubmit"
            >
              {{ submitting ? "提交中…" : "提交反馈" }}
            </MyButton>
          </template>
        </div>
      </template>
  </MyDialog>
</template>

<style scoped>
/* ── MyDialog 壳适配（class 经 attrs 透传到 el-dialog 根元素，故用 :global） ── */
:global(.feedback-dialog.el-dialog) {
  display: flex;
  flex-direction: column;
  max-height: 86vh;
}

:global(.feedback-dialog .el-dialog__body) {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

/* 让 #footer 分隔线紧贴 body 结束处（去掉 EP footer 自带的 padding-top） */
:global(.feedback-dialog .el-dialog__footer) {
  padding-top: 0;
}

.dlg-header {
  display: flex;
  align-items: center;
  gap: 10px;
}

.header-icon {
  font-size: 20px;
  display: inline-flex;
}

.header-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
}

.header-sub {
  font-size: 12px;
  color: var(--text-tertiary);
}

.scroll-body {
  flex: 1;
  overflow-y: auto;
  padding: 20px 24px;
}

/* ── 结果视图 ── */
.result-wrap {
  text-align: center;
  padding: 24px 12px;
}

.result-icon {
  font-size: 48px;
  margin-bottom: 12px;
}

.result-title {
  font-size: 16px;
  font-weight: 700;
  margin-bottom: 8px;
}

.result-title.is-success {
  color: var(--success);
}

.result-title.is-error {
  color: var(--error);
}

.result-error-box {
  display: inline-block;
  background: var(--error-muted);
  border: 1px solid var(--error);
  border-radius: var(--radius-md);
  padding: 10px 16px;
  font-size: 13px;
  color: var(--text-primary);
  line-height: 1.6;
  max-width: 440px;
  text-align: left;
  margin: 0 auto 12px;
}

.result-success-text {
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.6;
  max-width: 420px;
  margin: 0 auto;
}

.result-folder-row {
  margin-top: 16px;
}

.folder-btn-error {
  color: var(--error);
  border-color: var(--error);
}

/* ── 表单视图 ── */
.form-block {
  margin-bottom: 16px;
}

.field-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 6px;
  display: block;
}

.required-star {
  color: var(--error);
}

.desc-textarea {
  width: 100%;
  resize: vertical;
  background: var(--bg-input);
  color: var(--text-primary);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 10px 12px;
  font-size: 13px;
  line-height: 1.5;
  font-family: inherit;
  outline: none;
  min-height: 100px;
  max-height: 240px;
}

.desc-textarea:focus {
  border-color: var(--accent-primary);
}

.image-actions {
  display: flex;
  gap: 8px;
  margin-bottom: 8px;
}

.attachment-list {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.attachment-thumb {
  position: relative;
  width: 80px;
  height: 80px;
  border-radius: var(--radius-md);
  overflow: hidden;
  border: 1px solid var(--border-default);
  background: var(--bg-input);
}

.attachment-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.att-remove {
  position: absolute;
  top: 2px;
  right: 2px;
  width: 18px;
  height: 18px;
  border-radius: var(--radius-full);
  border: none;
  /* 图片缩略图上的悬浮删除钮：黑色半透明底 + 白字与主题无关（无对应令牌） */
  background: rgba(0, 0, 0, 0.6);
  color: #fff;
  font-size: 11px;
  cursor: pointer;
  line-height: 1;
  padding: 0;
}

.att-size {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  background: rgba(0, 0, 0, 0.5);
  color: #fff;
  font-size: 9px;
  text-align: center;
  padding: 1px 0;
}

.hint-text {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-top: 6px;
}

.log-hint {
  margin: 4px 0 6px;
}

.log-actions {
  display: flex;
  gap: 8px;
}

.log-pre {
  margin-top: 8px;
  max-height: 200px;
  overflow: auto;
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 10px;
  font-size: 11px;
  line-height: 1.5;
  color: var(--text-muted);
  white-space: pre-wrap;
  word-break: break-all;
}

/* ── Footer（#footer 插槽；左右/下负边距横贯弹窗全宽，抵消根元素内边距） ── */
.dlg-footer {
  border-top: 1px solid var(--border-subtle);
  margin: 0 calc(var(--el-dialog-padding-primary) * -1)
    calc(var(--el-dialog-padding-primary) * -1);
  padding: 12px 24px 16px;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
}

.gitee-link {
  font-size: 12px;
  color: var(--text-tertiary);
  text-decoration: none;
  cursor: pointer;
  margin-right: auto;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
</style>
