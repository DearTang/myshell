//! AI assistant — multi-provider chat (Claude / OpenAI / Ollama) with
//! streaming token output to the frontend, plus server health inspection.
//!
//! Streaming reuses the same `sink.emit` pattern as `ssh_output`: a command
//! wrapper calls [`chat_stream`], which resolves the provider config + key
//! from the vault-backed `ai_settings` table, fires an HTTP request, and
//! emits `ai_token` / `ai_done` / `ai_error` events via the `EventSink`
//! as SSE/NDJSON chunks arrive. Health inspection runs a preset
//! read-only script (SSH via `ssh::exec_once`, local via `std::process`) and
//! feeds the collected metrics into the same streaming path.
//!
//! Why an enum, not a trait object: the three providers differ only in
//! endpoint / auth headers / request-body shape / line-parsing, all of which
//! are synchronous and cheap. An enum + `match` avoids the async-trait /
//! `Pin<Box<dyn Future>>` machinery and keeps everything concrete.

use crate::{crypto, ssh, AppState};
use crate::{EventSink, EventSinkExt};
use futures_util::StreamExt;
use serde_json::{json, Value};

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

fn emit_error(sink: &dyn EventSink, request_id: &str, error: impl Into<String>) {
    let msg: String = error.into();
    // Every AI failure path (chat stream, health inspect, test) funnels
    // through here, so log once to capture the provider/HTTP reason for
    // post-mortem when a user reports "AI 不工作".
    log::warn!("[ai:{}] error: {}", request_id, msg);
    sink.emit(
        "ai_error",
        &AiErrorPayload {
            request_id: request_id.to_string(),
            error: msg,
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

/// Bundle the frontend's `ai_chat` args. Built by the command wrapper
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

#[derive(Clone, Copy, Debug)]
pub enum Provider {
    Claude,
    OpenAi,
    Ollama,
    /// OpenAI-compatible endpoint (GLM, MIMO, MiniMax, LongCat, DeepSeek,
    /// 通义千问, 混元, etc.). Uses the OpenAI request/response format but
    /// with a user-supplied base URL.
    OpenAiCompatible,
    /// Anthropic-compatible endpoint. Uses the Claude request/response format
    /// but with a user-supplied base URL.
    AnthropicCompatible,
}

impl Provider {
    pub fn parse(id: &str) -> Result<Self, String> {
        match id {
            "claude" => Ok(Self::Claude),
            "openai" => Ok(Self::OpenAi),
            "ollama" => Ok(Self::Ollama),
            "openai_compatible" => Ok(Self::OpenAiCompatible),
            "anthropic_compatible" => Ok(Self::AnthropicCompatible),
            other => Err(format!(
                "未知 AI 提供商: {}（支持 claude / openai / ollama / openai_compatible / anthropic_compatible）",
                other
            )),
        }
    }

    fn default_model(&self) -> &'static str {
        match self {
            Self::Claude => "claude-sonnet-4-6",
            Self::OpenAi => "gpt-4o",
            Self::Ollama => "llama3.1",
            Self::OpenAiCompatible => "default",
            Self::AnthropicCompatible => "default",
        }
    }

    /// Is this provider an OpenAI-protocol variant (including compatible)?
    fn is_openai_protocol(&self) -> bool {
        matches!(self, Self::OpenAi | Self::OpenAiCompatible | Self::Ollama)
    }

    /// Is this provider an Anthropic-protocol variant (including compatible)?
    fn is_anthropic_protocol(&self) -> bool {
        matches!(self, Self::Claude | Self::AnthropicCompatible)
    }

    /// String ID used in DB and IPC.
    pub fn id_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::OpenAi => "openai",
            Self::Ollama => "ollama",
            Self::OpenAiCompatible => "openai_compatible",
            Self::AnthropicCompatible => "anthropic_compatible",
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
            Self::Claude | Self::AnthropicCompatible => "messages",
            Self::OpenAi | Self::OpenAiCompatible => "chat/completions",
            Self::Ollama => "chat",
        };
        let base = base_url
            .as_deref()
            .map(|s| s.trim_end_matches('/'))
            .unwrap_or_else(|| match self {
                Self::Claude => "https://api.anthropic.com/v1",
                Self::OpenAi => "https://api.openai.com/v1",
                Self::Ollama => "http://localhost:11434/api",
                // Compatible providers MUST have a base_url — no sensible default.
                Self::OpenAiCompatible | Self::AnthropicCompatible => {
                    panic!("OpenAiCompatible/AnthropicCompatible require a base_url")
                }
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
            Self::Claude | Self::AnthropicCompatible => vec![
                ("x-api-key", api_key.to_string()),
                ("anthropic-version", "2023-06-01".to_string()),
            ],
            Self::OpenAi | Self::OpenAiCompatible => {
                vec![("Authorization", format!("Bearer {}", api_key))]
            }
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
        if self.is_anthropic_protocol() {
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
        } else if matches!(self, Self::Ollama) {
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
        } else {
            // OpenAi + OpenAiCompatible
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
    }

    /// Parse one line of the streaming response into a token / done / none.
    /// Claude & OpenAI use `data: <json>` SSE framing; Ollama streams raw
    /// NDJSON (one JSON object per line).
    fn token_from_line(&self, line: &str) -> LineToken {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return LineToken::None;
        }
        if self.is_anthropic_protocol() {
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
        } else if matches!(self, Self::Ollama) {
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
        } else {
            // OpenAi + OpenAiCompatible
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
    }
}

enum LineToken {
    Token(String),
    Done,
    None,
}

// ───────────────────────────── settings + system prompt ─────────────────────────────

/// Read the active AI model config and decrypt the API key with the session
/// DEK. Prefers the multi-model `ai_models` table (via `active_model_id`);
/// falls back to the legacy single-row `ai_settings` for backward compat.
fn load_settings(state: &AppState) -> Result<LoadedSettings, String> {
    let dek = state
        .dek
        .lock()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Vault 未解锁，请先解锁主密码库后再使用 AI".to_string())?;

    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Try multi-model path first: ai_settings.active_model_id → ai_models row.
    let active_id: Option<i64> = db
        .query_row(
            "SELECT active_model_id FROM ai_settings WHERE id = 1",
            [],
            |r| r.get::<_, Option<i64>>(0),
        )
        .unwrap_or(None);

    if let Some(model_id) = active_id {
        if let Ok(row) = db.query_row(
            "SELECT provider, model_id, base_url, api_key_enc, proxy_url, temperature \
             FROM ai_models WHERE id = ?1",
            [model_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, f64>(5)?,
                ))
            },
        ) {
            let (provider_id, primary_model, base_url, api_key_enc, proxy_url, temperature) = row;
            let provider = Provider::parse(&provider_id)?;
            let api_key = decrypt_key(&dek, api_key_enc.as_deref())?;
            // Resolve the specific model: active_model_string overrides the
            // supplier's primary model_id.
            let model = db
                .query_row(
                    "SELECT active_model_string FROM ai_settings WHERE id = 1",
                    [],
                    |r| r.get::<_, Option<String>>(0),
                )
                .ok()
                .flatten()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(primary_model);
            return Ok(LoadedSettings {
                provider,
                model,
                base_url,
                api_key,
                proxy_url,
                temperature,
            });
        }
    }

    // Fallback: legacy single-row ai_settings.
    let (provider_id, model, base_url, api_key_enc, proxy_url, temperature) = match db.query_row(
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
    };

    drop(db);

    let provider = Provider::parse(&provider_id)?;
    let model = model
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| provider.default_model().to_string());

    let api_key = decrypt_key(&dek, api_key_enc.as_deref())?;

    Ok(LoadedSettings {
        provider,
        model,
        base_url,
        api_key,
        proxy_url,
        temperature,
    })
}

fn decrypt_key(dek: &[u8; 32], api_key_enc: Option<&str>) -> Result<String, String> {
    match api_key_enc.filter(|s| !s.trim().is_empty()) {
        Some(enc) => {
            let bytes = crypto::decrypt_with_key(dek, enc)?;
            String::from_utf8(bytes).map_err(|e| format!("API key 解码失败: {}", e))
        }
        None => Err("未配置 API key，请在「设置 → AI 助手」填写".to_string()),
    }
}

/// Load settings for a specific supplier by id (not necessarily the active one),
/// with an optional override API key. Used by `test_settings` so the user can
/// test the supplier they're editing rather than the globally-active one.
fn load_settings_for_supplier(
    state: &AppState,
    supplier_id: i64,
    override_key: Option<&str>,
) -> Result<LoadedSettings, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    let (provider_id, primary_model, base_url, api_key_enc, proxy_url, temperature) = db
        .query_row(
            "SELECT provider, model_id, base_url, api_key_enc, proxy_url, temperature \
             FROM ai_models WHERE id = ?1",
            [supplier_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, f64>(5)?,
                ))
            },
        )
        .map_err(|e| format!("供应商不存在: {e}"))?;

    let provider = Provider::parse(&provider_id)?;

    // Resolve API key: override > vault-stored > error.
    let api_key = if let Some(key) = override_key.filter(|k| !k.is_empty()) {
        key.to_string()
    } else {
        let dek = state
            .dek
            .lock()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Vault 未解锁".to_string())?;
        decrypt_key(&dek, api_key_enc.as_deref())?
    };

    Ok(LoadedSettings {
        provider,
        model: primary_model,
        base_url,
        api_key,
        proxy_url,
        temperature,
    })
}

/// Same as `load_settings` but uses an externally supplied plaintext API key
/// instead of decrypting the vault-stored one. Lets test_settings honor an
/// override key even when the active supplier has no key saved in the vault
/// (e.g. create / edit-then-test before first save).
fn load_settings_with_key(
    state: &AppState,
    override_key: &str,
) -> Result<LoadedSettings, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Try multi-model path first.
    let active_id: Option<i64> = db
        .query_row(
            "SELECT active_model_id FROM ai_settings WHERE id = 1",
            [],
            |r| r.get::<_, Option<i64>>(0),
        )
        .unwrap_or(None);

    if let Some(model_id) = active_id {
        if let Ok(row) = db.query_row(
            "SELECT provider, model_id, base_url, proxy_url, temperature \
             FROM ai_models WHERE id = ?1",
            [model_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, f64>(4)?,
                ))
            },
        ) {
            let (provider_id, primary_model, base_url, proxy_url, temperature) = row;
            let provider = Provider::parse(&provider_id)?;
            let model = db
                .query_row(
                    "SELECT active_model_string FROM ai_settings WHERE id = 1",
                    [],
                    |r| r.get::<_, Option<String>>(0),
                )
                .ok()
                .flatten()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(primary_model);
            return Ok(LoadedSettings {
                provider,
                model,
                base_url,
                api_key: override_key.to_string(),
                proxy_url,
                temperature,
            });
        }
    }

    // Fallback: legacy single-row ai_settings.
    let (provider_id, model, base_url, proxy_url, temperature) = match db.query_row(
        "SELECT provider, model, base_url, proxy_url, temperature FROM ai_settings WHERE id = 1",
        [],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, f64>(4)?,
            ))
        },
    ) {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            ("claude".to_string(), None, None, None, 0.7)
        }
        Err(e) => return Err(e.to_string()),
    };

    let provider = Provider::parse(&provider_id)?;
    let model = model
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| provider.default_model().to_string());

    Ok(LoadedSettings {
        provider,
        model,
        base_url,
        api_key: override_key.to_string(),
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
    state: &AppState,
    sink: &dyn EventSink,
    params: AiChatParams,
) -> Result<(), String> {
    let req_id = params.request_id.clone();
    if let Err(e) = run_chat_stream(state, sink, params).await {
        emit_error(sink, &req_id, e);
    }
    Ok(())
}

async fn run_chat_stream(
    state: &AppState,
    sink: &dyn EventSink,
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
                    sink.emit(
                        "ai_token",
                        &AiTokenPayload {
                            request_id: req_id.clone(),
                            token: tok,
                        },
                    );
                }
                LineToken::Done => {
                    sink.emit(
                        "ai_done",
                        &AiDonePayload {
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
    sink.emit(
        "ai_done",
        &AiDonePayload {
            request_id: req_id,
        },
    );
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    // Step back to the nearest UTF-8 char boundary at or below `max`.
    // Slicing at an arbitrary byte index (`&s[..max]`) panics when `max`
    // lands inside a multi-byte character — common for CJK error bodies at
    // max=500.
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

// ───────────────────────────── settings test ─────────────────────────────

/// Form-level overrides for testing AI settings WITHOUT saving first. Any
/// field that is None or empty-string falls back to the vault-stored value
/// (via `load_settings`). `api_key` is the important one: the Settings form
/// passes the unsaved typed key here so the user can validate it before
/// committing; an empty string means "no override, use the vault key" — the
/// same convention as `save_ai_settings`'s "empty key = keep existing".
#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiTestOverrides {
    pub supplier_id: Option<i64>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub proxy_url: Option<String>,
    pub api_key: Option<String>,
    pub temperature: Option<f64>,
}

/// Probe the AI config with a minimal non-streaming request. Used by the
/// Settings panel's "测试" button. Reuses `load_settings` (vault config +
/// decrypted key) and `Provider` (endpoint / auth headers / body shape).
///
/// Override semantics: a non-empty override field replaces the vault value;
/// an empty/None field keeps the vault value. `api_key = Some("")` is treated
/// as "no override" (matches the save flow's "empty key = keep existing").
///
/// Non-streaming (`stream:false`, `max_tokens:16`) on purpose: a single HTTP
/// round-trip gives clean success/fail semantics, and crucially this never
/// emits `ai_token`/`ai_done` events — so it can't interfere with an open
/// AiPanel's streaming subscriptions.
pub async fn test_settings(
    state: &AppState,
    overrides: AiTestOverrides,
) -> Result<String, String> {
    // Load the specific supplier being tested (or fall back to active one).
    // Then overlay non-empty overrides — including an explicit API key, which
    // lets the user test a key before saving it to the vault.
    let mut s = if let Some(sid) = overrides.supplier_id {
        load_settings_for_supplier(state, sid, overrides.api_key.as_deref())?
    } else if overrides.api_key.as_deref().filter(|k| !k.is_empty()).is_some() {
        load_settings_with_key(state, overrides.api_key.as_deref().unwrap_or_default())?
    } else {
        load_settings(state)?
    };

    // Apply remaining overrides (empty strings are ignored → fall back to loaded value).
    if let Some(p) = overrides.provider.as_deref().filter(|p| !p.is_empty()) {
        s.provider = Provider::parse(p)?;
    }
    if let Some(m) = overrides.model.as_deref().filter(|m| !m.is_empty()) {
        s.model = m.to_string();
    }
    if let Some(b) = overrides.base_url.as_deref().filter(|b| !b.is_empty()) {
        s.base_url = Some(b.to_string());
    }
    if let Some(p) = overrides.proxy_url.as_deref().filter(|p| !p.is_empty()) {
        s.proxy_url = Some(p.to_string());
    }
    if let Some(t) = overrides.temperature {
        s.temperature = t;
    }

    // Minimal non-streaming body. `build_body` always sets stream:true, so we
    // flip it to false afterwards and cap max_tokens to keep the probe cheap.
    let mut body = s.provider.build_body(
        &s.model,
        &[ChatMessage {
            role: "user".into(),
            content: "ping".into(),
        }],
        &Some("Reply with the single word: ok".into()),
        s.temperature,
    );
    if let Some(obj) = body.as_object_mut() {
        obj.insert("stream".to_string(), serde_json::Value::Bool(false));
        // Claude requires max_tokens; OpenAI/Ollama accept it. Overwriting the
        // 4096 the Claude branch set keeps every provider to a tiny reply.
        obj.insert("max_tokens".to_string(), serde_json::json!(16));
    }

    let endpoint = s.provider.endpoint(&s.base_url);

    // reqwest client + optional proxy — same builder as run_chat_stream.
    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30));
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
        .map_err(|e| {
            log::warn!("[ai] test request failed ({:?}): {}", s.provider, e);
            format!("请求 AI 接口失败: {}", e)
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        log::warn!("[ai] test returned {} ({:?}): {}", status, s.provider, truncate(&text, 200));
        return Err(format!("AI 接口返回 {}：{}", status, truncate(&text, 500)));
    }

    // 200 — auth + endpoint are validated by reaching here. Best-effort: try
    // to surface a short reply snippet; if the body shape is unexpected, the
    // success message just omits it.
    let body_text = resp.text().await.unwrap_or_default();
    let snippet = extract_reply_snippet(&s.provider, &body_text);
    Ok(format!(
        "✓ 测试成功（provider={}，model={}）{}",
        s.provider.id_str(),
        s.model,
        snippet
    ))
}

/// Best-effort extraction of a short reply snippet from a non-streaming
/// response body. Returns "" on any parse failure or empty reply — the caller
/// treats a 200 as success regardless, so this is only for a richer message.
fn extract_reply_snippet(provider: &Provider, body: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return String::new();
    };
    let text = if provider.is_anthropic_protocol() {
        v["content"][0]["text"].as_str()
    } else if matches!(provider, Provider::Ollama) {
        v["message"]["content"].as_str()
    } else {
        // OpenAi + OpenAiCompatible
        v["choices"][0]["message"]["content"].as_str()
    };
    match text.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => format!("，回复：{}", truncate(t, 40)),
        None => String::new(),
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
    state: &AppState,
    sink: &dyn EventSink,
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
            emit_error(sink, request_id, msg);
            return Ok(());
        }
        Err(_) => {
            emit_error(sink, request_id, "巡检命令执行超时");
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
        sink,
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
    state: &AppState,
    sink: &dyn EventSink,
    request_id: &str,
) -> Result<(), String> {
    // Match inspect_health_ssh's 20s cap. Without it, a hung PowerShell
    // (WMI/CIM provider stuck) blocks spawn_blocking forever and the UI
    // shows "inspecting..." indefinitely.
    let raw = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        tokio::task::spawn_blocking(run_local_script),
    )
    .await
    .map_err(|_| "巡检命令执行超时".to_string())?
    .map_err(|e| format!("巡检任务失败: {}", e))?;

    let raw = match raw {
        Ok(s) => s,
        Err(e) => {
            emit_error(sink, request_id, e);
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
        sink,
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

// ───────────────────────────── multi-model management ─────────────────────────────

/// A single model belonging to a supplier (no secrets — just id + label).
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SupplierModelInfo {
    pub id: i64,
    pub supplier_id: i64,
    pub model_id: String,
    pub label: Option<String>,
    pub sort_order: i32,
}

/// Model info returned to the frontend (no api_key — only `has_key` flag).
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AiModelInfo {
    pub id: i64,
    pub name: String,
    pub provider: String,
    pub model_id: String,
    pub base_url: Option<String>,
    pub has_key: bool,
    pub proxy_url: Option<String>,
    pub temperature: f64,
    pub is_preset: bool,
    pub is_enabled: bool,
    pub sort_order: i32,
    /// Full model list for this supplier (includes the primary model_id).
    pub models: Vec<SupplierModelInfo>,
}

/// List all configured AI models (presets + user-created). Does NOT return
/// api_key — only a `has_key` boolean so the UI can prompt for one.
/// Each returned AiModelInfo includes its full model list (`models`).
pub fn list_ai_models_cmd(state: &AppState) -> Result<Vec<AiModelInfo>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare(
            "SELECT id, name, provider, model_id, base_url, api_key_enc, proxy_url, \
             temperature, is_preset, is_enabled, sort_order FROM ai_models ORDER BY sort_order, id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(AiModelInfo {
                id: r.get(0)?,
                name: r.get(1)?,
                provider: r.get(2)?,
                model_id: r.get(3)?,
                base_url: r.get(4)?,
                has_key: r
                    .get::<_, Option<String>>(5)?
                    .filter(|s| !s.is_empty())
                    .is_some(),
                proxy_url: r.get(6)?,
                temperature: r.get(7)?,
                is_preset: r.get::<_, i32>(8)? != 0,
                is_enabled: r.get::<_, i32>(9)? != 0,
                sort_order: r.get(10)?,
                models: Vec::new(), // filled in below
            })
        })
        .map_err(|e| e.to_string())?;
    let mut suppliers: Vec<AiModelInfo> = rows.filter_map(|r| r.ok()).collect();

    // Populate each supplier's model list from ai_supplier_models.
    for s in suppliers.iter_mut() {
        s.models = load_supplier_models(&db, s.id)?;
        // Ensure the primary model_id is always present as the first entry.
        let has_primary = s.models.iter().any(|m| m.model_id == s.model_id);
        if !has_primary {
            s.models.insert(
                0,
                SupplierModelInfo {
                    id: 0, // synthetic — not persisted
                    supplier_id: s.id,
                    model_id: s.model_id.clone(),
                    label: None,
                    sort_order: 0,
                },
            );
        }
    }
    Ok(suppliers)
}

/// Load the model list for a single supplier FROM ai_supplier_models.
/// Returns entries sorted by sort_order. The primary model_id (stored on
/// ai_models) is synthesized as id=0 by the caller if needed.
fn load_supplier_models(
    db: &rusqlite::Connection,
    supplier_id: i64,
) -> Result<Vec<SupplierModelInfo>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id, supplier_id, model_id, label, sort_order \
             FROM ai_supplier_models WHERE supplier_id = ?1 ORDER BY sort_order, id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([supplier_id], |r| {
            Ok(SupplierModelInfo {
                id: r.get(0)?,
                supplier_id: r.get(1)?,
                model_id: r.get(2)?,
                label: r.get(3)?,
                sort_order: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Get the currently active model id from ai_settings.
pub fn get_active_model_id(state: &AppState) -> Result<Option<i64>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.query_row(
        "SELECT active_model_id FROM ai_settings WHERE id = 1",
        [],
        |r| r.get::<_, Option<i64>>(0),
    )
    .or(Ok(None))
}

/// Save (create or update) an AI model. If `id` is Some, updates that row;
/// otherwise inserts a new row. `api_key` is encrypted before storage;
/// empty string means "keep existing key" on update.
///
/// `models` is the full list of supplier models to sync. When provided on
/// update, existing ai_supplier_models rows for this supplier are replaced
/// with the given list (the primary model_id is always kept as a fallback
/// even if not listed). On create, models are inserted after the supplier.
pub fn save_ai_model_cmd(
    state: &AppState,
    id: Option<i64>,
    name: String,
    provider: String,
    model_id: String,
    base_url: Option<String>,
    api_key: Option<String>,
    proxy_url: Option<String>,
    temperature: f64,
    models: Option<Vec<(String, Option<String>)>>, // (model_id, label)
) -> Result<i64, String> {
    let dek = state
        .dek
        .lock()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Vault 未解锁".to_string())?;

    let api_key_enc: Option<String> = match api_key.filter(|s| !s.is_empty()) {
        Some(key) => Some(crypto::encrypt_with_key(&dek, key.as_bytes())?),
        None => None,
    };

    let db = state.db.lock().map_err(|e| e.to_string())?;

    let row_id = if let Some(existing_id) = id {
        // Update existing row.
        if let Some(enc) = &api_key_enc {
            db.execute(
                "UPDATE ai_models SET name=?1, provider=?2, model_id=?3, base_url=?4, \
                 api_key_enc=?5, proxy_url=?6, temperature=?7 WHERE id=?8",
                rusqlite::params![name, provider, model_id, base_url, enc, proxy_url, temperature, existing_id],
            )
            .map_err(|e| e.to_string())?;
        } else {
            // Empty key = keep existing.
            db.execute(
                "UPDATE ai_models SET name=?1, provider=?2, model_id=?3, base_url=?4, \
                 proxy_url=?5, temperature=?6 WHERE id=?7",
                rusqlite::params![name, provider, model_id, base_url, proxy_url, temperature, existing_id],
            )
            .map_err(|e| e.to_string())?;
        }
        // Sync model list if provided.
        if let Some(model_list) = models {
            sync_supplier_models(&db, existing_id, model_list)?;
        }
        existing_id
    } else {
        // Insert new row.
        let enc = api_key_enc.unwrap_or_default();
        db.execute(
            "INSERT INTO ai_models (name, provider, model_id, base_url, api_key_enc, \
             proxy_url, temperature, is_preset, sort_order) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 100)",
            rusqlite::params![name, provider, model_id, base_url, enc, proxy_url, temperature],
        )
        .map_err(|e| e.to_string())?;
        let new_id = db.last_insert_rowid();
        // Insert model list if provided.
        if let Some(model_list) = models {
            sync_supplier_models(&db, new_id, model_list)?;
        }
        new_id
    };
    Ok(row_id)
}

/// Replace the full model list for a supplier. Deletes existing rows then
/// inserts the new list in order. The primary model_id (on ai_models) is
/// always preserved as a fallback by the reader, so it need not be in `models`.
fn sync_supplier_models(
    db: &rusqlite::Connection,
    supplier_id: i64,
    models: Vec<(String, Option<String>)>,
) -> Result<(), String> {
    db.execute(
        "DELETE FROM ai_supplier_models WHERE supplier_id = ?1",
        [supplier_id],
    )
    .map_err(|e| e.to_string())?;
    for (idx, (model_id, label)) in models.iter().enumerate() {
        db.execute(
            "INSERT INTO ai_supplier_models (supplier_id, model_id, label, sort_order) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![supplier_id, model_id, label, idx as i32],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Delete a user-created AI model. Presets (is_preset=1) cannot be deleted.
pub fn delete_ai_model_cmd(state: &AppState, id: i64) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let affected = db
        .execute(
            "DELETE FROM ai_models WHERE id = ?1 AND is_preset = 0",
            [id],
        )
        .map_err(|e| e.to_string())?;
    if affected == 0 {
        return Err("预设模型不可删除，或模型不存在".to_string());
    }
    // If the deleted model was the active one, clear the reference.
    let _ = db.execute(
        "UPDATE ai_settings SET active_model_id = NULL WHERE active_model_id = ?1",
        [id],
    );
    Ok(())
}

/// Switch the active AI model. The next `chat_stream` call will use this model.
/// `model_string` selects the specific model_id within the supplier (None =
/// fall back to the supplier's primary model_id).
pub fn set_active_ai_model_cmd(
    state: &AppState,
    id: i64,
    model_string: Option<String>,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    // Verify the model exists.
    let exists: bool = db
        .query_row(
            "SELECT COUNT(*) FROM ai_models WHERE id = ?1",
            [id],
            |r| r.get::<_, i64>(0),
        )
        .map_err(|e| e.to_string())?
        > 0;
    if !exists {
        return Err(format!("模型 ID {} 不存在", id));
    }
    // Ensure the settings row exists (should always be true after schema init).
    db.execute(
        "INSERT INTO ai_settings (id, active_model_id, active_model_string) VALUES (1, ?1, ?2) \
         ON CONFLICT(id) DO UPDATE SET active_model_id = ?1, active_model_string = ?2",
        rusqlite::params![id, model_string],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Supplier model management ─────────────────────────────────────────────

/// List all models for a given supplier (from ai_supplier_models).
pub fn list_supplier_models_cmd(
    state: &AppState,
    supplier_id: i64,
) -> Result<Vec<SupplierModelInfo>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    load_supplier_models(&db, supplier_id)
}

/// Add a model to a supplier. Returns the new row id.
pub fn add_supplier_model_cmd(
    state: &AppState,
    supplier_id: i64,
    model_id: String,
    label: Option<String>,
) -> Result<i64, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    // Compute next sort_order.
    let max_order: i64 = db
        .query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM ai_supplier_models WHERE supplier_id = ?1",
            [supplier_id],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0);
    db.execute(
        "INSERT INTO ai_supplier_models (supplier_id, model_id, label, sort_order) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![supplier_id, model_id, label, max_order],
    )
    .map_err(|e| e.to_string())?;
    Ok(db.last_insert_rowid())
}

/// Remove a single supplier model row by id.
pub fn remove_supplier_model_cmd(state: &AppState, id: i64) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute("DELETE FROM ai_supplier_models WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Toggle the enabled flag of a supplier. Disabled suppliers are hidden from
/// the AI chat model picker but kept in the settings list.
pub fn toggle_ai_model_enabled_cmd(
    state: &AppState,
    id: i64,
    enabled: bool,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute(
        "UPDATE ai_models SET is_enabled = ?1 WHERE id = ?2",
        rusqlite::params![if enabled { 1 } else { 0 }, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Built-in preset definitions. Inserted on first launch if the table is empty.
pub fn init_ai_presets_cmd(state: &AppState) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Only insert if no presets exist yet (idempotent).
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM ai_models WHERE is_preset = 1",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if count > 0 {
        return Ok(());
    }

    let presets: &[(&str, &str, &str, Option<&str>, i32)] = &[
        // (name, provider, model_id, base_url, sort_order)
        ("GLM Coding Plan (OpenAI)", "openai_compatible", "glm-4", Some("https://open.bigmodel.cn/api/coding/paas/v4"), 10),
        ("GLM Coding Plan (Anthropic)", "anthropic_compatible", "glm-4", Some("https://open.bigmodel.cn/api/anthropic"), 11),
        ("MIMO (OpenAI)", "openai_compatible", "mimo-default", Some("https://api.xiaomimimo.com/v1"), 20),
        ("MIMO (Anthropic)", "anthropic_compatible", "mimo-default", Some("https://api.xiaomimimo.com/anthropic"), 21),
        ("MiniMax M3 (OpenAI)", "openai_compatible", "MiniMax-M3", Some("https://api.minimaxi.com/v1"), 30),
        ("MiniMax M3 (Anthropic)", "anthropic_compatible", "MiniMax-M3", Some("https://api.minimaxi.com/anthropic"), 31),
        ("LongCat (OpenAI)", "openai_compatible", "longcat-default", Some("https://api.longcat.chat/openai"), 40),
        ("LongCat (Anthropic)", "anthropic_compatible", "longcat-default", Some("https://api.longcat.chat/anthropic"), 41),
        ("DeepSeek", "openai_compatible", "deepseek-chat", Some("https://api.deepseek.com/v1"), 50),
        ("通义千问 (阿里云)", "openai_compatible", "qwen-turbo", Some("https://dashscope.aliyuncs.com/compatible-mode/v1"), 60),
        ("混元 (腾讯云)", "openai_compatible", "hunyuan-lite", Some("https://api.hunyuan.cloud.tencent.com/v1"), 70),
        ("Claude", "claude", "claude-sonnet-4-6", None, 80),
        ("OpenAI", "openai", "gpt-4o", None, 90),
        ("Ollama (本地)", "ollama", "llama3.1", Some("http://localhost:11434/api"), 100),
    ];

    let mut stmt = db
        .prepare(
            "INSERT INTO ai_models (name, provider, model_id, base_url, is_preset, sort_order) \
             VALUES (?1, ?2, ?3, ?4, 1, ?5)",
        )
        .map_err(|e| e.to_string())?;
    for (name, provider, model_id, base_url, order) in presets {
        stmt.execute(rusqlite::params![name, provider, model_id, base_url, order])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Fetch available models from a provider's `/models` endpoint (OpenAI format).
/// Returns a list of model IDs sorted alphabetically. Works with any OpenAI-
/// compatible API (GLM, DeepSeek, 通义千问, LongCat, etc.).
///
/// For Anthropic-compatible providers we return an error since Anthropic has
/// no such endpoint — users must manually enter model IDs for those.
///
/// The returned IDs can be directly used as `model_id` when saving an
/// `ai_models` row — no more guessing model names.
pub async fn fetch_provider_models(
    provider_id: &str,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>, String> {
    let provider = Provider::parse(provider_id)?;

    // Anthropic protocol has no standardized /models endpoint.
    if matches!(provider, Provider::AnthropicCompatible) {
        return Err(
            "Anthropic 兼容协议暂无标准模型列表接口，请手动填写模型 ID".to_string()
        );
    }

    // Claude (official) hardcodes its model list — no /models endpoint either.
    if matches!(provider, Provider::Claude) {
        return Err(
            "Claude 官方 API 无 /models 接口，请手动填写模型 ID（如 claude-sonnet-4-6）"
                .to_string(),
        );
    }

    // Build the /models URL from base_url. Strip any trailing path after host
    // (e.g. "/v1/chat/completions" → "/v1") then append /models.
    let base = base_url.trim_end_matches('/');
    // Remove known suffixes like /chat/completions, /messages, /chat to get root.
    let root = base
        .trim_end_matches("/chat/completions")
        .trim_end_matches("/messages")
        .trim_end_matches("/chat");
    let models_url = format!("{}/models", root);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP 客户端构造失败: {e}"))?;

    let mut req = client.get(&models_url);
    // Auth header varies by provider.
    match provider {
        Provider::OpenAi | Provider::OpenAiCompatible => {
            req = req.bearer_auth(api_key);
        }
        Provider::Ollama => {
            // Ollama has no auth.
        }
        _ => unreachable!(),
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("请求模型列表失败: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!(
            "获取模型列表失败: HTTP {} — {}",
            status,
            truncate(&text, 200)
        ));
    }

    // Parse OpenAI-format response: { "data": [ { "id": "model-name", ... }, ... ] }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析模型列表响应失败: {e}"))?;

    let mut models: Vec<String> = json
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("id").and_then(|id| id.as_str()))
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    models.sort();
    models.dedup();
    Ok(models)
}

/// Fetch available models for a specific supplier by id. Decrypts the stored
/// API key server-side so the frontend never needs to handle plaintext keys
/// after a save. Falls back to an override key when the supplier has no
/// stored key yet (create / edit flow).
pub async fn fetch_models_for_supplier(
    state: &AppState,
    supplier_id: i64,
    override_key: Option<String>,
) -> Result<Vec<String>, String> {
    let s = load_settings_for_supplier(state, supplier_id, override_key.as_deref())?;
    fetch_provider_models(s.provider.id_str(), &s.base_url.unwrap_or_default(), &s.api_key).await
}
