import { useState, useEffect, useRef } from "react";
import { Sidebar } from "./components/Sidebar";
import { TabBar } from "./components/TabBar";
import { TerminalPanel } from "./components/TerminalPanel";
import { SftpPanel } from "./components/SftpPanel";
import { ServerInfoPanel } from "./components/ServerInfoPanel";
import { ConnectionDialog } from "./components/ConnectionDialog";
import { MasterPasswordGate } from "./components/MasterPasswordGate";
import { SettingsPanel } from "./components/SettingsPanel";
import { AiPanel } from "./components/AiPanel";
import { QuickCommandsPanel } from "./components/QuickCommandsPanel";
import { BrandLogo } from "./components/BrandLogo";
import type { Terminal } from "@xterm/xterm";
import {
  getConnections,
  deleteConnection,
  listFolders,
  sshConnect,
  sshDisconnect,
  ftpConnect,
  ftpDisconnect,
  localConnect,
  localDisconnect,
  vaultStatus,
} from "./api";
import type { ConnectionConfig, ConnType, Tab } from "./api";

export default function App() {
  const [connections, setConnections] = useState<ConnectionConfig[]>([]);
  const [folders, setFolders] = useState<string[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeTabId, setActiveTabId] = useState<string | null>(null);
  const [showDialog, setShowDialog] = useState(false);
  const [editConfig, setEditConfig] = useState<ConnectionConfig | null>(null);
  const [initialConnType, setInitialConnType] = useState<ConnType | undefined>(undefined);
  const [initialFolderPath, setInitialFolderPath] = useState<string | undefined>(undefined);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showQuickCommands, setShowQuickCommands] = useState(false);
  // AI assistant panel (global docked-right chat bar). Width persists in
  // localStorage so it survives reloads.
  const [showAiPanel, setShowAiPanel] = useState(false);
  const [aiPanelWidth, setAiPanelWidth] = useState(
    () => Number(localStorage.getItem("myshell.aiPanelWidth")) || 380
  );
  // Phase 3: live xterm registry, keyed by sessionId. The AI panel reads
  // terminal output/selection and pastes commands through it. Populated by
  // TerminalPanel.onTerminalReady, drained on close/reconnect.
  const terminalRegistryRef = useRef<Map<string, Terminal>>(new Map());
  const handleTerminalReady = (sid: string, term: Terminal) => {
    terminalRegistryRef.current.set(sid, term);
  };
  const handleTerminalGone = (sid: string) => {
    terminalRegistryRef.current.delete(sid);
  };
  const getTerminal = (sid?: string): Terminal | undefined =>
    sid ? terminalRegistryRef.current.get(sid) : undefined;
  // Preset scope when opening the quick-commands panel: null = global,
  // a connection id = that server's per-server scope. Set by the entry point
  // (Sidebar global button → null, CommandBar "管理" → current connection).
  const [qcInitialConnectionId, setQcInitialConnectionId] = useState<string | null>(null);

  // Vault gate: null = checking, "setup" = no vault yet, "unlock" = vault
  // exists but locked, "ready" = master key loaded. Render the gate until
  // ready, blocking all other UI so connection commands can't be invoked
  // without a derived key (which would just error with "Vault 未解锁").
  const [vault, setVault] = useState<"checking" | "setup" | "unlock" | "ready">("checking");

  useEffect(() => {
    if (vault !== "ready") {
      vaultStatus()
        .then((s) => {
          if (!s.initialized) setVault("setup");
          else if (s.unlocked) setVault("ready");
          else setVault("unlock");
        })
        .catch(() => setVault("setup"));
    }
  }, [vault]);

  useEffect(() => {
    if (vault === "ready") reload();
  }, [vault]);

  // Broadcast group: a Set of tab IDs whose terminal sessions should mirror
  // each other's keystrokes. Toggled per-tab from the TabBar 📡 button.
  // Only SSH terminal tabs are eligible — SFTP/FTP tabs can't accept shell
  // input, so they're filtered out at target-collection time.
  const [broadcastIds, setBroadcastIds] = useState<Set<string>>(new Set());

  function toggleBroadcast(tabId: string) {
    setBroadcastIds((prev) => {
      const next = new Set(prev);
      if (next.has(tabId)) next.delete(tabId);
      else next.add(tabId);
      return next;
    });
  }

  /** Compute the broadcast target sessionIds for a given tab:
   *  - If the tab is in the broadcast group: returns all SSH-terminal
   *    sessions that are also in the group (could include this tab itself).
   *  - Otherwise: empty array — TerminalPanel falls back to single-target.
   * The list is recomputed on every render so toggling membership is
   * reflected immediately; TerminalPanel reads it via a live ref so the
   * onData handler doesn't need rebinding. */
  function getBroadcastTargets(tab: Tab): string[] {
    if (!tab.sessionId || !broadcastIds.has(tab.id)) return [];
    const targets: string[] = [];
    for (const t of tabs) {
      if (
        broadcastIds.has(t.id) &&
        t.sessionId &&
        t.type === "terminal" &&
        t.connType === "ssh" &&
        t.status === "connected"
      ) {
        targets.push(t.sessionId);
      }
    }
    return targets;
  }

  async function reload() {
    try {
      const [conns, dirs] = await Promise.all([getConnections(), listFolders()]);
      setConnections(conns);
      setFolders(dirs);
    } catch (e) {
      console.error("Failed to load:", e);
    }
  }

  async function handleConnect(config: ConnectionConfig) {
    const connType = config.conn_type ?? "ssh";
    const display = connType === "local" ? config.name : `${config.username}@${config.host}`;

    // Create a temporary tab with "connecting" status
    const tempTabId = `temp-${Date.now()}`;
    const tempTab: Tab = {
      id: tempTabId,
      name: display,
      type: connType === "ftp" ? "ftp" : connType === "sftp" ? "sftp" : "terminal",
      connType,
      connectionId: config.id,
      status: "connecting",
      config: config,
    };

    setTabs((prev) => [...prev, tempTab]);
    setActiveTabId(tempTabId);

    try {
      if (connType === "ftp") {
        const ftpId = await ftpConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tempTabId
              ? {
                  id: ftpId,
                  name: display,
                  sessionId: ftpId,
                  type: "ftp",
                  connType: "ftp",
                  ftpSessionId: ftpId,
                  connectionId: config.id,
                  status: "connected",
                  config: config,
                }
              : t
          )
        );
        setActiveTabId(ftpId);
      } else if (connType === "local") {
        const sessionId = await localConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tempTabId
              ? {
                  id: sessionId,
                  name: display,
                  sessionId,
                  type: "terminal",
                  connType: "local",
                  connectionId: config.id,
                  status: "connected",
                  config: config,
                }
              : t
          )
        );
        setActiveTabId(sessionId);
      } else {
        const sessionId = await sshConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tempTabId
              ? {
                  id: sessionId,
                  name: display,
                  sessionId,
                  type: connType === "sftp" ? "sftp" : "terminal",
                  connType,
                  connectionId: config.id,
                  status: "connected",
                  config: config,
                }
              : t
          )
        );
        setActiveTabId(sessionId);
      }
    } catch (e) {
      const errorMessage = String(e);
      // Update tab to show error state
      setTabs((prev) =>
        prev.map((t) =>
          t.id === tempTabId
            ? {
                ...t,
                status: "error",
                errorMessage: errorMessage,
              }
            : t
        )
      );
    }
  }

  async function handleReconnect(tabId: string) {
    const tab = tabs.find((t) => t.id === tabId);
    if (!tab || !tab.config) return;

    // Update status to connecting
    setTabs((prev) =>
      prev.map((t) =>
        t.id === tabId
          ? { ...t, status: "connecting", errorMessage: undefined }
          : t
      )
    );

    try {
      const config = tab.config;
      const connType = config.conn_type ?? "ssh";

      if (connType === "ftp") {
        const ftpId = await ftpConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  id: ftpId,
                  sessionId: ftpId,
                  ftpSessionId: ftpId,
                  status: "connected",
                  errorMessage: undefined,
                }
              : t
          )
        );
        setActiveTabId(ftpId);
      } else if (connType === "local") {
        const sessionId = await localConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  id: sessionId,
                  sessionId,
                  status: "connected",
                  errorMessage: undefined,
                }
              : t
          )
        );
        setActiveTabId(sessionId);
      } else {
        const sessionId = await sshConnect(config);
        setTabs((prev) =>
          prev.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  id: sessionId,
                  sessionId,
                  status: "connected",
                  errorMessage: undefined,
                }
              : t
          )
        );
        setActiveTabId(sessionId);
      }
    } catch (e) {
      const errorMessage = String(e);
      setTabs((prev) =>
        prev.map((t) =>
          t.id === tabId
            ? { ...t, status: "error", errorMessage }
            : t
        )
      );
    }
  }

  async function handleCloseTab(tabId: string) {
    const tab = tabs.find((t) => t.id === tabId);
    if (tab?.sessionId && tab.status === "connected") {
      try {
        if (tab.connType === "ftp" && tab.ftpSessionId) {
          await ftpDisconnect(tab.ftpSessionId);
        } else if (tab.connType === "local") {
          await localDisconnect(tab.sessionId);
        } else {
          await sshDisconnect(tab.sessionId);
        }
      } catch (e) {
        console.error("Disconnect error:", e);
      }
    }
    setTabs((prev) => prev.filter((t) => t.id !== tabId));
    // Also drop the closed tab from the broadcast group so stale sessionIds
    // don't end up in the next broadcast fan-out.
    setBroadcastIds((prev) => {
      if (!prev.has(tabId)) return prev;
      const next = new Set(prev);
      next.delete(tabId);
      return next;
    });
    if (activeTabId === tabId) {
      const remaining = tabs.filter((t) => t.id !== tabId);
      setActiveTabId(remaining.length > 0 ? remaining[remaining.length - 1].id : null);
    }
  }

  function handleEdit(config: ConnectionConfig) {
    setEditConfig(config);
    setInitialConnType(undefined);
    setInitialFolderPath(undefined);
    setShowDialog(true);
  }

  function handleAddNew(initialType?: ConnType, initialFolderPath?: string) {
    setEditConfig(null);
    setInitialConnType(initialType);
    setInitialFolderPath(initialFolderPath);
    setShowDialog(true);
  }

  const activeTab = tabs.find((t) => t.id === activeTabId);

  // Vault gate — render the setup/unlock screen first, blocking the main
  // UI until the master key is loaded. Each onSuccess flips vault to
  // "checking" so the effect above re-queries status (and runs reload).
  if (vault === "checking") {
    return (
      <div
        style={{
          position: "fixed",
          inset: 0,
          background: "var(--bg-base)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "var(--text-muted)",
          fontSize: 13,
        }}
      >
        加载中…
      </div>
    );
  }
  if (vault === "setup") {
    return <MasterPasswordGate mode="setup" onSuccess={() => setVault("checking")} />;
  }
  if (vault === "unlock") {
    return <MasterPasswordGate mode="unlock" onSuccess={() => setVault("checking")} />;
  }

  return (
    <div style={{ display: "flex", height: "100vh", overflow: "hidden" }}>
      <Sidebar
        connections={connections}
        folders={folders}
        onConnect={handleConnect}
        onEdit={handleEdit}
        onDelete={async (id) => {
          await deleteConnection(id);
          reload();
        }}
        onAddNew={handleAddNew}
        onRefresh={reload}
        onOpenSettings={() => setShowSettings(true)}
        onOpenQuickCommands={() => {
          setQcInitialConnectionId(null);
          setShowQuickCommands(true);
        }}
        collapsed={sidebarCollapsed}
        onToggleCollapsed={() => setSidebarCollapsed((v) => !v)}
      />
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        <TabBar
          tabs={tabs}
          activeTabId={activeTabId}
          onSelect={setActiveTabId}
          onClose={handleCloseTab}
          onReconnect={handleReconnect}
          broadcastIds={broadcastIds}
          onToggleBroadcast={toggleBroadcast}
        />
        <div
          style={{
            flex: 1,
            display: "flex",
            flexDirection: "column",
            overflow: "hidden",
            position: "relative",
          }}
        >
          {tabs.length === 0 ? (
            <WelcomeScreen />
          ) : (
            tabs.map((tab) => {
              if (!tab.sessionId && tab.status !== "error" && tab.status !== "connecting") return null;
              const isActive = tab.id === activeTabId;
              // Use position:absolute + visibility:hidden instead of
              // display:none so every tab's container keeps a real size
              // at all times. xterm's renderer breaks when its container
              // collapses to 0x0 (RenderService throws "Cannot read
              // properties of undefined (reading 'dimensions')"), and the
              // FitAddon would otherwise read 0 cols/rows during the
              // hidden transition — which then propagates through the
              // broadcast sync as COLUMNS=0 and turns `ls` output into
              // one-file-per-line. Stacking tabs absolutely means the
              // ResizeObserver always sees the parent's full geometry.
              return (
                <div
                  key={tab.id}
                  style={{
                    position: "absolute",
                    inset: 0,
                    visibility: isActive ? "visible" : "hidden",
                    display: "flex",
                    flexDirection: "column",
                    overflow: "hidden",
                  }}
                >
                  {tab.status === "error" ? (
                    <ErrorState
                      message={tab.errorMessage || "连接失败"}
                      onReconnect={() => handleReconnect(tab.id)}
                      onClose={() => handleCloseTab(tab.id)}
                    />
                  ) : tab.status === "connecting" ? (
                    <ConnectingState />
                  ) : tab.type === "terminal" ? (
                    <TerminalPanel
                      sessionId={tab.sessionId!}
                      connectionId={tab.connectionId || ""}
                      connType={tab.connType}
                      fontOverride={connections.find((c) => c.id === (tab.connectionId || ""))?.terminal_font}
                      broadcastTargets={getBroadcastTargets(tab)}
                      onTerminalReady={handleTerminalReady}
                      onTerminalGone={handleTerminalGone}
                      onOpenAi={() => setShowAiPanel(true)}
                      active={isActive}
                      status={tab.status}
                      onReconnect={() => handleReconnect(tab.id)}
                      onOpenQuickCommandsManage={() => {
                        setQcInitialConnectionId(tab.connectionId || null);
                        setShowQuickCommands(true);
                      }}
                      onDisconnected={() => {
                        setTabs((prev) =>
                          prev.map((t) =>
                            t.id === tab.id ? { ...t, status: "disconnected" as const } : t
                          )
                        );
                      }}
                    />
                  ) : tab.type === "sftp" || tab.type === "ftp" ? (
                    <SftpPanel
                      sessionId={tab.sessionId!}
                      source={tab.type === "ftp" ? "ftp" : "ssh"}
                      fullHeight
                    />
                  ) : (
                    <TerminalPanel
                      sessionId={tab.sessionId!}
                      connectionId={tab.connectionId || ""}
                      connType={tab.connType}
                      fontOverride={connections.find((c) => c.id === (tab.connectionId || ""))?.terminal_font}
                      onTerminalReady={handleTerminalReady}
                      onTerminalGone={handleTerminalGone}
                      onOpenAi={() => setShowAiPanel(true)}
                      active={isActive}
                      status={tab.status}
                      onReconnect={() => handleReconnect(tab.id)}
                      onDisconnected={() => {
                        setTabs((prev) =>
                          prev.map((t) =>
                            t.id === tab.id ? { ...t, status: "disconnected" as const } : t
                          )
                        );
                      }}
                    />
                  )}
                </div>
              );
            })
          )}
        </div>
        {activeTab &&
          activeTab.sessionId &&
          activeTab.connType === "ssh" &&
          activeTab.type === "terminal" &&
          activeTab.status === "connected" && (
            <ServerInfoPanel
              sessionId={activeTab.sessionId}
              active={activeTab.id === activeTabId}
            />
          )}
      </div>
      {showAiPanel && (
        <AiPanel
          activeConnType={activeTab?.connType}
          activeConnectionName={activeTab?.config?.name ?? activeTab?.connectionId}
          activeSessionId={activeTab?.sessionId}
          getTerminal={getTerminal}
          width={aiPanelWidth}
          onWidthChange={(w) => {
            setAiPanelWidth(w);
            localStorage.setItem("myshell.aiPanelWidth", String(w));
          }}
          onClose={() => setShowAiPanel(false)}
        />
      )}
      {showDialog && (
        <ConnectionDialog
          config={editConfig}
          initialConnType={initialConnType}
          initialFolderPath={initialFolderPath}
          folders={folders}
          onClose={() => {
            setShowDialog(false);
            setEditConfig(null);
            setInitialConnType(undefined);
            setInitialFolderPath(undefined);
          }}
          onSave={() => {
            setShowDialog(false);
            setEditConfig(null);
            setInitialConnType(undefined);
            setInitialFolderPath(undefined);
            reload();
          }}
        />
      )}
      {showSettings && (
        <SettingsPanel
          onClose={() => setShowSettings(false)}
          onRefresh={reload}
          connectionCount={connections.length}
          onOpenQuickCommands={() => {
            setQcInitialConnectionId(null);
            setShowSettings(false);
            setShowQuickCommands(true);
          }}
        />
      )}
      {showQuickCommands && (
        <QuickCommandsPanel
          onClose={() => setShowQuickCommands(false)}
          connections={connections}
          initialConnectionId={qcInitialConnectionId}
          activeConnectionId={activeTab?.connectionId ?? null}
        />
      )}
    </div>
  );
}

function WelcomeScreen() {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        height: "100%",
        color: "var(--text-muted)",
        gap: 16,
      }}
    >
      <BrandLogo size={72} glow />
      <div style={{ fontSize: 20, fontWeight: 600, color: "var(--text-secondary)" }}>MyShell</div>
      <div style={{ fontSize: 13 }}>点击左侧连接列表开始新会话</div>
    </div>
  );
}

function ConnectingState() {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        height: "100%",
        color: "var(--text-muted)",
        gap: 12,
      }}
    >
      <div
        style={{
          fontSize: 24,
          animation: "spin 1s linear infinite",
        }}
      >
        ⏳
      </div>
      <div style={{ fontSize: 14 }}>正在连接...</div>
    </div>
  );
}

function ErrorState({
  message,
  onReconnect,
  onClose,
}: {
  message: string;
  onReconnect: () => void;
  onClose: () => void;
}) {
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        height: "100%",
        padding: 32,
      }}
    >
      <div
        style={{
          width: 80,
          height: 80,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "var(--error-muted)",
          borderRadius: "var(--radius-xl)",
          fontSize: 36,
          marginBottom: 20,
        }}
      >
        ❌
      </div>
      <div
        style={{
          fontSize: 18,
          fontWeight: 600,
          color: "var(--text-primary)",
          marginBottom: 8,
        }}
      >
        连接失败
      </div>
      <div
        style={{
          fontSize: 13,
          color: "var(--text-tertiary)",
          textAlign: "center",
          maxWidth: 400,
          marginBottom: 24,
          lineHeight: 1.6,
        }}
      >
        请确认下ip端口等是否填写错误!
        <br />
        {message}
      </div>
      <div style={{ display: "flex", gap: 12 }}>
        <button
          onClick={onClose}
          style={{
            padding: "10px 24px",
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
          关闭
        </button>
        <button
          onClick={onReconnect}
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
          onMouseEnter={(e) => {
            e.currentTarget.style.background = "var(--accent-primary-hover)";
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = "var(--accent-primary)";
          }}
        >
          重新连接
        </button>
      </div>
    </div>
  );
}
