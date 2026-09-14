<!-- 连接类型图标：渲染自打包的 iconfont 字体（iconfont.cn）。
     Glyph 映射（见 src/assets/iconfont/iconfont.css）：
       ssh   → icon-fuwuqi  (服务器)
       sftp  → icon-SFTP
       ftp   → icon-ftp
       local → icon-diannao (电脑)
     颜色默认取 CONN_COLOR 的类型语义色；size 控制字号（px）。
     取代了旧的 emoji 方案（🖥️ / 📁 / 📤 / 💻，跨平台渲染不一致）。 -->
<script lang="ts">
import type { ConnType } from "@/api";

/** ConnType → iconfont 类名映射；未知/缺省回退 ssh。 */
export const CONN_ICON_CLASS: Record<ConnType, string> = {
  ssh: "icon-fuwuqi",
  sftp: "icon-SFTP",
  ftp: "icon-ftp",
  local: "icon-diannao",
};

/**
 * 每种连接类型的语义着色，侧栏 / 标签栏 / 连接对话框共用同一色觉线索。
 * 调用方需要覆盖颜色时用 CSS（如 `.tab-icon { color: inherit !important }`，
 * 内联色优先级高于普通样式，故覆盖处需 !important）。
 */
export const CONN_COLOR: Record<ConnType, string> = {
  ssh: "var(--accent-primary)",
  sftp: "var(--accent-secondary)",
  ftp: "var(--warning)",
  local: "var(--text-secondary)",
};
</script>

<script setup lang="ts">
import { computed } from "vue";

defineOptions({ name: "ConnIcon" });

const props = withDefaults(
  defineProps<{
    /** 连接类型；缺省/未知回退为 ssh 图标 */
    type?: ConnType;
    /** 图标字号（px），默认 16 */
    size?: number;
    /** 可选的 title 提示文本（如连接名）；缺省时标记 aria-hidden */
    name?: string;
  }>(),
  { size: 16 },
);

const cls = computed(
  () => (props.type ? CONN_ICON_CLASS[props.type] || CONN_ICON_CLASS.ssh : CONN_ICON_CLASS.ssh),
);

const color = computed(
  () => (props.type ? CONN_COLOR[props.type] || CONN_COLOR.ssh : CONN_COLOR.ssh),
);
</script>

<template>
  <i
    class="iconfont"
    :class="cls"
    :title="name"
    :aria-hidden="name ? undefined : true"
    :style="{ fontSize: `${size}px`, color }"
  />
</template>
