<!-- 外观 — 配色方案 / 背景图片 / 终端字体 / 终端渲染（旧 appearance 分区） -->
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { MyButton, MyIcon, MySection, MySegmented, MySlider, MyToggle } from "myui";
import { readFileBase64 } from "@/api";
import { compressImageDataUrl } from "@/utils/image";
import {
  appearance,
  setPaletteId,
  setCustomPalette,
  clearCustomPalette,
  setBgImage,
  setSolidity,
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
import { PRESETS, type ColorPalette } from "@/themes";
import FontField from "@/components/FontField.vue";
import CustomPaletteDialog from "./CustomPaletteDialog.vue";

defineOptions({ name: "AppearanceSection" });

// 背景透明度滑杆范围。与下方 MySlider 的 min/max 保持同步。
const BG_OPACITY_MIN = 10;
const BG_OPACITY_MAX = 100;

// ── 界面通用设置（字号 / 实心度 / 动效 / 深浅）──

/** 实心度滑杆的百分比区间：下限 = 设计基线的玻璃质感，上限 = 完全不透明。
 *  与 ompDesktop 同一套口径。**不能**直接把 solidity×100 当显示值：
 *  solidity 0 会显示成 0%（被 el-slider 的 min=40 夹住），而真正的基线
 *  玻璃 alpha 是 0.55 —— 滑杆位置与实际透明度脱节（实测默认值 0.25 显示成
 *  40%，但玻璃 alpha 是 0.6625）。 */
const OPACITY_MIN_PERCENT = 40;
const OPACITY_MAX_PERCENT = 100;

function solidityToPercent(solidity: number): number {
  const ratio = Math.min(1, Math.max(0, solidity));
  return Math.round(OPACITY_MIN_PERCENT + ratio * (OPACITY_MAX_PERCENT - OPACITY_MIN_PERCENT));
}

function percentToSolidity(percent: number): number {
  const bounded = Math.min(OPACITY_MAX_PERCENT, Math.max(OPACITY_MIN_PERCENT, percent));
  return (bounded - OPACITY_MIN_PERCENT) / (OPACITY_MAX_PERCENT - OPACITY_MIN_PERCENT);
}

const uiOpacityPercent = computed({
  get: () => solidityToPercent(appearance.uiSolidity),
  set: (v: number) => setSolidity({ ui: percentToSolidity(v) }),
});

const popupOpacityPercent = computed({
  get: () => solidityToPercent(appearance.popupSolidity),
  set: (v: number) => setSolidity({ popup: percentToSolidity(v) }),
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
        <div class="setting-desc">切换深浅主题，33 套配色各自带深浅两套变体。</div>
      </div>
      <MyToggle :model-value="ui.isDark" @update:model-value="onDarkToggle" />
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
          越低越透（壁纸/底色透出顶栏、侧栏与 AI 栏），拉满为完全不透明。
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

  <!-- 配色方案 -->
  <MySection title="配色方案" description="选择预设配色方案，同步更新终端和界面颜色">
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
      {{ appearance.customPalette ? "管理自定义主题" : "自定义主题..." }}
    </MyButton>

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
  <MySection
    title="背景图片"
    description="作为整窗壁纸铺在最底层，顶栏 / 侧栏 / AI 栏按「界面实心度」透出它。图片会压缩后存进本地存储。"
  >
    <MyButton
      variant="secondary"
      class="block-btn"
      :disabled="bgImageBusy"
      @click="pickBgImage"
    >
      <MyIcon :size="14" name="Picture" />
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
        <MyIcon :size="13" :icon="appearance.bgImage.dataUrl ? 'CircleCheck' : 'Picture'" />
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
</style>
