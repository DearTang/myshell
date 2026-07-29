import { useEffect, useState } from "react";
import type { ZmodemStatus } from "../zmodem-bridge";

interface Props {
  status: ZmodemStatus;
  onCancel: () => void;
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function formatSpeed(bps: number): string {
  if (bps < 1) return "—";
  return `${formatBytes(bps)}/s`;
}

/** 格式化为 HH:MM:SS */
function formatTime(epochMs: number): string {
  if (!epochMs) return "—";
  const d = new Date(epochMs);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  return `${hh}:${mm}:${ss}`;
}

/** 格式化时长 → "X分Y秒" 或 "X小时Y分" */
function formatDuration(seconds: number): string {
  if (!isFinite(seconds) || seconds <= 0) return "—";
  if (seconds < 60) return `${Math.ceil(seconds)}秒`;
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60);
    const s = Math.round(seconds % 60);
    return `${m}分${s}秒`;
  }
  const h = Math.floor(seconds / 3600);
  const m = Math.round((seconds % 3600) / 60);
  return `${h}小时${m}分`;
}

export function ZmodemProgressOverlay({ status, onCancel }: Props) {
  // 1 秒 tick：驱动 ETA / 已用时间的实时刷新。
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!status.active) return;
    const id = setInterval(() => setTick((t) => t + 1), 1000);
    return () => clearInterval(id);
  }, [status.active]);

  if (!status.active) return null;

  const percent =
    status.bytesTotal > 0
      ? Math.min(100, (status.bytesTransferred / status.bytesTotal) * 100)
      : 0;

  const dirIcon = status.direction === "upload" ? "↑" : "↓";
  const dirLabel = status.direction === "upload" ? "上传" : "下载";

  // 计算 ETA 和已用时间
  const remaining = status.bytesTotal - status.bytesTransferred;
  const etaSeconds = status.speedBps > 0 ? remaining / status.speedBps : Infinity;
  const elapsedMs = status.startTime > 0 ? Date.now() - status.startTime : 0;

  return (
    <div
      style={{
        position: "absolute",
        left: 0,
        right: 0,
        bottom: 0,
        background: "rgba(30, 30, 46, 0.96)",
        borderTop: "1px solid #45475a",
        padding: "8px 16px 10px",
        display: "flex",
        flexDirection: "column",
        gap: 6,
        fontFamily: "'Cascadia Code', Consolas, monospace",
        fontSize: 12,
        color: "#cdd6f4",
        zIndex: 10,
      }}
    >
      {/* 第一行：方向 + 文件名 + 取消按钮 */}
      <div style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
        <div style={{ display: "flex", gap: 8, alignItems: "center", minWidth: 0 }}>
          <span style={{ color: "#89b4fa", fontSize: 16, lineHeight: 1 }}>{dirIcon}</span>
          <span style={{ color: "#a6e3a1", fontWeight: 600 }}>ZMODEM {dirLabel}</span>
          <span
            style={{
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              color: "#bac2de",
              minWidth: 0,
            }}
          >
            {status.currentFile || "—"}
          </span>
          {status.error && (
            <span style={{ color: "#f38ba8" }}>错误：{status.error}</span>
          )}
        </div>
        <button
          onClick={onCancel}
          style={{
            background: "transparent",
            border: "1px solid #f38ba8",
            color: "#f38ba8",
            padding: "2px 12px",
            borderRadius: 4,
            cursor: "pointer",
            fontSize: 12,
            fontFamily: "inherit",
            whiteSpace: "nowrap",
          }}
        >
          取消
        </button>
      </div>

      {/* 第二行：进度条 */}
      <div
        style={{
          height: 8,
          background: "#313244",
          borderRadius: 4,
          overflow: "hidden",
        }}
      >
        <div
          style={{
            width: `${percent}%`,
            height: "100%",
            background: "linear-gradient(90deg, #89b4fa, #b4befe)",
            transition: "width 0.3s ease",
          }}
        />
      </div>

      {/* 第三行：字节数 / 百分比 / 速度 */}
      <div style={{ display: "flex", justifyContent: "space-between", color: "#a6adc8" }}>
        <span>
          {formatBytes(status.bytesTransferred)} / {formatBytes(status.bytesTotal)}
          {status.bytesTotal > 0 && ` (${percent.toFixed(1)}%)`}
        </span>
        <span>{formatSpeed(status.speedBps)}</span>
      </div>

      {/* 第四行：开始时间 / 已用时间 / 预计剩余 */}
      <div style={{ display: "flex", justifyContent: "space-between", color: "#6c7086", fontSize: 11 }}>
        <span>开始 {formatTime(status.startTime)}</span>
        <span>已用 {elapsedMs > 0 ? formatDuration(elapsedMs / 1000) : "—"}</span>
        <span>剩余 {formatDuration(etaSeconds)}</span>
      </div>
    </div>
  );
}
