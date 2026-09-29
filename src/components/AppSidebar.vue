<!-- 左侧连接树侧栏（自包含，无 props）。
     从旧 src-legacy/components/Sidebar.tsx 原样移植：文件夹树 + 连接行、搜索过滤、
     展开/收起、长按拖拽移动（见 composables/useConnectionDrag.ts）、自绘右键菜单、
     行内 ⋯ 菜单、删除确认模态（folder 删除提示带红色强调计数）、宽度拖拽、折叠窄条、
     底部版本号/更新点/反馈。树形用本地 DOM 实现 —— myui MyTree 0.7.0 无自定义节点
     slot、不转发 node-contextmenu、draggable 为 el-tree 即拖语义（无长按/纯入文件夹/
     拖拽中 folders-only 视图），承载不了旧交互，故不硬凑。
     连接动作直接走 store/api：connect(sessions) / openConnectionDialog(ui) /
     deleteConnection + copyConnection + saveFolder + deleteFolder + renameFolder +
     renameConnection(@/api) 后 reloadConnections()。 -->
<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { ElMessageBox } from "element-plus";
import {
  Connection,
  CopyDocument,
  Delete,
  EditPen,
  Folder,
  FolderAdd,
  FolderOpened,
  MagicStick,
  Message,
  Moon,
  Plus,
  RefreshRight,
  Search,
  Setting,
  Sunny,
  Warning,
} from "@element-plus/icons-vue";
import { MyDialog, MyInput, toast } from "myui";
import {
  type ConnectionConfig,
  type ConnType,
  copyConnection,
  deleteConnection,
  deleteFolder,
  renameConnection,
  renameFolder,
  saveFolder,
} from "@/api";
import { connectionsStore, reloadConnections } from "@/store/connections";
import { connect } from "@/store/sessions";
import { openConnectionDialog, persistUi, setThemeChoice, ui } from "@/store/ui";
import { updateInfo } from "@/composables/useUpdateCheck";
import { useConnectionDrag } from "@/composables/useConnectionDrag";

defineOptions({ name: "AppSidebar" });

// ── 文件夹树模型 ─────────────────────────────────────────────────────────────
interface FolderNode {
  path: string;
  name: string;
  depth: number;
  children: FolderNode[];
  conns: ConnectionConfig[];
}

function buildTree(conns: ConnectionConfig[], folders: string[]): FolderNode {
  const root: FolderNode = { path: "/", name: "root", depth: 0, children: [], conns: [] };
  const nodeByPath = new Map<string, FolderNode>([["/", root]]);

  const ensureNode = (path: string): FolderNode => {
    const existing = nodeByPath.get(path);
    if (existing) return existing;
    const segments = path.split("/").filter(Boolean);
    const name = segments[segments.length - 1];
    const parentPath = "/" + segments.slice(0, -1).join("/");
    const parent = ensureNode(parentPath || "/");
    const node: FolderNode = {
      path,
      name,
      depth: segments.length,
      children: [],
      conns: [],
    };
    parent.children.push(node);
    nodeByPath.set(path, node);
    return node;
  };

  for (const f of folders) ensureNode(f);
  for (const c of conns) {
    const path = c.group_path || "/";
    ensureNode(path).conns.push(c);
  }

  const sortRec = (n: FolderNode): void => {
    n.children.sort((a, b) => a.name.localeCompare(b.name, "zh"));
    n.conns.sort((a, b) => a.name.localeCompare(b.name, "zh"));
    n.children.forEach(sortRec);
  };
  sortRec(root);
  return root;
}

// 连接类型图标来自 src/assets/iconfont (iconfont.cn)，全局已在 app.css 引入。
// Sidebar / TabBar / ConnectionDialog 统一用同一映射渲染，保证跨平台一致。
const CONN_ICON_CLASS: Record<ConnType, string> = {
  ssh: "icon-fuwuqi",
  sftp: "icon-SFTP",
  ftp: "icon-ftp",
  local: "icon-diannao",
};
/** 语义色按连接类型区分（与旧 ConnIcon 一致），供侧栏/标签栏/编辑框共用色觉。 */
const CONN_COLOR: Record<ConnType, string> = {
  ssh: "var(--accent-primary)",
  sftp: "var(--accent-secondary)",
  ftp: "var(--warning)",
  local: "var(--text-secondary)",
};
function connTypeOf(conn: ConnectionConfig): ConnType {
  return conn.conn_type || "ssh";
}

// ── 宽度：可拖拽调整，写入 ui.sidebarWidth + localStorage ────────────────────
// 默认 240；onResizeStart 钳制到 [200, 560]，避免拖得过窄不可用或吃掉整窗。
// 加宽后长连接名可完整露出。
function onResizeStart(e: MouseEvent): void {
  e.preventDefault();
  const startX = e.clientX;
  const startW = ui.sidebarWidth;
  const onMove = (ev: MouseEvent): void => {
    const w = Math.min(560, Math.max(200, startW + (ev.clientX - startX)));
    ui.sidebarWidth = w;
    localStorage.setItem("myshell.sidebarWidth", String(w));
  };
  const onUp = (): void => {
    document.removeEventListener("mousemove", onMove);
    document.removeEventListener("mouseup", onUp);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  };
  document.body.style.cursor = "col-resize";
  document.body.style.userSelect = "none";
  document.addEventListener("mousemove", onMove);
  document.addEventListener("mouseup", onUp);
}

// 壳层 grid 的列宽由 app.css 里 :root 的 --ui-sidebar-w 决定；自定义属性在子树内
// 设置传不到父级 grid，因此同步写到 documentElement 上，宽度拖拽才能真正生效。
function syncGridWidth(): void {
  try {
    document.documentElement.style.setProperty("--ui-sidebar-w", `${ui.sidebarWidth}px`);
  } catch {
    /* best-effort */
  }
}
watch(
  () => ui.sidebarWidth,
  () => syncGridWidth(),
);
onMounted(() => syncGridWidth());

// ── 展开状态 / 搜索 / 菜单 ───────────────────────────────────────────────────
const expanded = ref<Set<string>>(new Set());

interface CtxMenu {
  x: number;
  y: number;
  kind: "blank" | "folder";
  folderPath?: string;
}
const menu = ref<CtxMenu | null>(null);

const searchQuery = ref("");

// 主题切换（主题所有权在新壳层 store/ui；持久化 key 由 store 管理）
const isDark = computed(() => ui.themeChoice === "dark");
function toggleTheme(): void {
  setThemeChoice(isDark.value ? "light" : "dark");
}

// Confirm gates for destructive actions. `window.confirm` is silently
// swallowed by some Tauri WebViews, so we drive a real modal from these
// states instead. Each holds the action's payload (connection / folder path)
// and is cleared on confirm or cancel.
const deleteConnConfirm = ref<{ conn: ConnectionConfig } | null>(null);
const deleteFolderConfirm = ref<{ path: string; connCount: number; subFolderCount: number } | null>(null);

// 行内 ⋯ 菜单（同一时刻至多一个：旧版每行独立状态 + 全屏遮罩，打开新菜单前
// 必须先点掉旧菜单的遮罩，因此单份全局状态与旧行为等价）。
interface RowMenuState {
  conn: ConnectionConfig;
  x: number;
  y: number;
  /** 锚点靠近视口右缘时菜单向左展开（旧版按 anchor.x + 140 > innerWidth 判定） */
  flip: boolean;
}
const rowMenu = ref<RowMenuState | null>(null);

// ── 长按拖拽移动到文件夹 ─────────────────────────────────────────────────────
// 拖拽期间列表切换为 folders-only 视图（全部展开），避免为够到目标文件夹而
// 滚过一长串连接。onMoved 自动展开目标文件夹并刷新；onMoveError 弹错误提示。
const { dragState, beginDrag } = useConnectionDrag({
  onMoved: (targetFolderPath) => {
    expanded.value = new Set((() => {
      const next = new Set(expanded.value);
      next.add(targetFolderPath);
      return next;
    })());
    void reloadConnections();
  },
  onMoveError: (connId, err) => {
    toast(`移动失败: ${err}`, { type: "error" });
    console.error("[move_connection] failed for", connId, err);
  },
});
const isDragging = computed(() => dragState.value !== null);

// ── 搜索过滤 ────────────────────────────────────────────────────────────────
const filteredConnections = computed<ConnectionConfig[]>(() => {
  if (!searchQuery.value.trim()) return connectionsStore.connections;
  const query = searchQuery.value.toLowerCase();
  return connectionsStore.connections.filter((conn) => {
    const nameMatch = conn.name.toLowerCase().includes(query);
    const hostMatch = conn.host.toLowerCase().includes(query);
    return nameMatch || hostMatch;
  });
});

// ── 渲染列表（搜索时平铺，否则树形展开） ─────────────────────────────────────
type SidebarRow =
  | { kind: "folder"; key: string; node: FolderNode; isOpen: boolean; isDropTarget: boolean }
  | { kind: "conn"; key: string; conn: ConnectionConfig; depth: number; showGroupPath: boolean; noDrag: boolean };

const rows = computed<SidebarRow[]>(() => {
  // Build flat list for search results
  if (searchQuery.value.trim()) {
    return filteredConnections.value.map(
      (conn): SidebarRow => ({ kind: "conn", key: `search:${conn.id}`, conn, depth: 0, showGroupPath: true, noDrag: true }),
    );
  }

  const out: SidebarRow[] = [];
  const tree = buildTree(connectionsStore.connections, connectionsStore.folders);
  const walk = (node: FolderNode): void => {
    if (node.path !== "/") {
      const isOpen = isDragging.value ? true : expanded.value.has(node.path);
      out.push({
        kind: "folder",
        key: `f:${node.path}`,
        node,
        isOpen,
        isDropTarget: dragState.value?.hoverFolderPath === node.path,
      });
      // While dragging, render folders only (expanded) so the user can reach
      // any target folder without scrolling past the connections inside each.
      if (isDragging.value) {
        for (const child of node.children) walk(child);
        return;
      }
      if (!isOpen) return;
    }
    for (const c of node.conns) {
      out.push({ kind: "conn", key: `c:${c.id}`, conn: c, depth: node.depth, showGroupPath: false, noDrag: false });
    }
    for (const child of node.children) walk(child);
  };
  walk(tree);
  return out;
});

// ── 动作 ────────────────────────────────────────────────────────────────────
function handleConnect(conn: ConnectionConfig): void {
  void connect(conn);
}

function handleEdit(conn: ConnectionConfig): void {
  openConnectionDialog({ editConfig: conn });
}

/** 新建连接（可带初始类型/初始文件夹，新建入口与文件夹右键菜单共用）。 */
function handleAddNew(initialType?: ConnType, initialFolderPath?: string): void {
  openConnectionDialog({ initialConnType: initialType, initialFolderPath });
}

async function handleCopy(id: string): Promise<void> {
  try {
    await copyConnection(id);
    await reloadConnections();
  } catch (e) {
    toast(`复制失败: ${e}`, { type: "error" });
  }
}

function toggle(path: string): void {
  expanded.value = new Set((() => {
    const next = new Set(expanded.value);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    return next;
  })());
}

/** ElMessageBox.prompt 兜底：用户取消时 resolve null（旧版 window.prompt 取消
 * 返回 null 的语义；window.prompt 在 Tauri WebView 会被吞，故用 EP 实现）。 */
async function promptText(message: string, title: string, inputValue?: string): Promise<string | null> {
  try {
    const { value } = await ElMessageBox.prompt(message, title, {
      confirmButtonText: "确定",
      cancelButtonText: "取消",
      inputValue,
      closeOnClickModal: false,
    });
    return value;
  } catch {
    return null; // 用户取消
  }
}

async function handleAddFolder(parentPath: string): Promise<void> {
  const name = await promptText(parentPath === "/" ? "新建文件夹名称" : `在 ${parentPath} 下新建`, "新建文件夹");
  if (!name) return;
  const child = `${parentPath === "/" ? "" : parentPath}/${name.trim()}`;
  try {
    await saveFolder(child);
    await reloadConnections();
  } catch (e) {
    toast(`新建失败: ${e}`, { type: "error" });
  }
}

async function handleRenameFolder(oldPath: string): Promise<void> {
  const next = await promptText("重命名文件夹", "重命名文件夹", oldPath);
  if (!next || next === oldPath) return;
  try {
    await renameFolder(oldPath, next.trim());
    await reloadConnections();
  } catch (e) {
    toast(`重命名失败: ${e}`, { type: "error" });
  }
}

/** Rename a connection via the lightweight rename command (name is a plaintext
 * column, no full re-save needed). Prompted from the connection's ⋯ menu. */
async function handleRenameConnection(id: string, currentName: string): Promise<void> {
  const next = await promptText("重命名连接", "重命名连接", currentName);
  if (!next || next.trim() === currentName) return;
  try {
    await renameConnection(id, next.trim());
    await reloadConnections();
  } catch (e) {
    toast(`重命名失败: ${e}`, { type: "error" });
  }
}

/** Open the delete-connection confirm modal. The actual delete + recycle-bin
 * logic runs after the user confirms, so the prompt always shows regardless
 * of entry point (⋯ menu, etc.). */
function handleDeleteConnection(conn: ConnectionConfig): void {
  deleteConnConfirm.value = { conn };
}

/** Actually run the connection delete after the user confirmed. Soft-delete
 * (move to recycle bin). */
async function confirmDeleteConnection(): Promise<void> {
  if (!deleteConnConfirm.value) return;
  const id = deleteConnConfirm.value.conn.id;
  deleteConnConfirm.value = null;
  await deleteConnection(id);
  await reloadConnections();
}

function handleDeleteFolder(path: string): void {
  // Count connections + sub-folders under this path (recursively, so nested
  // children are caught too) to show a precise warning. Match rule mirrors
  // the backend like_prefix_pattern: a row belongs under `path` if its
  // group/path equals `path` or starts with `path + "/"`.
  const prefix = path.endsWith("/") ? path : path + "/";
  const isUnder = (p: string): boolean => p === path || p.startsWith(prefix);
  const connCount = connectionsStore.connections.filter((c) => isUnder(c.group_path || "/")).length;
  const subFolders = connectionsStore.folders.filter((f) => f !== path && isUnder(f));

  deleteFolderConfirm.value = { path, connCount, subFolderCount: subFolders.length };
}

/** Actually run the folder delete after the user confirmed. */
async function confirmDeleteFolder(): Promise<void> {
  if (!deleteFolderConfirm.value) return;
  const path = deleteFolderConfirm.value.path;
  deleteFolderConfirm.value = null;
  try {
    // One backend call soft-deletes child connections (into the recycle bin)
    // and physically drops this folder + all descendants, transaction-safe.
    await deleteFolder(path);
    await reloadConnections();
  } catch (e) {
    toast(`删除失败: ${e}`, { type: "error" });
  }
}

// ── 菜单开启/派发 ────────────────────────────────────────────────────────────
function closeMenu(): void {
  menu.value = null;
}
// Bound to both the sidebar root and the list container; the two were
// byte-identical duplicates, so one handler serves both.
function onBlankContext(e: MouseEvent): void {
  if ((e.target as HTMLElement) === e.currentTarget) {
    e.preventDefault();
    menu.value = { x: e.clientX, y: e.clientY, kind: "blank" };
  }
}
function onFolderContext(node: FolderNode, e: MouseEvent): void {
  e.preventDefault();
  if (isDragging.value) return;
  menu.value = { x: e.clientX, y: e.clientY, kind: "folder", folderPath: node.path };
}

function openRowMenu(conn: ConnectionConfig, e: MouseEvent): void {
  e.stopPropagation();
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
  rowMenu.value = {
    conn,
    x: r.right,
    y: r.bottom + 4,
    flip: r.right + 140 > window.innerWidth,
  };
}

/** 连接行 pointerdown 入口：长按拖拽仅树形视图可用（搜索平铺行不可拖，与旧版一致）。 */
function onConnPointerDown(conn: ConnectionConfig, noDrag: boolean, e: PointerEvent): void {
  if (noDrag) return;
  beginDrag(conn, e);
}

// 空白区菜单
function ctxNewConnection(): void {
  handleAddNew("ssh");
  closeMenu();
}
function ctxNewFolder(): void {
  void handleAddFolder("/");
  closeMenu();
}
// 文件夹菜单
function ctxNewConnectionInFolder(): void {
  const p = menu.value?.folderPath;
  handleAddNew("ssh", p);
  closeMenu();
}
function ctxNewSubFolder(): void {
  const p = menu.value?.folderPath;
  if (p) void handleAddFolder(p);
  closeMenu();
}
function ctxRenameFolder(): void {
  const p = menu.value?.folderPath;
  if (p) void handleRenameFolder(p);
  closeMenu();
}
function ctxDeleteFolder(): void {
  const p = menu.value?.folderPath;
  if (p) handleDeleteFolder(p);
  closeMenu();
}
// 行内 ⋯ 菜单
function rowMenuConnect(): void {
  const c = rowMenu.value?.conn;
  rowMenu.value = null;
  if (c) handleConnect(c);
}
function rowMenuEdit(): void {
  const c = rowMenu.value?.conn;
  rowMenu.value = null;
  if (c) handleEdit(c);
}
function rowMenuRename(): void {
  const c = rowMenu.value?.conn;
  rowMenu.value = null;
  if (c) void handleRenameConnection(c.id, c.name);
}
function rowMenuCopy(): void {
  const c = rowMenu.value?.conn;
  rowMenu.value = null;
  if (c) void handleCopy(c.id);
}
function rowMenuDelete(): void {
  const c = rowMenu.value?.conn;
  rowMenu.value = null;
  if (c) handleDeleteConnection(c);
}

// ── 壳层入口 ────────────────────────────────────────────────────────────────
function toggleCollapsed(): void {
  ui.sidebarCollapsed = !ui.sidebarCollapsed;
  // Persist. The equivalent toggle in the top bar calls persistUi()
  // (AppTopBar.vue), so collapsing from the sidebar's own ✕ button did not
  // survive a restart while collapsing from the top bar did.
  persistUi();
}
function openQuickCommands(): void {
  ui.qcInitialConnectionId = null;
  ui.showQuickCommands = true;
}
const updateAvailable = computed(() => !!updateInfo.value?.has_update);
function openAbout(): void {
  ui.about = { open: true, mode: "about" };
}
function openFeedback(): void {
  ui.showFeedback = true;
}

// 拖拽提示里的当前目标（去掉根斜杠再展示）
const dragHintTarget = computed<string | null>(() => {
  const p = dragState.value?.hoverFolderPath ?? null;
  return p && p !== "/" ? (p.startsWith("/") ? p.slice(1) : p) : null;
});

// 品牌Logo 渐变 id（内联 SVG，需全局唯一防冲突）
const brandGradId = `brandGrad-${Math.random().toString(36).slice(2, 10)}`;
</script>

<template>
  <!-- 折叠态：44px 窄条（仅 Logo / 竖排标题 / 展开按钮） -->
  <div v-if="ui.sidebarCollapsed" class="side side-collapsed">
    <svg
      class="brand-logo"
      width="26"
      height="26"
      viewBox="0 0 64 64"
      fill="none"
      role="img"
      aria-label="MyShell"
    >
      <defs>
        <linearGradient :id="brandGradId" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" style="stop-color: var(--accent-primary-hover)" />
          <stop offset="0.5" style="stop-color: var(--accent-primary)" />
          <stop offset="1" style="stop-color: var(--accent-secondary)" />
        </linearGradient>
      </defs>
      <!-- chevron `>` — vertex on the left, opens right -->
      <path
        d="M35 19 L16 32 L35 45"
        :stroke="`url(#${brandGradId})`"
        stroke-width="6.5"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
      <!-- cursor `_` -->
      <rect x="37" y="42" width="13" height="5" rx="2.5" style="fill: var(--accent-secondary)" />
    </svg>
    <button
      type="button"
      class="toggle-btn"
      :title="ui.sidebarCollapsed ? '展开侧栏' : '收起侧栏'"
      :aria-label="ui.sidebarCollapsed ? '展开侧栏' : '收起侧栏'"
      @click.stop="toggleCollapsed"
    >
      <svg
        width="12"
        height="12"
        viewBox="0 0 24 24"
        fill="none"
        aria-hidden="true"
        class="chevron-svg"
        :class="{ rotated: ui.sidebarCollapsed }"
      >
        <!-- Left-pointing chevron; rotates 180° to point right when collapsed -->
        <path d="M15 6l-6 6 6 6" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>
  </div>

  <!-- 展开态 -->
  <div
    v-else
    class="side"
    :style="{ width: `${ui.sidebarWidth}px`, minWidth: `${ui.sidebarWidth}px`, overflow: 'visible' }"
    @click="closeMenu"
    @contextmenu="onBlankContext"
  >
    <!-- 头部：动作图标行（myui/EP 单色图标体系） -->
    <div class="side-header">
      <div class="header-actions">
        <button type="button" class="icon-btn" :title="isDark ? '切换到亮色模式' : '切换到暗色模式'" @click="toggleTheme">
          <el-icon :size="15"><Sunny v-if="isDark" /><Moon v-else /></el-icon>
        </button>
        <button type="button" class="icon-btn" title="快捷命令" @click="openQuickCommands">
          <el-icon :size="15"><MagicStick /></el-icon>
        </button>
        <button type="button" class="icon-btn" title="设置" @click="ui.showSettings = true">
          <el-icon :size="15"><Setting /></el-icon>
        </button>
        <button type="button" class="icon-btn" title="刷新" @click="void reloadConnections()">
          <el-icon :size="15"><RefreshRight /></el-icon>
        </button>
        <button type="button" class="icon-btn" title="回收站" @click="ui.showRecycle = true">
          <el-icon :size="15"><Delete /></el-icon>
        </button>
        <button type="button" class="icon-btn" title="新建文件夹" @click="void handleAddFolder('/')">
          <el-icon :size="15"><FolderAdd /></el-icon>
        </button>
        <button type="button" class="btn-new" title="新建连接" aria-label="新建连接" @click="handleAddNew()">
          <el-icon :size="16"><Plus /></el-icon>
        </button>
      </div>
    </div>

    <!-- 搜索框（MyInput：small + 前缀图标 + 内建清除） -->
    <div class="search-box">
      <MyInput
        v-model="searchQuery"
        size="small"
        clearable
        placeholder="搜索连接名称或地址..."
      >
        <template #prefix>
          <el-icon :size="13"><Search /></el-icon>
        </template>
      </MyInput>
      <div v-if="searchQuery" class="search-count">找到 {{ filteredConnections.length }} 个连接</div>
    </div>

    <!-- 连接树列表 -->
    <div class="list" @contextmenu="onBlankContext">
      <!-- 拖拽横幅：说明手势语义 + 当前落点，用户永远不必猜 -->
      <div v-if="isDragging" class="drag-hint">
        <span class="drag-hint-title">正在移动「{{ dragState?.connName ?? "" }}」</span>
        <span>拖到文件夹上松开即可移动到该文件夹；ESC 或松开在空白处取消</span>
        <span v-if="dragHintTarget" class="drag-hint-target">当前目标：<span class="drag-hint-folder"><el-icon :size="11" style="vertical-align: -1px"><Folder /></el-icon> {{ dragHintTarget }}</span></span>
        <span v-else class="drag-hint-none">当前目标：移到文件夹上以选择</span>
      </div>

      <template v-for="row in rows" :key="row.key">
        <!-- 文件夹行：data-folder-path 供拖拽 elementFromPoint 命中测试 -->
        <div
          v-if="row.kind === 'folder'"
          class="folder-row"
          :class="{ 'drop-target': row.isDropTarget }"
          :style="{ paddingLeft: `${16 + row.node.depth * 14}px` }"
          :data-folder-path="row.node.path"
          @click="toggle(row.node.path)"
          @contextmenu="onFolderContext(row.node, $event)"
        >
          <span class="chevron" :class="{ open: row.isOpen }">›</span>
          <span class="folder-emoji" :class="{ open: row.isOpen }">
            <el-icon :size="14" :color="row.isOpen ? 'var(--accent-primary)' : 'var(--text-secondary)'">
              <FolderOpened v-if="row.isOpen" /><Folder v-else />
            </el-icon>
          </span>
          <span class="folder-name" :title="row.node.name">{{ row.node.name }}</span>
          <span v-if="row.node.conns.length > 0" class="folder-count">{{ row.node.conns.length }}</span>
        </div>

        <!-- 连接行：双击/回车连接；长按拖拽移动；右键仅抑制系统菜单（动作在 ⋯ 菜单） -->
        <div
          v-else
          class="conn-row"
          :class="{ 'is-dragged': dragState?.connId === row.conn.id }"
          :style="{ paddingLeft: `${24 + row.depth * 14}px` }"
          tabindex="0"
          @pointerdown="onConnPointerDown(row.conn, row.noDrag, $event)"
          @dblclick="handleConnect(row.conn)"
          @keydown.enter="handleConnect(row.conn)"
          @contextmenu.prevent
        >
          <i
            class="iconfont conn-icon"
            :class="CONN_ICON_CLASS[connTypeOf(row.conn)]"
            :style="{ fontSize: '16px', color: CONN_COLOR[connTypeOf(row.conn)] }"
            aria-hidden="true"
          ></i>
          <div
            class="conn-main"
            :title="`${row.conn.name}${row.conn.host ? `\n${row.conn.host}${row.conn.port ? `:${row.conn.port}` : ''}` : ''}`"
          >
            <span class="conn-name">{{ row.conn.name }}</span>
            <span v-if="row.showGroupPath && row.conn.group_path && row.conn.group_path !== '/'" class="conn-group">
              <el-icon :size="10" style="vertical-align: -1px"><FolderOpened /></el-icon>
              {{ row.conn.group_path.startsWith("/") ? row.conn.group_path.slice(1) : row.conn.group_path }}
            </span>
            <span v-if="row.showGroupPath" class="conn-host">{{ row.conn.host }}:{{ row.conn.port }}</span>
          </div>
          <span
            class="conn-more"
            title="更多操作"
            @click.stop="openRowMenu(row.conn, $event)"
            @pointerdown.stop
          >⋯</span>
        </div>
      </template>

      <!-- 搜索无结果 -->
      <div v-if="filteredConnections.length === 0 && searchQuery" class="empty">
        <div class="empty-emoji"><el-icon :size="26"><Search /></el-icon></div>
        <div>未找到匹配的连接</div>
        <div class="empty-sub">尝试其他关键词</div>
      </div>
      <!-- 完全为空 -->
      <div v-if="connectionsStore.connections.length === 0 && connectionsStore.folders.length === 0 && !searchQuery" class="empty">
        <i class="iconfont icon-fuwuqi empty-conn-icon" aria-hidden="true"></i>
        <div>暂无连接</div>
        <div class="empty-sub">点击 + 新建</div>
      </div>
    </div>

    <!-- 版本页脚：点版本打开关于；💬 反馈阻止冒泡。更新点出现时贴右缘。 -->
    <div
      v-if="ui.appVersion"
      class="footer"
      :title="updateAvailable ? '发现新版本，点击查看' : '关于 MyShell'"
      @click="openAbout"
    >
      <span class="footer-brand">MyShell</span>
      <span class="footer-version">v{{ ui.appVersion }}</span>
      <span v-if="updateAvailable" class="update-dot" title="发现新版本"></span>
      <span
        class="feedback-icon"
        :class="{ 'no-push': !updateAvailable }"
        title="反馈与建议"
        @click.stop="openFeedback"
      ><el-icon :size="13"><Message /></el-icon></span>
    </div>

    <!-- 空白区右键菜单 -->
    <div v-if="menu" class="ctx-menu" :style="{ left: `${menu.x}px`, top: `${menu.y}px` }" @click.stop>
      <template v-if="menu.kind === 'blank'">
        <div class="menu-item" @click="ctxNewConnection"><span class="menu-ico"><el-icon :size="13"><Plus /></el-icon></span>新建连接</div>
        <div class="menu-item" @click="ctxNewFolder"><span class="menu-ico"><el-icon :size="13"><FolderAdd /></el-icon></span>新建文件夹</div>
      </template>
      <template v-else>
        <div class="menu-item" @click="ctxNewConnectionInFolder"><span class="menu-ico"><el-icon :size="13"><Plus /></el-icon></span>新建连接</div>
        <div class="menu-item" @click="ctxNewSubFolder"><span class="menu-ico"><el-icon :size="13"><FolderAdd /></el-icon></span>新建子文件夹</div>
        <div class="menu-item" @click="ctxRenameFolder"><span class="menu-ico"><el-icon :size="13"><EditPen /></el-icon></span>重命名</div>
        <div class="menu-item danger" @click="ctxDeleteFolder"><span class="menu-ico"><el-icon :size="13"><Delete /></el-icon></span>删除</div>
      </template>
    </div>

    <!-- 连接行 ⋯ 菜单（全屏遮罩 + 锚定弹出） -->
    <template v-if="rowMenu">
      <div class="row-overlay" @click.stop="rowMenu = null"></div>
      <div
        class="ctx-menu"
        :style="{
          left: `${rowMenu.x}px`,
          top: `${rowMenu.y}px`,
          transform: rowMenu.flip ? 'translateX(-100%)' : undefined,
          transformOrigin: 'top right',
        }"
        @click.stop
      >
        <div class="menu-item" @click="rowMenuConnect"><span class="menu-ico"><el-icon :size="13"><Connection /></el-icon></span>连接</div>
        <div class="menu-item" @click="rowMenuEdit"><span class="menu-ico"><el-icon :size="13"><EditPen /></el-icon></span>编辑</div>
        <div class="menu-item" @click="rowMenuRename"><span class="menu-ico"><el-icon :size="13"><EditPen /></el-icon></span>重命名</div>
        <div class="menu-item" @click="rowMenuCopy"><span class="menu-ico"><el-icon :size="13"><CopyDocument /></el-icon></span>复制</div>
        <div class="menu-item danger" @click="rowMenuDelete"><span class="menu-ico"><el-icon :size="13"><Delete /></el-icon></span>删除</div>
      </div>
    </template>

    <!-- 删除连接确认模态（window.confirm 在 Tauri WebView 会被吞，用真实模态）。
         MyDialog 壳：danger 确认键；旧版「点遮罩 = 取消」用 close-on-click-modal 保持
         （Esc/右上角关闭旧版没有，保持禁用）。文案与处理函数逐字保留。 -->
    <MyDialog
      v-if="deleteConnConfirm"
      :model-value="true"
      size="sm"
      danger
      confirm-text="删除"
      cancel-text="取消"
      close-on-click-modal
      @confirm="confirmDeleteConnection"
      @cancel="deleteConnConfirm = null"
    >
      <template #header>
        <div class="confirm-head">
          <span class="confirm-ico"><el-icon :size="18" color="var(--warning)"><Warning /></el-icon></span>
          <span class="confirm-title">删除连接</span>
        </div>
      </template>
      <div class="confirm-msg">{{ `确定删除连接「${deleteConnConfirm.conn.name}」？\n删除后可在「设置 → 找回连接」中恢复。` }}</div>
    </MyDialog>

    <!-- 删除文件夹确认模态：按内容分三档提示，含连接时红色强调计数（富文本原样保留） -->
    <MyDialog
      v-if="deleteFolderConfirm"
      :model-value="true"
      size="sm"
      danger
      confirm-text="删除"
      cancel-text="取消"
      close-on-click-modal
      @confirm="confirmDeleteFolder"
      @cancel="deleteFolderConfirm = null"
    >
      <template #header>
        <div class="confirm-head">
          <span class="confirm-ico"><el-icon :size="18" color="var(--warning)"><Warning /></el-icon></span>
          <span class="confirm-title">
            {{ deleteFolderConfirm.connCount > 0 || deleteFolderConfirm.subFolderCount > 0 ? "删除文件夹" : "删除空文件夹" }}
          </span>
        </div>
      </template>
      <div class="confirm-msg">
        <template v-if="deleteFolderConfirm.connCount > 0"><span>文件夹「{{ deleteFolderConfirm.path }}」</span><span class="hl">下包含 </span><span class="hl">{{ deleteFolderConfirm.connCount }} 个连接</span><span v-if="deleteFolderConfirm.subFolderCount > 0" class="hl">、{{ deleteFolderConfirm.subFolderCount }} 个子文件夹</span><span>。删除后这些子文件夹将一并移除，其中的连接会移入回收站（可在「设置 → 找回连接」恢复）。</span><br />确定删除？</template>
        <template v-else-if="deleteFolderConfirm.subFolderCount > 0"><span>文件夹「{{ deleteFolderConfirm.path }}」下包含 {{ deleteFolderConfirm.subFolderCount }} 个子文件夹。</span><br />确定删除？</template>
        <template v-else><span>确定删除空文件夹「{{ deleteFolderConfirm.path }}」？</span></template>
      </div>
    </MyDialog>

    <!-- 右缘宽度拖拽手柄 -->
    <div class="resize-handle" title="拖动调整侧栏宽度" @mousedown="onResizeStart"></div>

    <!-- 折叠切换按钮（骑跨右缘） -->
    <button
      type="button"
      class="toggle-btn"
      :title="ui.sidebarCollapsed ? '展开侧栏' : '收起侧栏'"
      :aria-label="ui.sidebarCollapsed ? '展开侧栏' : '收起侧栏'"
      @click.stop="toggleCollapsed"
    >
      <svg
        width="12"
        height="12"
        viewBox="0 0 24 24"
        fill="none"
        aria-hidden="true"
        class="chevron-svg"
        :class="{ rotated: ui.sidebarCollapsed }"
      >
        <path d="M15 6l-6 6 6 6" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>
  </div>
</template>

<style scoped>
/* ── 容器：毛玻璃（与框架顶栏同配方——半透明面 + backdrop 模糊 + 玻璃描边） ── */
.side {
  position: relative;
  background: var(--surface-translucent);
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation));
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation));
  border-right: 1px solid var(--glass-border);
  display: flex;
  flex-direction: column;
}
.side-collapsed {
  width: 44px;
  min-width: 44px;
  align-items: center;
  padding: 12px 0;
}
.collapse-label {
  font-size: 10px;
  writing-mode: vertical-rl;
  margin-top: 16px;
  letter-spacing: 2px;
  color: var(--text-muted);
  opacity: 0.7;
}
.brand-logo {
  display: block;
}

/* ── 头部 ── */
.side-header {
  padding: 14px 16px;
  border-bottom: 1px solid var(--border-subtle);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.header-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.08em;
}
.header-actions {
  display: flex;
  gap: 4px;
  align-items: center;
}
.icon-btn {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  color: var(--text-tertiary);
  border: none;
  border-radius: var(--radius-md);
  font-size: 14px;
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-in-out);
}
.icon-btn:hover {
  background: var(--bg-surface-hover);
  color: var(--text-primary);
}
.btn-new {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-primary-muted);
  color: var(--accent-primary);
  border: 1px solid transparent;
  border-radius: var(--radius-md);
  cursor: pointer;
  transition:
    background var(--duration-normal) var(--ease-out-expo),
    color var(--duration-normal) var(--ease-out-expo),
    border-color var(--duration-normal) var(--ease-out-expo),
    box-shadow var(--duration-normal) var(--ease-out-expo),
    transform var(--duration-fast) var(--ease-out-expo);
}
.btn-new:hover {
  background: var(--accent-primary);
  color: var(--text-inverse);
  border-color: var(--accent-primary-hover);
  box-shadow: var(--shadow-glow);
}
.btn-new:active {
  transform: scale(0.88);
}

/* ── 搜索框 ── */
.search-box {
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-subtle);
}
/* MyInput 接管搜索框观感；这里只约束外层宽度 */
.search-box :deep(.my-field) {
  width: 100%;
}
.search-count {
  margin-top: 6px;
  font-size: 11px;
  color: var(--text-muted);
  text-align: center;
}

/* ── 列表 ── */
.list {
  flex: 1;
  overflow-y: auto;
  padding: 8px 0;
}
.empty {
  padding: 32px;
  text-align: center;
  color: var(--text-muted);
  font-size: 12px;
}
.empty-emoji {
  font-size: 32px;
  opacity: 0.3;
  margin-bottom: 12px;
}
.empty-conn-icon {
  font-size: 32px;
  opacity: 0.3;
  margin-bottom: 12px;
  color: var(--text-secondary);
}
.empty-sub {
  margin-top: 4px;
  color: var(--text-muted);
}

/* 文件夹行 */
.folder-row {
  padding: 7px 16px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-secondary);
  user-select: none;
  transition: background var(--duration-fast) var(--ease-out-expo);
  border-radius: 0 var(--radius-md) var(--radius-md) 0;
  margin-right: 8px;
}
/* drop target 高亮只在 isDropTarget 时生效；非 target 时保持 hover 背景 */
.folder-row:not(.drop-target):hover {
  background: var(--bg-surface-hover);
}
.folder-row.drop-target {
  background: var(--accent-primary-muted);
  box-shadow: inset 0 0 0 1px var(--border-accent);
}
.chevron {
  font-size: 8px;
  opacity: 0.6;
  transition: transform var(--duration-normal) var(--ease-out-expo);
  display: inline-block;
  transform: rotate(0deg);
}
.chevron.open {
  transform: rotate(90deg);
}
.folder-emoji {
  font-size: 14px;
  opacity: 0.7;
}
.folder-emoji.open {
  opacity: 1;
}
.folder-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.folder-count {
  font-size: 10px;
  color: var(--text-muted);
  background: var(--bg-surface);
  padding: 2px 6px;
  border-radius: var(--radius-full);
}

/* 连接行 */
.conn-row {
  padding: 8px 16px 8px 24px;
  cursor: grab;
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  color: var(--text-primary);
  outline: none;
  transition: background var(--duration-fast) var(--ease-in-out);
  border-radius: 0 var(--radius-md) var(--radius-md) 0;
  margin-right: 8px;
}
.conn-row:not(.is-dragged):hover {
  background: var(--bg-surface-hover);
}
/* 被拖行：pointer-events:none 让 elementFromPoint 穿过它命中下面的文件夹；
   同时降透明 + 抬升作为"正在拖这个"的视觉反馈。 */
.conn-row.is-dragged {
  cursor: grabbing;
  opacity: 0.4;
  pointer-events: none;
  transform: scale(1.02);
  box-shadow: var(--shadow-glow);
}
.conn-icon {
  opacity: 0.85;
}
.conn-main {
  flex: 1;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.conn-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.conn-group {
  font-size: 10px;
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.conn-host {
  font-size: 10px;
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.conn-more {
  font-size: 12px;
  opacity: 0.35;
  cursor: pointer;
  padding: 2px 6px;
  border-radius: var(--radius-sm);
  transition: all var(--duration-fast) var(--ease-in-out);
}
.conn-more:hover {
  opacity: 1;
  background: var(--bg-surface-active);
}

/* 拖拽横幅 */
.drag-hint {
  margin: 4px 8px 8px 8px;
  padding: 8px 12px;
  background: var(--accent-primary-muted);
  border: 1px solid var(--border-accent);
  border-radius: var(--radius-md);
  font-size: 11px;
  line-height: 1.5;
  color: var(--text-primary);
  display: flex;
  flex-direction: column;
  gap: 2px;
  animation: fadeIn var(--duration-normal) var(--ease-out-expo);
}
.drag-hint-title {
  font-weight: 600;
  color: var(--accent-primary);
}
.drag-hint-target {
  color: var(--text-tertiary);
}
.drag-hint-folder {
  color: var(--accent-secondary);
}
.drag-hint-none {
  color: var(--text-muted);
}
@keyframes fadeIn {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

/* ── 版本页脚 ── */
.footer {
  flex-shrink: 0;
  border-top: 1px solid var(--border-subtle);
  padding: 9px 16px;
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  user-select: none;
  transition: background var(--duration-fast) var(--ease-in-out);
}
.footer:hover {
  background: var(--bg-surface-hover);
}
.footer-brand {
  font-size: 11px;
  color: var(--text-tertiary);
}
.footer-version {
  font-size: 11px;
  color: var(--text-muted);
}
.update-dot {
  width: 7px;
  height: 7px;
  border-radius: var(--radius-full);
  background: var(--accent-secondary);
  box-shadow: 0 0 6px var(--accent-secondary);
  margin-left: auto;
}
.feedback-icon {
  font-size: 13px;
  line-height: 1;
  cursor: pointer;
  padding: 0 2px;
  opacity: 0.6;
}
.feedback-icon:hover {
  opacity: 1;
}
/* 没有更新点占据右缘时，反馈图标自己贴右 */
.feedback-icon.no-push {
  margin-left: auto;
}

/* ── 右键/⋯ 菜单 ── */
.ctx-menu {
  position: fixed;
  background: var(--bg-surface);
  border: 1px solid var(--border-emphasis);
  border-radius: var(--radius-lg);
  padding: 6px;
  z-index: 1000;
  min-width: 160px;
  box-shadow: var(--shadow-xl);
  backdrop-filter: blur(var(--glass-blur));
}
.menu-item {
  padding: 8px 14px;
  font-size: 12px;
  cursor: pointer;
  border-radius: var(--radius-md);
  display: flex;
  align-items: center;
  gap: 10px;
  color: var(--text-primary);
  transition: background var(--duration-fast) var(--ease-in-out);
}
.menu-item:hover {
  background: var(--bg-surface-hover);
}
.menu-item.danger {
  color: var(--error);
}
.menu-item.danger:hover {
  background: var(--error-muted);
}
.menu-ico {
  font-size: 14px;
  opacity: 0.8;
}
.row-overlay {
  position: fixed;
  inset: 0;
  z-index: 999;
}

/* ── 删除确认模态（MyDialog 壳；confirmDialog 不支持富文本强调，故用插槽内容） ── */
.confirm-head {
  display: flex;
  align-items: center;
  gap: 12px;
}
.confirm-ico {
  font-size: 18px;
  line-height: 1.2;
  display: inline-flex;
}
.confirm-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--text-primary);
}
.confirm-msg {
  font-size: 12.5px;
  color: var(--text-secondary);
  line-height: 1.7;
  white-space: pre-wrap;
}
.hl {
  color: var(--error);
  font-weight: 700;
}

/* ── 右缘宽度手柄 / 折叠按钮 ── */
.resize-handle {
  position: absolute;
  right: 0;
  top: 0;
  bottom: 0;
  width: 5px;
  cursor: col-resize;
  z-index: 6;
  /* 极淡的命中区；hover 提亮以便发现 */
  background: transparent;
  transition: background var(--duration-fast) var(--ease-in-out);
}
.resize-handle:hover {
  background: var(--accent-primary-muted);
}
.toggle-btn {
  position: absolute;
  right: -13px;
  top: 50%;
  width: 26px;
  height: 26px;
  transform: translateY(-50%);
  border-radius: var(--radius-full);
  background: var(--bg-surface);
  border: 1px solid var(--border-emphasis);
  color: var(--text-tertiary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
  padding: 0;
  box-shadow: var(--shadow-sm), inset 0 1px 0 rgba(255, 255, 255, 0.06);
  transition:
    background var(--duration-normal) var(--ease-out-expo),
    color var(--duration-normal) var(--ease-out-expo),
    border-color var(--duration-normal) var(--ease-out-expo),
    box-shadow var(--duration-normal) var(--ease-out-expo),
    transform var(--duration-normal) var(--ease-out-expo);
}
.toggle-btn:hover {
  background: var(--accent-primary-muted);
  border-color: var(--border-accent);
  color: var(--accent-primary);
  transform: translateY(-50%) scale(1.1);
  box-shadow: var(--shadow-md), inset 0 1px 0 rgba(255, 255, 255, 0.08);
}
.chevron-svg {
  display: block;
  transform: rotate(0deg);
  transition: transform var(--duration-normal) var(--ease-out-expo);
}
.chevron-svg.rotated {
  transform: rotate(180deg);
}
</style>
