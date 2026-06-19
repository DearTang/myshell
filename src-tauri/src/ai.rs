//! AI assistant — multi-provider chat (Claude / OpenAI / Ollama) with
//! streaming token output to the frontend, plus server health inspection.
//!
//! Streaming reuses the same `window.emit` pattern as `ssh_output`: a tauri
//! command calls [`chat_stream`], which resolves the provider config + key
//! from the vault-backed `ai_settings` table, fires an HTTP request, and
//! emits `ai_token` / `ai_done` / `ai_error` events scoped to the originating
//! webview as SSE/NDJSON chunks arrive. Health inspection runs a preset
//! read-only script (SSH via `ssh::exec_once`, local via `std::process`) and
//! feeds the collected metrics into the same streaming path.
//!
//! Why an enum, not a trait object: the three providers differ only in
//! endpoint / auth headers / request-body shape / line-parsing, all of which
//! are synchronous and cheap. An enum + `match` avoids the async-trait /
//! `Pin<Box<dyn Future>>` machinery and keeps everything concrete.

use crate::{crypto, ssh, AppState};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tauri::{Emitter, State, WebviewWindow};

// ───────────────────────────── payloads (Rust → frontend) ─────────────────────────────

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AiTokenPayload {
    request_id: String,
    token: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AiDonePayload {
    request_id: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AiErrorPayload {
    request_id: String,
    error: String,
}

fn emit_error(window: &WebviewWindow, request_id: &str, error: impl Into<String>) {
    let _ = window.emit(
        "ai_error",
        AiErrorPayload {
            request_id: request_id.to_string(),
            error: error.into(),
        },
    );
}

// ───────────────────────────── request types (frontend → Rust) ─────────────────────────────

#[derive(serde::Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Live terminal context the frontend attaches. Serialized into the system
/// prompt so the model sees the user's current shell state (selection /
/// recent output / inspection data) alongside their question.
#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiContext {
    pub terminal_output: Option<String>,
    pub selection: Option<String>,
    pub inspect_data: Option<String>,
    pub shell_hint: Option<String>,
    /// "ssh" / "local" / "sftp" / "ftp" — lets the system prompt describe
    /// the runtime (a remote server vs. the user's own machine).
    pub conn_type: Option<String>,
}

/// Bundle the frontend's `ai_chat` args. Built by the tauri command wrapper
/// in `main.rs` (which takes the fields flat for idiomatic camelCase IPC).
pub struct AiChatParams {
    pub request_id: String,
    pub messages: Vec<ChatMessage>,
    pub system: Option<String>,
    pub context: Option<AiContext>,
}

struct LoadedSettings {
    provider: Provider,
    model: String,
    base_url: Option<String>,
    api_key: String,
    proxy_url: Option<String>,
    temperature: f64,
}

// ───────────────────────────── provider abstraction ─────────────────────────────

#[derive(Clone, Copy)]
pub enum Provider {
    Claude,
    OpenAi,
    Ollama,
}

impl Provider {
    pub fn parse(id: &str) -> Result<Self, String> {
        match id {
            "claude" => Ok(Self::Claude),
            "openai" => Ok(Self::OpenAi),
            "ollama" => Ok(Self::Ollama),
            other => Err(format!(
                "未知 AI 提供商: {}（支持 claude / openai / ollama）",
                other
            )),
        }
    }

    fn default_model(&self) -> &'static str {
        match self {
            Self::Claude => "claude-sonnet-4-6",
            Self::OpenAi => "gpt-4o",
            Self::Ollama => "llama3.1",
        }
    }

    fn endpoint(&self, base_url: &Option<String>) -> String {
        // The base URL is the versioned root; we append only the provider's
        // final path segment. Defaults carry "/v1"; a compatible provider
        // like Zhipu fills its own version (e.g. ".../v4"). If the user
        // pasted the FULL endpoint (base already ends with the segment), use
        // it verbatim so we never double it up — accepts both
        // "https://open.bigmodel.cn/api/paas/v4" and ".../v4/chat/completions".
        let suffix = match self {
            Self::Claude => "messages",
            Self::OpenAi => "chat/completions",
            Self::Ollama => "chat",
        };
        let base = base_url
            .as_deref()
            .map(|s| s.trim_end_matches('/'))
            .unwrap_or_else(|| match self {
                Self::Claude => "https://api.anthropic.com/v1",
                Self::OpenAi => "https://api.openai.com/v1",
                Self::Ollama => "http://localhost:11434/api",
            });
        if base.ends_with(suffix) {
            base.to_string()
        } else {
            format!("{}/{}", base, suffix)
        }
    }

    /// Auth headers per provider. Claude uses `x-api-key` + a version header;
    /// OpenAI a bearer token; Ollama (local) none.
    fn auth_headers(&self, api_key: &str) -> Vec<(&'static str, String)> {
        match self {
            Self::Claude => vec![
                ("x-api-key", api_key.to_string()),
                ("anthropic-version", "2023-06-01".to_string()),
            ],
            Self::OpenAi => vec![("Authorization", format!("Bearer {}", api_key))],
            Self::Ollama => vec![],
        }
    }

    /// Build the provider-specific JSON body. Claude takes `system` as a
    /// top-level field; OpenAI/Ollama prepend it as a `system`-role message.
    fn build_body(
        &self,
        model: &str,
        messages: &[ChatMessage],
        system: &Option<String>,
        temperature: f64,
    ) -> Value {
        let msgs: Vec<Value> = messages
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect();
        match self {
            Self::Claude => {
                let mut body = json!({
                    "model": model,
                    "max_tokens": 4096,
                    "messages": msgs,
                    "stream": true,
                    "temperature": temperature,
                });
                if let Some(s) = system {
                    body["system"] = json!(s);
                }
                body
            }
            Self::OpenAi => {
                let mut all: Vec<Value> = Vec::new();
                if let Some(s) = system {
                    all.push(json!({ "role": "system", "content": s }));
                }
                all.extend(msgs);
                json!({
                    "model": model,
                    "messages": all,
                    "stream": true,
                    "temperature": temperature,
                })
            }
            Self::Ollama => {
                let mut all: Vec<Value> = Vec::new();
                if let Some(s) = system {
                    all.push(json!({ "role": "system", "content": s }));
                }
                all.extend(msgs);
                json!({
                    "model": model,
                    "messages": all,
                    "stream": true,
                    "options": { "temperature": temperature },
                })
            }
        }
    }

    /// Parse one line of the streaming response into a token / done / none.
    /// Claude & OpenAI use `data: <json>` SSE framing; Ollama streams raw
    /// NDJSON (one JSON object per line).
    fn token_from_line(&self, line: &str) -> LineToken {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return LineToken::None;
        }
        match self {
            Self::Claude => {
                let Some(json_str) = trimmed.strip_prefix("data: ") else {
                    return LineToken::None;
                };
                let Ok(v) = serde_json::from_str::<Value>(json_str) else {
                    return LineToken::None;
                };
                match v.get("type").and_then(|t| t.as_str()) {
                    Some("content_block_delta") => {
                        LineToken::Token(v["delta"]["text"].as_str().unwrap_or("").to_string())
                    }
                    Some("message_stop") => LineToken::Done,
                    _ => LineToken::None,
                }
            }
            Self::OpenAi => {
                let Some(json_str) = trimmed.strip_prefix("data: ") else {
                    return LineToken::None;
                };
                if json_str.trim() == "[DONE]" {
                    return LineToken::Done;
                }
                let Ok(v) = serde_json::from_str::<Value>(json_str) else {
                    return LineToken::None;
                };
                match v["choices"][0]["delta"]["content"].as_str() {
                    None | Some("") => LineToken::None,
                    Some(s) => LineToken::Token(s.to_string()),
                }
            }
            Self::Ollama => {
                let Ok(v) = serde_json::from_str::<Value>(trimmed) else {
                    return LineToken::None;
                };
                if v.get("done").and_then(|d| d.as_bool()).unwrap_or(false) {
                    return LineToken::Done;
                }
                match v["message"]["content"].as_str() {
                    None | Some("") => LineToken::None,
                    Some(s) => LineToken::Token(s.to_string()),
                }
            }
        }
    }
}

enum LineToken {
    Token(String),
    Done,
    None,
}

// ───────────────────────────── settings + system prompt ─────────────────────────────

/// Read `ai_settings` (single row, id=1) and decrypt the API key with the
/// session DEK. Friendly errors if the vault is locked or no key is set.
fn load_settings(state: &State<'_, AppState>) -> Result<LoadedSettings, String> {
    let dek = state
        .dek
        .lock()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Vault 未解锁，请先解锁主密码库后再使用 AI".to_string())?;

    let (provider_id, model, base_url, api_key_enc, proxy_url, temperature) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        match db.query_row(
            "SELECT provider, model, base_url, api_key_enc, proxy_url, temperature FROM ai_settings WHERE id = 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, f64>(5)?,
                ))
            },
        ) {
            Ok(row) => row,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                ("claude".to_string(), None, None, None, None, 0.7)
            }
            Err(e) => return Err(e.to_string()),
        }
    };

    let provider = Provider::parse(&provider_id)?;
    let model = model
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| provider.default_model().to_string());

    let api_key = match api_key_enc.filter(|s| !s.trim().is_empty()) {
        Some(enc) => {
            let bytes = crypto::decrypt_with_key(&dek, &enc)?;
            String::from_utf8(bytes).map_err(|e| format!("API key 解码失败: {}", e))?
        }
        None => return Err("未配置 API key，请在「设置 → AI 助手」填写".to_string()),
    };

    Ok(LoadedSettings {
        provider,
        model,
        base_url,
        api_key,
        proxy_url,
        temperature,
    })
}

fn default_system(ctx: &Option<AiContext>) -> String {
    let c = ctx.as_ref();
    let shell = c
        .and_then(|x| x.shell_hint.clone())
        .unwrap_or_else(|| "bash".to_string());
    let conn_type = c.and_then(|x| x.conn_type.as_deref()).unwrap_or("ssh");
    let env_desc = match conn_type {
        "local" => format!("用户的本机终端（非远程服务器），shell 是 {shell}"),
        _ => format!(
            "一台通过 SSH 连接的远程服务器，shell 大概率是 {shell}；具体发行版 / OS 未知，\
如需精确判断可建议用户运行 uname -a 与 cat /etc/os-release"
        ),
    };
    format!(
        "你是 MyShell 内置的 AI 终端助手，辅助用户在终端中生成、解释、排查 shell 命令与服务器运维问题。\n\n\
## 当前环境\n\
- 运行场景：{env_desc}。\n\
- 你是「辅助终端的 AI」——用户在真实终端里操作，你提供命令与建议；这台终端可能存在网络问题（SSH 连接不稳定、代理、防火墙、DNS 解析失败），所以生成涉及网络的命令时请考虑加超时 / 重试 / 国内镜像源，必要时提示用户先排查连通性。\n\
- 你会收到用户在终端的选区或最近输出作为上下文；如果上下文为空，说明用户只是提问、未附带终端内容。\n\n\
## 安全红线（必须遵守）\n\
- 你不会自动执行任何命令——生成的命令一律由用户审查后手动执行，但你仍要把好第一道关。\n\
- 绝不主动生成破坏性命令。以下高危操作必须在回复中用 ⚠️ 显著警示并说明后果，能让用户确认再确认：rm -rf（尤其针对 / 、~ 、未逐一确认的目录）、dd、mkfs、fdisk、chmod -R 777、chown -R 批量改属主、用重定向覆盖系统文件（写入 /etc、/boot、/usr 等）、fork 炸弹、shutdown / reboot / halt、DROP DATABASE / DROP TABLE、git push --force 到共享分支、systemctl disable 或 stop 关键服务。\n\
- 涉及 sudo / root 的操作要提示权限影响；优先给非破坏性、可回滚的替代方案（先备份、加 --dry-run、限定具体路径而非通配）。\n\
- 拿不准风险时，先解释清楚再给命令，而不是直接给命令。\n\n\
## 输出规范\n\
- 生成的命令用代码块包裹；必要时附简短说明（做什么、关键参数、风险）。\n\
- 回复用中文，简洁。"
    )
}

/// Merge the base system prompt with live terminal context. Falls back to a
/// sensible default so the model always knows its role + the user's shell.
fn build_system(base: Option<String>, ctx: &Option<AiContext>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(base.unwrap_or_else(|| default_system(ctx)));
    if let Some(c) = ctx {
        if let Some(s) = &c.selection {
            parts.push(format!("# 用户在终端选中的内容\n```\n{s}\n```"));
        }
        if let Some(o) = &c.terminal_output {
            parts.push(format!("# 终端最近输出\n```\n{o}\n```"));
        }
        if let Some(d) = &c.inspect_data {
            parts.push(format!("# 采集到的服务器指标\n```\n{d}\n```"));
        }
    }
    Some(parts.join("\n\n"))
}

// ───────────────────────────── streaming core ─────────────────────────────

/// Resolve provider/key, POST with `stream:true`, parse SSE/NDJSON
/// line-by-line, emit `ai_token` per token. Any error is emitted as
/// `ai_error`; the caller always gets a terminal event (`ai_done` or
/// `ai_error`), never a hanging stream.
pub async fn chat_stream(
    state: &State<'_, AppState>,
    window: &WebviewWindow,
    params: AiChatParams,
) -> Result<(), String> {
    let req_id = params.request_id.clone();
    if let Err(e) = run_chat_stream(state, window, params).await {
        emit_error(window, &req_id, e);
    }
    Ok(())
}

async fn run_chat_stream(
    state: &State<'_, AppState>,
    window: &WebviewWindow,
    params: AiChatParams,
) -> Result<(), String> {
    let s = load_settings(state)?;
    let system = build_system(params.system, &params.context);

    let body = s
        .provider
        .build_body(&s.model, &params.messages, &system, s.temperature);
    let endpoint = s.provider.endpoint(&s.base_url);

    // Optional network proxy (http/https/socks5/socks5h; auth may be embedded
    // as user:pass@host). Lets users reach Claude/OpenAI behind a corporate or
    // regional proxy. Ollama (local) typically leaves this blank.
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(120));
    if let Some(proxy_url) = s.proxy_url.as_deref().filter(|u| !u.trim().is_empty()) {
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|e| format!("代理配置无效 ({}): {}", proxy_url, e))?;
        builder = builder.proxy(proxy);
    }
    let client = builder
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let mut req = client.post(&endpoint).json(&body);
    for (name, value) in s.provider.auth_headers(&s.api_key) {
        req = req.header(name, value.as_str());
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("请求 AI 接口失败: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("AI 接口返回 {}：{}", status, truncate(&text, 500)));
    }

    let req_id = params.request_id;
    let mut stream = resp.bytes_stream();
    // Accumulate raw bytes and split on `\n` so a multi-byte UTF-8 token
    // split across chunks isn't corrupted by lossy decoding mid-character.
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("读取流失败: {}", e))?;
        buf.extend_from_slice(&bytes);
        while let Some(nl) = buf.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = buf.drain(..=nl).collect();
            let line = String::from_utf8_lossy(&line_bytes);
            match s.provider.token_from_line(&line) {
                LineToken::Token(tok) => {
                    let _ = window.emit(
                        "ai_token",
                        AiTokenPayload {
                            request_id: req_id.clone(),
                            token: tok,
                        },
                    );
                }
                LineToken::Done => {
                    let _ = window.emit(
                        "ai_done",
                        AiDonePayload {
                            request_id: req_id.clone(),
                        },
                    );
                    return Ok(());
                }
                LineToken::None => {}
            }
        }
    }
    // Stream ended without an explicit Done marker — still signal completion.
    let _ = window.emit(
        "ai_done",
        AiDonePayload {
            request_id: req_id,
        },
    );
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

// ───────────────────────────── health inspection ─────────────────────────────

const INSPECT_SYSTEM: &str = "你是一名经验丰富的运维工程师。下面是通过只读命令采集到的\
服务器运行指标。请输出一份简洁的 Markdown 健康报告：\
1) 总体状态（🟢 正常 / 🟡 警告 / 🔴 异常）；\
2) 逐项分析（系统信息、负载/运行时间、内存、磁盘、进程、端口、错误日志），标出异常或接近阈值的项；\
3) 可操作的优化或排查建议。异常项用 ⚠️ 加粗。回复用中文。";

const LINUX_INSPECT_SCRIPT: &str = r#"echo "=== 系统信息 ==="; uname -a; (. /etc/os-release 2>/dev/null && echo "$PRETTY_NAME") || true
echo "=== 运行时间 / 负载 ==="; uptime
echo "=== CPU 核数 ==="; nproc 2>/dev/null || grep -c '^processor' /proc/cpuinfo
echo "=== 内存 ==="; free -h
echo "=== 磁盘使用 ==="; df -h 2>/dev/null | grep -vE '^tmpfs|^devtmpfs|^overlay'
echo "=== Top 进程 (CPU) ==="; ps -eo pid,user,%cpu,%mem,comm --sort=-%cpu 2>/dev/null | head -11
echo "=== 监听端口 ==="; (ss -tulnp 2>/dev/null || netstat -tulnp 2>/dev/null) | head -20
echo "=== 最近错误日志 ==="; (journalctl -p err -n 15 --no-pager 2>/dev/null || dmesg 2>/dev/null | tail -15)"#;

#[cfg(windows)]
const LOCAL_INSPECT_SCRIPT: &str = r#"$PSVersionTable
Write-Output "=== 系统信息 ==="
$os = Get-CimInstance Win32_OperatingSystem; "$($os.Caption) $($os.Version)"
Write-Output "=== 开机时间 ==="; $os.LastBootUpTime
Write-Output "=== 内存 ==="; "$([math]::Round($os.TotalVisibleMemorySize/1MB,1)) GB 总 / $([math]::Round($os.FreePhysicalMemory/1MB,1)) GB 空闲"
Write-Output "=== 磁盘使用 ==="; Get-PSDrive -PSProvider FileSystem | ForEach-Object { "$($_.Name): $([math]::Round($_.Used/1GB,1))/$([math]::Round(($_.Used+$_.Free)/1GB,1)) GB" }
Write-Output "=== Top 进程 (CPU) ==="; Get-Process | Sort-Object CPU -Descending | Select-Object -First 10 Id,ProcessName,@{N='CPU(s)';E={[math]::Round($_.CPU,1)}},@{N='Mem(MB)';E={[math]::Round($_.WorkingSet/1MB,1)}}
Write-Output "=== 运行中的服务 (前 20) ==="; Get-Service | Where-Object {$_.Status -eq 'Running'} | Select-Object -First 20 Name"#;

/// SSH/Linux inspection: runs the read-only script over the open SSH session
/// (reuses `ssh::exec_once`, same path as `ssh_get_server_info`), then
/// streams an AI health report.
pub async fn inspect_health_ssh(
    state: &State<'_, AppState>,
    window: &WebviewWindow,
    session_id: &str,
    request_id: &str,
) -> Result<(), String> {
    let raw = match tokio::time::timeout(
        std::time::Duration::from_secs(20),
        ssh::exec_once(state, session_id, LINUX_INSPECT_SCRIPT),
    )
    .await
    {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            // ConnectFailed / channel-open errors mean the underlying SSH
            // connection is gone (the tab may still look live because the
            // terminal's main channel is buffered). Point the user at the
            // real fix: reconnect the tab.
            let msg = if e.contains("ConnectFailed")
                || e.contains("open channel")
                || e.contains("disconnect")
                || e.contains("Channel")
            {
                format!(
                    "无法在当前 SSH 会话上执行巡检命令——连接可能已断开或不稳定。\n\
                     请关闭该 tab 重新连接服务器后再试（终端看似连着，但底层连接已失效）。\n\
                     （底层：{}）",
                    e
                )
            } else {
                format!("执行巡检命令失败: {}", e)
            };
            emit_error(window, request_id, msg);
            return Ok(());
        }
        Err(_) => {
            emit_error(window, request_id, "巡检命令执行超时");
            return Ok(());
        }
    };

    let messages = vec![ChatMessage {
        role: "user".into(),
        content: format!(
            "以下是采集到的服务器指标，请输出健康报告：\n\n```\n{}\n```",
            raw
        ),
    }];
    chat_stream(
        state,
        window,
        AiChatParams {
            request_id: request_id.to_string(),
            messages,
            system: Some(INSPECT_SYSTEM.to_string()),
            context: None,
        },
    )
    .await
}

/// Local inspection (Windows uses PowerShell, other OSes sh): runs the
/// read-only script via the OS shell — no PTY session needed, it's the
/// user's own machine — then streams an AI health report.
pub async fn inspect_health_local(
    state: &State<'_, AppState>,
    window: &WebviewWindow,
    request_id: &str,
) -> Result<(), String> {
    let raw = tokio::task::spawn_blocking(run_local_script)
        .await
        .map_err(|e| format!("巡检任务失败: {}", e))?;

    let raw = match raw {
        Ok(s) => s,
        Err(e) => {
            emit_error(window, request_id, e);
            return Ok(());
        }
    };

    let messages = vec![ChatMessage {
        role: "user".into(),
        content: format!(
            "以下是采集到的本机指标，请输出健康报告：\n\n```\n{}\n```",
            raw
        ),
    }];
    chat_stream(
        state,
        window,
        AiChatParams {
            request_id: request_id.to_string(),
            messages,
            system: Some(INSPECT_SYSTEM.to_string()),
            context: None,
        },
    )
    .await
}

#[cfg(windows)]
fn run_local_script() -> Result<String, String> {
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", LOCAL_INSPECT_SCRIPT])
        .output()
        .map_err(|e| format!("执行 PowerShell 失败: {}", e))?;
    let mut s = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.stderr.is_empty() {
        s.push_str("\n[stderr]\n");
        s.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    Ok(s)
}

#[cfg(not(windows))]
fn run_local_script() -> Result<String, String> {
    let out = std::process::Command::new("sh")
        .args(["-c", LINUX_INSPECT_SCRIPT])
        .output()
        .map_err(|e| format!("执行 sh 失败: {}", e))?;
    let mut s = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.stderr.is_empty() {
        s.push_str("\n[stderr]\n");
        s.push_str(&String::from_utf8_lossy(&out.stderr));
    }
    Ok(s)
}
