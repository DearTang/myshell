// 连接域：连接列表 + 文件夹树 + 回收站元数据。
import { reactive } from "vue";
import { getConnections, listFolders, type ConnectionConfig } from "../api";

interface ConnectionsState {
  connections: ConnectionConfig[];
  folders: string[];
  loaded: boolean;
}

export const connectionsStore = reactive<ConnectionsState>({
  connections: [],
  folders: [],
  loaded: false,
});

/** 重新从后端拉取连接与文件夹（保险库解锁后、CRUD 后调用）。 */
export async function reloadConnections(): Promise<void> {
  try {
    const [conns, dirs] = await Promise.all([getConnections(), listFolders()]);
    connectionsStore.connections = conns;
    connectionsStore.folders = dirs;
    connectionsStore.loaded = true;
  } catch (e) {
    console.error("Failed to load connections:", e);
  }
}

export function findConnection(id: string): ConnectionConfig | undefined {
  return connectionsStore.connections.find((c) => c.id === id);
}
