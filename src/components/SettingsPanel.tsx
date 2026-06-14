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

interface Props {
  onClose: () => void;
  onRefresh: () => void;
  connectionCount: number;
}

const MIN_LEN = 6;

function strengthHint(p: string): { label: string; color: string } {
  let classes = 0;
  if (/[a-z]/.test(p)) classes++;
  if (/[A-Z]/.test(p)) classes++;
  if (/[0-9]/.test(p)) classes++;
  if (/[^A-Za-z0-9]/.test(p)) classes++;
  if (p.length < MIN_LEN) return { label: "至少 6 个字符", color: "var(--error)" };
  if (p.length < 8) return { label: "简单", color: "var(--warning)" };
  if (classes <= 2) return { label: "弱", color: "var(--error)" };
  if (classes === 3) return { label: "中", color: "var(--warning)" };
  return { label: "强", color: "var(--success)" };
}

export function SettingsPanel({ onClose, onRefresh, connectionCount }: Props) {
  // Change password state
  const [oldPass, setOldPass] = useState("");
  const [newPass, setNewPass] = useState("");
  const [confirmPass, setConfirmPass] = useState("");
  const [passwordBusy, setPasswordBusy] = useState(false);
  const [passwordErr, setPasswordErr] = useState<string | null>(null);
  const [passwordSuccess, setPasswordSuccess] = useState(false);

  // Export/Import state
  const [exportBusy, setExportBusy] = useState(false);
  const [importBusy, setImportBusy] = useState(false);
  const [ioErr, setIoErr] = useState<string | null>(null);

  // Export passphrase
  const [exportPass, setExportPass] = useState("");
  const [showExportDialog, setShowExportDialog] = useState(false);

  // Import passphrase
  const [importPass, setImportPass] = useState("");
  const [importPath, setImportPath] = useState<string | null>(null);
  const [showImportDialog, setShowImportDialog] = useState(false);

  // Backup state
  const [appVersion, setAppVersion] = useState<string>("");
  const [backups, setBackups] = useState<BackupInfo[]>([]);
  const [previousVersion, setPreviousVersion] = useState<string | null>(null);
  const [rollbackBusy, setRollbackBusy] = useState(false);
  const [rollbackResult, setRollbackResult] = useState<string | null>(null);

  const oldPassRef = useRef<HTMLInputElement>(null);

  // Load backup info on mount
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
      // Refresh backup list
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
        background: "rgba(0,0,0,0.55)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 2000,
      }}
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "var(--bg-panel)",
          border: "1px solid var(--border)",
          borderRadius: 10,
          padding: 24,
          width: 480,
          maxHeight: "80vh",
          overflowY: "auto",
          boxShadow: "0 12px 36px rgba(0,0,0,0.5)",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            marginBottom: 20,
          }}
        >
          <div style={{ fontSize: 16, fontWeight: 600 }}>⚙️ 设置</div>
          <button
            onClick={onClose}
            style={{
              background: "transparent",
              border: "none",
              color: "var(--text-muted)",
              fontSize: 18,
              cursor: "pointer",
              padding: 4,
            }}
          >
            ×
          </button>
        </div>

        {/* Change Password Section */}
        <section style={{ marginBottom: 24 }}>
          <div style={sectionTitleStyle}>修改登录密码</div>
          <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 12 }}>
            修改密码后需要使用新密码解锁应用
          </div>

          <label style={labelStyle}>原密码</label>
          <input
            ref={oldPassRef}
            type="password"
            value={oldPass}
            onChange={(e) => setOldPass(e.target.value)}
            placeholder="输入原密码"
            style={inputStyle}
          />

          <label style={{ ...labelStyle, marginTop: 8 }}>新密码</label>
          <input
            type="password"
            value={newPass}
            onChange={(e) => setNewPass(e.target.value)}
            placeholder="至少 6 个字符"
            style={inputStyle}
          />
          <div style={{ fontSize: 11, color: hint.color, marginTop: 4, height: 14 }}>
            {newPass.length > 0 && `强度：${hint.label}`}
          </div>

          <label style={{ ...labelStyle, marginTop: 8 }}>确认新密码</label>
          <input
            type="password"
            value={confirmPass}
            onChange={(e) => setConfirmPass(e.target.value)}
            placeholder="再次输入新密码"
            style={{
              ...inputStyle,
              borderColor: mismatch ? "var(--error)" : "var(--border)",
            }}
          />
          <div style={{ fontSize: 11, color: "var(--error)", marginTop: 4, height: 14 }}>
            {mismatch ? "两次输入不一致" : ""}
          </div>

          {passwordErr && (
            <div style={errorBoxStyle}>{passwordErr}</div>
          )}
          {passwordSuccess && (
            <div
              style={{
                ...errorBoxStyle,
                background: "rgba(166,227,161,0.12)",
                borderColor: "var(--success)",
                color: "var(--success)",
              }}
            >
              密码修改成功
            </div>
          )}

          <button
            onClick={handleChangePassword}
            disabled={!canChangePassword}
            style={{
              ...primaryBtnStyle,
              marginTop: 12,
              opacity: canChangePassword ? 1 : 0.45,
              cursor: canChangePassword ? "pointer" : "not-allowed",
            }}
          >
            {passwordBusy ? "处理中…" : "修改密码"}
          </button>
        </section>

        {/* Divider */}
        <div
          style={{
            height: 1,
            background: "var(--border)",
            margin: "16px 0",
          }}
        />

        {/* Export/Import Section */}
        <section>
          <div style={sectionTitleStyle}>配置导入导出</div>
          <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 12 }}>
            导出连接配置到加密文件，或从文件导入
          </div>

          {ioErr && <div style={errorBoxStyle}>{ioErr}</div>}

          <div style={{ display: "flex", gap: 8 }}>
            <button
              onClick={startExport}
              disabled={exportBusy || importBusy}
              style={{
                ...secondaryBtnStyle,
                flex: 1,
                opacity: exportBusy || importBusy ? 0.45 : 1,
              }}
            >
              📤 导出配置
            </button>
            <button
              onClick={startImport}
              disabled={exportBusy || importBusy}
              style={{
                ...secondaryBtnStyle,
                flex: 1,
                opacity: exportBusy || importBusy ? 0.45 : 1,
              }}
            >
              📥 导入配置
            </button>
          </div>
          <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 8 }}>
            当前共有 {connectionCount} 个连接
          </div>
        </section>

        {/* Divider */}
        <div
          style={{
            height: 1,
            background: "var(--border)",
            margin: "16px 0",
          }}
        />

        {/* Backup Section */}
        <section>
          <div style={sectionTitleStyle}>版本备份与回退</div>
          <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 12 }}>
            升级时自动备份配置，支持回退到旧版本
          </div>

          <div style={{ fontSize: 12, color: "var(--text-primary)", marginBottom: 8 }}>
            当前版本：<b>{appVersion || "加载中..."}</b>
          </div>

          {rollbackResult && (
            <div
              style={{
                ...errorBoxStyle,
                background: rollbackResult.includes("失败") ? "rgba(239,93,111,0.12)" : "rgba(166,227,161,0.12)",
                borderColor: rollbackResult.includes("失败") ? "var(--error)" : "var(--success)",
                color: rollbackResult.includes("失败") ? "var(--error)" : "var(--success)",
              }}
            >
              {rollbackResult}
            </div>
          )}

          {/* Quick rollback to previous version */}
          {previousVersion && (
            <button
              onClick={() => handleRollback(previousVersion)}
              disabled={rollbackBusy}
              style={{
                ...secondaryBtnStyle,
                width: "100%",
                marginBottom: 12,
                opacity: rollbackBusy ? 0.45 : 1,
              }}
            >
              ⏪ 快速回退到上一版本 ({previousVersion})
            </button>
          )}

          {/* Backup list */}
          {backups.length > 0 && (
            <div
              style={{
                border: "1px solid var(--border)",
                borderRadius: 6,
                overflow: "hidden",
              }}
            >
              <div
                style={{
                  padding: "8px 12px",
                  background: "var(--bg-input)",
                  fontSize: 11,
                  color: "var(--text-muted)",
                  borderBottom: "1px solid var(--border)",
                }}
              >
                可用备份（最多保留 5 个）
              </div>
              {backups.map((backup) => (
                <div
                  key={backup.version}
                  style={{
                    padding: "10px 12px",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    borderBottom: "1px solid var(--border)",
                  }}
                >
                  <div>
                    <div style={{ fontSize: 12, fontWeight: 500, color: "var(--text-primary)" }}>
                      v{backup.version}
                      {backup.version === appVersion && (
                        <span style={{ color: "var(--success)", marginLeft: 6 }}>当前</span>
                      )}
                    </div>
                    <div style={{ fontSize: 10, color: "var(--text-muted)" }}>
                      {backup.timestampStr} · {backup.files.length} 个文件
                    </div>
                  </div>
                  {backup.version !== appVersion && (
                    <button
                      onClick={() => handleRollback(backup.version)}
                      disabled={rollbackBusy}
                      style={{
                        padding: "4px 10px",
                        background: "transparent",
                        color: "var(--accent)",
                        border: "1px solid var(--accent)",
                        borderRadius: 4,
                        fontSize: 11,
                        cursor: rollbackBusy ? "not-allowed" : "pointer",
                        opacity: rollbackBusy ? 0.45 : 1,
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
            <div style={{ fontSize: 12, color: "var(--text-muted)", textAlign: "center", padding: 16 }}>
              暂无备份记录
            </div>
          )}
        </section>
      </div>

      {/* Export Passphrase Dialog */}
      {showExportDialog && (
        <div
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0,0,0,0.55)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 2100,
          }}
          onClick={() => setShowExportDialog(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{
              background: "var(--bg-panel)",
              border: "1px solid var(--border)",
              borderRadius: 10,
              padding: 20,
              width: 380,
              boxShadow: "0 12px 36px rgba(0,0,0,0.5)",
            }}
          >
            <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 8 }}>
              🔐 加密导出
            </div>
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 12 }}>
              设置加密密码，导入时需要此密码才能解密
            </div>
            <label style={labelStyle}>加密密码</label>
            <input
              type="password"
              value={exportPass}
              onChange={(e) => setExportPass(e.target.value)}
              placeholder="至少 6 个字符"
              style={inputStyle}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleExport();
              }}
            />
            <div style={{ display: "flex", gap: 8, marginTop: 16, justifyContent: "flex-end" }}>
              <button
                onClick={() => setShowExportDialog(false)}
                style={cancelBtnStyle}
              >
                取消
              </button>
              <button
                onClick={handleExport}
                disabled={exportPass.length < MIN_LEN || exportBusy}
                style={{
                  ...primaryBtnStyle,
                  opacity: exportPass.length >= MIN_LEN && !exportBusy ? 1 : 0.45,
                }}
              >
                {exportBusy ? "导出中…" : "导出"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Import Passphrase Dialog */}
      {showImportDialog && (
        <div
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0,0,0,0.55)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 2100,
          }}
          onClick={() => {
            setShowImportDialog(false);
            setImportPath(null);
          }}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{
              background: "var(--bg-panel)",
              border: "1px solid var(--border)",
              borderRadius: 10,
              padding: 20,
              width: 380,
              boxShadow: "0 12px 36px rgba(0,0,0,0.5)",
            }}
          >
            <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 8 }}>
              🔓 解密导入
            </div>
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 12 }}>
              输入导出时设置的密码以解密
            </div>
            <label style={labelStyle}>解密密码</label>
            <input
              type="password"
              value={importPass}
              onChange={(e) => setImportPass(e.target.value)}
              placeholder="至少 6 个字符"
              style={inputStyle}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleImport();
              }}
            />
            <div style={{ display: "flex", gap: 8, marginTop: 16, justifyContent: "flex-end" }}>
              <button
                onClick={() => {
                  setShowImportDialog(false);
                  setImportPath(null);
                }}
                style={cancelBtnStyle}
              >
                取消
              </button>
              <button
                onClick={handleImport}
                disabled={importPass.length < MIN_LEN || importBusy}
                style={{
                  ...primaryBtnStyle,
                  opacity: importPass.length >= MIN_LEN && !importBusy ? 1 : 0.45,
                }}
              >
                {importBusy ? "导入中…" : "导入"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

const sectionTitleStyle: React.CSSProperties = {
  fontSize: 13,
  fontWeight: 600,
  color: "var(--text-primary)",
  marginBottom: 8,
};

const labelStyle: React.CSSProperties = {
  display: "block",
  fontSize: 11,
  color: "var(--text-muted)",
  marginBottom: 4,
};

const inputStyle: React.CSSProperties = {
  width: "100%",
  boxSizing: "border-box",
  padding: "10px 12px",
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  border: "1px solid var(--border)",
  borderRadius: 6,
  fontSize: 13,
  outline: "none",
};

const errorBoxStyle: React.CSSProperties = {
  marginTop: 10,
  padding: "8px 10px",
  background: "rgba(239,93,111,0.12)",
  border: "1px solid var(--error)",
  borderRadius: 6,
  fontSize: 12,
  color: "var(--error)",
};

const primaryBtnStyle: React.CSSProperties = {
  padding: "8px 16px",
  background: "var(--accent)",
  color: "var(--bg-panel)",
  border: "none",
  borderRadius: 6,
  fontSize: 12,
  fontWeight: 600,
  cursor: "pointer",
};

const secondaryBtnStyle: React.CSSProperties = {
  padding: "8px 16px",
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  border: "1px solid var(--border)",
  borderRadius: 6,
  fontSize: 12,
  fontWeight: 500,
  cursor: "pointer",
};

const cancelBtnStyle: React.CSSProperties = {
  padding: "7px 14px",
  background: "transparent",
  color: "var(--text-secondary)",
  border: "1px solid var(--border)",
  borderRadius: 6,
  fontSize: 12,
  cursor: "pointer",
};
