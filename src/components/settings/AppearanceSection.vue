<!-- 外观 — 界面（深浅/主题色/字号/实心度/动效）/ 终端配色 / 背景（四模式）/ 终端字体 / 终端渲染 -->
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { MyButton, MyIcon, MyIconButton, MySection, MySegmented, MySlider, MyToggle } from "myui";
import { importWallpaper } from "@/api";
import {
  appearance,
  setPaletteId,
  setCustomPalette,
  clearCustomPalette,
  setBgImage,
  setSolidity,
  setAccentHue,
  setScreenshotStyleId,
  wallpaperUrl,
} from "@/store/appearance";
import {
  ui,
  setThemeChoice,
  setUiFontPx,
  setMotionEnabled,
  UI_FONT_PX_MIN,
  UI_FONT_PX_MAX,
  UI_FONT_PX_DEFAULT,
} from "@/store/ui";
import { primaryFont, setPrimaryFont } from "@/composables/useTerminalFont";
import {
  rendererBackend,
  setRendererBackend,
  gpuDisabled,
  setGpuDisabled,
  readGpuDisabled,
  type RendererBackend,
} from "@/composables/useRendererPref";
import { PRESETS, SCREENSHOT_STYLES, type BackgroundMode, type ColorPalette } from "@/themes";
import FontField from "@/components/FontField.vue";
import CustomPaletteDialog from "./CustomPaletteDialog.vue";
import {
  APPEARANCE_THEMES_MAX,
  applyAppearanceValues,
  appearanceThemeNameTaken,
  appearanceThemesWithinBudget,
  appearanceValuesEqual,
  newAppearanceThemeId,
  readAppearanceValues,
  validAppearanceThemeName,
  type AppearanceTheme,
} from "@/utils/appearanceThemes";
import { getAppearanceThemes, setAppearanceThemes } from "@/store/appearance";
import { confirmDialog, promptDialog, toast } from "myui";

defineOptions({ name: "AppearanceSection" });

// ── 界面通用设置（字号 / 实心度 / 动效 / 深浅）──

/** 界面实心度滑杆：40–100，压缩映射到 solidity 0–1（**ompDesktop 同口径**）。
 *
 *  40% = 最透档（solidity 0，壳层 alpha 取 app.css 的 --glass-alpha 基线），
 *  100% = 完全不透明。omp 把下限夹在 40 是刻意的：最低位仍保留一层可见的
 *  玻璃底，不会出现「滑杆拉到头、壳层变成一层几乎不存在的薄膜」。
 *
 *  曾改成一比一的 0–100%（2026-11-10，理由「读数即真实不透明度」），但用户
 *  比对 ompDesktop 后要求外观配置方式与它一致，故改回压缩区间。
 *  副作用是 40% 的读数不等于真实 alpha（真实值由各档基线公式算出），这与
 *  omp 完全一致 —— 用户已按 omp 的口径接受这个读数语义。 */
const OPACITY_MIN_PERCENT = 40;
const OPACITY_MAX_PERCENT = 100;
/** 弹窗实心度滑杆的下限：浮层压在文字上，不允许比这更透。 */
const POPUP_MIN_PERCENT = 40;

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

function solidityToPercent(solidity: number): number {
  return Math.round(
    OPACITY_MIN_PERCENT + clamp01(solidity) * (OPACITY_MAX_PERCENT - OPACITY_MIN_PERCENT),
  );
}

function percentToSolidity(percent: number): number {
  const bounded = Math.min(OPACITY_MAX_PERCENT, Math.max(OPACITY_MIN_PERCENT, percent));
  return (bounded - OPACITY_MIN_PERCENT) / (OPACITY_MAX_PERCENT - OPACITY_MIN_PERCENT);
}

/** 弹窗档：40–100 压缩映射（40 → solidity 0，100 → solidity 1）。 */
function popupSolidityToPercent(solidity: number): number {
  return Math.round(
    POPUP_MIN_PERCENT + clamp01(solidity) * (OPACITY_MAX_PERCENT - POPUP_MIN_PERCENT),
  );
}

function percentToPopupSolidity(percent: number): number {
  const bounded = Math.min(OPACITY_MAX_PERCENT, Math.max(POPUP_MIN_PERCENT, percent));
  return (bounded - POPUP_MIN_PERCENT) / (OPACITY_MAX_PERCENT - POPUP_MIN_PERCENT);
}

const uiOpacityPercent = computed({
  get: () => solidityToPercent(appearance.uiSolidity),
  set: (v: number) => setSolidity({ ui: percentToSolidity(v) }),
});

const popupOpacityPercent = computed({
  get: () => popupSolidityToPercent(appearance.popupSolidity),
  set: (v: number) => setSolidity({ popup: percentToPopupSolidity(v) }),
});

function onDarkToggle(v: string | number | boolean): void {
  setThemeChoice(v ? "dark" : "light");
}

function onUiFontChange(v: number | number[]): void {
  setUiFontPx(Array.isArray(v) ? v[0] : v);
}

function onUiSolidityChange(v: number | number[]): void {
  uiOpacityPercent.value = Array.isArray(v) ? v[0] : v;
}

function onPopupSolidityChange(v: number | number[]): void {
  popupOpacityPercent.value = Array.isArray(v) ? v[0] : v;
}

function onMotionToggle(v: string | number | boolean): void {
  setMotionEnabled(Boolean(v));
}

// ── 配色方案（预设只管终端 16 色；界面颜色由主题色滑杆 + 深浅决定）──
const showCustomDialog = ref(false);
const customBg = ref(appearance.customPalette?.dark.terminal?.background || "#1e1e2e");

const HEX_3_RE = /^#[0-9a-fA-F]{3}$/;
const HEX_6_RE = /^#[0-9a-fA-F]{6}/;
const HEX_VALID_RE = /^#[0-9a-fA-F]{3}([0-9a-fA-F]{3})?$/;

const bgValid = computed(() => HEX_VALID_RE.test(customBg.value.trim()));

function presetVariant(preset: ColorPalette): ColorPalette["dark"] {
  return ui.isDark ? preset.dark : preset.light;
}

function choosePreset(id: string): void {
  setPaletteId(id);
  clearCustomPalette();
}

function saveCustomPalette(): void {
  // 规范化为 7 位 #RRGGBB："#RGB" 缩写或自由文本若不处理，会拼出非法
  // CSS 颜色，被 xterm 静默丢弃。
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
  const bg = normHex(customBg.value);
  const id = `custom-${Date.now()}`;
  // 自定义预设只覆盖终端背景色，其余 16 色沿用 Carbon 基线 ——
  // UI 颜色已不由预设携带（主题色滑杆管强调色族）。
  const palette: ColorPalette = {
    id,
    name: "自定义主题",
    dark: { terminal: { ...PRESETS[0].dark.terminal, background: bg } },
    light: { terminal: { ...PRESETS[0].light.terminal, background: bg } },
  };
  setCustomPalette(palette);
  setPaletteId(id);
  showCustomDialog.value = false;
}

// ── 主题色（--accent-hue，0-360 色相）──
/** 色相轨道渐变：与 oklch 色相环一致（0° 红 → 120° 绿 → 240° 蓝 → 360° 红），
 *  滑杆下方常驻展示，用户不必猜「210 是什么颜色」。 */
const HUE_TRACK_GRADIENT =
  "linear-gradient(to right, oklch(0.72 0.15 0), oklch(0.72 0.15 60), oklch(0.72 0.15 120), oklch(0.72 0.15 180), oklch(0.72 0.15 240), oklch(0.72 0.15 300), oklch(0.72 0.15 360))";

function onAccentHueChange(v: number | number[]): void {
  setAccentHue(Array.isArray(v) ? v[0] : v);
}

// ── 背景（四模式：glow / grid / image / none）──
const BG_MODE_OPTIONS = [
  { label: "光晕", value: "glow" },
  { label: "网格", value: "grid" },
  { label: "壁纸", value: "image" },
  { label: "无", value: "none" },
];

const BG_MODE_HINTS: Record<string, string> = {
  glow: "主题色环境光晕（默认）：顶部两团柔和渐变，跟随「主题色」滑杆变色。",
  grid: "细网格纹理：1px 线按 32px 间距平铺，工程图纸气质。",
  image: "本地壁纸：导入时收进应用自管目录（不放 localStorage），设壁纸后壳层自动进入透出模式。",
  none: "纯底色：不加任何背景图形。",
};

/** 强度（0.05-0.8）。滑杆显示为百分比（5-80）。 */
const bgOpacity = computed(() => appearance.bgImage.opacity);
const bgImageBusy = ref(false);
/** 导入按钮文案：三态（导入中/已有壁纸换图/无壁纸选图），替代模板里的链式三元。 */
const bgImportLabel = computed(() => {
  if (bgImageBusy.value) return "导入中...";
  return appearance.bgImage.image ? "更换壁纸" : "选择壁纸";
});
function onBgModeChange(v: string | number | boolean): void {
  if (typeof v !== "string") return;
  setBgImage({ ...appearance.bgImage, mode: v as BackgroundMode });
}

function onOpacityChange(v: number | number[]): void {
  const percent = Array.isArray(v) ? v[0] : v;
  setBgImage({ ...appearance.bgImage, opacity: Math.min(0.8, Math.max(0.05, percent / 100)) });
}

/** 选图 → Rust 导入自管目录 → wallpaperUrl 生成带版本戳的 asset URL。
 *  导入而非直读的原因（asset scope / 文件被移）见 wallpaper.rs；
 *  版本戳的原因（同一 URL 不重取图）见 wallpaperUrl。 */
async function pickBgImage(): Promise<void> {
  bgImageBusy.value = true;
  try {
    const selected = await open({
      multiple: false,
      filters: [
        { name: "图片", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"] },
      ],
    });
    if (selected && !Array.isArray(selected)) {
      const stored = await importWallpaper(selected);
      setBgImage({
        mode: "image",
        image: wallpaperUrl(stored),
        opacity: appearance.bgImage.opacity,
      });
    }
  } catch (e) {
    console.error("壁纸导入失败:", e);
  } finally {
    bgImageBusy.value = false;
  }
}

function clearBgImage(): void {
  setBgImage({ mode: "image", image: "", opacity: appearance.bgImage.opacity });
}

// ── 外观主题快照（当前全套外观固化成命名主题，一键切换）──
const themes = ref<AppearanceTheme[]>(getAppearanceThemes());
const currentValues = computed(() =>
  readAppearanceValues({ appearance, ui: { themeChoice: ui.themeChoice, uiFontPx: ui.uiFontPx, motionEnabled: ui.motionEnabled } }),
);
/** 当前外观与哪套主题完全一致；不一致为 null（调乱后无高亮）。 */
const activeThemeId = computed(
  () => themes.value.find((t) => appearanceValuesEqual(t.values, currentValues.value))?.id ?? null,
);
const atThemeLimit = computed(() => themes.value.length >= APPEARANCE_THEMES_MAX);

function persistThemes(next: AppearanceTheme[]): void {
  themes.value = next;
  setAppearanceThemes(next);
}

function applyTheme(theme: AppearanceTheme): void {
  applyAppearanceValues(
    {
      setThemeChoice,
      setPaletteId,
      setAccentHue,
      setSolidity,
      setUiFontPx,
      setMotionEnabled,
      setBgImage,
    },
    theme.values,
  );
  toast(`已切换到「${theme.label}」`, { type: "success" });
}

async function saveTheme(): Promise<void> {
  if (atThemeLimit.value) {
    toast(`最多保存 ${APPEARANCE_THEMES_MAX} 套主题`, { type: "warning" });
    return;
  }
  const name = await promptDialog({
    title: "保存当前外观为主题",
    message: "把当前全部外观设置（深浅/主题色/实心度/字号/背景/终端配色）存为命名主题",
    inputPlaceholder: "例如：白天工作台",
    inputValidator: (value: string) => {
      if (!validAppearanceThemeName(value)) return "名称不能为空（40 字以内）";
      if (appearanceThemeNameTaken(value, themes.value)) return "已有同名主题";
      return true;
    },
  });
  if (name === null) return;
  const label = name.trim();
  if (!validAppearanceThemeName(label)) {
    toast("名称不能为空（40 字以内）", { type: "warning" });
    return;
  }
  const theme: AppearanceTheme = { id: newAppearanceThemeId(), label, values: currentValues.value };
  // 壁纸若是历史遗留的 data URL，单条就可能上百 KB —— 落库前先挡一道
  if (!appearanceThemesWithinBudget(themes.value, theme)) {
    toast("主题存储超预算（壁纸过大），请先在「背景」里重选壁纸", { type: "warning" });
    return;
  }
  persistThemes([...themes.value, theme]);
  toast("主题已保存", { type: "success" });
}

/** 把当前外观写回某套主题（调乱之后的「保存修改」）。 */
function updateTheme(theme: AppearanceTheme): void {
  const next = themes.value.map((t) =>
    t.id === theme.id ? { ...t, values: currentValues.value } : t,
  );
  if (
    !appearanceThemesWithinBudget(
      themes.value.filter((t) => t.id !== theme.id),
      { ...theme, values: currentValues.value },
    )
  ) {
    toast("主题存储超预算（壁纸过大），请先在「背景」里重选壁纸", { type: "warning" });
    return;
  }
  persistThemes(next);
  toast(`已更新「${theme.label}」`, { type: "success" });
}

async function removeTheme(theme: AppearanceTheme): Promise<void> {
  const confirmed = await confirmDialog({
    title: "删除主题",
    message: `删除「${theme.label}」？界面当前外观不会变，只是少了一套可一键切换的快照。`,
    type: "warning",
  });
  if (!confirmed) return;
  persistThemes(themes.value.filter((t) => t.id !== theme.id));
  toast("主题已删除", { type: "success" });
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
  { id: "auto", label: "自动（推荐）", desc: "WebGL（终端背景透明，需 GPU 合成；不可用时自动回退 Canvas）" },
  { id: "canvas", label: "Canvas", desc: "最稳定，光标/选区直接画在画布上" },
  { id: "webgl", label: "WebGL", desc: "性能最佳，依赖 GPU 合成" },
  { id: "dom", label: "DOM", desc: "最轻量，仅聚焦时显示光标" },
];

/* 渲染后端是一组单选项 → MySegmented（迁移前是 4 枚裸 button + 自绘
   .backend-chip.active 选中态）。MySegmented 逐项只渲染 label、不透传
   逐项 title，所以原先挂在 title 上的说明改为在下方常驻显示当前项的
   desc——tooltip 只能鼠标悬停时看到，这行字现在始终可见。 */
const RENDERER_SEGMENTED_OPTIONS = RENDERER_OPTIONS.map((o) => ({
  label: o.label,
  value: o.id,
}));

const rendererDesc = computed(
  () => RENDERER_OPTIONS.find((o) => o.id === rendererBackend.value)?.desc ?? "",
);

function onRendererChange(v: string | number | boolean): void {
  if (typeof v !== "string") return;
  if (!RENDERER_OPTIONS.some((o) => o.id === v)) return;
  setRendererBackend(v as RendererBackend);
}
</script>

<template>
  <!-- 界面（通用外观，对齐 ompDesktop 的「外观」通用设置）——
       深浅模式 / 界面字号 / 界面实心度 / 弹窗实心度 / 动效。 -->
  <MySection
    title="界面"
    description="调整整体观感：字号、玻璃实心度与动效。改动即时生效并自动保存。"
  >
    <div class="setting-row">
      <div class="setting-text">
        <div class="setting-title">暗色模式</div>
        <div class="setting-desc">切换深浅主题。终端配色预设各自带深浅两套变体。</div>
      </div>
      <MyToggle :model-value="ui.isDark" @update:model-value="onDarkToggle" />
    </div>

    <div class="setting-row">
      <div class="setting-text">
        <div class="setting-title">主题色</div>
        <div class="setting-desc">
          界面强调色族（按钮 / 选中态 / 光晕 / 玻璃底色）的色相，深浅两档自动适配。
        </div>
      </div>
      <div class="setting-control hue-control">
        <div class="slider-slot">
          <MySlider
            :model-value="appearance.accentHue"
            :min="0"
            :max="360"
            :step="5"
            unit="°"
            aria-label="主题色色相"
            @update:model-value="onAccentHueChange"
          />
        </div>
        <button
          type="button"
          class="reset-link"
          title="恢复默认（青绿 180°）"
          @click="onAccentHueChange(180)"
        >
          复位
        </button>
      </div>
      <!-- 色相轨道渐变条：滑杆本体的轨道是主题色画不出来的（每个位置颜色
           都不同），常驻一条参考带，用户看到 210 ≈ 蓝就不用试错。 -->
      <div class="hue-track" :style="{ background: HUE_TRACK_GRADIENT }" aria-hidden="true" />
    </div>

    <div class="setting-row">
      <div class="setting-text">
        <div class="setting-title">界面字号</div>
        <div class="setting-desc">
          整体缩放正文 / 按钮 / 下拉 / 弹窗文字。快捷键 Ctrl+= 放大、Ctrl+- 缩小、Ctrl+0
          复位（焦点在终端时调的是终端字号）。
        </div>
      </div>
      <div class="setting-control">
        <div class="slider-slot">
          <MySlider
            :model-value="ui.uiFontPx"
            :min="UI_FONT_PX_MIN"
            :max="UI_FONT_PX_MAX"
            :step="1"
            unit="px"
            aria-label="界面字号"
            @update:model-value="onUiFontChange"
          />
        </div>
        <button type="button" class="reset-link" @click="setUiFontPx(UI_FONT_PX_DEFAULT)">
          复位
        </button>
      </div>
    </div>

    <div class="setting-row">
      <div class="setting-text">
        <div class="setting-title">界面实心度</div>
        <div class="setting-desc">
          顶栏 / 侧栏 / 底栏 / AI 栏的实心度。40% 为最透档（仍保留一层薄玻璃，
          保证终端文字压在壁纸上可读），拉满为完全不透明。
        </div>
      </div>
      <div class="setting-control">
        <div class="slider-slot">
          <MySlider
            :model-value="uiOpacityPercent"
            :min="40"
            :max="100"
            :step="5"
            unit="%"
            aria-label="界面实心度"
            @update:model-value="onUiSolidityChange"
          />
        </div>
      </div>
    </div>

    <div class="setting-row">
      <div class="setting-text">
        <div class="setting-title">弹窗实心度</div>
        <div class="setting-desc">
          下拉 / 对话框 / 通知等浮层的实心度，默认比常驻界面更实，避免与背后文字糊在一起。
        </div>
      </div>
      <div class="setting-control">
        <div class="slider-slot">
          <MySlider
            :model-value="popupOpacityPercent"
            :min="40"
            :max="100"
            :step="5"
            unit="%"
            aria-label="弹窗实心度"
            @update:model-value="onPopupSolidityChange"
          />
        </div>
      </div>
    </div>

    <div class="setting-row">
      <div class="setting-text">
        <div class="setting-title">界面动效</div>
        <div class="setting-desc">关闭后一次性停掉全部过渡与动画（低配设备更跟手）。</div>
      </div>
      <MyToggle :model-value="ui.motionEnabled" @update:model-value="onMotionToggle" />
    </div>
  </MySection>

  <!-- 外观主题（命名快照）：把上面的全部外观控件固化成一套，一键切换 -->
  <MySection
    title="外观主题"
    description="保存当前全套外观为命名主题（深浅/主题色/实心度/字号/背景/终端配色），跨场景一键切换。"
  >
    <div v-if="themes.length === 0" class="theme-empty">还没有保存过主题 —— 调好上面各项后点「保存当前为主题」。</div>
    <ul v-else class="theme-list">
      <li v-for="theme in themes" :key="theme.id" class="theme-row">
        <span class="theme-name" :title="theme.label">{{ theme.label }}</span>
        <span v-if="activeThemeId === theme.id" class="theme-active">当前</span>
        <span class="theme-flex" />
        <MyButton size="small" variant="primary" @click="applyTheme(theme)">切换</MyButton>
        <MyButton size="small" variant="secondary" title="用当前外观覆盖这套主题" @click="updateTheme(theme)">更新</MyButton>
        <MyIconButton
          title="删除主题"
          label="删除主题"
          size="14"
          name="Delete"
          @click="removeTheme(theme)"
        />
      </li>
    </ul>
    <div class="theme-actions">
      <span class="theme-count">{{ themes.length }} / {{ APPEARANCE_THEMES_MAX }}</span>
      <MyButton size="small" variant="primary" :disabled="atThemeLimit" @click="saveTheme">
        <MyIcon :size="13" name="Plus" />
        保存当前为主题
      </MyButton>
    </div>
  </MySection>

  <!-- 配色方案 -->
  <MySection title="终端配色" description="预设只管终端 16 色；界面颜色由上方「主题色」与深浅模式决定">
    <div class="preset-grid">
      <MyButton
        v-for="preset in PRESETS"
        :key="preset.id"
        class="preset-card"
        variant="secondary"
        :class="{ active: appearance.paletteId === preset.id && !appearance.customPalette }"
        :title="preset.name"
        @click="choosePreset(preset.id)"
      >
        <span class="preset-body">
          <span
            class="preset-dot"
            :style="{ background: presetVariant(preset).terminal.background }"
          >
            <!-- 色板底部的光标色条：预设不再携带 UI 变量后，用终端光标色
                 （各套预设的强调色）做视觉区分。 -->
            <span
              class="preset-dot-bar"
              :style="{ background: presetVariant(preset).terminal.cursor }"
            />
          </span>
          <span class="preset-name">{{ preset.name }}</span>
        </span>
      </MyButton>
    </div>

    <!-- 自定义主题入口 -->
    <MyButton
      class="custom-entry"
      variant="secondary"
      :class="{ 'has-custom': !!appearance.customPalette }"
      @click="showCustomDialog = true"
    >
      <span class="custom-entry-icon">
        <MyIcon :size="14" name="Brush" />
      </span>
      {{ appearance.customPalette ? "管理自定义主题" : "自定义终端配色..." }}
    </MyButton>

    <!-- 自定义主题对话框 -->
    <CustomPaletteDialog
      v-if="showCustomDialog"
      v-model:bg="customBg"
      :bg-valid="bgValid"
      @save="saveCustomPalette"
      @close="showCustomDialog = false"
    />
  </MySection>

  <!-- 截图样式：只影响「📷 截图」导出的 PNG，不影响真实终端显示。
       与「终端配色」分开是因为两者服务不同场景 —— 用户可能喜欢终端用
       Tokyo Night，但截图贴进工单/文档时更习惯 PuTTY 的经典配色。 -->
  <MySection
    title="截图样式"
    description="仅影响「📷 截图」导出的 PNG；真实终端显示仍跟随上方「终端配色」。"
  >
    <div class="preset-grid">
      <MyButton
        v-for="style in SCREENSHOT_STYLES"
        :key="style.id"
        class="preset-card"
        variant="secondary"
        :class="{ active: appearance.screenshotStyleId === style.id }"
        :title="style.name"
        @click="setScreenshotStyleId(style.id)"
      >
        <span class="preset-body">
          <span class="preset-dot" :style="{ background: style.terminal.background }">
            <span class="preset-dot-bar" :style="{ background: style.terminal.cursor }" />
          </span>
          <span class="preset-name">{{ style.name }}</span>
        </span>
      </MyButton>
    </div>
    <div class="font-hint">
      PuTTY：复刻 PuTTY 出厂默认配色（黑底 187,187,187 前景 + 经典 16 色 ANSI）与 Courier New 字体，适合贴进工单与文档。
      跟随终端：PNG 颜色与当前终端配色一致（透明底会解析为实际可见的底色），适合留档当前会话画面。
    </div>
  </MySection>


  <!-- 背景（四模式：glow 光晕 / grid 网格 / image 壁纸 / none） -->
  <MySection
    title="背景"
    description="整窗背景层的绘制方式；壁纸之外还有光晕与网格两种程序化背景，不占存储。"
  >
    <MySegmented
      :model-value="appearance.bgImage.mode"
      :options="BG_MODE_OPTIONS"
      size="small"
      @update:model-value="onBgModeChange"
    />

    <div class="bg-mode-hint">{{ BG_MODE_HINTS[appearance.bgImage.mode] }}</div>

    <!-- 壁纸导入（仅 image 模式）：经 Rust 命令收进自管目录，localStorage
         只存 asset 路径 —— 旧版把 base64 塞 localStorage，大图直接撑爆配额。 -->
    <template v-if="appearance.bgImage.mode === 'image'">
      <div class="bg-image-row">
        <MyButton variant="secondary" :disabled="bgImageBusy" @click="pickBgImage">
          <MyIcon :size="14" name="Picture" />
          {{ bgImportLabel }}
        </MyButton>
        <MyButton
          v-if="appearance.bgImage.image"
          variant="ghost"
          class="clear-btn"
          @click="clearBgImage"
        >
          移除壁纸
        </MyButton>
      </div>

      <div
        v-if="appearance.bgImage.image"
        class="bg-preview"
        :style="{ backgroundImage: `url(${appearance.bgImage.image})` }"
      />
    </template>

    <!-- 背景强度：image 模式 = 壁纸清晰度（越低越模糊）；glow/grid = 图形浓度 -->
    <div class="opacity-block">
      <div class="opacity-head">
        <label class="opacity-label">
          {{ appearance.bgImage.mode === "image" ? "壁纸清晰度" : "背景强度" }}
        </label>
        <span class="opacity-value">{{ Math.round(bgOpacity * 100) }}%</span>
      </div>
      <MySlider
        :model-value="Math.round(bgOpacity * 100)"
        :min="5"
        :max="80"
        :step="1"
        @update:model-value="onOpacityChange"
      />
      <div v-if="appearance.bgImage.mode === 'image'" class="opacity-note">
        拉满清晰度会同时减薄雾层并调低壳层玻璃基线（顶栏 / 侧栏 / 工作区），壁纸更明显；再调低「界面实心度」可进一步透出。
      </div>
    </div>
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
      <MySegmented
        :model-value="rendererBackend"
        :options="RENDERER_SEGMENTED_OPTIONS"
        size="small"
        @update:model-value="onRendererChange"
      />
      <div class="backend-hint">{{ rendererDesc }}</div>
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
/* 通用设置行：左侧标题+说明，右侧控件（对齐 ompDesktop 的 field-row 口径：
   标签区可换行、控件区固定宽度不挤压说明文字）。 */
.setting-row {
  display: flex;
  /* 允许换行：主题色行的渐变参考带要独占第二行（flex-basis:100%）；
     单行 flex 里 100% 宽子项会参与主轴分配，把 setting-text（min-width:0）
     挤成 0 宽 → 标题逐字竖排。普通行宽度足够时不会触发换行。 */
  flex-wrap: wrap;
  align-items: flex-start;
  justify-content: space-between;
  gap: 24px;
  padding: 10px 0;
  border-bottom: 1px solid var(--border-subtle);
}

.setting-row:last-child {
  border-bottom: none;
}

.setting-text {
  flex: 1;
  min-width: 0;
}

.setting-title {
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--text-primary);
  display: flex;
  align-items: center;
  gap: 8px;
}

.setting-desc {
  margin-top: 4px;
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  line-height: 1.6;
}

.setting-control {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
  padding-top: 2px;
}

/* 滑杆宽度：**必须挂在包裹层**上。MySlider 是 inheritAttrs:false +
   内层 ElSlider 透传 attrs，写在 MySlider 上的 class 会落到内层
   .el-slider 而拿不到外层的 .my-slider flex 容器宽度 —— 实测直接把滑杆
   压成 6px（.el-slider 只有 runway 宽度）。与项目里 .cmd-slot 包裹
   MyInput 的写法同一个原因。 */
.slider-slot {
  width: 200px;
  flex-shrink: 0;
}

/* 主题色滑杆行：控件行 + 下方全宽色相渐变参考带（渐变由脚本内联写入，
   这里只管布局）。setting-row 是 flex 行，渐变带作为行的第三个子元素
   占满整行宽度，与上方滑杆对齐。 */
.hue-control {
  flex-wrap: wrap;
}
.hue-track {
  /* 换行后独占一行；不用 width:100%（单行 flex 里语义不同） */
  flex-basis: 100%;
  height: 6px;
  margin-top: 6px;
  border-radius: 3px;
  opacity: 0.75;
}

.reset-link {
  padding: 0;
  border: none;
  background: transparent;
  color: var(--accent-primary);
  font-size: var(--fs-xs);
  cursor: pointer;
}

.reset-link:hover {
  text-decoration: underline;
}

.preset-grid {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: 10px;
}

/* 色板卡片走 MyButton：hover / focus / disabled 交给 secondary 变体。
   卡片自身的纵向排版保留自绘。
   「选中」态仍自绘——myui 没有"选中态卡片按钮"原语，而 primary 的实心
   强调底会盖掉卡片里那张真实的配色预览图（这正是选中态要展示的东西），
   所以这里刻意保留淡强调底 + 强调色描边。 */
.preset-card {
  /* EP 的 .el-button 写死了 height:32px（`.el-button{...height:32px...}`），
     卡片内容（32px 色板 + 6px 间距 + 13px 名称 ≈ 51px）撑破这个固定高度，
     于是在按钮盒子上方溢出 9.5px —— 正是用户看到的"色板顶出卡片上沿"。
     纵向排版改由插槽内的 .preset-body 负责后，卡片自身必须按内容长高。 */
  height: auto;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 10px 6px;
  border-width: 2px;
  /* 卡片比按钮大一档圆角（令牌桥把 EP base 接到 --radius-md=10px，
     卡片沿用 --radius-lg=14px） */
  border-radius: var(--radius-lg);
  transition: all var(--duration-fast) var(--ease-out-expo);
}

.preset-card.active,
.preset-card.active:hover {
  background: var(--accent-primary-muted);
  border-color: var(--accent-primary);
}

.preset-dot {
  /* flex:none：作为 content 层（纵向 flex）的子项时不被压扁，色板始终是正方块 */
  flex: none;
  width: 32px;
  height: 32px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border-default);
  position: relative;
  overflow: hidden;
}

/* MyButton 底层是 EP ElButton，default 插槽外面包了**一层我们自己看不见的
   span**（EP 2.14.5 里是 `.el-button__text`，且只有 shouldAddSpace 为真时才
   真的带这个类名；此前这里写的是 `.el-button__content`，在 2.14.5 中根本不存在，
   规则永远匹配不到任何元素 —— 这就是「改了没反应」的原因）。

   纵向排布因此不再下推到 EP 内部层（打穿第三方内部类名本就不该做，且换个 EP
   版本就静默失效），而是在插槽内容里自己包一个 .preset-body：插槽内容由本组件
   编译，自带 scope id，普通 scoped 选择器即可命中，不依赖任何 EP 内部结构。
   卡片本该是「色板在上、名称在下」的纵向排布；此前色板与名称并排成一行，
   且行内盒的基线对齐把 32px 的色板顶出卡片上沿（用户反馈"图标和颜色名称的
   方框不协调"）。 */
.preset-body {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.preset-dot-bar {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  height: 8px;
}

.preset-name {
  font-size: var(--fs-2xs);
  font-weight: 400;
  color: var(--text-tertiary);
  text-align: center;
  line-height: 1.3;
}

.preset-card.active .preset-name {
  font-weight: 600;
  color: var(--accent-primary);
}

/* 自定义主题入口走 MyButton。虚线描边是刻意的设计偏离——它在表达
   "这里还没有内容，点我新建"，实线按钮表达不了"尚未配置"这层意思，
   故 border 保留自绘；底色/文字/hover 交给变体。 */
.custom-entry {
  margin-top: 10px;
  width: 100%;
  padding: 10px 14px;
  border: 1px dashed var(--border-default);
  font-size: var(--fs-xs);
  font-weight: 500;
  gap: 6px;
}

.custom-entry.has-custom {
  background: var(--accent-primary-muted);
  color: var(--accent-primary);
  border: 1px solid var(--border-accent);
}

.custom-entry-icon {
  font-size: var(--fs-md);
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
  font-size: var(--fs-2xs);
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
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  font-weight: 500;
}

.opacity-value {
  font-size: var(--fs-2xs);
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}

.clear-btn {
  margin-top: 12px;
  color: var(--error);
}

.opacity-note {
  margin-top: 6px;
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  line-height: 1.6;
}


.bg-mode-hint {
  margin-top: 8px;
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
  line-height: 1.5;
}

.bg-image-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 10px;
}
/* 行内的「移除壁纸」与选图按钮同排，不吃块级 clear-btn 的上边距。 */
.bg-image-row .clear-btn {
  margin-top: 0;
}
.clear-btn:disabled {
  color: var(--text-muted);
}

.field {
  margin-bottom: 12px;
}

.field-label {
  display: block;
  font-size: var(--fs-xs);
  color: var(--text-secondary);
  margin-bottom: 6px;
  font-weight: 500;
}

.font-hint {
  font-size: var(--fs-2xs);
  color: var(--text-muted);
  margin-top: 8px;
  line-height: 1.6;
}

/* 渲染后端改用 MySegmented 后，.backend-row / .backend-chip 及自绘的
   .backend-chip.active 选中态全部删除。说明文字从原来的逐项 title
   改为下方常驻一行。 */
.backend-hint {
  font-size: var(--fs-2xs);
  color: var(--text-muted);
  margin-top: 8px;
  line-height: 1.6;
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
  font-size: var(--fs-sm);
  color: var(--text-primary);
}

.gpu-desc {
  font-size: var(--fs-2xs);
  color: var(--text-muted);
  margin-top: 3px;
  line-height: 1.5;
}

/* ── 外观主题面板 ── */
.theme-empty {
  padding: 16px;
  border: 1px dashed var(--border-subtle);
  border-radius: var(--radius-md);
  font-size: var(--fs-xs);
  color: var(--text-tertiary);
  text-align: center;
}

.theme-list {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.theme-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  background: var(--bg-surface);
  font-size: var(--fs-xs);
}

.theme-name {
  min-width: 0;
  max-width: 40%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 500;
  color: var(--text-primary);
}

.theme-active {
  flex-shrink: 0;
  padding: 0 6px;
  border-radius: var(--radius-full);
  font-size: var(--fs-2xs);
  color: var(--accent-primary);
  background: var(--accent-primary-muted);
}

.theme-flex {
  flex: 1;
}

.theme-actions {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 10px;
}

.theme-count {
  font-size: var(--fs-2xs);
  color: var(--text-tertiary);
}
</style>
