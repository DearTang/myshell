import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Terminal } from "@xterm/xterm";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  aiChat,
  aiInspectHealthSsh,
  aiInspectHealthLocal,
  onAiToken,
  onAiDone,
  onAiError,
  type AiContext,
  type ChatMessage,
  type ConnType,
} from "../api";

interface Props {
  /** Active tab context. connType drives the shell hint; sessionId + getTerminal
   * let the panel read terminal output/selection and paste commands. */
  activeConnType?: ConnType;
  activeConnectionName?: string;
  activeSessionId?: string;
  getTerminal?: (sid?: string) => Terminal | undefined;
  width: number;
  onWidthChange: (w: number) => void;
  onClose: () => void;
}

interface Msg {
  id: string;
  role: "user" | "assistant";
  content: string;
  /** Context snapshot attached at send time — shown above the user message
   * as a muted snippet so the user sees what was sent to the AI. */
  selection?: string;
  error?: boolean;
  streaming?: boolean;
}

function newId(): string {
  try {
    return crypto.randomUUID();
  } catch {
    return Math.random().toString(36).slice(2) + Date.now().toString(36);
  }
}

function shellHintFor(connType?: ConnType): string {
  return connType === "local" ? "powershell" : "bash";
}

/** Read the last `n` non-empty lines from the normal buffer (skips blank
 * trailing rows). In an alternate buffer (vim/claude TUI) scrollback is empty,
 * so this just returns the current screen — fine for "what's on screen". */
function readRecentLines(term: Terminal, n: number): string {
  const buf = term.buffer.active;
  const total = buf.length;
  const lines: string[] = [];
  for (let i = Math.max(0, total - n); i < total; i++) {
    const line = buf.getLine(i);
    if (line) {
      const s = line.translateToString(true);
      if (s.trim()) lines.push(s);
    }
  }
  return lines.slice(-n).join("\n");
}

/** Cap auto-attached recent terminal output so we never flood the system
 * prompt. Keeps the most recent `max` characters and stamps a truncation note
 * at the top when something was dropped. User selections are NOT capped — the
 * user explicitly chose that text, so we send it verbatim. */
const RECENT_OUTPUT_MAX = 5000;
function capRecentOutput(s: string): string {
  if (s.length <= RECENT_OUTPUT_MAX) return s;
  const dropped = s.length - RECENT_OUTPUT_MAX;
  return (
    `…（已省略较早的 ${dropped} 个字符，仅保留最近输出）\n` +
    s.slice(s.length - RECENT_OUTPUT_MAX)
  );
}

export function AiPanel({
  activeConnType,
  activeConnectionName,
  activeSessionId,
  getTerminal,
  width,
  onWidthChange,
  onClose,
}: Props) {
  const [messages, setMessages] = useState<Msg[]>([]);
  // Mirror of `messages` for reading inside async callbacks without waiting for
  // the next render (setMessages is async). The send() handler reads this to
  // build the full conversation history for the backend — without it, each
  // request would only carry the latest user turn and the AI would lose all
  // prior context (the reported "no memory" symptom).
  const messagesRef = useRef<Msg[]>([]);
  // Keep the ref in sync with the state so async callbacks read the latest
  // conversation history without waiting for a re-render.
  messagesRef.current = messages;
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [attachedSelection, setAttachedSelection] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const activeReqRef = useRef<{ reqId: string; unlistens: UnlistenFn[] } | null>(null);

  // Cancel any in-flight subscription on unmount.
  useEffect(() => {
    return () => {
      activeReqRef.current?.unlistens.forEach((u) => u());
      activeReqRef.current = null;
    };
  }, []);

  // Auto-scroll to bottom as tokens arrive.
  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages]);

  const activeTerm = activeSessionId ? getTerminal?.(activeSessionId) : undefined;

  // Pull the active terminal's selection (if any) or the last ~40 lines as
  // context for the next request. Returned as an AiContext for the backend
  // to fold into the system prompt.
  const collectContext = useCallback((): AiContext => {
    const ctx: AiContext = { shellHint: shellHintFor(activeConnType), connType: activeConnType };
    if (attachedSelection) {
      ctx.selection = attachedSelection;
      return ctx;
    }
    if (activeTerm) {
      const sel = activeTerm.getSelection();
      if (sel && sel.trim()) ctx.selection = sel;
      else {
        const out = readRecentLines(activeTerm, 40);
        if (out) ctx.terminalOutput = capRecentOutput(out);
      }
    }
    return ctx;
  }, [activeConnType, activeTerm, attachedSelection]);

  /** Snapshot the selection/context that will be sent alongside this message.
   * Used to render a preview inside the user bubble so the user can see
   * exactly what context the AI received. */
  const snapshotSelection = useCallback((): string | undefined => {
    if (attachedSelection) return attachedSelection;
    if (activeTerm) {
      const sel = activeTerm.getSelection();
      if (sel && sel.trim()) return sel;
      const out = readRecentLines(activeTerm, 40);
      if (out) return capRecentOutput(out);
    }
    return undefined;
  }, [activeTerm, attachedSelection]);

  const pasteToTerminal = useCallback(
    (text: string) => {
      // term.paste() drives xterm's onData → the same path as the user typing,
      // so the command shows up in the terminal for the user to review and
      // press Enter (we deliberately don't auto-run AI output).
      activeTerm?.paste(text);
    },
    [activeTerm]
  );

  const cancelActive = useCallback(() => {
    activeReqRef.current?.unlistens.forEach((u) => u());
    activeReqRef.current = null;
    setStreaming(false);
    setMessages((prev) => prev.map((m) => (m.streaming ? { ...m, streaming: false } : m)));
  }, []);

  const send = useCallback(async () => {
    const text = input.trim();
    if (!text || streaming) return;
    const reqId = newId();
    const sel = snapshotSelection();
    const userMsg: Msg = { id: `${reqId}-u`, role: "user", content: text, selection: sel };
    const aiMsg: Msg = { id: reqId, role: "assistant", content: "", streaming: true };
    setMessages((prev) => [...prev, userMsg, aiMsg]);
    setInput("");
    setAttachedSelection(null);
    setStreaming(true);

    activeReqRef.current?.unlistens.forEach((u) => u());

    const finish = () => {
      setStreaming(false);
      activeReqRef.current?.unlistens.forEach((u) => u());
      activeReqRef.current = null;
      setMessages((prev) => prev.map((m) => (m.id === reqId ? { ...m, streaming: false } : m)));
    };

    const subs = await Promise.all([
      onAiToken(reqId, (tok) => {
        setMessages((prev) =>
          prev.map((m) => (m.id === reqId ? { ...m, content: m.content + tok } : m))
        );
      }),
      onAiDone(reqId, finish),
      onAiError(reqId, (err) => {
        setMessages((prev) =>
          prev.map((m) =>
            m.id === reqId ? { ...m, content: err, error: true, streaming: false } : m
          )
        );
        finish();
      }),
    ]);
    activeReqRef.current = { reqId, unlistens: subs };

    const context = collectContext();
    // Build the FULL conversation history to send to the backend. Previously
    // this only sent `[payload]` (the latest user turn), so the AI had zero
    // memory of earlier turns — the reported "no context continuity" bug.
    // We read from messagesRef (not `messages`, which may be stale in this
    // callback) and filter out failed/incomplete turns: an errored assistant
    // reply or a still-streaming bubble carries no usable answer to build on.
    const history: ChatMessage[] = messagesRef.current
      .filter((m) => !m.error && !m.streaming && m.content.trim().length > 0)
      .map((m) => ({ role: m.role, content: m.content }));
    history.push({ role: "user", content: text });
    aiChat(reqId, history, null, context).catch((e) => {
      setMessages((prev) =>
        prev.map((m) =>
          m.id === reqId ? { ...m, content: `请求失败: ${e}`, error: true, streaming: false } : m
        )
      );
      finish();
    });
  }, [input, streaming, collectContext, snapshotSelection]);

  // Health inspection: the backend runs a preset read-only script (SSH over
  // exec_once, local via the OS shell) and streams an AI health report over
  // the same ai_token/ai_done/ai_error events as a normal chat. SSH needs a
  // sessionId; local inspects the user's own machine. sftp/ftp tabs have no
  // shell, so the button only shows for ssh/local.
  const runInspection = useCallback(async () => {
    if (streaming) return;
    const reqId = newId();
    const isSsh = activeConnType === "ssh" && !!activeSessionId;
    const userMsg: Msg = {
      id: `${reqId}-u`,
      role: "user",
      content: isSsh ? "🔍 正在采集服务器指标并生成健康报告…" : "🔍 正在采集本机指标并生成健康报告…",
    };
    const aiMsg: Msg = { id: reqId, role: "assistant", content: "", streaming: true };
    setMessages((prev) => [...prev, userMsg, aiMsg]);
    setAttachedSelection(null);
    setStreaming(true);

    activeReqRef.current?.unlistens.forEach((u) => u());
    const finish = () => {
      setStreaming(false);
      activeReqRef.current?.unlistens.forEach((u) => u());
      activeReqRef.current = null;
      setMessages((prev) => prev.map((m) => (m.id === reqId ? { ...m, streaming: false } : m)));
    };
    const subs = await Promise.all([
      onAiToken(reqId, (tok) => {
        setMessages((prev) =>
          prev.map((m) => (m.id === reqId ? { ...m, content: m.content + tok } : m))
        );
      }),
      onAiDone(reqId, finish),
      onAiError(reqId, (err) => {
        setMessages((prev) =>
          prev.map((m) =>
            m.id === reqId ? { ...m, content: err, error: true, streaming: false } : m
          )
        );
        finish();
      }),
    ]);
    activeReqRef.current = { reqId, unlistens: subs };

    const kickoff = isSsh
      ? aiInspectHealthSsh(activeSessionId!, reqId)
      : aiInspectHealthLocal(reqId);
    kickoff.catch((e) => {
      setMessages((prev) =>
        prev.map((m) =>
          m.id === reqId
            ? { ...m, content: `巡检失败: ${e}`, error: true, streaming: false }
            : m
        )
      );
      finish();
    });
  }, [streaming, activeConnType, activeSessionId]);

  const attachSelection = () => {
    if (!activeTerm) return;
    const sel = activeTerm.getSelection();
    if (sel && sel.trim()) setAttachedSelection(sel);
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  };

  const onResizeStart = (e: ReactMouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = width;
    const onMove = (ev: globalThis.MouseEvent) => {
      const w = Math.min(720, Math.max(300, startW - (ev.clientX - startX)));
      onWidthChange(w);
    };
    const onUp = () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  };

  const copy = (text: string) => {
    navigator.clipboard?.writeText(text).catch(() => {});
  };

  const ctxLabel =
    activeConnectionName ?? (activeConnType ? `${activeConnType} 终端` : "未连接");

  return (
    <div
      style={{
        width,
        flexShrink: 0,
        height: "100%",
        display: "flex",
        flexDirection: "column",
        background: "var(--bg-sidebar)",
        borderLeft: "1px solid var(--border-default)",
        position: "relative",
      }}
    >
      <div
        onMouseDown={onResizeStart}
        style={{ position: "absolute", left: -3, top: 0, bottom: 0, width: 6, cursor: "col-resize", zIndex: 5 }}
      />
      {/* Header */}
      <div style={headerStyle}>
        <div style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
          <span style={{ fontWeight: 600, color: "var(--text-primary)", fontSize: 13 }}>
            🤖 AI 助手
          </span>
          <span style={ctxLabelStyle} title={ctxLabel}>
            上下文：{ctxLabel}
          </span>
        </div>
        {(activeConnType === "ssh" || activeConnType === "local") && (
          <button
            onClick={runInspection}
            disabled={streaming}
            style={inspectBtnStyle}
            title="采集服务器/本机指标，让 AI 出健康报告"
          >
            🔍 巡检
          </button>
        )}
        <button onClick={onClose} style={closeBtnStyle} title="关闭">
          ×
        </button>
      </div>

      {/* Messages */}
      <div ref={scrollRef} style={messagesStyle}>
        {messages.length === 0 && (
          <div style={{ color: "var(--text-tertiary)", fontSize: 13, lineHeight: 1.7 }}>
            向 AI 描述你想做的事，比如：
            <br />· “生成一个查找最大文件的命令”
            <br />· “解释 awk -F: &#123;print $1&#125; /etc/passwd”
            <br />· 选中终端里的报错，点下方“附带选区”让 AI 排查
          </div>
        )}
        {messages.map((m) => (
          <MessageBubble
            key={m.id}
            msg={m}
            onCopy={copy}
            onPaste={pasteToTerminal}
            canPaste={!!activeTerm}
          />
        ))}
      </div>

      {/* Input */}
      <div style={inputBarStyle}>
        {attachedSelection && (
          <div
            style={{
              fontSize: 11,
              color: "var(--text-secondary)",
              background: "var(--accent-primary-muted)",
              border: "1px solid var(--border-accent)",
              borderRadius: 6,
              padding: "4px 8px",
              marginBottom: 6,
              display: "flex",
              alignItems: "center",
              gap: 6,
            }}
          >
            <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
              📎 已附带选区：{attachedSelection.slice(0, 40)}
              {attachedSelection.length > 40 ? "…" : ""}
            </span>
            <button
              onClick={() => setAttachedSelection(null)}
              style={{ background: "transparent", border: "none", color: "var(--text-secondary)", cursor: "pointer", fontSize: 13 }}
            >
              ×
            </button>
          </div>
        )}
        <div style={{ position: "relative" }}>
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={onKeyDown}
            title="Enter 发送
Shift+Enter 换行"
            placeholder="描述你想做的事…
Enter 发送 ● Shift+Enter 换行"
            rows={4}
            style={textareaStyle}
          />
          {streaming && (
            <button
              onClick={cancelActive}
              style={stopBtnOverlayStyle}
              title="停止生成"
            >
              ■
            </button>
          )}
        </div>
        <div style={{ display: "flex", justifyContent: "space-between", marginTop: 6 }}>
          <button
            onClick={attachSelection}
            disabled={!activeTerm}
            style={{
              background: "transparent",
              border: "none",
              color: activeTerm ? "var(--text-secondary)" : "var(--text-muted)",
              fontSize: 11,
              cursor: activeTerm ? "pointer" : "default",
              padding: 0,
            }}
            title={activeTerm ? "把终端里选中的文本作为上下文" : "当前无活动终端"}
          >
            📎 附带选区
          </button>
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
            {activeTerm ? "将自动附带最近输出" : "AI 命令请确认后再执行"}
          </span>
        </div>
      </div>
    </div>
  );
}

const headerStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  padding: "10px 14px",
  borderBottom: "1px solid var(--border-default)",
  flexShrink: 0,
};
const ctxLabelStyle: CSSProperties = {
  fontSize: 11,
  color: "var(--text-tertiary)",
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
};
const closeBtnStyle: CSSProperties = {
  background: "transparent",
  color: "var(--text-secondary)",
  fontSize: 18,
  lineHeight: 1,
  padding: "4px 8px",
  borderRadius: 6,
};
const inspectBtnStyle: CSSProperties = {
  background: "var(--bg-surface)",
  color: "var(--text-secondary)",
  border: "1px solid var(--border-default)",
  borderRadius: 6,
  padding: "4px 10px",
  fontSize: 12,
  cursor: "pointer",
  marginRight: 4,
};
const messagesStyle: CSSProperties = {
  flex: 1,
  overflowY: "auto",
  padding: 14,
  display: "flex",
  flexDirection: "column",
  gap: 12,
};
const inputBarStyle: CSSProperties = {
  padding: 12,
  borderTop: "1px solid var(--border-default)",
  flexShrink: 0,
};
const textareaStyle: CSSProperties = {
  flex: 1,
  width: "100%",
  resize: "vertical",
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  border: "1px solid var(--border-default)",
  borderRadius: 8,
  padding: "8px 10px",
  fontSize: 13,
  lineHeight: 1.5,
  fontFamily: "inherit",
  outline: "none",
  minHeight: 48,
  maxHeight: 300,
  boxSizing: "border-box",
};

const stopBtnOverlayStyle: CSSProperties = {
  position: "absolute",
  right: 8,
  bottom: 8,
  width: 28,
  height: 28,
  background: "var(--error-muted)",
  border: "1px solid var(--error)",
  borderRadius: 6,
  color: "var(--error)",
  fontSize: 12,
  lineHeight: 1,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  cursor: "pointer",
  transition: "all 0.2s cubic-bezier(0.32,0.72,0,1)",
};

function MessageBubble({
  msg,
  onCopy,
  onPaste,
  canPaste,
}: {
  msg: Msg;
  onCopy: (t: string) => void;
  onPaste: (t: string) => void;
  canPaste: boolean;
}) {
  const isUser = msg.role === "user";
  const [selExpanded, setSelExpanded] = useState(false);
  return (
    <div style={{ display: "flex", justifyContent: isUser ? "flex-end" : "flex-start" }}>
      <div
        style={{
          maxWidth: "92%",
          padding: isUser ? "8px 12px" : "10px 12px",
          borderRadius: 10,
          background: msg.error
            ? "rgba(248,81,73,0.12)"
            : isUser
            ? "var(--accent-primary-muted)"
            : "var(--bg-surface)",
          border: msg.error
            ? "1px solid rgba(248,81,73,0.4)"
            : "1px solid var(--border-subtle)",
          color: msg.error ? "var(--error)" : "var(--text-primary)",
          fontSize: 13,
          lineHeight: 1.6,
          display: "flex",
          flexDirection: "column",
          gap: 6,
        }}
      >
        {isUser && msg.selection && (
          <div
            onDoubleClick={() => setSelExpanded((v) => !v)}
            title={selExpanded ? "双击收起为单行" : "双击展开完整内容"}
            style={{
              fontSize: 11,
              lineHeight: 1.5,
              color: "var(--text-tertiary)",
              background: "var(--bg-base)",
              border: "1px solid var(--border-subtle)",
              borderRadius: 6,
              padding: "6px 8px",
              whiteSpace: selExpanded ? "pre-wrap" : "nowrap",
              overflow: selExpanded ? "auto" : "hidden",
              textOverflow: selExpanded ? undefined : "ellipsis",
              wordBreak: selExpanded ? "break-word" : undefined,
              maxHeight: selExpanded ? 240 : undefined,
              fontFamily: "'Cascadia Code', Consolas, monospace",
              cursor: "default",
              userSelect: "none",
            }}
          >
            {selExpanded ? msg.selection : msg.selection.replace(/\s+/g, " ").trim()}
          </div>
        )}
        {isUser ? (
          <div style={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{msg.content}</div>
        ) : msg.content === "" && msg.streaming ? (
          <span style={{ color: "var(--text-muted)" }}>思考中…</span>
        ) : (
          <div className="ai-markdown">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                pre: ({ children }) => <>{children}</>,
                code: ({ className, children }) => {
                  const text = String(children).replace(/\n$/, "");
                  const isBlock =
                    typeof className === "string" && className.startsWith("language-");
                  if (isBlock) {
                    return (
                      <div
                        style={{
                          position: "relative",
                          margin: "8px 0",
                          borderRadius: 8,
                          overflow: "hidden",
                          border: "1px solid var(--border-default)",
                        }}
                      >
                        <div style={{ position: "absolute", top: 6, right: 6, zIndex: 2, display: "flex", gap: 4 }}>
                          {canPaste && (
                            <button
                              onClick={() => onPaste(text)}
                              style={codeBtnStyle}
                              title="把命令插入当前终端（需自行按回车执行）"
                            >
                              插入终端
                            </button>
                          )}
                          <button onClick={() => onCopy(text)} style={codeBtnStyle}>
                            复制
                          </button>
                        </div>
                        <pre
                          style={{
                            margin: 0,
                            padding: "10px 12px",
                            overflowX: "auto",
                            background: "var(--bg-base)",
                          }}
                        >
                          <code style={{ fontFamily: "'Cascadia Code', Consolas, monospace", fontSize: 12 }}>
                            {children}
                          </code>
                        </pre>
                      </div>
                    );
                  }
                  return (
                    <code
                      style={{
                        background: "var(--bg-surface-active)",
                        padding: "1px 5px",
                        borderRadius: 4,
                        fontFamily: "'Cascadia Code', Consolas, monospace",
                        fontSize: 12,
                      }}
                    >
                      {children}
                    </code>
                  );
                },
              }}
            >
              {msg.content}
            </ReactMarkdown>
          </div>
        )}
      </div>
    </div>
  );
}

const codeBtnStyle: CSSProperties = {
  background: "var(--bg-surface-active)",
  color: "var(--text-secondary)",
  border: "none",
  borderRadius: 5,
  padding: "3px 8px",
  fontSize: 11,
  cursor: "pointer",
};
