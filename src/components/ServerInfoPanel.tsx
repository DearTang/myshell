import { useEffect, useState } from "react";
import { sshGetServerInfo, type ServerInfo } from "../api";

interface Props {
  sessionId: string;
  active: boolean;
}

function formatBytes(b: number): string {
  if (b <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(b) / Math.log(1024)));
  const v = b / Math.pow(1024, i);
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

function UsageBar({ pct, stale }: { pct: number; stale: boolean }) {
  const color = pct > 85 ? "var(--error)" : pct > 60 ? "var(--warning)" : "var(--success)";
  const display = Math.max(0, Math.min(100, pct));
  return (
    <div
      style={{
        width: 40,
        height: 4,
        background: "var(--bg-input)",
        borderRadius: 2,
        overflow: "hidden",
        opacity: stale ? 0.4 : 1,
        flexShrink: 0,
      }}
    >
      <div
        style={{
          width: `${display}%`,
          height: "100%",
          background: color,
          borderRadius: 2,
          transition: "width 0.4s ease, background 0.3s",
        }}
      />
    </div>
  );
}

/** Single-row metric chip: icon + label + content + optional bar, all
 * inline. Designed for a compact bottom status bar. */
function MetricChip({
  label,
  icon,
  content,
  pct,
  stale,
  title,
}: {
  label: string;
  icon: string;
  content: string;
  pct?: number;
  stale: boolean;
  title?: string;
}) {
  return (
    <div
      title={title}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 6,
        padding: "0 12px",
        height: "100%",
        opacity: stale ? 0.55 : 1,
        minWidth: 0,
      }}
    >
      <span style={{ fontSize: 12, flexShrink: 0 }}>{icon}</span>
      <span
        style={{
          fontSize: 10,
          color: "var(--text-muted)",
          fontWeight: 600,
          letterSpacing: 0.3,
          textTransform: "uppercase",
          whiteSpace: "nowrap",
          flexShrink: 0,
        }}
      >
        {label}
      </span>
      <span
        style={{
          fontSize: 12,
          color: "var(--text-primary)",
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
          minWidth: 0,
        }}
      >
        {content}
      </span>
      {pct !== undefined && <UsageBar pct={pct} stale={stale} />}
    </div>
  );
}

export function ServerInfoPanel({ sessionId, active }: Props) {
  const [info, setInfo] = useState<ServerInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    const refresh = async () => {
      try {
        const v = await sshGetServerInfo(sessionId);
        if (!cancelled) {
          setInfo(v);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    };
    refresh();
    const tid = setInterval(refresh, 5000);
    return () => {
      cancelled = true;
      clearInterval(tid);
    };
  }, [sessionId, active]);

  const stale = info?.stale ?? false;
  const divider = { borderRight: "1px solid var(--border)" };

  return (
    <div
      style={{
        height: 36,
        minHeight: 36,
        background: "var(--bg-sidebar)",
        borderTop: "1px solid var(--border)",
        display: "flex",
        alignItems: "stretch",
        overflowX: "auto",
        overflowY: "hidden",
      }}
    >
      {!info && !error && (
        <div style={{ padding: "0 16px", fontSize: 11, color: "var(--text-muted)", alignSelf: "center" }}>
          加载中…
        </div>
      )}
      {error && (
        <div style={{ padding: "0 14px", fontSize: 11, color: "var(--error)", alignSelf: "center" }}>
          {error}
        </div>
      )}
      {info && (
        <>
          <div style={divider}>
            <MetricChip
              label="系统"
              icon="💿"
              content={info.osPretty || "unknown"}
              stale={stale}
              title={`内核 ${info.kernel || "-"}`}
            />
          </div>
          <div style={divider}>
            <MetricChip
              label="CPU"
              icon="⚙"
              content={`${info.cpuCores} 核 ${info.cpuUsagePct.toFixed(1)}%`}
              pct={info.cpuUsagePct}
              stale={stale}
            />
          </div>
          <div style={divider}>
            <MetricChip
              label="内存"
              icon="📊"
              content={`${formatBytes(info.memUsedBytes)}/${formatBytes(info.memTotalBytes)} ${info.memUsagePct.toFixed(0)}%`}
              pct={info.memUsagePct}
              stale={stale}
            />
          </div>
          <div>
            <MetricChip
              label="磁盘"
              icon="💾"
              content={
                info.diskMaxMount
                  ? `总 ${formatBytes(info.diskTotalBytes)} (${info.diskTotalPct.toFixed(0)}%) · 最高 ${info.diskMaxMount} ${info.diskMaxUsed}/${info.diskMaxSize} (${info.diskMaxPct.toFixed(0)}%)`
                  : `总 ${formatBytes(info.diskTotalBytes)} (${info.diskTotalPct.toFixed(0)}%)`
              }
              pct={info.diskMaxPct}
              stale={stale}
              title={
                info.diskMaxDev
                  ? `最高分区设备 ${info.diskMaxDev} 挂载于 ${info.diskMaxMount}`
                  : undefined
              }
            />
          </div>
          {stale && (
            <span
              style={{
                marginLeft: "auto",
                alignSelf: "center",
                padding: "0 12px",
                fontSize: 9,
                color: "var(--warning)",
              }}
              title="刷新超时"
            >
              ● stale
            </span>
          )}
        </>
      )}
    </div>
  );
}
