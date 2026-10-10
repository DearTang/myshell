<!-- 文件传输 — SFTP 下载并发（旧 transfer 分区）。
     并发数为 localStorage 读取的传输开始值；Rust 侧无论存什么都会钳到 1..=16。 -->
<script setup lang="ts">
import { ref } from "vue";
import { MySection, MySegmented } from "myui";
import {
  getSftpDownloadConcurrency,
  setSftpDownloadConcurrency,
  DEFAULT_SFTP_CONCURRENCY,
} from "@/utils/transfer-settings";

defineOptions({ name: "TransferSection" });

const CONCURRENCY_CHOICES = [1, 2, 3, 4, 6, 8, 12, 16];

/* 并发数是一组单选项，迁移前是 8 枚各自独立的裸 <button class="chip">，
   选中态靠自绘 .chip.active（淡强调底 + 强调色字）。这类"单选按钮组"
   在 myui 里正是 MySegmented 的场景——选中/未选、键盘方向键导航、
   radio 语义与无障碍全部由库承担，这里不再维护第二套选中态配色。 */
const CONCURRENCY_OPTIONS = CONCURRENCY_CHOICES.map((n) => ({
  label: n === DEFAULT_SFTP_CONCURRENCY ? `${n}（默认）` : String(n),
  value: n,
}));

const sftpConcurrency = ref(getSftpDownloadConcurrency());

function setConcurrency(v: string | number | boolean): void {
  const n = typeof v === "number" ? v : Number(v);
  if (!Number.isFinite(n) || !CONCURRENCY_CHOICES.includes(n)) return;
  sftpConcurrency.value = n;
  setSftpDownloadConcurrency(n);
}
</script>

<template>
  <MySection
    title="SFTP 下载"
    description="勾选的文件夹会递归下载整个子树（含空目录）；多文件同时传输时以下发线程数并行拉取，对大量小文件提速明显。"
  >
    <div class="field">
      <label class="field-label">并发下载线程数</label>
      <MySegmented
        :model-value="sftpConcurrency"
        :options="CONCURRENCY_OPTIONS"
        size="small"
        @update:model-value="setConcurrency"
      />
    </div>
    <div class="chip-hint">
      线程数决定同时下载的文件数（单一文件不受影响）。所有线程复用同一条 SSH 连接，过高线程数对高延迟链路收益有限、部分服务器可能限制并发句柄。修改对下一次下载立即生效。ZMODEM（终端
      rz/sz）为协议单流串行传输，不适用此设置；终端里用 `sz -r 目录` 可递归下载文件夹。
    </div>
  </MySection>
</template>

<style scoped>
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

/* 并发线程数改用 MySegmented 后，.chip-row / .chip 及自绘的 .chip.active
   选中态全部删除——单选按钮组的选中表达由 myui 承接。 */

.chip-hint {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-top: 10px;
  line-height: 1.5;
}
</style>
