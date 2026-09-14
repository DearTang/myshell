<!-- 服务器监控条（活动 SSH 终端）：5s 轮询 sshGetServerInfo，展示 系统/CPU/内存/磁盘
     占用与 stale 标记。自 src-legacy/components/ServerInfoPanel.tsx 原样移植；
     占用条由本地 div 换为 myui MyProgress（阈值配色经 EP 主题桥映射回 MyShell 令牌）。
     说明：任务书提到的"网络展示"在旧版组件中不存在（ServerInfo 亦无网络字段），
     按"行为语义零变化"不予新增。 -->
<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Cpu, DataLine, Files, TrendCharts } from "@element-plus/icons-vue";
import { MyProgress } from "myui";
import { sshGetServerInfo, type ServerInfo } from "@/api";

defineOptions({ name: "ServerInfoPanel" });

interface Props {
  sessionId: string;
  active: boolean;
}

const props = defineProps<Props>();

const info = ref<ServerInfo | null>(null);
const error = ref<string | null>(null);

// 轮询语义与旧版 useEffect 一致：仅 active 时启动；立即刷一次，之后每 5s 一次；
// 依赖变化时取消在途请求的落库并清理定时器；info/error 不因切换而提前清空。
watch(
  () => [props.sessionId, props.active] as const,
  ([sessionId, active]) => {
    if (!active) return;
    let cancelled = false;
    const refresh = async (): Promise<void> => {
      try {
        const v = await sshGetServerInfo(sessionId);
        if (!cancelled) {
          info.value = v;
          error.value = null;
        }
      } catch (e) {
        if (!cancelled) error.value = String(e);
      }
    };
    void refresh();
    const tid = setInterval(() => {
      void refresh();
    }, 5000);
    return () => {
      cancelled = true;
      clearInterval(tid);
    };
  },
  { immediate: true },
);

const stale = computed(() => info.value?.stale ?? false);

// 磁盘文案拼成完整字符串（与旧版模板字符串逐字一致，避免模板空白折叠差异）。
const diskContent = computed(() => {
  const d = info.value;
  if (!d) return "";
  const base = `总 ${formatBytes(d.diskTotalBytes)} (${d.diskTotalPct.toFixed(0)}%)`;
  if (d.diskMaxMount) {
    return `${base} · 最高 ${d.diskMaxMount} ${d.diskMaxUsed}/${d.diskMaxSize} (${d.diskMaxPct.toFixed(0)}%)`;
  }
  return base;
});

function formatBytes(b: number): string {
  if (b <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(b) / Math.log(1024)));
  const v = b / Math.pow(1024, i);
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

// 占用条阈值配色（>85% 红 / >60% 黄 / 其余绿），经 myui→EP 主题桥取 MyShell 令牌。
function barStatus(pct: number): "success" | "warning" | "exception" {
  if (pct > 85) return "exception";
  if (pct > 60) return "warning";
  return "success";
}

function barColor(pct: number): string {
  if (pct > 85) return "var(--error)";
  if (pct > 60) return "var(--warning)";
  return "var(--success)";
}

// EP 的 percentage 需夹在 0-100（旧版 UsageBar 渲染前同样夹取）。
function clampPct(pct: number): number {
  return Math.max(0, Math.min(100, pct));
}
</script>

<template>
  <div class="server-info">
    <div v-if="!info && !error" class="hint-loading">加载中…</div>
    <div v-if="error" class="hint-error">{{ error }}</div>
    <template v-if="info">
      <div class="group is-divided">
        <!-- Single-row metric chip: icon + label + content + optional bar, all
             inline. Designed for a compact bottom status bar. -->
        <div class="chip" :class="{ stale }" :title="`内核 ${info.kernel || '-'}`">
          <span class="chip-icon"><el-icon :size="12"><Files /></el-icon></span>
          <span class="chip-label">系统</span>
          <span class="chip-content">{{ info.osPretty || "unknown" }}</span>
        </div>
      </div>
      <div class="group is-divided">
        <div class="chip" :class="{ stale }">
          <span class="chip-icon"><el-icon :size="12"><Cpu /></el-icon></span>
          <span class="chip-label">CPU</span>
          <span class="chip-content">{{ info.cpuCores }} 核 {{ info.cpuUsagePct.toFixed(1) }}%</span>
          <div class="usage-bar" :class="{ stale }">
            <MyProgress
              :percentage="clampPct(info.cpuUsagePct)"
              :status="barStatus(info.cpuUsagePct)"
              :color="barColor(info.cpuUsagePct)"
              :stroke-width="4"
              :show-text="false"
            />
          </div>
        </div>
      </div>
      <div class="group is-divided">
        <div class="chip" :class="{ stale }">
          <span class="chip-icon"><el-icon :size="12"><TrendCharts /></el-icon></span>
          <span class="chip-label">内存</span>
          <span class="chip-content">
            {{ formatBytes(info.memUsedBytes) }}/{{ formatBytes(info.memTotalBytes) }} {{ info.memUsagePct.toFixed(0) }}%
          </span>
          <div class="usage-bar" :class="{ stale }">
            <MyProgress
              :percentage="clampPct(info.memUsagePct)"
              :status="barStatus(info.memUsagePct)"
              :color="barColor(info.memUsagePct)"
              :stroke-width="4"
              :show-text="false"
            />
          </div>
        </div>
      </div>
      <div class="group">
        <div
          class="chip"
          :class="{ stale }"
          :title="info.diskMaxDev ? `最高分区设备 ${info.diskMaxDev} 挂载于 ${info.diskMaxMount}` : undefined"
        >
          <span class="chip-icon"><el-icon :size="12"><DataLine /></el-icon></span>
          <span class="chip-label">磁盘</span>
          <span class="chip-content">{{ diskContent }}</span>
          <div class="usage-bar" :class="{ stale }">
            <MyProgress
              :percentage="clampPct(info.diskMaxPct)"
              :status="barStatus(info.diskMaxPct)"
              :color="barColor(info.diskMaxPct)"
              :stroke-width="4"
              :show-text="false"
            />
          </div>
        </div>
      </div>
      <span v-if="stale" class="stale-flag" title="刷新超时">● stale</span>
    </template>
  </div>
</template>

<style scoped>
/* 旧版对应组件为全内联样式，此处逐条转为 scoped CSS；色值一律走令牌。 */

.server-info {
  height: 36px;
  min-height: 36px;
  /* 旧版即引用未定义令牌（--bg-sidebar/--border 在两版主题里均未定义 →
     背景透明、顶边框不渲染）。逐字保留以维持行为一致；若壳层日后补充
     这些令牌则自动生效。 */
  background: var(--bg-sidebar);
  border-top: 1px solid var(--border);
  display: flex;
  align-items: stretch;
  overflow-x: auto;
  overflow-y: hidden;
}

.hint-loading {
  padding: 0 16px;
  font-size: 11px;
  color: var(--text-muted);
  align-self: center;
}

.hint-error {
  padding: 0 14px;
  font-size: 11px;
  color: var(--error);
  align-self: center;
}

.group.is-divided {
  border-right: 1px solid var(--border);
}

.chip {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 12px;
  height: 100%;
  opacity: 1;
  min-width: 0;
}

.chip.stale {
  opacity: 0.55;
}

.chip-icon {
  font-size: 12px;
  flex-shrink: 0;
}

.chip-label {
  font-size: 10px;
  color: var(--text-muted);
  font-weight: 600;
  letter-spacing: 0.3px;
  text-transform: uppercase;
  white-space: nowrap;
  flex-shrink: 0;
}

.chip-content {
  font-size: 12px;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
}

/* 占用条：40×4px 迷你条。myui MyProgress（el-progress 封装）+
   少量 :deep 微调，使其与旧版本地 UsageBar 像素级一致（轨道色 --bg-input、
   圆角 2px、width .4s/background .3s 过渡）；填充色经 color 属性透传令牌。 */
.usage-bar {
  width: 40px;
  height: 4px;
  border-radius: 2px;
  overflow: hidden;
  opacity: 1;
  flex-shrink: 0;
}

.usage-bar.stale {
  opacity: 0.4;
}

.usage-bar :deep(.el-progress) {
  width: 100%;
}

.usage-bar :deep(.el-progress .el-progress-bar__outer) {
  background-color: var(--bg-input);
  border-radius: 2px;
}

.usage-bar :deep(.el-progress .el-progress-bar__inner) {
  border-radius: 2px;
  transition: width 0.4s ease, background-color 0.3s;
}

.stale-flag {
  margin-left: auto;
  align-self: center;
  padding: 0 12px;
  font-size: 9px;
  color: var(--warning);
}
</style>
