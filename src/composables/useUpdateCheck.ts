// 更新检查（Gitee latest release）。从旧 useUpdateCheck.ts 移植为响应式单例。
// enabled 翻转为 true 后每次会话只自动检查一次；checkNow 供 About 手动复查。
import { ref } from "vue";
import { checkForUpdates, type UpdateInfo } from "../api";

export const updateInfo = ref<UpdateInfo | null>(null);
export const updateChecking = ref(false);

let inFlight = false;
let autoRan = false;

export async function runUpdateCheck(): Promise<void> {
  if (inFlight) return;
  inFlight = true;
  updateChecking.value = true;
  try {
    updateInfo.value = await checkForUpdates();
  } catch {
    /* checkForUpdates 失败时自行 resolve，此分支兜底：保持原值 */
  } finally {
    inFlight = false;
    updateChecking.value = false;
  }
}

/** 保险库就绪后调用：本会话首次触发自动检查。 */
export function autoCheckUpdates(): void {
  if (autoRan) return;
  autoRan = true;
  void runUpdateCheck();
}

export function checkNow(): void {
  void runUpdateCheck();
}
