//! Network proxy (SOCKS5 + HTTP CONNECT) handshake for SSH/FTP connections.
//!
//! Returns a tokio `TcpStream` already past the proxy handshake. Feed it to
//! `russh::client::connect_stream` (SSH) or `AsyncFtpStream::connect_with_stream`
//! (FTP) — both treat it as a direct connection to the target host:port.
//!
//! SOCKS5 uses tokio-socks (RFC 1928 + RFC 1929 user/pass auth). HTTP CONNECT
//! is implemented inline since the protocol is trivial (~30 lines).

use base64::Engine;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::ConnectionConfig;

/// 10-second hard cap on proxy handshake. A misbehaving proxy or wrong port
/// would otherwise hang the entire connect flow with no UI feedback.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Proxy configuration derived from a ConnectionConfig's `proxy_*` fields.
/// Caller is expected to have resolved `proxy_password` from the keyring
/// before constructing this — secrets handling lives in main.rs.
#[derive(Debug, Clone)]
pub enum ProxyConfig {
    Socks5 {
        host: String,
        port: u16,
        username: Option<String>,
        password: Option<String>,
    },
    HttpConnect {
        host: String,
        port: u16,
        username: Option<String>,
        password: Option<String>,
    },
}

impl ProxyConfig {
    /// Returns Ok(None) when `proxy_type` is "none" or empty — caller should
    /// skip the proxy path entirely and use direct connect.
    pub fn from_config(config: &ConnectionConfig) -> Result<Option<Self>, String> {
        match config.proxy_type.as_str() {
            "none" | "" => Ok(None),
            "socks5" => {
                let host = config
                    .proxy_host
                    .clone()
                    .filter(|h| !h.is_empty())
                    .ok_or_else(|| "SOCKS5 代理 host 缺失".to_string())?;
                let port = config.proxy_port.unwrap_or(1080);
                Ok(Some(ProxyConfig::Socks5 {
                    host,
                    port,
                    username: config.proxy_username.clone(),
                    password: config.proxy_password.clone(),
                }))
            }
            "http" => {
                let host = config
                    .proxy_host
                    .clone()
                    .filter(|h| !h.is_empty())
                    .ok_or_else(|| "HTTP 代理 host 缺失".to_string())?;
                let port = config.proxy_port.unwrap_or(8080);
                Ok(Some(ProxyConfig::HttpConnect {
                    host,
                    port,
                    username: config.proxy_username.clone(),
                    password: config.proxy_password.clone(),
                }))
            }
            other => Err(format!("未知代理类型: {}", other)),
        }
    }

    pub(crate) fn host(&self) -> &str {
        match self {
            ProxyConfig::Socks5 { host, .. } | ProxyConfig::HttpConnect { host, .. } => host,
        }
    }

    pub(crate) fn port(&self) -> u16 {
        match self {
            ProxyConfig::Socks5 { port, .. } | ProxyConfig::HttpConnect { port, .. } => *port,
        }
    }
}

/// Validate the eventual proxy target host. Rejects empty hosts and any
/// containing control/whitespace characters — CR/LF in particular would let
/// a malicious `target_host` smuggle extra bytes into the `CONNECT` request
/// line or `Host:` header of `http_connect_handshake`. A valid hostname or
/// IP literal never contains such characters.
fn validate_target_host(host: &str) -> Result<(), String> {
    if host.is_empty() {
        return Err("目标主机为空".to_string());
    }
    if host.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("目标主机包含非法字符".to_string());
    }
    Ok(())
}

/// Run the proxy handshake and return a stream ready for protocol use.
/// The returned `TcpStream` is positioned just past the handshake — caller
/// should treat it as if it were a direct `TcpStream` to the target.
pub async fn connect_via_proxy(
    proxy: &ProxyConfig,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream, String> {
    validate_target_host(target_host)?;
    let proxy_host = proxy.host().to_string();
    let proxy_port = proxy.port();

    let handshake = async {
        match proxy {
            ProxyConfig::Socks5 { username, password, .. } => {
                socks5_connect(
                    (proxy_host.as_str(), proxy_port),
                    target_host,
                    target_port,
                    username.as_deref(),
                    password.as_deref(),
                )
                .await
            }
            ProxyConfig::HttpConnect { username, password, .. } => {
                let stream = TcpStream::connect((proxy_host.as_str(), proxy_port))
                    .await
                    .map_err(|e| {
                        format!("连接代理服务器失败 ({}:{}): {}", proxy_host, proxy_port, e)
                    })?;
                http_connect_handshake(
                    stream,
                    target_host,
                    target_port,
                    username.as_deref(),
                    password.as_deref(),
                )
                .await
            }
        }
    };

    tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake)
        .await
        .map_err(|_| {
            format!(
                "代理握手超时 ({}秒) — 检查代理地址/端口/网络",
                HANDSHAKE_TIMEOUT.as_secs()
            )
        })?
}

/// SOCKS5 (RFC 1928 + RFC 1929) handshake via tokio-socks. tokio-socks does
/// its own TCP connect to the proxy, so we pass the proxy address (not a
/// pre-connected stream). `into_inner()` yields the underlying TcpStream
/// after the SOCKS5 handshake completes.
async fn socks5_connect(
    proxy: (&str, u16),
    target_host: &str,
    target_port: u16,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<TcpStream, String> {
    let target = format!("{}:{}", target_host, target_port);
    let use_auth = matches!((username, password), (Some(u), Some(_)) if !u.is_empty());

    let stream = if use_auth {
        let u = username.unwrap();
        let p = password.unwrap();
        tokio_socks::tcp::Socks5Stream::connect_with_password(proxy, target, u, p)
            .await
            .map_err(|e| {
                log::warn!("[proxy] SOCKS5 auth handshake failed {:?}: {}", proxy, e);
                format!("SOCKS5 认证握手失败: {}", e)
            })?
    } else {
        tokio_socks::tcp::Socks5Stream::connect(proxy, target)
            .await
            .map_err(|e| {
                log::warn!("[proxy] SOCKS5 handshake failed {:?}: {}", proxy, e);
                format!("SOCKS5 握手失败: {}", e)
            })?
    };
    Ok(stream.into_inner())
}

/// HTTP CONNECT (RFC 7231 §4.3.6). Sends `CONNECT host:port HTTP/1.1` and
/// reads until `\r\n\r\n`.
///
/// Reads the response one byte at a time so we never over-read past the
/// terminating `\r\n\r\n`. Any subsequent bytes the proxy forwarded (e.g.,
/// the SSH banner or FTP 220 greeting) stay in the kernel's TCP buffer and
/// are seen by the next reader of the returned TcpStream. Buffered reads
/// would risk swallowing those bytes — the protocol layer would then hang
/// waiting for a greeting that's already been consumed.
async fn http_connect_handshake(
    mut stream: TcpStream,
    target_host: &str,
    target_port: u16,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<TcpStream, String> {
    let target = format!("{}:{}", target_host, target_port);

    let auth_header = match (username, password) {
        (Some(u), Some(p)) if !u.is_empty() => {
            let credentials =
                base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", u, p));
            format!("Proxy-Authorization: Basic {}\r\n", credentials)
        }
        _ => String::new(),
    };

    let request = format!(
        "CONNECT {tgt} HTTP/1.1\r\nHost: {tgt}\r\n{auth}User-Agent: myshell\r\n\r\n",
        tgt = target,
        auth = auth_header,
    );

    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| {
            log::warn!("[proxy] HTTP CONNECT write failed → {}: {}", target, e);
            format!("HTTP CONNECT 写入失败: {}", e)
        })?;

    let mut buf: Vec<u8> = Vec::with_capacity(256);
    let mut byte = [0u8; 1];
    loop {
        let n = stream
            .read(&mut byte)
            .await
            .map_err(|e| {
                log::warn!("[proxy] HTTP CONNECT read failed → {}: {}", target, e);
                format!("HTTP CONNECT 读取失败: {}", e)
            })?;
        if n == 0 {
            log::warn!("[proxy] HTTP CONNECT proxy closed early → {}", target);
            return Err("HTTP CONNECT 代理提前关闭连接".to_string());
        }
        buf.push(byte[0]);
        if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
            break;
        }
        if buf.len() > 8192 {
            return Err("HTTP CONNECT 响应过长".to_string());
        }
    }

    let response = String::from_utf8_lossy(&buf);
    let status_line = response.lines().next().unwrap_or("");
    let status_code = status_line.split_whitespace().nth(1).unwrap_or("");
    if status_code != "200" {
        log::warn!("[proxy] HTTP CONNECT failed ({}) → {}: {}", status_code, target, status_line);
        return Err(format!("HTTP CONNECT 失败 ({}): {}", status_code, status_line));
    }

    Ok(stream)
}
