<!-- 端口转发 — SSH 隧道规则管理（本地 -L / 远程 -R）。
     规则为明文存储（主机名与端口暴露内网拓扑，敏感级同连接的 host 字段），
     因此本区在保险库未解锁时也能列出规则；凭据从不落在这里，每条规则引用
     一个连接，密码/私钥留在 keyring。

     启动隧道时后端为该规则单独拨一条 SSH 连接（不复用终端标签的会话）：
     隧道必须比配置它的那个标签活得更久，共享会话会让关掉标签静默杀死隧道。 -->
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { MyButton, MyIcon, MySection, confirmDialog, toast } from "myui";
import {
  forwardDeleteRule,
  forwardListRules,
  forwardSaveRule,
  forwardSetEnabled,
  forwardStart,
  forwardStop,
  type ForwardDirection,
  type ForwardRule,
} from "@/api";
import { connectionsStore } from "@/store/connections";

defineOptions({ name: "ForwardSection" });

const rules = ref<ForwardRule[]>([]);
const loading = ref(false);
/** rule id currently being started/stopped — drives the per-row spinner. */
const pendingId = ref<string | null>(null);

/** 规则引用的连接是否还存在。后端只返回 id，这里对照连接列表补上名称，
    引用的连接被删掉时明确标出来，而不是显示一个静默失效的规则。 */
const connectionName = (id: string): string =>
  connectionsStore.connections.find((c) => c.id === id)?.name ?? "（连接已删除）";

const connectionMissing = (id: string): boolean =>
  !connectionsStore.connections.some((c) => c.id === id);

const activeCount = computed(() => rules.value.filter((r) => r.tunnel).length);

async function reload(): Promise<void> {
  loading.value = true;
  try {
    rules.value = await forwardListRules();
  } catch (e) {
    toast(`加载端口转发规则失败：${e}`, { type: "error" });
  } finally {
    loading.value = false;
  }
}

onMounted(reload);

// ── 新建 / 编辑表单 ──
const showForm = ref(false);
const editingId = ref<string | null>(null);
const form = ref({
  name: "",
  connectionId: "",
  direction: "local" as ForwardDirection,
  bindHost: "127.0.0.1",
  bindPort: 0,
  targetHost: "127.0.0.1",
  targetPort: 0,
  enabled: false,
});

function openNew(): void {
  editingId.value = null;
  form.value = {
    name: "",
    connectionId: connectionsStore.connections[0]?.id ?? "",
    direction: "local",
    bindHost: "127.0.0.1",
    bindPort: 0,
    targetHost: "127.0.0.1",
    targetPort: 0,
    enabled: false,
  };
  showForm.value = true;
}

function openEdit(rule: ForwardRule): void {
  editingId.value = rule.id;
  form.value = {
    name: rule.name,
    connectionId: rule.connectionId,
    direction: rule.direction,
    bindHost: rule.bindHost,
    bindPort: rule.bindPort,
    targetHost: rule.targetHost,
    targetPort: rule.targetPort,
    enabled: rule.enabled,
  };
  showForm.value = true;
}

function portNum(v: string): number {
  const n = Number(v);
  return Number.isFinite(n) ? Math.trunc(n) : 0;
}

async function save(): Promise<void> {
  const f = form.value;
  if (!f.name.trim()) {
    toast("请填写规则名称", { type: "error" });
    return;
  }
  if (!f.connectionId) {
    toast("请选择该规则使用的连接", { type: "error" });
    return;
  }
  if (f.bindPort < 1 || f.bindPort > 65535) {
    toast("监听端口需在 1–65535 之间", { type: "error" });
    return;
  }
  if (f.direction === "remote" && f.bindPort === 0) {
    toast("远程转发必须指定服务器端口", { type: "error" });
    return;
  }
  if (f.targetPort < 1 || f.targetPort > 65535) {
    toast("目标端口需在 1–65535 之间", { type: "error" });
    return;
  }
  try {
    await forwardSaveRule({
      id: editingId.value ?? "",
      name: f.name.trim(),
      connectionId: f.connectionId,
      direction: f.direction,
      bindHost: f.bindHost.trim() || "127.0.0.1",
      bindPort: f.bindPort,
      targetHost: f.targetHost.trim(),
      targetPort: f.targetPort,
      enabled: f.enabled,
      createdAt: "",
      tunnel: null,
    });
    showForm.value = false;
    await reload();
    toast(editingId.value ? "规则已更新" : "规则已创建", { type: "success" });
  } catch (e) {
    toast(`保存失败：${e}`, { type: "error" });
  }
}

async function remove(rule: ForwardRule): Promise<void> {
  const ok = await confirmDialog({
    title: "删除端口转发规则",
    message: `将删除「${rule.name}」${
      rule.tunnel ? "，其运行中的隧道会一并停止。" : "。"
    }此操作不可恢复。`,
    type: "warning",
  });
  if (!ok) return;
  try {
    await forwardDeleteRule(rule.id);
    await reload();
    toast("已删除", { type: "success" });
  } catch (e) {
    toast(`删除失败：${e}`, { type: "error" });
  }
}

async function toggleEnabled(rule: ForwardRule, enabled: boolean): Promise<void> {
  try {
    await forwardSetEnabled(rule.id, enabled);
    await reload();
  } catch (e) {
    toast(`更新失败：${e}`, { type: "error" });
  }
}

async function start(rule: ForwardRule): Promise<void> {
  pendingId.value = rule.id;
  try {
    const port = await forwardStart(rule.id);
    toast(`隧道已启动，实际监听端口 ${port}`, { type: "success" });
    await reload();
  } catch (e) {
    toast(`启动失败：${e}`, { type: "error" });
  } finally {
    pendingId.value = null;
  }
}

async function stop(rule: ForwardRule): Promise<void> {
  pendingId.value = rule.id;
  try {
    await forwardStop(rule.id);
    await reload();
  } catch (e) {
    toast(`停止失败：${e}`, { type: "error" });
  } finally {
    pendingId.value = null;
  }
}

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

/** 运行时显示用的实际端口：规则里写的端口为 0（让系统挑）时，隧道起来后
    才知道落在哪 —— 但 TunnelStatus 只带 ruleId 不带端口，所以这里只能显示
    「随机」占位，由 start() 的 toast 告知真实端口。 */
function displayPort(rule: ForwardRule): string {
  if (rule.bindPort === 0 && rule.tunnel) return "随机";
  return String(rule.bindPort);
}
</script>

<template>
  <MySection
    title="端口转发"
    description="通过 SSH 隧道把远端服务映射到本机（本地转发），或把本机服务暴露给远端（远程转发）。规则保存在本地数据库中，凭据仍由对应连接保管。"
  >
    <template #actions>
      <MyButton size="small" variant="primary" @click="openNew">
        <MyIcon :size="13" name="Plus" />
        新建规则
      </MyButton>
    </template>

    <div class="fwd-hint">
      隧道启动时后端会为该规则单独建立一条 SSH 连接，因此关闭配置它的终端标签不会影响隧道。应用重启后规则保持停止状态——需要时手动启动，避免每次开机都静默开放端口。
    </div>

    <!-- 规则表 -->
    <div v-if="rules.length === 0" class="fwd-empty">暂无端口转发规则</div>
    <table v-else class="fwd-table">
      <thead>
        <tr>
          <th>名称</th>
          <th>方向</th>
          <th>监听</th>
          <th>目标</th>
          <th>状态</th>
          <th>操作</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="r in rules" :key="r.id">
          <td>
            <div class="cell-name">{{ r.name }}</div>
            <div class="cell-sub" :class="{ missing: connectionMissing(r.connectionId) }">
              {{ connectionName(r.connectionId) }}
            </div>
          </td>
          <td>
            <span class="dir-tag" :class="r.direction">{{ r.direction === "local" ? "本地 -L" : "远程 -R" }}</span>
          </td>
          <td class="mono">{{ r.bindHost }}:{{ displayPort(r) }}</td>
          <td class="mono">{{ r.targetHost }}:{{ r.targetPort }}</td>
          <td>
            <span v-if="r.tunnel" class="state-tag" :class="r.tunnel.state">
              {{ r.tunnel.state === "error" ? "错误" : "运行中" }}
            </span>
            <span v-else class="state-tag idle">已停止</span>
            <div v-if="r.tunnel && r.tunnel.state !== 'error'" class="cell-sub">
              ↑{{ fmtBytes(r.tunnel.bytesOut) }} ↓{{ fmtBytes(r.tunnel.bytesIn) }}
              <span v-if="r.tunnel.connections">· {{ r.tunnel.connections }} 连接</span>
            </div>
            <div v-else-if="r.tunnel && r.tunnel.error" class="cell-sub err">{{ r.tunnel.error }}</div>
          </td>
          <td class="ops">
            <MyButton
              v-if="!r.tunnel"
              size="small"
              variant="primary"
              :loading="pendingId === r.id"
              :disabled="connectionMissing(r.connectionId)"
              @click="start(r)"
            >
              启动
            </MyButton>
            <MyButton
              v-else
              size="small"
              :loading="pendingId === r.id"
              @click="stop(r)"
            >
              停止
            </MyButton>
            <MyButton size="small" @click="openEdit(r)">编辑</MyButton>
            <MyButton size="small" variant="danger" @click="remove(r)">删除</MyButton>
          </td>
        </tr>
      </tbody>
    </table>

    <!-- 新建 / 编辑表单 -->
    <div v-if="showForm" class="fwd-form">
      <div class="form-title">{{ editingId ? "编辑规则" : "新建规则" }}</div>
      <div class="form-grid">
        <label class="field">
          <span class="field-label">名称</span>
          <input v-model="form.name" class="input" placeholder="例如：数据库隧道" />
        </label>
        <label class="field">
          <span class="field-label">使用连接</span>
          <select v-model="form.connectionId" class="input">
            <option v-for="c in connectionsStore.connections" :key="c.id" :value="c.id">
              {{ c.name }}（{{ c.host }}）
            </option>
          </select>
        </label>
        <label class="field">
          <span class="field-label">方向</span>
          <select v-model="form.direction" class="input">
            <option value="local">本地转发（-L）远端服务映射到本机</option>
            <option value="remote">远程转发（-R）本机服务暴露给远端</option>
          </select>
        </label>
        <label class="field">
          <span class="field-label">监听地址</span>
          <input v-model="form.bindHost" class="input" placeholder="127.0.0.1" />
        </label>
        <label class="field">
          <span class="field-label">监听端口</span>
          <input
            :value="form.bindPort"
            class="input"
            type="number"
            min="1"
            max="65535"
            @input="form.bindPort = portNum(($event.target as HTMLInputElement).value)"
          />
        </label>
        <label class="field">
          <span class="field-label">目标主机</span>
          <input v-model="form.targetHost" class="input" placeholder="127.0.0.1" />
        </label>
        <label class="field">
          <span class="field-label">目标端口</span>
          <input
            :value="form.targetPort"
            class="input"
            type="number"
            min="1"
            max="65535"
            @input="form.targetPort = portNum(($event.target as HTMLInputElement).value)"
          />
        </label>
      </div>
      <div class="form-hint">
        本地转发：连上 <b>{{ form.bindHost }}:{{ form.bindPort }}</b> 即等于访问远端的
        <b>{{ form.targetHost }}:{{ form.targetPort }}</b>（目标地址相对 SSH 服务器解析）。
        远程转发：SSH 服务器监听 <b>{{ form.bindHost }}:{{ form.bindPort }}</b>，把连接转回本机的
        <b>{{ form.targetHost }}:{{ form.targetPort }}</b>（需服务器开启 AllowTcpForwarding）。
      </div>
      <div class="form-ops">
        <MyButton size="small" variant="primary" @click="save">保存</MyButton>
        <MyButton size="small" @click="showForm = false">取消</MyButton>
      </div>
    </div>
  </MySection>
</template>

<style scoped>
.fwd-hint {
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  line-height: 1.6;
  margin-bottom: 12px;
}

.fwd-empty {
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  padding: 24px 0;
  text-align: center;
}

.fwd-table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--fs-xs);
}

.fwd-table th {
  text-align: left;
  font-weight: 500;
  color: var(--text-secondary);
  padding: 6px 8px;
  border-bottom: 1px solid var(--border-subtle);
  white-space: nowrap;
}

.fwd-table td {
  padding: 8px;
  border-bottom: 1px solid var(--border-subtle);
  vertical-align: top;
}

.cell-name {
  color: var(--text-primary);
  font-weight: 500;
}

.cell-sub {
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  margin-top: 2px;
}

.cell-sub.missing {
  color: var(--warning, #d97706);
}

.cell-sub.err {
  color: var(--danger, #dc2626);
}

.mono {
  font-family: var(--font-mono, ui-monospace, monospace);
  color: var(--text-secondary);
  white-space: nowrap;
}

.dir-tag {
  display: inline-block;
  padding: 1px 6px;
  border-radius: var(--radius-sm);
  font-size: var(--fs-2xs);
  white-space: nowrap;
}

.dir-tag.local {
  background: color-mix(in srgb, var(--accent-primary) 14%, transparent);
  color: var(--accent-primary);
}

.dir-tag.remote {
  background: color-mix(in srgb, var(--success, #16a34a) 14%, transparent);
  color: var(--success, #16a34a);
}

.state-tag {
  display: inline-block;
  padding: 1px 6px;
  border-radius: var(--radius-sm);
  font-size: var(--fs-2xs);
}

.state-tag.listening,
.state-tag.forwarding {
  background: color-mix(in srgb, var(--success, #16a34a) 14%, transparent);
  color: var(--success, #16a34a);
}

.state-tag.error {
  background: color-mix(in srgb, var(--danger, #dc2626) 14%, transparent);
  color: var(--danger, #dc2626);
}

.state-tag.idle {
  background: var(--bg-subtle, rgba(127, 127, 127, 0.12));
  color: var(--text-tertiary);
}

.ops {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}

.fwd-form {
  margin-top: 16px;
  padding: 14px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
}

.form-title {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: 12px;
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px 14px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.field-label {
  font-size: var(--fs-2xs);
  color: var(--text-secondary);
}

.input {
  height: 30px;
  padding: 0 8px;
  font-size: var(--fs-xs);
  color: var(--text-primary);
  background: var(--bg-base);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-sm);
}

.input:focus {
  outline: none;
  border-color: var(--accent-primary);
}

.form-hint {
  margin-top: 10px;
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  line-height: 1.6;
}

.form-ops {
  display: flex;
  gap: 8px;
  margin-top: 12px;
}
</style>
