<!-- 数据管理 — 配置导入导出 / 回收站入口 / 版本备份与回退（旧 data 分区） -->
<script setup lang="ts">
import { onMounted, ref } from "vue";
import { open, save } from "@tauri-apps/plugin-dialog";
import { MyButton, MyDialog, MyIcon, MyInput, MySection, confirmDialog } from "myui";
import {
  exportConnections,
  importConnections,
  listBackups,
  rollbackBackup,
  getAppVersion,
  getPreviousVersion,
  readTextFile,
  saveConnection,
  type BackupInfo,
  type ConnectionConfig,
} from "@/api";
import {
  parseCsv,
  parseMobaXterm,
  parsePuttyReg,
  parseSecureCrt,
  parseSshConfig,
  type ImportError,
} from "@/utils/importers";
import { ui } from "@/store/ui";
import { reloadConnections } from "@/store/connections";

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

// ── 外部工具导入（ssh_config / PuTTY / MobaXterm / SecureCRT / CSV）──
// 两步走：选文件 → 解析预览（可勾选）→ 逐条 saveConnection。
// 凭据一律不导入：这些格式要么不存密码，要么用各家私有方式加密，硬解
// 只会制造错误的安全感。预览列表把密码/私钥列标注为「导入后需自行填写」。
const extFormats = [
  { key: "ssh_config", label: "OpenSSH ssh_config", ext: ["", "config", "conf"], parser: parseSshConfig },
  { key: "putty", label: "PuTTY 注册表导出", ext: ["reg"], parser: parsePuttyReg },
  { key: "moba", label: "MobaXterm", ext: ["mobaconf", "mxtsessions", "ini"], parser: parseMobaXterm },
  { key: "securecrt", label: "SecureCRT", ext: ["ini", "dat"], parser: parseSecureCrt },
  { key: "csv", label: "CSV", ext: ["csv"], parser: parseCsv },
] as const;

const showExtDialog = ref(false);
const extBusy = ref(false);
const extErr = ref<string | null>(null);
/** 解析出来的草稿（含勾选状态）。 */
const extDrafts = ref<(ConnectionConfig & { _checked: boolean })[]>([]);
/** 解析过程中的逐条失败。 */
const extParseErrors = ref<ImportError[]>([]);
const extFileName = ref("");

async function startExternalImport(): Promise<void> {
  const selected = await open({
    multiple: false,
    filters: [
      { name: "连接导出文件", extensions: ["config", "conf", "reg", "mobaconf", "mxtsessions", "ini", "dat", "csv", ""] },
    ],
  });
  if (!selected || Array.isArray(selected)) return;
  extBusy.value = true;
  extErr.value = null;
  try {
    const text = await readTextFile(selected);
    extFileName.value = selected.split(/[\\/]/).pop() ?? selected;
    const parsed = pickParser(selected, text);
    extParseErrors.value = parsed.errors;
    extDrafts.value = parsed.configs.map((c) => ({ ...c, _checked: true }));
    if (extDrafts.value.length === 0 && parsed.errors.length === 0) {
      extErr.value = "文件里没有可导入的连接";
      return;
    }
    showExtDialog.value = true;
  } catch (e) {
    extErr.value = `读取或解析失败：${e}`;
  } finally {
    extBusy.value = false;
  }
}

/** 按扩展名选解析器；认不出来时按内容特征兜底。 */
function pickParser(path: string, text: string) {
  const lower = path.toLowerCase();
  const byExt = extFormats.find((f) => f.ext.some((e) => e && lower.endsWith(`.${e}`)));
  if (byExt) return byExt.parser(text);
  // ssh_config：以 "Host " 开头的行
  if (/^\s*Host\s+\S/im.test(text)) return parseSshConfig(text);
  // PuTTY .reg：注册表段头
  if (/SimonTatham\\PuTTY\\Sessions/i.test(text)) return parsePuttyReg(text);
  // MobaXterm：SubRep= 键
  if (/^\s*SubRep=/im.test(text)) return parseMobaXterm(text);
  // SecureCRT：S:"..."= 键
  if (/^\s*S:"/m.test(text)) return parseSecureCrt(text);
  return parseCsv(text);
}

async function commitExternalImport(): Promise<void> {
  const chosen = extDrafts.value.filter((d) => d._checked);
  if (chosen.length === 0) {
    extErr.value = "没有勾选任何连接";
    return;
  }
  extBusy.value = true;
  extErr.value = null;
  const failed: string[] = [];
  let ok = 0;
  for (const d of chosen) {
    try {
      // 逐个保存：一个坏行不该拖垮整批，用户还能从错误列表里看出是哪个。
      const { _checked, ...config } = d;
      await saveConnection(config);
      ok++;
    } catch (e) {
      failed.push(`${d.name || d.host}：${e}`);
    }
  }
  extBusy.value = false;
  if (failed.length > 0) {
    extErr.value = `已导入 ${ok} 个，${failed.length} 个失败：\n${failed.join("\n")}`;
  }
  if (ok > 0) {
    showExtDialog.value = false;
    await reloadConnections();
    emit("refresh");
  }
}

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
      <MyIcon :size="14" name="CircleClose" />
      {{ ioErr }}
    </div>

    <div class="io-row">
      <MyButton
        variant="secondary"
        class="io-btn"
        :disabled="exportBusy || importBusy"
        @click="startExport"
      >
        <MyIcon :size="14" name="Upload" />
        导出配置
      </MyButton>
      <MyButton
        variant="secondary"
        class="io-btn"
        :disabled="exportBusy || importBusy"
        @click="startImport"
      >
        <MyIcon :size="14" name="Download" />
        导入配置
      </MyButton>
      <MyButton
        variant="secondary"
        class="io-btn"
        :disabled="exportBusy || importBusy"
        title="从 OpenSSH / PuTTY / MobaXterm / SecureCRT / CSV 迁移连接"
        @click="startExternalImport"
      >
        <MyIcon :size="14" name="Connection" />
        从外部工具导入
      </MyButton>
    </div>
    <div class="count-hint">当前共有 {{ connectionCount }} 个连接</div>

    <!-- 回收站入口 — 恢复或彻底清除软删除的连接。 -->
    <MyButton variant="secondary" class="recycle-btn" @click="ui.showRecycle = true">
      <MyIcon :size="14" name="TrashRecycle" />
      找回连接 / 回收站
    </MyButton>
  </MySection>

  <div class="divider" />

  <!-- 备份 -->
  <MySection title="版本备份与回退" description="升级时自动备份配置，支持回退到旧版本">
    <div class="version-card">
      <span class="version-icon">
        <MyIcon :size="16" name="PriceTag" />
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
      <MyIcon :size="14" name="RefreshLeft" />
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

  <!-- 导出密码对话框。宽度走 myui v1.9.7 的档位契约：单字段表单弹窗用
       size="sm"（420），不再手写 400 这类中间值。 -->
  <MyDialog
    v-model="showExportDialog"
    title="加密导出"
    size="sm"
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

  <!-- 外部工具导入：解析预览 + 勾选 + 逐条保存。
       密码/私钥一律不导入（见 script 里的说明），所以每行都标一句
       「导入后需自行填写凭据」，别让用户以为填完名字就能连。 -->
  <MyDialog
    v-model="showExtDialog"
    title="从外部工具导入"
    :width="640"
    dismissable
    @cancel="showExtDialog = false"
  >
    <div class="ext-intro">
      已从 <b>{{ extFileName }}</b> 解析出下列连接。勾选要导入的条目，确认后写入连接列表。
      密码与私钥不会从外部文件带入，导入后请逐个连接补填凭据。
    </div>
    <div v-if="extParseErrors.length > 0" class="ext-errors">
      <div class="ext-errors-title">{{ extParseErrors.length }} 条未能解析：</div>
      <div v-for="(e, i) in extParseErrors" :key="i" class="ext-error-row">
        <b>{{ e.source }}</b> — {{ e.reason }}
      </div>
    </div>
    <div class="ext-list">
      <label v-for="(d, i) in extDrafts" :key="i" class="ext-row">
        <input v-model="d._checked" type="checkbox" />
        <span class="ext-name">{{ d.name }}</span>
        <span class="ext-host">{{ d.username }}@{{ d.host }}:{{ d.port }}</span>
        <span class="ext-type">{{ d.conn_type }}</span>
        <span v-if="d.group_path && d.group_path !== '/'" class="ext-group">{{ d.group_path }}</span>
      </label>
    </div>
    <div class="dialog-actions">
      <MyButton variant="secondary" @click="showExtDialog = false">取消</MyButton>
      <MyButton variant="primary" :loading="extBusy" @click="commitExternalImport">
        导入勾选的 {{ extDrafts.filter((d) => d._checked).length }} 个
      </MyButton>
    </div>
  </MyDialog>

  <!-- 导入密码对话框：同样是单字段表单弹窗，用 size="sm"（420）。 -->
  <MyDialog
    v-model="showImportDialog"
    title="解密导入"
    size="sm"
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
  font-size: var(--fs-xs);
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
  font-size: var(--fs-2xs);
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
  font-size: var(--fs-lg);
  color: var(--accent-primary);
}

.version-label {
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
}

.version-value {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
}

.rollback-result {
  margin-bottom: 12px;
  padding: 10px 14px;
  border-radius: var(--radius-md);
  font-size: var(--fs-xs);
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
  font-size: var(--fs-2xs);
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
  font-size: var(--fs-xs);
  font-weight: 500;
  color: var(--text-primary);
}

.backup-current {
  margin-left: 8px;
  padding: 2px 8px;
  background: var(--success-muted);
  color: var(--success);
  border-radius: var(--radius-full);
  font-size: var(--fs-2xs);
  font-weight: 600;
}

.backup-meta {
  font-size: var(--fs-2xs);
  color: var(--text-muted);
  margin-top: 2px;
}

.backup-empty {
  font-size: var(--fs-xs);
  color: var(--text-muted);
  text-align: center;
  padding: 24px;
}

.dialog-intro {
  font-size: var(--fs-xs);
  color: var(--text-muted);
  margin-bottom: 14px;
}

.field {
  margin-bottom: 12px;
}

.field-label {
  display: block;
  font-size: var(--fs-xs);
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

/* ── 外部工具导入预览 ── */
.ext-intro {
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  line-height: 1.6;
  margin-bottom: 10px;
}
.ext-errors {
  margin-bottom: 10px;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--danger, #dc2626) 10%, transparent);
  font-size: var(--fs-2xs);
  color: var(--text-secondary);
  max-height: 120px;
  overflow-y: auto;
}
.ext-errors-title {
  font-weight: 600;
  margin-bottom: 4px;
}
.ext-error-row {
  line-height: 1.5;
}
.ext-list {
  max-height: 320px;
  overflow-y: auto;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
}
.ext-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  font-size: var(--fs-xs);
  cursor: pointer;
  border-bottom: 1px solid var(--border-subtle);
}
.ext-row:last-child {
  border-bottom: none;
}
.ext-row:hover {
  background: var(--bg-surface-hover);
}
.ext-name {
  font-weight: 500;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 180px;
}
.ext-host {
  font-family: var(--font-mono, ui-monospace, monospace);
  color: var(--text-secondary);
  flex: 1;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ext-type,
.ext-group {
  flex-shrink: 0;
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  padding: 1px 5px;
  border-radius: var(--radius-sm);
  background: var(--bg-subtle, rgba(127, 127, 127, 0.12));
}
</style>
