import { useState, useRef, useEffect } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  changeMasterPassword,
  exportConnections,
  importConnections,
  listBackups,
  rollbackBackup,
  getAppVersion,
  getPreviousVersion,
} from "../api";
import type { BackupInfo } from "../api";
import { useTheme } from "../hooks/useTheme";

interface Props {
  onClose: () => void;
  onRefresh: () => void;
  connectionCount: number;
}

const MIN_LEN = 6;

function strengthHint(p: string): { label: string; color: string; width: string } {
  let classes = 0;
  if (/[a-z]/.test(p)) classes++;
  if (/[A-Z]/.test(p)) classes++;
  if (/[0-9]/.test(p)) classes++;
  if (/[^A-Za-z0-9]/.test(p)) classes++;
  if (p.length < MIN_LEN) return { label: "至少 6 个字符", color: "var(--error)", width: "0%" };
  if (p.length < 8) return { label: "简单", color: "var(--warning)", width: "25%" };
  if (classes <= 2) return { label: "弱", color: "var(--error)", width: "50%" };
  if (classes === 3) return { label: "中等", color: "var(--warning)", width: "75%" };
  return { label: "强", color: "var(--success)", width: "100%" };
}

export function SettingsPanel({ onClose, onRefresh, connectionCount }: Props) {
  const [oldPass, setOldPass] = useState("");
  const [newPass, setNewPass] = useState("");
  const [confirmPass, setConfirmPass] = useState("");
  const [passwordBusy, setPasswordBusy] = useState(false);
  const [passwordErr, setPasswordErr] = useState<string | null>(null);
  const [passwordSuccess, setPasswordSuccess] = useState(false);
  const [exportBusy, setExportBusy] = useState(false);
  const [importBusy, setImportBusy] = useState(false);
  const [ioErr, setIoErr] = useState<string | null>(null);
  const [exportPass, setExportPass] = useState("");
  const [showExportDialog, setShowExportDialog] = useState(false);
  const [importPass, setImportPass] = useState("");
  const [importPath, setImportPath] = useState<string | null>(null);
  const [showImportDialog, setShowImportDialog] = useState(false);
  const [appVersion, setAppVersion] = useState<string>("");
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [previousVersion, setPreviousVersion] = useState<string | null>(null);
  const [rollbackBusy, setRollbackBusy] = useState(false);
  const [rollbackResult, setRollbackResult] = useState<string | null>(null);
  const oldPassRef = useRef<HTMLInputElement>(null);
  const { theme, toggleTheme } = useTheme();

  useEffect(() => {
    getAppVersion().then(setAppVersion).catch(() => {});
    listBackups().then(setBackups).catch(() => {});
    getPreviousVersion().then(setPreviousVersion).catch(() => {});
  }, []);

  const hint = strengthHint(newPass);
  const mismatch = confirmPass.length > 0 && confirmPass !== newPass;
  const canChangePassword =
    oldPass.length >= MIN_LEN &&
    newPass.length >= MIN_LEN &&
    confirmPass === newPass &&
    !passwordBusy;

  async function handleChangePassword() {
    if (!canChangePassword) return;
    setPasswordBusy(true);
    setPasswordErr(null);
    setPasswordSuccess(false);
    try {
      await changeMasterPassword(oldPass, newPass);
      setPasswordSuccess(true);
      setOldPass("");
      setNewPass("");
      setConfirmPass("");
    } catch (e) {
      setPasswordErr(String(e));
    } finally {
      setPasswordBusy(false);
    }
  }

  async function startExport() {
    if (connectionCount === 0) {
      setIoErr("暂无连接可导出");
      return;
    }
    setExportPass("");
    setShowExportDialog(true);
  }

  async function handleExport() {
    if (exportPass.length < MIN_LEN) {
      setIoErr("加密密码至少 6 个字符");
      return;
    }
    const path = await save({
      defaultPath: `myshell-export-${new Date().toISOString().slice(0, 10)}.json`,
      filters: [{ name: "MyShell Encrypted Dump", extensions: ["json"] }],
    });
    if (!path) return;

    setExportBusy(true);
    setIoErr(null);
    try {
      const n = await exportConnections(exportPass, path);
      setShowExportDialog(false);
      setExportPass("");
      alert(`已导出 ${n} 个连接到\n${path}`);
    } catch (e) {
      setIoErr(String(e));
    } finally {
      setExportBusy(false);
    }
  }

  async function startImport() {
    const selected = await open({
      multiple: false,
      filters: [{ name: "MyShell Encrypted Dump", extensions: ["json"] }],
    });
    if (!selected || Array.isArray(selected)) return;
    setImportPath(selected);
    setImportPass("");
    setShowImportDialog(true);
  }

  async function handleImport() {
    if (!importPath) return;
    if (importPass.length < MIN_LEN) {
      setIoErr("解密密码至少 6 个字符");
      return;
    }

    setImportBusy(true);
    setIoErr(null);
    try {
      const n = await importConnections(importPass, importPath);
      setShowImportDialog(false);
      setImportPath(null);
      setImportPass("");
      alert(`已导入 ${n} 个连接`);
      onRefresh();
    } catch (e) {
      setIoErr(String(e));
    } finally {
      setImportBusy(false);
    }
  }

  async function handleRollback(version: string) {
    if (!window.confirm(`确定要回退到版本 ${version} 吗？\n\n当前配置将被覆盖，回退后需要重启应用。`)) {
      return;
    }
    setRollbackBusy(true);
    setRollbackResult(null);
    try {
      const result = await rollbackBackup(version);
      setRollbackResult(result);
      const updated = await listBackups();
      setBackups(updated);
    } catch (e) {
      setRollbackResult(`回退失败: ${e}`);
    } finally {
      setRollbackBusy(false);
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
        zIndex: 2000,
        backdropFilter: "blur(8px)",
      }}
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="animate-scale-in"
        style={{
          background: "var(--bg-elevated)",
          border: "1px solid var(--border-emphasis)",
          borderRadius: "var(--radius-xl)",
          width: 520,
          maxHeight: "85vh",
          overflow: "hidden",
          boxShadow: "var(--shadow-xl)",
          display: "flex",
          flexDirection: "column",
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
            flexShrink: 0,
          }}
        >
          <div>
            <div style={{ fontSize: 16, fontWeight: 600, color: "var(--text-primary)" }}>
              设置
            </div>
            <div style={{ fontSize: 12, color: "var(--text-tertiary)", marginTop: 2 }}>
              管理应用配置与安全设置
            </div>
          </div>
          <button
            onClick={onClose}
            style={{
              width: 32,
              height: 32,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              background: "transparent",
              border: "none",
              color: "var(--text-tertiary)",
              fontSize: 18,
              cursor: "pointer",
              borderRadius: "var(--radius-md)",
              transition: "all var(--duration-fast) var(--ease-in-out)",
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = "var(--bg-surface-hover)";
              e.currentTarget.style.color = "var(--text-primary)";
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "transparent";
              e.currentTarget.style.color = "var(--text-tertiary)";
            }}
          >
            ✕
          </button>
        </div>

        {/* Content */}
        <div style={{ flex: 1, overflowY: "auto", padding: "20px 24px" }}>
          {/* Theme Section */}
          <Section title="外观设置">
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                padding: "14px 16px",
                background: "var(--bg-surface)",
                border: "1px solid var(--border-default)",
                borderRadius: "var(--radius-lg)",
              }}
            >
              <div>
                <div style={{ fontSize: 13, fontWeight: 500, color: "var(--text-primary)" }}>
                  主题模式
                </div>
                <div style={{ fontSize: 12, color: "var(--text-tertiary)", marginTop: 2 }}>
                  当前：{theme === "dark" ? "暗色模式" : "亮色模式"}
                </div>
              </div>
              <button
                onClick={toggleTheme}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  padding: "8px 14px",
                  background: "var(--bg-surface-hover)",
                  border: "1px solid var(--border-default)",
                  borderRadius: "var(--radius-md)",
                  fontSize: 12,
                  fontWeight: 500,
                  color: "var(--text-primary)",
                  cursor: "pointer",
                  transition: "all var(--duration-fast) var(--ease-in-out)",
                }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.background = "var(--accent-primary-muted)";
                  e.currentTarget.style.borderColor = "var(--border-accent)";
                  e.currentTarget.style.color = "var(--accent-primary)";
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = "var(--bg-surface-hover)";
                  e.currentTarget.style.borderColor = "var(--border-default)";
                  e.currentTarget.style.color = "var(--text-primary)";
                }}
              >
                <span style={{ fontSize: 14 }}>{theme === "dark" ? "󰖨" : "󰖙"}</span>
                切换到{theme === "dark" ? "亮色" : "暗色"}模式
              </button>
            </div>
          </Section>

          <Divider />

          {/* Change Password Section */}
          <Section title="修改登录密码">
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14 }}>
              修改密码后需要使用新密码解锁应用
            </div>

            <Field label="原密码">
              <Input
                inputRef={oldPassRef}
                type="password"
                value={oldPass}
                onChange={setOldPass}
                placeholder="输入原密码"
              />
            </Field>

            <Field label="新密码">
              <Input
                type="password"
                value={newPass}
                onChange={setNewPass}
                placeholder="至少 6 个字符"
              />
              {newPass.length > 0 && (
                <div style={{ marginTop: 8 }}>
                  <div
                    style={{
                      height: 4,
                      background: "var(--bg-surface)",
                      borderRadius: "var(--radius-full)",
                      overflow: "hidden",
                    }}
                  >
                    <div
                      style={{
                        height: "100%",
                        width: hint.width,
                        background: hint.color,
                        borderRadius: "var(--radius-full)",
                        transition: "width var(--duration-normal) var(--ease-out-expo)",
                      }}
                    />
                  </div>
                  <div style={{ marginTop: 4, fontSize: 11, color: hint.color }}>
                    强度：{hint.label}
                  </div>
                </div>
              )}
            </Field>

            <Field label="确认新密码">
              <Input
                type="password"
                value={confirmPass}
                onChange={setConfirmPass}
                placeholder="再次输入新密码"
                error={mismatch ? "两次输入不一致" : undefined}
              />
            </Field>

            {passwordErr && <ErrorBox>{passwordErr}</ErrorBox>}
            {passwordSuccess && <SuccessBox>密码修改成功</SuccessBox>}

            <button
              onClick={handleChangePassword}
              disabled={!canChangePassword}
              style={{
                marginTop: 8,
                width: "100%",
                padding: "12px 20px",
                background: canChangePassword ? "var(--accent-primary)" : "var(--bg-surface)",
                color: canChangePassword ? "white" : "var(--text-muted)",
                border: "none",
                borderRadius: "var(--radius-md)",
                fontSize: 13,
                fontWeight: 600,
                cursor: canChangePassword ? "pointer" : "not-allowed",
                opacity: canChangePassword ? 1 : 0.5,
                transition: "all var(--duration-fast) var(--ease-in-out)",
                boxShadow: canChangePassword ? "var(--shadow-glow)" : "none",
              }}
            >
              {passwordBusy ? "处理中..." : "修改密码"}
            </button>
          </Section>

          <Divider />

          {/* Export/Import Section */}
          <Section title="配置导入导出">
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14 }}>
              导出连接配置到加密文件，或从文件导入
            </div>

            {ioErr && <ErrorBox>{ioErr}</ErrorBox>}

            <div style={{ display: "flex", gap: 10 }}>
              <button
                onClick={startExport}
                disabled={exportBusy || importBusy}
                style={{
                  flex: 1,
                  padding: "12px 16px",
                  background: "var(--bg-surface)",
                  color: "var(--text-primary)",
                  border: "1px solid var(--border-default)",
                  borderRadius: "var(--radius-md)",
                  fontSize: 13,
                  fontWeight: 500,
                  cursor: exportBusy || importBusy ? "not-allowed" : "pointer",
                  opacity: exportBusy || importBusy ? 0.5 : 1,
                  transition: "all var(--duration-fast) var(--ease-in-out)",
                }}
                onMouseEnter={(e) => {
                  if (!exportBusy && !importBusy) {
                    e.currentTarget.style.background = "var(--bg-surface-hover)";
                    e.currentTarget.style.borderColor = "var(--border-emphasis)";
                  }
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = "var(--bg-surface)";
                  e.currentTarget.style.borderColor = "var(--border-default)";
                }}
              >
                󰉁 导出配置
              </button>
              <button
                onClick={startImport}
                disabled={exportBusy || importBusy}
                style={{
                  flex: 1,
                  padding: "12px 16px",
                  background: "var(--bg-surface)",
                  color: "var(--text-primary)",
                  border: "1px solid var(--border-default)",
                  borderRadius: "var(--radius-md)",
                  fontSize: 13,
                  fontWeight: 500,
                  cursor: exportBusy || importBusy ? "not-allowed" : "pointer",
                  opacity: exportBusy || importBusy ? 0.5 : 1,
                  transition: "all var(--duration-fast) var(--ease-in-out)",
                }}
                onMouseEnter={(e) => {
                  if (!exportBusy && !importBusy) {
                    e.currentTarget.style.background = "var(--bg-surface-hover)";
                    e.currentTarget.style.borderColor = "var(--border-emphasis)";
                  }
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = "var(--bg-surface)";
                  e.currentTarget.style.borderColor = "var(--border-default)";
                }}
              >
                󰉀 导入配置
              </button>
            </div>
            <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 10, textAlign: "center" }}>
              当前共有 {connectionCount} 个连接
            </div>
          </Section>

          <Divider />

          {/* Backup Section */}
          <Section title="版本备份与回退">
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14 }}>
              升级时自动备份配置，支持回退到旧版本
            </div>

            <div
              style={{
                padding: "10px 14px",
                background: "var(--bg-surface)",
                border: "1px solid var(--border-default)",
                borderRadius: "var(--radius-md)",
                marginBottom: 12,
                display: "flex",
                alignItems: "center",
                gap: 10,
              }}
            >
              <span style={{ fontSize: 16, color: "var(--accent-primary)" }}>󰀫</span>
              <div>
                <div style={{ fontSize: 11, color: "var(--text-tertiary)" }}>当前版本</div>
                <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)" }}>
                  {appVersion || "加载中..."}
                </div>
              </div>
            </div>

            {rollbackResult && (
              <div
                style={{
                  marginBottom: 12,
                  padding: "10px 14px",
                  background: rollbackResult.includes("失败") ? "var(--error-muted)" : "var(--success-muted)",
                  border: `1px solid ${rollbackResult.includes("失败") ? "var(--error)" : "var(--success)"}`,
                  borderRadius: "var(--radius-md)",
                  fontSize: 12,
                  color: rollbackResult.includes("失败") ? "var(--error)" : "var(--success)",
                }}
              >
                {rollbackResult}
              </div>
            )}

            {previousVersion && (
              <button
                onClick={() => handleRollback(previousVersion)}
                disabled={rollbackBusy}
                style={{
                  width: "100%",
                  marginBottom: 12,
                  padding: "10px 16px",
                  background: "transparent",
                  color: "var(--accent-primary)",
                  border: "1px solid var(--border-accent)",
                  borderRadius: "var(--radius-md)",
                  fontSize: 12,
                  fontWeight: 500,
                  cursor: rollbackBusy ? "not-allowed" : "pointer",
                  opacity: rollbackBusy ? 0.5 : 1,
                  transition: "all var(--duration-fast) var(--ease-in-out)",
                }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.background = "var(--accent-primary-muted)";
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = "transparent";
                }}
              >
                ⏪ 快速回退到上一版本 ({previousVersion})
              </button>
            )}

            {backups.length > 0 && (
              <div
                style={{
                  border: "1px solid var(--border-default)",
                  borderRadius: "var(--radius-lg)",
                  overflow: "hidden",
                }}
              >
                <div
                  style={{
                    padding: "10px 14px",
                    background: "var(--bg-surface)",
                    fontSize: 11,
                    color: "var(--text-tertiary)",
                    borderBottom: "1px solid var(--border-subtle)",
                  }}
                >
                  可用备份（最多保留 5 个）
                </div>
                {backups.map((backup, index) => (
                  <div
                    key={backup.version}
                    style={{
                      padding: "12px 14px",
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      borderBottom: index < backups.length - 1 ? "1px solid var(--border-subtle)" : "none",
                      transition: "background var(--duration-fast) var(--ease-in-out)",
                    }}
                    onMouseEnter={(e) => {
                      e.currentTarget.style.background = "var(--bg-surface-hover)";
                    }}
                    onMouseLeave={(e) => {
                      e.currentTarget.style.background = "transparent";
                    }}
                  >
                    <div>
                      <div style={{ fontSize: 12, fontWeight: 500, color: "var(--text-primary)" }}>
                        v{backup.version}
                        {backup.version === appVersion && (
                          <span
                            style={{
                              marginLeft: 8,
                              padding: "2px 8px",
                              background: "var(--success-muted)",
                              color: "var(--success)",
                              borderRadius: "var(--radius-full)",
                              fontSize: 10,
                              fontWeight: 600,
                            }}
                          >
                            当前
                          </span>
                        )}
                      </div>
                      <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 2 }}>
                        {backup.timestampStr} · {backup.files.length} 个文件
                      </div>
                    </div>
                    {backup.version !== appVersion && (
                      <button
                        onClick={() => handleRollback(backup.version)}
                        disabled={rollbackBusy}
                        style={{
                          padding: "6px 12px",
                          background: "transparent",
                          color: "var(--accent-primary)",
                          border: "1px solid var(--border-default)",
                          borderRadius: "var(--radius-sm)",
                          fontSize: 11,
                          cursor: rollbackBusy ? "not-allowed" : "pointer",
                          opacity: rollbackBusy ? 0.5 : 1,
                          transition: "all var(--duration-fast) var(--ease-in-out)",
                        }}
                        onMouseEnter={(e) => {
                          e.currentTarget.style.background = "var(--accent-primary-muted)";
                          e.currentTarget.style.borderColor = "var(--border-accent)";
                        }}
                        onMouseLeave={(e) => {
                          e.currentTarget.style.background = "transparent";
                          e.currentTarget.style.borderColor = "var(--border-default)";
                        }}
                      >
                        回退
                      </button>
                    )}
                  </div>
                ))}
              </div>
            )}

            {backups.length === 0 && (
              <div style={{ fontSize: 12, color: "var(--text-muted)", textAlign: "center", padding: 24 }}>
                暂无备份记录
              </div>
            )}
          </Section>
        </div>
      </div>

      {/* Export Passphrase Dialog */}
      {showExportDialog && (
        <Dialog title="加密导出" icon="󰍁" onClose={() => setShowExportDialog(false)}>
          <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14 }}>
            设置加密密码，导入时需要此密码才能解密
          </div>
          <Field label="加密密码">
            <Input
              type="password"
              value={exportPass}
              onChange={setExportPass}
              placeholder="至少 6 个字符"
              autoFocus
            />
          </Field>
          <div style={{ display: "flex", gap: 10, marginTop: 20, justifyContent: "flex-end" }}>
            <button
              onClick={() => setShowExportDialog(false)}
              style={{
                padding: "10px 18px",
                background: "transparent",
                color: "var(--text-secondary)",
                border: "1px solid var(--border-default)",
                borderRadius: "var(--radius-md)",
                fontSize: 13,
                cursor: "pointer",
                transition: "all var(--duration-fast) var(--ease-in-out)",
              }}
            >
              取消
            </button>
            <button
              onClick={handleExport}
              disabled={exportPass.length < MIN_LEN || exportBusy}
              style={{
                padding: "10px 24px",
                background: exportPass.length >= MIN_LEN && !exportBusy ? "var(--accent-primary)" : "var(--bg-surface)",
                color: exportPass.length >= MIN_LEN && !exportBusy ? "white" : "var(--text-muted)",
                border: "none",
                borderRadius: "var(--radius-md)",
                fontSize: 13,
                fontWeight: 600,
                cursor: exportPass.length >= MIN_LEN && !exportBusy ? "pointer" : "not-allowed",
                opacity: exportPass.length >= MIN_LEN && !exportBusy ? 1 : 0.5,
                transition: "all var(--duration-fast) var(--ease-in-out)",
              }}
            >
              {exportBusy ? "导出中..." : "导出"}
            </button>
          </div>
        </Dialog>
      )}

      {/* Import Passphrase Dialog */}
      {showImportDialog && (
        <Dialog title="解密导入" icon="󰍂" onClose={() => { setShowImportDialog(false); setImportPath(null); }}>
          <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14 }}>
            输入导出时设置的密码以解密
          </div>
          <Field label="解密密码">
            <Input
              type="password"
              value={importPass}
              onChange={setImportPass}
              placeholder="至少 6 个字符"
              autoFocus
            />
          </Field>
          <div style={{ display: "flex", gap: 10, marginTop: 20, justifyContent: "flex-end" }}>
            <button
              onClick={() => { setShowImportDialog(false); setImportPath(null); }}
              style={{
                padding: "10px 18px",
                background: "transparent",
                color: "var(--text-secondary)",
                border: "1px solid var(--border-default)",
                borderRadius: "var(--radius-md)",
                fontSize: 13,
                cursor: "pointer",
                transition: "all var(--duration-fast) var(--ease-in-out)",
              }}
            >
              取消
            </button>
            <button
              onClick={handleImport}
              disabled={importPass.length < MIN_LEN || importBusy}
              style={{
                padding: "10px 24px",
                background: importPass.length >= MIN_LEN && !importBusy ? "var(--accent-primary)" : "var(--bg-surface)",
                color: importPass.length >= MIN_LEN && !importBusy ? "white" : "var(--text-muted)",
                border: "none",
                borderRadius: "var(--radius-md)",
                fontSize: 13,
                fontWeight: 600,
                cursor: importPass.length >= MIN_LEN && !importBusy ? "pointer" : "not-allowed",
                opacity: importPass.length >= MIN_LEN && !importBusy ? 1 : 0.5,
                transition: "all var(--duration-fast) var(--ease-in-out)",
              }}
            >
              {importBusy ? "导入中..." : "导入"}
            </button>
          </div>
        </Dialog>
      )}
    </div>
  );
}

// Shared Components

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section style={{ marginBottom: 20 }}>
      <div
        style={{
          fontSize: 13,
          fontWeight: 600,
          color: "var(--text-primary)",
          marginBottom: 12,
          display: "flex",
          alignItems: "center",
          gap: 8,
        }}
      >
        {title}
      </div>
      {children}
    </section>
  );
}

function Divider() {
  return (
    <div
      style={{
        height: 1,
        background: "var(--border-subtle)",
        margin: "20px 0",
      }}
    />
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ marginBottom: 12 }}>
      <label style={{ display: "block", fontSize: 12, color: "var(--text-secondary)", marginBottom: 6, fontWeight: 500 }}>
        {label}
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
  inputRef,
  autoFocus,
  error,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: string;
  inputRef?: React.RefObject<HTMLInputElement>;
  autoFocus?: boolean;
  error?: string;
}) {
  return (
    <>
      <input
        ref={inputRef}
        type={type || "text"}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        autoFocus={autoFocus}
        onFocus={(e) => {
          e.currentTarget.style.borderColor = "var(--accent-primary)";
          e.currentTarget.style.boxShadow = "0 0 0 3px var(--accent-primary-muted)";
        }}
        onBlur={(e) => {
          e.currentTarget.style.borderColor = error ? "var(--error)" : "var(--border-default)";
          e.currentTarget.style.boxShadow = "none";
        }}
        style={{
          width: "100%",
          padding: "10px 12px",
          background: "var(--bg-input)",
          color: "var(--text-primary)",
          border: `1px solid ${error ? "var(--error)" : "var(--border-default)"}`,
          borderRadius: "var(--radius-md)",
          fontSize: 13,
          outline: "none",
          fontFamily: type === "password" ? "ui-monospace, monospace" : "inherit",
          transition: "border-color var(--duration-fast) var(--ease-in-out), box-shadow var(--duration-fast) var(--ease-in-out)",
        }}
      />
      {error && (
        <div style={{ marginTop: 4, fontSize: 11, color: "var(--error)" }}>{error}</div>
      )}
    </>
  );
}

function ErrorBox({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        marginBottom: 12,
        padding: "10px 14px",
        background: "var(--error-muted)",
        border: "1px solid var(--error)",
        borderRadius: "var(--radius-md)",
        fontSize: 12,
        color: "var(--error)",
        display: "flex",
        alignItems: "center",
        gap: 8,
      }}
    >
      <span style={{ fontSize: 14 }}>󰀦</span>
      {children}
    </div>
  );
}

function SuccessBox({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        marginBottom: 12,
        padding: "10px 14px",
        background: "var(--success-muted)",
        border: "1px solid var(--success)",
        borderRadius: "var(--radius-md)",
        fontSize: 12,
        color: "var(--success)",
        display: "flex",
        alignItems: "center",
        gap: 8,
      }}
    >
      <span style={{ fontSize: 14 }}>󰗠</span>
      {children}
    </div>
  );
}

function Dialog({
  title,
  icon,
  children,
  onClose,
}: {
  title: string;
  icon: string;
  children: React.ReactNode;
  onClose: () => void;
}) {
  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--bg-overlay)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 2100,
        backdropFilter: "blur(8px)",
      }}
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="animate-scale-in"
        style={{
          background: "var(--bg-elevated)",
          border: "1px solid var(--border-emphasis)",
          borderRadius: "var(--radius-xl)",
          padding: 24,
          width: 400,
          boxShadow: "var(--shadow-xl)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 8 }}>
          <span style={{ fontSize: 20, color: "var(--accent-primary)" }}>{icon}</span>
          <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text-primary)" }}>{title}</div>
        </div>
        {children}
      </div>
    </div>
  );
}
