import { useState, useRef, useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { saveConnection, getConnectionPassword, getConnectionProxyPassword } from "../api";
import type { ConnectionConfig, ConnType, FtpTls, ProxyType } from "../api";
import { PasswordVerifyDialog } from "./PasswordVerifyDialog";

interface Props {
  config: ConnectionConfig | null;
  onClose: () => void;
  onSave: () => void;
  initialConnType?: ConnType;
  initialFolderPath?: string;
  folders?: string[];
}

const TYPE_OPTIONS: { value: ConnType; label: string; icon: string; defaultPort: number }[] = [
  { value: "ssh", label: "SSH", icon: "🖥️", defaultPort: 22 },
  { value: "sftp", label: "SFTP", icon: "📁", defaultPort: 22 },
  { value: "ftp", label: "FTP", icon: "📤", defaultPort: 21 },
  { value: "local", label: "本地", icon: "💻", defaultPort: 0 },
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
  const [saving, setSaving] = useState(false);
  const [showPassword, setShowPassword] = useState(false);
  const [showProxyPassword, setShowProxyPassword] = useState(false);
  const [passwordVerifyTarget, setPasswordVerifyTarget] = useState<"password" | "proxy" | null>(null);
  const nameTouchedRef = useRef(!!config?.name);

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
    setHost(v);
    if (!nameTouchedRef.current) setName(v);
  }

  function handleNameChange(v: string) {
    nameTouchedRef.current = true;
    setName(v);
  }

  function handleTypeChange(t: ConnType) {
    setConnType(t);
    setPort((prev) => {
      const n = parseInt(prev, 10);
      const isDefault = n === 22 || n === 21 || prev === "";
      if (!isDefault) return prev;
      if (t === "ftp") return "21";
      if (t === "local") return prev; // local terminals have no port
      return "22";
    });
  }

  async function handleSave() {
    // Local terminal — no host/port/auth, just a shell to spawn.
    if (connType === "local") {
      if (!name.trim()) {
        alert("请填写连接名称");
        return;
      }
      if (!shellPath.trim()) {
        alert("请填写启动 shell 路径");
        return;
      }
      setSaving(true);
      try {
        const trimmedGroup = groupPath
          .trim()
          .replace(/^\/+|\/+$/g, "")
          .replace(/\/+/g, "/");
        const conn: ConnectionConfig = {
          id: config?.id || crypto.randomUUID(),
          name: name.trim(),
          host: "",
          port: 0,
          username: "",
          auth_method: "password", // unused for local, struct requires a value
          conn_type: "local",
          group_path: trimmedGroup ? `/${trimmedGroup}` : "/",
          shell_path: shellPath.trim(),
          shell_args: shellArgs.trim() || undefined,
          init_command: initCommand.trim() || undefined,
          created_at: config?.created_at || new Date().toISOString(),
        };
        await saveConnection(conn);
        onSave();
      } catch (e) {
        alert(`保存失败: ${e}`);
      } finally {
        setSaving(false);
      }
      return;
    }

    if (!name.trim() || !host.trim() || !username.trim()) {
      alert("请填写必要字段");
      return;
    }

    const portNum = parseInt(port, 10);
    if (!Number.isInteger(portNum) || portNum < 1 || portNum > 65535) {
      alert("端口号必须为 1-65535 之间的整数");
      return;
    }

    const isEditing = !!config?.id;
    if (authMethod === "password" && !isEditing && !password) {
      alert("请填写密码");
      return;
    }

    let proxyPortNum: number | undefined;
    if (proxyType !== "none") {
      if (!proxyHost.trim()) {
        alert("代理主机地址不能为空");
        return;
      }
      const pp = parseInt(proxyPort, 10);
      if (!Number.isInteger(pp) || pp < 1 || pp > 65535) {
        alert("代理端口必须为 1-65535 之间的整数");
        return;
      }
      proxyPortNum = pp;
    }

    setSaving(true);
    try {
      const passwordToSend =
        authMethod === "password" && password ? password : undefined;
      const trimmedGroup = groupPath
        .trim()
        .replace(/^\/+|\/+$/g, "")
        .replace(/\/+/g, "/");
      const conn: ConnectionConfig = {
        id: config?.id || crypto.randomUUID(),
        name: name.trim(),
        host: host.trim(),
        port: portNum,
        username: username.trim(),
        auth_method: authMethod,
        password: passwordToSend,
        private_key_pem: authMethod === "key" ? privateKeyPem : undefined,
        conn_type: connType,
        group_path: trimmedGroup ? `/${trimmedGroup}` : "/",
        ftp_tls: connType === "ftp" ? ftpTls : "none",
        ftp_passive: connType === "ftp" ? ftpPassive : true,
        proxy_type: proxyType,
        proxy_host: proxyType !== "none" ? proxyHost.trim() : undefined,
        proxy_port: proxyPortNum,
        proxy_username:
          proxyType !== "none" && proxyUsername.trim()
            ? proxyUsername.trim()
            : undefined,
        proxy_password:
          proxyType !== "none" && proxyPassword ? proxyPassword : undefined,
        created_at: config?.created_at || new Date().toISOString(),
      };
      await saveConnection(conn);
      onSave();
    } catch (e) {
      alert(`保存失败: ${e}`);
    } finally {
      setSaving(false);
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
            <FormField label="连接名称">
              <Input
                value={name}
                onChange={handleNameChange}
                placeholder={connType === "local" ? "本地终端" : `${connType}_${host || "server"}`}
              />
            </FormField>
            {connType !== "local" && (
              <>
                <div style={{ display: "flex", gap: 12 }}>
                  <div style={{ flex: 2 }}>
                    <FormField label="主机地址">
                      <Input
                        value={host}
                        onChange={handleHostChange}
                        placeholder="192.168.1.100"
                        autoFocus
                      />
                    </FormField>
                  </div>
                  <div style={{ flex: 1 }}>
                    <FormField label="端口">
                      <Input value={port} onChange={setPort} placeholder="22" />
                    </FormField>
                  </div>
                </div>
                <FormField label="用户名">
                  <Input value={username} onChange={setUsername} placeholder="root" />
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
              <FormField label="可执行文件路径">
                <Input
                  value={shellPath}
                  onChange={setShellPath}
                  placeholder="pwsh.exe 或完整路径"
                  autoFocus
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
                💡 本地终端在本机启动一个 shell（PowerShell / CMD / WSL 等），等同打开一个本地命令行窗口。「启动命令」会在 shell 就绪后自动执行一次（如打开即跑 claude）。
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
              <FormField label="密码">
                <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                  <div style={{ flex: 1 }}>
                    <Input
                      value={password}
                      onChange={setPassword}
                      type={showPassword ? "text" : "password"}
                      placeholder={config ? "留空保持不变" : "••••••"}
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
                    <FormField label="代理主机">
                      <Input
                        value={proxyHost}
                        onChange={setProxyHost}
                        placeholder="127.0.0.1"
                      />
                    </FormField>
                  </div>
                  <div style={{ flex: 1 }}>
                    <FormField label="端口">
                      <Input
                        value={proxyPort}
                        onChange={setProxyPort}
                        placeholder={proxyType === "http" ? "8080" : "1080"}
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
        </div>

        {/* Footer */}
        <div
          style={{
            padding: "16px 24px",
            borderTop: "1px solid var(--border-subtle)",
            display: "flex",
            justifyContent: "flex-end",
            gap: 10,
            background: "var(--bg-surface)",
            borderRadius: "0 0 var(--radius-xl) var(--radius-xl)",
          }}
        >
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
            disabled={saving}
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
            <span style={{ fontSize: 22 }}>{opt.icon}</span>
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

function FormField({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <label style={{ fontSize: 12, color: "var(--text-secondary)", fontWeight: 500 }}>{label}</label>
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
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: string;
  autoFocus?: boolean;
}) {
  return (
    <input
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      type={type || "text"}
      autoFocus={autoFocus}
      onFocus={(e) => {
        e.currentTarget.style.borderColor = "var(--accent-primary)";
        e.currentTarget.style.boxShadow = "0 0 0 3px var(--accent-primary-muted)";
      }}
      onBlur={(e) => {
        e.currentTarget.style.borderColor = "var(--border-default)";
        e.currentTarget.style.boxShadow = "none";
      }}
      style={{
        width: "100%",
        background: "var(--bg-input)",
        border: "1px solid var(--border-default)",
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
      const content: string = await invoke("read_text_file", { path: selected });
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
