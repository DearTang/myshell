// 会话域：标签页编排 + 连接生命周期 + 广播组 + xterm 注册表。
// 逻辑从旧 App.tsx 原样移植（语义与注释要点保留），改为响应式单例。
// 关键不变量：tab.id 稳定（重连只换 sessionId，保留 scrollback）。
import { reactive, ref } from "vue";
import type { Terminal } from "@xterm/xterm";
import {
  type ConnectionConfig,
  type Tab,
  sshConnect,
  sshDisconnect,
  localConnect,
  localDisconnect,
  ftpConnect,
  ftpDisconnect,
  resetKnownHost,
} from "../api";
import { connectionsStore } from "./connections";
import { ui } from "./ui";

interface SessionsState {
  tabs: Tab[];
  activeTabId: string | null;
  /** 广播组：组内 SSH 终端 tab 的键击互相镜像 */
  broadcastIds: Set<string>;
}

export const sessions = reactive<SessionsState>({
  tabs: [],
  activeTabId: null,
  broadcastIds: new Set(),
});

// ── 广播重复连接提醒（会话级，不持久化） ──
export const broadcastDup = reactive({
  prompt: null as null | { tabId: string; connectionName: string; existingCount: number; initialDontRemind: boolean },
});
export const broadcastDupDismissed = ref(false);
export const broadcastDupDontRemindPref = ref(true);

// ── xterm 实例注册表（非响应式）：AI 面板读取输出/粘贴命令、重连快照用 ──
const terminalRegistry = new Map<string, Terminal>();

export function registerTerminal(sessionId: string, term: Terminal): void {
  terminalRegistry.set(sessionId, term);
}

export function unregisterTerminal(sessionId: string): void {
  terminalRegistry.delete(sessionId);
}

export function getTerminal(sessionId?: string): Terminal | undefined {
  return sessionId ? terminalRegistry.get(sessionId) : undefined;
}

export function getTab(tabId: string): Tab | undefined {
  return sessions.tabs.find((t) => t.id === tabId);
}

export function setActiveTab(tabId: string | null): void {
  sessions.activeTabId = tabId;
}

/** 清掉已消费的重连快照，避免后续重挂载重复写入。 */
export function clearReconnectSnapshot(tabId: string): void {
  patchTab(tabId, { reconnectSnapshot: undefined });
}

function patchTab(tabId: string, patch: Partial<Tab>): void {
  const idx = sessions.tabs.findIndex((t) => t.id === tabId);
  if (idx >= 0) sessions.tabs[idx] = { ...sessions.tabs[idx], ...patch };
}

// ── 连接生命周期 ──

/** 打开一个连接（terminal / sftp / ftp / local）。语义与旧 handleConnect 一致。 */
export async function connect(config: ConnectionConfig): Promise<void> {
  const connType = config.conn_type ?? "ssh";
  const display = connType === "local" ? config.name : `${config.username}@${config.host}`;

  const newTabId = `tab-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  const newTab: Tab = {
    id: newTabId,
    name: display,
    type: connType === "ftp" ? "ftp" : connType === "sftp" ? "sftp" : "terminal",
    connType,
    connectionId: config.id,
    status: "connecting",
    config,
  };

  sessions.tabs.push(newTab);
  sessions.activeTabId = newTabId;

  try {
    if (connType === "ftp") {
      const ftpId = await ftpConnect(config);
      patchTab(newTabId, { sessionId: ftpId, ftpSessionId: ftpId, status: "connected", errorMessage: undefined });
    } else if (connType === "local") {
      const sessionId = await localConnect(config);
      patchTab(newTabId, { sessionId, status: "connected", errorMessage: undefined });
    } else {
      const sessionId = await sshConnect(config);
      patchTab(newTabId, { sessionId, status: "connected", errorMessage: undefined });
    }
  } catch (e) {
    const errorMessage = String(e);
    // 主机密钥变更（服务器重装/换 key）→ 错误页提供"重置密钥并重连"入口
    const hostKeyMismatch = errorMessage.includes("主机密钥已变更");
    patchTab(newTabId, { status: "error", errorMessage, hostKeyMismatch });
  }
}

/** 重连单个 tab。成功返回 true。优先用连接列表里的最新配置。 */
export async function reconnectOne(tabId: string): Promise<boolean> {
  const tab = getTab(tabId);
  if (!tab || !tab.config) return false;

  // 抓旧终端 scrollback 快照，重连后由新 xterm 恢复（上限 5000 行）
  let snapshot: string | undefined;
  if (tab.type === "terminal" && tab.sessionId) {
    const oldTerm = terminalRegistry.get(tab.sessionId);
    if (oldTerm) {
      try {
        const buf = oldTerm.buffer.active;
        const lines: string[] = [];
        for (let i = 0; i < buf.length; i++) {
          const line = buf.getLine(i);
          if (line) lines.push(line.translateToString(true));
        }
        snapshot = lines.slice(-5000).join("\r\n");
      } catch {
        /* 快照失败不阻塞重连 */
      }
    }
  }

  if (tab.sessionId) {
    try {
      if (tab.connType === "ftp" && tab.ftpSessionId) {
        await ftpDisconnect(tab.ftpSessionId);
      } else if (tab.connType === "local") {
        await localDisconnect(tab.sessionId);
      } else {
        await sshDisconnect(tab.sessionId);
      }
    } catch {
      /* 旧会话可能已死，尽力而为 */
    }
  }

  if (snapshot) patchTab(tabId, { reconnectSnapshot: snapshot });
  patchTab(tabId, { status: "connecting", errorMessage: undefined });

  try {
    const tabConfig = tab.config;
    const config = connectionsStore.connections.find((c) => c.id === tabConfig.id) ?? tabConfig;
    const connType = config.conn_type ?? "ssh";
    if (connType === "ftp") {
      const ftpId = await ftpConnect(config);
      patchTab(tabId, { sessionId: ftpId, ftpSessionId: ftpId, status: "connected", errorMessage: undefined });
    } else if (connType === "local") {
      const sessionId = await localConnect(config);
      patchTab(tabId, { sessionId, status: "connected", errorMessage: undefined });
    } else {
      const sessionId = await sshConnect(config);
      patchTab(tabId, { sessionId, status: "connected", errorMessage: undefined });
    }
    return true;
  } catch (e) {
    const errorMessage = String(e);
    patchTab(tabId, {
      status: "error",
      errorMessage,
      hostKeyMismatch: errorMessage.includes("主机密钥已变更"),
    });
    return false;
  }
}

/** 重连 + 广播级联：若该 tab 在广播组内，同组掉线成员一并重连。 */
export async function reconnect(tabId: string): Promise<void> {
  await reconnectOne(tabId);
  if (sessions.broadcastIds.has(tabId)) {
    const downSiblings = sessions.tabs.filter(
      (t) => t.id !== tabId && sessions.broadcastIds.has(t.id) && t.config && (t.status === "disconnected" || t.status === "error"),
    );
    await Promise.all(downSiblings.map((t) => reconnectOne(t.id)));
  }
}

export async function reconnectAll(): Promise<void> {
  const down = sessions.tabs.filter((t) => t.status === "disconnected" || t.status === "error");
  await Promise.all(down.map((t) => reconnectOne(t.id)));
}

/** 主机密钥变更恢复：忘掉旧指纹后重连（重新走 TOFU）。 */
export async function resetHostKeyAndReconnect(tabId: string): Promise<void> {
  const tab = getTab(tabId);
  if (!tab?.config) return;
  try {
    await resetKnownHost(tab.config.host, tab.config.port);
  } catch (e) {
    window.alert(`重置主机密钥失败: ${e}`);
    return;
  }
  await reconnect(tabId);
}

/** 关闭 tab：断开会话（幂等）→ 移出广播组 → 移除 tab → 活动页回退。 */
export async function closeTab(tabId: string): Promise<void> {
  const tab = getTab(tabId);
  if (tab?.sessionId) {
    try {
      if (tab.connType === "ftp" && tab.ftpSessionId) {
        if (tab.status === "connected") await ftpDisconnect(tab.ftpSessionId);
      } else if (tab.connType === "local") {
        if (tab.status === "connected") await localDisconnect(tab.sessionId);
      } else {
        // SSH/SFTP 关页一律断开：后端幂等；不断会泄漏后端会话与 TCP
        await sshDisconnect(tab.sessionId);
      }
    } catch (e) {
      console.error("Disconnect error:", e);
    }
  }
  sessions.tabs = sessions.tabs.filter((t) => t.id !== tabId);
  sessions.broadcastIds.delete(tabId);
  if (sessions.activeTabId === tabId) {
    const remaining = sessions.tabs;
    sessions.activeTabId = remaining.length > 0 ? remaining[remaining.length - 1].id : null;
  }
}

/** 一键关闭所有掉线会话。 */
export async function closeDisconnected(): Promise<void> {
  const down = sessions.tabs.filter((t) => t.status === "disconnected" || t.status === "error");
  await Promise.all(down.map((t) => closeTab(t.id)));
}

// ── 广播组 ──

function setBroadcastMembership(tabId: string, inGroup: boolean): void {
  if (inGroup) sessions.broadcastIds.add(tabId);
  else sessions.broadcastIds.delete(tabId);
}

/** 切换某 tab 的广播组成员资格；同连接重复入组时弹确认（BroadcastDupDialog）。 */
export function toggleBroadcast(tabId: string): void {
  const tab = getTab(tabId);
  if (!tab) return;
  if (sessions.broadcastIds.has(tabId) || !tab.connectionId) {
    setBroadcastMembership(tabId, !sessions.broadcastIds.has(tabId));
    return;
  }
  const dupCount = sessions.tabs.filter((t) => sessions.broadcastIds.has(t.id) && t.connectionId === tab.connectionId).length;
  if (dupCount === 0) {
    setBroadcastMembership(tabId, true);
    return;
  }
  if (broadcastDupDismissed.value) {
    setBroadcastMembership(tabId, true);
    return;
  }
  const connName = connectionsStore.connections.find((c) => c.id === tab.connectionId)?.name || tab.name;
  broadcastDup.prompt = {
    tabId,
    connectionName: connName,
    existingCount: dupCount,
    initialDontRemind: broadcastDupDontRemindPref.value,
  };
}

/** 计算某 tab 的广播目标 sessionId 列表（TerminalPanel 的 onData 用）。 */
export function getBroadcastTargets(tab: Tab): string[] {
  if (!tab.sessionId || !sessions.broadcastIds.has(tab.id)) return [];
  const targets: string[] = [];
  for (const t of sessions.tabs) {
    if (!sessions.broadcastIds.has(t.id) || !t.sessionId || t.type !== "terminal" || t.connType !== "ssh" || t.status !== "connected") {
      continue;
    }
    targets.push(t.sessionId);
  }
  return targets;
}

/** 全部已连接 SSH 终端加入广播组 / 全部退出。 */
export function broadcastAll(): void {
  for (const t of sessions.tabs) {
    if (t.type === "terminal" && t.connType === "ssh" && t.status === "connected") sessions.broadcastIds.add(t.id);
  }
}

export function exitAllBroadcast(): void {
  sessions.broadcastIds.clear();
}

// ── 多窗口 ──

export async function enterMultiWindow(ids: string[]): Promise<void> {
  ui.multiWindowIds = ids;
  ui.multiWindowMode = ids.length > 0;
  ui.showMultiWindowPicker = false;
  if (ids.length > 0) {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    setTimeout(async () => {
      try {
        if (!(await win.isMaximized())) await win.maximize();
      } catch (e) {
        console.warn("[multiwindow] maximize failed:", e);
      }
    }, 100);
  }
}

export async function exitMultiWindow(): Promise<void> {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const win = getCurrentWindow();
  ui.multiWindowMode = false;
  ui.multiWindowIds = [];
  try {
    if (await win.isMaximized()) await win.unmaximize();
  } catch {
    /* 尽力而为 */
  }
}
