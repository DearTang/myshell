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
  mcpDetectTools,
  mcpWriteConfig,
  mcpRemoveConfig,
  getAttachmentDir,
  setAttachmentDir,
  showInFolder,
  getCommandRules,
  setCommandRules,
} from "../api";
import type { CommandRules } from "../api";
import type { BackupInfo, AiToolInfo } from "../api";
import { useTheme } from "../hooks/useTheme";
import { useColorScheme } from "../hooks/useColorScheme";
import { useTerminalFont } from "../hooks/useTerminalFont";
import {
  useRendererPref,
  useGpuPref,
  readGpuDisabled,
  type RendererBackend,
} from "../hooks/useRendererPref";
import { useAiConfig } from "../hooks/useAiConfig";
import type { AiProvider, SupplierModel } from "../api";
import {
  addSupplierModel,
  fetchModelsForSupplier,
  fetchProviderModels,
  removeSupplierModel,
  saveAiModel,
  setActiveAiModel,
  toggleAiModelEnabled,
} from "../api";
import { RecycleDialog } from "./RecycleDialog";
import { compressImageDataUrl } from "../utils/image";
import {
  getSftpDownloadConcurrency,
  setSftpDownloadConcurrency,
  DEFAULT_SFTP_CONCURRENCY,
} from "../utils/transfer-settings";
import { aiTestSettings } from "../api";
import { PRESETS, type ColorPalette } from "../themes";
import { FontField } from "./FontField";

// ── System AI provider presets (mirrors init_ai_presets_cmd in ai.rs) ──
// These are NOT shown in the sidebar — they're offered as templates when
// creating a new supplier so the user can auto-fill provider + baseUrl.
interface AiPreset {
  name: string;
  provider: Exclude<AiProvider, "claude" | "openai" | "ollama">;
  baseUrl: string;
}
const AI_PRESETS: AiPreset[] = [
  { name: "GLM (智谱)", provider: "openai_compatible", baseUrl: "https://open.bigmodel.cn/api/paas/v4" },
  { name: "GLM Coding Plan (OpenAI)", provider: "openai_compatible", baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4" },
  { name: "GLM Coding Plan (Anthropic)", provider: "anthropic_compatible", baseUrl: "https://open.bigmodel.cn/api/anthropic" },
  { name: "MIMO", provider: "openai_compatible", baseUrl: "https://api.xiaomimimo.com/v1" },
  { name: "MiniMax M3", provider: "openai_compatible", baseUrl: "https://api.minimaxi.com/v1" },
  { name: "LongCat", provider: "openai_compatible", baseUrl: "https://api.longcat.chat/openai" },
  { name: "DeepSeek", provider: "openai_compatible", baseUrl: "https://api.deepseek.com/v1" },
  { name: "通义千问 (阿里云)", provider: "openai_compatible", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1" },
  { name: "混元 (腾讯云)", provider: "openai_compatible", baseUrl: "https://api.hunyuan.cloud.tencent.com/v1" },
  { name: "Ollama (本地)", provider: "openai_compatible", baseUrl: "http://localhost:11434/api" },
];
const PROVIDER_LABELS: Record<string, string> = {
  openai_compatible: "OpenAI 兼容",
  anthropic_compatible: "Anthropic 兼容",
  claude: "Claude 官方",
  openai: "OpenAI 官方",
  ollama: "Ollama 本地",
};
/** Detect if a user supplier matches a known preset by provider + baseUrl. */
function findPresetName(provider: string, baseUrl?: string): string | null {
  if (!baseUrl) return null;
  const match = AI_PRESETS.find(
    (p) => p.provider === provider && p.baseUrl === baseUrl,
  );
  return match ? match.name : null;
}

const categories = [
  { id: "appearance", label: "外观", icon: "🎨" },
  { id: "ai", label: "AI 助手", icon: "🤖" },
  { id: "mcp", label: "MCP 支持", icon: "🔌" },
  { id: "multiWindow", label: "多窗口", icon: "🪟" },
  { id: "transfer", label: "文件传输", icon: "📤" },
  { id: "security", label: "安全", icon: "🔒" },
  { id: "data", label: "数据管理", icon: "💾" },
  { id: "quickCommands", label: "快捷命令", icon: "⚡" },
];

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
  // Dummy tick to force re-render when the auto-lock select changes (reads localStorage).
  const [, setAutoLockTick] = useState(0);
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
  const [activeCategory, setActiveCategory] = useState("appearance");
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
  // Terminal renderer backend + GPU-off escape hatch (see useRendererPref).
  // rendererBackend is pure-frontend (localStorage) and applies to NEW tabs.
  // gpuDisabled is Rust-backed and applies on the NEXT launch — we load its
  // persisted value from the flag file once on mount.
  const { rendererBackend, setRendererBackend } = useRendererPref();
  const [gpuDisabledInit, setGpuDisabledInit] = useState(false);
  const { gpuDisabled, setGpuDisabled } = useGpuPref(gpuDisabledInit);
  // ── SFTP download concurrency (localStorage; read at transfer start) ──
  const [sftpConcurrency, setSftpConcurrencyState] = useState(
    getSftpDownloadConcurrency()
  );
  const setSftpConcurrency = (n: number) => {
    setSftpConcurrencyState(n);
    setSftpDownloadConcurrency(n);
  };

  // ── MCP server state ──
  const [mcpEnabled, setMcpEnabled] = useState(false);
  const [mcpTools, setMcpTools] = useState<AiToolInfo[]>([]);
  const [mcpConfiguring, setMcpConfiguring] = useState(false);
  // ── Attachment directory (where screenshots are auto-saved) ──
  const [attachmentDir, setAttachmentDirState] = useState<string | null>(null);
  const [attachmentDirPicking, setAttachmentDirPicking] = useState(false);
  const [attachmentDirAcknowledged, setAttachmentDirAcknowledged] = useState(false);
  // ── Command confirmation rules (whitelist/blacklist regex) ──
  const [rulesBlacklist, setRulesBlacklist] = useState<string[]>([]);
  const [rulesWhitelist, setRulesWhitelist] = useState<string[]>([]);
  const [rulesConfirmUnknown, setRulesConfirmUnknown] = useState(false);
  const [rulesShowInGui, setRulesShowInGui] = useState(true);
  const [rulesSaving, setRulesSaving] = useState(false);
  // Search + add-input state for the list-style rule editor.
  const [blacklistSearch, setBlacklistSearch] = useState("");
  const [blacklistInput, setBlacklistInput] = useState("");
  const [whitelistSearch, setWhitelistSearch] = useState("");
  const [whitelistInput, setWhitelistInput] = useState("");
  useEffect(() => {
    let cancelled = false;
    readGpuDisabled()
      .then((v) => {
        if (!cancelled) setGpuDisabledInit(v);
      })
      .catch(() => {
        /* default false is fine */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // ── MCP server initialization ──
  useEffect(() => {
    if (activeCategory !== "mcp") return;
    let cancelled = false;
    // Detect installed tools
    mcpDetectTools()
      .then((tools) => {
        if (!cancelled) {
          setMcpTools(tools);
          // If any tool is already configured, reflect that in the toggle
          setMcpEnabled(tools.some((t) => t.configured));
        }
      })
      .catch(() => {});
    // Load attachment directory
    getAttachmentDir()
      .then((dir) => { if (!cancelled) setAttachmentDirState(dir); })
      .catch(() => {});
    // Load command confirmation rules
    getCommandRules()
      .then((r) => {
        if (cancelled) return;
        setRulesBlacklist(r.blacklist);
        setRulesWhitelist(r.whitelist);
        setRulesConfirmUnknown(r.confirm_unknown);
        setRulesShowInGui(r.show_in_gui);
      })
      .catch(() => {});
    // Load "user has acknowledged the attachment-dir prompt" flag
    try {
      const ack = localStorage.getItem("myshell-attachment-dir-acknowledged");
      if (ack === "1") setAttachmentDirAcknowledged(true);
    } catch {}
    return () => { cancelled = true; };
  }, [activeCategory]);

  /** Pick a directory via the OS folder picker and persist it. */
  async function pickAttachmentDir() {
    setAttachmentDirPicking(true);
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected === "string") {
        const canonical = await setAttachmentDir(selected);
        setAttachmentDirState(canonical);
        try { localStorage.setItem("myshell-attachment-dir-acknowledged", "1"); } catch {}
        setAttachmentDirAcknowledged(true);
      }
    } catch (e) {
      console.error("pickAttachmentDir failed:", e);
    } finally {
      setAttachmentDirPicking(false);
    }
  }

  /** Parse the textarea contents into a CommandRules and persist it. */
  async function saveCommandRules() {
    setRulesSaving(true);
    try {
      const rules: CommandRules = {
        blacklist: rulesBlacklist,
        whitelist: rulesWhitelist,
        confirm_unknown: rulesConfirmUnknown,
        show_in_gui: rulesShowInGui,
      };
      await setCommandRules(rules);
    } catch (e) {
      console.error("saveCommandRules failed:", e);
    } finally {
      setRulesSaving(false);
    }
  }

  // The GPU flag only takes effect on next launch. Track whether the in-memory
  // toggle disagrees with what's persisted so we can show "重启生效" / "已生效".
  const gpuPendingRestart = gpuDisabled !== gpuDisabledInit;

  // ── AI assistant config ── left-right layout: supplier list (left) +
  // detail editor (right). The multi-model store is the source of truth;
  // the right panel edits a copy of the selected supplier.
  const {
    models: aiModels,
    activeId: aiActiveId,
    reload: reloadAi,
    setActive: setActiveAiModelHook,
    loading: aiLoading,
  } = useAiConfig();
  const [selectedSupplierId, setSelectedSupplierId] = useState<number | null>(null);
  // "create mode": when true, the right panel shows a new-supplier form.
  const [creating, setCreating] = useState(false);
  const [createPresetIdx, setCreatePresetIdx] = useState(-1); // -1 = custom
  // Edit buffer for the selected supplier (right panel).
  const [editName, setEditName] = useState("");
  const [editProvider, setEditProvider] = useState<AiProvider>("openai_compatible");
  const [editBaseUrl, setEditBaseUrl] = useState("");
  const [editProxy, setEditProxy] = useState("");
  const [editKey, setEditKey] = useState("");
  const [editTemp, setEditTemp] = useState(0.7);
  const [editModels, setEditModels] = useState<SupplierModel[]>([]);
  const [editSaving, setEditSaving] = useState(false);
  const [editTesting, setEditTesting] = useState(false);
  const [editMsg, setEditMsg] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  // Inline rename state (double-click on supplier name).
  const [renamingId, setRenamingId] = useState<number | null>(null);
  const [renameBuf, setRenameBuf] = useState("");
  // Model fetching state (right panel "从接口获取模型").
  const [fetchingModels, setFetchingModels] = useState(false);
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [fetchedModels, setFetchedModels] = useState<string[]>([]);
  // Manual model add buffer.
  const [showAddModel, setShowAddModel] = useState(false);
  const [newModelId, setNewModelId] = useState("");
  const [newModelLabel, setNewModelLabel] = useState("");
  // In-app toast/dialog for action feedback (replaces ugly native alert).
  const [toast, setToast] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const showToast = (kind: "ok" | "err", text: string) => {
    setToast({ kind, text });
    window.setTimeout(() => setToast(null), 4000);
  };
  // Track whether the current edit buffer passed a test (for create-mode gate).
  const [testedOk, setTestedOk] = useState(false);

  // Sync the right-panel edit buffer whenever the selected supplier changes
  // (or the underlying store reloads). In create mode, initialize from the
  // chosen preset (or blank for custom).
  const selectedSupplier = aiModels.find((m) => m.id === selectedSupplierId) ?? null;
  // Only show non-preset suppliers in the sidebar.
  const userSuppliers = aiModels.filter((m) => !m.isPreset);
  useEffect(() => {
    if (creating) {
      // Initialize create-mode buffer from the selected preset.
      const preset = createPresetIdx >= 0 ? AI_PRESETS[createPresetIdx] : null;
      setEditName(preset ? preset.name : "");
      setEditProvider((preset?.provider ?? "openai_compatible") as AiProvider);
      setEditBaseUrl(preset?.baseUrl ?? "");
      setEditProxy("");
      setEditKey("");
      setEditTemp(0.7);
      setEditModels([]);
      setFetchedModels([]);
      setFetchError(null);
      setShowAddModel(false);
      return;
    }
    if (selectedSupplier) {
      setEditName(selectedSupplier.name);
      setEditProvider(selectedSupplier.provider);
      setEditBaseUrl(selectedSupplier.baseUrl ?? "");
      setEditProxy(selectedSupplier.proxyUrl ?? "");
      setEditTemp(selectedSupplier.temperature);
      setEditModels(selectedSupplier.models);
      setEditKey("");
      setFetchedModels([]);
      setFetchError(null);
      setShowAddModel(false);
      setTestedOk(false);
    }
  }, [selectedSupplierId, aiModels, creating, createPresetIdx]);

  // Auto-select the first USER supplier once data loads (skip presets).
  useEffect(() => {
    if (!aiLoading && !creating && selectedSupplierId === null && userSuppliers.length > 0) {
      setSelectedSupplierId(userSuppliers[0].id);
    }
  }, [aiLoading, userSuppliers, selectedSupplierId, creating]);

  const handleSaveSupplier = async () => {
    // Create-mode gate: must have at least one model AND pass a test first.
    if (creating) {
      if (editModels.length === 0) {
        showToast("err", "请先添加至少一个模型再创建供应商");
        return;
      }
      if (!testedOk) {
        showToast("err", "请先点击「测试」通过后再创建");
        return;
      }
    }
    setEditSaving(true);
    setEditMsg(null);
    const wasCreating = creating;
    try {
      const id = await saveAiModel({
        // No id = create new (create mode); has id = update existing.
        ...(creating ? {} : { id: selectedSupplierId ?? undefined }),
        name: editName.trim() || "未命名",
        provider: editProvider,
        modelId: editModels[0]?.modelId ?? "default",
        baseUrl: editBaseUrl.trim() || undefined,
        apiKey: editKey || undefined,
        proxyUrl: editProxy.trim() || undefined,
        temperature: editTemp,
        models: editModels.map((m) => ({ modelId: m.modelId, label: m.label })),
      });
      setEditKey("");
      await reloadAi();
      // Exit create mode + select the newly created supplier.
      setCreating(false);
      setSelectedSupplierId(id);
      showToast("ok", wasCreating ? "供应商已创建" : "供应商已保存");
    } catch (e) {
      showToast("err", `保存失败: ${e}`);
    } finally {
      setEditSaving(false);
    }
  };

  // Test the current supplier's config via overrides — does NOT save first.
  const handleTestSupplier = async () => {
    setEditTesting(true);
    try {
      const msg = await aiTestSettings({
        supplierId: creating ? undefined : selectedSupplierId ?? undefined,
        provider: editProvider,
        model: editModels[0]?.modelId ?? "default",
        baseUrl: editBaseUrl.trim() || undefined,
        proxyUrl: editProxy.trim() || undefined,
        apiKey: editKey,
        temperature: editTemp,
      });
      showToast("ok", msg);
      setTestedOk(true);
    } catch (e) {
      showToast("err", `测试失败: ${e}`);
      setTestedOk(false);
    } finally {
      setEditTesting(false);
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
  // Recycle bin (restore deleted connections) dialog state
  const [showRecycleDialog, setShowRecycleDialog] = useState(false);
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
          width: 850,
          height: "85vh",
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

        {/* Main Content Area */}
        <div style={{ flex: 1, display: "flex", overflow: "hidden" }}>
          {/* Left Navigation */}
          <SettingsNavigation
            activeCategory={activeCategory}
            onCategoryChange={setActiveCategory}
          />

          {/* Right Content */}
          <div style={{ flex: 1, overflowY: "auto", padding: "20px 24px", minHeight: 480 }}>

          {/* Appearance Category — 配色 / 背景图 / 字体 / 渲染后端 */}
          {activeCategory === "appearance" && (
            <>
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
                    const raw = await readFileBase64(selected);
                    // Compress before storing: raw base64 of a multi-MB photo
                    // blows past the localStorage ~5 MB quota (setItem throws,
                    // is silently swallowed, and the image never persists).
                    // Downscale to ≤1920px + JPEG keeps it well under quota.
                    const dataUrl = await compressImageDataUrl(raw);
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

          {/* Terminal Rendering Section — renderer backend + GPU escape hatch.
              Targets the "cursor invisible / selection highlight invisible"
              reports: those trace to xterm.js's renderer layer, and this lets
              a user on a misbehaving GPU recover. */}
          <Section title="终端渲染">
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14, lineHeight: 1.6 }}>
              若出现光标不显示、选中区域看不到高亮等问题，可在此切换渲染后端或关闭 GPU 加速。仅影响新打开的终端标签页。
            </div>
            <Field label="渲染后端">
              <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                {(
                  [
                    { id: "auto", label: "自动（推荐）", desc: "默认 Canvas，透明背景时用 WebGL" },
                    { id: "canvas", label: "Canvas", desc: "最稳定，光标/选区直接画在画布上" },
                    { id: "webgl", label: "WebGL", desc: "性能最佳，依赖 GPU 合成" },
                    { id: "dom", label: "DOM", desc: "最轻量，仅聚焦时显示光标" },
                  ] as { id: RendererBackend; label: string; desc: string }[]
                ).map((opt) => {
                  const active = rendererBackend === opt.id;
                  return (
                    <button
                      key={opt.id}
                      title={opt.desc}
                      onClick={() => setRendererBackend(opt.id)}
                      style={{
                        padding: "8px 14px",
                        background: active ? "var(--accent-primary-muted)" : "var(--bg-input)",
                        color: active ? "var(--accent-primary)" : "var(--text-secondary)",
                        border: `1px solid ${active ? "var(--accent-primary)" : "var(--border-default)"}`,
                        borderRadius: "var(--radius-md)",
                        fontSize: 12,
                        fontWeight: active ? 600 : 400,
                        cursor: "pointer",
                        transition: "all var(--duration-fast) var(--ease-in-out)",
                      }}
                    >
                      {opt.label}
                    </button>
                  );
                })}
              </div>
            </Field>

            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 12,
                marginTop: 14,
                padding: "12px 14px",
                background: "var(--bg-input)",
                border: "1px solid var(--border-default)",
                borderRadius: "var(--radius-md)",
              }}
            >
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 13, color: "var(--text-primary)" }}>
                  禁用 GPU 硬件加速
                </div>
                <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 3, lineHeight: 1.5 }}>
                  切换 WebGL/canvas 合成异常（光标/选区消失）时的最终手段。{gpuPendingRestart ? "更改需重启应用后生效。" : "当前设置已生效。"}
                </div>
              </div>
              <Toggle checked={gpuDisabled} onChange={setGpuDisabled} />
            </div>
          </Section>
            </>
          )}

          {/* AI Category — left-right layout: supplier list + detail editor */}
          {activeCategory === "ai" && (
            <>
          <Section title="🤖 AI 助手">
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14, lineHeight: 1.6 }}>
              配置 AI 供应商与模型，用于命令生成、输出诊断与服务器巡检。左侧选择供应商，右侧编辑详情。API key 经主密码库加密存储。
            </div>
            <div style={{ display: "flex", gap: 16, minHeight: 420 }}>
              {/* ── Left: supplier list ── */}
              <div style={{ width: 240, flexShrink: 0, display: "flex", flexDirection: "column" }}>
                <button
                  onClick={() => {
                    setCreating(true);
                    setSelectedSupplierId(null);
                    setCreatePresetIdx(-1);
                  }}
                  style={{
                    width: "100%",
                    padding: "8px 12px",
                    background: "var(--accent-primary)",
                    color: "#fff",
                    border: "none",
                    borderRadius: "var(--radius-md)",
                    fontSize: 12,
                    fontWeight: 600,
                    cursor: "pointer",
                    marginBottom: 8,
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    gap: 6,
                  }}
                >
                  <span style={{ fontSize: 14 }}>＋</span> 新建供应商
                </button>
                <div style={{ flex: 1, overflowY: "auto", display: "flex", flexDirection: "column", gap: 4 }}>
                  {userSuppliers.map((m) => {
                    const isSelected = selectedSupplierId === m.id;
                    return (
                      <div
                        key={m.id}
                        onClick={() => setSelectedSupplierId(m.id)}
                        onDoubleClick={(e) => {
                          e.stopPropagation();
                          setRenamingId(m.id);
                          setRenameBuf(m.name);
                        }}
                        style={{
                          padding: "8px 10px",
                          borderRadius: "var(--radius-md)",
                          border: `1px solid ${
                            isSelected ? "var(--accent-primary)" : "var(--border-default)"
                          }`,
                          background: isSelected ? "var(--accent-primary-muted)" : "var(--bg-surface)",
                          cursor: "pointer",
                        }}
                      >
                        {/* Row 1: name + enable/disable toggle */}
                        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                          <span
                            style={{
                              flex: 1,
                              fontSize: 12,
                              fontWeight: isSelected ? 600 : 400,
                              color: isSelected ? "var(--accent-primary)" : "var(--text-primary)",
                              overflow: "hidden",
                              textOverflow: "ellipsis",
                              whiteSpace: "nowrap",
                            }}
                          >
                            {renamingId === m.id ? (
                              <input
                                autoFocus
                                value={renameBuf}
                                onChange={(e) => setRenameBuf(e.target.value)}
                                onBlur={async () => {
                                  if (renameBuf.trim() && renameBuf !== m.name) {
                                    try {
                                      await saveAiModel({
                                        id: m.id,
                                        name: renameBuf.trim(),
                                        provider: m.provider,
                                        modelId: m.modelId,
                                        baseUrl: m.baseUrl,
                                        temperature: m.temperature,
                                        models: m.models.map((x) => ({ modelId: x.modelId, label: x.label })),
                                      });
                                      await reloadAi();
                                    } catch { /* ignore */ }
                                  }
                                  setRenamingId(null);
                                }}
                                onKeyDown={(e) => {
                                  if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                                  if (e.key === "Escape") setRenamingId(null);
                                }}
                                onClick={(e) => e.stopPropagation()}
                                style={{
                                  width: "100%",
                                  padding: "2px 4px",
                                  fontSize: 12,
                                  background: "var(--bg-input)",
                                  color: "var(--text-primary)",
                                  border: "1px solid var(--accent-primary)",
                                  borderRadius: "var(--radius-sm)",
                                  outline: "none",
                                }}
                              />
                            ) : (
                              <>
                                <span>{m.name}</span>
                                {!m.isEnabled && (
                                  <span style={{ fontSize: 9, marginLeft: 4, color: "var(--text-muted)" }}>已禁用</span>
                                )}
                              </>
                            )}
                          </span>
                          {/* Enable / Disable toggle switch */}
                          <button
                            onClick={async (e) => {
                              e.stopPropagation();
                              try {
                                await toggleAiModelEnabled(m.id, !m.isEnabled);
                                await reloadAi();
                              } catch (e2) {
                                showToast("err", `操作失败: ${e2}`);
                              }
                            }}
                            title={m.isEnabled ? "点击禁用" : "点击启用"}
                            role="switch"
                            aria-checked={m.isEnabled}
                            style={{
                              flexShrink: 0,
                              width: 32,
                              height: 18,
                              padding: 2,
                              background: m.isEnabled ? "var(--success)" : "var(--bg-surface-active)",
                              border: "none",
                              borderRadius: "var(--radius-full)",
                              cursor: "pointer",
                              transition: "background 0.2s",
                              display: "flex",
                              alignItems: "center",
                              position: "relative",
                            }}
                          >
                            <span
                              style={{
                                width: 14,
                                height: 14,
                                background: "#fff",
                                borderRadius: "50%",
                                transform: m.isEnabled ? "translateX(14px)" : "translateX(0)",
                                transition: "transform 0.2s",
                                boxShadow: "0 1px 2px rgba(0,0,0,0.15)",
                              }}
                            />
                          </button>
                        </div>
                      </div>
                    );
                  })}
                  {userSuppliers.length === 0 && !aiLoading && (
                    <div style={{ fontSize: 11, color: "var(--text-muted)", textAlign: "center", padding: 20 }}>
                      暂无自定义供应商，点击上方按钮新建
                    </div>
                  )}
                </div>
              </div>

              {/* ── Right: supplier detail editor ── */}
              <div style={{ flex: 1, minWidth: 0, overflowY: "auto" }}>
                {!selectedSupplier && !creating ? (
                  <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100%", color: "var(--text-muted)", fontSize: 13 }}>
                    选择左侧供应商或新建一个
                  </div>
                ) : (
                  <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                    {/* Create-mode: preset selector */}
                    {creating && (
                      <Field label="选择供应商">
                        <select
                          value={createPresetIdx}
                          onChange={(e) => setCreatePresetIdx(Number(e.target.value))}
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
                          <option value={-1}>— 自定义 —</option>
                          {AI_PRESETS.map((p, idx) => (
                            <option key={idx} value={idx}>
                              {p.name}（{PROVIDER_LABELS[p.provider] ?? p.provider}）
                            </option>
                          ))}
                        </select>
                      </Field>
                    )}
                    {/* Basic info */}
                    <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
                      <Field label="供应商名称">
                        <Input
                          value={editName}
                          onChange={setEditName}
                          placeholder="如：GLM、腾讯云"
                        />
                      </Field>
                      <Field label="协议 (Provider)">
                        <select
                          value={editProvider}
                          onChange={(e) => setEditProvider(e.target.value as AiProvider)}
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
                          <option value="openai_compatible">OpenAI 兼容</option>
                          <option value="anthropic_compatible">Anthropic 兼容</option>
                          <option value="claude">Claude (Anthropic 官方)</option>
                          <option value="openai">OpenAI (官方)</option>
                          <option value="ollama">Ollama (本地)</option>
                        </select>
                      </Field>
                    </div>
                    <Field label="API Base URL">
                      <Input
                        value={editBaseUrl}
                        onChange={setEditBaseUrl}
                        placeholder="如 https://open.bigmodel.cn/api/paas/v4"
                      />
                    </Field>
                    <Field label="网络代理（可选：http:// 或 socks5://）">
                      <Input
                        value={editProxy}
                        onChange={setEditProxy}
                        placeholder="留空直连；如 http://127.0.0.1:7890"
                      />
                    </Field>
                    <Field label={`API Key${!creating && selectedSupplier?.hasKey ? "（已保存，留空保持不变）" : ""}`}>
                      <Input
                        value={editKey}
                        onChange={setEditKey}
                        type="password"
                        placeholder={
                          creating
                            ? "粘贴 API key（必填，用于测试和对话）"
                            : selectedSupplier?.hasKey
                            ? "••••••（已保存）"
                            : "粘贴 API key"
                        }
                      />
                    </Field>
                    <Field label={`Temperature（${editTemp}）`}>
                      <input
                        type="range"
                        min={0}
                        max={1}
                        step={0.1}
                        value={editTemp}
                        onChange={(e) => setEditTemp(Number(e.target.value))}
                        style={{ width: "100%" }}
                      />
                    </Field>

                    {/* ── Model management ── */}
                    <div style={{ borderTop: "1px solid var(--border-subtle)", paddingTop: 12 }}>
                      <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text-primary)", marginBottom: 8 }}>
                        模型列表
                      </div>
                      <div style={{ display: "flex", flexDirection: "column", gap: 4, marginBottom: 8 }}>
                        {editModels.length === 0 && (
                          <div style={{ fontSize: 11, color: "var(--text-muted)", padding: "4px 0" }}>
                            暂无模型，请添加或从接口获取
                          </div>
                        )}
                        {editModels.map((model, idx) => (
                          <div
                            key={model.id || idx}
                            style={{
                              display: "flex",
                              alignItems: "center",
                              gap: 6,
                              padding: "5px 8px",
                              background: "var(--bg-surface)",
                              border: "1px solid var(--border-default)",
                              borderRadius: "var(--radius-sm)",
                            }}
                          >
                            <span
                              style={{
                                flex: 1,
                                fontSize: 11,
                                color: "var(--text-primary)",
                                overflow: "hidden",
                                textOverflow: "ellipsis",
                                whiteSpace: "nowrap",
                              }}
                            >
                              {model.label ? `${model.label} (${model.modelId})` : model.modelId}
                            </span>
                            {idx === 0 && (
                              <span style={{ fontSize: 9, color: "var(--text-muted)" }}>主</span>
                            )}
                            <button
                              onClick={() => {
                                if (idx === 0) return; // can't remove primary
                                setEditModels((prev) => prev.filter((_, i) => i !== idx));
                              }}
                              disabled={idx === 0}
                              style={{
                                background: "transparent",
                                border: "none",
                                color: idx === 0 ? "var(--text-muted)" : "var(--error)",
                                fontSize: 14,
                                cursor: idx === 0 ? "default" : "pointer",
                                padding: "0 4px",
                              }}
                              title={idx === 0 ? "主模型不可删除" : "删除"}
                            >
                              ×
                            </button>
                          </div>
                        ))}
                      </div>

                      {/* Fetch models from API */}
                      {(editProvider === "openai_compatible" ||
                        editProvider === "ollama") && (
                        <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8 }}>
                          <button
                            onClick={async () => {
                              if (!editBaseUrl) {
                                showToast("err", "请先填写 Base URL");
                                return;
                              }
                              if (!editKey && !selectedSupplier?.hasKey) {
                                showToast("err", "请先填写 API Key");
                                return;
                              }
                              setFetchingModels(true);
                              setFetchError(null);
                              setFetchedModels([]);
                              try {
                                let models: string[];
                                if (creating || !selectedSupplierId) {
                                  // New supplier not yet saved: use form values directly.
                                  models = await fetchProviderModels(
                                    editProvider,
                                    editBaseUrl,
                                    editKey,
                                  );
                                } else {
                                  // Existing supplier: decrypt key server-side,
                                  // but honor editKey override if user typed a new one.
                                  models = await fetchModelsForSupplier(
                                    selectedSupplierId,
                                    editKey || undefined,
                                  );
                                }
                                setFetchedModels(models);
                                if (models.length === 0) {
                                  showToast("err", "接口返回空模型列表");
                                }
                              } catch (e) {
                                showToast("err", `获取模型失败: ${e}`);
                              } finally {
                                setFetchingModels(false);
                              }
                            }}
                            disabled={fetchingModels || !editBaseUrl}
                            style={{
                              background: "var(--bg-surface)",
                              color: "var(--text-secondary)",
                              border: "1px solid var(--border-default)",
                              borderRadius: "var(--radius-md)",
                              padding: "6px 12px",
                              fontSize: 11,
                              cursor: fetchingModels || !editBaseUrl ? "default" : "pointer",
                              opacity: fetchingModels ? 0.7 : 1,
                            }}
                          >
                            {fetchingModels ? "获取中…" : "从接口获取模型"}
                          </button>
                          {fetchError && (
                            <span style={{ fontSize: 11, color: "var(--error)" }}>
                              {fetchError}
                            </span>
                          )}
                        </div>
                      )}

                      {/* Fetched models — click to add */}
                      {fetchedModels.length > 0 && (
                        <div style={{ marginBottom: 8 }}>
                          <div style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 4 }}>
                            点击添加到列表：
                          </div>
                          <div style={{ display: "flex", flexWrap: "wrap", gap: 4, maxHeight: 100, overflowY: "auto" }}>
                            {fetchedModels.map((mid) => {
                              const alreadyAdded = editModels.some((m) => m.modelId === mid);
                              return (
                                <button
                                  key={mid}
                                  disabled={alreadyAdded}
                                  onClick={() => {
                                    if (!alreadyAdded) {
                                      setEditModels((prev) => [
                                        ...prev,
                                        { id: 0, supplierId: selectedSupplierId ?? 0, modelId: mid, label: undefined, sortOrder: prev.length },
                                      ]);
                                    }
                                  }}
                                  style={{
                                    background: alreadyAdded
                                      ? "var(--bg-input)"
                                      : "var(--accent-primary-muted)",
                                    border: `1px solid ${alreadyAdded ? "var(--border-default)" : "var(--accent-primary)"}`,
                                    borderRadius: "var(--radius-full)",
                                    padding: "3px 10px",
                                    fontSize: 11,
                                    color: alreadyAdded ? "var(--text-muted)" : "var(--text-primary)",
                                    cursor: alreadyAdded ? "default" : "pointer",
                                    opacity: alreadyAdded ? 0.5 : 1,
                                  }}
                                >
                                  {alreadyAdded ? "✓ " : "+ "}{mid}
                                </button>
                              );
                            })}
                          </div>
                        </div>
                      )}

                      {/* Manual add model */}
                      {showAddModel ? (
                        <div style={{ display: "flex", gap: 6, alignItems: "flex-end", marginBottom: 4 }}>
                          <div style={{ flex: 1 }}>
                            <Field label="模型 ID">
                              <Input
                                value={newModelId}
                                onChange={setNewModelId}
                                placeholder="如 glm-4-plus"
                              />
                            </Field>
                          </div>
                          <div style={{ flex: 1 }}>
                            <Field label="显示名（可选）">
                              <Input
                                value={newModelLabel}
                                onChange={setNewModelLabel}
                                placeholder="如 GLM-4 Plus"
                              />
                            </Field>
                          </div>
                          <button
                            onClick={() => {
                              if (!newModelId.trim()) return;
                              setEditModels((prev) => [
                                ...prev,
                                { id: 0, supplierId: selectedSupplierId ?? 0, modelId: newModelId.trim(), label: newModelLabel.trim() || undefined, sortOrder: prev.length },
                              ]);
                              setNewModelId("");
                              setNewModelLabel("");
                              setShowAddModel(false);
                            }}
                            disabled={!newModelId.trim()}
                            style={{
                              background: "var(--accent-primary)",
                              color: "#fff",
                              border: "none",
                              borderRadius: "var(--radius-md)",
                              padding: "8px 12px",
                              fontSize: 12,
                              cursor: newModelId.trim() ? "pointer" : "default",
                              opacity: newModelId.trim() ? 1 : 0.5,
                              marginBottom: 12,
                            }}
                          >
                            添加
                          </button>
                          <button
                            onClick={() => setShowAddModel(false)}
                            style={{
                              background: "transparent",
                              color: "var(--text-secondary)",
                              border: "1px solid var(--border-default)",
                              borderRadius: "var(--radius-md)",
                              padding: "8px 12px",
                              fontSize: 12,
                              cursor: "pointer",
                              marginBottom: 12,
                            }}
                          >
                            取消
                          </button>
                        </div>
                      ) : (
                        <button
                          onClick={() => setShowAddModel(true)}
                          style={{
                            background: "transparent",
                            color: "var(--accent-primary)",
                            border: "1px dashed var(--border-default)",
                            borderRadius: "var(--radius-md)",
                            padding: "6px 12px",
                            fontSize: 11,
                            cursor: "pointer",
                            width: "100%",
                          }}
                        >
                          ＋ 手动添加模型
                        </button>
                      )}
                    </div>

                    {/* ── Actions ── */}
                    <div style={{ display: "flex", alignItems: "center", gap: 10, borderTop: "1px solid var(--border-subtle)", paddingTop: 12 }}>
                      {creating && (
                        <button
                          onClick={() => {
                            setCreating(false);
                            setSelectedSupplierId(null);
                          }}
                          style={{
                            background: "transparent",
                            color: "var(--text-secondary)",
                            border: "1px solid var(--border-default)",
                            borderRadius: "var(--radius-md)",
                            padding: "8px 16px",
                            fontSize: 12,
                            cursor: "pointer",
                          }}
                        >
                          取消
                        </button>
                      )}
                      <button
                        onClick={handleTestSupplier}
                        disabled={editTesting || editSaving}
                        style={{
                          background: "var(--bg-surface)",
                          color: "var(--text-secondary)",
                          border: "1px solid var(--border-default)",
                          borderRadius: "var(--radius-md)",
                          padding: "8px 16px",
                          fontSize: 12,
                          fontWeight: 500,
                          cursor: editTesting ? "default" : "pointer",
                          opacity: editTesting ? 0.7 : 1,
                        }}
                      >
                        {editTesting ? "测试中…" : "测试"}
                      </button>
                      <button
                        onClick={handleSaveSupplier}
                        disabled={editSaving || editTesting}
                        style={{
                          background: "var(--accent-primary)",
                          color: "#fff",
                          border: "none",
                          borderRadius: "var(--radius-md)",
                          padding: "8px 16px",
                          fontSize: 12,
                          fontWeight: 500,
                          cursor: editSaving ? "default" : "pointer",
                          opacity: editSaving ? 0.7 : 1,
                        }}
                      >
                        {editSaving ? "保存中…" : creating ? "创建" : "保存"}
                      </button>
                      {!creating && selectedSupplierId && (
                        <button
                          onClick={async () => {
                            if (!selectedSupplier) return;
                            const ok = await confirm(`删除供应商「${selectedSupplier.name}」？`, { title: "确认删除", kind: "warning" });
                            if (ok) {
                              try {
                                const wasActive = aiActiveId === selectedSupplier.id;
                                const { deleteAiModel } = await import("../api");
                                await deleteAiModel(selectedSupplier.id);
                                await reloadAi();
                                // If the deleted supplier was active, auto-select
                                // the first remaining enabled user supplier.
                                if (wasActive) {
                                  const remaining = aiModels.filter(
                                    (m) => m.id !== selectedSupplier.id && m.isEnabled && !m.isPreset,
                                  );
                                  if (remaining.length > 0) {
                                    await setActiveAiModelHook(remaining[0].id);
                                  }
                                }
                                setSelectedSupplierId(null);
                                showToast("ok", "供应商已删除");
                              } catch (e) {
                                showToast("err", `删除失败: ${e}`);
                              }
                            }
                          }}
                          style={{
                            background: "transparent",
                            color: "var(--error, #ff3b30)",
                            border: "1px solid var(--error, #ff3b30)",
                            borderRadius: "var(--radius-md)",
                            padding: "8px 16px",
                            fontSize: 12,
                            cursor: "pointer",
                          }}
                        >
                          删除
                        </button>
                      )}
                    </div>
                  </div>
                )}
              </div>
            </div>
          </Section>
            </>
          )}

          {/* MCP Support — AI 工具集成 */}
          {activeCategory === "mcp" && (
            <>
              <Section title="MCP 服务">
                <div style={{ marginBottom: 12 }}>
                  <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                    <div>
                      <div style={{ fontSize: 13, fontWeight: 500 }}>启用 MCP 支持</div>
                      <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 2 }}>
                        允许 AI 工具（Claude / Opencode / Zcode）通过 MyShell 操作远程服务器
                      </div>
                    </div>
                    <Toggle
                      checked={mcpEnabled}
                      onChange={async (next) => {
                        setMcpEnabled(next);
                        if (!next) {
                          // Disable: remove configs from all tools
                          const tools = await mcpDetectTools();
                          for (const t of tools) {
                            if (t.configured) {
                              await mcpRemoveConfig(t.id).catch(() => {});
                            }
                          }
                          setMcpTools(await mcpDetectTools());
                          showToast("ok", "已禁用 MCP 支持");
                        } else {
                          showToast("ok", "已启用 MCP 支持，请配置下方密码");
                        }
                      }}
                    />
                  </div>
                </div>

                {mcpEnabled && (
                  <>

                    {/* Attachment Directory — where screenshots are auto-saved.
                        First time the user opens MCP settings without this
                        configured, show a warning banner pointing at the picker. */}
                    <div style={{ marginBottom: 12 }}>
                      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 8 }}>
                        <div style={{ fontSize: 12, fontWeight: 500 }}>📎 附件目录</div>
                        <div style={{ display: "flex", gap: 6 }}>
                          {attachmentDir && (
                            <button
                              onClick={() => attachmentDir && showInFolder(attachmentDir)}
                              style={{
                                padding: "4px 10px",
                                borderRadius: 6,
                                border: "1px solid var(--border)",
                                background: "var(--bg-input)",
                                color: "var(--text-secondary)",
                                fontSize: 11,
                                cursor: "pointer",
                              }}
                            >
                              打开目录
                            </button>
                          )}
                          <button
                            onClick={pickAttachmentDir}
                            disabled={attachmentDirPicking}
                            style={{
                              padding: "4px 10px",
                              borderRadius: 6,
                              border: "1px solid var(--accent-primary)",
                              background: "var(--accent-primary)",
                              color: "white",
                              fontSize: 11,
                              cursor: attachmentDirPicking ? "wait" : "pointer",
                              opacity: attachmentDirPicking ? 0.6 : 1,
                            }}
                          >
                            {attachmentDirPicking ? "选择中..." : attachmentDir ? "更改目录" : "选择目录"}
                          </button>
                        </div>
                      </div>

                      {/* First-time prompt: user hasn't set a dir AND hasn't
                          acknowledged the warning yet. Banner invites them to
                          configure. Once acknowledged, we don't nag again even
                          if they unset it later. */}
                      {!attachmentDir && !attachmentDirAcknowledged && (
                        <div style={{
                          padding: "8px 10px",
                          borderRadius: 6,
                          background: "var(--warning-bg, rgba(255, 200, 0, 0.08))",
                          border: "1px solid var(--warning, #f0ad4e)",
                          fontSize: 11,
                          color: "var(--text-secondary)",
                          marginBottom: 8,
                        }}>
                          ⚠️ 尚未配置附件目录。终端截图功能（CommandBar 的 📷 按钮）会保存到这里，请先选择一个目录。
                        </div>
                      )}

                      {attachmentDir ? (
                        <div style={{
                          padding: "8px 10px",
                          borderRadius: 6,
                          background: "var(--bg-input)",
                          border: "1px solid var(--border)",
                          fontSize: 11,
                          color: "var(--text-secondary)",
                          fontFamily: "monospace",
                          wordBreak: "break-all",
                        }}>
                          {attachmentDir}
                        </div>
                      ) : (
                        <div style={{ fontSize: 11, color: "var(--text-muted)" }}>
                          未配置 — 截图按钮点击后会提示先来此配置
                        </div>
                      )}
                      <div style={{ fontSize: 10, color: "var(--text-muted)", marginTop: 6 }}>
                        终端截图文件名为「截图_&lt;连接名&gt;_&lt;时间戳&gt;.png」，自动保存到此目录。
                      </div>
                    </div>

                    {/* Command Confirmation Rules — whitelist/blacklist regex
                        controlling which ssh_exec commands skip the OS dialog. */}
                    <div style={{ marginBottom: 12 }}>
                      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 8 }}>
                        <div style={{ fontSize: 12, fontWeight: 500 }}>🛡️ 命令确认规则</div>
                        <div style={{ display: "flex", gap: 6 }}>
                          <button
                            onClick={() => {
                              // Reset to defaults (fetch a fresh default by clearing then reloading).
                              // The simplest way: write the empty object so the backend returns defaults.
                              setRulesBlacklist([]);
                              setRulesWhitelist([]);
                              setRulesConfirmUnknown(false);
                            }}
                            style={{
                              padding: "4px 10px", borderRadius: 6,
                              border: "1px solid var(--border)",
                              background: "var(--bg-input)",
                              color: "var(--text-secondary)",
                              fontSize: 11, cursor: "pointer",
                            }}
                          >
                            清空编辑
                          </button>
                          <button
                            onClick={saveCommandRules}
                            disabled={rulesSaving}
                            style={{
                              padding: "4px 10px", borderRadius: 6,
                              border: "1px solid var(--accent-primary)",
                              background: "var(--accent-primary)",
                              color: "white", fontSize: 11,
                              cursor: rulesSaving ? "wait" : "pointer",
                              opacity: rulesSaving ? 0.6 : 1,
                            }}
                          >
                            {rulesSaving ? "保存中..." : "保存规则"}
                          </button>
                        </div>
                      </div>

                      <div style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 8 }}>
                        控制 <code style={{ fontFamily: "monospace" }}>ssh_exec</code> 执行哪些命令时弹人工确认对话框。每行一条正则表达式（大小写不敏感）。
                      </div>

                      {/* confirm_unknown toggle */}
                      <label style={{
                        display: "flex", alignItems: "center", gap: 6,
                        fontSize: 11, color: "var(--text-secondary)",
                        marginBottom: 10, cursor: "pointer",
                      }}>
                        <input
                          type="checkbox"
                          checked={rulesConfirmUnknown}
                          onChange={(e) => setRulesConfirmUnknown(e.target.checked)}
                        />
                        <span>未匹配任何规则的命令也需要确认（严格模式，默认关闭）</span>
                      </label>

                      {/* show_in_gui toggle */}
                      <label style={{
                        display: "flex", alignItems: "center", gap: 6,
                        fontSize: 11, color: "var(--text-secondary)",
                        marginBottom: 10, cursor: "pointer",
                      }}>
                        <input
                          type="checkbox"
                          checked={rulesShowInGui}
                          onChange={(e) => setRulesShowInGui(e.target.checked)}
                        />
                        <span>ssh_exec 命令在界面终端同步展示（关闭则后台静默执行，默认开启）</span>
                      </label>

                      {/* Blacklist — searchable sorted list */}
                      <div style={{ marginBottom: 8 }}>
                        <div style={{ fontSize: 12, fontWeight: 500, color: "var(--error)", marginBottom: 4 }}>
                          ⛔ 黑名单（命中则确认，除非白名单豁免）
                        </div>
                        <div style={{ display: "flex", gap: 4, marginBottom: 4 }}>
                          <input
                            type="text"
                            value={blacklistInput}
                            onChange={(e) => setBlacklistInput(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter" && blacklistInput.trim()) {
                                const v = blacklistInput.trim();
                                if (!rulesBlacklist.includes(v)) {
                                  setRulesBlacklist([...rulesBlacklist, v].sort());
                                }
                                setBlacklistInput("");
                              }
                            }}
                            placeholder="输入正则后回车添加…"
                            style={{
                              flex: 1, fontSize: 13, fontFamily: "monospace",
                              background: "var(--bg-input)",
                              border: "1px solid var(--border)",
                              borderRadius: 6, padding: "5px 8px",
                              color: "var(--text-primary)",
                            }}
                          />
                          <button
                            style={{
                              padding: "4px 12px", borderRadius: 6, fontSize: 12,
                              border: "1px solid var(--border)",
                              background: "var(--bg-input)", color: "var(--text)",
                              cursor: "pointer", flexShrink: 0,
                            }}
                            onClick={() => {
                              const v = blacklistInput.trim();
                              if (v && !rulesBlacklist.includes(v)) {
                                setRulesBlacklist([...rulesBlacklist, v].sort());
                              }
                              setBlacklistInput("");
                            }}
                          >添加</button>
                        </div>
                        {rulesBlacklist.length > 3 && (
                          <input
                            type="text"
                            value={blacklistSearch}
                            onChange={(e) => setBlacklistSearch(e.target.value)}
                            placeholder="🔍 搜索…"
                            style={{
                              width: "100%", fontSize: 12,
                              background: "var(--bg-input)",
                              border: "1px solid var(--border)",
                              borderRadius: 6, padding: "4px 8px", marginBottom: 4,
                              color: "var(--text-primary)",
                            }}
                          />
                        )}
                        <div style={{ maxHeight: 200, overflowY: "auto", border: "1px solid var(--border)", borderRadius: 6, background: "var(--bg-input)" }}>
                          {rulesBlacklist
                            .filter((r) => !blacklistSearch || r.toLowerCase().includes(blacklistSearch.toLowerCase()))
                            .map((rule, i) => (
                              <div key={rule} style={{
                                display: "flex", alignItems: "center", gap: 6,
                                padding: "4px 8px", borderBottom: i < rulesBlacklist.length - 1 ? "1px solid var(--border)" : "none",
                              }}>
                                <span style={{ fontSize: 13, fontFamily: "monospace", color: "var(--error)", flex: 1, wordBreak: "break-all" }}>
                                  {rule}
                                </span>
                                <button
                                  style={{
                                    border: "none", background: "transparent",
                                    color: "var(--text-muted)", cursor: "pointer",
                                    fontSize: 14, padding: "0 4px", flexShrink: 0,
                                  }}
                                  onClick={() => setRulesBlacklist(rulesBlacklist.filter((r) => r !== rule))}
                                  title="删除"
                                >✕</button>
                              </div>
                            ))}
                          {rulesBlacklist.length === 0 && (
                            <div style={{ padding: "8px", fontSize: 12, color: "var(--text-muted)", textAlign: "center" }}>
                              暂无规则
                            </div>
                          )}
                        </div>
                        <div style={{ fontSize: 10, color: "var(--text-muted)", marginTop: 2 }}>
                          共 {rulesBlacklist.length} 条，自动按字母排序
                        </div>
                      </div>

                      {/* Whitelist — searchable sorted list */}
                      <div style={{ marginBottom: 8 }}>
                        <div style={{ fontSize: 12, fontWeight: 500, color: "var(--success)", marginBottom: 4 }}>
                          ✅ 白名单豁免（命中则免确认，优先级高于黑名单）
                        </div>
                        <div style={{ display: "flex", gap: 4, marginBottom: 4 }}>
                          <input
                            type="text"
                            value={whitelistInput}
                            onChange={(e) => setWhitelistInput(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter" && whitelistInput.trim()) {
                                const v = whitelistInput.trim();
                                if (!rulesWhitelist.includes(v)) {
                                  setRulesWhitelist([...rulesWhitelist, v].sort());
                                }
                                setWhitelistInput("");
                              }
                            }}
                            placeholder="输入正则后回车添加…"
                            style={{
                              flex: 1, fontSize: 13, fontFamily: "monospace",
                              background: "var(--bg-input)",
                              border: "1px solid var(--border)",
                              borderRadius: 6, padding: "5px 8px",
                              color: "var(--text-primary)",
                            }}
                          />
                          <button
                            style={{
                              padding: "4px 12px", borderRadius: 6, fontSize: 12,
                              border: "1px solid var(--border)",
                              background: "var(--bg-input)", color: "var(--text)",
                              cursor: "pointer", flexShrink: 0,
                            }}
                            onClick={() => {
                              const v = whitelistInput.trim();
                              if (v && !rulesWhitelist.includes(v)) {
                                setRulesWhitelist([...rulesWhitelist, v].sort());
                              }
                              setWhitelistInput("");
                            }}
                          >添加</button>
                        </div>
                        {rulesWhitelist.length > 3 && (
                          <input
                            type="text"
                            value={whitelistSearch}
                            onChange={(e) => setWhitelistSearch(e.target.value)}
                            placeholder="🔍 搜索…"
                            style={{
                              width: "100%", fontSize: 12,
                              background: "var(--bg-input)",
                              border: "1px solid var(--border)",
                              borderRadius: 6, padding: "4px 8px", marginBottom: 4,
                              color: "var(--text-primary)",
                            }}
                          />
                        )}
                        <div style={{ maxHeight: 150, overflowY: "auto", border: "1px solid var(--border)", borderRadius: 6, background: "var(--bg-input)" }}>
                          {rulesWhitelist
                            .filter((r) => !whitelistSearch || r.toLowerCase().includes(whitelistSearch.toLowerCase()))
                            .map((rule, i) => (
                              <div key={rule} style={{
                                display: "flex", alignItems: "center", gap: 6,
                                padding: "4px 8px", borderBottom: i < rulesWhitelist.length - 1 ? "1px solid var(--border)" : "none",
                              }}>
                                <span style={{ fontSize: 13, fontFamily: "monospace", color: "var(--success)", flex: 1, wordBreak: "break-all" }}>
                                  {rule}
                                </span>
                                <button
                                  style={{
                                    border: "none", background: "transparent",
                                    color: "var(--text-muted)", cursor: "pointer",
                                    fontSize: 14, padding: "0 4px", flexShrink: 0,
                                  }}
                                  onClick={() => setRulesWhitelist(rulesWhitelist.filter((r) => r !== rule))}
                                  title="删除"
                                >✕</button>
                              </div>
                            ))}
                          {rulesWhitelist.length === 0 && (
                            <div style={{ padding: "8px", fontSize: 12, color: "var(--text-muted)", textAlign: "center" }}>
                              暂无规则
                            </div>
                          )}
                        </div>
                        <div style={{ fontSize: 10, color: "var(--text-muted)", marginTop: 2 }}>
                          共 {rulesWhitelist.length} 条，自动按字母排序
                        </div>
                      </div>

                      <div style={{ fontSize: 10, color: "var(--text-muted)", marginTop: 4, lineHeight: 1.6 }}>
                        判定顺序：① 命令替换 / 写重定向 / 管道到 shell → 始终确认（不可配置）；
                        ② 黑名单命中且白名单未命中 → 确认；③ 黑名单未命中 → 放行（除非开启严格模式）。
                        空配置文件自动使用内置默认规则。
                      </div>
                    </div>

                    {/* AI Tool Configuration */}
                    <div style={{ marginBottom: 12 }}>
                      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 8 }}>
                        <div style={{ fontSize: 12, fontWeight: 500 }}>已安装的 AI 工具</div>
                        <button
                          style={{
                            padding: "4px 10px",
                            borderRadius: 6,
                            border: "1px solid var(--border)",
                            background: "transparent",
                            color: "var(--text)",
                            fontSize: 11,
                            cursor: mcpConfiguring ? "default" : "pointer",
                            opacity: mcpConfiguring ? 0.6 : 1,
                          }}
                          disabled={mcpConfiguring}
                          onClick={async () => {
                            setMcpConfiguring(true);
                            try {
                              const tools = await mcpDetectTools();
                              let configured = 0;
                              let skipped = 0;
                              for (const tool of tools) {
                                if (!tool.installed) continue;
                                if (tool.configured) { skipped++; continue; }
                                const written = await mcpWriteConfig(tool.id);
                                if (written) configured++;
                              }
                              setMcpTools(await mcpDetectTools());
                              showToast("ok", `配置完成：新增 ${configured} 个，跳过 ${skipped} 个已配置`);
                            } catch (e: any) {
                              showToast("err", `配置失败: ${e}`);
                            } finally {
                              setMcpConfiguring(false);
                            }
                          }}
                        >
                          {mcpConfiguring ? "配置中…" : "一键配置全部"}
                        </button>
                      </div>

                      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                        {mcpTools.map((tool) => (
                          <div
                            key={tool.id}
                            style={{
                              display: "flex",
                              alignItems: "center",
                              justifyContent: "space-between",
                              padding: "8px 12px",
                              borderRadius: 6,
                              background: "var(--bg-secondary)",
                              border: "1px solid var(--border)",
                            }}
                          >
                            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                              <span style={{ fontSize: 14 }}>
                                {tool.id === "claude" ? "🤖" : tool.id === "opencode" ? "⚡" : "🔧"}
                              </span>
                              <div>
                                <div style={{ fontSize: 12, fontWeight: 500 }}>{tool.name}</div>
                                <div style={{ fontSize: 10, color: "var(--text-muted)" }}>
                                  {tool.installed ? (
                                    tool.configured ? (
                                      <span style={{ color: "var(--success)" }}>✓ 已配置 MCP</span>
                                    ) : (
                                      <span>已安装，未配置</span>
                                    )
                                  ) : (
                                    <span>未检测到</span>
                                  )}
                                </div>
                              </div>
                            </div>
                            <div style={{ display: "flex", gap: 6 }}>
                              {tool.installed && !tool.configured && (
                                <button
                                  style={{
                                    padding: "4px 10px",
                                    borderRadius: 4,
                                    border: "1px solid var(--accent)",
                                    background: "var(--accent)",
                                    color: "#fff",
                                    fontSize: 11,
                                    cursor: "pointer",
                                  }}
                                  onClick={async () => {
                                    try {
                                      const written = await mcpWriteConfig(tool.id);
                                      setMcpTools(await mcpDetectTools());
                                      showToast("ok", written ? `已为 ${tool.name} 配置 MCP` : "已存在，跳过");
                                    } catch (e: any) {
                                      showToast("err", `配置失败: ${e}`);
                                    }
                                  }}
                                >
                                  配置
                                </button>
                              )}
                              {tool.configured && (
                                <button
                                  style={{
                                    padding: "4px 10px",
                                    borderRadius: 4,
                                    border: "1px solid var(--error)",
                                    background: "transparent",
                                    color: "var(--error)",
                                    fontSize: 11,
                                    cursor: "pointer",
                                  }}
                                  onClick={async () => {
                                    try {
                                      await mcpRemoveConfig(tool.id);
                                      setMcpTools(await mcpDetectTools());
                                      showToast("ok", `已从 ${tool.name} 移除 MCP 配置`);
                                    } catch (e: any) {
                                      showToast("err", `移除失败: ${e}`);
                                    }
                                  }}
                                >
                                  移除
                                </button>
                              )}
                            </div>
                          </div>
                        ))}
                      </div>

                      <div style={{
                        marginTop: 10,
                        padding: 10,
                        borderRadius: 6,
                        background: "var(--bg-secondary)",
                        border: "1px solid var(--border)",
                        fontSize: 11,
                        color: "var(--text-muted)",
                        lineHeight: 1.5,
                      }}>
                        <strong>其他 AI 工具？</strong>只需在 MCP 配置文件中添加如下 server：<br />
                        <code style={{ fontSize: 10, background: "var(--bg-primary)", padding: "2px 4px", borderRadius: 3 }}>
                          {'{"command": "…/myshell-mcp.exe"}'}
                        </code><br />
                        Claude Desktop → .claude/mcp.json | Cursor → .cursor/mcp.json | 其他工具可参考其 MCP 配置文档。
                      </div>
                    </div>
                  </>
                )}
              </Section>
            </>
          )}

          {/* Security Category — 管理员权限 / 修改登录密码 */}
          {activeCategory === "security" && (
            <>
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

          {/* Auto-lock Section */}
          <Section title="🔒 自动锁定">
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14, lineHeight: 1.6 }}>
              无操作超过指定时间后自动锁定应用，需重新输入密码解锁。「不启用」则首次解锁后不再自动锁定（重启后恢复）。
            </div>
            <Field label="空闲锁定时长">
              <select
                value={localStorage.getItem("myshell-auto-lock-minutes") ?? "30"}
                onChange={(e) => {
                  localStorage.setItem("myshell-auto-lock-minutes", e.target.value);
                  window.dispatchEvent(new Event("myshell-auto-lock-changed"));
                  // Force re-render so the select shows the new value.
                  setAutoLockTick((t) => t + 1);
                }}
                style={{
                  background: "var(--bg-surface)",
                  color: "var(--text-primary)",
                  border: "1px solid var(--border)",
                  borderRadius: "var(--radius)",
                  padding: "6px 10px",
                  fontSize: 13,
                  outline: "none",
                  cursor: "pointer",
                }}
              >
                <option value="10">10 分钟</option>
                <option value="30">30 分钟（默认）</option>
                <option value="60">1 小时</option>
                <option value="0">不启用</option>
              </select>
            </Field>
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
            </>
          )}

          {/* Data Category — 配置导入导出 / 版本备份与回退 */}
          {activeCategory === "multiWindow" && (
          <>
            <Section title="🪟 多窗口网格">
              <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14, lineHeight: 1.6 }}>
                设置多窗口模式的默认网格行列数。进入多窗口时自动按此布局排列，并最大化窗口。
              </div>
              <div style={{ display: "flex", gap: 16, alignItems: "flex-end" }}>
                <Field label="行数">
                  <select
                    value={localStorage.getItem("myshell-multiwindow-rows") ?? "2"}
                    onChange={(e) => {
                      localStorage.setItem("myshell-multiwindow-rows", e.target.value);
                      window.dispatchEvent(new Event("myshell-multiwindow-changed"));
                      setAutoLockTick((t) => t + 1);
                    }}
                    style={{ background: "var(--bg-surface)", color: "var(--text-primary)", border: "1px solid var(--border)", borderRadius: "var(--radius)", padding: "6px 10px", fontSize: 13, outline: "none", cursor: "pointer" }}
                  >
                    <option value="1">1 行</option>
                    <option value="2">2 行（默认）</option>
                    <option value="3">3 行</option>
                    <option value="4">4 行</option>
                  </select>
                </Field>
                <Field label="列数">
                  <select
                    value={localStorage.getItem("myshell-multiwindow-cols") ?? "3"}
                    onChange={(e) => {
                      localStorage.setItem("myshell-multiwindow-cols", e.target.value);
                      window.dispatchEvent(new Event("myshell-multiwindow-changed"));
                      setAutoLockTick((t) => t + 1);
                    }}
                    style={{ background: "var(--bg-surface)", color: "var(--text-primary)", border: "1px solid var(--border)", borderRadius: "var(--radius)", padding: "6px 10px", fontSize: 13, outline: "none", cursor: "pointer" }}
                  >
                    <option value="1">1 列</option>
                    <option value="2">2 列</option>
                    <option value="3">3 列（默认）</option>
                    <option value="4">4 列</option>
                    <option value="5">5 列</option>
                    <option value="6">6 列</option>
                  </select>
                </Field>
              </div>
              <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginTop: 10, lineHeight: 1.5 }}>
                默认 2 行 × 3 列，最多同时展示 6 个窗口。超出部分可向下滚动查看。修改后下次进入多窗口生效。
              </div>
            </Section>
          </>
        )}

        {/* Transfer Category — SFTP 下载并发 */}
        {activeCategory === "transfer" && (
          <>
            <Section title="📤 SFTP 下载">
              <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14, lineHeight: 1.6 }}>
                勾选的文件夹会递归下载整个子树（含空目录）；多文件同时传输时以下发线程数并行拉取，对大量小文件提速明显。
              </div>
              <Field label="并发下载线程数">
                <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                  {[1, 2, 3, 4, 6, 8, 12, 16].map((n) => {
                    const active = sftpConcurrency === n;
                    return (
                      <button
                        key={n}
                        title={n === DEFAULT_SFTP_CONCURRENCY ? "默认" : undefined}
                        onClick={() => setSftpConcurrency(n)}
                        style={{
                          minWidth: 44,
                          padding: "8px 10px",
                          background: active ? "var(--accent-primary-muted)" : "var(--bg-input)",
                          color: active ? "var(--accent-primary)" : "var(--text-secondary)",
                          border: `1px solid ${active ? "var(--accent-primary)" : "var(--border-default)"}`,
                          borderRadius: "var(--radius-md)",
                          fontSize: 12,
                          fontWeight: active ? 600 : 400,
                          cursor: "pointer",
                          transition: "all var(--duration-fast) var(--ease-in-out)",
                        }}
                      >
                        {n}
                        {n === DEFAULT_SFTP_CONCURRENCY ? "（默认）" : ""}
                      </button>
                    );
                  })}
                </div>
              </Field>
              <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginTop: 10, lineHeight: 1.5 }}>
                线程数决定同时下载的文件数（单一文件不受影响）。所有线程复用同一条 SSH 连接，过高线程数对高延迟链路收益有限、部分服务器可能限制并发句柄。修改对下一次下载立即生效。ZMODEM（终端 rz/sz）为协议单流串行传输，不适用此设置；终端里用 `sz -r 目录` 可递归下载文件夹。
              </div>
            </Section>
          </>
        )}

        {activeCategory === "data" && (
            <>
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

            {/* Recycle bin entry — restore or purge soft-deleted connections. */}
            <button
              onClick={() => setShowRecycleDialog(true)}
              style={{
                width: "100%",
                marginTop: 12,
                padding: "11px 16px",
                background: "var(--bg-surface)",
                color: "var(--text-secondary)",
                border: "1px solid var(--border-default)",
                borderRadius: "var(--radius-md)",
                fontSize: 13,
                fontWeight: 500,
                cursor: "pointer",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                gap: 8,
                transition: "all var(--duration-fast) var(--ease-in-out)",
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = "var(--bg-surface-hover)";
                e.currentTarget.style.borderColor = "var(--border-emphasis)";
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = "var(--bg-surface)";
                e.currentTarget.style.borderColor = "var(--border-default)";
              }}
            >
              🗑️ 找回连接 / 回收站
            </button>
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
            </>
          )}

          {/* Quick Commands Category — 全局与服务器专属快捷命令入口 */}
          {activeCategory === "quickCommands" && (
            <>
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

          <Divider />

          {/* 行间延迟 — 多行快捷命令逐行发送的间隔控制，避免下一条命令在交互
              提示（sudo/mysql 密码）就绪前被送出。三档模式 + ##delay:N 行内指令。 */}
          {(() => {
            const selectStyle = {
              background: "var(--bg-surface)",
              color: "var(--text-primary)",
              border: "1px solid var(--border)",
              borderRadius: "var(--radius)",
              padding: "6px 10px",
              fontSize: 13,
              outline: "none",
              cursor: "pointer",
            };
            const codeStyle = {
              background: "var(--bg-input)",
              padding: "1px 5px",
              borderRadius: 3,
              fontFamily: "'Cascadia Code','Fira Code',monospace",
              fontSize: 12,
            };
            // Effective mode: explicit key, else migrate from the old delay-ms
            // value (>0 ⇒ fixed). Re-read each render (forced via setAutoLockTick).
            const qcMode =
              localStorage.getItem("myshell-quick-command-mode") ||
              (Number(localStorage.getItem("myshell-quick-command-line-delay-ms")) > 0
                ? "fixed"
                : "off");
            const qcMs = localStorage.getItem("myshell-quick-command-line-delay-ms") || "300";
            return (
              <Section title="⏱ 行间延迟">
                <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14, lineHeight: 1.6 }}>
                  多行快捷命令逐行发送时的间隔策略。<b>智能等待</b>监听终端输出，等上一行输出静止后再发下一行（推荐，自动适配命令速度，能处理密码提示）；<b>固定延迟</b>每行固定等待；<b>关闭</b>一次性全部发出。
                  <br />
                  需在某处精确等待时，可在命令里单独写一行 <code style={codeStyle}>##delay:800</code>（毫秒）或 <code style={codeStyle}>##delay:1s</code>（秒），作为最小等待下限（与模式叠加生效）。
                </div>
                <Field label="等待模式">
                  <select
                    value={qcMode}
                    onChange={(e) => {
                      localStorage.setItem("myshell-quick-command-mode", e.target.value);
                      setAutoLockTick((t) => t + 1);
                    }}
                    style={selectStyle}
                  >
                    <option value="off">关闭（一次性发出）</option>
                    <option value="fixed">固定延迟</option>
                    <option value="idle">智能等待（推荐）</option>
                  </select>
                </Field>
                {qcMode !== "off" && (
                  <Field label={qcMode === "idle" ? "静止判定时长" : "固定延迟时长"}>
                    <select
                      value={qcMs}
                      onChange={(e) => {
                        localStorage.setItem("myshell-quick-command-line-delay-ms", e.target.value);
                        setAutoLockTick((t) => t + 1);
                      }}
                      style={selectStyle}
                    >
                      <option value="100">100 毫秒</option>
                      <option value="300">300 毫秒</option>
                      <option value="500">500 毫秒（推荐）</option>
                      <option value="1000">1 秒</option>
                      <option value="2000">2 秒</option>
                    </select>
                  </Field>
                )}
                {qcMode === "idle" && (
                  <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 4, lineHeight: 1.5 }}>
                    ⚠ 基于"输出静止"判定：对断续输出的命令（如 apt install 有较长停顿）可能略早发送下一条；可调大静止时长，或在该处用 ##delay:N 兜底。
                  </div>
                )}
              </Section>
            );
          })()}
            </>
          )}
          </div>
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
      {showRecycleDialog && (
        <RecycleDialog
          onChanged={onRefresh}
          onClose={() => setShowRecycleDialog(false)}
        />
      )}
      {/* In-app toast for AI settings feedback */}
      {toast && (
        <div
          style={{
            position: "fixed",
            top: 30,
            left: "50%",
            transform: "translateX(-50%)",
            zIndex: 3000,
            padding: "12px 20px",
            borderRadius: "var(--radius-md)",
            background: toast.kind === "ok" ? "var(--success-muted)" : "var(--error-muted)",
            border: `1px solid ${toast.kind === "ok" ? "var(--success)" : "var(--error)"}`,
            color: toast.kind === "ok" ? "var(--success)" : "var(--error)",
            fontSize: 13,
            fontWeight: 500,
            boxShadow: "var(--shadow-lg)",
            maxWidth: 460,
            lineHeight: 1.5,
            animation: "animate-slide-up 0.3s var(--ease-out-expo)",
          }}
        >
          {toast.kind === "ok" ? "✓ " : "⚠ "}{toast.text}
        </div>
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

/**
 * Compact on/off switch built on the design-system CSS variables. Used for the
 * GPU-acceleration toggle. `onChange` is async so the caller can persist via a
 * Tauri command before the UI settles; the switch optimistically reflects the
 * new state immediately.
 */
function Toggle({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (next: boolean) => void | Promise<void>;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      style={{
        flexShrink: 0,
        width: 40,
        height: 22,
        padding: 2,
        background: checked ? "var(--accent-primary)" : "var(--bg-surface-active)",
        border: "none",
        borderRadius: "var(--radius-full)",
        cursor: "pointer",
        transition: "background var(--duration-fast) var(--ease-in-out)",
        display: "flex",
        alignItems: "center",
      }}
    >
      <span
        style={{
          width: 18,
          height: 18,
          background: "#ffffff",
          borderRadius: "50%",
          transform: checked ? "translateX(18px)" : "translateX(0)",
          transition: "transform var(--duration-fast) var(--ease-in-out)",
          boxShadow: "var(--shadow-sm)",
        }}
      />
    </button>
  );
}

function Field({ label, required, children }: { label: string; required?: boolean; children: React.ReactNode }) {
  return (
    <div style={{ marginBottom: 12 }}>
      <label style={{ display: "block", fontSize: 12, color: "var(--text-secondary)", marginBottom: 6, fontWeight: 500 }}>
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
  inputRef,
  autoFocus,
  error,
  invalid,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: string;
  inputRef?: React.RefObject<HTMLInputElement>;
  autoFocus?: boolean;
  error?: string;
  /** Red border/glow WITHOUT a trailing message — use when the error text is
   * rendered elsewhere (e.g. beside a color picker). `error` is the older
   * "red border + inline message" combo; pick one per field. */
  invalid?: boolean;
}) {
  const bad = !!error || !!invalid;
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
          e.currentTarget.style.borderColor = bad ? "var(--error)" : "var(--accent-primary)";
          e.currentTarget.style.boxShadow = bad
            ? "0 0 0 3px var(--error-muted)"
            : "0 0 0 3px var(--accent-primary-muted)";
        }}
        onBlur={(e) => {
          e.currentTarget.style.borderColor = bad ? "var(--error)" : "var(--border-default)";
          e.currentTarget.style.boxShadow = "none";
        }}
        style={{
          width: "100%",
          padding: "10px 12px",
          background: "var(--bg-input)",
          color: "var(--text-primary)",
          border: `1px solid ${bad ? "var(--error)" : "var(--border-default)"}`,
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
  // Validate the hex inputs — the color picker always produces valid #RRGGBB,
  // but the free-text Input next to it can hold anything. Rejecting invalid
  // hex here prevents silently baking broken CSS colors into the saved theme.
  const isValidHex = (h: string) => /^#[0-9a-fA-F]{3}([0-9a-fA-F]{3})?$/.test(h.trim());
  const accentValid = isValidHex(accent);
  const bgValid = isValidHex(bg);
  const canSave = accentValid && bgValid;
  return (
    <Dialog title="自定义主题" icon="🎨" onClose={onClose}>
      <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14 }}>
        选择主题色和终端背景色，保存为自定义主题
      </div>

      <Field label="主题色" required>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <input
            type="color"
            value={accentValid ? accent : "#6366f1"}
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
            invalid={!accentValid}
          />
        </div>
        {!accentValid && (
          <div style={{ fontSize: 11, color: "var(--error)", marginTop: 4 }}>
            请输入合法的颜色值，如 #6366f1 或 #f1f
          </div>
        )}
      </Field>

      <Field label="终端背景色" required>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <input
            type="color"
            value={bgValid ? bg : "#1e1e2e"}
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
            invalid={!bgValid}
          />
        </div>
        {!bgValid && (
          <div style={{ fontSize: 11, color: "var(--error)", marginTop: 4 }}>
            请输入合法的颜色值，如 #1e1e2e 或 #222
          </div>
        )}
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
          disabled={!canSave}
          title={canSave ? undefined : "请输入合法的主题色和背景色"}
          style={{
            padding: "10px 24px",
            background: canSave ? "var(--accent-primary)" : "var(--bg-surface-hover)",
            color: canSave ? "white" : "var(--text-muted)",
            border: "none",
            borderRadius: "var(--radius-md)",
            fontSize: 13,
            fontWeight: 600,
            cursor: canSave ? "pointer" : "not-allowed",
            boxShadow: canSave ? "var(--shadow-glow)" : "none",
            transition: "all var(--duration-fast) var(--ease-in-out)",
          }}
        >
          保存
        </button>
      </div>
    </Dialog>
  );
}

function SettingsNavigation({
  activeCategory,
  onCategoryChange,
}: {
  activeCategory: string;
  onCategoryChange: (category: string) => void;
}) {
  return (
    <div
      style={{
        width: 200,
        borderRight: "1px solid var(--border-subtle)",
        background: "var(--bg-surface)",
        padding: "16px 0",
        display: "flex",
        flexDirection: "column",
        gap: 2,
        overflowY: "auto",
      }}
    >
      {categories.map((category) => {
        const isActive = activeCategory === category.id;
        return (
          <button
            key={category.id}
            onClick={() => onCategoryChange(category.id)}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 10,
              padding: "10px 16px",
              margin: "0 8px",
              background: isActive ? "var(--accent-primary-muted)" : "transparent",
              border: "none",
              borderRadius: "var(--radius-md)",
              cursor: "pointer",
              transition: "all var(--duration-fast) var(--ease-in-out)",
              textAlign: "left",
              width: "calc(100% - 16px)",
            }}
            onMouseEnter={(e) => {
              if (!isActive) {
                e.currentTarget.style.background = "var(--bg-surface-hover)";
              }
            }}
            onMouseLeave={(e) => {
              if (!isActive) {
                e.currentTarget.style.background = "transparent";
              }
            }}
          >
            <span style={{ fontSize: 16, width: 20, textAlign: "center" }}>
              {category.icon}
            </span>
            <span
              style={{
                fontSize: 13,
                fontWeight: isActive ? 600 : 400,
                color: isActive ? "var(--accent-primary)" : "var(--text-secondary)",
              }}
            >
              {category.label}
            </span>
          </button>
        );
      })}
    </div>
  );
}
