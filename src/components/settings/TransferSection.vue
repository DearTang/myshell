<!-- 文件传输 — SFTP 下载并发（旧 transfer 分区）。
     并发数为 localStorage 读取的传输开始值；Rust 侧无论存什么都会钳到 1..=16。 -->
<script setup lang="ts">
import { ref } from "vue";
import { MySection } from "myui";
import {
  getSftpDownloadConcurrency,
  setSftpDownloadConcurrency,
  DEFAULT_SFTP_CONCURRENCY,
} from "@/utils/transfer-settings";

defineOptions({ name: "TransferSection" });

const CONCURRENCY_CHOICES = [1, 2, 3, 4, 6, 8, 12, 16];

const sftpConcurrency = ref(getSftpDownloadConcurrency());

function setConcurrency(n: number): void {
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
      <div class="chip-row">
        <button
          v-for="n in CONCURRENCY_CHOICES"
          :key="n"
          class="chip"
          :class="{ active: sftpConcurrency === n }"
          :title="n === DEFAULT_SFTP_CONCURRENCY ? '默认' : undefined"
          type="button"
          @click="setConcurrency(n)"
        >
          {{ n }}{{ n === DEFAULT_SFTP_CONCURRENCY ? "（默认）" : "" }}
        </button>
      </div>
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

.chip-row {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.chip {
  min-width: 44px;
  padding: 8px 10px;
  background: var(--bg-input);
  color: var(--text-secondary);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  font-size: 12px;
  font-weight: 400;
  cursor: pointer;
  transition: all var(--duration-fast) var(--ease-in-out);
}

.chip.active {
  background: var(--accent-primary-muted);
  color: var(--accent-primary);
  border-color: var(--accent-primary);
  font-weight: 600;
}

.chip-hint {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-top: 10px;
  line-height: 1.5;
}
</style>
