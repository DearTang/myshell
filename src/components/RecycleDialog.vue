<!-- 回收站对话框：列出软删除的连接，支持找回 / 彻底删除 / 清空。
     从 src-legacy/components/RecycleDialog.tsx 原样移植（注释与文案逐字保留）。
     自包含：操作成功后调用 reloadConnections() 刷新侧栏连接列表。
     旧版的 window.confirm/alert 按迁移约定替换为 myui confirmDialog/toast（文案不变）。
     壳：MyDialog（dismissable 保持旧版「点遮罩/右上角 ✕ 关闭」；Esc 关闭旧版没有，
     显式禁用以守住行为不变）。清空按钮随旧版留在 #header 行内；#footer 放底部提示条。 -->
<script setup lang="ts">
import { onMounted, ref } from "vue";
import { Delete } from "@element-plus/icons-vue";
import { MyButton, MyDialog, confirmDialog, toast } from "myui";
import type { ConnType, DeletedConnection } from "@/api";
import {
  getDeletedConnections,
  restoreConnection,
  purgeConnection,
  purgeAllDeletedConnections,
} from "@/api";
import { reloadConnections } from "@/store/connections";

defineOptions({ name: "RecycleDialog" });

const emit = defineEmits<{ close: [] }>();

const items = ref<DeletedConnection[]>([]);
const loading = ref(true);
const busyId = ref<string | null>(null);

async function reload(): Promise<void> {
  try {
    const list = await getDeletedConnections();
    items.value = list;
  } catch (e) {
    console.error("[recycle] load failed", e);
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  void reload();
});

async function handleRestore(id: string): Promise<void> {
  busyId.value = id;
  try {
    await restoreConnection(id);
    void reloadConnections();
    await reload();
  } catch (e) {
    toast(`找回失败: ${e}`, { type: "error" });
  } finally {
    busyId.value = null;
  }
}

async function handlePurgeOne(id: string, name: string): Promise<void> {
  const ok = await confirmDialog({
    message: `彻底删除「${name}」？此操作不可恢复。`,
    title: "彻底删除",
    type: "warning",
  });
  if (!ok) return;
  busyId.value = id;
  try {
    await purgeConnection(id);
    void reloadConnections();
    await reload();
  } catch (e) {
    toast(`删除失败: ${e}`, { type: "error" });
  } finally {
    busyId.value = null;
  }
}

async function handlePurgeAll(): Promise<void> {
  if (items.value.length === 0) return;
  const ok = await confirmDialog({
    message: `确定清空回收站？将彻底删除 ${items.value.length} 个连接，此操作不可恢复。`,
    title: "清空回收站",
    type: "warning",
  });
  if (!ok) return;
  busyId.value = "__all__";
  try {
    await purgeAllDeletedConnections();
    void reloadConnections();
    await reload();
  } catch (e) {
    toast(`清空失败: ${e}`, { type: "error" });
  } finally {
    busyId.value = null;
  }
}

// ── 连接类型图标（沿用旧 iconfont 类与 ConnIcon 的配色映射） ──
const CONN_ICON_CLASS: Record<ConnType, string> = {
  ssh: "icon-fuwuqi",
  sftp: "icon-SFTP",
  ftp: "icon-ftp",
  local: "icon-diannao",
};

const CONN_COLOR: Record<ConnType, string> = {
  ssh: "var(--accent-primary)",
  sftp: "var(--accent-secondary)",
  ftp: "var(--warning)",
  local: "var(--text-secondary)",
};

function connIconClass(connType: ConnType): string {
  return CONN_ICON_CLASS[connType] || CONN_ICON_CLASS.ssh;
}

function connColor(connType: ConnType): string {
  return CONN_COLOR[connType] || CONN_COLOR.ssh;
}

/** Format an ISO timestamp as a Chinese relative-time string ("3 分钟前"). */
function relativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const diff = Date.now() - then;
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return "刚刚";
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min} 分钟前`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr} 小时前`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day} 天前`;
  const mon = Math.floor(day / 30);
  if (mon < 12) return `${mon} 个月前`;
  return `${Math.floor(mon / 12)} 年前`;
}
</script>

<template>
  <MyDialog
    :model-value="true"
    title="找回连接"
    size="lg"
    align-center
    dismissable
    :close-on-press-escape="false"
    class="recycle-dialog"
    @cancel="emit('close')"
  >
    <template #header>
      <div class="dlg-header">
        <span class="header-icon">
          <el-icon :size="18"><Delete /></el-icon>
        </span>
        <div class="header-text">
          <div class="header-title">找回连接</div>
          <div class="header-sub">
            {{ loading ? "加载中…" : `${items.length} 个已删除的连接` }}
          </div>
        </div>
        <MyButton
          v-if="items.length > 0"
          variant="danger"
          size="small"
          :disabled="busyId === '__all__'"
          @click="handlePurgeAll"
        >
          {{ busyId === "__all__" ? "清空中…" : "清空回收站" }}
        </MyButton>
      </div>
    </template>

    <!-- List -->
    <div class="list">
      <div v-if="!loading && items.length === 0" class="empty-state">
        <div class="empty-icon">
          <el-icon :size="36"><Delete /></el-icon>
        </div>
        回收站为空
        <div class="empty-hint">删除的连接会暂存在这里</div>
      </div>
      <template v-else>
        <div v-for="item in items" :key="item.id" class="row">
          <i
            class="iconfont conn-icon"
            :class="connIconClass((item.conn_type as ConnType) || 'ssh')"
            :style="{ color: connColor((item.conn_type as ConnType) || 'ssh') }"
          ></i>
          <div class="row-info">
            <div class="row-name" :title="item.name">{{ item.name }}</div>
            <div class="row-meta">
              {{ item.host ? `${item.host}:${item.port}` : "—" }}
              <span class="meta-dot">·</span>
              {{ relativeTime(item.deletedAt) }}
            </div>
          </div>
          <MyButton
            variant="primary"
            size="small"
            :disabled="busyId === item.id"
            title="找回该连接"
            @click="handleRestore(item.id)"
          >
            {{ busyId === item.id ? "…" : "找回" }}
          </MyButton>
          <MyButton
            variant="ghost"
            size="small"
            class="purge-btn"
            :disabled="busyId === item.id"
            title="彻底删除（不可恢复）"
            @click="handlePurgeOne(item.id, item.name)"
          >
            删除
          </MyButton>
        </div>
      </template>
    </div>

    <!-- Footer hint -->
    <template #footer>
      <div class="footer-hint">
        最多保留 30 条记录，超出将自动彻底删除最早的连接
      </div>
    </template>
  </MyDialog>
</template>

<style scoped>
/* ── MyDialog 壳适配（class 经 attrs 透传到 el-dialog 根元素，故用 :global） ── */
:global(.recycle-dialog.el-dialog) {
  display: flex;
  flex-direction: column;
  max-height: 80vh;
}

:global(.recycle-dialog .el-dialog__body) {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

.dlg-header {
  display: flex;
  align-items: center;
  gap: 10px;
  /* 右上角 X（绝对定位）需要避让空间 */
  padding-right: 28px;
}

.header-icon {
  font-size: 18px;
  display: inline-flex;
}

.header-text {
  flex: 1;
  min-width: 0;
}

.header-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
}

.header-sub {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 2px;
}

/* 列表区（el-dialog body 内滚动） */
.list {
  padding: 8px;
}

.empty-state {
  padding: 48px 20px;
  text-align: center;
  color: var(--text-muted);
  font-size: 13px;
  line-height: 1.7;
}

.empty-icon {
  font-size: 36px;
  opacity: 0.3;
  margin-bottom: 14px;
}

.empty-hint {
  margin-top: 6px;
  font-size: 11px;
  color: var(--text-tertiary);
}

.row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 10px;
  border-radius: var(--radius-md);
  transition: background var(--duration-fast) var(--ease-in-out);
}

.row:hover {
  background: var(--bg-surface-hover);
}

.conn-icon {
  opacity: 0.7;
  flex-shrink: 0;
}

.row-info {
  flex: 1;
  min-width: 0;
}

.row-name {
  font-size: 13px;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-meta {
  font-size: 11px;
  color: var(--text-muted);
}

.meta-dot {
  margin: 0 6px;
  opacity: 0.5;
}

.purge-btn {
  color: var(--text-muted);
}

/* 让 #footer 分隔线紧贴 body 结束处（去掉 EP footer 自带的 padding-top） */
:global(.recycle-dialog .el-dialog__footer) {
  padding-top: 0;
}

/* ── #footer：底部提示条（左右/下负边距横贯弹窗全宽，抵消根元素内边距） ── */
.footer-hint {
  border-top: 1px solid var(--border-subtle);
  margin: 0 calc(var(--el-dialog-padding-primary) * -1)
    calc(var(--el-dialog-padding-primary) * -1);
  padding: 10px 20px;
  font-size: 11px;
  color: var(--text-muted);
  text-align: center;
}
</style>
