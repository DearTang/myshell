<!-- 匿名使用统计授权对话框（每个新版本首次启动时弹一次，用户此前未同意过）。
     从 src-legacy/components/StatsConsentDialog.tsx 原样移植（文案逐字保留）。
     说明收集什么（仅版本号 + 操作系统 + 随机设备 ID）、绝不收集什么（服务器地址、
     密码、连接内容），以及授权模式（同意一次 = 后续版本自动；暂不 = 下个版本再问）。
     ESC = 暂不（不追踪的那一侧）：由 MyDialog 的 close-on-press-escape 承担（触发
     cancel → decline）；点遮罩 = 暂不（close-on-click-modal → cancel → decline）。
     同意/暂不的实际落库在壳层（App.vue 的 onStatsAgree/onStatsDecline → lib/usageStats），
     本组件只 emit（confirm→agree / cancel→decline）。 -->
<script setup lang="ts">
import { TrendCharts } from "@element-plus/icons-vue";
import { MyDialog } from "myui";

defineOptions({ name: "StatsConsentDialog" });

defineProps<{ version: string }>();

const emit = defineEmits<{ agree: []; decline: [] }>();
</script>

<template>
  <!-- 壳层 v-if 挂载，恒为打开态；右上角关闭按钮旧版没有 → showClose 维持禁用。 -->
  <MyDialog
    :model-value="true"
    :width="460"
    align-center
    confirm-text="允许匿名统计"
    cancel-text="暂不"
    close-on-click-modal
    close-on-press-escape
    @confirm="emit('agree')"
    @cancel="emit('decline')"
  >
    <template #header>
      <div class="dlg-head">
        <span class="icon">
          <el-icon :size="28"><TrendCharts /></el-icon>
        </span>
        <span class="title">帮助 MyShell 变得更好</span>
      </div>
    </template>

    <div class="desc">
      检测到你升级到了 v{{ version }}。是否允许发送一次
      <strong class="strong">完全匿名</strong>
      的统计数据，帮助我们了解有多少用户在使用？
    </div>

    <div class="collect-box">
      <div class="collect-title">收集的内容（仅此而已）：</div>
      ✓ 应用版本号（v{{ version }}）<br />
      ✓ 操作系统（如 Windows）<br />
      ✓ 一个随机设备 ID（用于去重计数，不绑定任何个人信息）
      <div class="never-title">绝不收集：</div>
      ✗ 服务器地址 / 用户名 / 密码 / 连接内容
    </div>

    <div class="repeat-hint">
      每次升级到新版本都会询问一次。同意将发送本次匿名统计；选择暂不则本次不发送。
    </div>
  </MyDialog>
</template>

<style scoped>
.dlg-head {
  display: flex;
  align-items: center;
  gap: 12px;
}

.icon {
  font-size: 28px;
  line-height: 1;
  display: inline-flex;
}

.title {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.desc {
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.7;
  margin-bottom: 16px;
}

.strong {
  color: var(--text-primary);
}

.collect-box {
  background: var(--bg-input);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  padding: 12px 14px;
  margin-bottom: 20px;
  font-size: 12px;
  color: var(--text-muted);
  line-height: 1.6;
}

.collect-title {
  font-weight: 600;
  color: var(--text-secondary);
  margin-bottom: 6px;
}

.never-title {
  font-weight: 600;
  color: var(--text-secondary);
  margin-top: 10px;
  margin-bottom: 4px;
}

.repeat-hint {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-bottom: 4px;
  line-height: 1.5;
}
</style>
