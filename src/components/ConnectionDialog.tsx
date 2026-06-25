import { useState, useRef, useEffect } from "react";
import { FontField } from "./FontField";
import { open } from "@tauri-apps/plugin-dialog";
import { saveConnection, getConnectionPassword, getConnectionProxyPassword, readTextFile, testConnection } from "../api";
import type { ConnectionConfig, ConnType, FtpTls, ProxyType } from "../api";
import { PasswordVerifyDialog } from "./PasswordVerifyDialog";
import { ConnIcon } from "./ConnIcon";

interface Props {
  config: ConnectionConfig | null;
  onClose: () => void;
  onSave: () => void;
  initialConnType?: ConnType;
  initialFolderPath?: string;
  folders?: string[];
}

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
] as const;

export function ConnectionDialog({ config, onClose, onSave, initialConnType, initialFolderPath, folders = [] }: Props) {
  const [connType, setConnType] = useState<ConnType>(
    (config?.conn_type as ConnType) || initialConnType || "ssh"
  );
  const [name, setName] = useState(config?.name || "");
  const [host, setHost] = useState(config?.host || "");
  const [port, setPort] = useState(String(config?.port || (connType === "ftp" ? 21 : 22)));
  const [username, setUsername] = useState(config?.username || "");
  const [authMethod, setAuthMethod] = useState(config?.auth_method || "password");
  const [password, setPassword] = useState(config?.password || "");
  const [privateKeyPem, setPrivateKeyPem] = useState<string | undefined>(config?.private_key_pem);
  const [privateKeyName, setPrivateKeyName] = useState<string | undefined>(undefined);
  const [hadExistingKey, setHadExistingKey] = useState(!!config?.private_key_pem);
  const [groupPath, setGroupPath] = useState(
    config?.group_path
      ? (config.group_path || "/").slice(1)
      : initialFolderPath
        ? (initialFolderPath.startsWith("/") ? initialFolderPath.slice(1) : initialFolderPath)
        : ""
  );
  const [ftpTls, setFtpTls] = useState<FtpTls>(config?.ftp_tls || "none");
  const [ftpPassive, setFtpPassive] = useState(config?.ftp_passive ?? true);
  const [proxyType, setProxyType] = useState<ProxyType>(
    (config?.proxy_type as ProxyType) || "none"
  );
  const [proxyHost, setProxyHost] = useState(config?.proxy_host || "");
  const [proxyPort, setProxyPort] = useState(
    String(config?.proxy_port || (config?.proxy_type === "http" ? 8080 : 1080))
  );
  const [proxyUsername, setProxyUsername] = useState(config?.proxy_username || "");
  const [proxyPassword, setProxyPassword] = useState("");
  const [shellPath, setShellPath] = useState(config?.shell_path || "");
  const [shellArgs, setShellArgs] = useState(config?.shell_args || "");
  const [initCommand, setInitCommand] = useState(config?.init_command || "");
  const [terminalFont, setTerminalFont] = useState(config?.terminal_font || "");
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const [showPassword, setShowPassword] = useState(false);
  const [showProxyPassword, setShowProxyPassword] = useState(false);
  const [passwordVerifyTarget, setPasswordVerifyTarget] = useState<"password" | "proxy" | null>(null);

  // refs to the actual DOM inputs, keyed by the validate() field keys so we
  // can focus the first invalid field on Save. Registered by each Input via
  // the `errorKey` prop.
  const fieldRefs = useRef<Record<string, HTMLElement | null>>({});
  function registerField(key: string, el: HTMLElement | null) {
    fieldRefs.current[key] = el;
  }
  function focusField(key: string) {
    fieldRefs.current[key]?.focus();
  }

  // Fields currently flagged as invalid. Keys are stable strings; a value of
  // true means "show red border + shake". Bumped whenever Save hits a fresh
  // validation failure so the shake animation replays even if the same field
  // was already in the error set.
  const [fieldErrors, setFieldErrors] = useState<Record<string, boolean>>({});
  const [shakeNonce, setShakeNonce] = useState(0);

  /** Mark a field as valid again as soon as the user touches/edits it. */
  function clearFieldError(key: string) {
    setFieldErrors((prev) => (prev[key] ? { ...prev, [key]: false } : prev));
  }

  useEffect(() => {
    return () => {
      setPassword("");
      setPrivateKeyPem(undefined);
      setProxyPassword("");
      setShowPassword(false);
      setShowProxyPassword(false);
    };
  }, []);

  function handleHostChange(v: string) {
    const prevHost = host;
    setHost(v);
    clearFieldError("host");
    clearFieldError("name");
    // Auto-fill the connection name from the host while the user is still
    // "riding" the auto-fill — i.e. the name so far equals the host's prefix.
    // We detect this by checking whether name === prevHost (the host value one
    // keystroke ago): if so, the name was auto-mirroring and should keep
    // tracking the host. The moment the user edits the name independently it
    // diverges from that prefix and we stop syncing, so their manual edit
    // sticks. Typing then clearing the host fully resets to auto-fill.
    if (name === prevHost || name.trim() === "") {
      setName(v);
    }
  }

  function handleNameChange(v: string) {
    setName(v);
    clearFieldError("name");
  }

  function handleTypeChange(t: ConnType) {
    setConnType(t);
    // Required fields differ per type (e.g. local has no host/port), so any
    // stale per-field flags are meaningless now — clear them all.
    setFieldErrors({});
    setPort((prev) => {
      const n = parseInt(prev, 10);
      const isDefault = n === 22 || n === 21 || prev === "";
      if (!isDefault) return prev;
      if (t === "ftp") return "21";
      if (t === "local") return prev; // local terminals have no port
      return "22";
    });
  }

  // Validate the form. Returns a map of { fieldKey -> message } for every
  // problem found; empty map = valid. Runs ALL checks (doesn't short-circuit)
  // so Save can flag every missing field at once instead of one alert at a
  // time. Keyed by the same stable ids used by the Input `error` props.
  function validate(): Record<string, string> {
    const errs: Record<string, string> = {};

    // Local terminal — no host/port/auth, just a shell to spawn.
    if (connType === "local") {
      if (!name.trim()) errs.name = "请填写连接名称";
      if (!shellPath.trim()) errs.shellPath = "请填写启动 shell 路径";
      return errs;
    }

    if (!name.trim()) errs.name = "请填写连接名称";
    if (!host.trim()) errs.host = "请填写主机地址";
    if (!username.trim()) errs.username = "请填写用户名";

    const portNum = parseInt(port, 10);
    if (!Number.isInteger(portNum) || portNum < 1 || portNum > 65535) {
      errs.port = "端口必须为 1-65535 之间的整数";
    }

    const isEditing = !!config?.id;
    if (authMethod === "password" && !isEditing && !password) {
      errs.password = "请填写密码";
    }

    if (proxyType !== "none") {
      if (!proxyHost.trim()) errs.proxyHost = "代理主机地址不能为空";
      const pp = parseInt(proxyPort, 10);
      if (!Number.isInteger(pp) || pp < 1 || pp > 65535) {
        errs.proxyPort = "代理端口必须为 1-65535 之间的整数";
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

    const trimmedGroup = groupPath
      .trim()
      .replace(/^\/+|\/+$/g, "")
      .replace(/\/+/g, "/");
    const group = trimmedGroup ? `/${trimmedGroup}` : "/";

    // Local terminal — no host/port/auth, just a shell to spawn.
    if (connType === "local") {
      return {
        id: config?.id || crypto.randomUUID(),
        name: name.trim(),
        host: "",
        port: 0,
        username: "",
        auth_method: "password", // unused for local, struct requires a value
        conn_type: "local",
        group_path: group,
        shell_path: shellPath.trim(),
        shell_args: shellArgs.trim() || undefined,
        init_command: initCommand.trim() || undefined,
        terminal_font: terminalFont.trim() || undefined,
        created_at: config?.created_at || new Date().toISOString(),
      };
    }

    const portNum = parseInt(port, 10);
    const isEditing = !!config?.id;
    const passwordToSend =
      authMethod === "password" && password ? password : undefined;

    let proxyPortNum: number | undefined;
    if (proxyType !== "none") proxyPortNum = parseInt(proxyPort, 10);

    return {
      id: config?.id || crypto.randomUUID(),
      name: name.trim(),
      host: host.trim(),
      port: portNum,
      username: username.trim(),
      auth_method: authMethod,
      password: passwordToSend,
      private_key_pem: authMethod === "key" ? privateKeyPem : undefined,
      conn_type: connType,
      group_path: group,
      ftp_tls: connType === "ftp" ? ftpTls : "none",
      ftp_passive: connType === "ftp" ? ftpPassive : true,
      proxy_type: proxyType,
      proxy_host: proxyType !== "none" ? proxyHost.trim() : undefined,
      proxy_port: proxyPortNum,
      proxy_username:
        proxyType !== "none" && proxyUsername.trim()
          ? proxyUsername.trim()
          : undefined,
      proxy_password: proxyType !== "none" && proxyPassword ? proxyPassword : undefined,
      terminal_font: terminalFont.trim() || undefined,
      created_at: config?.created_at || new Date().toISOString(),
    };
  }

  // Surface validation errors inline (red border + shake) and focus the first
  // invalid field. Returns true when the form is valid, false otherwise.
  // Shared by Save and Test so both flag missing fields the same way.
  function showValidationErrors(): boolean {
    const errs = validate();
    if (Object.keys(errs).length === 0) {
      setFieldErrors({});
      return true;
    }
    const next: Record<string, boolean> = {};
    for (const k of Object.keys(errs)) next[k] = true;
    setFieldErrors(next);
    setShakeNonce((n) => n + 1);
    const firstKey = FIELD_FOCUS_ORDER.find((k) => next[k]);
    if (firstKey) focusField(firstKey);
    return false;
  }

  async function handleSave() {
    if (!showValidationErrors()) return;
    const conn = buildConfig();
    if (!conn) return;
    setSaving(true);
    try {
      await saveConnection(conn);
      onSave();
    } catch (e) {
      alert(`保存失败: ${e}`);
    } finally {
      setSaving(false);
    }
  }

  async function handleTest() {
    if (!showValidationErrors()) return;
    const conn = buildConfig();
    if (!conn) return;
    setTesting(true);
    setTestResult(null);
    try {
      const msg = await testConnection(conn);
      setTestResult({ kind: "ok", text: msg });
    } catch (e) {
      setTestResult({ kind: "err", text: String(e) });
    } finally {
      setTesting(false);
    }
  }

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--bg-overlay)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 999,
        backdropFilter: "blur(8px)",
      }}
    >
      <div
        className="animate-scale-in"
        style={{
          background: "var(--bg-elevated)",
          border: "1px solid var(--border-emphasis)",
          borderRadius: "var(--radius-xl)",
          width: 480,
          maxHeight: "90vh",
          overflowY: "auto",
          boxShadow: "var(--shadow-xl)",
        }}
      >
        {/* Header */}
        <div
          style={{
            padding: "20px 24px",
            borderBottom: "1px solid var(--border-subtle)",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <div>
            <div style={{ fontSize: 16, fontWeight: 600, color: "var(--text-primary)" }}>
              {config ? "编辑连接" : "新建连接"}
            </div>
            <div style={{ fontSize: 12, color: "var(--text-tertiary)", marginTop: 2 }}>
              配置您的远程服务器连接
            </div>
          </div>
          <div
            style={{
              padding: "4px 10px",
              background: "var(--accent-primary-muted)",
              borderRadius: "var(--radius-full)",
              fontSize: 11,
              fontWeight: 600,
              color: "var(--accent-primary)",
              letterSpacing: "0.04em",
            }}
          >
            {connType.toUpperCase()}
          </div>
        </div>

        {/* Type Selector */}
        <div style={{ padding: "16px 24px 8px" }}>
          <TypeSelector value={connType} onChange={handleTypeChange} disabled={!!config} />
        </div>

        {/* Form */}
        <div style={{ padding: "12px 24px 20px", display: "flex", flexDirection: "column", gap: 16 }}>
          <FieldGroup label="基本设置">
            <FormField label="连接名称" required>
              <Input
                value={name}
                onChange={handleNameChange}
                placeholder={connType === "local" ? "本地终端" : `${connType}_${host || "server"}`}
                errorKey="name"
                error={fieldErrors.name}
                shakeNonce={shakeNonce}
                registerField={registerField}
              />
            </FormField>
            {connType !== "local" && (
              <>
                <div style={{ display: "flex", gap: 12 }}>
                  <div style={{ flex: 2 }}>
                    <FormField label="主机地址" required>
                      <Input
                        value={host}
                        onChange={handleHostChange}
                        placeholder="192.168.1.100"
                        autoFocus
                        errorKey="host"
                        error={fieldErrors.host}
                        shakeNonce={shakeNonce}
                        registerField={registerField}
                      />
                    </FormField>
                  </div>
                  <div style={{ flex: 1 }}>
                    <FormField label="端口" required>
                      <Input
                        value={port}
                        onChange={(v) => {
                          setPort(v);
                          clearFieldError("port");
                        }}
                        placeholder="22"
                        errorKey="port"
                        error={fieldErrors.port}
                        shakeNonce={shakeNonce}
                        registerField={registerField}
                      />
                    </FormField>
                  </div>
                </div>
                <FormField label="用户名" required>
                  <Input
                    value={username}
                    onChange={(v) => {
                      setUsername(v);
                      clearFieldError("username");
                    }}
                    placeholder="root"
                    errorKey="username"
                    error={fieldErrors.username}
                    shakeNonce={shakeNonce}
                    registerField={registerField}
                  />
                </FormField>
              </>
            )}
          </FieldGroup>

          {connType === "local" ? (
            <FieldGroup label="启动 Shell">
              <FormField label="Shell 类型（快速选择）">
                <Select
                  value={shellPath}
                  onChange={setShellPath}
                  options={[{ value: "", label: "— 自定义路径 —" }, ...SHELL_PRESETS]}
                />
              </FormField>
              <FormField label="可执行文件路径" required>
                <Input
                  value={shellPath}
                  onChange={(v) => {
                    setShellPath(v);
                    clearFieldError("shellPath");
                  }}
                  placeholder="pwsh.exe 或完整路径"
                  autoFocus
                  errorKey="shellPath"
                  error={fieldErrors.shellPath}
                  shakeNonce={shakeNonce}
                  registerField={registerField}
                />
              </FormField>
              <FormField label="启动参数（可选）">
                <Input
                  value={shellArgs}
                  onChange={setShellArgs}
                  placeholder="-d Ubuntu / --login -i"
                />
              </FormField>
              <FormField label="启动命令（可选）">
                <Input
                  value={initCommand}
                  onChange={setInitCommand}
                  placeholder="claude / docker ps"
                />
              </FormField>
              <div style={{
                fontSize: 11,
                color: "var(--text-muted)",
                lineHeight: 1.5,
                padding: "8px 10px",
                background: "var(--bg-surface)",
                borderRadius: "var(--radius-sm)",
              }}>
                💡 本地终端在本机启动一个 shell（PowerShell / CMD / WSL 等），等同打开一个本地命令行窗口。「启动命令」会在 shell 就绪后自动执行一次（如打开即跑 claude）。需要管理员权限？在「设置 → 管理员权限」以管理员重启 MyShell，所有本地连接即获得管理员权限。
              </div>
            </FieldGroup>
          ) : (
          <FieldGroup label="认证方式">
            <FormField label="认证类型">
              <Select
                value={authMethod}
                onChange={setAuthMethod}
                options={[
                  { value: "password", label: "密码认证" },
                  { value: "key", label: "私钥认证" },
                ]}
              />
            </FormField>
            {authMethod === "password" ? (
              <FormField label="密码" required={!config}>
                <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                  <div style={{ flex: 1 }}>
                    <Input
                      value={password}
                      onChange={(v) => {
                        setPassword(v);
                        clearFieldError("password");
                      }}
                      type={showPassword ? "text" : "password"}
                      placeholder={config ? "留空保持不变" : "••••••"}
                      errorKey="password"
                      error={fieldErrors.password}
                      shakeNonce={shakeNonce}
                      registerField={registerField}
                    />
                  </div>
                  {config && (
                    <button
                      type="button"
                      onClick={() => setPasswordVerifyTarget("password")}
                      title={showPassword ? "隐藏密码" : "查看密码"}
                      style={{
                        padding: "10px 12px",
                        background: "var(--bg-surface)",
                        border: "1px solid var(--border-default)",
                        borderRadius: "var(--radius-md)",
                        fontSize: 14,
                        cursor: "pointer",
                        color: showPassword ? "var(--success)" : "var(--text-tertiary)",
                        transition: "all var(--duration-fast) var(--ease-in-out)",
                      }}
                    >
                      {showPassword ? "󰈉" : "󰈈"}
                    </button>
                  )}
                </div>
              </FormField>
            ) : (
              <FormField label="私钥文件">
                <KeyPicker
                  pem={privateKeyPem}
                  fileName={privateKeyName}
                  hadExisting={hadExistingKey}
                  onPick={(content, name) => {
                    setPrivateKeyPem(content);
                    setPrivateKeyName(name);
                    setHadExistingKey(false);
                  }}
                  onClear={() => {
                    setPrivateKeyPem(undefined);
                    setPrivateKeyName(undefined);
                    setHadExistingKey(false);
                  }}
                />
              </FormField>
            )}
          </FieldGroup>
          )}

          {connType === "ftp" && (
            <FieldGroup label="FTP 选项">
              <FormField label="TLS 模式">
                <Select
                  value={ftpTls}
                  onChange={(v) => setFtpTls(v as FtpTls)}
                  options={[
                    { value: "none", label: "不加密 (FTP)" },
                    { value: "explicit", label: "显式 TLS (FTPES)" },
                    { value: "implicit", label: "隐式 TLS (FTPS, 990)" },
                  ]}
                />
              </FormField>
              <FormField label="传输模式">
                <Select
                  value={ftpPassive ? "passive" : "active"}
                  onChange={(v) => setFtpPassive(v === "passive")}
                  options={[
                    { value: "passive", label: "被动模式 (PASV, 推荐)" },
                    { value: "active", label: "主动模式 (PORT)" },
                  ]}
                />
              </FormField>
            </FieldGroup>
          )}

          {connType !== "local" && (
          <FieldGroup label="代理设置">
            <FormField label="代理类型">
              <Select
                value={proxyType}
                onChange={(v) => setProxyType(v as ProxyType)}
                options={[
                  { value: "none", label: "直连（不使用代理）" },
                  { value: "socks5", label: "SOCKS5 代理" },
                  { value: "http", label: "HTTP CONNECT 代理" },
                ]}
              />
            </FormField>
            {proxyType !== "none" && (
              <>
                <div style={{ display: "flex", gap: 12 }}>
                  <div style={{ flex: 2 }}>
                    <FormField label="代理主机" required>
                      <Input
                        value={proxyHost}
                        onChange={(v) => {
                          setProxyHost(v);
                          clearFieldError("proxyHost");
                        }}
                        placeholder="127.0.0.1"
                        errorKey="proxyHost"
                        error={fieldErrors.proxyHost}
                        shakeNonce={shakeNonce}
                        registerField={registerField}
                      />
                    </FormField>
                  </div>
                  <div style={{ flex: 1 }}>
                    <FormField label="端口" required>
                      <Input
                        value={proxyPort}
                        onChange={(v) => {
                          setProxyPort(v);
                          clearFieldError("proxyPort");
                        }}
                        placeholder={proxyType === "http" ? "8080" : "1080"}
                        errorKey="proxyPort"
                        error={fieldErrors.proxyPort}
                        shakeNonce={shakeNonce}
                        registerField={registerField}
                      />
                    </FormField>
                  </div>
                </div>
                <FormField label="代理用户名（可选）">
                  <Input
                    value={proxyUsername}
                    onChange={setProxyUsername}
                    placeholder="anonymous"
                  />
                </FormField>
                <FormField label="代理密码（可选）">
                  <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                    <div style={{ flex: 1 }}>
                      <Input
                        value={proxyPassword}
                        onChange={setProxyPassword}
                        type={showProxyPassword ? "text" : "password"}
                        placeholder={config?.proxy_type && config.proxy_type !== "none" ? "留空保持不变" : "••••••"}
                      />
                    </div>
                    {config?.proxy_type && config.proxy_type !== "none" && (
                      <button
                        type="button"
                        onClick={() => setPasswordVerifyTarget("proxy")}
                        title={showProxyPassword ? "隐藏密码" : "查看密码"}
                        style={{
                          padding: "10px 12px",
                          background: "var(--bg-surface)",
                          border: "1px solid var(--border-default)",
                          borderRadius: "var(--radius-md)",
                          fontSize: 14,
                          cursor: "pointer",
                          color: showProxyPassword ? "var(--success)" : "var(--text-tertiary)",
                          transition: "all var(--duration-fast) var(--ease-in-out)",
                        }}
                      >
                        {showProxyPassword ? "󰈉" : "󰈈"}
                      </button>
                    )}
                  </div>
                </FormField>
                {connType === "ftp" && ftpTls !== "none" && (
                  <div style={{
                    padding: "10px 12px",
                    background: "var(--warning-muted)",
                    border: "1px solid var(--warning)",
                    borderRadius: "var(--radius-md)",
                    fontSize: 11,
                    color: "var(--warning)",
                    lineHeight: 1.5,
                  }}>
                    ⚠ 注意：FTPS（TLS）当前版本暂不支持，连接时会报错。如需走代理，请把 TLS 模式切回「不加密」。
                  </div>
                )}
              </>
            )}
          </FieldGroup>
          )}

          <FieldGroup label="分组">
            <FormField label="选择或输入分组路径">
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {folders.length > 0 && (
                  <select
                    value={groupPath}
                    onChange={(e) => setGroupPath(e.target.value)}
                    style={{
                      width: "100%",
                      background: "var(--bg-input)",
                      border: "1px solid var(--border-default)",
                      borderRadius: "var(--radius-md)",
                      padding: "10px 12px",
                      color: "var(--text-primary)",
                      fontSize: 13,
                      outline: "none",
                      cursor: "pointer",
                      transition: "border-color var(--duration-fast) var(--ease-in-out)",
                    }}
                    onFocus={(e) => {
                      e.currentTarget.style.borderColor = "var(--accent-primary)";
                    }}
                    onBlur={(e) => {
                      e.currentTarget.style.borderColor = "var(--border-default)";
                    }}
                  >
                    <option value="">根目录 (/)</option>
                    {folders.map((f) => {
                      const display = f.startsWith("/") ? f.slice(1) : f;
                      return (
                        <option key={f} value={display}>
                          {display}
                        </option>
                      );
                    })}
                  </select>
                )}
                <Input
                  value={groupPath}
                  onChange={setGroupPath}
                  placeholder={folders.length > 0 ? "或手动输入新分组路径" : "生产/web"}
                />
                {folders.length === 0 && (
                  <div style={{
                    fontSize: 11,
                    color: "var(--text-muted)",
                    padding: "6px 10px",
                    background: "var(--bg-surface)",
                    borderRadius: "var(--radius-sm)",
                  }}>
                    💡 提示：先在左侧创建文件夹，这里就可以快速选择了
                  </div>
                )}
              </div>
            </FormField>
          </FieldGroup>

          {(connType === "ssh" || connType === "local") && (
            <FieldGroup label="终端">
              <FormField label="字体（可选）">
                <FontField
                  value={terminalFont}
                  onChange={setTerminalFont}
                  placeholder="留空使用全局字体"
                />
              </FormField>
              <div style={{ fontSize: 11, color: "var(--text-muted)", lineHeight: 1.5 }}>
                为该连接单独指定终端字体；留空则使用设置中的全局字体。
              </div>
            </FieldGroup>
          )}
        </div>

        {/* Footer */}
        <div
          style={{
            padding: "16px 24px",
            borderTop: "1px solid var(--border-subtle)",
            display: "flex",
            flexDirection: "column",
            gap: 10,
            background: "var(--bg-surface)",
            borderRadius: "0 0 var(--radius-xl) var(--radius-xl)",
            // Sticky so Save/Cancel/Test stay visible without scrolling to
            // the bottom — the card above is the scroll container (overflowY auto).
            position: "sticky",
            bottom: 0,
            flexShrink: 0,
            zIndex: 5,
          }}
        >
          {testResult && (
            <div
              style={{
                fontSize: 12,
                lineHeight: 1.5,
                color: testResult.kind === "ok" ? "var(--success)" : "var(--error)",
                background:
                  testResult.kind === "ok"
                    ? "var(--success-muted)"
                    : "var(--error-muted)",
                border: `1px solid ${testResult.kind === "ok" ? "var(--success)" : "var(--error)"}`,
                borderRadius: "var(--radius-md)",
                padding: "8px 12px",
                wordBreak: "break-word",
              }}
            >
              {testResult.kind === "ok" ? "✓ " : "✗ "}
              {testResult.text}
            </div>
          )}
          <div style={{ display: "flex", justifyContent: "flex-end", gap: 10 }}>
            <button
              onClick={handleTest}
              disabled={testing || saving}
              title="验证当前配置能否连通（不保存）"
              style={{
                padding: "10px 20px",
                background: "var(--bg-surface)",
                color: "var(--text-secondary)",
                border: "1px solid var(--border-default)",
                borderRadius: "var(--radius-md)",
                fontSize: 13,
                cursor: testing || saving ? "default" : "pointer",
                opacity: testing ? 0.7 : 1,
                transition: "all var(--duration-fast) var(--ease-in-out)",
                marginRight: "auto",
              }}
              onMouseEnter={(e) => {
                if (!testing && !saving) {
                  e.currentTarget.style.background = "var(--bg-surface-hover)";
                  e.currentTarget.style.borderColor = "var(--border-emphasis)";
                }
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "var(--bg-surface)";
                e.currentTarget.style.borderColor = "var(--border-default)";
              }}
            >
              {testing ? "测试中..." : "测试"}
            </button>
            <button
              onClick={onClose}
              style={{
                padding: "10px 20px",
                background: "transparent",
                color: "var(--text-secondary)",
                border: "1px solid var(--border-default)",
                borderRadius: "var(--radius-md)",
                fontSize: 13,
                cursor: "pointer",
                transition: "all var(--duration-fast) var(--ease-in-out)",
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = "var(--bg-surface-hover)";
                e.currentTarget.style.borderColor = "var(--border-emphasis)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "transparent";
                e.currentTarget.style.borderColor = "var(--border-default)";
              }}
            >
              取消
            </button>
            <button
              onClick={handleSave}
              disabled={saving || testing}
              style={{
                padding: "10px 28px",
                background: "var(--accent-primary)",
                color: "white",
                border: "none",
                borderRadius: "var(--radius-md)",
                fontSize: 13,
                fontWeight: 600,
                cursor: saving ? "wait" : "pointer",
                opacity: saving ? 0.7 : 1,
                transition: "all var(--duration-fast) var(--ease-in-out)",
                boxShadow: "var(--shadow-glow)",
              }}
              onMouseEnter={(e) => {
                if (!saving) {
                  e.currentTarget.style.background = "var(--accent-primary-hover)";
                  e.currentTarget.style.transform = "translateY(-1px)";
                }
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "var(--accent-primary)";
                e.currentTarget.style.transform = "translateY(0)";
              }}
            >
              {saving ? "保存中..." : "保存"}
            </button>
          </div>
        </div>
      </div>

      {passwordVerifyTarget && (
        <PasswordVerifyDialog
          onSuccess={async () => {
            if (passwordVerifyTarget === "password" && config?.id) {
              try {
                const pw = await getConnectionPassword(config.id);
                if (pw) {
                  setPassword(pw);
                  setShowPassword(true);
                }
              } catch {
                alert("获取密码失败");
              }
            } else if (passwordVerifyTarget === "proxy" && config?.id) {
              try {
                const pw = await getConnectionProxyPassword(config.id);
                if (pw) {
                  setProxyPassword(pw);
                  setShowProxyPassword(true);
                }
              } catch {
                alert("获取代理密码失败");
              }
            }
            setPasswordVerifyTarget(null);
          }}
          onClose={() => setPasswordVerifyTarget(null)}
        />
      )}
    </div>
  );
}

function TypeSelector({
  value,
  onChange,
  disabled,
}: {
  value: ConnType;
  onChange: (v: ConnType) => void;
  disabled?: boolean;
}) {
  return (
    <div style={{ display: "flex", gap: 8, opacity: disabled ? 0.5 : 1 }}>
      {TYPE_OPTIONS.map((opt) => {
        const active = value === opt.value;
        return (
          <button
            key={opt.value}
            disabled={disabled}
            onClick={() => onChange(opt.value)}
            style={{
              flex: 1,
              background: active ? "var(--accent-primary-muted)" : "var(--bg-surface)",
              color: active ? "var(--accent-primary)" : "var(--text-secondary)",
              border: active ? "1px solid var(--border-accent)" : "1px solid var(--border-default)",
              borderRadius: "var(--radius-lg)",
              padding: "14px 12px",
              fontSize: 13,
              fontWeight: active ? 600 : 500,
              cursor: disabled ? "not-allowed" : "pointer",
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 8,
              transition: "all var(--duration-normal) var(--ease-out-expo)",
              boxShadow: active ? "var(--shadow-glow)" : "none",
            }}
          >
            <ConnIcon connType={opt.value} size={24} style={{ color: active ? "var(--accent-primary)" : undefined }} />
            <span>{opt.label}</span>
          </button>
        );
      })}
    </div>
  );
}

function FieldGroup({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div
      style={{
        background: "var(--bg-surface)",
        border: "1px solid var(--border-default)",
        borderRadius: "var(--radius-lg)",
        padding: "16px",
        display: "flex",
        flexDirection: "column",
        gap: 12,
      }}
    >
      <div
        style={{
          fontSize: 10,
          fontWeight: 700,
          color: "var(--text-tertiary)",
          textTransform: "uppercase",
          letterSpacing: "0.1em",
        }}
      >
        {label}
      </div>
      {children}
    </div>
  );
}

function FormField({ label, required, children }: { label: string; required?: boolean; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <label style={{ fontSize: 12, color: "var(--text-secondary)", fontWeight: 500 }}>
        {label}
        {required && <span style={{ color: "var(--error)", marginLeft: 3, fontWeight: 700 }}>*</span>}
      </label>
      {children}
    </div>
  );
}

function Input({
  value,
  onChange,
  placeholder,
  type,
  autoFocus,
  errorKey,
  error,
  shakeNonce,
  registerField,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: string;
  autoFocus?: boolean;
  /** Stable id shared with validate() so this input can be flagged + focused. */
  errorKey?: string;
  /** When true the input renders the red error border/glow. */
  error?: boolean;
  /** Bumped on each Save validation failure; when it changes while `error` is
   * set, the shake animation replays. */
  shakeNonce?: number;
  registerField?: (key: string, el: HTMLElement | null) => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const errStyle = error
    ? { borderColor: "var(--error)", boxShadow: "0 0 0 3px var(--error-muted)" }
    : { borderColor: "var(--border-default)", boxShadow: "none" };

  // Register the DOM node so handleSave can focus the first invalid field.
  useEffect(() => {
    if (errorKey) registerField?.(errorKey, inputRef.current);
  });

  // Replay the shake whenever a new validation failure (shakeNonce bump)
  // lands on a field that's currently flagged. Toggling the class off→on
  // restarts the CSS animation without remounting (which would lose focus).
  useEffect(() => {
    if (!error || !shakeNonce || !inputRef.current) return;
    const el = inputRef.current;
    el.classList.remove("field-error-shake");
    // force reflow so the browser registers the class removal
    void el.offsetWidth;
    el.classList.add("field-error-shake");
  }, [error, shakeNonce]);

  return (
    <input
      ref={inputRef}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      type={type || "text"}
      autoFocus={autoFocus}
      onFocus={(e) => {
        // Error inputs keep a red focus ring so they stay visually distinct.
        const accentBorder = error ? "var(--error)" : "var(--accent-primary)";
        const accentGlow = error ? "var(--error-muted)" : "var(--accent-primary-muted)";
        e.currentTarget.style.borderColor = accentBorder;
        e.currentTarget.style.boxShadow = `0 0 0 3px ${accentGlow}`;
      }}
      onBlur={(e) => {
        e.currentTarget.style.borderColor = errStyle.borderColor;
        e.currentTarget.style.boxShadow = errStyle.boxShadow;
      }}
      style={{
        width: "100%",
        background: "var(--bg-input)",
        border: `1px solid ${errStyle.borderColor}`,
        boxShadow: errStyle.boxShadow,
        borderRadius: "var(--radius-md)",
        padding: "10px 12px",
        color: "var(--text-primary)",
        fontSize: 13,
        outline: "none",
        transition: "border-color var(--duration-fast) var(--ease-in-out), box-shadow var(--duration-fast) var(--ease-in-out)",
      }}
    />
  );
}

function Select({
  value,
  onChange,
  options,
}: {
  value: string;
  onChange: (v: string) => void;
  options: { value: string; label: string }[];
}) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      style={{
        width: "100%",
        background: "var(--bg-input)",
        border: "1px solid var(--border-default)",
        borderRadius: "var(--radius-md)",
        padding: "10px 12px",
        color: "var(--text-primary)",
        fontSize: 13,
        outline: "none",
        cursor: "pointer",
        transition: "border-color var(--duration-fast) var(--ease-in-out)",
      }}
      onFocus={(e) => {
        e.currentTarget.style.borderColor = "var(--accent-primary)";
      }}
      onBlur={(e) => {
        e.currentTarget.style.borderColor = "var(--border-default)";
      }}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

function KeyPicker({
  pem,
  fileName,
  hadExisting,
  onPick,
  onClear,
}: {
  pem: string | undefined;
  fileName: string | undefined;
  hadExisting: boolean;
  onPick: (content: string, name: string) => void;
  onClear: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const hasNew = !!pem;
  const hasExisting = !hasNew && hadExisting;

  async function pick() {
    setErr(null);
    setBusy(true);
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
      const name = selected.split(/[\\/]/).pop() || "key";
      onPick(content, name);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <button
          type="button"
          onClick={pick}
          disabled={busy}
          style={{
            padding: "8px 14px",
            background: "var(--accent-primary)",
            color: "white",
            border: "none",
            borderRadius: "var(--radius-md)",
            fontSize: 12,
            fontWeight: 600,
            cursor: busy ? "wait" : "pointer",
            opacity: busy ? 0.7 : 1,
            transition: "all var(--duration-fast) var(--ease-in-out)",
          }}
        >
          {busy ? "读取中..." : hasExisting ? "替换私钥" : "󰈗 选择私钥文件"}
        </button>
        {(hasNew || hasExisting) && (
          <button
            type="button"
            onClick={onClear}
            title="清除"
            style={{
              padding: "8px 12px",
              background: "transparent",
              color: "var(--text-tertiary)",
              border: "1px solid var(--border-default)",
              borderRadius: "var(--radius-md)",
              fontSize: 12,
              cursor: "pointer",
              transition: "all var(--duration-fast) var(--ease-in-out)",
            }}
          >
            ✕
          </button>
        )}
        <span
          style={{
            fontSize: 11,
            color: hasNew || hasExisting ? "var(--success)" : "var(--text-muted)",
            flex: 1,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {hasNew
            ? `✓ 已导入：${fileName}`
            : hasExisting
              ? "✓ 已加密存储"
              : "未选择"}
        </span>
      </div>
      {err && (
        <div style={{ marginTop: 8, fontSize: 11, color: "var(--error)" }}>{err}</div>
      )}
      <div style={{
        marginTop: 8,
        fontSize: 10,
        color: "var(--text-muted)",
        lineHeight: 1.5,
        padding: "8px 10px",
        background: "var(--bg-surface)",
        borderRadius: "var(--radius-sm)",
      }}>
        🔒 私钥将以主密码加密后存入本地数据库，原文件不会被修改。
      </div>
    </div>
  );
}
