<!-- 数据管理 — 配置导入导出 / 回收站入口 / 版本备份与回退（旧 data 分区） -->
<script setup lang="ts">
import { onMounted, ref } from "vue";
import { open, save } from "@tauri-apps/plugin-dialog";
import { CircleClose, Delete, Download, PriceTag, RefreshLeft, Upload } from "@element-plus/icons-vue";
import { MyButton, MyDialog, MyInput, MySection, confirmDialog } from "myui";
import {
  exportConnections,
  importConnections,
  listBackups,
  rollbackBackup,
  getAppVersion,
  getPreviousVersion,
  type BackupInfo,
} from "@/api";
import { ui } from "@/store/ui";

defineOptions({ name: "DataSection" });

const props = defineProps<{ connectionCount: number }>();

const emit = defineEmits<{
  refresh: [];
}>();

const MIN_LEN = 6;

// ── 导入导出 ──
const exportBusy = ref(false);
const importBusy = ref(false);
const ioErr = ref<string | null>(null);
const exportPass = ref("");
const showExportDialog = ref(false);
const importPass = ref("");
const importPath = ref<string | null>(null);
const showImportDialog = ref(false);

// ── 版本备份与回退 ──
const appVersion = ref("");
const backups = ref<BackupInfo[]>([]);
const previousVersion = ref<string | null>(null);
const rollbackBusy = ref(false);
const rollbackResult = ref<string | null>(null);

onMounted(() => {
  getAppVersion()
    .then((v) => (appVersion.value = v))
    .catch(() => {});
  listBackups()
    .then((b) => (backups.value = b))
    .catch(() => {});
  getPreviousVersion()
    .then((v) => (previousVersion.value = v))
    .catch(() => {});
});

// 回收站：由壳层的全局 RecycleDialog 承载（ui.showRecycle）。
// 对应旧版 RecycleDialog 的 onChanged={onRefresh} — 新版 RecycleDialog
// 在每次找回/删除成功后自行调用 reloadConnections()，无需此处转发。

function startExport(): void {
  if (props.connectionCount === 0) {
    ioErr.value = "暂无连接可导出";
    return;
  }
  exportPass.value = "";
  showExportDialog.value = true;
}

async function handleExport(): Promise<void> {
  if (exportPass.value.length < MIN_LEN) {
    ioErr.value = "加密密码至少 6 个字符";
    return;
  }
  const path = await save({
    defaultPath: `myshell-export-${new Date().toISOString().slice(0, 10)}.json`,
    filters: [{ name: "MyShell Encrypted Dump", extensions: ["json"] }],
  });
  if (!path) return;

  exportBusy.value = true;
  ioErr.value = null;
  try {
    const result = await exportConnections(exportPass.value, path);
    showExportDialog.value = false;
    // 凭据读不出来时不能报成功——那是一份换机后无法认证的备份。
    const missing = result.missingCredentials ?? [];
    if (missing.length > 0) {
      alert(
        `已导出 ${result.exported} 个连接到\n${path}\n\n` +
          `⚠ 其中 ${missing.length} 项凭据未能读取，这份备份在换机后将无法自动登录：\n` +
          missing.map((m) => `  - ${m}`).join("\n") +
          `\n\n可改用「连接管理」逐个检查这些连接的密码，或在原机器上重新保存一次密码。`,
      );
    } else {
      alert(`已导出 ${result.exported} 个连接到\n${path}`);
    }
  } catch (e) {
    ioErr.value = String(e);
  } finally {
    exportBusy.value = false;
    // 无论哪条退出路径都清空密码 — 失败重试时密码也不会残留在
    // 组件状态里（成功时对话框已关闭）。
    exportPass.value = "";
  }
}

async function startImport(): Promise<void> {
  const selected = await open({
    multiple: false,
    filters: [{ name: "MyShell Encrypted Dump", extensions: ["json"] }],
  });
  if (!selected || Array.isArray(selected)) return;
  importPath.value = selected;
  importPass.value = "";
  showImportDialog.value = true;
}

async function handleImport(): Promise<void> {
  if (!importPath.value) return;
  if (importPass.value.length < MIN_LEN) {
    ioErr.value = "解密密码至少 6 个字符";
    return;
  }

  importBusy.value = true;
  ioErr.value = null;
  try {
    const n = await importConnections(importPass.value, importPath.value);
    showImportDialog.value = false;
    importPath.value = null;
    alert(`已导入 ${n} 个连接`);
    emit("refresh");
  } catch (e) {
    ioErr.value = String(e);
  } finally {
    importBusy.value = false;
    // 无论哪条退出路径都清空密码 — 导入失败大概率是密码错误，
    // 强制重输比留着错误值更安全也更好用。
    importPass.value = "";
  }
}

function closeImportDialog(): void {
  showImportDialog.value = false;
  importPath.value = null;
  // The comments on the import/export handlers both claim "无论哪条退出路径都
  // 清空密码", but the CANCEL paths didn't: this handler cleared the path
  // only, and the export dialog's cancel button was a bare
  // `showExportDialog = false`. These panels are kept mounted by v-show, so
  // the export/import passphrase survived in component state for the rest of
  // the session.
  importPass.value = "";
}

function closeExportDialog(): void {
  showExportDialog.value = false;
  exportPass.value = "";
}

async function handleRollback(version: string): Promise<void> {
  const ok = await confirmDialog({
    message: `确定要回退到版本 ${version} 吗？\n\n当前配置将被覆盖，回退后需要重启应用。`,
    title: "回退确认",
    type: "warning",
  });
  if (!ok) return;
  rollbackBusy.value = true;
  rollbackResult.value = null;
  try {
    const result = await rollbackBackup(version);
    rollbackResult.value = result;
    const updated = await listBackups();
    backups.value = updated;
  } catch (e) {
    rollbackResult.value = `回退失败: ${e}`;
  } finally {
    rollbackBusy.value = false;
  }
}
</script>

<template>
  <!-- 导入导出 -->
  <MySection title="配置导入导出" description="导出连接配置到加密文件，或从文件导入">
    <div v-if="ioErr" class="msg-box error">
      <el-icon :size="14"><CircleClose /></el-icon>
      {{ ioErr }}
    </div>

    <div class="io-row">
      <MyButton
        variant="secondary"
        class="io-btn"
        :disabled="exportBusy || importBusy"
        @click="startExport"
      >
        <el-icon :size="14"><Upload /></el-icon>
        导出配置
      </MyButton>
      <MyButton
        variant="secondary"
        class="io-btn"
        :disabled="exportBusy || importBusy"
        @click="startImport"
      >
        <el-icon :size="14"><Download /></el-icon>
        导入配置
      </MyButton>
    </div>
    <div class="count-hint">当前共有 {{ connectionCount }} 个连接</div>

    <!-- 回收站入口 — 恢复或彻底清除软删除的连接。 -->
    <MyButton variant="secondary" class="recycle-btn" @click="ui.showRecycle = true">
      <el-icon :size="14"><Delete /></el-icon>
      找回连接 / 回收站
    </MyButton>
  </MySection>

  <div class="divider" />

  <!-- 备份 -->
  <MySection title="版本备份与回退" description="升级时自动备份配置，支持回退到旧版本">
    <div class="version-card">
      <span class="version-icon">
        <el-icon :size="16"><PriceTag /></el-icon>
      </span>
      <div>
        <div class="version-label">当前版本</div>
        <div class="version-value">{{ appVersion || "加载中..." }}</div>
      </div>
    </div>

    <div
      v-if="rollbackResult"
      class="rollback-result"
      :class="rollbackResult.includes('失败') ? 'error' : 'success'"
    >
      {{ rollbackResult }}
    </div>

    <MyButton
      v-if="previousVersion"
      variant="ghost"
      class="quick-rollback"
      :disabled="rollbackBusy"
      @click="handleRollback(previousVersion)"
    >
      <el-icon :size="14"><RefreshLeft /></el-icon>
      快速回退到上一版本 ({{ previousVersion }})
    </MyButton>

    <div v-if="backups.length > 0" class="backup-list">
      <div class="backup-head">可用备份（最多保留 5 个）</div>
      <div
        v-for="(backup, index) in backups"
        :key="backup.version"
        class="backup-row"
        :class="{ divided: index < backups.length - 1 }"
      >
        <div>
          <div class="backup-version">
            v{{ backup.version }}
            <span v-if="backup.version === appVersion" class="backup-current">当前</span>
          </div>
          <div class="backup-meta">{{ backup.timestampStr }} · {{ backup.files.length }} 个文件</div>
        </div>
        <MyButton
          v-if="backup.version !== appVersion"
          variant="ghost"
          size="small"
          :disabled="rollbackBusy"
          @click="handleRollback(backup.version)"
        >
          回退
        </MyButton>
      </div>
    </div>

    <div v-if="backups.length === 0" class="backup-empty">暂无备份记录</div>
  </MySection>

  <!-- 导出密码对话框 -->
  <MyDialog
    v-model="showExportDialog"
    title="加密导出"
    :width="400"
    :dismissable="false"
    @cancel="closeExportDialog"
    hide-footer
  >
    <div class="dialog-intro">设置加密密码，导入时需要此密码才能解密</div>
    <div class="field">
      <label class="field-label">加密密码</label>
      <MyInput
        v-model="exportPass"
        type="password"
        placeholder="至少 6 个字符"
        autofocus
      />
    </div>
    <div class="dialog-actions">
      <MyButton variant="secondary" @click="closeExportDialog">取消</MyButton>
      <MyButton
        variant="primary"
        :disabled="exportPass.length < MIN_LEN || exportBusy"
        @click="handleExport"
      >
        {{ exportBusy ? "导出中..." : "导出" }}
      </MyButton>
    </div>
  </MyDialog>

  <!-- 导入密码对话框 -->
  <MyDialog
    v-model="showImportDialog"
    title="解密导入"
    :width="400"
    :dismissable="false"
    hide-footer
    @cancel="closeImportDialog"
  >
    <div class="dialog-intro">输入导出时设置的密码以解密</div>
    <div class="field">
      <label class="field-label">解密密码</label>
      <MyInput
        v-model="importPass"
        type="password"
        placeholder="至少 6 个字符"
        autofocus
      />
    </div>
    <div class="dialog-actions">
      <MyButton variant="secondary" @click="closeImportDialog">取消</MyButton>
      <MyButton
        variant="primary"
        :disabled="importPass.length < MIN_LEN || importBusy"
        @click="handleImport"
      >
        {{ importBusy ? "导入中..." : "导入" }}
      </MyButton>
    </div>
  </MyDialog>
</template>

<style scoped>
.msg-box {
  margin-bottom: 12px;
  padding: 10px 14px;
  border-radius: var(--radius-md);
  font-size: 12px;
  display: flex;
  align-items: center;
  gap: 8px;
}

.msg-box.error {
  background: var(--error-muted);
  border: 1px solid var(--error);
  color: var(--error);
}

.io-row {
  display: flex;
  gap: 10px;
}

.io-btn {
  flex: 1;
}

.count-hint {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 10px;
  text-align: center;
}

.recycle-btn {
  width: 100%;
  margin-top: 12px;
}

.divider {
  height: 1px;
  background: var(--border-subtle);
  margin: 20px 0;
}

.version-card {
  padding: 10px 14px;
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  margin-bottom: 12px;
  display: flex;
  align-items: center;
  gap: 10px;
}

.version-icon {
  font-size: 16px;
  color: var(--accent-primary);
}

.version-label {
  font-size: 11px;
  color: var(--text-tertiary);
}

.version-value {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.rollback-result {
  margin-bottom: 12px;
  padding: 10px 14px;
  border-radius: var(--radius-md);
  font-size: 12px;
}

.rollback-result.error {
  background: var(--error-muted);
  border: 1px solid var(--error);
  color: var(--error);
}

.rollback-result.success {
  background: var(--success-muted);
  border: 1px solid var(--success);
  color: var(--success);
}

.quick-rollback {
  width: 100%;
  margin-bottom: 12px;
  color: var(--accent-primary);
}

.backup-list {
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  overflow: hidden;
}

.backup-head {
  padding: 10px 14px;
  background: var(--bg-surface);
  font-size: 11px;
  color: var(--text-tertiary);
  border-bottom: 1px solid var(--border-subtle);
}

.backup-row {
  padding: 12px 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.backup-row:hover {
  background: var(--bg-surface-hover);
}

.backup-row.divided {
  border-bottom: 1px solid var(--border-subtle);
}

.backup-version {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
}

.backup-current {
  margin-left: 8px;
  padding: 2px 8px;
  background: var(--success-muted);
  color: var(--success);
  border-radius: var(--radius-full);
  font-size: 10px;
  font-weight: 600;
}

.backup-meta {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 2px;
}

.backup-empty {
  font-size: 12px;
  color: var(--text-muted);
  text-align: center;
  padding: 24px;
}

.dialog-intro {
  font-size: 12px;
  color: var(--text-muted);
  margin-bottom: 14px;
}

.field {
  margin-bottom: 12px;
}

.field-label {
  display: block;
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 6px;
  font-weight: 500;
}

.dialog-actions {
  display: flex;
  gap: 10px;
  margin-top: 20px;
  justify-content: flex-end;
}
</style>
