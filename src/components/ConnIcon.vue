<!-- 连接类型图标：渲染 myui 的线性图标（v1.1.1 起提供四枚连接语义图标）。
     名称映射（myui line-icons）：
       ssh   → Server         两节机架服务器
       sftp  → Sftp           协议名字标
       ftp   → Ftp            协议名字标
       local → LocalTerminal  笔记本 + `>` 提示符
     颜色默认取 CONN_COLOR 的类型语义色；size 控制字号（px）。
     历史：emoji（🖥️/📁/📤/💻，跨平台渲染不一致）→ iconfont 字体
     （icon-fuwuqi 等，需维护 ttf 资产）→ 现版本直接用 myui 图标，
     ttf 资产已随之删除。 -->
<script lang="ts">
import type { ConnType } from "@/api";

/** ConnType → myui MyIcon 名称映射；未知/缺省回退 ssh。 */
export const CONN_ICON_NAME: Record<ConnType, string> = {
  ssh: "Server",
  sftp: "Sftp",
  ftp: "Ftp",
  local: "LocalTerminal",
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
import { MyIcon } from "myui";

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

const iconName = computed(
  () => (props.type ? CONN_ICON_NAME[props.type] || CONN_ICON_NAME.ssh : CONN_ICON_NAME.ssh),
);

const color = computed(
  () => (props.type ? CONN_COLOR[props.type] || CONN_COLOR.ssh : CONN_COLOR.ssh),
);
</script>

<template>
  <MyIcon
    :name="iconName"
    :size="size"
    :title="name"
    :aria-hidden="name ? undefined : true"
    :style="{ color }"
  />
</template>
