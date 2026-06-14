import { useState, useRef, useEffect } from "react";
import { verifyPassword } from "../api";

interface Props {
  onSuccess: () => void;
  onClose: () => void;
}

const MIN_LEN = 6;

export function PasswordVerifyDialog({ onSuccess, onClose }: Props) {
  const [pass, setPass] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const canSubmit = pass.length >= MIN_LEN && !busy;

  async function submit() {
    if (!canSubmit) return;
    setBusy(true);
    setErr(null);
    try {
      const valid = await verifyPassword(pass);
      if (valid) {
        setPass("");
        onSuccess();
      } else {
        setErr("密码错误");
        setPass("");
      }
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.55)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 3000,
      }}
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "var(--bg-panel)",
          border: "1px solid var(--border)",
          borderRadius: 10,
          padding: 20,
          width: 360,
          boxShadow: "0 12px 36px rgba(0,0,0,0.5)",
        }}
      >
        <div style={{ fontSize: 14, fontWeight: 600, marginBottom: 6 }}>
          🔐 验证密码
        </div>
        <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 14 }}>
          请输入登录密码以查看明文密码
        </div>

        <label style={labelStyle}>登录密码</label>
        <input
          ref={inputRef}
          type="password"
          value={pass}
          onChange={(e) => setPass(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") submit();
          }}
          placeholder="输入登录密码"
          style={inputStyle}
        />

        {err && (
          <div style={errorBoxStyle}>{err}</div>
        )}

        <div style={{ display: "flex", gap: 8, marginTop: 16, justifyContent: "flex-end" }}>
          <button onClick={onClose} style={cancelBtnStyle}>
            取消
          </button>
          <button
            onClick={submit}
            disabled={!canSubmit}
            style={{
              ...primaryBtnStyle,
              opacity: canSubmit ? 1 : 0.45,
              cursor: canSubmit ? "pointer" : "not-allowed",
            }}
          >
            {busy ? "验证中…" : "确认"}
          </button>
        </div>
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
  fontSize: 13,
  outline: "none",
};

const errorBoxStyle: React.CSSProperties = {
  marginTop: 10,
  padding: "8px 10px",
  background: "rgba(239,93,111,0.12)",
  border: "1px solid var(--error)",
  borderRadius: 6,
  fontSize: 12,
  color: "var(--error)",
};

const primaryBtnStyle: React.CSSProperties = {
  padding: "8px 16px",
  background: "var(--accent)",
  color: "var(--bg-panel)",
  border: "none",
  borderRadius: 6,
  fontSize: 12,
  fontWeight: 600,
};

const cancelBtnStyle: React.CSSProperties = {
  padding: "7px 14px",
  background: "transparent",
  color: "var(--text-secondary)",
  border: "1px solid var(--border)",
  borderRadius: 6,
  fontSize: 12,
  cursor: "pointer",
};
