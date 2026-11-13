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

/** 解析某终端实际使用的渲染器。
 *
 *  auto → **webgl**：终端背景现在恒透明（底色由外层玻璃 div 承载，见
 *  TerminalPanel 的 .terminal-wrap），而透明合成只有 WebGL 干净 ——
 *  canvas 渲染器在透明背景上会留拖影、输入字符看起来「跳跃」
 *  （本地 ConPTY 路径最明显，见 TerminalPanel 里 loadRenderer 的长注释）。
 *  此前是「有背景图才 webgl、否则 canvas」，前提是终端自己画不透底的
 *  theme.background；这个前提随玻璃化改造消失了。
 *
 *  webgl 不可用（老 GPU/驱动）时 loadRenderer 会回退 canvas，功能不受影响。
 *  显式指定 dom/canvas 的仍然尊重用户选择。 */
export function resolveRenderer(pref: RendererBackend): "dom" | "canvas" | "webgl" {
  if (pref === "webgl") return "webgl";
  if (pref === "dom") return "dom";
  if (pref === "canvas") return "canvas";
  return "webgl";
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
