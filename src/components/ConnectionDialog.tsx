import { useState, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { saveConnection } from "../api";
import type { ConnectionConfig, ConnType, FtpTls } from "../api";

interface Props {
  config: ConnectionConfig | null;
  onClose: () => void;
  onSave: () => void;
  initialConnType?: ConnType;
}

const TYPE_OPTIONS: { value: ConnType; label: string; icon: string; defaultPort: number }[] = [
  { value: "ssh", label: "SSH", icon: "🖥", defaultPort: 22 },
  { value: "sftp", label: "SFTP", icon: "📁", defaultPort: 22 },
  { value: "ftp", label: "FTP", icon: "📤", defaultPort: 21 },
];

export function ConnectionDialog({ config, onClose, onSave, initialConnType }: Props) {
  const [connType, setConnType] = useState<ConnType>(
    (config?.conn_type as ConnType) || initialConnType || "ssh"
  );
  const [name, setName] = useState(config?.name || "");
  const [host, setHost] = useState(config?.host || "");
  const [port, setPort] = useState(String(config?.port || (connType === "ftp" ? 21 : 22)));
  const [username, setUsername] = useState(config?.username || "");
  const [authMethod, setAuthMethod] = useState(config?.auth_method || "password");
  const [password, setPassword] = useState(config?.password || "");
  // Private key in vault: state holds PEM content (transient) + a display
  // name for the UI. When editing an existing connection that already has
  // a PEM in the vault, we know the key is present without re-fetching the
  // content (it isn't returned by get_connections to avoid leaking it
  // through IPC); show "已加密存储" and only overwrite if user picks a new file.
  const [privateKeyPem, setPrivateKeyPem] = useState<string | undefined>(config?.private_key_pem);
  const [privateKeyName, setPrivateKeyName] = useState<string | undefined>(undefined);
  const [hadExistingKey, setHadExistingKey] = useState(!!config?.private_key_pem);
  const [groupPath, setGroupPath] = useState((config?.group_path || "/").slice(1));
  const [ftpTls, setFtpTls] = useState<FtpTls>(config?.ftp_tls || "none");
  const [ftpPassive, setFtpPassive] = useState(config?.ftp_passive ?? true);
  const [saving, setSaving] = useState(false);

  // Tracks whether the user manually edited the name field. Until they do,
  // the name auto-syncs to host (per spec: "主机地址填了后连接名默认跟主机地址一致").
  const nameTouchedRef = useRef(!!config?.name);

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
    // Auto-adjust port to the type's default IF the user hasn't customized it.
    setPort((prev) => {
      const n = parseInt(prev, 10);
      const isDefault = n === 22 || n === 21 || prev === "";
      if (!isDefault) return prev;
      return String(t === "ftp" ? 21 : 22);
    });
  }

  async function handleSave() {
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
        background: "rgba(0,0,0,0.6)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 999,
        backdropFilter: "blur(2px)",
      }}
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "var(--bg-dark)",
          border: "1px solid var(--border)",
          borderRadius: 12,
          width: 440,
          maxHeight: "90vh",
          overflowY: "auto",
          boxShadow: "0 12px 40px rgba(0,0,0,0.6)",
        }}
      >
        {/* Header */}
        <div
          style={{
            padding: "16px 20px",
            borderBottom: "1px solid var(--border)",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <span style={{ fontSize: 15, fontWeight: 600, color: "var(--text-primary)" }}>
            {config ? "编辑连接" : "新建连接"}
          </span>
          <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
            {connType.toUpperCase()}
          </span>
        </div>

        {/* Type Selector */}
        <div style={{ padding: "14px 20px 4px" }}>
          <TypeSelector value={connType} onChange={handleTypeChange} disabled={!!config} />
        </div>

        {/* Form */}
        <div style={{ padding: "12px 20px 16px", display: "flex", flexDirection: "column", gap: 14 }}>
          <FieldGroup label="基本">
            <FormField label="连接名称">
              <Input
                value={name}
                onChange={handleNameChange}
                placeholder={`${connType}_${host || "host"}`}
              />
            </FormField>
            <FormField label="主机地址">
              <Input
                value={host}
                onChange={handleHostChange}
                placeholder="192.168.1.100"
                autoFocus
              />
            </FormField>
            <div style={{ display: "flex", gap: 10 }}>
              <div style={{ flex: 1 }}>
                <FormField label="端口">
                  <Input value={port} onChange={setPort} placeholder="22" />
                </FormField>
              </div>
              <div style={{ flex: 1 }}>
                <FormField label="用户名">
                  <Input value={username} onChange={setUsername} placeholder="root" />
                </FormField>
              </div>
            </div>
          </FieldGroup>

          <FieldGroup label="认证">
            <FormField label="认证方式">
              <Select
                value={authMethod}
                onChange={setAuthMethod}
                options={[
                  { value: "password", label: "密码" },
                  { value: "key", label: "私钥" },
                ]}
              />
            </FormField>
            {authMethod === "password" ? (
              <FormField label="密码">
                <Input
                  value={password}
                  onChange={setPassword}
                  type="password"
                  placeholder={config ? "留空保持不变" : "••••••"}
                  accent="warning"
                />
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

          <FieldGroup label="分组">
            <FormField label="文件夹路径（用 / 分隔）">
              <Input
                value={groupPath}
                onChange={setGroupPath}
                placeholder="生产/web"
              />
            </FormField>
          </FieldGroup>
        </div>

        {/* Footer */}
        <div
          style={{
            padding: "12px 20px",
            borderTop: "1px solid var(--border)",
            display: "flex",
            justifyContent: "flex-end",
            gap: 8,
          }}
        >
          <button onClick={onClose} style={btnSecondary}>取消</button>
          <button onClick={handleSave} disabled={saving} style={btnPrimary(saving)}>
            {saving ? "保存中..." : "保存"}
          </button>
        </div>
      </div>
    </div>
  );
}

const btnSecondary: React.CSSProperties = {
  background: "var(--bg-input)",
  color: "var(--text-secondary)",
  border: "1px solid var(--border)",
  borderRadius: 6,
  padding: "8px 20px",
  fontSize: 13,
  cursor: "pointer",
  transition: "all 0.15s",
};

const btnPrimary = (saving: boolean): React.CSSProperties => ({
  background: "var(--accent)",
  color: "var(--bg-panel)",
  border: "none",
  borderRadius: 6,
  padding: "8px 26px",
  fontSize: 13,
  fontWeight: 600,
  cursor: saving ? "wait" : "pointer",
  opacity: saving ? 0.6 : 1,
  transition: "all 0.15s",
});

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
    <div style={{ display: "flex", gap: 6, opacity: disabled ? 0.5 : 1 }}>
      {TYPE_OPTIONS.map((opt) => {
        const active = value === opt.value;
        return (
          <button
            key={opt.value}
            disabled={disabled}
            onClick={() => onChange(opt.value)}
            style={{
              flex: 1,
              background: active ? "var(--accent)" : "var(--bg-input)",
              color: active ? "var(--bg-panel)" : "var(--text-secondary)",
              border: active
                ? "1px solid var(--accent)"
                : "1px solid var(--border)",
              borderRadius: 8,
              padding: "10px 8px",
              fontSize: 13,
              fontWeight: active ? 600 : 500,
              cursor: disabled ? "not-allowed" : "pointer",
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 4,
              transition: "all 0.15s",
            }}
          >
            <span style={{ fontSize: 18 }}>{opt.icon}</span>
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
        background: "var(--bg-panel)",
        border: "1px solid var(--border)",
        borderRadius: 8,
        padding: "12px 14px",
        display: "flex",
        flexDirection: "column",
        gap: 10,
      }}
    >
      <div
        style={{
          fontSize: 10,
          fontWeight: 700,
          color: "var(--text-muted)",
          textTransform: "uppercase",
          letterSpacing: 0.8,
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
    <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <label style={{ fontSize: 12, color: "var(--text-secondary)" }}>{label}</label>
      {children}
    </div>
  );
}

function Input({
  value,
  onChange,
  placeholder,
  type,
  accent,
  autoFocus,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: string;
  accent?: "warning" | "info";
  autoFocus?: boolean;
}) {
  const accentColor =
    accent === "warning"
      ? "var(--warning)"
      : accent === "info"
      ? "var(--accent)"
      : "var(--border)";
  return (
    <input
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      type={type || "text"}
      autoFocus={autoFocus}
      onFocus={(e) => {
        e.currentTarget.style.borderColor = "var(--accent)";
        e.currentTarget.style.boxShadow = "0 0 0 2px rgba(137,180,250,0.18)";
      }}
      onBlur={(e) => {
        e.currentTarget.style.borderColor = accentColor;
        e.currentTarget.style.boxShadow = "none";
      }}
      style={{
        width: "100%",
        background: "var(--bg-input)",
        border: `1px solid ${accentColor}`,
        borderRadius: 6,
        padding: "7px 10px",
        color: "var(--text-primary)",
        fontSize: 13,
        outline: "none",
        transition: "border-color 0.15s, box-shadow 0.15s",
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
        border: "1px solid var(--border)",
        borderRadius: 6,
        padding: "7px 10px",
        color: "var(--text-primary)",
        fontSize: 13,
        outline: "none",
        cursor: "pointer",
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

/** Private-key picker: a single button that opens a file dialog, reads the
 * PEM content via the `read_text_file` Rust command, and surfaces either
 * "已导入：xxx" or "已加密存储（vault 中已有）" depending on state. The
 * clear button resets both. We never display the path on screen — once
 * imported, the path is irrelevant; only the encrypted-in-vault content
 * matters. */
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
      <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
        <button
          type="button"
          onClick={pick}
          disabled={busy}
          style={{
            padding: "6px 12px",
            background: "var(--accent)",
            color: "var(--bg-panel)",
            border: "none",
            borderRadius: 5,
            fontSize: 12,
            fontWeight: 600,
            cursor: busy ? "wait" : "pointer",
            opacity: busy ? 0.6 : 1,
          }}
        >
          {busy ? "读取中…" : hasExisting ? "替换私钥" : "📁 选择私钥文件"}
        </button>
        {(hasNew || hasExisting) && (
          <button
            type="button"
            onClick={onClear}
            title="清除"
            style={{
              padding: "6px 10px",
              background: "transparent",
              color: "var(--text-muted)",
              border: "1px solid var(--border)",
              borderRadius: 5,
              fontSize: 12,
              cursor: "pointer",
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
              ? "✓ 已加密存储（vault 中已有）"
              : "未选择"}
        </span>
      </div>
      {err && (
        <div style={{ marginTop: 6, fontSize: 11, color: "var(--error)" }}>{err}</div>
      )}
      <div style={{ marginTop: 6, fontSize: 10, color: "var(--text-muted)", lineHeight: 1.4 }}>
        私钥将以主密码加密后存入本地数据库，原文件不会被修改。
      </div>
    </div>
  );
}
