import { useEffect, useRef, useState } from "react";
import { setupVault, unlockVault } from "../api";

interface Props {
  mode: "setup" | "unlock";
  onSuccess: () => void;
}

const MIN_LEN = 12;

function strengthHint(p: string): { label: string; color: string } {
  let classes = 0;
  if (/[a-z]/.test(p)) classes++;
  if (/[A-Z]/.test(p)) classes++;
  if (/[0-9]/.test(p)) classes++;
  if (/[^A-Za-z0-9]/.test(p)) classes++;
  if (p.length < MIN_LEN) return { label: "至少 12 个字符", color: "var(--error)" };
  if (classes <= 2) return { label: "弱", color: "var(--error)" };
  if (classes === 3) return { label: "中", color: "var(--warning)" };
  return { label: "强", color: "var(--success)" };
}

export function MasterPasswordGate({ mode, onSuccess }: Props) {
  const [pass, setPass] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

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
      onSuccess();
    } catch (e) {
      setErr(String(e));
      setBusy(false);
      setPass("");
      if (isSetup) setConfirm("");
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--bg-dark)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 5000,
      }}
    >
      <div
        style={{
          width: 420,
          padding: 28,
          background: "var(--bg-panel)",
          border: "1px solid var(--border)",
          borderRadius: 12,
          boxShadow: "0 20px 60px rgba(0,0,0,0.5)",
        }}
      >
        <div style={{ textAlign: "center", marginBottom: 20 }}>
          <div style={{ fontSize: 42, opacity: 0.6, marginBottom: 10 }}>🔐</div>
          <div style={{ fontSize: 17, fontWeight: 600 }}>
            {isSetup ? "设置主密码" : "解锁 MyShell"}
          </div>
          <div
            style={{
              fontSize: 12,
              color: "var(--text-muted)",
              marginTop: 8,
              lineHeight: 1.6,
            }}
          >
            {isSetup ? (
              <>
                主密码用于加密所有连接信息（host/账号/私钥/密码）。
                <br />
                <b style={{ color: "var(--warning)" }}>遗忘后将无法找回，所有数据彻底丢失。</b>
                <br />
                请妥善保存（如密码管理器、纸质抄写）。
              </>
            ) : (
              <>
                输入主密码解锁 vault。
                <br />
                连续 5 次错误后建议重置 vault（会清空所有连接）。
              </>
            )}
          </div>
        </div>

        <label style={labelStyle}>主密码</label>
        <input
          ref={inputRef}
          type="password"
          value={pass}
          onChange={(e) => setPass(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !isSetup) submit();
          }}
          placeholder="至少 12 个字符"
          style={{
            ...inputStyle,
            borderColor: tooShort ? "var(--error)" : "var(--border)",
          }}
        />
        <div style={{ fontSize: 11, color: hint.color, marginTop: 4, height: 14 }}>
          {pass.length > 0 && `强度：${hint.label}（长度 ${pass.length}）`}
        </div>

        {isSetup && (
          <>
            <label style={{ ...labelStyle, marginTop: 8 }}>再次输入</label>
            <input
              type="password"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
              placeholder="确认主密码"
              style={{
                ...inputStyle,
                borderColor: mismatch ? "var(--error)" : "var(--border)",
              }}
            />
            <div style={{ fontSize: 11, color: "var(--error)", marginTop: 4, height: 14 }}>
              {mismatch ? "两次输入不一致" : ""}
            </div>
          </>
        )}

        {err && (
          <div
            style={{
              marginTop: 10,
              padding: "8px 10px",
              background: "rgba(239,93,111,0.12)",
              border: "1px solid var(--error)",
              borderRadius: 6,
              fontSize: 12,
              color: "var(--error)",
            }}
          >
            {err}
          </div>
        )}

        <button
          onClick={submit}
          disabled={!canSubmit}
          style={{
            marginTop: 18,
            width: "100%",
            padding: "10px 14px",
            background: "var(--accent)",
            color: "var(--bg-panel)",
            border: "none",
            borderRadius: 6,
            fontSize: 13,
            fontWeight: 600,
            cursor: canSubmit ? "pointer" : "not-allowed",
            opacity: canSubmit ? 1 : 0.45,
          }}
        >
          {busy ? "处理中…" : isSetup ? "设置并加密所有数据" : "解锁"}
        </button>
      </div>
    </div>
  );
}

const labelStyle: React.CSSProperties = {
  display: "block",
  fontSize: 11,
  color: "var(--text-muted)",
  marginBottom: 4,
};

const inputStyle: React.CSSProperties = {
  width: "100%",
  boxSizing: "border-box",
  padding: "10px 12px",
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  border: "1px solid var(--border)",
  borderRadius: 6,
  fontSize: 14,
  outline: "none",
  fontFamily: "ui-monospace, monospace",
};
