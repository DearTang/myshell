<!-- SFTP/FTP 文件面板（由 src-legacy/components/SftpPanel.tsx 移植）。
     目录浏览 + 上传/下载（传输进度浮层）+ 新建目录/删除/重命名。
     source 分流：ssh 走 russh-sftp 子系统，ftp 走 suppaftp —— 两者共享同一 UI，
     让用户跨连接类型复用肌肉记忆。 -->
<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Back,
  Close,
  Document,
  Download,
  Edit,
  Folder,
  FolderAdd,
  RefreshRight,
  Right,
  Top,
  Upload,
} from "@element-plus/icons-vue";
import {
  ftpCancelTransfer,
  ftpDownload,
  ftpListDir,
  ftpMkdir,
  ftpRemove,
  ftpRename,
  ftpUpload,
  onSftpTransferDone,
  onSftpTransferProgress,
  onSshClosed,
  sftpCancelTransfer,
  sftpDownload,
  sftpListDir,
  sftpMkdir,
  sftpRemove,
  sftpRename,
  sftpUpload,
} from "@/api";
import type { FileEntry, SftpTransferProgressPayload, Tab } from "@/api";
import { getSftpDownloadConcurrency } from "@/utils/transfer-settings";
import { MyButton, MyDialog, MyInput, MyProgress, MySpinner, confirmDialog, toast } from "myui";

defineOptions({ name: "SftpPanel" });

const props = withDefaults(
  defineProps<{
    sessionId: string;
    /** "ssh" routes through the russh-sftp subsystem (default). "ftp" routes
     * through suppaftp — both share the same UI surface so the user can reuse
     * muscle memory across connection types. */
    source: "ftp" | "ssh";
    /** When true, the panel stretches to fill its parent — used when an FTP/SFTP
     * connection opens in its own tab. Default (side panel) is fixed-width. */
    fullHeight?: boolean;
    /** Connection status from the parent tab — drives the reconnect overlay. */
    status?: Tab["status"];
  }>(),
  { fullHeight: false },
);

/** SSH channel closed (server EOF / shell exit / network drop) → 父层把标签页
 * 状态点从绿翻红；用户在断连浮层点了重新连接。 */
const emit = defineEmits<{ reconnect: []; disconnected: [] }>();

/** In-flight or finished transfer shown in the overlay. */
interface TransferState {
  phase: "upload" | "download";
  currentFile: string;
  fileIndex: number;
  fileCount: number;
  bytesDone: number;
  bytesTotal: number;
  errors: string[];
  done: boolean;
  startTime: number;
  requestId: string;
}

// FTP servers don't understand "~" — use "/" as the natural root. SSH/SFTP
// honors "~" for the home directory shortcut (resolved server-side).
const initialPath = props.source === "ftp" ? "/" : "~";

const currentPath = ref(initialPath);
const entries = ref<FileEntry[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);
const history = ref<string[]>([initialPath]);
const historyIndex = ref(0);
const newFolderName = ref("");
const showMkdir = ref(false);
// Multi-select for batch download. Stores entry.path so it survives sorting.
const selected = ref<Set<string>>(new Set());
// Active transfer overlay state (null = no overlay).
const transfer = ref<TransferState | null>(null);
// Unlisteners for the active transfer's events — cleared on overlay close.
let transferUnlisten: Array<() => void> = [];

// 1s tick to keep elapsed/ETA live in the transfer overlay（等价 React 强制重渲）。
const tickNow = ref(Date.now());
let tickTimer: ReturnType<typeof setInterval> | null = null;

// ── 协议分流：FTP sessions don't live in the SSH session map，后端 FTP 传输
//    发出相同的 progress/done 事件，浮层逻辑完全复用。 ──
function listDir(sessionId: string, path: string): Promise<FileEntry[]> {
  return props.source === "ftp" ? ftpListDir(sessionId, path) : sftpListDir(sessionId, path);
}
function makeDir(sessionId: string, path: string): Promise<void> {
  return props.source === "ftp" ? ftpMkdir(sessionId, path) : sftpMkdir(sessionId, path);
}
function removeEntry(sessionId: string, path: string, isDir: boolean): Promise<void> {
  return props.source === "ftp" ? ftpRemove(sessionId, path, isDir) : sftpRemove(sessionId, path);
}
function renameEntry(sessionId: string, oldPath: string, newPath: string): Promise<void> {
  return props.source === "ftp"
    ? ftpRename(sessionId, oldPath, newPath)
    : sftpRename(sessionId, oldPath, newPath);
}
function uploadFiles(
  sessionId: string,
  localPaths: string[],
  remoteDestDir: string,
  requestId: string,
): Promise<void> {
  return props.source === "ftp"
    ? ftpUpload(sessionId, localPaths, remoteDestDir, requestId)
    : sftpUpload(sessionId, localPaths, remoteDestDir, requestId);
}
function downloadFiles(
  sessionId: string,
  remotePaths: string[],
  localDestDir: string,
  requestId: string,
): Promise<void> {
  return props.source === "ftp"
    ? ftpDownload(sessionId, remotePaths, localDestDir, requestId, getSftpDownloadConcurrency())
    : sftpDownload(sessionId, remotePaths, localDestDir, requestId, getSftpDownloadConcurrency());
}
function cancelTransfer(requestId: string): Promise<void> {
  return props.source === "ftp" ? ftpCancelTransfer(requestId) : sftpCancelTransfer(requestId);
}

async function loadDir(path: string): Promise<void> {
  loading.value = true;
  error.value = null;
  try {
    const files = await listDir(props.sessionId, path);
    entries.value = files;
    currentPath.value = path;
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

// Initial load；sessionId 变化（重连拿到新 session）时重新从初始目录加载。
watch(
  () => props.sessionId,
  () => {
    void loadDir(initialPath);
  },
  { immediate: true },
);

// Clear selection when the directory changes — stale paths would be invalid.
watch(currentPath, () => {
  selected.value = new Set();
});

// Cleanup any active transfer listeners on unmount.
onUnmounted(() => {
  transferUnlisten.forEach((u) => u());
  transferUnlisten = [];
  if (tickTimer) {
    clearInterval(tickTimer);
    tickTimer = null;
  }
});

// ── Subscribe to ssh_closed so the tab's status dot flips to red when the
//    underlying SSH channel dies (server EOF, shell exit, network drop).
//    Without this, SFTP tabs stay green forever because only TerminalPanel
//    subscribed to ssh_closed — SftpPanel rendered in its own tab never heard it.
//    重连会换 sessionId，这里跟随重新订阅。 ──
let unlistenClosed: (() => void) | null = null;
let closedSeq = 0;
watch(
  () => [props.sessionId, props.source] as const,
  ([sid, src]) => {
    unlistenClosed?.();
    unlistenClosed = null;
    const seq = ++closedSeq;
    if (src !== "ssh") return;
    onSshClosed(sid, () => {
      emit("disconnected");
    }).then((u) => {
      if (seq !== closedSeq) {
        // Component unmounted / session swapped before the promise resolved — clean up immediately.
        u();
      } else {
        unlistenClosed = u;
      }
    });
  },
  { immediate: true },
);
onUnmounted(() => {
  closedSeq++;
  unlistenClosed?.();
  unlistenClosed = null;
});

function navigateTo(path: string): void {
  const newHistory = history.value.slice(0, historyIndex.value + 1);
  newHistory.push(path);
  history.value = newHistory;
  historyIndex.value = newHistory.length - 1;
  void loadDir(path);
}

function goBack(): void {
  if (historyIndex.value > 0) {
    const newIndex = historyIndex.value - 1;
    historyIndex.value = newIndex;
    void loadDir(history.value[newIndex]);
  }
}

function goForward(): void {
  if (historyIndex.value < history.value.length - 1) {
    const newIndex = historyIndex.value + 1;
    historyIndex.value = newIndex;
    void loadDir(history.value[newIndex]);
  }
}

function goUp(): void {
  const parts = currentPath.value.replace(/\/$/, "").split("/");
  if (parts.length > 1) {
    const parent = parts.slice(0, -1).join("/") || "/";
    navigateTo(parent);
  }
}

// 新建目录输入条出现时自动聚焦（等价旧 autoFocus）。
const mkdirInputRef = ref<HTMLInputElement | null>(null);
watch(showMkdir, (v) => {
  if (v) void nextTick(() => mkdirInputRef.value?.focus());
});

async function handleMkdir(): Promise<void> {
  if (!newFolderName.value.trim()) return;
  const fullPath = currentPath.value.endsWith("/")
    ? currentPath.value + newFolderName.value
    : currentPath.value + "/" + newFolderName.value;
  try {
    await makeDir(props.sessionId, fullPath);
    showMkdir.value = false;
    newFolderName.value = "";
    void loadDir(currentPath.value);
  } catch (e) {
    toast(`创建失败: ${e}`, { type: "error" });
  }
}

async function handleDelete(entry: FileEntry): Promise<void> {
  const ok = await confirmDialog({
    message: `确认删除 "${entry.name}"？`,
    title: "删除",
    type: "warning",
  });
  if (!ok) return;
  try {
    await removeEntry(props.sessionId, entry.path, entry.is_dir);
    void loadDir(currentPath.value);
  } catch (e) {
    toast(`删除失败: ${e}`, { type: "error" });
  }
}

// 旧版用 window.prompt（Tauri webview 不支持）——按迁移约定改为本地对话框。
const renameOpen = ref(false);
const renameName = ref("");
const renameTarget = ref<FileEntry | null>(null);

function openRenameDialog(entry: FileEntry): void {
  renameTarget.value = entry;
  renameName.value = entry.name;
  renameOpen.value = true;
}

function onRenameConfirm(): void {
  const entry = renameTarget.value;
  const newName = renameName.value;
  if (!entry || !newName || newName === entry.name) return;
  const parentDir = currentPath.value.endsWith("/") ? currentPath.value : currentPath.value + "/";
  const newPath = parentDir + newName;
  renameEntry(props.sessionId, entry.path, newPath)
    .then(() => {
      void loadDir(currentPath.value);
    })
    .catch((e) => {
      toast(`重命名失败: ${e}`, { type: "error" });
    });
}

function toggleSelected(path: string): void {
  const next = new Set(selected.value);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  selected.value = next;
}

// Download accepts files AND folders — folders are expanded recursively
// on the Rust side (subtree mirrors under <dest>/<folder-name>/...).
const selectedFileEntries = computed(() => entries.value.filter((e) => selected.value.has(e.path)));

/** Wire up progress/done listeners, run the transfer, then refresh if asked.
 * Shared by upload + download so the overlay lifecycle lives in one place. */
async function runTransfer(
  phase: "upload" | "download",
  start: (requestId: string) => Promise<void>,
  refreshAfter: boolean,
): Promise<void> {
  const requestId = crypto.randomUUID();
  transfer.value = {
    phase,
    currentFile: "",
    fileIndex: 0,
    fileCount: 0,
    bytesDone: 0,
    bytesTotal: 0,
    errors: [],
    done: false,
    startTime: Date.now(),
    requestId,
  };
  // Subscribe BEFORE invoking so the earliest progress events land.
  const unP = await onSftpTransferProgress(requestId, (p: SftpTransferProgressPayload) => {
    if (!transfer.value) return;
    transfer.value.currentFile = p.currentFile;
    transfer.value.fileIndex = p.fileIndex;
    transfer.value.fileCount = p.fileCount;
    transfer.value.bytesDone = p.bytesDone;
    transfer.value.bytesTotal = p.bytesTotal;
  });
  const unD = await onSftpTransferDone(requestId, (errors) => {
    if (!transfer.value) return;
    transfer.value.done = true;
    transfer.value.errors = errors;
    tickNow.value = Date.now();
  });
  transferUnlisten = [unP, unD];
  try {
    await start(requestId);
    // Backend emits `done` right before returning Ok; if the listener hasn't
    // processed it yet (async), mark done here so the overlay finalizes.
    if (transfer.value && !transfer.value.done) {
      transfer.value.done = true;
      tickNow.value = Date.now();
    }
  } catch (e) {
    if (transfer.value) {
      transfer.value.done = true;
      transfer.value.errors = [...transfer.value.errors, String(e)];
      tickNow.value = Date.now();
    }
  } finally {
    // Reset selection after transfer completes (download selects files
    // via checkboxes — stale checks after download are confusing).
    selected.value = new Set();
    if (refreshAfter) void loadDir(currentPath.value);
  }
}

async function handleUpload(): Promise<void> {
  const picked = await open({ multiple: true, title: "选择要上传的文件" });
  if (!picked) return;
  const paths = Array.isArray(picked) ? picked : [picked];
  if (paths.length === 0) return;
  await runTransfer(
    "upload",
    (rid) => uploadFiles(props.sessionId, paths, currentPath.value, rid),
    true,
  );
}

async function handleDownload(): Promise<void> {
  if (selectedFileEntries.value.length === 0) {
    toast("请先勾选要下载的文件或文件夹", { type: "warning" });
    return;
  }
  const dest = await open({ directory: true, title: "选择保存位置" });
  if (!dest) return;
  const destDir = typeof dest === "string" ? dest : Array.isArray(dest) ? dest[0] : "";
  if (!destDir) return;
  const paths = selectedFileEntries.value.map((e) => e.path);
  await runTransfer(
    "download",
    (rid) => downloadFiles(props.sessionId, paths, destDir, rid),
    false,
  );
}

function closeTransfer(): void {
  transferUnlisten.forEach((u) => u());
  transferUnlisten = [];
  transfer.value = null;
}

function onCancelTransfer(): void {
  const t = transfer.value;
  if (!t) return;
  cancelTransfer(t.requestId).catch(() => {});
}

// 传输进行中每秒走一次 tick；done 后停表（elapsed 冻结在完成时刻）。
watch(
  () => [transfer.value?.done ?? true, transfer.value?.startTime ?? 0] as const,
  ([done]) => {
    if (done || !transfer.value) {
      if (tickTimer) {
        clearInterval(tickTimer);
        tickTimer = null;
      }
      return;
    }
    tickNow.value = Date.now();
    if (!tickTimer) {
      tickTimer = setInterval(() => {
        tickNow.value = Date.now();
      }, 1000);
    }
  },
);

function formatClock(epochMs: number): string {
  if (!epochMs) return "—";
  const d = new Date(epochMs);
  return [d.getHours(), d.getMinutes(), d.getSeconds()]
    .map((n) => String(n).padStart(2, "0"))
    .join(":");
}

function formatDuration(seconds: number): string {
  if (!isFinite(seconds) || seconds <= 0) return "—";
  if (seconds < 60) return `${Math.ceil(seconds)}秒`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = Math.ceil(seconds % 60);
    return `${m}分${s}秒`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}小时${m}分`;
}

function formatSize(bytes: number): string {
  if (bytes === 0) return "-";
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  let size = bytes;
  while (size >= 1024 && i < units.length - 1) {
    size /= 1024;
    i++;
  }
  return `${size.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

/** Derive a short type label from the file name extension. */
function fileType(name: string, isDir: boolean): string {
  if (isDir) return "目录";
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return "文件";
  const ext = name.slice(dot + 1).toLowerCase();
  const map: Record<string, string> = {
    sh: "Shell", bash: "Shell", py: "Python", rb: "Ruby", pl: "Perl",
    js: "JS", ts: "TS", jsx: "JSX", tsx: "TSX", vue: "Vue", svelte: "Svelte",
    rs: "Rust", go: "Go", java: "Java", kt: "Kotlin", c: "C", cpp: "C++",
    h: "Header", cs: "C#", swift: "Swift", php: "PHP", lua: "Lua",
    html: "HTML", htm: "HTML", css: "CSS", scss: "SCSS", less: "Less",
    json: "JSON", yaml: "YAML", yml: "YAML", toml: "TOML", xml: "XML",
    md: "Markdown", txt: "文本", log: "日志", csv: "CSV", sql: "SQL",
    gz: "压缩", zip: "压缩", tar: "压缩", tgz: "压缩", "7z": "压缩", rar: "压缩",
    png: "图片", jpg: "图片", jpeg: "图片", gif: "图片", svg: "图片", webp: "图片", ico: "图片",
    mp3: "音频", wav: "音频", flac: "音频", aac: "音频", ogg: "音频",
    mp4: "视频", mkv: "视频", avi: "视频", mov: "视频", webm: "视频",
    pdf: "PDF", doc: "Word", docx: "Word", xls: "Excel", xlsx: "Excel",
    ppt: "PPT", pptx: "PPT",
    conf: "配置", cfg: "配置", ini: "配置", env: "配置", service: "配置",
    pem: "证书", key: "证书", crt: "证书", cert: "证书",
    dockerfile: "Docker", gitignore: "Git",
  };
  return map[ext] || ext.toUpperCase();
}

/** Format a Unix timestamp (seconds) to a readable local time string. */
function formatTime(ts: number): string {
  if (!ts) return "-";
  const d = new Date(ts * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** Truncate a permissions string (e.g. "drwxr-xr-x") to fit. */
function truncatePerm(p: string): string {
  if (p.length <= 10) return p;
  return p.slice(0, 10);
}

const pctDone = computed(() => {
  const t = transfer.value;
  if (!t || t.bytesTotal <= 0) return 0;
  return Math.min(100, Math.round((t.bytesDone / t.bytesTotal) * 100));
});

const elapsed = computed(() => {
  const t = transfer.value;
  if (!t || t.startTime <= 0) return 0;
  return (tickNow.value - t.startTime) / 1000;
});

const speed = computed(() => (elapsed.value > 1 ? (transfer.value?.bytesDone ?? 0) / elapsed.value : 0));

const remaining = computed(() => {
  const t = transfer.value;
  const sp = speed.value;
  return sp > 0 && t !== null && t.bytesTotal > 0 ? (t.bytesTotal - t.bytesDone) / sp : Infinity;
});
</script>

<template>
  <div class="sftp-panel" :class="{ 'full-height': fullHeight }">
    <!-- Toolbar -->
    <div class="toolbar">
      <button type="button" class="nav-btn" title="后退" :disabled="historyIndex <= 0" @click="goBack">
        <el-icon :size="14"><Back /></el-icon>
      </button>
      <button
        type="button"
        class="nav-btn"
        title="前进"
        :disabled="historyIndex >= history.length - 1"
        @click="goForward"
      >
        <el-icon :size="14"><Right /></el-icon>
      </button>
      <button type="button" class="nav-btn" title="上级目录" @click="goUp">
        <el-icon :size="14"><Top /></el-icon>
      </button>
      <button type="button" class="nav-btn" title="刷新" @click="loadDir(currentPath)">
        <el-icon :size="14"><RefreshRight /></el-icon>
      </button>
      <div class="toolbar-sep" />
      <button type="button" class="nav-btn" title="新建文件夹" @click="showMkdir = !showMkdir">
        <el-icon :size="14"><FolderAdd /></el-icon>
      </button>
      <template v-if="source === 'ssh'">
        <div class="toolbar-sep" />
        <button type="button" class="nav-btn" title="上传文件（可多选）" @click="handleUpload">
          <el-icon :size="14"><Upload /></el-icon>
        </button>
        <button
          type="button"
          class="nav-btn"
          :title="selectedFileEntries.length > 0 ? `下载 ${selectedFileEntries.length} 个选中文件` : '下载选中文件（先勾选文件）'"
          :disabled="selectedFileEntries.length === 0"
          @click="handleDownload"
        >
          <el-icon :size="14"><Download /></el-icon>
          <span v-if="selectedFileEntries.length > 0" class="nav-count">{{ selectedFileEntries.length }}</span>
        </button>
      </template>
    </div>

    <!-- Path Bar -->
    <div class="path-bar">
      <!-- Terminal/server prefix — reinforces "remote filesystem" vs sidebar's local tree -->
      <svg
        class="term-glyph"
        width="13"
        height="13"
        viewBox="0 0 16 16"
        fill="none"
        stroke="var(--text-muted)"
        stroke-width="1.2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <polyline points="4 5 7 8 4 11" />
        <line x1="9" y1="11" x2="12" y2="11" />
      </svg>
      <input v-model="currentPath" class="path-input" @keydown.enter="navigateTo(currentPath)" />
    </div>

    <!-- Mkdir Input -->
    <div v-if="showMkdir" class="mkdir-bar">
      <input
        ref="mkdirInputRef"
        v-model="newFolderName"
        class="mkdir-input"
        placeholder="文件夹名称"
        @keydown.enter="handleMkdir"
        @keydown.esc="showMkdir = false"
      />
      <MyButton variant="primary" size="small" @click="handleMkdir">创建</MyButton>
    </div>

    <!-- Column Headers -->
    <div class="column-headers">
      <span v-if="source === 'ssh'" class="hdr-check" />
      <span class="hdr-icon" />
      <span class="hdr-name">名称</span>
      <span class="hdr-size">大小</span>
      <span class="hdr-type">类型</span>
      <span class="hdr-mtime">修改时间</span>
      <span class="hdr-perm">权限</span>
      <span class="hdr-actions" />
    </div>

    <!-- File List -->
    <div class="file-list">
      <div v-if="loading" class="list-note">
        <MySpinner :size="16" />
        加载中...
      </div>
      <div v-else-if="error" class="list-note error-note">{{ error }}</div>
      <template v-else>
        <div
          v-for="entry in entries"
          :key="entry.path"
          class="file-row"
          :class="{ selected: selected.has(entry.path) }"
          @dblclick="entry.is_dir && navigateTo(entry.path)"
        >
          <!-- Checkbox — files only (folders aren't transferable, files-only contract). -->
          <span v-if="source === 'ssh'" class="col-check">
            <input
              v-if="!entry.is_dir"
              type="checkbox"
              class="row-check"
              :checked="selected.has(entry.path)"
              @click.stop
              @change="toggleSelected(entry.path)"
            />
          </span>
          <!-- Icon — dirs use accent-primary (blue), files a neutral muted glyph,
              so SFTP and SSH tabs read as one surface. -->
          <span class="col-icon" :class="entry.is_dir ? 'is-dir' : 'is-file'">
            <el-icon :size="entry.is_dir ? 15 : 14">
              <Folder v-if="entry.is_dir" />
              <Document v-else />
            </el-icon>
          </span>
          <!-- Name — dirs get accent-primary color + medium weight to match the
              SSH visual grammar (accent = navigable/interactive). -->
          <span class="col-name" :class="{ 'is-dir': entry.is_dir }">{{ entry.name }}</span>
          <!-- Size — right-aligned. Directories show dash. tabular-nums keeps the column aligned. -->
          <span class="col-size" :class="{ 'is-dir': entry.is_dir }">
            {{ entry.is_dir ? "—" : formatSize(entry.size) }}
          </span>
          <!-- Type — subtle text label (no badge). -->
          <span class="col-type" :class="{ 'is-dir': entry.is_dir }">{{ fileType(entry.name, entry.is_dir) }}</span>
          <!-- Modified — monospace for alignment -->
          <span class="col-mtime">{{ entry.modified ? formatTime(Number(entry.modified)) : "—" }}</span>
          <!-- Permissions — monospace, dimmer -->
          <span class="col-perm">{{ entry.permissions ? truncatePerm(entry.permissions) : "—" }}</span>
          <!-- Actions -->
          <span class="col-actions">
            <span class="row-action rename" title="重命名" @click.stop="openRenameDialog(entry)">
              <el-icon :size="12"><Edit /></el-icon>
            </span>
            <span class="row-action delete" title="删除" @click.stop="handleDelete(entry)">
              <el-icon :size="13"><Close /></el-icon>
            </span>
          </span>
        </div>
        <div v-if="entries.length === 0" class="list-note">空目录</div>
      </template>
    </div>

    <!-- Transfer overlay — 4-row layout matching ZmodemProgressOverlay -->
    <div v-if="transfer" class="transfer-overlay">
      <!-- Row 1: direction + filename + cancel/close -->
      <div class="tx-row1">
        <div class="tx-left">
          <span class="tx-arrow">{{ transfer.phase === "upload" ? "↑" : "↓" }}</span>
          <span class="tx-label">
            {{ transfer.done ? (transfer.phase === "upload" ? "上传完成" : "下载完成") : transfer.phase === "upload" ? "SFTP 上传" : "SFTP 下载" }}
          </span>
          <span v-if="transfer.fileCount > 1" class="tx-count">
            {{ Math.min(transfer.fileIndex + (transfer.done ? 0 : 1), transfer.fileCount) }}/{{ transfer.fileCount }}
          </span>
          <span class="tx-file">{{ transfer.currentFile || "准备中…" }}</span>
        </div>
        <MyButton v-if="transfer.done" variant="primary" size="small" @click="closeTransfer">关闭</MyButton>
        <MyButton v-else variant="danger" size="small" @click="onCancelTransfer">取消</MyButton>
      </div>

      <!-- Row 2: progress bar -->
      <MyProgress class="tx-bar" :percentage="pctDone" :stroke-width="8" :show-text="false" />

      <!-- Row 3: bytes + percent + speed -->
      <div class="tx-row3">
        <span>
          {{ formatSize(transfer.bytesDone) }} / {{ formatSize(transfer.bytesTotal) }}<template v-if="transfer.bytesTotal > 0"> ({{ pctDone }}%)</template>
        </span>
        <span>{{ speed >= 1 ? formatSize(speed) + "/s" : "—" }}</span>
      </div>

      <!-- Row 4: timing -->
      <div class="tx-row4">
        <span>开始 {{ formatClock(transfer.startTime) }}</span>
        <span>已用 {{ formatDuration(elapsed) }}</span>
        <span>剩余 {{ transfer.done ? "—" : formatDuration(remaining) }}</span>
      </div>

      <!-- Errors (only when done) -->
      <div v-if="transfer.done && transfer.errors.length > 0" class="tx-errors">
        <div v-for="(er, i) in transfer.errors.slice(0, 3)" :key="i">• {{ er }}</div>
        <div v-if="transfer.errors.length > 3">…等 {{ transfer.errors.length }} 个错误</div>
      </div>
    </div>

    <!-- Disconnected overlay — shown when the SSH channel dies.
        Covers the file list so the user can't interact with a dead session. -->
    <div v-if="status === 'disconnected' || status === 'error'" class="dead-overlay">
      <span class="dead-text">{{ status === "error" ? "连接失败" : "连接已断开" }}</span>
      <MyButton variant="primary" @click="emit('reconnect')">重新连接</MyButton>
    </div>
  </div>
</template>

<style scoped>
.sftp-panel {
  position: relative;
  width: 360px;
  min-width: 360px;
  background: var(--bg-elevated);
  border-left: 1px solid var(--border-default);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.sftp-panel.full-height {
  width: 100%;
  min-width: 0;
  border-left: none;
}

/* ─── 工具条 ─── */
.toolbar {
  padding: 6px 8px;
  border-bottom: 1px solid var(--border-default);
  display: flex;
  align-items: center;
  gap: 2px;
  flex-wrap: wrap;
  flex-shrink: 0;
}

.nav-btn {
  background: transparent;
  color: var(--text-secondary);
  padding: 5px 6px;
  font-size: 13px;
  border-radius: var(--radius-sm);
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid transparent;
  cursor: pointer;
  transition:
    background var(--duration-fast),
    color var(--duration-fast),
    border-color var(--duration-fast);
}

.nav-btn:hover:not(:disabled) {
  background: var(--bg-surface-hover);
  color: var(--text-primary);
  border-color: var(--border-default);
}

.nav-btn:disabled {
  color: var(--text-muted);
  opacity: 0.35;
  cursor: default;
}

.nav-count {
  margin-left: 2px;
  font-size: 11px;
}

.toolbar-sep {
  width: 1px;
  height: 20px;
  background: var(--border-default);
  margin: 0 4px;
}

/* ─── 路径条 ─── */
.path-bar {
  padding: 5px 8px;
  border-bottom: 1px solid var(--border-default);
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.term-glyph {
  flex-shrink: 0;
}

.path-input {
  flex: 1;
  min-width: 0;
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  padding: 4px 8px;
  color: var(--text-primary);
  font-size: 11px;
  font-family: "JetBrains Mono", "Cascadia Code", "Fira Code", ui-monospace, monospace;
  letter-spacing: -0.01em;
  outline: none;
}

.path-input:focus {
  border-color: var(--border-accent);
}

/* ─── 新建目录条 ─── */
.mkdir-bar {
  padding: 5px 8px;
  border-bottom: 1px solid var(--border-default);
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.mkdir-input {
  flex: 1;
  min-width: 0;
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  padding: 3px 8px;
  color: var(--text-primary);
  font-size: 12px;
  outline: none;
}

.mkdir-input:focus {
  border-color: var(--border-accent);
}

/* ─── 列头 ─── */
.column-headers {
  display: flex;
  align-items: center;
  padding: 6px 10px 6px 12px;
  border-bottom: 1px solid var(--border-subtle);
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-muted);
  gap: 10px;
  user-select: none;
  flex-shrink: 0;
}

.hdr-check {
  width: 18px;
  flex-shrink: 0;
}

.hdr-icon {
  width: 20px;
  flex-shrink: 0;
}

.hdr-name {
  flex: 1;
  min-width: 0;
}

.hdr-size {
  width: 64px;
  text-align: right;
  flex-shrink: 0;
}

.hdr-type {
  width: 50px;
  text-align: center;
  flex-shrink: 0;
}

.hdr-mtime {
  width: 110px;
  text-align: right;
  flex-shrink: 0;
}

.hdr-perm {
  width: 76px;
  text-align: right;
  flex-shrink: 0;
}

.hdr-actions {
  width: 36px;
  flex-shrink: 0;
}

/* ─── 文件列表 ─── */
.file-list {
  flex: 1;
  overflow-y: auto;
}

.list-note {
  padding: 24px;
  text-align: center;
  color: var(--text-muted);
  font-size: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
}

.error-note {
  color: var(--error);
}

.file-row {
  padding: 5px 10px 5px 12px;
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  cursor: pointer;
  color: var(--text-primary);
  transition: background var(--duration-fast) var(--ease-in-out);
}

.file-row:hover:not(.selected) {
  background: var(--bg-surface-hover);
}

.file-row.selected {
  background: var(--accent-primary-muted);
}

.col-check {
  width: 18px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
}

.row-check {
  margin: 0;
  accent-color: var(--accent-primary);
  cursor: pointer;
}

.col-icon {
  width: 20px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}

.col-icon.is-dir {
  color: var(--accent-primary);
}

.col-icon.is-file {
  color: var(--text-tertiary);
}

.col-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  letter-spacing: 0.01em;
}

.col-name.is-dir {
  color: var(--accent-primary);
  font-weight: 500;
}

.col-size {
  width: 64px;
  text-align: right;
  flex-shrink: 0;
  color: var(--text-secondary);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}

.col-size.is-dir {
  color: var(--text-muted);
}

.col-type {
  width: 50px;
  text-align: center;
  flex-shrink: 0;
  font-size: 10px;
  letter-spacing: 0.02em;
  color: var(--text-tertiary);
}

.col-type.is-dir {
  color: var(--accent-secondary);
}

.col-mtime {
  width: 110px;
  text-align: right;
  flex-shrink: 0;
  color: var(--text-secondary);
  font-size: 10px;
  font-family: "JetBrains Mono", "Cascadia Code", "Fira Code", ui-monospace, monospace;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.01em;
}

.col-perm {
  width: 76px;
  text-align: right;
  flex-shrink: 0;
  color: var(--text-muted);
  font-size: 10px;
  font-family: "JetBrains Mono", "Cascadia Code", "Fira Code", ui-monospace, monospace;
  letter-spacing: -0.02em;
}

.col-actions {
  width: 36px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 4px;
  justify-content: flex-end;
}

.row-action {
  color: var(--text-muted);
  cursor: pointer;
  line-height: 1;
  opacity: 0.6;
  display: flex;
}

.row-action:hover {
  opacity: 1;
  color: var(--text-primary);
}

.row-action.delete:hover {
  color: var(--error);
}

/* ─── 传输浮层 ─── */
.transfer-overlay {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  background: var(--glass-bg);
  border-top: 1px solid var(--border-emphasis);
  padding: 8px 16px 10px;
  font-size: 12px;
  font-family: "Cascadia Code", Consolas, monospace;
  z-index: 10;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.tx-row1 {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}

.tx-left {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.tx-arrow {
  color: var(--accent-primary);
  font-size: 16px;
  line-height: 1;
}

.tx-label {
  color: var(--success);
  font-weight: 600;
  white-space: nowrap;
}

.tx-count {
  color: var(--text-tertiary);
  white-space: nowrap;
}

.tx-file {
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tx-bar :deep(.el-progress-bar__outer) {
  background: var(--bg-surface-active);
  border-radius: 4px;
}

.tx-row3 {
  display: flex;
  justify-content: space-between;
  color: var(--text-secondary);
}

.tx-row4 {
  display: flex;
  justify-content: space-between;
  color: var(--text-tertiary);
  font-size: 11px;
}

.tx-errors {
  color: var(--error);
  max-height: 42px;
  overflow-y: auto;
  line-height: 1.4;
}

/* ─── 断连浮层 ─── */
.dead-overlay {
  position: absolute;
  inset: 0;
  background: var(--bg-overlay);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  z-index: 15;
}

.dead-text {
  color: var(--text-secondary);
  font-size: 13px;
}
</style>
