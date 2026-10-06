// 界面壳层状态：主题、布局尺寸、多窗口、全局对话框开关。
// 与模板 unified-ui-vue 的 store/ui.ts 同构，但保留 MyShell 自己的状态域划分
// 与 localStorage key（用户无感升级）。
import { reactive } from "vue";

export type ThemeChoice = "dark" | "light";

interface UiState {
  themeChoice: ThemeChoice;
  isDark: boolean;
  locale: "zh-CN" | "en";
  /** 侧栏折叠与宽度（宽度持久化 key 沿用旧版） */
  sidebarCollapsed: boolean;
  sidebarWidth: number;
  /** AI 面板 */
  showAiPanel: boolean;
  aiPanelWidth: number;
  /** 多窗口模式 */
  multiWindowMode: boolean;
  multiWindowIds: string[];
  showMultiWindowPicker: boolean;
  /** 应用信息 */
  appVersion: string;
  updateAvailable: boolean;
  /** 全局对话框开关（旧 App.tsx 的 useState 群收口于此） */
  showSettings: boolean;
  showQuickCommands: boolean;
  qcInitialConnectionId: string | null;
  showFeedback: boolean;
  showRecycle: boolean;
  about: { open: boolean; mode: "whatsnew" | "about" };
  statsPrompt: { version: string } | null;
  mwOverflowPrompt: { ids: string[]; cap: number } | null;
  /** MCP 危险命令确认（GUI 对话框形态；resolver 见 mcpConfirmResolver） */
  mcpConfirm: {
    command: string;
    connectionName: string;
    reasons: string[];
  } | null;
  /** 本轮会话已授权自动放行高危命令（MCP 确认框「本轮会话均允许」的结果反馈） */
  mcpSessionAllowed: boolean;
}

const PERSIST_KEY = "myshell.ui.prefs";

function readPersisted(): Partial<UiState> {
  try {
    return JSON.parse(localStorage.getItem(PERSIST_KEY) ?? "{}") as Partial<UiState>;
  } catch {
    return {};
  }
}

const persisted = readPersisted();

/**
 * Only these keys may come back from localStorage.
 *
 * The blob was previously spread wholesale over the defaults, so ANY key it
 * contained won — including `mcpSessionAllowed`, a security flag that
 * `persistUi()` deliberately never writes. A stale blob from an older build,
 * or one written by any other code path on the machine, would therefore grant
 * "all dangerous MCP commands allowed for this session" at startup, silently
 * suppressing the confirmation dialog.
 *
 * (Note: JSON.parse + spread cannot pollute Object.prototype, so this is
 * untrusted-input acceptance rather than prototype pollution.)
 */
const PERSISTED_KEYS = [
  "themeChoice",
  "sidebarCollapsed",
  "showAiPanel",
  "locale",
] as const;

function pickPersisted(): Partial<UiState> {
  const out: Record<string, unknown> = {};
  for (const k of PERSISTED_KEYS) {
    const v = (persisted as Record<string, unknown>)[k];
    if (v !== undefined) out[k] = v;
  }
  return out as Partial<UiState>;
}

export const ui = reactive<UiState>({
  themeChoice: "dark",
  isDark: true,
  locale: "zh-CN",
  sidebarCollapsed: false,
  sidebarWidth: 240,
  showAiPanel: false,
  aiPanelWidth: 380,
  multiWindowMode: false,
  multiWindowIds: [],
  showMultiWindowPicker: false,
  appVersion: "",
  updateAvailable: false,
  showSettings: false,
  showQuickCommands: false,
  qcInitialConnectionId: null,
  showFeedback: false,
  showRecycle: false,
  about: { open: false, mode: "about" },
  statsPrompt: null,
  mwOverflowPrompt: null,
  mcpConfirm: null,
  mcpSessionAllowed: false,
  ...pickPersisted(),
});

// 初始化持久化的数值字段（带边界钳制，沿用旧版语义）。
// 注意 getItem 返回 null 时 Number(null)=0（不是 NaN），若直接 Number.isFinite(0)
// 为真会把空存储钳到下限——全新安装/存储失忆时会拿到 200px 而非默认宽度。
// 因此 null 一律按 NaN 处理（视作"未存储"，走默认值）。
{
  const rawSidebar = localStorage.getItem("myshell.sidebarWidth");
  const storedSidebar = rawSidebar === null ? NaN : Number(rawSidebar);
  if (Number.isFinite(storedSidebar)) ui.sidebarWidth = Math.min(560, Math.max(200, storedSidebar));
  const rawAi = localStorage.getItem("myshell.aiPanelWidth");
  const storedAi = rawAi === null ? NaN : Number(rawAi);
  if (Number.isFinite(storedAi)) ui.aiPanelWidth = Math.min(720, Math.max(300, storedAi));
  const storedTheme = localStorage.getItem("myshell-theme-choice");
  if (storedTheme === "light" || storedTheme === "dark") ui.themeChoice = storedTheme;
}

export function persistUi(): void {
  const snapshot = {
    themeChoice: ui.themeChoice,
    sidebarCollapsed: ui.sidebarCollapsed,
    showAiPanel: ui.showAiPanel,
    locale: ui.locale,
  };
  localStorage.setItem(PERSIST_KEY, JSON.stringify(snapshot));
}

/** 把主题应用到文档根：html.dark/.light 供 myui / Element Plus 使用，
 *  html[data-theme] 供 MyShell 令牌层（themes.ts 预设覆盖其上）使用。
 *  配色预设的 UI 变量由 store/appearance.ts 响应 isDark 变化自行重应用。 */
export function applyTheme(): void {
  ui.isDark = ui.themeChoice === "dark";
  const root = document.documentElement;
  root.classList.toggle("dark", ui.isDark);
  root.classList.toggle("light", !ui.isDark);
  root.setAttribute("data-theme", ui.themeChoice);
  root.lang = ui.locale;
}

/** MCP 确认框的 Promise resolver（非响应式，模块内持有）。
 *  结果为 "allow"（本次放行）/ "session"（本轮会话均允许）/ "deny"（拒绝）。 */
export type McpConfirmDecision = "allow" | "session" | "deny";

let mcpConfirmResolver: ((decision: McpConfirmDecision) => void) | null = null;

export function showMcpConfirm(
  command: string,
  connectionName: string,
  reasons: string[],
): Promise<McpConfirmDecision> {
  return new Promise((resolve) => {
    mcpConfirmResolver = resolve;
    ui.mcpConfirm = { command, connectionName, reasons };
  });
}

export function resolveMcpConfirm(decision: McpConfirmDecision): void {
  ui.mcpConfirm = null;
  if (decision === "session") ui.mcpSessionAllowed = true;
  mcpConfirmResolver?.(decision);
  mcpConfirmResolver = null;
}

/** 会话授权在 MCP 服务器进程内生效，此标志仅用于 GUI 侧的状态展示与
 *  「本轮会话均允许」后的再次确认抑制（MCP 自己也会跳过确认请求）。 */
export function setMcpSessionAllowed(v: boolean): void {
  ui.mcpSessionAllowed = v;
}

export function setThemeChoice(choice: ThemeChoice): void {
  ui.themeChoice = choice;
  try {
    localStorage.setItem("myshell-theme-choice", choice);
  } catch {
    /* best-effort */
  }
  applyTheme();
  persistUi();
}

// ── 连接编辑对话框状态 ──
export const connectionDialog = reactive<{
  open: boolean;
  editConfig: import("../api").ConnectionConfig | null;
  initialConnType: import("../api").ConnType | undefined;
  initialFolderPath: string | undefined;
}>({
  open: false,
  editConfig: null,
  initialConnType: undefined,
  initialFolderPath: undefined,
});

export function openConnectionDialog(options?: {
  editConfig?: import("../api").ConnectionConfig | null;
  initialConnType?: import("../api").ConnType;
  initialFolderPath?: string;
}): void {
  connectionDialog.editConfig = options?.editConfig ?? null;
  connectionDialog.initialConnType = options?.initialConnType;
  connectionDialog.initialFolderPath = options?.initialFolderPath;
  connectionDialog.open = true;
}

export function closeConnectionDialog(): void {
  connectionDialog.open = false;
  connectionDialog.editConfig = null;
  connectionDialog.initialConnType = undefined;
  connectionDialog.initialFolderPath = undefined;
}
