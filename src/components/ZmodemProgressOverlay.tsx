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

export function ZmodemProgressOverlay({ status, onCancel }: Props) {
  // Local tick so speed/percentage stays fresh even when bridge isn't emitting.
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!status.active) return;
    const id = setInterval(() => setTick((t) => t + 1), 500);
    return () => clearInterval(id);
  }, [status.active]);

  if (!status.active) return null;

  const percent =
    status.bytesTotal > 0
      ? Math.min(100, (status.bytesTransferred / status.bytesTotal) * 100)
      : 0;

  const dirIcon = status.direction === "upload" ? "↑" : "↓";
  const dirLabel = status.direction === "upload" ? "上传" : "下载";

  return (
    <div
      style={{
        position: "absolute",
        left: 0,
        right: 0,
        bottom: 0,
        height: 84,
        background: "rgba(30, 30, 46, 0.96)",
        borderTop: "1px solid #45475a",
        padding: "8px 16px",
        display: "flex",
        flexDirection: "column",
        gap: 6,
        fontFamily: "'Cascadia Code', Consolas, monospace",
        fontSize: 12,
        color: "#cdd6f4",
        zIndex: 10,
      }}
    >
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
            transition: "width 0.2s ease",
          }}
        />
      </div>

      <div style={{ display: "flex", justifyContent: "space-between", color: "#a6adc8" }}>
        <span>
          {formatBytes(status.bytesTransferred)} / {formatBytes(status.bytesTotal)}
          {status.bytesTotal > 0 && ` (${percent.toFixed(1)}%)`}
        </span>
        <span>{formatSpeed(status.speedBps)}</span>
      </div>
    </div>
  );
}
