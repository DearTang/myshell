import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { zipSync, strToU8 } from "fflate";
import {
  clearFeedbackDir,
  getFeedbackLog,
  openExternalUrl,
  revealPath,
  saveFeedbackZip,
  writeFrontendLog,
  type FeedbackLogInfo,
} from "../api";

interface Props {
  version: string;
  onClose: () => void;
}

type FeedbackType = "bug" | "feature" | "other";

interface Attachment {
  name: string;
  dataUrl: string; // data:image/png;base64,…
  bytes: number;
}

const TYPE_LABELS: Record<FeedbackType, string> = {
  bug: "问题报告",
  feature: "功能建议",
  other: "其他",
};

// ── Styles (mirror AboutDialog / AiPanel conventions) ────────────────────

const overlay: React.CSSProperties = {
  position: "fixed",
  inset: 0,
  background: "var(--bg-overlay)",
  backdropFilter: "blur(8px)",
  WebkitBackdropFilter: "blur(8px)",
  zIndex: 2100,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
};

const panel: React.CSSProperties = {
  background: "var(--bg-elevated)",
  border: "1px solid var(--border-emphasis)",
  borderRadius: "var(--radius-xl)",
  boxShadow: "var(--shadow-xl)",
  width: 580,
  maxWidth: "92vw",
  maxHeight: "86vh",
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
};

const scrollBody: React.CSSProperties = {
  flex: 1,
  overflowY: "auto",
  padding: "20px 24px",
};

const footer: React.CSSProperties = {
  flexShrink: 0,
  padding: "12px 24px",
  borderTop: "1px solid var(--border-subtle)",
  display: "flex",
  alignItems: "center",
  justifyContent: "flex-end",
  gap: 8,
  background: "var(--bg-surface)",
};

const labelStyle: React.CSSProperties = {
  fontSize: 12,
  fontWeight: 600,
  color: "var(--text-secondary)",
  marginBottom: 6,
  display: "block",
};

const textareaStyle: React.CSSProperties = {
  width: "100%",
  resize: "vertical" as const,
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  border: "1px solid var(--border-default)",
  borderRadius: "var(--radius-md)",
  padding: "10px 12px",
  fontSize: 13,
  lineHeight: 1.5,
  fontFamily: "inherit",
  outline: "none",
  minHeight: 100,
  maxHeight: 240,
  boxSizing: "border-box" as const,
};

const inputStyle: React.CSSProperties = {
  width: "100%",
  background: "var(--bg-input)",
  color: "var(--text-primary)",
  border: "1px solid var(--border-default)",
  borderRadius: "var(--radius-md)",
  padding: "8px 12px",
  fontSize: 13,
  outline: "none",
  boxSizing: "border-box" as const,
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

const sectionBtn: React.CSSProperties = {
  padding: "6px 12px",
  background: "var(--bg-input)",
  color: "var(--text-secondary)",
  border: "1px solid var(--border-default)",
  borderRadius: "var(--radius-md)",
  fontSize: 12,
  cursor: "pointer",
};

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

export function FeedbackDialog({ version, onClose }: Props) {
  const [type, setType] = useState<FeedbackType>("bug");
  const [description, setDescription] = useState("");
  const [contact, setContact] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [attachLog, setAttachLog] = useState(true);
  const [logInfo, setLogInfo] = useState<FeedbackLogInfo | null>(null);
  const [logLoading, setLogLoading] = useState(true);
  const [showLog, setShowLog] = useState(false);

  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState<
    | { kind: "success"; message: string; savedPath?: string }
    | { kind: "error"; message: string; savedPath?: string }
    | null
  >(null);

  const cancelledRef = useRef(false);

  // Wrap onClose to clear the feedback dir when the dialog closes. This runs
  // on every close path (cancel, ESC, "完成" button, overlay click) and
  // prevents old zip packages from piling up on disk.
  const handleClose = () => {
    void clearFeedbackDir().catch(() => {});
    onClose();
  };

  // Load the (already-scrubbed) log on open.
  useEffect(() => {
    cancelledRef.current = false;
    getFeedbackLog()
      .then((info) => {
        if (!cancelledRef.current) {
          setLogInfo(info);
          setLogLoading(false);
        }
      })
      .catch((e) => {
        if (!cancelledRef.current) {
          setLogInfo({
            logDir: "",
            content: `(读取日志失败: ${String(e)})`,
            truncated: false,
          });
          setLogLoading(false);
        }
      });
    return () => {
      cancelledRef.current = true;
    };
  }, []);

  // ESC to close.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !submitting) handleClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [handleClose, submitting]);

  async function addImageFromFile() {
    try {
      const selected = await open({
        multiple: true,
        filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] }],
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      for (const p of paths) {
        await loadFileAsAttachment(p);
      }
    } catch {
      // dialog cancelled — ignore
    }
  }

  // Read a local file via the existing read_file_base64 command (returns a
  // data URL). This reuses an audited command rather than a new one.
  async function loadFileAsAttachment(path: string) {
    const { readFileBase64 } = await import("../api");
    const dataUrl = await readFileBase64(path);
    const base64 = dataUrl.split(",")[1] ?? "";
    // Approximate byte size from base64 length.
    const bytes = Math.floor((base64.length * 3) / 4);
    const name = path.replace(/\\/g, "/").split("/").pop() ?? "image";
    setAttachments((prev) => [...prev, { name, dataUrl, bytes }]);
  }

  async function captureScreen() {
    try {
      // getDisplayMedia may not be available in all webview versions; the
      // catch degrades to "pick a file" silently.
      const mediaDevices = navigator.mediaDevices as MediaDevices & {
        getDisplayMedia?: (c: DisplayMediaStreamOptions) => Promise<MediaStream>;
      };
      if (!mediaDevices?.getDisplayMedia) {
        await addImageFromFile();
        return;
      }
      const stream = await mediaDevices.getDisplayMedia({ video: true });
      const track = stream.getVideoTracks()[0];
      // ImageCapture isn't universally typed; fall back to a video element grab.
      const video = document.createElement("video");
      video.srcObject = stream;
      await video.play();
      // Wait one frame for the video to render.
      await new Promise((r) => requestAnimationFrame(r));
      const canvas = document.createElement("canvas");
      canvas.width = video.videoWidth;
      canvas.height = video.videoHeight;
      const ctx = canvas.getContext("2d");
      ctx?.drawImage(video, 0, 0);
      track.stop();
      stream.getTracks().forEach((t) => t.stop());
      const dataUrl = canvas.toDataURL("image/png");
      const base64 = dataUrl.split(",")[1] ?? "";
      const bytes = Math.floor((base64.length * 3) / 4);
      const ts = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
      setAttachments((prev) => [
        ...prev,
        { name: `screenshot-${ts}.png`, dataUrl, bytes },
      ]);
    } catch {
      // User cancelled the screen picker, or getDisplayMedia unsupported.
      await addImageFromFile();
    }
  }

  function removeAttachment(idx: number) {
    setAttachments((prev) => prev.filter((_, i) => i !== idx));
  }

  /**
   * Build a local zip with: feedback.txt (type + description + env), the log
   * (if attached), and all images. Saved via the Rust save_feedback_zip
   * command into the feedback dir. This is the "open folder / manual send"
   * fallback that works regardless of the Web3Forms origin question.
   */
  async function buildAndSaveZip(): Promise<string | null> {
    const ts = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
    const feedbackText = [
      `MyShell 反馈报告`,
      `时间: ${new Date().toLocaleString()}`,
      `类型: ${TYPE_LABELS[type]}`,
      `版本: v${version}`,
      `平台: ${navigator.platform}`,
      `联系方式: ${contact || "(未提供)"}`,
      ``,
      `──── 描述 ────`,
      description,
      ``,
    ].join("\n");

    const files: Record<string, Uint8Array> = {
      "feedback.txt": strToU8(feedbackText),
    };

    if (attachLog && logInfo?.content) {
      files["myshell.log"] = strToU8(logInfo.content);
    }

    for (let i = 0; i < attachments.length; i++) {
      const att = attachments[i];
      const base64 = att.dataUrl.split(",")[1] ?? "";
      // Decode base64 → binary. atob is available in the webview.
      const bin = atob(base64);
      const u8 = new Uint8Array(bin.length);
      for (let j = 0; j < bin.length; j++) u8[j] = bin.charCodeAt(j);
      const ext = att.name.split(".").pop() ?? "png";
      files[`images/${i + 1}.${ext}`] = u8;
    }

    const zipped = zipSync(files);
    const path = await saveFeedbackZip(`myshell-feedback-${ts}`, zipped);
    return path;
  }

  async function handleSubmit() {
    if (!description.trim()) return;
    setSubmitting(true);
    setResult(null);

    // Step 1: Always build the local zip — it's the reliable backup and the
    // thing the user attaches to the email (mailto can't auto-attach files).
    let savedPath: string | null = null;
    try {
      savedPath = await buildAndSaveZip();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      console.warn("feedback zip save failed", e);
      writeFrontendLog("warn", `[feedback] 本地反馈包保存失败: ${msg}`);
    }

    // Step 2: Prepare the feedback content for the clipboard. We DON'T put
    // this in the mailto: body because many email clients (especially QQ Mail,
    // Outlook) silently drop the body when the URL is too long or the encoding
    // differs from what they expect. Instead we copy to clipboard and ask the
    // user to Ctrl+V — 100% reliable.
    const typeLabel = TYPE_LABELS[type];
    const subject = `【MYSHELL】${typeLabel} v${version}`;

    // Extract just the filename from savedPath for the clipboard note.
    const zipFileName = savedPath
      ? savedPath.replace(/[\\/]/g, "/").split("/").pop() ?? ""
      : "";

    const clipboardText = [
      `类型: ${typeLabel}`,
      `版本: v${version}`,
      `平台: ${navigator.platform}`,
      `联系方式: ${contact || "(未提供)"}`,
      ``,
      `描述:`,
      description,
    ].join("\n");

    // mailto: with subject only — short and reliable across all email clients.
    const mailtoUrl = `mailto:argustang@qq.com?subject=${encodeURIComponent(subject)}`;

    try {
      // Copy feedback content to clipboard first, then open the mail client.
      await navigator.clipboard.writeText(clipboardText);
      await openExternalUrl(mailtoUrl);
      writeFrontendLog("info", `[feedback] 已复制内容到剪贴板并唤起邮件客户端`);

      const noteParts = ["反馈内容已复制到剪贴板，请在邮件正文中按 Ctrl+V 粘贴。"];
      if (zipFileName) {
        noteParts.push(
          `然后点击下方按钮打开反馈包，将 "${zipFileName}" 拖入邮件作为附件后发送。`,
        );
      }
      setResult({
        kind: "success",
        message: noteParts.join(""),
        savedPath: savedPath ?? undefined,
      });

      // Extra popup reminder — the result panel might be missed if the email
      // client window covers it. This ensures the user sees the instruction.
      const reminder = zipFileName
        ? `反馈内容已复制到剪贴板！\n\n请在邮件中：\n1. 正文区域按 Ctrl+V 粘贴\n2. 将 "${zipFileName}" 拖入邮件作为附件`
        : `反馈内容已复制到剪贴板！\n\n请在邮件正文区域按 Ctrl+V 粘贴。`;
      window.alert(reminder);
      // Intentionally NOT auto-opening the folder — explorer.exe and the
      // mailto handler race for window focus, and either one can steal focus
      // from the other depending on OS scheduling. The result screen has a
      // button to open the folder after the email client has opened.
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      writeFrontendLog("error", `[feedback] 唤起邮件客户端失败: ${msg}`);
      setResult({
        kind: "error",
        message: `无法打开邮件客户端：${msg}。反馈包已保存在本地，你可以手动发送到 argustang@qq.com。`,
        savedPath: savedPath ?? undefined,
      });
    } finally {
      setSubmitting(false);
    }
  }

  const canSubmit = description.trim().length > 0 && !submitting;

  return (
    <div style={overlay} onClick={() => !submitting && handleClose()}>
      <div
        style={panel}
        className="animate-scale-in"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div
          style={{
            padding: "16px 24px",
            display: "flex",
            alignItems: "center",
            gap: 10,
          }}
        >
          <span style={{ fontSize: 20 }}>💬</span>
          <div>
            <div style={{ fontSize: 15, fontWeight: 600, color: "var(--text-primary)" }}>
              提交反馈
            </div>
            <div style={{ fontSize: 12, color: "var(--text-tertiary)" }}>
              MyShell v{version} · 帮助我们做得更好
            </div>
          </div>
        </div>

        {result ? (
          /* ── Result view ── */
          <div style={scrollBody}>
            <div
              style={{
                textAlign: "center",
                padding: "24px 12px",
              }}
            >
              <div
                style={{
                  fontSize: 48,
                  marginBottom: 12,
                  filter: result.kind === "error" ? "none" : "none",
                }}
              >
                {result.kind === "success" ? "✅" : "❌"}
              </div>
              <div
                style={{
                  fontSize: 16,
                  fontWeight: 700,
                  color:
                    result.kind === "success"
                      ? "var(--success, #40c057)"
                      : "var(--error, #ff3b30)",
                  marginBottom: 8,
                }}
              >
                {result.kind === "success" ? "提交成功" : "提交失败"}
              </div>
              {result.kind === "error" && (
                <div
                  style={{
                    display: "inline-block",
                    background: "var(--error-muted, rgba(255,59,48,0.1))",
                    border: "1px solid var(--error, #ff3b30)",
                    borderRadius: "var(--radius-md)",
                    padding: "10px 16px",
                    fontSize: 13,
                    color: "var(--text-primary)",
                    lineHeight: 1.6,
                    maxWidth: 440,
                    textAlign: "left",
                    margin: "0 auto 12px",
                  }}
                >
                  {result.message}
                </div>
              )}
              {result.kind === "success" && (
                <div
                  style={{
                    fontSize: 13,
                    color: "var(--text-secondary)",
                    lineHeight: 1.6,
                    maxWidth: 420,
                    margin: "0 auto",
                  }}
                >
                  {result.message}
                </div>
              )}
              {result.savedPath && (
                <div style={{ marginTop: 16 }}>
                  <button
                    style={{
                      ...btnGhost,
                      display: "inline-flex",
                      gap: 6,
                      ...(result.kind === "error"
                        ? {
                            borderColor: "var(--error, #ff3b30)",
                            color: "var(--error, #ff3b30)",
                          }
                        : {}),
                    }}
                    onClick={() => {
                      const dir = result.savedPath!.replace(/[\\/][^\\/]+$/, "");
                      void revealPath(dir);
                    }}
                  >
                    📂 打开反馈包所在文件夹
                  </button>
                </div>
              )}
            </div>
          </div>
        ) : (
          /* ── Form view ── */
          <div style={scrollBody}>
            {/* Type */}
            <div style={{ marginBottom: 16 }}>
              <label style={labelStyle}>反馈类型</label>
              <div style={{ display: "flex", gap: 8 }}>
                {(Object.keys(TYPE_LABELS) as FeedbackType[]).map((t) => (
                  <button
                    key={t}
                    onClick={() => setType(t)}
                    style={{
                      ...sectionBtn,
                      flex: 1,
                      padding: "8px 0",
                      textAlign: "center",
                      ...(type === t
                        ? {
                            background: "var(--accent-primary-muted)",
                            borderColor: "var(--accent-primary)",
                            color: "var(--text-primary)",
                            fontWeight: 600,
                          }
                        : {}),
                    }}
                  >
                    {TYPE_LABELS[t]}
                  </button>
                ))}
              </div>
            </div>

            {/* Description */}
            <div style={{ marginBottom: 16 }}>
              <label style={labelStyle}>
                描述 <span style={{ color: "var(--error)" }}>*</span>
              </label>
              <textarea
                style={textareaStyle}
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder={
                  type === "bug"
                    ? "发生了什么？你期望什么结果？复现步骤？（越详细越容易修复）"
                    : "你想看到什么功能？为什么需要它？"
                }
                autoFocus
              />
            </div>

            {/* Contact */}
            <div style={{ marginBottom: 16 }}>
              <label style={labelStyle}>联系方式（选填）</label>
              <input
                style={inputStyle}
                value={contact}
                onChange={(e) => setContact(e.target.value)}
                placeholder="邮箱 / QQ / 微信，方便我们追问细节"
              />
            </div>

            {/* Images */}
            <div style={{ marginBottom: 16 }}>
              <label style={labelStyle}>截图 / 图片（选填）</label>
              <div style={{ display: "flex", gap: 8, marginBottom: 8 }}>
                <button style={sectionBtn} onClick={() => void captureScreen()}>
                  📸 截图
                </button>
                <button style={sectionBtn} onClick={() => void addImageFromFile()}>
                  🖼️ 选择图片
                </button>
              </div>
              {attachments.length > 0 && (
                <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
                  {attachments.map((att, i) => (
                    <div
                      key={i}
                      style={{
                        position: "relative",
                        width: 80,
                        height: 80,
                        borderRadius: "var(--radius-md)",
                        overflow: "hidden",
                        border: "1px solid var(--border-default)",
                        background: "var(--bg-input)",
                      }}
                    >
                      <img
                        src={att.dataUrl}
                        alt={att.name}
                        style={{ width: "100%", height: "100%", objectFit: "cover" }}
                      />
                      <button
                        onClick={() => removeAttachment(i)}
                        title="移除"
                        style={{
                          position: "absolute",
                          top: 2,
                          right: 2,
                          width: 18,
                          height: 18,
                          borderRadius: "var(--radius-full)",
                          border: "none",
                          background: "rgba(0,0,0,0.6)",
                          color: "#fff",
                          fontSize: 11,
                          cursor: "pointer",
                          lineHeight: 1,
                          padding: 0,
                        }}
                      >
                        ✕
                      </button>
                      <div
                        style={{
                          position: "absolute",
                          bottom: 0,
                          left: 0,
                          right: 0,
                          background: "rgba(0,0,0,0.5)",
                          color: "#fff",
                          fontSize: 9,
                          textAlign: "center",
                          padding: "1px 0",
                        }}
                      >
                        {formatBytes(att.bytes)}
                      </div>
                    </div>
                  ))}
                </div>
              )}
              <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginTop: 6 }}>
                提交时截图会上传到免费图床并随邮件发送链接，同时也会保存在本地反馈包中作为备份。
              </div>
            </div>

            {/* Log */}
            <div>
              <label
                style={{
                  ...labelStyle,
                  display: "flex",
                  alignItems: "center",
                  gap: 8,
                  cursor: "pointer",
                }}
              >
                <input
                  type="checkbox"
                  checked={attachLog}
                  onChange={(e) => setAttachLog(e.target.checked)}
                  style={{ cursor: "pointer" }}
                />
                <span>
                  附带运行日志
                  {logLoading
                    ? "（加载中…）"
                    : logInfo
                      ? `（${formatBytes(new Blob([logInfo.content]).size)}${
                          logInfo.truncated ? "，已截断" : ""
                        }）`
                      : ""}
                </span>
              </label>
              <div style={{ fontSize: 11, color: "var(--text-tertiary)", marginBottom: 6 }}>
                日志已自动脱敏（主机名/用户名/IP 会被掩码），可放心提交。
                {logInfo?.truncated && " 仅包含最近的日志条目。"}
              </div>
              {attachLog && logInfo && (
                <div style={{ display: "flex", gap: 8 }}>
                  <button
                    style={sectionBtn}
                    onClick={() => setShowLog((s) => !s)}
                  >
                    {showLog ? "收起日志" : "查看日志内容"}
                  </button>
                  {logInfo.logDir && (
                    <button
                      style={sectionBtn}
                      onClick={() => void revealPath(logInfo.logDir)}
                    >
                      📂 打开日志目录
                    </button>
                  )}
                </div>
              )}
              {showLog && logInfo && (
                <pre
                  style={{
                    marginTop: 8,
                    maxHeight: 200,
                    overflow: "auto",
                    background: "var(--bg-input)",
                    border: "1px solid var(--border-default)",
                    borderRadius: "var(--radius-md)",
                    padding: 10,
                    fontSize: 11,
                    lineHeight: 1.5,
                    color: "var(--text-muted)",
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-all",
                  }}
                >
                  {logInfo.content || "(空)"}
                </pre>
              )}
            </div>
          </div>
        )}

        {/* Footer */}
        <div style={footer}>
          {result ? (
            <button style={btnPrimary} onClick={handleClose}>
              完成
            </button>
          ) : (
            <>
              <a
                style={{
                  fontSize: 12,
                  color: "var(--text-tertiary)",
                  textDecoration: "none",
                  cursor: "pointer",
                  marginRight: "auto",
                }}
                onClick={(e) => {
                  e.preventDefault();
                  void openExternalUrl(
                    "https://gitee.com/argustang/myshell/issues/new",
                  );
                }}
                title="在 Gitee 上提交 Issue"
              >
                🔗 也可通过 Gitee Issue 提交
              </a>
              <button
                style={btnGhost}
                onClick={handleClose}
                disabled={submitting}
              >
                取消
              </button>
              <button
                style={{
                  ...btnPrimary,
                  opacity: canSubmit ? 1 : 0.5,
                  cursor: canSubmit ? "pointer" : "not-allowed",
                }}
                onClick={() => void handleSubmit()}
                disabled={!canSubmit}
              >
                {submitting ? "提交中…" : "提交反馈"}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
