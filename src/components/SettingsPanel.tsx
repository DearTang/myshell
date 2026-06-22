import { useState, useRef, useEffect } from "react";
import { open, save, confirm } from "@tauri-apps/plugin-dialog";
import {
  changeMasterPassword,
  exportConnections,
  importConnections,
  listBackups,
  rollbackBackup,
  getAppVersion,
  getPreviousVersion,
  readFileBase64,
  isElevated,
  restartAsAdmin,
} from "../api";
import type { BackupInfo } from "../api";
import { useTheme } from "../hooks/useTheme";
import { useColorScheme } from "../hooks/useColorScheme";
import { useTerminalFont } from "../hooks/useTerminalFont";
import { useAiConfig } from "../hooks/useAiConfig";
import type { AiProvider } from "../api";
import { PRESETS, type ColorPalette } from "../themes";
import { FontField } from "./FontField";

interface Props {
  onClose: () => void;
  onRefresh: () => void;
  connectionCount: number;
  onOpenQuickCommands: () => void;
}

const MIN_LEN = 6;
// Background opacity slider range. Keep in sync with the <input type="range"> below.
const BG_OPACITY_MIN = 10;
const BG_OPACITY_MAX = 100;

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

export function SettingsPanel({ onClose, onRefresh, connectionCount, onOpenQuickCommands }: Props) {
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
  const {
    paletteId,
    setPaletteId,
    customPalette,
    setCustomPalette,
    clearCustomPalette,
    bgImage,
    setBgImage,
  } = useColorScheme();
  const { primaryFont, setPrimaryFont } = useTerminalFont();

  // ── AI assistant config ── editable local state, initialized once from the
  // vault-backed settings, persisted via useAiConfig.save (key re-encrypted
  // in the backend; an empty key field means "keep existing").
  const { settings: aiSettings, save: saveAiConfig, loading: aiLoading } = useAiConfig();
  const [aiProvider, setAiProvider] = useState<AiProvider>("claude");
  const [aiModel, setAiModel] = useState("");
  const [aiBaseUrl, setAiBaseUrl] = useState("");
  const [aiProxy, setAiProxy] = useState("");
  const [aiKey, setAiKey] = useState("");
  const [aiTemp, setAiTemp] = useState(0.7);
  const [aiSaving, setAiSaving] = useState(false);
  const [aiMsg, setAiMsg] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const aiInitRef = useRef(false);
  useEffect(() => {
    // Sync once the backend config finishes loading — NOT on first render,
    // when aiSettings is still the DEFAULT placeholder. Without the loading
    // gate the ref flips before the real values arrive (IPC is async) and the
    // saved provider/model/baseUrl never populate the form.
    if (!aiLoading && !aiInitRef.current) {
      aiInitRef.current = true;
      setAiProvider(aiSettings.provider);
      setAiModel(aiSettings.model ?? "");
      setAiBaseUrl(aiSettings.baseUrl ?? "");
      setAiProxy(aiSettings.proxyUrl ?? "");
      setAiTemp(aiSettings.temperature);
    }
  }, [aiSettings, aiLoading]);

  const handleSaveAi = async () => {
    setAiSaving(true);
    setAiMsg(null);
    try {
      await saveAiConfig({
        provider: aiProvider,
        model: aiModel.trim() || undefined,
        baseUrl: aiBaseUrl.trim() || undefined,
        proxyUrl: aiProxy.trim() || undefined,
        apiKey: aiKey,
        temperature: aiTemp,
      });
      setAiKey("");
      setAiMsg({ kind: "ok", text: "已保存" });
    } catch (e) {
      setAiMsg({ kind: "err", text: `保存失败: ${e}` });
    } finally {
      setAiSaving(false);
      window.setTimeout(() => setAiMsg(null), 3000);
    }
  };

  // Background image local state (preview before applying)
  const [bgImagePath, setBgImagePath] = useState<string | null>(null);
  const [bgOpacity, setBgOpacity] = useState(bgImage.opacity * 100);
  const [bgImageBusy, setBgImageBusy] = useState(false);
  // Map the slider value into a 0–100% track position so the progress fill
  // lines up with the thumb (which itself is placed over [MIN, MAX]).
  const bgOpacityFillPct =
    ((bgOpacity - BG_OPACITY_MIN) / (BG_OPACITY_MAX - BG_OPACITY_MIN)) * 100;

  // Custom palette dialog state
  const [showCustomDialog, setShowCustomDialog] = useState(false);
  const [customAccent, setCustomAccent] = useState(
    customPalette?.dark.ui?.["--accent-primary"]?.toString() || "#6366f1"
  );
  const [customBg, setCustomBg] = useState(
    customPalette?.dark.terminal?.background || "#1e1e2e"
  );
  // null = not yet checked. Drives the admin/elevation status chip + restart button.
  const [elevated, setElevated] = useState<boolean | null>(null);
  const [restartBusy, setRestartBusy] = useState(false);

  useEffect(() => {
    getAppVersion().then(setAppVersion).catch(() => {});
    listBackups().then(setBackups).catch(() => {});
    getPreviousVersion().then(setPreviousVersion).catch(() => {});
    isElevated().then(setElevated).catch(() => setElevated(false));
  }, []);

  const hint = strengthHint(newPass);
  const mismatch = confirmPass.length > 0 && confirmPass !== newPass;
  const canChangePassword =
    oldPass.length >= MIN_LEN &&
    newPass.length >= MIN_LEN &&
    confirmPass === newPass &&
    !passwordBusy;

  async function handleRestartAdmin() {
    const ok = await confirm(
      "将以管理员身份重启 MyShell，当前所有终端会话将被关闭。是否继续？",
      { title: "以管理员重启", kind: "warning" }
    );
    if (!ok) return;
    setRestartBusy(true);
    try {
      await restartAsAdmin();
      // On success the process exits and the elevated instance takes over —
      // we intentionally don't reset restartBusy (this UI is about to vanish).
    } catch (e) {
      setRestartBusy(false);
      const msg = String(e);
      // Silent on UAC cancel; surface everything else.
      if (!msg.includes("取消")) alert(`以管理员重启失败: ${msg}`);
    }
  }

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
      // Clear the passphrase on every exit path so it never lingers in
      // component state after a failed attempt (success already closed
      // the dialog).
      setExportPass("");
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
      // Clear the passphrase on every exit path — a failed import is most
      // likely a wrong passphrase, so forcing re-entry is both safer and
      // better UX than leaving the wrong value populated.
      setImportPass("");
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
          <Section title="快捷命令">
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 12,
              }}
            >
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 13, color: "var(--text-primary)" }}>
                  全局与服务器专属快捷命令
                </div>
                <div style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 2 }}>
                  定义可复用的命令片段，支持多行按顺序执行，终端中一键运行
                </div>
              </div>
              <button
                onClick={onOpenQuickCommands}
                style={{
                  padding: "8px 14px",
                  background: "var(--accent-primary)",
                  color: "white",
                  border: "none",
                  borderRadius: "var(--radius-md)",
                  fontSize: 13,
                  fontWeight: 600,
                  cursor: "pointer",
                  whiteSpace: "nowrap",
                }}
              >
                🧩 管理
              </button>
            </div>
          </Section>

          {/* Color Scheme Section */}
          <Section title="配色方案">
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14 }}>
              选择预设配色方案，同步更新终端和界面颜色
            </div>
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "repeat(5, 1fr)",
                gap: 10,
              }}
            >
              {PRESETS.map((preset) => {
                const isActive = paletteId === preset.id && !customPalette;
                const variant = theme === "dark" ? preset.dark : preset.light;
                const dotColor = variant.terminal.background;
                const accentColor =
                  variant.ui?.["--accent-primary"] ||
                  (theme === "dark" ? "#6366f1" : "#4f46e5");
                return (
                  <button
                    key={preset.id}
                    onClick={() => {
                      setPaletteId(preset.id);
                      clearCustomPalette();
                    }}
                    title={preset.name}
                    style={{
                      display: "flex",
                      flexDirection: "column",
                      alignItems: "center",
                      gap: 6,
                      padding: "10px 6px",
                      background: isActive
                        ? "var(--accent-primary-muted)"
                        : "var(--bg-surface)",
                      border: isActive
                        ? "2px solid var(--accent-primary)"
                        : "2px solid transparent",
                      borderRadius: "var(--radius-lg)",
                      cursor: "pointer",
                      transition: "all var(--duration-fast) var(--ease-out-expo)",
                    }}
                    onMouseEnter={(e) => {
                      if (!isActive) {
                        e.currentTarget.style.borderColor = "var(--border-emphasis)";
                        e.currentTarget.style.background = "var(--bg-surface-hover)";
                      }
                    }}
                    onMouseLeave={(e) => {
                      if (!isActive) {
                        e.currentTarget.style.borderColor = "transparent";
                        e.currentTarget.style.background = "var(--bg-surface)";
                      }
                    }}
                  >
                    <div
                      style={{
                        width: 32,
                        height: 32,
                        borderRadius: "var(--radius-md)",
                        background: dotColor,
                        border: "1px solid var(--border-default)",
                        position: "relative",
                        overflow: "hidden",
                      }}
                    >
                      {/* Color accent bar at bottom of swatch */}
                      <div
                        style={{
                          position: "absolute",
                          bottom: 0,
                          left: 0,
                          right: 0,
                          height: 8,
                          background: accentColor,
                        }}
                      />
                    </div>
                    <span
                      style={{
                        fontSize: 10,
                        fontWeight: isActive ? 600 : 400,
                        color: isActive
                          ? "var(--accent-primary)"
                          : "var(--text-tertiary)",
                        textAlign: "center",
                        lineHeight: 1.3,
                      }}
                    >
                      {preset.name}
                    </span>
                  </button>
                );
              })}
            </div>

            {/* Custom palette entry */}
            <button
              onClick={() => setShowCustomDialog(true)}
              style={{
                marginTop: 10,
                width: "100%",
                padding: "10px 14px",
                background: customPalette
                  ? "var(--accent-primary-muted)"
                  : "var(--bg-surface)",
                color: customPalette
                  ? "var(--accent-primary)"
                  : "var(--text-tertiary)",
                border: customPalette
                  ? "1px solid var(--border-accent)"
                  : "1px dashed var(--border-default)",
                borderRadius: "var(--radius-md)",
                fontSize: 12,
                fontWeight: 500,
                cursor: "pointer",
                transition: "all var(--duration-fast) var(--ease-in-out)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                gap: 6,
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = "var(--accent-primary-muted)";
                e.currentTarget.style.borderColor = "var(--border-accent)";
                e.currentTarget.style.color = "var(--accent-primary)";
              }}
              onMouseLeave={(e) => {
                if (!customPalette) {
                  e.currentTarget.style.background = "var(--bg-surface)";
                  e.currentTarget.style.borderColor = "var(--border-default)";
                  e.currentTarget.style.color = "var(--text-tertiary)";
                }
              }}
            >
              <span style={{ fontSize: 14 }}>🎨</span>
              {customPalette ? "管理自定义主题" : "自定义主题..."}
            </button>

            {/* Custom palette dialog */}
            {showCustomDialog && (
              <CustomPaletteDialog
                accent={customAccent}
                bg={customBg}
                onAccentChange={setCustomAccent}
                onBgChange={setCustomBg}
                onSave={() => {
                  // Normalize to 7-char #RRGGBB before appending alpha
                  // suffixes ("cc", "26", ...). A free-text hex like "#RGB"
                  // shorthand or "#RRGGBBAA" would otherwise produce invalid
                  // CSS colors (e.g. "#fffcc") that browsers silently drop.
                  const normHex = (hex: string): string => {
                    const h = hex.trim();
                    if (/^#[0-9a-fA-F]{3}$/.test(h)) {
                      return "#" + h
                        .slice(1)
                        .split("")
                        .map((c) => c + c)
                        .join("");
                    }
                    return /^#[0-9a-fA-F]{6}/.test(h) ? h.slice(0, 7) : h;
                  };
                  const accent = normHex(customAccent);
                  const bg = normHex(customBg);
                  const id = `custom-${Date.now()}`;
                  const palette: ColorPalette = {
                    id,
                    name: "自定义主题",
                    dark: {
                      terminal: { ...PRESETS[0].dark.terminal, background: bg },
                      ui: {
                        ...PRESETS[0].dark.ui,
                        "--accent-primary": accent,
                        "--accent-primary-hover": accent + "cc",
                        "--accent-primary-muted": accent + "26",
                        "--border-accent": accent + "66",
                        "--shadow-glow": `0 0 20px ${accent}40`,
                        "--bg-base": bg,
                      },
                    },
                    light: {
                      terminal: { ...PRESETS[0].light.terminal, background: bg },
                      ui: {
                        ...PRESETS[0].light.ui,
                        "--accent-primary": accent,
                        "--accent-primary-hover": accent + "cc",
                        "--accent-primary-muted": accent + "1a",
                        "--border-accent": accent + "66",
                        "--shadow-glow": `0 0 20px ${accent}33`,
                      },
                    },
                  };
                  setCustomPalette(palette);
                  setPaletteId(id);
                  setShowCustomDialog(false);
                }}
                onClose={() => setShowCustomDialog(false)}
              />
            )}
          </Section>

          {/* Background Image Section */}
          <Section title="背景图片">
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14 }}>
              为终端区域设置背景图片
            </div>

            {/* Select image file button */}
            <button
              onClick={async () => {
                setBgImageBusy(true);
                try {
                  const selected = await open({
                    multiple: false,
                    filters: [
                      { name: "图片", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] },
                    ],
                  });
                  if (selected && !Array.isArray(selected)) {
                    const dataUrl = await readFileBase64(selected);
                    setBgImagePath(dataUrl);
                    // Auto-apply immediately
                    setBgImage({ dataUrl: dataUrl, opacity: bgOpacity / 100 });
                  }
                } catch (e) {
                  console.error("Failed to read image:", e);
                } finally {
                  setBgImageBusy(false);
                }
              }}
              disabled={bgImageBusy}
              style={{
                width: "100%",
                padding: "10px 16px",
                background: "var(--bg-surface)",
                color: "var(--text-primary)",
                border: "1px solid var(--border-default)",
                borderRadius: "var(--radius-md)",
                fontSize: 13,
                fontWeight: 500,
                cursor: bgImageBusy ? "not-allowed" : "pointer",
                opacity: bgImageBusy ? 0.5 : 1,
                transition: "all var(--duration-fast) var(--ease-in-out)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                gap: 8,
              }}
              onMouseEnter={(e) => {
                if (!bgImageBusy) {
                  e.currentTarget.style.background = "var(--bg-surface-hover)";
                  e.currentTarget.style.borderColor = "var(--border-emphasis)";
                }
              }}
              onMouseLeave={(e) => {
                if (!bgImageBusy) {
                  e.currentTarget.style.background = "var(--bg-surface)";
                  e.currentTarget.style.borderColor = "var(--border-default)";
                }
              }}
            >
              <span style={{ fontSize: 14 }}>🖼️</span>
              {bgImageBusy ? "选择中..." : bgImage.dataUrl ? "更换图片" : "选择图片"}
            </button>

            {/* Image preview thumbnail */}
            {bgImage.dataUrl && (
              <div
                style={{
                  marginTop: 10,
                  width: "100%",
                  height: 120,
                  borderRadius: "var(--radius-lg)",
                  backgroundImage: `url(${bgImage.dataUrl})`,
                  backgroundSize: "cover",
                  backgroundPosition: "center",
                  border: "1px solid var(--border-default)",
                  opacity: bgOpacity / 100,
                }}
              />
            )}

            {/* Status */}
            <div
              style={{
                marginTop: 10,
                padding: "8px 14px",
                background: "var(--bg-surface)",
                border: "1px solid var(--border-default)",
                borderRadius: "var(--radius-md)",
                fontSize: 11,
                color: bgImage.dataUrl
                  ? "var(--success)"
                  : "var(--text-muted)",
                display: "flex",
                alignItems: "center",
                gap: 6,
              }}
            >
              <span>{bgImage.dataUrl ? "✅" : "💭"}</span>
              <span>
                {bgImage.dataUrl
                  ? `已设置背景 (透明度: ${Math.round(bgOpacity)}%)`
                  : "未设置背景图片"}
              </span>
            </div>

            {/* Opacity slider — live-applied */}
            <div style={{ marginTop: 12 }}>
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  marginBottom: 4,
                }}
              >
                <label
                  style={{
                    fontSize: 12,
                    color: "var(--text-secondary)",
                    fontWeight: 500,
                  }}
                >
                  透明度
                </label>
                <span
                  style={{
                    fontSize: 11,
                    color: "var(--text-muted)",
                    fontVariantNumeric: "tabular-nums",
                  }}
                >
                  {Math.round(bgOpacity)}%
                </span>
              </div>
              {/* Range slider with proper vendor-prefixed pseudo-element reset */}
              <style>{`
                input[type="range"].bg-opacity-slider {
                  -webkit-appearance: none;
                  appearance: none;
                  width: 100%;
                  height: 6px;
                  border-radius: 3px;
                  outline: none;
                  cursor: pointer;
                  background: var(--bg-surface);
                }
                input[type="range"].bg-opacity-slider::-webkit-slider-runnable-track {
                  -webkit-appearance: none;
                  height: 6px;
                  border-radius: 3px;
                }
                input[type="range"].bg-opacity-slider::-webkit-slider-thumb {
                  -webkit-appearance: none;
                  width: 16px;
                  height: 16px;
                  border-radius: 50%;
                  background: var(--accent-primary);
                  border: 2px solid var(--bg-elevated);
                  margin-top: -5px;
                  cursor: pointer;
                  box-shadow: 0 1px 3px rgba(0,0,0,0.3);
                }
                input[type="range"].bg-opacity-slider::-moz-range-thumb {
                  width: 16px;
                  height: 16px;
                  border-radius: 50%;
                  background: var(--accent-primary);
                  border: 2px solid var(--bg-elevated);
                  cursor: pointer;
                  box-shadow: 0 1px 3px rgba(0,0,0,0.3);
                }
                input[type="range"].bg-opacity-slider::-moz-range-track {
                  height: 6px;
                  border-radius: 3px;
                  background: var(--bg-surface);
                }
                /* Progress fill via linear-gradient on the track */
                input[type="range"].bg-opacity-slider {
                  background: linear-gradient(
                    to right,
                    var(--accent-primary) 0%,
                    var(--accent-primary) ${bgOpacityFillPct}%,
                    var(--bg-surface) ${bgOpacityFillPct}%,
                    var(--bg-surface) 100%
                  );
                }
              `}</style>
              <input
                type="range"
                className="bg-opacity-slider"
                min={BG_OPACITY_MIN}
                max={BG_OPACITY_MAX}
                value={bgOpacity}
                onChange={(e) => {
                  const v = Number(e.target.value);
                  setBgOpacity(v);
                  // Live-update opacity on existing background
                  if (bgImage.dataUrl) {
                    setBgImage({ dataUrl: bgImage.dataUrl, opacity: v / 100 });
                  }
                }}
              />
            </div>

            {/* Clear button */}
            <div style={{ marginTop: 12 }}>
              <button
                onClick={() => {
                  setBgImage({ dataUrl: null, opacity: 1 });
                  setBgImagePath(null);
                  setBgOpacity(85);
                }}
                disabled={!bgImage.dataUrl}
                style={{
                  width: "100%",
                  padding: "10px 16px",
                  background: "transparent",
                  color: bgImage.dataUrl ? "var(--error)" : "var(--text-muted)",
                  border: `1px solid ${bgImage.dataUrl ? "var(--error)" : "var(--border-default)"}`,
                  borderRadius: "var(--radius-md)",
                  fontSize: 13,
                  fontWeight: 500,
                  cursor: bgImage.dataUrl ? "pointer" : "not-allowed",
                  opacity: bgImage.dataUrl ? 1 : 0.5,
                  transition: "all var(--duration-fast) var(--ease-in-out)",
                }}
                onMouseEnter={(e) => {
                  if (bgImage.dataUrl) {
                    e.currentTarget.style.background = "var(--error-muted)";
                  }
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = "transparent";
                }}
              >
                清除背景图片
              </button>
            </div>
          </Section>

          {/* Terminal Font Section */}
          <Section title="终端字体">
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14 }}>
              自定义终端字体；留空使用内置默认（已含 Nerd Font 回退）。填入本机已安装的字体名即可显示图标 / Powerline 字形。
            </div>
            <Field label="字体 (Font Family)">
              <FontField
                value={primaryFont}
                onChange={setPrimaryFont}
                placeholder="例如：CaskaydiaCove Nerd Font（留空用默认）"
              />
            </Field>
            <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 8, lineHeight: 1.6 }}>
              常见 Nerd Font：CaskaydiaCove Nerd Font、MesloLGM NF、JetBrainsMono Nerd Font、FiraCode Nerd Font、Hack Nerd Font
            </div>
          </Section>

          {/* AI Assistant Section */}
          <Section title="🤖 AI 助手">
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14, lineHeight: 1.6 }}>
              配置 AI 提供商，用于命令生成、输出诊断与服务器巡检。API key 经主密码库（vault）加密存储；本地 Ollama 无需 key、数据不出本机。
            </div>
            <Field label="提供商 (Provider)">
              <select
                value={aiProvider}
                onChange={(e) => setAiProvider(e.target.value as AiProvider)}
                style={{
                  width: "100%",
                  padding: "10px 12px",
                  background: "var(--bg-input)",
                  color: "var(--text-primary)",
                  border: "1px solid var(--border-default)",
                  borderRadius: "var(--radius-md)",
                  fontSize: 13,
                  outline: "none",
                }}
              >
                <option value="claude">Claude (Anthropic)</option>
                <option value="openai">OpenAI (GPT)</option>
                <option value="ollama">Ollama (本地)</option>
              </select>
            </Field>
            <Field label="模型 (Model)">
              <Input
                value={aiModel}
                onChange={setAiModel}
                placeholder={
                  aiProvider === "claude"
                    ? "claude-sonnet-4-6（留空用默认）"
                    : aiProvider === "openai"
                    ? "gpt-4o（留空用默认）"
                    : "llama3.1（留空用默认）"
                }
              />
            </Field>
            <Field label="API Base URL（可选：自建 / 代理 / Ollama 地址）">
              <Input
                value={aiBaseUrl}
                onChange={setAiBaseUrl}
                placeholder={
                  aiProvider === "ollama"
                    ? "http://localhost:11434/api（留空用默认）"
                    : "留空用官方；自定义填到版本路径，如智谱 https://open.bigmodel.cn/api/paas/v4"
                }
              />
            </Field>
            <Field label="网络代理（可选：http:// 或 socks5://）">
              <Input
                value={aiProxy}
                onChange={setAiProxy}
                placeholder="留空直连；如 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080（认证写 user:pass@host）"
              />
            </Field>
            <Field
              label={`API Key${aiSettings.hasKey ? "（已保存，留空保持不变）" : ""}`}
            >
              <Input
                value={aiKey}
                onChange={setAiKey}
                type="password"
                placeholder={aiSettings.hasKey ? "••••••（已保存）" : "粘贴 API key"}
              />
            </Field>
            <Field label={`Temperature（创造性 ${aiTemp}）`}>
              <input
                type="range"
                min={0}
                max={1}
                step={0.1}
                value={aiTemp}
                onChange={(e) => setAiTemp(Number(e.target.value))}
                style={{ width: "100%" }}
              />
            </Field>
            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <button
                onClick={handleSaveAi}
                disabled={aiSaving}
                style={{
                  background: "var(--accent-primary)",
                  color: "#fff",
                  border: "none",
                  borderRadius: "var(--radius-md)",
                  padding: "9px 16px",
                  fontSize: 13,
                  fontWeight: 500,
                  cursor: aiSaving ? "default" : "pointer",
                  opacity: aiSaving ? 0.7 : 1,
                }}
              >
                {aiSaving ? "保存中…" : "保存 AI 配置"}
              </button>
              {aiMsg && (
                <span
                  style={{
                    fontSize: 12,
                    color: aiMsg.kind === "ok" ? "var(--success)" : "var(--error)",
                  }}
                >
                  {aiMsg.text}
                </span>
              )}
            </div>
          </Section>
          <Divider />

          {/* Admin / Elevation Section */}
          <Section title="🛡️ 管理员权限">
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14, lineHeight: 1.6 }}>
              本地终端以 MyShell 自身的权限运行 shell。需要管理员权限执行命令（如安装软件、修改系统配置）时，以管理员身份重启 MyShell，之后所有本地连接即获得管理员权限。
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
              <span style={{
                fontSize: 12,
                fontWeight: 600,
                padding: "5px 12px",
                borderRadius: "var(--radius-full)",
                background: elevated ? "var(--bg-surface-hover)" : "var(--bg-surface)",
                color: elevated ? "var(--success)" : "var(--text-tertiary)",
                border: elevated ? "1px solid var(--success)" : "1px solid var(--border-default)",
              }}>
                {elevated === null ? "检测中…" : elevated ? "✓ 已是管理员" : "当前：普通用户"}
              </span>
              {elevated === false && (
                <button
                  onClick={handleRestartAdmin}
                  disabled={restartBusy}
                  style={{
                    padding: "8px 16px",
                    background: "var(--accent-primary)",
                    color: "white",
                    border: "none",
                    borderRadius: "var(--radius-md)",
                    fontSize: 13,
                    fontWeight: 600,
                    cursor: restartBusy ? "wait" : "pointer",
                    opacity: restartBusy ? 0.7 : 1,
                    transition: "all var(--duration-fast) var(--ease-in-out)",
                  }}
                >
                  {restartBusy ? "启动中…" : "以管理员重启"}
                </button>
              )}
            </div>
            {elevated === false && (
              <div style={{
                marginTop: 10,
                padding: "8px 12px",
                fontSize: 11,
                color: "var(--warning)",
                background: "var(--warning-muted)",
                border: "1px solid var(--warning)",
                borderRadius: "var(--radius-md)",
                lineHeight: 1.5,
              }}>
                ⚠ 重启将关闭当前所有终端会话；Windows 会弹出 UAC 确认，点击「是」后以管理员启动新实例。
              </div>
            )}
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

// ── Custom Palette Dialog ──

function CustomPaletteDialog({
  accent,
  bg,
  onAccentChange,
  onBgChange,
  onSave,
  onClose,
}: {
  accent: string;
  bg: string;
  onAccentChange: (v: string) => void;
  onBgChange: (v: string) => void;
  onSave: () => void;
  onClose: () => void;
}) {
  return (
    <Dialog title="自定义主题" icon="🎨" onClose={onClose}>
      <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14 }}>
        选择主题色和终端背景色，保存为自定义主题
      </div>

      <Field label="主题色">
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <input
            type="color"
            value={accent}
            onChange={(e) => onAccentChange(e.target.value)}
            style={{
              width: 36,
              height: 36,
              padding: 0,
              border: "1px solid var(--border-default)",
              borderRadius: "var(--radius-md)",
              cursor: "pointer",
              background: "transparent",
            }}
          />
          <Input
            value={accent}
            onChange={onAccentChange}
            placeholder="#6366f1"
          />
        </div>
      </Field>

      <Field label="终端背景色">
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <input
            type="color"
            value={bg}
            onChange={(e) => onBgChange(e.target.value)}
            style={{
              width: 36,
              height: 36,
              padding: 0,
              border: "1px solid var(--border-default)",
              borderRadius: "var(--radius-md)",
              cursor: "pointer",
              background: "transparent",
            }}
          />
          <Input
            value={bg}
            onChange={onBgChange}
            placeholder="#1e1e2e"
          />
        </div>
      </Field>

      {/* Preview */}
      <div
        style={{
          marginTop: 16,
          padding: 16,
          background: bg,
          border: `2px solid ${accent}`,
          borderRadius: "var(--radius-lg)",
          display: "flex",
          flexDirection: "column",
          gap: 8,
        }}
      >
        <div style={{ fontSize: 11, color: "#888", textAlign: "center" }}>
          预览
        </div>
        <div
          style={{
            padding: "6px 12px",
            background: accent,
            borderRadius: "var(--radius-md)",
            fontSize: 12,
            fontWeight: 600,
            color: "#fff",
            textAlign: "center",
          }}
        >
          主题按钮
        </div>
      </div>

      <div style={{ display: "flex", gap: 10, marginTop: 20, justifyContent: "flex-end" }}>
        <button
          onClick={onClose}
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
          onClick={onSave}
          style={{
            padding: "10px 24px",
            background: "var(--accent-primary)",
            color: "white",
            border: "none",
            borderRadius: "var(--radius-md)",
            fontSize: 13,
            fontWeight: 600,
            cursor: "pointer",
            transition: "all var(--duration-fast) var(--ease-in-out)",
            boxShadow: "var(--shadow-glow)",
          }}
        >
          保存
        </button>
      </div>
    </Dialog>
  );
}
