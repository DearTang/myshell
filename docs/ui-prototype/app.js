const root = document.documentElement;
const body = document.body;
const appFrame = document.getElementById("appFrame");
const sidebarTrigger = document.getElementById("sidebarTrigger");
const themeToggle = document.getElementById("themeToggle");
const localeToggle = document.getElementById("localeToggle");
const localeLabel = document.getElementById("localeLabel");
const settingsButton = document.getElementById("settingsButton");
const settingsDrawer = document.getElementById("settingsDrawer");
const closeSettings = document.getElementById("closeSettings");
const drawerBackdrop = document.getElementById("drawerBackdrop");
const commandSearch = document.getElementById("commandSearch");
const commandPalette = document.getElementById("commandPalette");
const paletteInput = document.getElementById("paletteInput");
const versionButton = document.getElementById("versionButton");
const updateModal = document.getElementById("updateModal");
const toastRegion = document.getElementById("toastRegion");
const layoutGuideButton = document.getElementById("layoutGuideButton");
const notificationButton = document.getElementById("notificationButton");
const navTooltip = document.getElementById("navTooltip");

const state = {
  locale: "zh",
  theme: "dark",
  sidebarCollapsed: false,
  activePage: "overview",
};

const translations = {
  zh: {
    brandSubtitle: "统一工作台", workspaceStatus: "工作区在线", search: "搜索功能或命令", role: "管理员",
    navWorkspace: "工作区", navTools: "工具", overview: "总览", connections: "连接管理", terminal: "终端会话",
    transfers: "文件传输", automation: "自动化任务", monitoring: "运行监控", logs: "操作日志",
    resourceUsage: "资源使用", resourceDetail: "8.2 GB / 12 GB 本地缓存", spaceName: "生产环境",
    welcome: "欢迎回来，Argus", welcomeDescription: "12 台主机已纳入统一管理，当前所有关键服务运行正常。",
    layoutGuide: "架构标注", newConnection: "新建连接", managedHosts: "已管理主机", thisMonth: "本月",
    onlineSessions: "在线会话", todayTransfers: "今日传输", pendingEvents: "待处理事件", needAttention: "需要关注",
    activeSessions: "活跃会话", all: "全部", server: "服务器", status: "状态", resource: "资源", lastActive: "最近活动",
    online: "在线", transferring: "传输中", idle: "空闲", justNow: "刚刚", viewAllConnections: "查看全部连接",
    quickTerminal: "快速终端", typeCommand: "输入命令，按 Enter 发送", transferQueue: "传输队列", completed: "完成", queued: "等待中",
    systemHealth: "系统健康度", healthy: "健康", memory: "内存", disk: "磁盘", network: "网络", recentActivity: "最近活动",
    deployComplete: "部署已完成", fileDownloaded: "文件下载完成", memoryWarning: "内存使用率偏高", twoMinutesAgo: "2 分钟前",
    eightMinutesAgo: "8 分钟前", twentyMinutesAgo: "20 分钟前", assistantReady: "助手已就绪",
    assistantQuestion: "需要检查服务器状态吗？", assistantDescription: "我可以汇总资源、日志与服务状态，并生成一份只读巡检报告。",
    startInspection: "开始巡检", settings: "设置", appearance: "界面外观", dark: "深色", light: "浅色", system: "跟随系统",
    layoutDensity: "布局密度", compactMode: "紧凑模式", compactModeDesc: "减少列表和面板的垂直间距",
    showUtilityRail: "显示右侧辅助区", showUtilityRailDesc: "健康度、活动记录与智能助手", showArchitecture: "架构标注模式",
    showArchitectureDesc: "在各个可复用区域上显示名称", accentColor: "品牌强调色", reset: "恢复默认", saveChanges: "保存更改",
    searchActions: "搜索页面、连接或操作…", quickActions: "快捷操作", newConnectionDesc: "添加 SSH、SFTP 或本地终端",
    openTerminal: "打开终端", openSettings: "打开设置", configureWorkspace: "配置界面与工作区", newVersion: "发现新版本 2.15.0",
    updateDescription: "包含工作区性能优化、新的连接诊断面板和 6 项问题修复。", packageSize: "安装包大小", estimatedTime: "预计用时",
    later: "稍后", updateNow: "立即更新", commandSent: "命令已发送到 prod-api-01", settingsSaved: "工作区设置已保存",
    inspectionStarted: "服务器巡检已开始", connectionCreated: "已打开新建连接面板", notificationsRead: "通知已全部标记为已读",
    feedbackOpened: "已打开反馈与建议入口", themeChanged: "界面主题已切换", defaultsRestored: "已恢复默认界面设置", updateStarted: "正在下载 MyShell 2.15.0…",
  },
  en: {
    brandSubtitle: "Unified Console", workspaceStatus: "Workspace online", search: "Search actions or commands", role: "Administrator",
    navWorkspace: "Workspace", navTools: "Tools", overview: "Overview", connections: "Connections", terminal: "Terminal sessions",
    transfers: "File transfers", automation: "Automation", monitoring: "Monitoring", logs: "Activity logs",
    resourceUsage: "Resource usage", resourceDetail: "8.2 GB / 12 GB local cache", spaceName: "Production",
    welcome: "Welcome back, Argus", welcomeDescription: "12 hosts are centrally managed and all critical services are operating normally.",
    layoutGuide: "Architecture map", newConnection: "New connection", managedHosts: "Managed hosts", thisMonth: "this month",
    onlineSessions: "Online sessions", todayTransfers: "Today's transfers", pendingEvents: "Pending events", needAttention: "Needs attention",
    activeSessions: "Active sessions", all: "All", server: "Server", status: "Status", resource: "Resource", lastActive: "Last active",
    online: "Online", transferring: "Transferring", idle: "Idle", justNow: "Just now", viewAllConnections: "View all connections",
    quickTerminal: "Quick terminal", typeCommand: "Type a command and press Enter", transferQueue: "Transfer queue", completed: "Complete", queued: "Queued",
    systemHealth: "System health", healthy: "Healthy", memory: "Memory", disk: "Disk", network: "Network", recentActivity: "Recent activity",
    deployComplete: "Deployment complete", fileDownloaded: "File download complete", memoryWarning: "High memory usage", twoMinutesAgo: "2 minutes ago",
    eightMinutesAgo: "8 minutes ago", twentyMinutesAgo: "20 minutes ago", assistantReady: "Assistant ready",
    assistantQuestion: "Run a server health check?", assistantDescription: "I can summarize resources, logs and service health in a read-only inspection report.",
    startInspection: "Start inspection", settings: "Settings", appearance: "Appearance", dark: "Dark", light: "Light", system: "System",
    layoutDensity: "Layout density", compactMode: "Compact mode", compactModeDesc: "Reduce vertical spacing in lists and panels",
    showUtilityRail: "Show utility rail", showUtilityRailDesc: "Health, activity and intelligent assistant", showArchitecture: "Architecture annotations",
    showArchitectureDesc: "Label each reusable application region", accentColor: "Brand accent", reset: "Reset", saveChanges: "Save changes",
    searchActions: "Search pages, connections or actions…", quickActions: "Quick actions", newConnectionDesc: "Add SSH, SFTP or local terminal",
    openTerminal: "Open terminal", openSettings: "Open settings", configureWorkspace: "Configure the interface and workspace", newVersion: "MyShell 2.15.0 is available",
    updateDescription: "Includes workspace performance improvements, a new connection diagnostics panel and six fixes.", packageSize: "Package size", estimatedTime: "Estimated time",
    later: "Later", updateNow: "Update now", commandSent: "Command sent to prod-api-01", settingsSaved: "Workspace settings saved",
    inspectionStarted: "Server inspection started", connectionCreated: "New connection panel opened", notificationsRead: "All notifications marked as read",
    feedbackOpened: "Feedback panel opened", themeChanged: "Interface theme changed", defaultsRestored: "Default interface settings restored", updateStarted: "Downloading MyShell 2.15.0…",
  },
};

const pageContent = {
  overview: {
    zh: ["总览", "欢迎回来，Argus", "12 台主机已纳入统一管理，当前所有关键服务运行正常。"],
    en: ["Overview", "Welcome back, Argus", "12 hosts are centrally managed and all critical services are operating normally."],
  },
  connections: {
    zh: ["连接管理", "连接与凭据", "集中管理 SSH、SFTP、FTP 与本地终端连接。"],
    en: ["Connections", "Connections and credentials", "Centrally manage SSH, SFTP, FTP and local terminal connections."],
  },
  terminal: {
    zh: ["终端会话", "终端工作区", "查看在线会话、广播组与最近使用的快捷命令。"],
    en: ["Terminal sessions", "Terminal workspace", "Review online sessions, broadcast groups and recent quick commands."],
  },
  transfers: {
    zh: ["文件传输", "传输中心", "统一查看 SFTP、FTP 与 ZMODEM 传输任务。"],
    en: ["File transfers", "Transfer center", "Review SFTP, FTP and ZMODEM transfer tasks in one place."],
  },
  automation: {
    zh: ["自动化任务", "自动化工作流", "编排快捷命令、定时任务与批量服务器操作。"],
    en: ["Automation", "Automation workflows", "Orchestrate quick commands, schedules and bulk server operations."],
  },
  monitoring: {
    zh: ["运行监控", "系统运行态势", "聚合 CPU、内存、磁盘、网络和服务健康度。"],
    en: ["Monitoring", "System operating posture", "Aggregate CPU, memory, disk, network and service health."],
  },
  logs: {
    zh: ["操作日志", "日志与审计", "追踪连接、部署、传输和安全确认记录。"],
    en: ["Activity logs", "Logs and audit", "Trace connections, deployments, transfers and security confirmations."],
  },
};

function t(key) {
  return translations[state.locale][key] || key;
}

function applyLocale() {
  root.lang = state.locale === "zh" ? "zh-CN" : "en";
  document.querySelectorAll("[data-i18n]").forEach((node) => {
    const key = node.dataset.i18n;
    if (translations[state.locale][key]) node.textContent = translations[state.locale][key];
  });
  document.querySelectorAll("[data-i18n-placeholder]").forEach((node) => {
    const key = node.dataset.i18nPlaceholder;
    if (translations[state.locale][key]) node.placeholder = translations[state.locale][key];
  });
  localeLabel.textContent = state.locale === "zh" ? "中" : "EN";
  const content = pageContent[state.activePage][state.locale];
  document.getElementById("pageSection").textContent = content[0];
  document.getElementById("pageTitle").textContent = content[1];
  document.getElementById("pageDescription").textContent = content[2];
}

function setTheme(theme, notify = false) {
  const resolved = theme === "system"
    ? (window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark")
    : theme;
  state.theme = theme;
  root.dataset.theme = resolved;
  themeToggle.setAttribute("aria-label", resolved === "dark" ? "切换亮色模式" : "切换暗色模式");
  document.querySelectorAll("[data-theme-choice]").forEach((button) => {
    button.classList.toggle("active", button.dataset.themeChoice === theme);
  });
  if (notify) showToast(t("themeChanged"));
}

function toggleSidebar() {
  hideNavTooltip();
  if (window.innerWidth <= 680) {
    appFrame.classList.toggle("mobile-menu-open");
    sidebarTrigger.setAttribute("aria-expanded", String(appFrame.classList.contains("mobile-menu-open")));
    return;
  }
  state.sidebarCollapsed = !state.sidebarCollapsed;
  appFrame.classList.toggle("sidebar-collapsed", state.sidebarCollapsed);
  sidebarTrigger.setAttribute("aria-expanded", String(!state.sidebarCollapsed));
  sidebarTrigger.setAttribute("aria-label", state.sidebarCollapsed ? "展开侧边栏" : "收起侧边栏");
}

function openSettingsDrawer() {
  settingsDrawer.classList.add("open");
  drawerBackdrop.classList.add("open");
  settingsDrawer.setAttribute("aria-hidden", "false");
  closeSettings.focus();
}

function closeSettingsDrawer() {
  settingsDrawer.classList.remove("open");
  drawerBackdrop.classList.remove("open");
  settingsDrawer.setAttribute("aria-hidden", "true");
  settingsButton.focus();
}

function openPalette() {
  commandPalette.classList.add("open");
  commandPalette.setAttribute("aria-hidden", "false");
  window.setTimeout(() => paletteInput.focus(), 60);
}

function closePalette() {
  commandPalette.classList.remove("open");
  commandPalette.setAttribute("aria-hidden", "true");
  paletteInput.value = "";
}

function openUpdate() {
  updateModal.classList.add("open");
  updateModal.setAttribute("aria-hidden", "false");
}

function closeUpdate() {
  updateModal.classList.remove("open");
  updateModal.setAttribute("aria-hidden", "true");
}

function toggleSwitch(button, forced) {
  const active = forced ?? !button.classList.contains("active");
  button.classList.toggle("active", active);
  button.setAttribute("aria-checked", String(active));
  return active;
}

function showToast(message) {
  const toast = document.createElement("div");
  toast.className = "toast";
  toast.innerHTML = `<i></i><span>${message}</span>`;
  toastRegion.appendChild(toast);
  window.setTimeout(() => toast.remove(), 2800);
}

function showNavTooltip(item) {
  if (!appFrame.classList.contains("sidebar-collapsed") || window.innerWidth <= 680) return;
  const label = item.querySelector(".nav-text")?.textContent?.trim();
  if (!label) return;
  const rect = item.getBoundingClientRect();
  navTooltip.textContent = label;
  navTooltip.style.left = `${rect.right}px`;
  navTooltip.style.top = `${rect.top + rect.height / 2}px`;
  navTooltip.classList.add("visible");
  navTooltip.setAttribute("aria-hidden", "false");
}

function hideNavTooltip() {
  navTooltip.classList.remove("visible");
  navTooltip.setAttribute("aria-hidden", "true");
}

sidebarTrigger.addEventListener("click", toggleSidebar);
themeToggle.addEventListener("click", () => setTheme(root.dataset.theme === "dark" ? "light" : "dark", true));
localeToggle.addEventListener("click", () => {
  state.locale = state.locale === "zh" ? "en" : "zh";
  applyLocale();
});
settingsButton.addEventListener("click", openSettingsDrawer);
closeSettings.addEventListener("click", closeSettingsDrawer);
drawerBackdrop.addEventListener("click", closeSettingsDrawer);
commandSearch.addEventListener("click", openPalette);
versionButton.addEventListener("click", openUpdate);
document.getElementById("feedbackButton").addEventListener("click", () => showToast(t("feedbackOpened")));
layoutGuideButton.addEventListener("click", () => {
  const enabled = !body.classList.contains("annotation-mode");
  body.classList.toggle("annotation-mode", enabled);
  toggleSwitch(document.getElementById("annotationSwitch"), enabled);
});
notificationButton.addEventListener("click", () => {
  notificationButton.querySelector(".indicator")?.remove();
  showToast(t("notificationsRead"));
});

commandPalette.addEventListener("click", (event) => {
  if (event.target === commandPalette) closePalette();
});
updateModal.addEventListener("click", (event) => {
  if (event.target === updateModal) closeUpdate();
});
document.getElementById("laterUpdate").addEventListener("click", closeUpdate);
document.getElementById("startUpdate").addEventListener("click", () => {
  closeUpdate();
  versionButton.querySelector(".update-dot")?.remove();
  showToast(t("updateStarted"));
});

window.addEventListener("keydown", (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    openPalette();
  }
  if (event.key === "Escape") {
    if (commandPalette.classList.contains("open")) closePalette();
    else if (settingsDrawer.classList.contains("open")) closeSettingsDrawer();
    else if (updateModal.classList.contains("open")) closeUpdate();
    else if (appFrame.classList.contains("mobile-menu-open")) appFrame.classList.remove("mobile-menu-open");
  }
});

window.addEventListener("resize", () => {
  hideNavTooltip();
  if (window.innerWidth > 680) appFrame.classList.remove("mobile-menu-open");
});
document.querySelector(".navigation").addEventListener("scroll", hideNavTooltip, { passive: true });

document.querySelectorAll(".nav-item").forEach((button) => {
  button.addEventListener("mouseenter", () => showNavTooltip(button));
  button.addEventListener("mouseleave", hideNavTooltip);
  button.addEventListener("focus", () => showNavTooltip(button));
  button.addEventListener("blur", hideNavTooltip);
  button.addEventListener("click", () => {
    document.querySelectorAll(".nav-item").forEach((item) => item.classList.remove("active"));
    button.classList.add("active");
    state.activePage = button.dataset.page;
    const content = pageContent[state.activePage][state.locale];
    document.getElementById("pageSection").textContent = content[0];
    document.getElementById("pageTitle").textContent = content[1];
    document.getElementById("pageDescription").textContent = content[2];
    if (window.innerWidth <= 680) appFrame.classList.remove("mobile-menu-open");
  });
});

document.querySelectorAll("[data-theme-choice]").forEach((button) => {
  button.addEventListener("click", () => setTheme(button.dataset.themeChoice, true));
});

document.getElementById("compactSwitch").addEventListener("click", (event) => {
  body.classList.toggle("compact", toggleSwitch(event.currentTarget));
});
document.getElementById("railSwitch").addEventListener("click", (event) => {
  body.classList.toggle("hide-rail", !toggleSwitch(event.currentTarget));
});
document.getElementById("annotationSwitch").addEventListener("click", (event) => {
  body.classList.toggle("annotation-mode", toggleSwitch(event.currentTarget));
});

document.querySelectorAll("[data-accent]").forEach((button) => {
  button.addEventListener("click", () => {
    document.querySelectorAll("[data-accent]").forEach((item) => item.classList.remove("active"));
    button.classList.add("active");
    const hex = button.dataset.accent;
    const value = hex.replace("#", "");
    const rgb = [0, 2, 4].map((index) => parseInt(value.slice(index, index + 2), 16));
    root.style.setProperty("--accent", hex);
    root.style.setProperty("--accent-rgb", rgb.join(", "));
    root.style.setProperty("--accent-hover", hex);
  });
});

document.getElementById("resetSettings").addEventListener("click", () => {
  setTheme("dark");
  body.classList.remove("compact", "hide-rail", "annotation-mode");
  toggleSwitch(document.getElementById("compactSwitch"), false);
  toggleSwitch(document.getElementById("railSwitch"), true);
  toggleSwitch(document.getElementById("annotationSwitch"), false);
  root.style.removeProperty("--accent");
  root.style.removeProperty("--accent-rgb");
  root.style.removeProperty("--accent-hover");
  document.querySelectorAll("[data-accent]").forEach((item, index) => item.classList.toggle("active", index === 0));
  showToast(t("defaultsRestored"));
});

document.getElementById("saveSettings").addEventListener("click", () => {
  closeSettingsDrawer();
  showToast(t("settingsSaved"));
});

document.getElementById("newConnectionButton").addEventListener("click", () => showToast(t("connectionCreated")));
document.getElementById("assistantButton").addEventListener("click", () => showToast(t("inspectionStarted")));

document.getElementById("terminalForm").addEventListener("submit", (event) => {
  event.preventDefault();
  const input = document.getElementById("terminalInput");
  if (!input.value.trim()) return;
  const terminal = document.querySelector(".terminal-screen");
  const line = document.createElement("div");
  line.className = "terminal-line command";
  line.textContent = input.value.trim();
  terminal.insertBefore(line, terminal.lastElementChild);
  input.value = "";
  terminal.scrollTop = terminal.scrollHeight;
  showToast(t("commandSent"));
});

document.querySelectorAll(".segmented button").forEach((button) => {
  button.addEventListener("click", () => {
    document.querySelectorAll(".segmented button").forEach((item) => item.classList.remove("active"));
    button.classList.add("active");
    document.querySelectorAll(".session-data").forEach((row) => {
      row.style.display = button.dataset.filter === "all" || row.dataset.type === button.dataset.filter ? "grid" : "none";
    });
  });
});

document.querySelectorAll(".palette-list button").forEach((button, index) => {
  button.addEventListener("click", () => {
    closePalette();
    if (index === 0) showToast(t("connectionCreated"));
    if (index === 1) showToast(t("commandSent"));
    if (index === 2) openSettingsDrawer();
  });
});

applyLocale();
setTheme("dark");
