import { useEffect } from "react";

interface Props {
  version: string;
  onAgree: () => void;
  onDecline: () => void;
}

const overlay: React.CSSProperties = {
  position: "fixed",
  inset: 0,
  background: "var(--bg-overlay)",
  backdropFilter: "blur(8px)",
  WebkitBackdropFilter: "blur(8px)",
  zIndex: 2200,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
};

const panel: React.CSSProperties = {
  background: "var(--bg-elevated)",
  border: "1px solid var(--border-emphasis)",
  borderRadius: "var(--radius-xl)",
  boxShadow: "var(--shadow-xl)",
  width: 460,
  maxWidth: "92vw",
  padding: "24px 28px",
};

const btnPrimary: React.CSSProperties = {
  padding: "8px 18px",
  background: "var(--accent-primary)",
  color: "#ffffff",
  border: "none",
  borderRadius: "var(--radius-md)",
  fontSize: 13,
  fontWeight: 600,
  cursor: "pointer",
};

const btnGhost: React.CSSProperties = {
  padding: "8px 16px",
  background: "transparent",
  color: "var(--text-secondary)",
  border: "1px solid var(--border-default)",
  borderRadius: "var(--radius-md)",
  fontSize: 13,
  cursor: "pointer",
};

/**
 * One-time consent prompt for anonymous usage statistics. Shown on the first
 * launch of each new version (if the user hasn't previously agreed).
 *
 * The dialog explains WHAT is collected (only version + OS + random device
 * ID), what is NOT collected (no hosts, passwords, connection data), and the
 * consent model (agree once = auto for future versions; decline = asked again
 * next version).
 */
export function StatsConsentDialog({ version, onAgree, onDecline }: Props) {
  // ESC = decline (the non-tracking option).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onDecline();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onDecline]);

  return (
    <div style={overlay} onClick={onDecline}>
      <div
        style={panel}
        className="animate-scale-in"
        onClick={(e) => e.stopPropagation()}
      >
        <div style={{ fontSize: 28, marginBottom: 12 }}>📊</div>
        <div
          style={{
            fontSize: 16,
            fontWeight: 600,
            color: "var(--text-primary)",
            marginBottom: 12,
          }}
        >
          帮助 MyShell 变得更好
        </div>
        <div
          style={{
            fontSize: 13,
            color: "var(--text-secondary)",
            lineHeight: 1.7,
            marginBottom: 16,
          }}
        >
          检测到你升级到了 v{version}。是否允许发送一次{" "}
          <strong style={{ color: "var(--text-primary)" }}>完全匿名</strong>
          的统计数据，帮助我们了解有多少用户在使用？
        </div>

        <div
          style={{
            background: "var(--bg-input)",
            border: "1px solid var(--border-default)",
            borderRadius: "var(--radius-md)",
            padding: "12px 14px",
            marginBottom: 20,
            fontSize: 12,
            color: "var(--text-muted)",
            lineHeight: 1.6,
          }}
        >
          <div style={{ fontWeight: 600, color: "var(--text-secondary)", marginBottom: 6 }}>
            收集的内容（仅此而已）：
          </div>
          ✓ 应用版本号（v{version}）<br />
          ✓ 操作系统（如 Windows）<br />
          ✓ 一个随机设备 ID（用于去重计数，不绑定任何个人信息）
          <div
            style={{
              fontWeight: 600,
              color: "var(--text-secondary)",
              marginTop: 10,
              marginBottom: 4,
            }}
          >
            绝不收集：
          </div>
          ✗ 服务器地址 / 用户名 / 密码 / 连接内容
        </div>

        <div
          style={{
            fontSize: 11,
            color: "var(--text-tertiary)",
            marginBottom: 20,
            lineHeight: 1.5,
          }}
        >
          每次升级到新版本都会询问一次。同意将发送本次匿名统计；选择暂不则本次不发送。
        </div>

        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
          <button style={btnGhost} onClick={onDecline}>
            暂不
          </button>
          <button style={btnPrimary} onClick={onAgree} autoFocus>
            允许匿名统计
          </button>
        </div>
      </div>
    </div>
  );
}
