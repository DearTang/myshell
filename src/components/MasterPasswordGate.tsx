import { useEffect, useRef, useState } from "react";
import { setupVault, unlockVault, getLockoutInfo } from "../api";
import type { LockoutInfo } from "../api";

interface Props {
  mode: "setup" | "unlock";
  onSuccess: () => void;
}

const MIN_LEN = 6;

function strengthHint(p: string): { label: string; color: string; width: string } {
  let classes = 0;
  if (/[a-z]/.test(p)) classes++;
  if (/[A-Z]/.test(p)) classes++;
  if (/[0-9]/.test(p)) classes++;
  if (/[^A-Za-z0-9]/.test(p)) classes++;
  if (p.length < MIN_LEN) return { label: "至少 6 个字符", color: "var(--error)", width: "0%" };
  if (p.length < 8) return { label: "简单", color: "var(--warning)", width: "25%" };
  if (classes <= 2) return { label: "弱", color: "var(--error)", width: "50%" };
  if (classes === 3) return { label: "中等", color: "var(--warning)", width: "75%" };
  return { label: "强", color: "var(--success)", width: "100%" };
}

export function MasterPasswordGate({ mode, onSuccess }: Props) {
  const [pass, setPass] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [lockoutInfo, setLockoutInfo] = useState<LockoutInfo | null>(null);
  const [showPassword, setShowPassword] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    if (mode === "unlock") {
      getLockoutInfo()
        .then(setLockoutInfo)
        .catch(() => {});
    }
  }, [mode]);

  const isSetup = mode === "setup";
  const hint = strengthHint(pass);
  const tooShort = pass.length > 0 && pass.length < MIN_LEN;
  const mismatch = isSetup && confirm.length > 0 && confirm !== pass;
  const canSubmit =
    pass.length >= MIN_LEN && (!isSetup || confirm === pass) && !busy;

  async function submit() {
    if (!canSubmit) return;
    setBusy(true);
    setErr(null);
    try {
      if (isSetup) {
        await setupVault(pass);
      } else {
        await unlockVault(pass);
      }
      setPass("");
      setConfirm("");
      onSuccess();
    } catch (e) {
      setErr(String(e));
      setBusy(false);
      setPass("");
      if (isSetup) setConfirm("");
      if (!isSetup) {
        getLockoutInfo()
          .then(setLockoutInfo)
          .catch(() => {});
      }
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }

  function formatTime(timestamp: number | null): string {
    if (!timestamp) return "";
    const date = new Date(timestamp * 1000);
    return date.toLocaleString("zh-CN", {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--bg-base)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 5000,
      }}
    >
      {/* Background Gradient Mesh */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          background: `
            radial-gradient(ellipse at 30% 20%, var(--accent-primary-muted) 0%, transparent 50%),
            radial-gradient(ellipse at 70% 80%, var(--accent-secondary-muted) 0%, transparent 50%),
            var(--bg-base)
          `,
          opacity: 0.6,
        }}
      />

      {/* Main Card */}
      <div
        className="animate-scale-in"
        style={{
          position: "relative",
          width: 420,
          padding: 32,
          background: "var(--bg-elevated)",
          border: "1px solid var(--border-emphasis)",
          borderRadius: "var(--radius-xl)",
          boxShadow: "var(--shadow-xl)",
        }}
      >
        {/* Header */}
        <div style={{ textAlign: "center", marginBottom: 28 }}>
          <div
            style={{
              width: 64,
              height: 64,
              margin: "0 auto 16px",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              background: "var(--accent-primary-muted)",
              borderRadius: "var(--radius-xl)",
              fontSize: 28,
            }}
          >
            {isSetup ? "🔐" : "🔓"}
          </div>
          <div
            style={{
              fontSize: 20,
              fontWeight: 600,
              color: "var(--text-primary)",
              letterSpacing: "-0.02em",
            }}
          >
            {isSetup ? "设置登录密码" : "解锁 MyShell"}
          </div>
          <div
            style={{
              fontSize: 13,
              color: "var(--text-tertiary)",
              marginTop: 8,
              lineHeight: 1.6,
            }}
          >
            {isSetup ? (
              <>
                设置登录密码用于保护您的连接数据
                <div
                  style={{
                    marginTop: 8,
                    padding: "10px 12px",
                    background: "var(--warning-muted)",
                    border: "1px solid var(--warning)",
                    borderRadius: "var(--radius-md)",
                    color: "var(--warning)",
                    fontSize: 12,
                  }}
                >
                  ⚠ 遗忘后将无法找回，所有数据彻底丢失
                </div>
              </>
            ) : (
              <>
                输入登录密码解锁应用
                {lockoutInfo && lockoutInfo.dailyFailures > 0 && (
                  <div
                    style={{
                      marginTop: 10,
                      padding: "8px 12px",
                      background: "var(--warning-muted)",
                      borderRadius: "var(--radius-md)",
                      fontSize: 12,
                      color: "var(--warning)",
                    }}
                  >
                    今日已错 {lockoutInfo.dailyFailures} 次
                    {lockoutInfo.lastFailureTime && (
                      <span style={{ opacity: 0.8 }}>
                        {" "}
                        · 上次错误：{formatTime(lockoutInfo.lastFailureTime)}
                      </span>
                    )}
                  </div>
                )}
              </>
            )}
          </div>
        </div>

        {/* Password Input */}
        <div style={{ marginBottom: 16 }}>
          <label
            style={{
              display: "block",
              fontSize: 12,
              color: "var(--text-secondary)",
              marginBottom: 6,
              fontWeight: 500,
            }}
          >
            登录密码
          </label>
          <div style={{ position: "relative" }}>
            <input
              ref={inputRef}
              type={showPassword ? "text" : "password"}
              value={pass}
              onChange={(e) => setPass(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !isSetup) submit();
              }}
              placeholder="至少 6 个字符"
              onFocus={(e) => {
                e.currentTarget.style.borderColor = "var(--accent-primary)";
                e.currentTarget.style.boxShadow = "0 0 0 3px var(--accent-primary-muted)";
              }}
              onBlur={(e) => {
                e.currentTarget.style.borderColor = tooShort ? "var(--error)" : "var(--border-default)";
                e.currentTarget.style.boxShadow = "none";
              }}
              style={{
                width: "100%",
                padding: "12px 44px 12px 14px",
                background: "var(--bg-input)",
                color: "var(--text-primary)",
                border: `1px solid ${tooShort ? "var(--error)" : "var(--border-default)"}`,
                borderRadius: "var(--radius-md)",
                fontSize: 14,
                outline: "none",
                fontFamily: "ui-monospace, monospace",
                transition: "border-color var(--duration-fast) var(--ease-in-out), box-shadow var(--duration-fast) var(--ease-in-out)",
              }}
            />
            <button
              type="button"
              onClick={() => setShowPassword(!showPassword)}
              style={{
                position: "absolute",
                right: 12,
                top: "50%",
                transform: "translateY(-50%)",
                padding: 4,
                background: "transparent",
                border: "none",
                cursor: "pointer",
                color: "var(--text-tertiary)",
                fontSize: 16,
                opacity: pass ? 1 : 0.4,
                transition: "opacity var(--duration-fast) var(--ease-in-out)",
              }}
            >
              {showPassword ? "👁️" : "👁️‍🗨️"}
            </button>
          </div>
          {/* Strength Indicator - Only show in setup mode */}
          {isSetup && pass.length > 0 && (
            <div style={{ marginTop: 10 }}>
              <div
                style={{
                  height: 4,
                  background: "var(--bg-surface)",
                  borderRadius: "var(--radius-full)",
                  overflow: "hidden",
                }}
              >
                <div
                  style={{
                    height: "100%",
                    width: hint.width,
                    background: hint.color,
                    borderRadius: "var(--radius-full)",
                    transition: "width var(--duration-normal) var(--ease-out-expo)",
                  }}
                />
              </div>
              <div
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                  marginTop: 6,
                  fontSize: 11,
                  color: hint.color,
                }}
              >
                <span>强度：{hint.label}</span>
                <span style={{ color: "var(--text-muted)" }}>{pass.length} 字符</span>
              </div>
            </div>
          )}
        </div>

        {/* Confirm Input (Setup Only) */}
        {isSetup && (
          <div style={{ marginBottom: 16 }}>
            <label
              style={{
                display: "block",
                fontSize: 12,
                color: "var(--text-secondary)",
                marginBottom: 6,
                fontWeight: 500,
              }}
            >
              再次输入
            </label>
            <input
              type="password"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
              placeholder="确认登录密码"
              onFocus={(e) => {
                e.currentTarget.style.borderColor = "var(--accent-primary)";
                e.currentTarget.style.boxShadow = "0 0 0 3px var(--accent-primary-muted)";
              }}
              onBlur={(e) => {
                e.currentTarget.style.borderColor = mismatch ? "var(--error)" : "var(--border-default)";
                e.currentTarget.style.boxShadow = "none";
              }}
              style={{
                width: "100%",
                padding: "12px 14px",
                background: "var(--bg-input)",
                color: "var(--text-primary)",
                border: `1px solid ${mismatch ? "var(--error)" : "var(--border-default)"}`,
                borderRadius: "var(--radius-md)",
                fontSize: 14,
                outline: "none",
                fontFamily: "ui-monospace, monospace",
                transition: "border-color var(--duration-fast) var(--ease-in-out), box-shadow var(--duration-fast) var(--ease-in-out)",
              }}
            />
            {mismatch && (
              <div style={{ marginTop: 6, fontSize: 11, color: "var(--error)" }}>
                两次输入不一致
              </div>
            )}
          </div>
        )}

        {/* Error Message */}
        {err && (
          <div
            className="animate-fade-up"
            style={{
              marginBottom: 16,
              padding: "10px 14px",
              background: "var(--error-muted)",
              border: "1px solid var(--error)",
              borderRadius: "var(--radius-md)",
              fontSize: 12,
              color: "var(--error)",
              display: "flex",
              alignItems: "center",
              gap: 8,
            }}
          >
            <span style={{ fontSize: 14 }}>⚠️</span>
            {err}
          </div>
        )}

        {/* Submit Button */}
        <button
          onClick={submit}
          disabled={!canSubmit}
          style={{
            width: "100%",
            padding: "14px 20px",
            background: canSubmit ? "var(--accent-primary)" : "var(--bg-surface)",
            color: canSubmit ? "white" : "var(--text-muted)",
            border: "none",
            borderRadius: "var(--radius-md)",
            fontSize: 14,
            fontWeight: 600,
            cursor: canSubmit ? "pointer" : "not-allowed",
            opacity: canSubmit ? 1 : 0.6,
            transition: "all var(--duration-normal) var(--ease-out-expo)",
            boxShadow: canSubmit ? "var(--shadow-glow)" : "none",
          }}
          onMouseEnter={(e) => {
            if (canSubmit) {
              e.currentTarget.style.background = "var(--accent-primary-hover)";
              e.currentTarget.style.transform = "translateY(-1px)";
            }
          }}
          onMouseLeave={(e) => {
            if (canSubmit) {
              e.currentTarget.style.background = "var(--accent-primary)";
              e.currentTarget.style.transform = "translateY(0)";
            }
          }}
        >
          {busy ? (
            <span style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: 8 }}>
              <span style={{ animation: "spin 1s linear infinite" }}>⏳</span>
              处理中...
            </span>
          ) : isSetup ? (
            "设置并加密所有数据"
          ) : (
            "解锁"
          )}
        </button>

        {/* Footer Hint */}
        <div
          style={{
            marginTop: 20,
            textAlign: "center",
            fontSize: 11,
            color: "var(--text-muted)",
          }}
        >
          密码错误锁定机制：3 次错误锁定 5 分钟，每日最多 30 次
        </div>
      </div>
    </div>
  );
}
