<!-- 连接编辑对话框 — 从 src-legacy/components/ConnectionDialog.tsx 原样移植
     （行为语义、中文文案与关键注释逐字保留）。
     结构：MyDialog 壳（表单类对话框防误关：dismissable 默认 false —— 遮罩/Esc/右上角
     关闭全禁用；只能经「取消」/保存成功退出，与旧版一致）。
     保存成功 → emit("save") + emit("close")，由壳层 reloadConnections() 并复位状态
     （对齐旧 App.tsx onSave = 关闭 + reload）。密码明文揭示走 PasswordVerifyDialog。
     旧版 alert 按迁移约定替换为 myui toast（文案不变；Tauri webview 会吞 window.alert）。
     表单控件用 myui（MyInput/MySelect/MyCheckbox/MyButton），字段标签沿用旧 FormField
     形态（label 右上角红星）；字体选择复用已迁移的 FontField.vue，类型图标复用 ConnIcon.vue。
     校验失败交互保留旧版「红框 + 抖动 + 聚焦首个错误字段」（比顶部横幅更精确，不用 error prop）。
     密码输入用 MyInput attrs 透传 :type="password|text"（显隐由主密码验证门禁控制，
     不用 EP 的 show-password —— 那会绕过揭示门禁）。 -->
<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";
import type { ComponentPublicInstance } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpened, Hide, View } from "@element-plus/icons-vue";
import { MyButton, MyCheckbox, MyDialog, MyInput, MySelect, toast, type SelectOption } from "myui";
import {
  readTextFile,
  revealConnectionPassword,
  revealConnectionProxyPassword,
  saveConnection,
  testConnection,
} from "@/api";
import type { ConnectionConfig, ConnType, FtpTls, ProxyType } from "@/api";
import PasswordVerifyDialog from "./PasswordVerifyDialog.vue";
import ConnIcon from "./ConnIcon.vue";
import FontField from "./FontField.vue";

defineOptions({ name: "ConnectionDialog" });

const props = withDefaults(
  defineProps<{
    config: ConnectionConfig | null;
    initialConnType?: ConnType;
    initialFolderPath?: string;
    folders?: string[];
  }>(),
  { folders: () => [] },
);

const emit = defineEmits<{
  close: [];
  save: [];
}>();

const TYPE_OPTIONS: { value: ConnType; label: string; defaultPort: number }[] = [
  { value: "ssh", label: "SSH", defaultPort: 22 },
  { value: "sftp", label: "SFTP", defaultPort: 22 },
  { value: "ftp", label: "FTP", defaultPort: 21 },
  { value: "local", label: "本地", defaultPort: 0 },
];

/// Quick-pick shell presets for conn_type='local'. The user can still type a
/// custom path in the input — these just save looking up common exes.
const SHELL_PRESETS: { value: string; label: string }[] = [
  { value: "pwsh.exe", label: "PowerShell 7 (pwsh.exe)" },
  { value: "powershell.exe", label: "Windows PowerShell (powershell.exe)" },
  { value: "cmd.exe", label: "命令提示符 (cmd.exe)" },
  { value: "wsl.exe", label: "WSL (wsl.exe)" },
  { value: "C:\\Program Files\\Git\\bin\\bash.exe", label: "Git Bash" },
];

/// Validation field keys in the order we want to focus them when Save fails.
/// Matches the keys emitted by `validate()` and consumed by the Input `error`
/// props. Used to drop the cursor into the first empty required field.
const FIELD_FOCUS_ORDER = [
  "name",
  "host",
  "port",
  "username",
  "password",
  "shellPath",
  "proxyHost",
  "proxyPort",
  // `validate()` can flag these two (range checks), but they were missing here
  // — so a Save that failed ONLY on them shook the fields red and then focused
  // nothing, contradicting this file's own header promise to "drop the cursor
  // into the first empty required field".
  "connectTimeout",
  "keepaliveInterval",
] as const;

const initialType: ConnType = props.config?.conn_type ?? props.initialConnType ?? "ssh";

const connType = ref<ConnType>(initialType);
const name = ref(props.config?.name || "");
const host = ref(props.config?.host || "");
const port = ref(String(props.config?.port || (initialType === "ftp" ? 21 : 22)));
const username = ref(props.config?.username || "");
const authMethod = ref(props.config?.auth_method || "password");
const password = ref(props.config?.password || "");
// 私钥永远不会从后端回到这里（Rust 侧 skip_serializing）——打开编辑时一律为空，
// 「已存储」状态由 has_private_key 这个布尔量表达。
const privateKeyPem = ref<string | undefined>(undefined);
const privateKeyName = ref<string | undefined>(undefined);
const hadExistingKey = ref(!!props.config?.has_private_key);
// ✕ 对「已存储的私钥」是显式删除请求——因为省略 private_key_pem 现在表示
// 「保持不变」而不是「删除」，删除必须是一个单独的、用户主动做出的信号。
const clearPrivateKey = ref(false);
const groupPath = ref(
  props.config?.group_path
    ? (props.config.group_path || "/").slice(1)
    : props.initialFolderPath
      ? props.initialFolderPath.startsWith("/")
        ? props.initialFolderPath.slice(1)
        : props.initialFolderPath
      : "",
);
const ftpTls = ref<FtpTls>(props.config?.ftp_tls || "none");
const ftpPassive = ref(props.config?.ftp_passive ?? true);
const proxyType = ref<ProxyType>(props.config?.proxy_type ?? "none");
const proxyHost = ref(props.config?.proxy_host || "");
const proxyPort = ref(
  String(props.config?.proxy_port || (props.config?.proxy_type === "http" ? 8080 : 1080)),
);
const proxyUsername = ref(props.config?.proxy_username || "");
const proxyPassword = ref("");
const shellPath = ref(props.config?.shell_path || "");
const shellArgs = ref(props.config?.shell_args || "");
const initCommand = ref(props.config?.init_command || "");
const terminalFont = ref(props.config?.terminal_font || "");
// Advanced SSH/SFTP options. Empty string = "use backend default" (auto /
// 10s / 15s) — buildConfig maps "" to undefined so the column stays NULL.
const addressFamily = ref(props.config?.address_family || "auto");
const connectTimeout = ref(
  props.config?.connect_timeout_secs != null ? String(props.config.connect_timeout_secs) : "",
);
const keepaliveInterval = ref(
  props.config?.keepalive_interval_secs != null ? String(props.config.keepalive_interval_secs) : "",
);
const suppressTmout = ref(props.config?.suppress_tmout === true);
const saving = ref(false);
const testing = ref(false);
const testResult = ref<{ kind: "ok" | "err"; text: string } | null>(null);
const showPassword = ref(false);
const showProxyPassword = ref(false);
const passwordVerifyTarget = ref<"password" | "proxy" | null>(null);

// refs to the actual DOM containers, keyed by the validate() field keys so we
// can focus the first invalid field on Save. Registered by each field's
// wrapper via a function ref.
const fieldRefs: Record<string, HTMLElement | null> = {};
function registerField(key: string, el: Element | ComponentPublicInstance | null): void {
  fieldRefs[key] = el instanceof HTMLElement ? el : null;
}
function focusField(key: string): void {
  fieldRefs[key]?.querySelector<HTMLInputElement>("input")?.focus();
}

// Fields currently flagged as invalid. Keys are stable strings; a value of
// true means "show red border + shake". Bumped whenever Save hits a fresh
// validation failure so the shake animation replays even if the same field
// was already in the error set.
const fieldErrors = ref<Record<string, boolean>>({});

/** Mark a field as valid again as soon as the user touches/edits it. */
function clearFieldError(key: string): void {
  if (fieldErrors.value[key]) {
    fieldErrors.value = { ...fieldErrors.value, [key]: false };
  }
}

// Replay the shake whenever a new validation failure lands on a flagged field.
// Removing + re-adding the class (with a forced reflow) restarts the CSS
// animation without remounting (which would lose focus).
function replayShake(key: string): void {
  const wrap = fieldRefs[key];
  if (!wrap) return;
  const target = wrap.querySelector<HTMLElement>(".el-input__wrapper") ?? wrap;
  target.classList.remove("field-error-shake");
  // force reflow so the browser registers the class removal
  void target.offsetWidth;
  target.classList.add("field-error-shake");
}

// 卸载时清空明文密钥（对齐旧版 useEffect cleanup）。
onUnmounted(() => {
  password.value = "";
  privateKeyPem.value = undefined;
  proxyPassword.value = "";
  showPassword.value = false;
  showProxyPassword.value = false;
});

function handleHostChange(v: string): void {
  const prevHost = host.value;
  host.value = v;
  clearFieldError("host");
  clearFieldError("name");
  // Auto-fill the connection name from the host while the user is still
  // "riding" the auto-fill — i.e. the name so far equals the host's prefix.
  // We detect this by checking whether name === prevHost (the host value one
  // keystroke ago): if so, the name was auto-mirroring and should keep
  // tracking the host. The moment the user edits the name independently it
  // diverges from that prefix and we stop syncing, so their manual edit
  // sticks. Typing then clearing the host fully resets to auto-fill.
  if (name.value === prevHost || name.value.trim() === "") {
    name.value = v;
  }
}

function handleNameChange(v: string): void {
  name.value = v;
  clearFieldError("name");
}

function handleTypeChange(t: ConnType): void {
  connType.value = t;
  // Required fields differ per type (e.g. local has no host/port), so any
  // stale per-field flags are meaningless now — clear them all.
  fieldErrors.value = {};
  const prev = port.value;
  const n = parseInt(prev, 10);
  const isDefault = n === 22 || n === 21 || prev === "";
  if (!isDefault) return;
  if (t === "ftp") {
    port.value = "21";
  } else if (t === "local") {
    // local terminals have no port
  } else {
    port.value = "22";
  }
}

// Validate the form. Returns a map of { fieldKey -> message } for every
// problem found; empty map = valid. Runs ALL checks (doesn't short-circuit)
// so Save can flag every missing field at once instead of one alert at a
// time. Keyed by the same stable ids used by the Input `error` props.
function validate(): Record<string, string> {
  const errs: Record<string, string> = {};

  // Local terminal — no host/port/auth, just a shell to spawn.
  if (connType.value === "local") {
    if (!name.value.trim()) errs.name = "请填写连接名称";
    if (!shellPath.value.trim()) errs.shellPath = "请填写启动 shell 路径";
    return errs;
  }

  if (!name.value.trim()) errs.name = "请填写连接名称";
  if (!host.value.trim()) errs.host = "请填写主机地址";
  if (!username.value.trim()) errs.username = "请填写用户名";

  const portNum = parseInt(port.value, 10);
  if (!Number.isInteger(portNum) || portNum < 1 || portNum > 65535) {
    errs.port = "端口必须为 1-65535 之间的整数";
  }

  const isEditing = !!props.config?.id;
  if (authMethod.value === "password" && !isEditing && !password.value) {
    errs.password = "请填写密码";
  }

  if (proxyType.value !== "none") {
    if (!proxyHost.value.trim()) errs.proxyHost = "代理主机地址不能为空";
    const pp = parseInt(proxyPort.value, 10);
    if (!Number.isInteger(pp) || pp < 1 || pp > 65535) {
      errs.proxyPort = "代理端口必须为 1-65535 之间的整数";
    }
  }

  // Advanced options (SSH/SFTP only). Empty = use backend default; when
  // filled, must be a sane positive integer (cap at 1 hour).
  if (connType.value === "ssh" || connType.value === "sftp") {
    if (connectTimeout.value.trim()) {
      const ct = parseInt(connectTimeout.value, 10);
      if (!Number.isInteger(ct) || ct < 1 || ct > 3600) {
        errs.connectTimeout = "连接超时须为 1-3600 之间的整数（秒）";
      }
    }
    if (keepaliveInterval.value.trim()) {
      const ki = parseInt(keepaliveInterval.value, 10);
      if (!Number.isInteger(ki) || ki < 1 || ki > 3600) {
        errs.keepaliveInterval = "Keepalive 间隔须为 1-3600 之间的整数（秒）";
      }
    }
  }
  return errs;
}

// Build the ConnectionConfig exactly as `handleSave` would (so the test
// reflects precisely what would be committed). Returns null when validation
// fails — the caller is expected to have run `validate()` first and surfaced
// the per-field errors. Shared by both save and test so the two paths can't
// drift.
function buildConfig(): ConnectionConfig | null {
  const errs = validate();
  if (Object.keys(errs).length > 0) return null;

  const trimmedGroup = groupPath.value
    .trim()
    .replace(/^\/+|\/+$/g, "")
    .replace(/\/+/g, "/");
  const group = trimmedGroup ? `/${trimmedGroup}` : "/";

  // Local terminal — no host/port/auth, just a shell to spawn.
  if (connType.value === "local") {
    return {
      id: props.config?.id || crypto.randomUUID(),
      name: name.value.trim(),
      host: "",
      port: 0,
      username: "",
      auth_method: "password", // unused for local, struct requires a value
      conn_type: "local",
      group_path: group,
      shell_path: shellPath.value.trim(),
      shell_args: shellArgs.value.trim() || undefined,
      init_command: initCommand.value.trim() || undefined,
      terminal_font: terminalFont.value.trim() || undefined,
      created_at: props.config?.created_at || new Date().toISOString(),
    };
  }

  const portNum = parseInt(port.value, 10);
  const isEditing = !!props.config?.id;
  const passwordToSend =
    authMethod.value === "password" && password.value ? password.value : undefined;

  let proxyPortNum: number | undefined;
  if (proxyType.value !== "none") proxyPortNum = parseInt(proxyPort.value, 10);

  return {
    id: props.config?.id || crypto.randomUUID(),
    name: name.value.trim(),
    host: host.value.trim(),
    port: portNum,
    username: username.value.trim(),
    auth_method: authMethod.value,
    password: passwordToSend,
    private_key_pem: authMethod.value === "key" ? privateKeyPem.value : undefined,
    clear_private_key: clearPrivateKey.value,
    conn_type: connType.value,
    group_path: group,
    ftp_tls: connType.value === "ftp" ? ftpTls.value : "none",
    ftp_passive: connType.value === "ftp" ? ftpPassive.value : true,
    proxy_type: proxyType.value,
    proxy_host: proxyType.value !== "none" ? proxyHost.value.trim() : undefined,
    proxy_port: proxyPortNum,
    proxy_username:
      proxyType.value !== "none" && proxyUsername.value.trim()
        ? proxyUsername.value.trim()
        : undefined,
    proxy_password: proxyType.value !== "none" && proxyPassword.value ? proxyPassword.value : undefined,
    terminal_font: terminalFont.value.trim() || undefined,
    address_family: addressFamily.value,
    connect_timeout_secs: connectTimeout.value.trim() ? parseInt(connectTimeout.value, 10) : undefined,
    keepalive_interval_secs: keepaliveInterval.value.trim()
      ? parseInt(keepaliveInterval.value, 10)
      : undefined,
    suppress_tmout: suppressTmout.value,
    created_at: props.config?.created_at || new Date().toISOString(),
  };
}

// Surface validation errors inline (red border + shake) and focus the first
// invalid field. Returns true when the form is valid, false otherwise.
// Shared by Save and Test so both flag missing fields the same way.
function showValidationErrors(): boolean {
  const errs = validate();
  if (Object.keys(errs).length === 0) {
    fieldErrors.value = {};
    return true;
  }
  const next: Record<string, boolean> = {};
  for (const k of Object.keys(errs)) next[k] = true;
  fieldErrors.value = next;
  for (const k of Object.keys(next)) replayShake(k);
  const firstKey = FIELD_FOCUS_ORDER.find((k) => next[k]);
  if (firstKey) focusField(firstKey);
  return false;
}

async function handleSave(): Promise<void> {
  if (!showValidationErrors()) return;
  const conn = buildConfig();
  if (!conn) return;
  saving.value = true;
  try {
    await saveConnection(conn);
    // 旧 App.tsx onSave = 复位 + reload；新壳层把两件事拆成 close/save 两个事件
    emit("save");
    emit("close");
  } catch (e) {
    // 旧版 alert 按迁移约定替换为 toast（文案不变）
    toast(`保存失败: ${e}`, { type: "error" });
  } finally {
    saving.value = false;
  }
}

async function handleTest(): Promise<void> {
  if (!showValidationErrors()) return;
  const conn = buildConfig();
  if (!conn) return;
  testing.value = true;
  testResult.value = null;
  try {
    const msg = await testConnection(conn);
    testResult.value = { kind: "ok", text: msg };
  } catch (e) {
    testResult.value = { kind: "err", text: String(e) };
  } finally {
    testing.value = false;
  }
}

// ── 密码原子揭示（旧版 onSuccess 内联逻辑） ──
async function onPasswordVerified(masterPassword: string): Promise<void> {
  // Atomic backend reveal: verify master password + decrypt in ONE
  // command — the frontend can no longer be the only gate.
  if (passwordVerifyTarget.value === "password" && props.config?.id) {
    try {
      const pw = await revealConnectionPassword(props.config.id, masterPassword);
      if (pw) {
        password.value = pw;
        showPassword.value = true;
      }
    } catch {
      toast("获取密码失败", { type: "error" });
    }
  } else if (passwordVerifyTarget.value === "proxy" && props.config?.id) {
    try {
      const pw = await revealConnectionProxyPassword(props.config.id, masterPassword);
      if (pw) {
        proxyPassword.value = pw;
        showProxyPassword.value = true;
      }
    } catch {
      toast("获取代理密码失败", { type: "error" });
    }
  }
  passwordVerifyTarget.value = null;
}

function closePasswordVerify(): void {
  passwordVerifyTarget.value = null;
}

// ── 私钥选择器（旧 KeyPicker 子组件内联；同一时刻仅存在一个实例） ──
const keyBusy = ref(false);
const keyErr = ref<string | null>(null);
const hasNewKey = computed(() => !!privateKeyPem.value);
const hasExistingKey = computed(
  () => !hasNewKey.value && hadExistingKey.value && !clearPrivateKey.value,
);

async function pickKeyFile(): Promise<void> {
  keyErr.value = null;
  keyBusy.value = true;
  try {
    const selected = await open({
      multiple: false,
      title: "选择私钥文件",
      filters: [
        { name: "Private key", extensions: ["pem", "key", "id_rsa", "ppk", "openssh"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (!selected || Array.isArray(selected)) return;
    const content: string = await readTextFile(selected);
    const keyName = selected.split(/[\\/]/).pop() || "key";
    privateKeyPem.value = content;
    privateKeyName.value = keyName;
    hadExistingKey.value = false;
    clearPrivateKey.value = false; // 新导入的密钥取代「删除」意图
  } catch (e) {
    keyErr.value = String(e);
  } finally {
    keyBusy.value = false;
  }
}

function clearKey(): void {
  privateKeyPem.value = undefined;
  privateKeyName.value = undefined;
  // 只是丢弃本次刚导入的文件 → 不动已存储的私钥；
  // 清的是已存储的私钥 → 标记显式删除。
  if (!hadExistingKey.value) return;
  hadExistingKey.value = false;
  clearPrivateKey.value = true;
}

// ── 派生值与下拉选项 ──
const namePlaceholder = computed(() =>
  connType.value === "local" ? "本地终端" : `${connType.value}_${host.value || "server"}`,
);
const passwordPlaceholder = computed(() => (props.config ? "留空保持不变" : "••••••"));
const hasProxyPw = computed(() => !!props.config?.proxy_type && props.config.proxy_type !== "none");
const proxyPasswordPlaceholder = computed(() => (hasProxyPw.value ? "留空保持不变" : "••••••"));
const ftpModeValue = computed(() => (ftpPassive.value ? "passive" : "active"));

const shellPresetOptions: SelectOption[] = [
  { value: "", label: "— 自定义路径 —" },
  ...SHELL_PRESETS,
];
const authOptions: SelectOption[] = [
  { value: "password", label: "密码认证" },
  { value: "key", label: "私钥认证" },
];
const ftpTlsOptions: SelectOption[] = [
  { value: "none", label: "不加密 (FTP)" },
  { value: "explicit", label: "显式 TLS (FTPES)" },
  { value: "implicit", label: "隐式 TLS (FTPS, 990)" },
];
const ftpModeOptions: SelectOption[] = [
  { value: "passive", label: "被动模式 (PASV, 推荐)" },
  { value: "active", label: "主动模式 (PORT)" },
];
const proxyTypeOptions: SelectOption[] = [
  { value: "none", label: "直连（不使用代理）" },
  { value: "socks5", label: "SOCKS5 代理" },
  { value: "http", label: "HTTP CONNECT 代理" },
];
const addressFamilyOptions: SelectOption[] = [
  { value: "auto", label: "自动（IPv4 / IPv6）" },
  { value: "ipv4", label: "强制 IPv4" },
  { value: "ipv6", label: "强制 IPv6" },
];
const folderOptions = computed<SelectOption[]>(() => [
  { value: "", label: "根目录 (/)" },
  ...props.folders.map((f) => {
    const display = f.startsWith("/") ? f.slice(1) : f;
    return { value: display, label: display };
  }),
]);

// MySelect 的 update:modelValue 载荷类型是 unknown；各下拉按域收窄
function onAuthMethod(v: unknown): void {
  authMethod.value = v as string;
}
function onShellPreset(v: unknown): void {
  shellPath.value = v as string;
}
function onFtpTls(v: unknown): void {
  ftpTls.value = v as FtpTls;
}
function onFtpMode(v: unknown): void {
  ftpPassive.value = v === "passive";
}
function onProxyType(v: unknown): void {
  proxyType.value = v as ProxyType;
}
function onAddressFamily(v: unknown): void {
  addressFamily.value = v as string;
}
function onGroupPick(v: unknown): void {
  groupPath.value = v as string;
}
function onSuppressTmout(v: boolean | (string | number | boolean)[]): void {
  suppressTmout.value = v === true;
}

// 带副作用的文本输入（更新即清除该字段错误标记）
function onPortInput(v: string): void {
  port.value = v;
  clearFieldError("port");
}
function onUsernameInput(v: string): void {
  username.value = v;
  clearFieldError("username");
}
function onPasswordInput(v: string): void {
  password.value = v;
  clearFieldError("password");
}
function onShellPathInput(v: string): void {
  shellPath.value = v;
  clearFieldError("shellPath");
}
function onProxyHostInput(v: string): void {
  proxyHost.value = v;
  clearFieldError("proxyHost");
}
function onProxyPortInput(v: string): void {
  proxyPort.value = v;
  clearFieldError("proxyPort");
}
function onConnectTimeoutInput(v: string): void {
  connectTimeout.value = v;
  clearFieldError("connectTimeout");
}
function onKeepaliveInput(v: string): void {
  keepaliveInterval.value = v;
  clearFieldError("keepaliveInterval");
}
</script>

<template>
  <!-- MyDialog 壳：dismissable 默认 false（遮罩/Esc/X 全禁用，表单防误关，与旧版一致）；
       header/footer 均为自定义插槽（标题动态：编辑连接/新建连接）。壳层 v-if 挂载，恒为打开态。 -->
  <MyDialog :model-value="true" :width="480" align-center class="conn-dialog">
    <template #header>
      <div class="dlg-header">
        <div>
          <div class="title">{{ config ? "编辑连接" : "新建连接" }}</div>
          <div class="subtitle">配置您的远程服务器连接</div>
        </div>
        <div class="type-badge">{{ connType.toUpperCase() }}</div>
      </div>
    </template>

    <!-- Type Selector -->
      <div class="type-selector" :class="{ disabled: !!config }">
        <button
          v-for="opt in TYPE_OPTIONS"
          :key="opt.value"
          type="button"
          class="type-btn"
          :class="{ active: connType === opt.value }"
          :disabled="!!config"
          @click="handleTypeChange(opt.value)"
        >
          <ConnIcon :type="opt.value" :size="24" class="type-icon" />
          <span>{{ opt.label }}</span>
        </button>
      </div>

      <!-- Form -->
      <div class="form-body">
        <!-- 基本设置 -->
        <div class="field-group">
          <div class="field-group-label">基本设置</div>
          <div class="form-field">
            <label class="form-field-label">连接名称<span class="req">*</span></label>
            <div
              class="conn-field"
              :class="{ 'is-error': !!fieldErrors.name }"
              :ref="(el) => registerField('name', el)"
            >
              <MyInput
                :model-value="name"
                :clearable="false"
                :placeholder="namePlaceholder"
                @update:model-value="handleNameChange"
              />
            </div>
          </div>
          <template v-if="connType !== 'local'">
            <div class="row">
              <div class="col-2">
                <div class="form-field">
                  <label class="form-field-label">主机地址<span class="req">*</span></label>
                  <div
                    class="conn-field"
                    :class="{ 'is-error': !!fieldErrors.host }"
                    :ref="(el) => registerField('host', el)"
                  >
                    <MyInput
                      :model-value="host"
                      :clearable="false"
                      placeholder="192.168.1.100"
                      autofocus
                      @update:model-value="handleHostChange"
                    />
                  </div>
                </div>
              </div>
              <div class="col-1">
                <div class="form-field">
                  <label class="form-field-label">端口<span class="req">*</span></label>
                  <div
                    class="conn-field"
                    :class="{ 'is-error': !!fieldErrors.port }"
                    :ref="(el) => registerField('port', el)"
                  >
                    <MyInput
                      :model-value="port"
                      :clearable="false"
                      placeholder="22"
                      @update:model-value="onPortInput"
                    />
                  </div>
                </div>
              </div>
            </div>
            <div class="form-field">
              <label class="form-field-label">用户名<span class="req">*</span></label>
              <div
                class="conn-field"
                :class="{ 'is-error': !!fieldErrors.username }"
                :ref="(el) => registerField('username', el)"
              >
                <MyInput
                  :model-value="username"
                  :clearable="false"
                  placeholder="root"
                  @update:model-value="onUsernameInput"
                />
              </div>
            </div>
          </template>
        </div>

        <!-- 本地终端：启动 Shell / 其余：认证方式 -->
        <div v-if="connType === 'local'" class="field-group">
          <div class="field-group-label">启动 Shell</div>
          <div class="form-field">
            <label class="form-field-label">Shell 类型（快速选择）</label>
            <div class="conn-field">
              <MySelect
                :model-value="shellPath"
                :options="shellPresetOptions"
                :filterable="false"
                :clearable="false"
                @update:model-value="onShellPreset"
              />
            </div>
          </div>
          <div class="form-field">
            <label class="form-field-label">可执行文件路径<span class="req">*</span></label>
            <div
              class="conn-field"
              :class="{ 'is-error': !!fieldErrors.shellPath }"
              :ref="(el) => registerField('shellPath', el)"
            >
              <MyInput
                :model-value="shellPath"
                :clearable="false"
                placeholder="pwsh.exe 或完整路径"
                autofocus
                @update:model-value="onShellPathInput"
              />
            </div>
          </div>
          <div class="form-field">
            <label class="form-field-label">启动参数（可选）</label>
            <div class="conn-field">
              <MyInput v-model="shellArgs" :clearable="false" placeholder="-d Ubuntu / --login -i" />
            </div>
          </div>
          <div class="form-field">
            <label class="form-field-label">启动命令（可选）</label>
            <div class="conn-field">
              <MyInput v-model="initCommand" :clearable="false" placeholder="claude / docker ps" />
            </div>
          </div>
          <div class="hint-box">
            本地终端在本机启动一个 shell（PowerShell / CMD / WSL 等），等同打开一个本地命令行窗口。「启动命令」会在 shell 就绪后自动执行一次（如打开即跑 claude）。需要管理员权限？在「设置 → 管理员权限」以管理员重启 MyShell，所有本地连接即获得管理员权限。
          </div>
        </div>
        <div v-else class="field-group">
          <div class="field-group-label">认证方式</div>
          <div class="form-field">
            <label class="form-field-label">认证类型</label>
            <div class="conn-field">
              <MySelect
                :model-value="authMethod"
                :options="authOptions"
                :filterable="false"
                :clearable="false"
                @update:model-value="onAuthMethod"
              />
            </div>
          </div>
          <div v-if="authMethod === 'password'" class="form-field">
            <label class="form-field-label">密码<span v-if="!config" class="req">*</span></label>
            <div class="pw-row">
              <div
                class="conn-field pw-grow"
                :class="{ 'is-error': !!fieldErrors.password }"
                :ref="(el) => registerField('password', el)"
              >
                <MyInput
                  :model-value="password"
                  :clearable="false"
                  :type="showPassword ? 'text' : 'password'"
                  :placeholder="passwordPlaceholder"
                  @update:model-value="onPasswordInput"
                />
              </div>
              <button
                v-if="config"
                type="button"
                class="reveal-btn"
                :class="{ shown: showPassword }"
                :title="showPassword ? '隐藏密码' : '查看密码'"
                @click="passwordVerifyTarget = 'password'"
              >
                <el-icon :size="14"><Hide v-if="showPassword" /><View v-else /></el-icon>
              </button>
            </div>
          </div>
          <div v-else class="form-field">
            <label class="form-field-label">私钥文件</label>
            <div class="key-row">
              <MyButton
                variant="primary"
                size="small"
                :disabled="keyBusy"
                @click="pickKeyFile"
              >
                <el-icon v-if="!keyBusy && !hasExistingKey" :size="13"><FolderOpened /></el-icon>
                {{ keyBusy ? "读取中..." : hasExistingKey ? "替换私钥" : "选择私钥文件" }}
              </MyButton>
              <button
                v-if="hasNewKey || hasExistingKey"
                type="button"
                class="key-clear"
                title="清除"
                @click="clearKey"
              >
                ✕
              </button>
              <span class="key-status" :class="{ ok: hasNewKey || hasExistingKey }">
                {{
                  hasNewKey
                    ? `✓ 已导入：${privateKeyName}`
                    : hasExistingKey
                      ? "✓ 已加密存储"
                      : clearPrivateKey
                        ? "⚠ 保存后将删除已存储的私钥"
                        : "未选择"
                }}
              </span>
            </div>
            <div v-if="keyErr" class="key-err">{{ keyErr }}</div>
            <div class="key-hint">私钥将以主密码加密后存入本地数据库，原文件不会被修改。</div>
          </div>
        </div>

        <!-- FTP 选项 -->
        <div v-if="connType === 'ftp'" class="field-group">
          <div class="field-group-label">FTP 选项</div>
          <div class="form-field">
            <label class="form-field-label">TLS 模式</label>
            <div class="conn-field">
              <MySelect
                :model-value="ftpTls"
                :options="ftpTlsOptions"
                :filterable="false"
                :clearable="false"
                @update:model-value="onFtpTls"
              />
            </div>
          </div>
          <div class="form-field">
            <label class="form-field-label">传输模式</label>
            <div class="conn-field">
              <MySelect
                :model-value="ftpModeValue"
                :options="ftpModeOptions"
                :filterable="false"
                :clearable="false"
                @update:model-value="onFtpMode"
              />
            </div>
          </div>
        </div>

        <!-- 代理设置 -->
        <div v-if="connType !== 'local'" class="field-group">
          <div class="field-group-label">代理设置</div>
          <div class="form-field">
            <label class="form-field-label">代理类型</label>
            <div class="conn-field">
              <MySelect
                :model-value="proxyType"
                :options="proxyTypeOptions"
                :filterable="false"
                :clearable="false"
                @update:model-value="onProxyType"
              />
            </div>
          </div>
          <template v-if="proxyType !== 'none'">
            <div class="row">
              <div class="col-2">
                <div class="form-field">
                  <label class="form-field-label">代理主机<span class="req">*</span></label>
                  <div
                    class="conn-field"
                    :class="{ 'is-error': !!fieldErrors.proxyHost }"
                    :ref="(el) => registerField('proxyHost', el)"
                  >
                    <MyInput
                      :model-value="proxyHost"
                      :clearable="false"
                      placeholder="127.0.0.1"
                      @update:model-value="onProxyHostInput"
                    />
                  </div>
                </div>
              </div>
              <div class="col-1">
                <div class="form-field">
                  <label class="form-field-label">端口<span class="req">*</span></label>
                  <div
                    class="conn-field"
                    :class="{ 'is-error': !!fieldErrors.proxyPort }"
                    :ref="(el) => registerField('proxyPort', el)"
                  >
                    <MyInput
                      :model-value="proxyPort"
                      :clearable="false"
                      :placeholder="proxyType === 'http' ? '8080' : '1080'"
                      @update:model-value="onProxyPortInput"
                    />
                  </div>
                </div>
              </div>
            </div>
            <div class="form-field">
              <label class="form-field-label">代理用户名（可选）</label>
              <div class="conn-field">
                <MyInput v-model="proxyUsername" :clearable="false" placeholder="anonymous" />
              </div>
            </div>
            <div class="form-field">
              <label class="form-field-label">代理密码（可选）</label>
              <div class="pw-row">
                <div class="conn-field pw-grow">
                  <MyInput
                    v-model="proxyPassword"
                    :clearable="false"
                    :type="showProxyPassword ? 'text' : 'password'"
                    :placeholder="proxyPasswordPlaceholder"
                  />
                </div>
                <button
                  v-if="hasProxyPw"
                  type="button"
                  class="reveal-btn"
                  :class="{ shown: showProxyPassword }"
                  :title="showProxyPassword ? '隐藏密码' : '查看密码'"
                  @click="passwordVerifyTarget = 'proxy'"
                >
                  <el-icon :size="14"><Hide v-if="showProxyPassword" /><View v-else /></el-icon>
                </button>
              </div>
            </div>
            <div v-if="connType === 'ftp' && ftpTls !== 'none'" class="warning-box">
              注意：FTPS（TLS）当前版本暂不支持，连接时会报错。如需走代理，请把 TLS 模式切回「不加密」。
            </div>
          </template>
        </div>

        <!-- 高级选项（SSH/SFTP） -->
        <div v-if="connType === 'ssh' || connType === 'sftp'" class="field-group">
          <div class="field-group-label">高级选项</div>
          <div class="form-field">
            <label class="form-field-label">地址族</label>
            <div class="conn-field">
              <MySelect
                :model-value="addressFamily"
                :options="addressFamilyOptions"
                :filterable="false"
                :clearable="false"
                @update:model-value="onAddressFamily"
              />
            </div>
          </div>
          <div class="row">
            <div class="col-1">
              <div class="form-field">
                <label class="form-field-label">连接超时（秒，留空=10）</label>
                <div
                  class="conn-field"
                  :class="{ 'is-error': !!fieldErrors.connectTimeout }"
                  :ref="(el) => registerField('connectTimeout', el)"
                >
                  <MyInput
                    :model-value="connectTimeout"
                    :clearable="false"
                    placeholder="10"
                    @update:model-value="onConnectTimeoutInput"
                  />
                </div>
              </div>
            </div>
            <div class="col-1">
              <div class="form-field">
                <label class="form-field-label">Keepalive 间隔（秒，留空=15）</label>
                <div
                  class="conn-field"
                  :class="{ 'is-error': !!fieldErrors.keepaliveInterval }"
                  :ref="(el) => registerField('keepaliveInterval', el)"
                >
                  <MyInput
                    :model-value="keepaliveInterval"
                    :clearable="false"
                    placeholder="15"
                    @update:model-value="onKeepaliveInput"
                  />
                </div>
              </div>
            </div>
          </div>
          <div class="tmout-row">
            <MyCheckbox :model-value="suppressTmout" @update:model-value="onSuppressTmout">
              防止 shell 空闲自动登出（TMOUT）
            </MyCheckbox>
          </div>
          <div v-if="suppressTmout" class="tmout-hint">
            登录后自动执行 export TMOUT=0，阻止服务器按空闲时间踢人。不额外开通道，受限账号也可用。
          </div>
        </div>

        <!-- 分组 -->
        <div class="field-group">
          <div class="field-group-label">分组</div>
          <div class="form-field">
            <label class="form-field-label">选择或输入分组路径</label>
            <div class="stack">
              <div v-if="folders.length > 0" class="conn-field">
                <MySelect
                  :model-value="groupPath"
                  :options="folderOptions"
                  :filterable="false"
                  :clearable="false"
                  @update:model-value="onGroupPick"
                />
              </div>
              <div class="conn-field">
                <MyInput
                  v-model="groupPath"
                  :clearable="false"
                  :placeholder="folders.length > 0 ? '或手动输入新分组路径' : '生产/web'"
                />
              </div>
              <div v-if="folders.length === 0" class="hint-box">
                提示：先在左侧创建文件夹，这里就可以快速选择了
              </div>
            </div>
          </div>
        </div>

        <!-- 终端（SSH/本地） -->
        <div v-if="connType === 'ssh' || connType === 'local'" class="field-group">
          <div class="field-group-label">终端</div>
          <div class="form-field">
            <label class="form-field-label">字体（可选）</label>
            <FontField v-model="terminalFont" placeholder="留空使用全局字体" />
          </div>
          <div class="field-note">为该连接单独指定终端字体；留空则使用设置中的全局字体。</div>
        </div>
      </div>

      <!-- Footer（#footer 插槽：测试/取消/保存 三键 + 测试结果横幅。
           旧版 sticky 语义由壳承担：弹窗 body 是滚动容器，footer 恒定可见） -->
      <template #footer>
        <div class="dlg-footer">
          <div v-if="testResult" class="test-result" :class="testResult.kind === 'ok' ? 'ok' : 'err'">
            {{ testResult.kind === "ok" ? "✓ " : "✗ " }}{{ testResult.text }}
          </div>
          <div class="actions">
            <MyButton
              variant="secondary"
              class="test-btn"
              :disabled="testing || saving"
              title="验证当前配置能否连通（不保存）"
              @click="handleTest"
            >
              {{ testing ? "测试中..." : "测试" }}
            </MyButton>
            <MyButton variant="secondary" @click="emit('close')">取消</MyButton>
            <MyButton variant="primary" :disabled="saving || testing" @click="handleSave">
              {{ saving ? "保存中..." : "保存" }}
            </MyButton>
          </div>
        </div>
      </template>
  </MyDialog>

  <!-- 密码原子揭示门禁 -->
  <PasswordVerifyDialog
    v-if="passwordVerifyTarget !== null"
    @success="onPasswordVerified"
    @close="closePasswordVerify"
  />
</template>

<style scoped>
/* ── MyDialog 壳适配（class 经 attrs 透传到 el-dialog 根元素，故用 :global） ──
   旧版卡片是滚动容器 + sticky footer；等价改为：弹窗整体限高，body 滚动，
   header/footer 恒定可见。 */
:global(.conn-dialog.el-dialog) {
  display: flex;
  flex-direction: column;
  max-height: 90vh;
}

:global(.conn-dialog .el-dialog__body) {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

/* 让 #footer 分隔线紧贴 body 结束处（去掉 EP footer 自带的 padding-top） */
:global(.conn-dialog .el-dialog__footer) {
  padding-top: 0;
}

/* ── Header（#header 插槽：标题 + 副标题 + 连接类型徽标） ──
   EP 2.14：.el-dialog 根元素带 16px 内边距，header 元素自带 padding-bottom 16px。
   负边距抵消根内边距，让分隔线横贯整个弹窗（旧版观感）。 */
.dlg-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--border-subtle);
  margin: 0 calc(var(--el-dialog-padding-primary) * -1);
  padding: 0 24px;
}

.title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.subtitle {
  font-size: 12px;
  color: var(--text-tertiary);
  margin-top: 2px;
}

.type-badge {
  padding: 4px 10px;
  background: var(--accent-primary-muted);
  border-radius: var(--radius-full);
  font-size: 11px;
  font-weight: 600;
  color: var(--accent-primary);
  letter-spacing: 0.04em;
}

/* ── 类型选择器 ── */
.type-selector {
  padding: 16px 24px 8px;
  display: flex;
  gap: 8px;
}

.type-selector.disabled {
  opacity: 0.5;
}

.type-btn {
  flex: 1;
  background: var(--bg-surface);
  color: var(--text-secondary);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  padding: 14px 12px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  transition: all var(--duration-normal) var(--ease-out-expo);
  box-shadow: none;
}

.type-btn.active {
  background: var(--accent-primary-muted);
  color: var(--accent-primary);
  border: 1px solid var(--border-accent);
  font-weight: 600;
  box-shadow: var(--shadow-glow);
}

.type-btn:disabled {
  cursor: not-allowed;
}

/* 活动类型图标着主色（覆盖 ConnIcon 内联类型色，需 !important） */
.type-btn.active .type-icon {
  color: var(--accent-primary) !important;
}

/* ── 表单区 ── */
.form-body {
  padding: 12px 24px 20px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.field-group {
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.field-group-label {
  font-size: 10px;
  font-weight: 700;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.1em;
}

/* 旧 FormField：标签在上（红星在后），控件在下 */
.form-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-field-label {
  font-size: 12px;
  color: var(--text-secondary);
  font-weight: 500;
}

.req {
  color: var(--error);
  margin-left: 3px;
  font-weight: 700;
}

.row {
  display: flex;
  gap: 12px;
}

.col-2 {
  flex: 2;
}

.col-1 {
  flex: 1;
}

.stack {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

/* myui 控件容器：错误态红框 + 抖动动画的挂载点 */
.conn-field {
  width: 100%;
}

.conn-field :deep(.el-select) {
  width: 100%;
}

/* 错误态：红边 + 红色外发光（对齐旧 Input 的 errStyle，含聚焦时保持红色） */
.conn-field.is-error :deep(.el-input__wrapper) {
  box-shadow:
    0 0 0 3px var(--error-muted),
    0 0 0 1px var(--error) inset !important;
}

/* Horizontal jitter used to flag a required field that's still empty when the
   user hits Save — draws the eye to exactly which input needs filling. */
@keyframes conn-field-shake {
  0%,
  100% {
    transform: translateX(0);
  }
  20% {
    transform: translateX(-6px);
  }
  40% {
    transform: translateX(5px);
  }
  60% {
    transform: translateX(-4px);
  }
  80% {
    transform: translateX(3px);
  }
}

.conn-field :deep(.field-error-shake) {
  animation: conn-field-shake 0.4s var(--ease-in-out);
}

/* 密码行：输入 + 显隐按钮 */
.pw-row {
  display: flex;
  gap: 8px;
  align-items: stretch;
}

.pw-grow {
  flex: 1;
}

.reveal-btn {
  padding: 0 12px;
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  font-size: 14px;
  cursor: pointer;
  color: var(--text-tertiary);
  transition: all var(--duration-fast) var(--ease-in-out);
}

.reveal-btn.shown {
  color: var(--success);
}

/* ── 私钥选择器 ── */
.key-row {
  display: flex;
  gap: 8px;
  align-items: center;
}

.key-clear {
  padding: 8px 12px;
  background: transparent;
  color: var(--text-tertiary);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  font-size: 12px;
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-in-out);
}

.key-status {
  font-size: 11px;
  color: var(--text-muted);
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.key-status.ok {
  color: var(--success);
}

.key-err {
  margin-top: 8px;
  font-size: 11px;
  color: var(--error);
}

.key-hint {
  margin-top: 8px;
  font-size: 10px;
  color: var(--text-muted);
  line-height: 1.5;
  padding: 8px 10px;
  background: var(--bg-surface);
  border-radius: var(--radius-sm);
}

/* ── 提示块 ── */
.hint-box {
  font-size: 11px;
  color: var(--text-muted);
  line-height: 1.5;
  padding: 8px 10px;
  background: var(--bg-surface);
  border-radius: var(--radius-sm);
}

.warning-box {
  padding: 10px 12px;
  background: var(--warning-muted);
  border: 1px solid var(--warning);
  border-radius: var(--radius-md);
  font-size: 11px;
  color: var(--warning);
  line-height: 1.5;
}

.field-note {
  font-size: 11px;
  color: var(--text-muted);
  line-height: 1.5;
}

.tmout-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 4px;
}

.tmout-hint {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-top: 4px;
  line-height: 1.5;
}

/* ── Footer（#footer 插槽：测试结果横幅 + 三键） ──
   左右/下负边距横贯弹窗全宽（抵消 .el-dialog 根元素内边距）；不用顶部负边距
   （会与可滚动 body 重叠），与 body 的间隔由上方 :global 清零 footer padding-top 解决 */
.dlg-footer {
  border-top: 1px solid var(--border-subtle);
  margin: 0 calc(var(--el-dialog-padding-primary) * -1)
    calc(var(--el-dialog-padding-primary) * -1);
  padding: 12px 24px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.test-result {
  font-size: 12px;
  line-height: 1.5;
  border-radius: var(--radius-md);
  padding: 8px 12px;
  word-break: break-word;
}

.test-result.ok {
  color: var(--success);
  background: var(--success-muted);
  border: 1px solid var(--success);
}

.test-result.err {
  color: var(--error);
  background: var(--error-muted);
  border: 1px solid var(--error);
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

.test-btn {
  margin-right: auto;
}
</style>
