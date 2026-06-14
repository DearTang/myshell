import { useState, useEffect } from "react";
import { Sidebar } from "./components/Sidebar";
import { TabBar } from "./components/TabBar";
import { TerminalPanel } from "./components/TerminalPanel";
import { SftpPanel } from "./components/SftpPanel";
import { ServerInfoPanel } from "./components/ServerInfoPanel";
import { ConnectionDialog } from "./components/ConnectionDialog";
import { MasterPasswordGate } from "./components/MasterPasswordGate";
import { SettingsPanel } from "./components/SettingsPanel";
import {
  getConnections,
  listFolders,
  sshConnect,
  sshDisconnect,
  ftpConnect,
  ftpDisconnect,
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
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [showSettings, setShowSettings] = useState(false);

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
        t.connType === "ssh"
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
    try {
      const connType = config.conn_type ?? "ssh";
      const display = `${config.username}@${config.host}`;
      if (connType === "ftp") {
        const ftpId = await ftpConnect(config);
        const newTab: Tab = {
          id: ftpId,
          name: display,
          sessionId: ftpId,
          type: "ftp",
          connType: "ftp",
          ftpSessionId: ftpId,
          connectionId: config.id,
        };
        setTabs((prev) => [...prev, newTab]);
        setActiveTabId(ftpId);
        return;
      }
      const sessionId = await sshConnect(config);
      const newTab: Tab = {
        id: sessionId,
        name: display,
        sessionId,
        type: connType === "sftp" ? "sftp" : "terminal",
        connType,
        connectionId: config.id,
      };
      setTabs((prev) => [...prev, newTab]);
      setActiveTabId(sessionId);
    } catch (e) {
      alert(`连接失败: ${e}`);
    }
  }

  async function handleCloseTab(tabId: string) {
    const tab = tabs.find((t) => t.id === tabId);
    if (tab?.sessionId) {
      try {
        if (tab.connType === "ftp" && tab.ftpSessionId) {
          await ftpDisconnect(tab.ftpSessionId);
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
    setShowDialog(true);
  }

  function handleAddNew(initialType?: ConnType) {
    setEditConfig(null);
    setInitialConnType(initialType);
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
          background: "var(--bg-dark)",
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
          const { deleteConnection } = await import("./api");
          await deleteConnection(id);
          reload();
        }}
        onAddNew={handleAddNew}
        onRefresh={reload}
        onOpenSettings={() => setShowSettings(true)}
        collapsed={sidebarCollapsed}
        onToggleCollapsed={() => setSidebarCollapsed((v) => !v)}
      />
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        <TabBar
          tabs={tabs}
          activeTabId={activeTabId}
          onSelect={setActiveTabId}
          onClose={handleCloseTab}
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
              if (!tab.sessionId) return null;
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
                  {tab.type === "terminal" ? (
                    <TerminalPanel
                      sessionId={tab.sessionId}
                      connectionId={tab.connectionId || ""}
                      broadcastTargets={getBroadcastTargets(tab)}
                      active={isActive}
                    />
                  ) : tab.type === "sftp" || tab.type === "ftp" ? (
                    <SftpPanel
                      sessionId={tab.sessionId}
                      source={tab.type === "ftp" ? "ftp" : "ssh"}
                      fullHeight
                    />
                  ) : (
                    <TerminalPanel
                      sessionId={tab.sessionId}
                      connectionId={tab.connectionId || ""}
                      active={isActive}
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
          activeTab.type === "terminal" && (
            <ServerInfoPanel
              sessionId={activeTab.sessionId}
              active={activeTab.id === activeTabId}
            />
          )}
      </div>
      {showDialog && (
        <ConnectionDialog
          config={editConfig}
          initialConnType={initialConnType}
          onClose={() => {
            setShowDialog(false);
            setEditConfig(null);
            setInitialConnType(undefined);
          }}
          onSave={() => {
            setShowDialog(false);
            setEditConfig(null);
            setInitialConnType(undefined);
            reload();
          }}
        />
      )}
      {showSettings && (
        <SettingsPanel
          onClose={() => setShowSettings(false)}
          onRefresh={reload}
          connectionCount={connections.length}
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
      <div style={{ fontSize: 48, opacity: 0.3 }}>⚡</div>
      <div style={{ fontSize: 20, fontWeight: 600 }}>MyShell</div>
      <div style={{ fontSize: 13 }}>点击左侧连接列表开始新会话</div>
    </div>
  );
}
