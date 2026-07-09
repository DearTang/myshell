import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { zipSync, strToU8 } from "fflate";
import {
  getFeedbackLog,
  revealPath,
  saveFeedbackZip,
  uploadScreenshot,
  type FeedbackLogInfo,
} from "../api";

// ── Web3Forms ───────────────────────────────────────────────────────────
// The access key is PUBLIC by design (Web3Forms' own model — it's an alias to
// a fixed recipient email, not a credential). Even if extracted from the
// binary, the worst case is inbox spam to our address, NOT account takeover.
// Replace with the real key from web3forms.com after registering.
// TODO: replace this placeholder with the real Web3Forms access key.
const WEB3FORMS_ACCESS_KEY = "d16fbbbe-6348-4c08-843b-0a556c7d904e";
const WEB3FORMS_ENDPOINT = "https://api.web3forms.com/submit";

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
      if (e.key === "Escape" && !submitting) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, submitting]);

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

    let savedPath: string | null = null;
    try {
      // Always build the local zip first — it's the reliable fallback and
      // also gives the user a copy they can attach to a manual email.
      savedPath = await buildAndSaveZip();
    } catch (e) {
      // Zip save failure is non-fatal — we still try to submit the text.
      console.warn("feedback zip save failed", e);
    }

    // Upload each screenshot to a free image host (Telegraph) so the URL can
    // be embedded in the email body — Web3Forms free tier has no attachment
    // support. Failures are non-fatal: the screenshot is already in the local
    // zip, and we note in the email how many images failed to upload.
    const uploadedUrls: string[] = [];
    let uploadFailures = 0;
    let uploadError = "";
    for (const att of attachments) {
      try {
        const base64 = att.dataUrl.split(",")[1] ?? "";
        const bin = atob(base64);
        const u8 = new Uint8Array(bin.length);
        for (let j = 0; j < bin.length; j++) u8[j] = bin.charCodeAt(j);
        const mime = att.dataUrl.slice(
          att.dataUrl.indexOf(":") + 1,
          att.dataUrl.indexOf(";"),
        );
        const url = await uploadScreenshot(u8, mime);
        uploadedUrls.push(url);
      } catch (e) {
        console.warn("screenshot upload failed", e);
        uploadFailures++;
        uploadError = e instanceof Error ? e.message : String(e);
      }
    }

    // Build the email message: user description + embedded screenshot URLs.
    // Web3Forms renders the message field as the email body; markdown image
    // syntax becomes clickable links (and inline images in many clients).
    let message = description;
    if (uploadedUrls.length > 0) {
      message +=
        "\n\n--- 截图 ---\n" +
        uploadedUrls.map((u, i) => `截图${i + 1}: ${u}`).join("\n");
    }

    // Submit text + log + screenshot URLs via Web3Forms.
    try {
      const body = {
        access_key: WEB3FORMS_ACCESS_KEY,
        subject: `[MyShell反馈] ${TYPE_LABELS[type]} - v${version}`,
        from_name: "MyShell 反馈",
        ...(contact ? { email: contact } : {}),
        message,
        反馈类型: TYPE_LABELS[type],
        应用版本: `v${version}`,
        操作系统: navigator.platform,
        浏览器标识: navigator.userAgent.slice(0, 200),
        运行日志: attachLog && logInfo?.content ? logInfo.content : "(未附日志)",
        截图数量: String(attachments.length),
        截图链接: uploadedUrls.length
          ? uploadedUrls.join("\n")
          : uploadFailures > 0
            ? `${uploadFailures} 张截图上传失败，请查看本地反馈包`
            : "(无)",
        备注:
          uploadFailures > 0
            ? `${uploadFailures} 张截图上传图床失败，已保存在本地反馈包中。`
            : attachments.length > 0
              ? `${uploadedUrls.length} 张截图已上传图床，链接见上方"截图链接"。`
              : "",
      };

      const res = await fetch(WEB3FORMS_ENDPOINT, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });

      if (res.ok) {
        const data = await res.json().catch(() => ({}));
        if (data.success === false) {
          throw new Error(data.message || "提交被拒绝");
        }
        setResult({
          kind: "success",
          message:
            attachments.length > 0 && uploadFailures > 0
              ? `反馈已提交成功！${uploadedUrls.length}/${attachments.length} 张截图已上传，${uploadFailures} 张上传失败（${uploadError || "网络问题"}）已保存在本地反馈包中。`
              : attachments.length > 0
                ? "反馈已提交成功！截图已随邮件发送，感谢你的支持！"
                : "反馈已提交成功，感谢你的支持！",
          savedPath: savedPath ?? undefined,
        });
      } else if (res.status === 403) {
        // Web3Forms blocks non-browser / server-side origins. A Tauri webview
        // sends Origin: http://tauri.localhost which may trip this.
        setResult({
          kind: "error",
          message:
            "提交被服务器拒绝（可能是 Tauri 桌面端 origin 不被 Web3Forms 支持）。别担心——完整反馈（含日志和截图）已保存为本地反馈包，你可以手动发送。",
          savedPath: savedPath ?? undefined,
        });
      } else {
        throw new Error(`HTTP ${res.status}`);
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setResult({
        kind: "error",
        message:
          msg.includes("Failed to fetch") || msg.includes("NetworkError")
            ? "网络请求失败（可能是桌面端 origin 限制或网络问题）。完整反馈已保存为本地反馈包，可手动发送。"
            : `提交失败：${msg}。完整反馈已保存为本地反馈包，可手动发送。`,
        savedPath: savedPath ?? undefined,
      });
    } finally {
      setSubmitting(false);
    }
  }

  const canSubmit = description.trim().length > 0 && !submitting;

  return (
    <div style={overlay} onClick={() => !submitting && onClose()}>
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
              <div style={{ fontSize: 40, marginBottom: 12 }}>
                {result.kind === "success" ? "✅" : "📦"}
              </div>
              <div
                style={{
                  fontSize: 15,
                  fontWeight: 600,
                  color: "var(--text-primary)",
                  marginBottom: 8,
                }}
              >
                {result.kind === "success" ? "提交成功" : "已保存本地反馈包"}
              </div>
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
              {result.savedPath && (
                <div style={{ marginTop: 16 }}>
                  <button
                    style={{ ...btnGhost, display: "inline-flex", gap: 6 }}
                    onClick={() => {
                      // reveal the parent feedback dir (reveal_path whitelists
                      // the logs dir; feedback/ is a sibling — reveal the file's
                      // dir via its parent).
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
            <button style={btnPrimary} onClick={onClose}>
              完成
            </button>
          ) : (
            <>
              <button
                style={btnGhost}
                onClick={onClose}
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
