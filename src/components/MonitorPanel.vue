<!-- 监控面板：多主机资源一览。与底栏 ServerInfoPanel 的分工是——
      底栏是「当前这一个会话」的常驻速览（5s 一条），本面板是「所有已保存
      连接」的可开可关详情视图：列可选、刷新率可调、行内直达终端。

      采集走 sshGetServerInfo（同一个 exec 探针，一次往返拿齐 CPU/内存/磁盘/
      磁盘 I/O/网络/延迟），因此需要连接凭据；每个连接都是「探测即连」，
      没有常驻会话。探测失败的行显示原因，不阻塞其他行。

      刷新率下限 1s：探针本身带 sleep 1（CPU 需要两个 /proc/stat 采样点），
      所以 1s 是这个接口的物理下限，不是随手定的选项。 -->
<script setup lang="ts">
import { computed, onUnmounted, reactive, ref, watch } from "vue";
import { MyButton, MyIcon, MyIconButton } from "myui";
import { sshGetServerInfo, type ConnectionConfig, type ServerInfo } from "@/api";
import { connectionsStore } from "@/store/connections";
import { connect, sessions } from "@/store/sessions";

defineOptions({ name: "MonitorPanel" });

const props = defineProps<{
  /** 面板是否可见。关掉时停掉所有轮询 —— 十几个连接同时 1s 探测会
      把 CPU 和远端 sshd 都打满。 */
  open: boolean;
}>();

const emit = defineEmits<{
  close: [];
}>();

/** 可显示的列。value 与 ServerInfo 字段对应。 */
const COLUMNS = [
  { key: "cpu", label: "CPU" },
  { key: "mem", label: "内存" },
  { key: "disk", label: "磁盘" },
  { key: "diskIo", label: "磁盘 I/O" },
  { key: "net", label: "网络" },
  { key: "latency", label: "延迟" },
] as const;

type ColumnKey = (typeof COLUMNS)[number]["key"];

const REFRESH_CHOICES = [1, 2, 5, 10, 30] as const;

const visibleCols = ref<Set<ColumnKey>>(new Set(["cpu", "mem", "disk", "latency"]));
const refreshSecs = ref<number>(5);
const search = ref("");

/** 每个连接的最近一次探测结果。key = connection id。fails 为连续失败
 *  次数（探测熔断用，见 probe）。 */
interface Row {
  info: ServerInfo | null;
  error: string | null;
  loading: boolean;
  fails: number;
}
const rows = reactive(new Map<string, Row>());

/** 只探测 SSH/SFTP 连接——FTP 与本地终端没有 exec 通道。 */
const monitorable = computed<ConnectionConfig[]>(() =>
  connectionsStore.connections.filter((c) => c.conn_type === "ssh" || c.conn_type === "sftp"),
);

const shown = computed<ConnectionConfig[]>(() => {
  const q = search.value.trim().toLowerCase();
  if (!q) return monitorable.value;
  return monitorable.value.filter(
    (c) => c.name.toLowerCase().includes(q) || c.host.toLowerCase().includes(q),
  );
});

/** 视图模型行：连接 + 它的探测结果合成一个对象，模板里不必到处
 *  rows.get(c.id)?.info 三连取。 */
interface ViewRow {
  id: string;
  name: string;
  host: string;
  port: number;
  conn: ConnectionConfig;
  info: ServerInfo | null;
  error: string | null;
  loading: boolean;
}

const viewRows = computed<ViewRow[]>(() =>
  shown.value.map((c) => {
    const r = rows.get(c.id);
    return {
      id: c.id,
      name: c.name,
      host: c.host,
      port: c.port,
      conn: c,
      info: r?.info ?? null,
      error: r?.error ?? null,
      loading: r?.loading ?? false,
    };
  }),
);

function rowOf(id: string): Row {
  let r = rows.get(id);
  if (!r) {
    r = { info: null, error: null, loading: false, fails: 0 };
    rows.set(id, r);
  }
  return r;
}

/** 连接 id → 活跃 session id。没有已连接标签时返回 null。
 *  探测复用该会话的 SSH 连接（exec_once 在其上开 exec 通道），
 *  不额外建连 —— 否则每秒一次刷新会在服务器上留下同样多的连接。 */
function activeSessionId(connectionId: string): string | null {
  const tab = sessions.tabs.find(
    (t) => t.connectionId === connectionId && t.sessionId && t.status === "connected",
  );
  return tab?.sessionId ?? null;
}

/** 探测一个连接。失败不影响其他行，错误文本留在行内。连续 2 次失败后
 *  熔断跳过——死会话每 tick 重试只会刷日志与红色错误（后端日志实测单个
 *  死会话一天被重试 7104 次）；面板重开/搜索/刷新率变化由 startPolling 重置。 */
async function probe(conn: ConnectionConfig): Promise<void> {
  const row = rowOf(conn.id);
  if (row.fails >= 2) return;
  const sid = activeSessionId(conn.id);
  if (!sid) {
    // 没有打开的终端就没有可复用的会话。这不是错误，是面板的既定范围，
    // 用 hint 提示而不是红色报错。
    row.info = null;
    row.error = null;
    return;
  }
  row.loading = true;
  try {
    row.info = await sshGetServerInfo(sid);
    row.error = null;
    row.fails = 0;
  } catch (e) {
    row.error = String(e);
    row.info = null;
    row.fails += 1;
  } finally {
    row.loading = false;
  }
}

let timer: ReturnType<typeof setInterval> | null = null;

function startPolling(): void {
  stopPolling();
  // 轮询重启 = 一次显式的「再试一轮」：清掉各行的熔断计数。
  for (const r of rows.values()) r.fails = 0;
  const tick = () => {
    for (const c of shown.value) void probe(c);
  };
  tick();
  timer = setInterval(tick, refreshSecs.value * 1000);
}

function stopPolling(): void {
  if (timer !== null) {
    clearInterval(timer);
    timer = null;
  }
}

watch(
  () => [props.open, refreshSecs.value] as const,
  ([open]) => {
    if (open) startPolling();
    else stopPolling();
  },
  { immediate: true },
);

watch(search, () => {
  if (props.open) startPolling();
});

onUnmounted(stopPolling);

// ── 格式化 ──
function fmtBytes(b: number): string {
  if (b <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(b) / Math.log(1024)));
  const v = b / Math.pow(1024, i);
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

function fmtRate(bps: number): string {
  return `${fmtBytes(bps)}/s`;
}

function pctColor(pct: number): string {
  if (pct > 85) return "var(--error)";
  if (pct > 60) return "var(--warning)";
  return "var(--success)";
}

function toggleCol(key: ColumnKey): void {
  const next = new Set(visibleCols.value);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  visibleCols.value = next;
}

function openTerminal(conn: ConnectionConfig): void {
  void connect(conn);
  emit("close");
}
</script>

<template>
  <div v-if="open" class="monitor-panel">
    <header class="mp-header">
      <span class="mp-title">
        <MyIcon :size="14" name="Monitor" />
        监控
      </span>
      <div class="mp-tools">
        <select v-model="refreshSecs" class="mp-select" title="刷新间隔">
          <option v-for="s in REFRESH_CHOICES" :key="s" :value="s">{{ s }}s</option>
        </select>
        <MyIconButton
          title="关闭面板"
          label="关闭面板"
          size="14"
          name="Close"
          @click="emit('close')"
        />
      </div>
    </header>

    <div class="mp-hint">
      仅监控已打开终端的连接（探测复用其 SSH 会话，不额外建连）。磁盘 I/O
      只统计硬件块设备，网络只统计物理网卡，均不计入分区与虚拟设备。
    </div>

    <div class="mp-search">
      <input v-model="search" class="mp-input" placeholder="搜索连接名称或地址..." />
    </div>

    <div class="mp-cols">
      <button
        v-for="col in COLUMNS"
        :key="col.key"
        class="mp-col-chip"
        :class="{ on: visibleCols.has(col.key) }"
        @click="toggleCol(col.key)"
      >
        {{ col.label }}
      </button>
    </div>

    <div class="mp-body">
      <table class="mp-table">
        <thead>
          <tr>
            <th class="mp-host-col">主机</th>
            <th v-if="visibleCols.has('cpu')">CPU</th>
            <th v-if="visibleCols.has('mem')">内存</th>
            <th v-if="visibleCols.has('disk')">磁盘</th>
            <th v-if="visibleCols.has('diskIo')">磁盘 I/O</th>
            <th v-if="visibleCols.has('net')">网络</th>
            <th v-if="visibleCols.has('latency')">延迟</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          <!-- 视图模型行：把「连接 + 它的探测结果」合成一个对象，模板里就不必
               到处写 rows.get(c.id)?.info 这种三连取。 -->
          <tr v-for="r in viewRows" :key="r.id">
            <td class="mp-host-col">
              <div class="mp-name">{{ r.name }}</div>
              <div class="mp-sub">{{ r.host }}:{{ r.port }}</div>
            </td>
            <td v-if="visibleCols.has('cpu')">
              <template v-if="r.info">
                <div class="mp-bar">
                  <div
                    class="mp-bar-fill"
                    :style="{
                      width: `${Math.min(100, r.info.cpuUsagePct)}%`,
                      background: pctColor(r.info.cpuUsagePct),
                    }"
                  />
                </div>
                <span class="mp-num">{{ r.info.cpuUsagePct.toFixed(0) }}%</span>
              </template>
              <span v-else-if="r.error" class="mp-err" :title="r.error">错误</span>
              <span v-else class="mp-na">{{ r.loading ? "…" : "—" }}</span>
            </td>
            <td v-if="visibleCols.has('mem')">
              <template v-if="r.info">
                <div class="mp-bar">
                  <div
                    class="mp-bar-fill"
                    :style="{
                      width: `${Math.min(100, r.info.memUsagePct)}%`,
                      background: pctColor(r.info.memUsagePct),
                    }"
                  />
                </div>
                <span class="mp-num">{{ r.info.memUsagePct.toFixed(0) }}%</span>
              </template>
              <span v-else-if="r.error" class="mp-err" :title="r.error">错误</span>
              <span v-else class="mp-na">{{ r.loading ? "…" : "—" }}</span>
            </td>
            <td v-if="visibleCols.has('disk')">
              <template v-if="r.info">
                <span class="mp-num">{{ r.info.diskTotalPct.toFixed(0) }}%</span>
                <span class="mp-sub">{{ fmtBytes(r.info.diskTotalBytes) }}</span>
              </template>
              <span v-else-if="r.error" class="mp-err" :title="r.error">错误</span>
              <span v-else class="mp-na">{{ r.loading ? "…" : "—" }}</span>
            </td>
            <td v-if="visibleCols.has('diskIo')">
              <template v-if="r.info">
                <span class="mp-num">R {{ fmtRate(r.info.diskReadBps) }}</span>
                <span class="mp-sub">W {{ fmtRate(r.info.diskWriteBps) }}</span>
              </template>
              <span v-else-if="r.error" class="mp-err" :title="r.error">错误</span>
              <span v-else class="mp-na">{{ r.loading ? "…" : "—" }}</span>
            </td>
            <td v-if="visibleCols.has('net')">
              <template v-if="r.info">
                <span class="mp-num">↓ {{ fmtRate(r.info.netRxBps) }}</span>
                <span class="mp-sub">↑ {{ fmtRate(r.info.netTxBps) }}</span>
              </template>
              <span v-else-if="r.error" class="mp-err" :title="r.error">错误</span>
              <span v-else class="mp-na">{{ r.loading ? "…" : "—" }}</span>
            </td>
            <td v-if="visibleCols.has('latency')">
              <span v-if="r.info" class="mp-num">{{ r.info.latencyMs }} ms</span>
              <span v-else-if="r.error" class="mp-err" :title="r.error">错误</span>
              <span v-else class="mp-na">{{ r.loading ? "…" : "—" }}</span>
            </td>
            <td class="mp-ops">
              <MyButton size="small" @click="openTerminal(r.conn)">打开终端</MyButton>
            </td>
          </tr>
          <tr v-if="viewRows.length === 0">
            <td :colspan="COLUMNS.length + 2" class="mp-empty">
              {{ monitorable.length === 0 ? "暂无 SSH/SFTP 连接" : "没有匹配的连接" }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<style scoped>
.monitor-panel {
  position: absolute;
  top: var(--chrome-top-height);
  right: 0;
  bottom: 0;
  width: 720px;
  max-width: 90vw;
  z-index: 40;
  display: flex;
  flex-direction: column;
  /* strong 档玻璃：贴在壳层之上的次级面板，介于壳层（base）与弹层（popup）
     之间——比壳层实（压得住终端内容），又不必像弹层那么实。 */
  background: var(--glass-bg-strong);
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  border-left: 1px solid var(--glass-border-strong);
  box-shadow: var(--glass-highlight), var(--shadow-lg);
}

.mp-header {
  flex-shrink: 0;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 8px 0 12px;
  border-bottom: 1px solid var(--border-subtle);
}

.mp-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
}

.mp-tools {
  display: flex;
  align-items: center;
  gap: 6px;
}

.mp-select {
  height: 22px;
  font-size: var(--fs-2xs);
  color: var(--text-primary);
  background: var(--bg-base);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-sm);
}

.mp-hint {
  flex-shrink: 0;
  padding: 8px 12px;
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  line-height: 1.6;
  border-bottom: 1px solid var(--border-subtle);
}

.mp-search {
  flex-shrink: 0;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-subtle);
}

.mp-input {
  width: 100%;
  height: 26px;
  padding: 0 8px;
  font-size: var(--fs-xs);
  color: var(--text-primary);
  background: var(--bg-base);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-sm);
}

.mp-cols {
  flex-shrink: 0;
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-subtle);
}

.mp-col-chip {
  padding: 2px 8px;
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  background: var(--bg-base);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-sm);
  cursor: pointer;
}

.mp-col-chip.on {
  color: var(--accent-primary);
  border-color: var(--accent-primary);
}

.mp-body {
  flex: 1;
  overflow: auto;
}

.mp-table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--fs-xs);
}

.mp-table th {
  position: sticky;
  top: 0;
  z-index: 1;
  text-align: left;
  font-weight: 500;
  font-size: var(--fs-2xs);
  color: var(--text-secondary);
  padding: 6px 8px;
  background: var(--glass-bg-strong);
  border-bottom: 1px solid var(--border-subtle);
  white-space: nowrap;
}

.mp-table td {
  padding: 6px 8px;
  border-bottom: 1px solid var(--border-subtle);
  vertical-align: middle;
}

.mp-host-col {
  min-width: 140px;
}

.mp-name {
  font-weight: 500;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 180px;
}

.mp-sub {
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  white-space: nowrap;
}

.mp-bar {
  width: 48px;
  height: 4px;
  border-radius: 2px;
  background: var(--border-subtle);
  overflow: hidden;
  display: inline-block;
  vertical-align: middle;
  margin-right: 6px;
}

.mp-bar-fill {
  height: 100%;
  border-radius: 2px;
}

.mp-num {
  font-size: var(--fs-2xs);
  color: var(--text-secondary);
  white-space: nowrap;
}

.mp-na {
  color: var(--text-tertiary);
}

.mp-ops {
  white-space: nowrap;
}

.mp-empty {
  padding: 24px;
  text-align: center;
  color: var(--text-tertiary);
}
</style>
