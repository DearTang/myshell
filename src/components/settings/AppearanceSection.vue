<!-- 外观 — 配色方案 / 背景图片 / 终端字体 / 终端渲染（旧 appearance 分区） -->
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { Brush, CircleCheck, Picture } from "@element-plus/icons-vue";
import { MyButton, MySection, MySlider, MyToggle } from "myui";
import { readFileBase64 } from "@/api";
import { compressImageDataUrl } from "@/utils/image";
import {
  appearance,
  setPaletteId,
  setCustomPalette,
  clearCustomPalette,
  setBgImage,
} from "@/store/appearance";
import { ui } from "@/store/ui";
import { primaryFont, setPrimaryFont } from "@/composables/useTerminalFont";
import {
  rendererBackend,
  setRendererBackend,
  gpuDisabled,
  setGpuDisabled,
  readGpuDisabled,
  type RendererBackend,
} from "@/composables/useRendererPref";
import { PRESETS, type ColorPalette } from "@/themes";
import FontField from "@/components/FontField.vue";
import CustomPaletteDialog from "./CustomPaletteDialog.vue";

defineOptions({ name: "AppearanceSection" });

// 背景透明度滑杆范围。与下方 <input type="range"> 保持同步。
const BG_OPACITY_MIN = 10;
const BG_OPACITY_MAX = 100;

// ── 配色方案 ──
const showCustomDialog = ref(false);
const customAccent = ref(
  appearance.customPalette?.dark.ui?.["--accent-primary"]?.toString() || "#6366f1",
);
const customBg = ref(appearance.customPalette?.dark.terminal?.background || "#1e1e2e");

const HEX_3_RE = /^#[0-9a-fA-F]{3}$/;
const HEX_6_RE = /^#[0-9a-fA-F]{6}/;
const HEX_VALID_RE = /^#[0-9a-fA-F]{3}([0-9a-fA-F]{3})?$/;

const accentValid = computed(() => HEX_VALID_RE.test(customAccent.value.trim()));
const bgValid = computed(() => HEX_VALID_RE.test(customBg.value.trim()));

function presetVariant(preset: ColorPalette): ColorPalette["dark"] {
  return ui.isDark ? preset.dark : preset.light;
}

function choosePreset(id: string): void {
  setPaletteId(id);
  clearCustomPalette();
}

function saveCustomPalette(): void {
  // 先规范化为 7 位 #RRGGBB 再追加 alpha 后缀（"cc"、"26"...）。
  // "#RGB" 缩写或 "#RRGGBBAA" 之类的自由文本若不处理，会拼出非法 CSS
  // 颜色（如 "#fffcc"），被浏览器静默丢弃。
  const normHex = (hex: string): string => {
    const h = hex.trim();
    if (HEX_3_RE.test(h)) {
      return (
        "#" +
        h
          .slice(1)
          .split("")
          .map((c) => c + c)
          .join("")
      );
    }
    return HEX_6_RE.test(h) ? h.slice(0, 7) : h;
  };
  const accent = normHex(customAccent.value);
  const bg = normHex(customBg.value);
  const id = `custom-${Date.now()}`;
  const palette: ColorPalette = {
    id,
    name: "自定义主题",
    dark: {
      terminal: { ...PRESETS[0].dark.terminal, background: bg },
      ui: {
        ...PRESETS[0].dark.ui,
        "--accent-primary": accent,
        "--accent-primary-hover": accent + "cc",
        "--accent-primary-muted": accent + "26",
        "--border-accent": accent + "66",
        "--shadow-glow": `0 0 20px ${accent}40`,
        "--bg-base": bg,
      },
    },
    light: {
      terminal: { ...PRESETS[0].light.terminal, background: bg },
      ui: {
        ...PRESETS[0].light.ui,
        "--accent-primary": accent,
        "--accent-primary-hover": accent + "cc",
        "--accent-primary-muted": accent + "1a",
        "--border-accent": accent + "66",
        "--shadow-glow": `0 0 20px ${accent}33`,
      },
    },
  };
  setCustomPalette(palette);
  setPaletteId(id);
  showCustomDialog.value = false;
}

// ── 背景图片（先预览后应用的本地状态） ──
const bgOpacity = ref(appearance.bgImage.opacity * 100);
const bgImageBusy = ref(false);

async function pickBgImage(): Promise<void> {
  bgImageBusy.value = true;
  try {
    const selected = await open({
      multiple: false,
      filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] }],
    });
    if (selected && !Array.isArray(selected)) {
      const raw = await readFileBase64(selected);
      // 存储前先压缩：多 MB 照片的原始 base64 会撑爆 localStorage 的
      // ~5 MB 配额（setItem 抛错被静默吞掉，图片永远不落盘）。
      // 降采样到 ≤1920px + JPEG 可稳定控制在配额以内。
      const dataUrl = await compressImageDataUrl(raw);
      // 立即自动应用
      setBgImage({ dataUrl: dataUrl, opacity: bgOpacity.value / 100 });
    }
  } catch (e) {
    console.error("Failed to read image:", e);
  } finally {
    bgImageBusy.value = false;
  }
}

function onOpacityChange(v: number | number[]): void {
  const val = Array.isArray(v) ? v[0] : v;
  bgOpacity.value = val;
  // 已有背景图时实时更新透明度
  if (appearance.bgImage.dataUrl) {
    setBgImage({ dataUrl: appearance.bgImage.dataUrl, opacity: val / 100 });
  }
}

function clearBgImage(): void {
  setBgImage({ dataUrl: null, opacity: 1 });
  bgOpacity.value = 85;
}

// ── 终端渲染：渲染后端 + GPU 开关。 ──
// rendererBackend 是纯前端（localStorage），对新开的标签页生效。
// gpuDisabled 由 Rust 落盘，下次启动时生效 — 挂载时从旗标文件读一次
// 持久化值。
const gpuDisabledInit = ref(false);

onMounted(() => {
  readGpuDisabled()
    .then((v) => {
      gpuDisabledInit.value = v;
    })
    .catch(() => {
      /* 默认 false 即可 */
    });
});

// GPU 旗标下次启动才生效。内存开关与持久化值不一致时提示"重启生效"。
const gpuPendingRestart = computed(() => gpuDisabled.value !== gpuDisabledInit.value);

function onGpuToggle(v: string | number | boolean): void {
  void setGpuDisabled(Boolean(v));
}

const RENDERER_OPTIONS: { id: RendererBackend; label: string; desc: string }[] = [
  { id: "auto", label: "自动（推荐）", desc: "默认 Canvas，透明背景时用 WebGL" },
  { id: "canvas", label: "Canvas", desc: "最稳定，光标/选区直接画在画布上" },
  { id: "webgl", label: "WebGL", desc: "性能最佳，依赖 GPU 合成" },
  { id: "dom", label: "DOM", desc: "最轻量，仅聚焦时显示光标" },
];
</script>

<template>
  <!-- 配色方案 -->
  <MySection title="配色方案" description="选择预设配色方案，同步更新终端和界面颜色">
    <div class="preset-grid">
      <button
        v-for="preset in PRESETS"
        :key="preset.id"
        class="preset-card"
        :class="{ active: appearance.paletteId === preset.id && !appearance.customPalette }"
        :title="preset.name"
        type="button"
        @click="choosePreset(preset.id)"
      >
        <span
          class="preset-dot"
          :style="{ background: presetVariant(preset).terminal.background }"
        >
          <!-- 色板底部的强调色条 -->
          <span
            class="preset-dot-bar"
            :style="{
              background:
                presetVariant(preset).ui?.['--accent-primary'] ||
                (ui.isDark ? '#6366f1' : '#4f46e5'),
            }"
          />
        </span>
        <span class="preset-name">{{ preset.name }}</span>
      </button>
    </div>

    <!-- 自定义主题入口 -->
    <button
      class="custom-entry"
      :class="{ 'has-custom': !!appearance.customPalette }"
      type="button"
      @click="showCustomDialog = true"
    >
      <span class="custom-entry-icon">
        <el-icon :size="14"><Brush /></el-icon>
      </span>
      {{ appearance.customPalette ? "管理自定义主题" : "自定义主题..." }}
    </button>

    <!-- 自定义主题对话框 -->
    <CustomPaletteDialog
      v-if="showCustomDialog"
      v-model:accent="customAccent"
      v-model:bg="customBg"
      :accent-valid="accentValid"
      :bg-valid="bgValid"
      @save="saveCustomPalette"
      @close="showCustomDialog = false"
    />
  </MySection>

  <!-- 背景图片 -->
  <MySection title="背景图片" description="为终端区域设置背景图片">
    <MyButton
      variant="secondary"
      class="block-btn"
      :disabled="bgImageBusy"
      @click="pickBgImage"
    >
      <el-icon :size="14"><Picture /></el-icon>
      {{ bgImageBusy ? "选择中..." : appearance.bgImage.dataUrl ? "更换图片" : "选择图片" }}
    </MyButton>

    <!-- 图片预览缩略图 -->
    <div
      v-if="appearance.bgImage.dataUrl"
      class="bg-preview"
      :style="{
        backgroundImage: `url(${appearance.bgImage.dataUrl})`,
        opacity: bgOpacity / 100,
      }"
    />

    <!-- 状态 -->
    <div class="bg-status" :class="{ ok: !!appearance.bgImage.dataUrl }">
      <span class="bg-status-icon">
        <el-icon :size="13">
          <CircleCheck v-if="appearance.bgImage.dataUrl" />
          <Picture v-else />
        </el-icon>
      </span>
      <span>
        {{
          appearance.bgImage.dataUrl
            ? `已设置背景 (透明度: ${Math.round(bgOpacity)}%)`
            : "未设置背景图片"
        }}
      </span>
    </div>

    <!-- 透明度滑杆 — 实时应用 -->
    <div class="opacity-block">
      <div class="opacity-head">
        <label class="opacity-label">透明度</label>
        <span class="opacity-value">{{ Math.round(bgOpacity) }}%</span>
      </div>
      <MySlider
        :model-value="bgOpacity"
        :min="BG_OPACITY_MIN"
        :max="BG_OPACITY_MAX"
        :step="1"
        @update:model-value="onOpacityChange"
      />
    </div>

    <!-- 清除按钮 -->
    <MyButton
      variant="ghost"
      class="block-btn clear-btn"
      :disabled="!appearance.bgImage.dataUrl"
      @click="clearBgImage"
    >
      清除背景图片
    </MyButton>
  </MySection>

  <!-- 终端字体 -->
  <MySection
    title="终端字体"
    description="自定义终端字体；留空使用内置默认（已含 Nerd Font 回退）。填入本机已安装的字体名即可显示图标 / Powerline 字形。"
  >
    <div class="field">
      <label class="field-label">字体 (Font Family)</label>
      <FontField
        :model-value="primaryFont"
        placeholder="例如：CaskaydiaCove Nerd Font（留空用默认）"
        @update:model-value="setPrimaryFont"
      />
    </div>
    <div class="font-hint">
      常见 Nerd Font：CaskaydiaCove Nerd Font、MesloLGM NF、JetBrainsMono Nerd Font、FiraCode Nerd
      Font、Hack Nerd Font
    </div>
  </MySection>

  <!-- 终端渲染 — 渲染后端 + GPU 开关。针对"光标不显示 / 选中区域看不到
       高亮"类反馈：根因在 xterm.js 渲染层，让 GPU 异常的用户有路可退。 -->
  <MySection
    title="终端渲染"
    description="若出现光标不显示、选中区域看不到高亮等问题，可在此切换渲染后端或关闭 GPU 加速。仅影响新打开的终端标签页。"
  >
    <div class="field">
      <label class="field-label">渲染后端</label>
      <div class="backend-row">
        <button
          v-for="opt in RENDERER_OPTIONS"
          :key="opt.id"
          class="backend-chip"
          :class="{ active: rendererBackend === opt.id }"
          :title="opt.desc"
          type="button"
          @click="setRendererBackend(opt.id)"
        >
          {{ opt.label }}
        </button>
      </div>
    </div>

    <div class="gpu-row">
      <div class="gpu-text">
        <div class="gpu-title">禁用 GPU 硬件加速</div>
        <div class="gpu-desc">
          切换 WebGL/canvas 合成异常（光标/选区消失）时的最终手段。{{
            gpuPendingRestart ? "更改需重启应用后生效。" : "当前设置已生效。"
          }}
        </div>
      </div>
      <MyToggle
        :model-value="gpuDisabled"
        @update:model-value="onGpuToggle"
      />
    </div>
  </MySection>
</template>

<style scoped>
.preset-grid {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: 10px;
}

.preset-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 10px 6px;
  background: var(--bg-surface);
  border: 2px solid transparent;
  border-radius: var(--radius-lg);
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-out-expo);
}

.preset-card:hover {
  border-color: var(--border-emphasis);
  background: var(--bg-surface-hover);
}

.preset-card.active {
  background: var(--accent-primary-muted);
  border-color: var(--accent-primary);
}

.preset-card.active:hover {
  border-color: var(--accent-primary);
  background: var(--accent-primary-muted);
}

.preset-dot {
  width: 32px;
  height: 32px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border-default);
  position: relative;
  overflow: hidden;
}

.preset-dot-bar {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  height: 8px;
}

.preset-name {
  font-size: 10px;
  font-weight: 400;
  color: var(--text-tertiary);
  text-align: center;
  line-height: 1.3;
}

.preset-card.active .preset-name {
  font-weight: 600;
  color: var(--accent-primary);
}

.custom-entry {
  margin-top: 10px;
  width: 100%;
  padding: 10px 14px;
  background: var(--bg-surface);
  color: var(--text-tertiary);
  border: 1px dashed var(--border-default);
  border-radius: var(--radius-md);
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-in-out);
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
}

.custom-entry:hover {
  background: var(--accent-primary-muted);
  border-color: var(--border-accent);
  color: var(--accent-primary);
}

.custom-entry.has-custom {
  background: var(--accent-primary-muted);
  color: var(--accent-primary);
  border: 1px solid var(--border-accent);
}

.custom-entry-icon {
  font-size: 14px;
}

.block-btn {
  width: 100%;
}

.bg-preview {
  margin-top: 10px;
  width: 100%;
  height: 120px;
  border-radius: var(--radius-lg);
  background-size: cover;
  background-position: center;
  border: 1px solid var(--border-default);
}

.bg-status {
  margin-top: 10px;
  padding: 8px 14px;
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  font-size: 11px;
  color: var(--text-muted);
  display: flex;
  align-items: center;
  gap: 6px;
}

.bg-status.ok {
  color: var(--success);
}

.opacity-block {
  margin-top: 12px;
}

.opacity-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 4px;
}

.opacity-label {
  font-size: 12px;
  color: var(--text-secondary);
  font-weight: 500;
}

.opacity-value {
  font-size: 11px;
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}

.clear-btn {
  margin-top: 12px;
  color: var(--error);
}

.clear-btn:disabled {
  color: var(--text-muted);
}

.field {
  margin-bottom: 12px;
}

.field-label {
  display: block;
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 6px;
  font-weight: 500;
}

.font-hint {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 8px;
  line-height: 1.6;
}

.backend-row {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.backend-chip {
  padding: 8px 14px;
  background: var(--bg-input);
  color: var(--text-secondary);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  font-size: 12px;
  font-weight: 400;
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-in-out);
}

.backend-chip.active {
  background: var(--accent-primary-muted);
  color: var(--accent-primary);
  border-color: var(--accent-primary);
  font-weight: 600;
}

.gpu-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 14px;
  padding: 12px 14px;
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
}

.gpu-text {
  flex: 1;
  min-width: 0;
}

.gpu-title {
  font-size: 13px;
  color: var(--text-primary);
}

.gpu-desc {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 3px;
  line-height: 1.5;
}
</style>
