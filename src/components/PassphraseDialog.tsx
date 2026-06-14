import { useEffect, useRef, useState } from "react";

interface Props {
  mode: "export" | "import";
  count: number;
  onSubmit: (passphrase: string) => Promise<void>;
  onClose: () => void;
}

const MIN_LEN = 12;

// Character pools for strong-password generation. Each pool is sampled at
// least once per password so the output always clears our 3-class strength
// bar (lower + upper + digit + symbol = 4 classes).
const LOWER = "abcdefghijklmnopqrstuvwxyz";
const UPPER = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGIT = "0123456789";
const SYMBOL = "!@#$%^&*-_=+?";

function randomFrom(pool: string): string {
  const buf = new Uint32Array(1);
  crypto.getRandomValues(buf);
  return pool[buf[0] % pool.length];
}

function generateStrongPassword(len = 24): string {
  // One of each class first to guarantee the strength floor, then fill the
  // rest from the combined pool. crypto.getRandomValues is CSPRNG (Web
  // Crypto), so this is safe for actual key derivation.
  const required = [randomFrom(LOWER), randomFrom(UPPER), randomFrom(DIGIT), randomFrom(SYMBOL)];
  const all = LOWER + UPPER + DIGIT + SYMBOL;
  const rest = Array.from({ length: len - required.length }, () => randomFrom(all));
  // Fisher-Yates shuffle so the required chars aren't always at the front.
  const arr = [...required, ...rest];
  for (let i = arr.length - 1; i > 0; i--) {
    const buf = new Uint32Array(1);
    crypto.getRandomValues(buf);
    const j = buf[0] % (i + 1);
    [arr[i], arr[j]] = [arr[j], arr[i]];
  }
  return arr.join("");
}

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

export function PassphraseDialog({ mode, count, onSubmit, onClose }: Props) {
  // Export mode: pre-fill with a generated strong password so the user can
  // just hit "复制 + 导出" without typing. Import mode: empty — user must
  // recall the password from a prior export.
  const [pass, setPass] = useState(() => (mode === "export" ? generateStrongPassword() : ""));
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (mode === "import") inputRef.current?.focus();
    else inputRef.current?.select();
  }, [mode]);

  const hint = strengthHint(pass);
  const tooShort = pass.length > 0 && pass.length < MIN_LEN;
  const canSubmit = pass.length >= MIN_LEN && !busy;

  function regen() {
    setPass(generateStrongPassword());
    setCopied(false);
    requestAnimationFrame(() => inputRef.current?.select());
  }

  async function copyPass() {
    try {
      await navigator.clipboard.writeText(pass);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Fallback for older webviews: select the field so Cmd/Ctrl+C works.
      inputRef.current?.select();
    }
  }

  async function submit() {
    if (!canSubmit) return;
    setBusy(true);
    setErr(null);
    try {
      await onSubmit(pass);
    } catch (e) {
      setErr(String(e));
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
        zIndex: 2000,
      }}
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: "var(--bg-panel)",
          border: "1px solid var(--border)",
          borderRadius: 10,
          padding: 22,
          width: 420,
          boxShadow: "0 12px 36px rgba(0,0,0,0.5)",
        }}
      >
        <div style={{ fontSize: 15, fontWeight: 600, marginBottom: 6 }}>
          {mode === "export" ? "🔐 加密导出" : "🔓 解密导入"}
        </div>
        <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 16, lineHeight: 1.5 }}>
          {mode === "export" ? (
            <>
              已生成强密码，请 <b>立即复制保存</b>。导入时需要此密码才能解密。
              <br />
              密码本身不会写入文件或保存到本地。
            </>
          ) : (
            <>
              从加密文件导入 {count > 0 ? "" : ""}连接，请输入导出时设置的密码。
              <br />
              密码错误将无法解密。
            </>
          )}
        </div>

        <label style={labelStyle}>加密密码</label>
        <div style={{ display: "flex", gap: 6 }}>
          <input
            ref={inputRef}
            type="text"
            value={pass}
            onChange={(e) => {
              setPass(e.target.value);
              setCopied(false);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit();
            }}
            style={{
              ...inputStyle,
              flex: 1,
              borderColor: tooShort ? "var(--error)" : "var(--border)",
              fontFamily: "ui-monospace, monospace",
            }}
          />
          {mode === "export" && (
            <>
              <button
                type="button"
                onClick={regen}
                title="重新生成"
                style={iconBtnStyle}
              >
                🔄
              </button>
              <button
                type="button"
                onClick={copyPass}
                title={copied ? "已复制" : "复制密码"}
                style={{
                  ...iconBtnStyle,
                  color: copied ? "var(--success)" : "var(--text-primary)",
                }}
              >
                {copied ? "✓" : "📋"}
              </button>
            </>
          )}
        </div>
        <div style={{ fontSize: 11, color: hint.color, marginTop: 4, height: 14 }}>
          {pass.length > 0 && (
            <>
              强度：<b>{hint.label}</b> · 长度 {pass.length}
              {mode === "export" && copied && " · 已复制到剪贴板"}
            </>
          )}
        </div>

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
              wordBreak: "break-all",
            }}
          >
            {err}
          </div>
        )}

        <div style={{ display: "flex", gap: 8, marginTop: 18, justifyContent: "flex-end" }}>
          <button onClick={onClose} style={cancelBtnStyle} disabled={busy}>
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
            {busy ? "处理中…" : mode === "export" ? "下一步" : "导入"}
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
  padding: "8px 10px",
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  border: "1px solid var(--border)",
  borderRadius: 6,
  fontSize: 13,
  outline: "none",
};

const iconBtnStyle: React.CSSProperties = {
  width: 36,
  height: 36,
  padding: 0,
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  border: "1px solid var(--border)",
  borderRadius: 6,
  fontSize: 14,
  cursor: "pointer",
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

const primaryBtnStyle: React.CSSProperties = {
  padding: "7px 14px",
  background: "var(--accent)",
  color: "var(--bg-panel)",
  border: "none",
  borderRadius: 6,
  fontSize: 12,
  fontWeight: 600,
};
