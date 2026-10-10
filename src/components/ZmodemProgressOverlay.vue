<!-- ZMODEM 传输进度浮层：方向 / 文件名 / 进度条 / 速度 / 开始·已用·剩余时间。
     从 src-legacy/components/ZmodemProgressOverlay.tsx 逐行移植。
     status 未传（App.vue 全局挂载点）时按空闲态处理、不渲染任何内容；
     TerminalPanel 嵌入并传实时 status 时行为与旧版完全一致。 -->
<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import { MyButton, MyIcon, MyProgress } from "myui";
import type { ZmodemStatus } from "@/zmodem-bridge";

defineOptions({ name: "ZmodemProgressOverlay" });

const IDLE_STATUS: ZmodemStatus = {
  active: false,
  direction: null,
  currentFile: "",
  bytesTransferred: 0,
  bytesTotal: 0,
  speedBps: 0,
  error: null,
  startTime: 0,
};

const props = withDefaults(
  defineProps<{
    status?: ZmodemStatus;
  }>(),
  { status: undefined },
);

const emit = defineEmits<{
  (e: "cancel"): void;
}>();

// 兜底空闲态：无 props 的挂载点（App.vue）永远不渲染。
const st = computed<ZmodemStatus>(() => props.status ?? IDLE_STATUS);

// 1 秒 tick：驱动 ETA / 已用时间的实时刷新。
const tick = ref(0);
let timer: ReturnType<typeof setInterval> | null = null;

watch(
  () => st.value.active,
  (active) => {
    if (timer) {
      clearInterval(timer);
      timer = null;
    }
    if (active) timer = setInterval(() => tick.value++, 1000);
  },
  { immediate: true },
);

onUnmounted(() => {
  if (timer) {
    clearInterval(timer);
    timer = null;
  }
});

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function formatSpeed(bps: number): string {
  if (bps < 1) return "—";
  return `${formatBytes(bps)}/s`;
}

/** 格式化为 HH:MM:SS */
function formatTime(epochMs: number): string {
  if (!epochMs) return "—";
  const d = new Date(epochMs);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  return `${hh}:${mm}:${ss}`;
}

/** 格式化时长 → "X分Y秒" 或 "X小时Y分" */
function formatDuration(seconds: number): string {
  if (!isFinite(seconds) || seconds <= 0) return "—";
  if (seconds < 60) return `${Math.ceil(seconds)}秒`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = Math.round(seconds % 60);
    return `${m}分${s}秒`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.round((seconds % 3600) / 60);
  return `${h}小时${m}分`;
}

const percent = computed(() =>
  st.value.bytesTotal > 0 ? Math.min(100, (st.value.bytesTransferred / st.value.bytesTotal) * 100) : 0,
);

const dirLabel = computed(() => (st.value.direction === "upload" ? "上传" : "下载"));

// 计算 ETA 和已用时间（elapsed 依赖 tick：每秒重算 Date.now()）
const elapsedMs = computed(() => {
  void tick.value;
  return st.value.startTime > 0 ? Date.now() - st.value.startTime : 0;
});

const etaSeconds = computed(() => {
  const remaining = st.value.bytesTotal - st.value.bytesTransferred;
  return st.value.speedBps > 0 ? remaining / st.value.speedBps : Infinity;
});
</script>

<template>
  <div v-if="st.active" class="zm-overlay">
    <!-- 第一行：方向 + 文件名 + 取消按钮 -->
    <div class="row-top">
      <div class="left">
        <MyIcon class="dir-icon" :size="16" :name="st.direction === 'upload' ? 'ArrowUp' : 'ArrowDown'" />
        <span class="dir-label">ZMODEM {{ dirLabel }}</span>
        <span class="file">{{ st.currentFile || "—" }}</span>
        <span v-if="st.error" class="err">错误：{{ st.error }}</span>
      </div>
      <MyButton variant="ghost-danger" size="small" @click="emit('cancel')">取消</MyButton>
    </div>

    <!-- 第二行：进度条 -->
    <MyProgress
      class="zm-progress"
      :percentage="percent"
      :stroke-width="8"
      :show-text="false"
      :transition-duration="120"
    />

    <!-- 第三行：字节数 / 百分比 / 速度 -->
    <div class="row-bytes">
      <span>
        {{ formatBytes(st.bytesTransferred) }} / {{ formatBytes(st.bytesTotal) }}<template v-if="st.bytesTotal > 0">
          ({{ percent.toFixed(1) }}%)</template>
      </span>
      <span>{{ formatSpeed(st.speedBps) }}</span>
    </div>

    <!-- 第四行：开始时间 / 已用时间 / 预计剩余 -->
    <div class="row-times">
      <span>开始 {{ formatTime(st.startTime) }}</span>
      <span>已用 {{ elapsedMs > 0 ? formatDuration(elapsedMs / 1000) : "—" }}</span>
      <span>剩余 {{ formatDuration(etaSeconds) }}</span>
    </div>
  </div>
</template>

<style scoped>
.zm-overlay {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  background: var(--glass-bg);
  border-top: 1px solid var(--border-emphasis);
  padding: 8px 16px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-family: "Cascadia Code", Consolas, monospace;
  font-size: 12px;
  color: var(--text-primary);
  z-index: 10;
}

.row-top {
  display: flex;
  justify-content: space-between;
  gap: 12px;
}

.row-top .left {
  display: flex;
  gap: 8px;
  align-items: center;
  min-width: 0;
}

.dir-icon {
  color: var(--accent-primary);
  flex: none;
}

.dir-label {
  color: var(--success);
  font-weight: 600;
}

.file {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-secondary);
  min-width: 0;
}

.err {
  color: var(--error);
}

/* 过渡时长改用 MyProgress 的 transitionDuration prop（v0.28.0 起）——
   Short + LINEAR。progress 以固定 10 Hz 到达（Rust 侧把 zmodem_progress
   节流到 100ms）。EP 内置的是 .6s 长缓动，会被不断重新定向、永远跑不完，
   结果是进度条落后百分比几十个点。

   v0.27.x 时这里只能 `:deep(.el-progress-bar__inner)` 打穿 EP 内部类名，
   myui 换底层会静默失效；现已由 MyUI 自己写 --my-progress-transition 接管，
   故不再有任何对 EP 内部结构的耦合。 */
.zm-progress {
  margin: 0;
}
.zm-progress :deep(.el-progress-bar__outer) {
  background: var(--bg-surface);
}

.row-bytes {
  display: flex;
  justify-content: space-between;
  color: var(--text-secondary);
}

.row-times {
  display: flex;
  justify-content: space-between;
  color: var(--text-tertiary);
  font-size: 11px;
}
</style>
