// 保险库门禁状态机 + 空闲自动锁。逻辑从旧 App.tsx 的 vault useEffect 群原样移植。
// 状态：checking（查询中/已上锁回到这里）→ setup（未初始化）/ unlock（已锁）/ ready（已解锁）。
import { ref, watch, onScopeDispose } from "vue";
import { vaultStatus, lockVault } from "../api";
import { isTauri } from "../utils/environment";

export type VaultState = "checking" | "setup" | "unlock" | "ready";

export const vault = ref<VaultState>("checking");

/** 查询后端保险库状态并推进状态机（未 ready 时由 App.vue 周期驱动）。
 *  浏览器预览（无 Tauri IPC）直接落到 setup 态，不依赖 invoke 抛错。 */
export async function refreshVaultStatus(): Promise<void> {
  if (vault.value === "ready") return;
  if (!isTauri()) {
    vault.value = "setup";
    return;
  }
  try {
    const s = await vaultStatus();
    if (!s.initialized) vault.value = "setup";
    else if (s.unlocked) vault.value = "ready";
    else vault.value = "unlock";
  } catch {
    vault.value = "setup";
  }
}

// ── 空闲自动锁 ──────────────────────────────────────────────────────────
// 背景：窗口最小化时 webview 会节流 setTimeout，纯定时器不可靠。
// 因此同时记录 lastActivityAt（墙钟），窗口重新可见/聚焦时补查。
let autoLockTimer: ReturnType<typeof setTimeout> | null = null;
let autoLockListeners: Array<() => void> = [];

function getAutoLockMinutes(): number {
  const raw = localStorage.getItem("myshell-auto-lock-minutes") ?? "30";
  const n = parseInt(raw, 10);
  return isNaN(n) ? 30 : n;
}

export function startAutoLockWatcher(onLock: () => void): void {
  stopAutoLockWatcher();

  const minutes = getAutoLockMinutes();
  if (minutes <= 0) return; // 已禁用

  const timeoutMs = () => minutes * 60 * 1000;
  let lastActivityAt = Date.now();
  let lastReset = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const doLock = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    lockVault().catch(() => {
      /* 尽力而为 */
    });
    stopAutoLockWatcher();
    onLock();
  };

  const armTimer = () => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(doLock, timeoutMs());
  };

  const checkElapsed = () => {
    if (Date.now() - lastActivityAt >= timeoutMs()) doLock();
    else armTimer();
  };

  const onActivity = () => {
    lastActivityAt = Date.now();
    // 节流：至多 5 秒重置一次（避免 mousemove 洪泛）
    const now = Date.now();
    if (now - lastReset < 5000) return;
    lastReset = now;
    if (timer) armTimer();
  };

  const onVisibility = () => {
    if (document.visibilityState === "visible") checkElapsed();
  };

  armTimer();
  const events: Array<keyof WindowEventMap> = ["mousemove", "keydown", "click", "wheel", "touchstart"];
  events.forEach((ev) => window.addEventListener(ev, onActivity, { capture: true, passive: true }));
  window.addEventListener("visibilitychange", onVisibility);
  window.addEventListener("focus", checkElapsed);
  window.addEventListener("pageshow", checkElapsed);
  // 设置面板改动后广播此事件，即时生效
  window.addEventListener("myshell-auto-lock-changed", () => startAutoLockWatcher(onLock));

  autoLockListeners = [
    () => events.forEach((ev) => window.removeEventListener(ev, onActivity, { capture: true })),
    () => window.removeEventListener("visibilitychange", onVisibility),
    () => window.removeEventListener("focus", checkElapsed),
    () => window.removeEventListener("pageshow", checkElapsed),
  ];
}

export function stopAutoLockWatcher(): void {
  if (autoLockTimer) {
    clearTimeout(autoLockTimer);
    autoLockTimer = null;
  }
  autoLockListeners.forEach((off) => off());
  autoLockListeners = [];
}

/** 把 vault 状态机作为应用级作用域挂载（App.vue setup 里调用一次）。 */
export function useVault(): { vault: typeof vault; refreshVaultStatus: typeof refreshVaultStatus } {
  const stop = watchVault();
  onScopeDispose(stop);
  return { vault, refreshVaultStatus };
}

function watchVault(): () => void {
  const stop = watch(vault, (state) => {
    if (state === "ready") {
      startAutoLockWatcher(() => {
        vault.value = "checking";
        void refreshVaultStatus();
      });
    } else {
      stopAutoLockWatcher();
    }
  });
  return stop;
}
