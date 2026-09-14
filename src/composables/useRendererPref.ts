// 终端渲染后端偏好 + GPU 开关。从旧 useRendererPref.ts 移植（注释要点见原文件）。
import { ref } from "vue";
import { getGpuAccelerationDisabled, setGpuAccelerationDisabled } from "../api";

export type RendererBackend = "auto" | "dom" | "canvas" | "webgl";

export const STORAGE_KEY_RENDERER = "myshell-renderer-backend";
export const DEFAULT_RENDERER: RendererBackend = "auto";

const VALID_BACKENDS: ReadonlySet<string> = new Set(["auto", "dom", "canvas", "webgl"]);

function readRenderer(): RendererBackend {
  try {
    const v = localStorage.getItem(STORAGE_KEY_RENDERER);
    if (v && VALID_BACKENDS.has(v)) return v as RendererBackend;
  } catch {
    /* 用默认值 */
  }
  return DEFAULT_RENDERER;
}

/** 解析某终端实际使用的渲染器：auto + 背景图 → webgl（唯一能透明合成的），
 *  auto 无背景图 → canvas（光标/选区最稳），显式指定则尊重用户。 */
export function resolveRenderer(pref: RendererBackend, hasBgImage: boolean): "dom" | "canvas" | "webgl" {
  if (pref === "webgl") return "webgl";
  if (pref === "dom") return "dom";
  if (pref === "canvas") return "canvas";
  return hasBgImage ? "webgl" : "canvas";
}

export const rendererBackend = ref<RendererBackend>(readRenderer());

export function setRendererBackend(b: RendererBackend): void {
  rendererBackend.value = b;
  try {
    localStorage.setItem(STORAGE_KEY_RENDERER, b);
  } catch {
    /* 仅内存态 */
  }
}

// ── GPU 加速开关（Rust 落盘，重启后生效）──

export const gpuDisabled = ref(false);

export async function readGpuDisabled(): Promise<boolean> {
  try {
    return await getGpuAccelerationDisabled();
  } catch {
    return false;
  }
}

export async function setGpuDisabled(disabled: boolean): Promise<void> {
  gpuDisabled.value = disabled;
  try {
    await setGpuAccelerationDisabled(disabled);
  } catch (e) {
    console.error("[rendererPref] setGpuAccelerationDisabled failed:", e);
  }
}
