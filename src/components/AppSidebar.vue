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
  MyBadge,
  MyDialog,
  MyDropdown,
  MyEmpty,
  MyIcon,
  MyIconButton,
  MyInput,
  MyStatusDot,
  toast,
  type MyDropdownItem,
} from "myui";
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
import ConnIcon from "./ConnIcon.vue";
import { connectionsStore, reloadConnections } from "@/store/connections";
import { connect } from "@/store/sessions";
import { openConnectionDialog, setThemeChoice, ui } from "@/store/ui";
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
  /** 整棵子树（含本层）的连接总数。徽标显示它，而不是 conns.length。
   *  只数直接子级时，「连接都在二级文件夹里」的父级行会显示不出任何数字，
   *  折叠状态下无法判断这棵树里有多少连接。 */
  totalConns: number;
}

function buildTree(conns: ConnectionConfig[], folders: string[]): FolderNode {
  const root: FolderNode = { path: "/", name: "root", depth: 0, children: [], conns: [], totalConns: 0 };
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
      totalConns: 0,
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

  // 后序累加子树连接数：父级 = 本层 + 所有后代。
  // 放在树上做而不是另开一个「按 group_path 前缀匹配」的统计，是为了不依赖
  // 路径字符串约定——树的实际结构就是唯一事实来源。
  const tallyRec = (n: FolderNode): number => {
    let total = n.conns.length;
    for (const child of n.children) total += tallyRec(child);
    n.totalConns = total;
    return total;
  };
  tallyRec(root);

  return root;
}

// 连接类型图标与语义色统一来自 ConnIcon.vue（myui MyIcon：Server/Sftp/Ftp/LocalTerminal）。
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

// 行内 ⋯ 菜单：MyDropdown 自带"同一时刻至多一个 + 点外关闭 + 视口碰撞翻转"，
// 故不再需要旧版的「每行独立状态 + 全屏遮罩 + 手工 flip 判定」。
// 菜单项是静态的；"属于哪条连接"由模板闭包在 @command 里带上，无需全局状态。
const ROW_MENU_ITEMS: MyDropdownItem[] = [
  { command: "connect", label: "连接", name: "Connection" },
  { command: "edit", label: "编辑", name: "EditPen" },
  { command: "rename", label: "重命名", name: "EditPen" },
  { command: "copy", label: "复制", name: "CopyDocument" },
  { command: "delete", label: "删除", name: "Delete", danger: true },
];

function rowMenuItems(): MyDropdownItem[] {
  return ROW_MENU_ITEMS.map((it) => ({ ...it }));
}

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

/** 连接行 ⋯ 菜单派发：菜单项 + 当前连接 → 原有各处理函数。
 *
 *  ⚠️ 2026-10-10 修复「菜单能展开、点任何一项都没反应」：myui 的 `@command`
 *  回传的是**整个 item 对象**，不是 item.command 的值 —— 见 MyDropdownItem.command
 *  的类型注释原文：「command 载荷；@command 回传整个 item，业务从 item.command
 *  取值」。此前这里写的是 `String(cmd)`，把对象直接 String() 得到
 *  `"[object Object]"`，switch 一个分支都命中不了，于是菜单点什么都静默返回。
 *  （`tsconfig` 的 `strict` 抓不到：旧签名 `cmd: string | number | object` 与
 *  库声明的 `onCommand?: (item: MyDropdownItem) => any` 兼容，模板里的箭头函数
 *  形参又没有类型标注。） */
function onRowMenuCommand(item: MyDropdownItem, conn: ConnectionConfig): void {
  switch (String(item.command ?? "")) {
    case "connect":
      rowMenuConnect(conn);
      break;
    case "edit":
      rowMenuEdit(conn);
      break;
    case "rename":
      rowMenuRename(conn);
      break;
    case "copy":
      rowMenuCopy(conn);
      break;
    case "delete":
      rowMenuDelete(conn);
      break;
  }
}

/** 行内交互控件（目前是行尾的「⋯」菜单钮）。落在这类元素上的 pointerdown
 *  不能进入长按拖拽流程。
 *
 *  2026-10-10 修复「⋯ 菜单点不开」：beginDrag 第一件事就是给行元素
 *  `el.setPointerCapture(pointerId)`。指针捕获生效后，浏览器把后续的 pointerup
 *  以及与之配对的兼容性 mouseup / click 一并重定向到**捕获元素（行本身）**，
 *  而 click 的目标是 pointerdown 目标与 pointerup 目标的最近共同祖先 ——
 *  即 `<button class="conn-more">` 与 `.conn-row` 的共同祖先 = 行本身。
 *  于是按钮上的 click 永远不会触发，ElDropdown 的触发器与业务 @click 一起哑火，
 *  表现为「⋯ 点不动」。这与 dragState 无关，是纯粹的事件路由问题，
 *  所以在入口按事件目标短路，而不是去改拖拽状态机。 */
const ROW_INTERACTIVE_SELECTOR =
  'button, a, input, select, textarea, [role="button"], [contenteditable="true"]';

/** 连接行 pointerdown 入口：长按拖拽仅树形视图可用（搜索平铺行不可拖，与旧版一致）。 */
function onConnPointerDown(conn: ConnectionConfig, noDrag: boolean, e: PointerEvent): void {
  if (noDrag) return;
  const target = e.target;
  if (target instanceof Element && target.closest(ROW_INTERACTIVE_SELECTOR)) return;
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
// 行内 ⋯ 菜单：菜单项的 command 直接带连接对象，MyDropdown 点击后收起。
function rowMenuConnect(c: ConnectionConfig): void {
  handleConnect(c);
}
function rowMenuEdit(c: ConnectionConfig): void {
  handleEdit(c);
}
function rowMenuRename(c: ConnectionConfig): void {
  void handleRenameConnection(c.id, c.name);
}
function rowMenuCopy(c: ConnectionConfig): void {
  void handleCopy(c.id);
}
function rowMenuDelete(c: ConnectionConfig): void {
  handleDeleteConnection(c);
}

// ── 壳层入口 ────────────────────────────────────────────────────────────────
// 侧栏的收起/展开入口只保留顶栏最左一枚 ☰（AppTopBar.vue toggleSidebar），
// 侧栏自身不再提供 —— 隐藏态下这里整个不可见，留着也没人点得到。
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
</script>

<template>
  <!-- 侧栏本体。
       此前的「折叠态」是一整块 44px 窄条模板（品牌 Logo + 一枚展开钮），
       现在 `ui.sidebarCollapsed` 的语义改为**完全隐藏**（壳层第一列归零 +
       display:none），窄条随之删除 —— 收起/展开只由顶栏最左的 ☰ 负责，
       侧栏里不该再有第二个入口。
       组件本身没有 v-if：壳层用 display:none 隐藏，组件仍留在树上，
       连接树的展开态 / 搜索词 / 右键菜单状态在隐藏再显示后原样还在。 -->
  <div
    class="side"
    :style="{ width: `${ui.sidebarWidth}px`, minWidth: `${ui.sidebarWidth}px` }"
    @click="closeMenu"
    @contextmenu="onBlankContext"
  >
      <!-- 头部：动作图标行（myui/EP 单色图标体系） -->
    <div class="side-header">
      <div class="header-actions">
        <MyIconButton
          :title="isDark ? '切换到亮色模式' : '切换到暗色模式'"
          class="icon-btn"
          :label="isDark ? '切换到亮色模式' : '切换到暗色模式'"
          size="15"
          :name="isDark ? 'Sunny' : 'Moon'"
          @click="toggleTheme"
        />
        <MyIconButton
          title="快捷命令"
          class="icon-btn"
          label="快捷命令"
          size="15"
          name="MagicStick"
          @click="openQuickCommands"
        />
        <!-- 设置入口只在顶栏（右上角），此处不再重复，避免头部一行过挤 -->
        <MyIconButton
          title="刷新"
          class="icon-btn"
          label="刷新"
          size="15"
          name="RefreshRight"
          @click="void reloadConnections()"
        />
        <MyIconButton
          title="回收站"
          class="icon-btn"
          label="回收站"
          size="15"
          name="Delete"
          @click="ui.showRecycle = true"
        />
        <MyIconButton
          title="新建文件夹"
          class="icon-btn"
          label="新建文件夹"
          size="15"
          name="FolderAdd"
          @click="void handleAddFolder('/')"
        />
        <MyIconButton
          title="新建连接"
          class="icon-btn"
          label="新建连接"
          size="15"
          name="Plus"
          @click="handleAddNew()"
        />
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
          <MyIcon :size="13" name="Search" />
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
        <span v-if="dragHintTarget" class="drag-hint-target">当前目标：<span class="drag-hint-folder"><MyIcon :size="11" style="vertical-align: -1px" name="Folder" /> {{ dragHintTarget }}</span></span>
        <span v-else class="drag-hint-none">当前目标：移到文件夹上以选择</span>
      </div>

      <template v-for="row in rows" :key="row.key">
        <!-- 文件夹行：data-folder-path 供拖拽 elementFromPoint 命中测试 -->
        <div
          v-if="row.kind === 'folder'"
          class="folder-row"
          :class="{ 'drop-target': row.isDropTarget }"
          :style="{ paddingLeft: `${8 + row.node.depth * 12}px` }"
          :data-folder-path="row.node.path"
          @click="toggle(row.node.path)"
          @contextmenu="onFolderContext(row.node, $event)"
        >
          <!-- 展开指示：myui MyIcon 箭头。折叠=ArrowRight、展开=ArrowDown，
               两个独立图标（非旋转同一图标），方向语义更直白。
               注：ArrowRight 需 myui ≥0.27（0.18 无此图标名，会渲染成 Unknown）。 -->
          <MyIcon class="chevron" :name="row.isOpen ? 'ArrowDown' : 'ArrowRight'" :size="14" />
          <!-- 颜色交给 CSS（.folder-emoji / .folder-emoji.open / :hover），
               内联 :color 会挡住悬停变色（内联优先级高于类选择器） -->
          <span class="folder-emoji" :class="{ open: row.isOpen }">
            <MyIcon :size="14" :icon="row.isOpen ? 'FolderOpened' : 'Folder'" />
          </span>
          <span class="folder-name" :title="row.node.name">{{ row.node.name }}</span>
          <MyBadge
        v-if="row.node.totalConns > 0"
        class="folder-count"
        type="info"
        :value="row.node.totalConns"
        :title="`本文件夹及其子文件夹共 ${row.node.totalConns} 个连接`"
      />
        </div>

        <!-- 连接行：双击/回车连接；长按拖拽移动；右键仅抑制系统菜单（动作在 ⋯ 菜单） -->
        <div
          v-else
          class="conn-row"
          :class="{ 'is-dragged': dragState?.connId === row.conn.id }"
          :style="{ paddingLeft: `${12 + row.depth * 12}px` }"
          tabindex="0"
          @pointerdown="onConnPointerDown(row.conn, row.noDrag, $event)"
          @dblclick="handleConnect(row.conn)"
          @keydown.enter="handleConnect(row.conn)"
          @contextmenu.prevent
        >
          <ConnIcon class="conn-icon" :type="connTypeOf(row.conn)" :size="16" />
          <div
            class="conn-main"
            :title="`${row.conn.name}${row.conn.host ? `\n${row.conn.host}${row.conn.port ? `:${row.conn.port}` : ''}` : ''}`"
          >
            <span class="conn-name">{{ row.conn.name }}</span>
            <span v-if="row.showGroupPath && row.conn.group_path && row.conn.group_path !== '/'" class="conn-group">
              <MyIcon :size="10" style="vertical-align: -1px" name="FolderOpened" />
              {{ row.conn.group_path.startsWith("/") ? row.conn.group_path.slice(1) : row.conn.group_path }}
            </span>
            <span v-if="row.showGroupPath" class="conn-host">{{ row.conn.host }}:{{ row.conn.port }}</span>
          </div>
          <MyDropdown
          trigger="click"
          placement="bottom-end"
          :items="rowMenuItems()"
          @command="(item) => onRowMenuCommand(item, row.conn)"
        >
          <!-- MyDropdown 的 default 插槽**就是触发元素**（EP ElDropdown 把它当
               reference）。迁移时只写了 items/trigger/@command 而漏了这个插槽，
               等于一个触发点都没有 —— 菜单永远弹不出来，表现为「⋯ 点不动」。
               这里用 MyIconButton 补回触发钮，与行内其他动作钮同一套观感。 -->
          <MyIconButton
            title="更多操作" class="conn-more" label="更多操作" size="15" name="More" />
        </MyDropdown>
        </div>
      </template>

      <!-- 搜索无结果 -->
      <MyEmpty
        v-if="filteredConnections.length === 0 && searchQuery"
        class="empty"
        icon="Search"
        :image-size="26"
        description="未找到匹配的连接"
        hint="尝试其他关键词"
        compact
      />
      <!-- 完全为空 -->
      <MyEmpty
        v-if="connectionsStore.connections.length === 0 && connectionsStore.folders.length === 0 && !searchQuery"
        class="empty"
        icon="Monitor"
        :image-size="26"
        description="暂无连接"
        hint="点击 + 新建"
        compact
      />
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
      <MyStatusDot
        v-if="updateAvailable"
        class="update-dot"
        tone="accent"
        glow
        aria-label="发现新版本"
      />
      <span
        class="feedback-icon"
        :class="{ 'no-push': !updateAvailable }"
        title="反馈与建议"
        @click.stop="openFeedback"
      ><MyIcon :size="13" name="Message" /></span>
    </div>

    <!-- 空白区右键菜单 -->
    <div v-if="menu" class="ctx-menu" :style="{ left: `${menu.x}px`, top: `${menu.y}px` }" @click.stop>
      <template v-if="menu.kind === 'blank'">
        <div class="menu-item" @click="ctxNewConnection"><span class="menu-ico"><MyIcon :size="13" name="Plus" /></span>新建连接</div>
        <div class="menu-item" @click="ctxNewFolder"><span class="menu-ico"><MyIcon :size="13" name="FolderAdd" /></span>新建文件夹</div>
      </template>
      <template v-else>
        <div class="menu-item" @click="ctxNewConnectionInFolder"><span class="menu-ico"><MyIcon :size="13" name="Plus" /></span>新建连接</div>
        <div class="menu-item" @click="ctxNewSubFolder"><span class="menu-ico"><MyIcon :size="13" name="FolderAdd" /></span>新建子文件夹</div>
        <div class="menu-item" @click="ctxRenameFolder"><span class="menu-ico"><MyIcon :size="13" name="EditPen" /></span>重命名</div>
        <div class="menu-item danger" @click="ctxDeleteFolder"><span class="menu-ico"><MyIcon :size="13" name="Delete" /></span>删除</div>
      </template>
    </div>

    <!-- 连接行 ⋯ 菜单已内联为 MyDropdown（见行内 ⋯ 触发钮）：
         迁移前是「全屏遮罩 + getBoundingClientRect 手工定位 + flip 判定」
         三段自建逻辑，右侧空间不足时要手工 translateX(-100%) 翻边。
         popper 自带视口碰撞翻转与点外关闭，故这三条一并删除。 -->

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
          <span class="confirm-ico"><MyIcon :size="18" color="var(--warning)" name="Warning" /></span>
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
          <span class="confirm-ico"><MyIcon :size="18" color="var(--warning)" name="Warning" /></span>
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
  </div>
</template>

<style scoped>
/* ── 容器：毛玻璃（与框架顶栏同配方——半透明面 + backdrop 模糊 + 玻璃描边） ── */
.side {
  position: relative;
  background: var(--surface-translucent);
  backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  -webkit-backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-saturation, 180%));
  border-right: 1px solid var(--glass-border);
  display: flex;
  flex-direction: column;
}
/* ── 头部 ── */
.side-header {
  /* 横向 padding 收窄（16→8）：侧栏最窄 200px 时 6 个 28px 图标 + gap2
     恰好单行（194px），不再溢出/换行 */
  padding: 12px 8px;
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
  gap: 2px;
  align-items: center;
  /* 兜底：更窄时在头部内换行，而不是借 .side 的
     overflow:visible 溢出面板（骑跨按钮需要 visible） */
  flex-wrap: wrap;
}
/* 头部动作钮：外观（悬停/按下/禁用/焦点环）由 MyIconButton 承担，
   这里只把尺寸压到侧栏密度，并保留静止态的次级文字色——
   tertiary 在 --bg-elevated 上仅 ~3.8:1，暗色下整排图标会糊成一团灰。 */
.icon-btn {
  width: 28px;
  height: 28px;
  color: var(--text-secondary);
}
/* 悬停/按下改用强调色系（库的默认是中性 bg-elevated）：
   「悬停变主色」是本项目一贯的可用性反馈，标题栏与命令栏一致。 */
.icon-btn:hover {
  background: var(--accent-primary-muted);
  border-color: var(--border-accent);
  color: var(--accent-primary);
}
.icon-btn:active {
  background: var(--bg-surface-active);
  color: var(--accent-primary-hover);
}

/* ── 搜索框 ── */
.search-box {
  /* 12→8：与列表行的左侧缩进节奏对齐（行 padding-left 8px + 行右 margin 6px），
     否则输入框左缘会比下面所有行的 chevron 更靠右，视觉上像多缩进一级。 */
  padding: 8px 8px 8px 8px;
  border-bottom: 1px solid var(--border-subtle);
}
/* MyInput 接管搜索框观感；这里只约束外层宽度 */
.search-box :deep(.my-field) {
  width: 100%;
}
.search-count {
  margin-top: 6px;
  font-size: 11px;
  color: var(--text-tertiary);
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
  color: var(--text-tertiary);
  font-size: 12px;
}

/* 文件夹行 */
.folder-row {
  /* 左侧缩进只保留一处。此前是 margin-left:6px + padding-left:16px 叠加出 22px，
     行首 chevron 之前留出一条空列（见 feedback 截图），侧栏最窄 200px 时白扔
     1/10 宽度。这里去掉行的左外边距，内边距压到 8px —— 与右侧 16px 相比
     刻意更窄，因为左侧还要叠 depth*12px 的层级缩进，两者相加才接近视觉平衡。
     纵向 7→5，与连接行同步收紧（阶段 190）：文件夹行是单行文字，原 7px 会
     让它比连接行矮一截，列表密度不统一。 */
  padding: 5px 16px 5px 8px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  /* secondary → 暗色下 #8b949e 对 --bg-elevated 约 6:1，标题层级清晰不糊 */
  color: var(--text-secondary);
  user-select: none;
  transition: background var(--duration-fast) var(--ease-out-expo), color var(--duration-fast) var(--ease-in-out);
  border-radius: var(--radius-md);
  /* 只保留右外边距：hover/drop 高亮的圆角背景靠它与右缘保持 6px 呼吸位，
     左侧则由 padding-left 统一承担缩进，不再叠加（见上方注释）。
     行间距同样收到 0（阶段 190）。 */
  margin: 0 6px;
}
/* drop target 高亮只在 isDropTarget 时生效；非 target 时保持 hover 背景 */
.folder-row:not(.drop-target):hover {
  background: var(--bg-surface-hover);
  color: var(--text-primary);
}
/* 悬停只提亮文字，不把折叠文件夹染成强调色——蓝色专指"已展开的分支"，
   否则折叠项悬停时与展开项同色，层级线索就废了 */
.folder-row.drop-target {
  background: var(--accent-primary-muted);
  box-shadow: inset 0 0 0 1px var(--border-accent);
  color: var(--text-primary);
}
/* 展开箭头（MyIcon 箭头图标）：
   替换原先的 CSS 字符 ›（8px + opacity .75，实测几乎不可见）。
   折叠 → ArrowRight、展开 → ArrowDown（按状态换图标，不再靠 rotate）。
   颜色走 secondary，悬停提到 primary。 */
.chevron {
  color: var(--text-secondary);
  flex-shrink: 0;
  transition: color var(--duration-fast) var(--ease-in-out);
}
.folder-emoji {
  font-size: 14px;
  display: inline-flex;
  align-items: center;
  transition: color var(--duration-fast) var(--ease-in-out);
  /* 折叠态：跟随层级用中性亮灰（tertiary 在暗色下只有 3.8:1，图标会糊掉）；
     展开态：强调色点出"当前展开的分支"（对齐参考图的蓝色文件夹） */
  color: var(--text-secondary);
}
.folder-emoji.open {
  color: var(--accent-primary);
}
.folder-row:hover .chevron {
  color: var(--text-primary);
}
.folder-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  /* 层级：文件夹=菜单(中层) / 连接=会话(顶层)，两者必须一眼可分。
     连名是纯 primary(14.8:1)，此处混 40% primary → 9.3:1，亮度差 1.6x；
     若继续调到 78%(12.8:1) 只差 1.16x，就和连接名糊成一片了（走过的弯路）。
     用 color-mix 而非硬编码，33 套预设（含 myui oklch 灰阶）自动跟随。 */
  color: color-mix(in oklab, var(--text-primary) 40%, var(--text-secondary));
  font-weight: 500;
}
.folder-count {
  /* 文件夹内连接数徽标：外观与配色由 MyBadge 承担，这里只压一下字号
     （侧栏密度高于通用场景）并靠右推。原先手搓的版本是 muted 色压在
     surface 上只有 ~1.8:1，徽标几乎不可见 —— 这类可读性判断随组件走。 */
  margin-left: auto;
  transform: scale(0.88);
  transform-origin: right center;
}

/* 连接行。
   右侧内边距与行外边距都收到最小：此前是 padding-right 6px + 行 margin-right 6px
   再加 MyIconButton 自身的 36px 盒（字形居中 ⇒ 右侧还空 18px），「⋯」的字形离
   侧栏右缘仍有约 30px 空白。用户要求「右边留一点点」——把行的右外边距清零、
   内边距收到 2px，并让 .conn-more 自身收窄，使字形几乎贴住右缘；腾出的宽度
   全部让给连接名（侧栏只有 264px，长名形如「火山_115.190.136…」本来就常被截断）。
   左侧的 padding-left 由模板按层级 depth 动态覆盖，不受此处影响。 */
.conn-row {
  /* 同 .folder-row：左侧缩进去掉行的 6px 外边距，内边距 24→12。
     连接名在此之后右移 18px，264px 侧栏下长名多露一截（阶段 181 的右缘
     贴边结论保持不变 —— 这里只动左边）。 */
  /* 纵向内边距收到 2px —— 行高完全由文字撑出，上下只留 2px 呼吸位。
     树形视图下连接行是**单行**（`.conn-host` 受 `v-if="showGroupPath"` 控制，
     只有搜索结果才显示 IP 副标题），文字块 13px × 1.25 ≈ 19px，
     实测行高从 29px 降到 23px，一屏可多放约 2 条连接。
     注意：必须同时放开 `.conn-more` 的高度（见下），否则 36px 的按钮盒
     反而成为新瓶颈，行高卡在 29px 下不去 —— padding 改成 2px 却看不出变化。 */
  padding: 2px 2px 2px 12px;
  cursor: grab;
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  color: var(--text-primary);
  outline: none;
  transition: background var(--duration-fast) var(--ease-in-out), box-shadow var(--duration-fast) var(--ease-in-out);
  border-radius: var(--radius-md);
  /* 行间距 1→0：行高已收窄，再留 1px 只会让密度看起来仍偏松。 */
  margin: 0;
  /* 会话=叶子节点，比文件夹(菜单)权重更高：文件名主色已是最亮，
     再用 600 字重把它从"目录标题"里拎出来 */
  font-weight: 600;
}
.conn-row:not(.is-dragged):hover {
  background: var(--bg-surface-hover);
  /* 左侧强调条：悬停行在长列表里更好定位 */
  box-shadow: inset 2px 0 0 0 var(--accent-primary);
}
.conn-row:focus-visible {
  box-shadow: inset 0 0 0 1px var(--border-accent);
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
  /* 不再叠透明度：类型色（蓝/青/黄）本身就是识别线索，压暗后反而认不出 */
  flex-shrink: 0;
}
.conn-main {
  flex: 1;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  /* 主名与副标题（IP / 分组路径）的间距 2→1px：配合收紧的上下内边距，
     副标题仍与主名有可辨的分组，不至于糊成一行。 */
  gap: 1px;
}
.conn-name {
  /* 显式行高 1.25（默认 1.5 ⇒ 19.5px）：这是连接行变高的主要来源。
     13px 字号下 1.25 仍留出正常的字面呼吸，不会上下贴边。 */
  line-height: 1.25;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  /* 字重由 .conn-row 统一给（600），此处不重复声明以免规则打架 */
}
.conn-group {
  font-size: 10px;
  line-height: 1.3;
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.conn-host {
  font-size: 10px;
  /* 副标题同样收行高（默认 1.5 ⇒ 15px → 1.3 ⇒ 13px）。 */
  line-height: 1.3;
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 连接行「⋯」触发钮：盒子与焦点环由 MyIconButton 承担，这里只保留
   静止态次级色与「悬停转强调色」的项目惯例（库默认悬停是中性底）。
   宽度收到 24px：MyIconButton 默认 36px 盒、字形居中，光是盒子自身就在
   右缘留出 6px 空白，再叠加行的 2px 内边距就又是 8px；收窄后字形才真正
   贴住侧栏右缘（悬停底色也随之内收，不再溢出到行外）。 */
.conn-more {
  color: var(--text-secondary);
  flex: none;
  width: 24px;
  padding: 0;
  /* 高度放开（阶段 193）：MyIconButton 默认 36px 盒，比 23px 的行还高，
     在 padding 收到 2px 之后它就成了行高的唯一决定者。改成 height:auto
     后按钮随行高收缩，"⋯" 的可点区从 36×24 变为 23×24 ——
     横向未变，纵向随行高收紧，是这次降低密度的必要代价。 */
  height: auto;
}

.conn-more:hover {
  background: var(--accent-primary-muted);
  color: var(--accent-primary);
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
/* 高度必须钉死 36px，与右侧 CommandBar / ServerInfoPanel 一致。
   此前只有 `padding: 9px 16px`、高度由内容撑出（约 35px），于是这条
   border-top 比同排相邻两条线低 1~2px，整条底栏看着是「歪」的
   （用户反馈"底部的线条高度不一致"）。三者同为 36px + 1px 上边框，
   box-sizing:border-box 下总高一致，顶线自然对齐。 */
.footer {
  height: 36px;
  min-height: 36px;
  flex-shrink: 0;
  border-top: 1px solid var(--border-subtle);
  padding: 0 16px;
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
  color: var(--text-secondary);
}
.footer-version {
  font-size: 11px;
  color: var(--text-tertiary);
}
/* 更新提示圆点：光晕与尺寸由 MyStatusDot 的 glow 承担，这里只靠右推。 */
.update-dot {
  margin-left: auto;
}
.feedback-icon {
  font-size: 13px;
  line-height: 1;
  cursor: pointer;
  padding: 0 2px;
  color: var(--text-tertiary);
  transition: color var(--duration-fast) var(--ease-in-out);
}
.feedback-icon:hover {
  color: var(--accent-primary);
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
</style>
