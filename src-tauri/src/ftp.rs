//! FTP/FTPS client built on suppaftp v10 (tokio runtime). Mirrors the
//! sftp.rs command surface so the frontend can reuse SftpPanel with
//! `source: "ftp"`. Sessions live in AppState::ftp_sessions keyed by UUID.
//!
//! TLS: explicit FTPS (AUTH TLS before login) and implicit FTPS (TLS from
//! byte 0, port 990) are both supported via suppaftp's rustls connector.
//! The trust chain is the Mozilla root bundle — certificate verification is
//! always on; there is deliberately no skip-verify escape hatch.
//!
//! Injection safety: suppaftp >= 10.0.2 validates command lines against
//! CR/LF on the wire (RUSTSEC-2026-0271), and this module re-validates every
//! user-controlled argument BEFORE it reaches suppaftp (`validate_ftp_arg`)
//! so the policy is ours, not the dependency's.
//!
//! Transfers: FTP has ONE control connection and strictly sequential data
//! commands (`guard_multiple_data_connections` rejects parallel data
//! streams), so batch transfers run sequentially — the `concurrency` knob
//! that applies to SFTP downloads is intentionally ignored here.

use crate::proxy;
use crate::sftp::{basename, emit_transfer_progress, TransferDonePayload};
use crate::{AppState, ConnectionConfig, EventSink, EventSinkExt, FileEntry};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use suppaftp::list::File as FtpFile;
use suppaftp::tokio::{
    AsyncFtpStream, AsyncRustlsConnector, AsyncRustlsStream, ImplAsyncFtpStream, TokioTlsStream,
};
use suppaftp::types::FileType;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TRANSFER_CHUNK: usize = 32 * 1024;
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(200);
const MAX_RECURSE_DEPTH: usize = 64;

/// suppaftp models FTPS as a different stream type (`ImplAsyncFtpStream<T>`),
/// and a session can't switch T at runtime. Both flavors forward to the same
/// operations via `ftp_dispatch!`; transfer helpers are generic over `T`.
pub enum FtpStream {
    Plain(AsyncFtpStream),
    Tls(Box<ImplAsyncFtpStream<AsyncRustlsStream>>),
}

pub struct FtpSession {
    pub stream: FtpStream,
}

/// Dispatch one await-able control-channel method across both stream kinds.
macro_rules! ftp_dispatch {
    ($stream:expr, $method:ident ( $($arg:expr),* )) => {
        match &mut $stream {
            FtpStream::Plain(s) => s.$method($($arg),*).await,
            FtpStream::Tls(s) => s.$method($($arg),*).await,
        }
    };
}

/// FTP control-channel safety gate. Arguments travel in a line-oriented
/// protocol: CR/LF would terminate the intended command early and let a
/// second, attacker-chosen command run inside the same authenticated session
/// (FTP command injection). NUL is protocol garbage. Every user-controlled
/// string (username, password, paths) passes through here first.
fn validate_ftp_arg(kind: &str, value: &str) -> Result<(), String> {
    if value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
        return Err(format!("FTP {} 包含非法字符（CR/LF/NUL）", kind));
    }
    Ok(())
}

/// rustls connector anchored to the Mozilla root bundle. Verification is
/// always enabled — a hostkey/CA failure aborts the connection.
fn ftps_connector() -> Result<AsyncRustlsConnector, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("FTPS TLS 配置失败: {e}"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(AsyncRustlsConnector::from(tokio_rustls::TlsConnector::from(
        Arc::new(config),
    )))
}

/// Login + binary transfer type for an already-established (possibly TLS)
/// control channel. Active/passive selection happens in `connect()` where the
/// stream is owned (`active_mode` is a consuming builder).
async fn login_and_set_binary<T>(
    stream: &mut ImplAsyncFtpStream<T>,
    username: &str,
    password: &str,
) -> Result<(), String>
where
    T: TokioTlsStream + Send + 'static,
{
    stream
        .login(username, password)
        .await
        .map_err(|e| format!("FTP login failed: {}", e))?;
    stream
        .transfer_type(FileType::Binary)
        .await
        .map_err(|e| format!("FTP binary type failed: {}", e))?;
    Ok(())
}

pub async fn connect(cfg: &ConnectionConfig) -> Result<FtpSession, String> {
    let implicit = cfg.ftp_tls == "implicit";
    let explicit = cfg.ftp_tls == "explicit";
    if (implicit || explicit) && cfg.proxy_type != "none" {
        return Err(
            "FTPS 暂不支持经代理连接，请改用直连，或选择「不加密 (FTP)」。".to_string(),
        );
    }
    let default_port = if implicit { 990 } else { 21 };
    let port = if cfg.port == 0 { default_port } else { cfg.port };
    let addr = format!("{}:{}", cfg.host, port);

    let password = cfg.password.clone().unwrap_or_default();
    validate_ftp_arg("用户名", &cfg.username)?;
    validate_ftp_arg("密码", &password)?;

    // ── Implicit FTPS: TLS handshake before the greeting. ──
    if implicit {
        log::info!(
            "[ftp] connecting implicit FTPS to {}:{}",
            crate::redact::host(&cfg.host),
            port
        );
        let connector = ftps_connector()?;
        let mut stream =
            ImplAsyncFtpStream::<AsyncRustlsStream>::connect_secure_implicit(
                addr.as_str(),
                connector,
                &cfg.host,
            )
            .await
            .map_err(|e| format!("FTPS (implicit) connect failed: {}", e))?;
        if !cfg.ftp_passive {
            stream = stream.active_mode(Duration::from_secs(10));
        }
        login_and_set_binary(&mut stream, &cfg.username, &password).await?;
        log::info!("[ftp] implicit FTPS session ready");
        return Ok(FtpSession {
            stream: FtpStream::Tls(Box::new(stream)),
        });
    }

    // ── Explicit FTPS: connect plain, then AUTH TLS on the control channel.
    // The stream is typed `ImplAsyncFtpStream<AsyncRustlsStream>` from the
    // start: `into_secure` requires the connector's stream type to match the
    // session's T (a plain session's T is `AsyncNoTlsStream`).
    if explicit {
        log::info!(
            "[ftp] connecting for explicit FTPS to {}:{}",
            crate::redact::host(&cfg.host),
            port
        );
        let mut plain = ImplAsyncFtpStream::<AsyncRustlsStream>::connect(addr.as_str())
            .await
            .map_err(|e| format!("FTP connect failed: {}", e))?;
        log::info!(
            "[ftp] upgrading to explicit FTPS (AUTH TLS) with {}",
            crate::redact::host(&cfg.host)
        );
        let connector = ftps_connector()?;
        let mut tls_stream = plain
            .into_secure(connector, &cfg.host)
            .await
            .map_err(|e| format!("FTPS (explicit) AUTH TLS 失败: {}", e))?;
        if !cfg.ftp_passive {
            tls_stream = tls_stream.active_mode(Duration::from_secs(10));
        }
        login_and_set_binary(&mut tls_stream, &cfg.username, &password).await?;
        log::info!("[ftp] explicit FTPS session ready");
        return Ok(FtpSession {
            stream: FtpStream::Tls(Box::new(tls_stream)),
        });
    }

    // ── Plain TCP connect (proxy or direct). ──
    let mut stream = match proxy::ProxyConfig::from_config(cfg)? {
            Some(proxy_cfg) => {
                log::info!(
                    "[ftp] connecting via {} proxy {}:{} → {}:{}",
                    cfg.proxy_type,
                    crate::redact::host(proxy_cfg.host()),
                    proxy_cfg.port(),
                    crate::redact::host(&cfg.host),
                    port
                );
                let stream = proxy::connect_via_proxy(&proxy_cfg, &cfg.host, port).await?;
                AsyncFtpStream::connect_with_stream(stream)
                    .await
                    .map_err(|e| format!("FTP connect via proxy failed: {}", e))?
            }
            None => {
                log::info!("[ftp] connecting to {}:{}", crate::redact::host(&cfg.host), port);
                AsyncFtpStream::connect(addr.as_str())
                    .await
                    .map_err(|e| {
                        // 10060 = WSAETIMEDOUT on Windows: TCP connect timed out.
                        // Surface a hint that it's almost always network
                        // reachability, not a bad credential. The full host here
                        // is a user-facing toast (not logged verbatim).
                        let raw = e.to_string();
                        if raw.contains("10060")
                            || raw.contains("timed out")
                            || raw.contains("timeout")
                        {
                            format!(
                                "FTP connect failed: TCP 连接超时（os error 10060）。通常不是密码错误，而是：目标 {}:{} 无法到达——防火墙拦截、IP/端口填错、或 FTP 服务未运行。先 ping/网络验证该地址端口是否可达。原始错误：{}",
                                cfg.host, port, raw
                            )
                        } else {
                            format!("FTP connect failed: {}", raw)
                        }
                    })?
            }
    };

    // ── Plain FTP. ──
    log::info!(
        "[ftp] TCP established, logging in as {}",
        crate::redact::user(&cfg.username)
    );
    stream
        .login(&cfg.username, &password)
        .await
        .map_err(|e| format!("FTP login failed: {}", e))?;
    if !cfg.ftp_passive {
        stream = stream.active_mode(std::time::Duration::from_secs(10));
    }
    stream
        .transfer_type(FileType::Binary)
        .await
        .map_err(|e| format!("FTP binary type failed: {}", e))?;
    log::info!("[ftp] session ready");
    Ok(FtpSession {
        stream: FtpStream::Plain(stream),
    })
}

pub async fn list_dir(s: &mut FtpSession, path: &str) -> Result<Vec<FileEntry>, String> {
    validate_ftp_arg("路径", path)?;
    // MLSD is the modern machine-readable listing. Fall back to LIST (POSIX)
    // for servers that don't support it. Both commands return raw lines —
    // ListParser turns them into typed File structs.
    let lines: Vec<String> = match ftp_dispatch!(s.stream, mlsd (Some(path))) {
        Ok(v) if !v.is_empty() => v,
        _ => ftp_dispatch!(s.stream, list (Some(path)))
            .map_err(|e| format!("FTP list failed: {}", e))?,
    };

    let parent = path.trim_end_matches('/');
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let parsed = suppaftp::list::ListParser::parse_mlsd(&line)
            .or_else(|_| suppaftp::list::ListParser::parse_posix(&line));
        let f = match parsed {
            Ok(f) => f,
            Err(_) => continue,
        };
        let name = f.name().to_string();
        if name == "." || name == ".." {
            continue;
        }
        let full = if parent.is_empty() || parent == "." {
            format!("/{}", name)
        } else {
            format!("{}/{}", parent, name)
        };
        out.push(FileEntry {
            name,
            path: full,
            is_dir: f.is_directory(),
            size: f.size() as u64,
            permissions: format_pex(&f),
            modified: format_time(f.modified()),
        });
    }
    out.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(out)
}

/// SIZE of a remote path — Ok only for files. Used to distinguish a plain
/// file from a directory when expanding a download selection.
async fn remote_size(s: &mut FtpSession, path: &str) -> Option<u64> {
    validate_ftp_arg("路径", path).ok()?;
    match ftp_dispatch!(s.stream, size (path)) {
        Ok(n) => Some(n as u64),
        Err(_) => None,
    }
}

pub async fn mkdir(s: &mut FtpSession, path: &str) -> Result<(), String> {
    validate_ftp_arg("路径", path)?;
    ftp_dispatch!(s.stream, mkdir (path)).map_err(|e| format!("FTP mkdir failed: {}", e))
}

pub async fn remove(s: &mut FtpSession, path: &str, is_dir: bool) -> Result<(), String> {
    validate_ftp_arg("路径", path)?;
    let r = if is_dir {
        ftp_dispatch!(s.stream, rmdir (path))
    } else {
        ftp_dispatch!(s.stream, rm (path))
    };
    r.map_err(|e| format!("FTP remove failed: {}", e))
}

pub async fn rename(s: &mut FtpSession, from: &str, to: &str) -> Result<(), String> {
    validate_ftp_arg("源路径", from)?;
    validate_ftp_arg("目标路径", to)?;
    ftp_dispatch!(s.stream, rename (from, to))
        .map_err(|e| format!("FTP rename failed: {}", e))
}

pub async fn disconnect(s: &mut FtpSession) -> Result<(), String> {
    // quit sends QUIT command and closes — best-effort, ignore errors.
    let _ = ftp_dispatch!(s.stream, quit ());
    Ok(())
}

/// Probe an FTP/FTPS connection by establishing it then immediately tearing
/// down. Reuses `connect` (proxy/TLS + login + transfer_type) and
/// `disconnect` (QUIT), so the test exercises the same path a real connect
/// takes. Used by `test_connection`.
pub async fn test_connection(cfg: &ConnectionConfig) -> Result<String, String> {
    let started = std::time::Instant::now();
    let mut session = connect(cfg).await?;
    let _ = disconnect(&mut session).await;
    let ms = started.elapsed().as_millis();
    let mode = match cfg.ftp_tls.as_str() {
        "explicit" => "FTPS explicit",
        "implicit" => "FTPS implicit",
        _ => "FTP 登录",
    };
    Ok(format!("连接成功（{} ms，{}通过）", ms, mode))
}

// ============ Batch transfer (upload / download) ============
//
// FTP transfers emit the SAME `sftp_transfer_progress` / `sftp_transfer_done`
// events as SFTP so the frontend overlay works unchanged. The session is
// taken out of AppState for the duration (single control connection) and
// always returned, even on error.

fn take(state: &AppState, session_id: &str) -> Result<FtpSession, String> {
    let mut sessions = state.ftp_sessions.lock().map_err(|e| e.to_string())?;
    sessions
        .remove(session_id)
        .ok_or_else(|| "FTP session not found".to_string())
}

fn put_back(state: &AppState, session_id: &str, session: FtpSession) {
    if let Ok(mut sessions) = state.ftp_sessions.lock() {
        sessions.insert(session_id.to_string(), session);
    }
}

/// Trim trailing slashes so `<dest>/<name>` doesn't double them. A dest of
/// "." or "" yields just `<name>` (relative to the login directory).
fn join_remote(dest: &str, name: &str) -> String {
    let dest = dest.trim_end_matches('/');
    if dest.is_empty() || dest == "." {
        name.to_string()
    } else {
        format!("{}/{}", dest, name)
    }
}

pub async fn upload(
    state: &AppState,
    session_id: &str,
    local_paths: Vec<String>,
    remote_dest_dir: &str,
    request_id: &str,
    sink: Arc<dyn EventSink>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut session = take(state, session_id)?;
    let result = upload_impl(&mut session, sink.as_ref(), request_id, local_paths, remote_dest_dir, &cancel).await;
    put_back(state, session_id, session);
    result
}

async fn upload_impl(
    s: &mut FtpSession,
    sink: &dyn EventSink,
    request_id: &str,
    local_paths: Vec<String>,
    remote_dest_dir: &str,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    validate_ftp_arg("目标目录", remote_dest_dir)?;

    // Pre-stat: total files + bytes so the overlay shows a real progress bar.
    let mut errors: Vec<String> = Vec::new();
    let mut tasks: Vec<String> = Vec::new();
    let mut bytes_total: u64 = 0;
    for lp in &local_paths {
        match tokio::fs::metadata(lp).await {
            Ok(md) if md.is_file() => {
                bytes_total = bytes_total.saturating_add(md.len());
                tasks.push(lp.clone());
            }
            Ok(_) => errors.push(format!("{}: 不是文件（已跳过）", basename(lp))),
            Err(e) => errors.push(format!("{}: 读取本地信息失败: {}", basename(lp), e)),
        }
    }
    let file_count = tasks.len();

    let mut bytes_done: u64 = 0;
    for (i, lp) in tasks.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            errors.push("已取消".to_string());
            break;
        }
        let name = basename(lp);
        let remote_path = join_remote(remote_dest_dir, &name);
        let mut last_emit = Instant::now();
        emit_transfer_progress(sink, request_id, "upload", &name, i, file_count, bytes_done, bytes_total);
        let res = dispatch_transfer(s, sink, TransferOp::Put {
            local_path: lp.clone(),
            remote_path,
        }, request_id, i, file_count, bytes_total, &mut bytes_done, &mut last_emit, cancel)
        .await;
        if let Err(e) = res {
            errors.push(e);
            if cancel.load(Ordering::Relaxed) {
                break;
            }
        }
    }

    emit_transfer_progress(sink, request_id, "upload", "", file_count, file_count, bytes_done, bytes_total);
    sink.emit(
        "sftp_transfer_done",
        &TransferDonePayload {
            request_id: request_id.to_string(),
            errors,
        },
    );
    Ok(())
}

pub async fn download(
    state: &AppState,
    session_id: &str,
    remote_paths: Vec<String>,
    local_dest_dir: &str,
    request_id: &str,
    _concurrency: usize,
    sink: Arc<dyn EventSink>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut session = take(state, session_id)?;
    let result = download_impl(
        &mut session,
        sink.as_ref(),
        request_id,
        remote_paths,
        local_dest_dir,
        &cancel,
    )
    .await;
    put_back(state, session_id, session);
    result
}

#[derive(Clone)]
struct FtpDownloadTask {
    remote_path: String,
    relative: String,
    size: u64,
}

async fn download_impl(
    s: &mut FtpSession,
    sink: &dyn EventSink,
    request_id: &str,
    remote_paths: Vec<String>,
    local_dest_dir: &str,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    tokio::fs::create_dir_all(local_dest_dir)
        .await
        .map_err(|e| format!("创建本地目录失败: {}", e))?;

    // Phase 1: expand the selection recursively (single control connection —
    // sequential listing).
    let mut errors: Vec<String> = Vec::new();
    let mut tasks: Vec<FtpDownloadTask> = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    for rp in &remote_paths {
        expand_download(s, rp, &basename(rp), &mut tasks, &mut dirs, &mut errors, 0).await;
    }
    // Phase 1.5: local folder skeleton — validated components, no symlinks.
    for d in &dirs {
        match local_components(local_dest_dir, d) {
            Ok(p) => {
                if let Err(e) = crate::path_safety::ensure_no_symlink_components(&p).await {
                    errors.push(format!("{}: {}", d, e));
                    continue;
                }
                if let Err(e) = tokio::fs::create_dir_all(&p).await {
                    errors.push(format!("{}: 创建本地目录失败: {}", d, e));
                }
            }
            Err(reason) => errors.push(format!("{}: {}", d, reason)),
        }
    }

    // Phase 2: sequential transfer (FTP forbids parallel data connections).
    let file_count = tasks.len();
    let bytes_total = tasks.iter().fold(0u64, |acc, t| acc.saturating_add(t.size));
    let mut bytes_done: u64 = 0;
    for (i, task) in tasks.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            errors.push("已取消".to_string());
            break;
        }
        let local_path = match local_components(local_dest_dir, &task.relative) {
            Ok(p) => p,
            Err(reason) => {
                errors.push(format!("{}: {}", task.relative, reason));
                continue;
            }
        };
        let mut last_emit = Instant::now();
        let name = basename(&task.remote_path);
        emit_transfer_progress(sink, request_id, "download", &name, i, file_count, bytes_done, bytes_total);
        let res = dispatch_transfer(s, sink, TransferOp::Get {
            remote_path: task.remote_path.clone(),
            local_path,
            size: task.size,
        }, request_id, i, file_count, bytes_total, &mut bytes_done, &mut last_emit, cancel)
        .await;
        if let Err(e) = res {
            errors.push(e);
            if cancel.load(Ordering::Relaxed) {
                break;
            }
        }
    }
    if cancel.load(Ordering::Relaxed) && !errors.iter().any(|e| e == "已取消") {
        errors.push("已取消".to_string());
    }

    let bytes_done_now = bytes_done;
    emit_transfer_progress(sink, request_id, "download", "", file_count, file_count, bytes_done_now, bytes_total);
    sink.emit(
        "sftp_transfer_done",
        &TransferDonePayload {
            request_id: request_id.to_string(),
            errors,
        },
    );
    Ok(())
}

/// Recursively expand one selected remote path into download tasks. A path
/// with a SIZE is a file; anything else is listed as a directory. Every name
/// passes `path_safety::validate_component` before it can become part of a
/// local relative path.
async fn expand_download(
    s: &mut FtpSession,
    remote_path: &str,
    rel: &str,
    tasks: &mut Vec<FtpDownloadTask>,
    dirs: &mut Vec<String>,
    errors: &mut Vec<String>,
    depth: usize,
) {
    if depth > MAX_RECURSE_DEPTH {
        errors.push(format!("{}: 目录层级过深（>{}）", rel, MAX_RECURSE_DEPTH));
        return;
    }
    if let Err(reason) = crate::path_safety::validate_component(rel) {
        errors.push(format!("{}: {}", rel, reason));
        return;
    }
    // File? SIZE succeeds only on files (we forced binary mode at connect).
    if let Some(size) = remote_size(s, remote_path).await {
        tasks.push(FtpDownloadTask {
            remote_path: remote_path.to_string(),
            relative: rel.to_string(),
            size,
        });
        return;
    }
    // Directory (or SIZE-unsupported file — disambiguated below).
    let entries = match list_dir(s, remote_path).await {
        Ok(e) => e,
        Err(e) => {
            errors.push(format!("{}: 读取目录失败: {}", rel, e));
            return;
        }
    };
    let base = rel.rsplit('/').next().unwrap_or(rel);
    if entries.len() == 1 && !entries[0].is_dir && entries[0].name == base {
        // LIST on a file path returns the file itself — treat as a file.
        tasks.push(FtpDownloadTask {
            remote_path: remote_path.to_string(),
            relative: rel.to_string(),
            size: entries[0].size,
        });
        return;
    }
    dirs.push(rel.to_string());
    for e in entries {
        let child_rel = format!("{}/{}", rel, e.name);
        if e.is_dir {
            Box::pin(expand_download(
                s, &e.path, &child_rel, tasks, dirs, errors, depth + 1,
            ))
            .await;
        } else {
            if let Err(reason) = crate::path_safety::validate_component(&e.name) {
                errors.push(format!("{}: {}", child_rel, reason));
                continue;
            }
            tasks.push(FtpDownloadTask {
                remote_path: e.path,
                relative: child_rel,
                size: e.size,
            });
        }
    }
}

/// Split a "/"-joined relative task path into validated components and build
/// the local absolute path under `dest` (shared logic with sftp.rs).
fn local_components(dest: &str, relative: &str) -> Result<std::path::PathBuf, String> {
    let parts: Vec<&str> = relative.split('/').collect();
    for p in &parts {
        crate::path_safety::validate_component(p)?;
    }
    Ok(crate::path_safety::build_path(Path::new(dest), &parts))
}

/// Which half of the Plain/TLS enum a transfer runs against. The helper fns
/// are generic over `T: TokioTlsStream` so both arms share one body.
enum TransferOp {
    Put {
        local_path: String,
        remote_path: String,
    },
    Get {
        remote_path: String,
        local_path: std::path::PathBuf,
        size: u64,
    },
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_transfer(
    s: &mut FtpSession,
    sink: &dyn EventSink,
    op: TransferOp,
    request_id: &str,
    file_index: usize,
    file_count: usize,
    bytes_total: u64,
    bytes_done: &mut u64,
    last_emit: &mut Instant,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    match &mut s.stream {
        FtpStream::Plain(st) => {
            run_transfer_op(st, sink, op, request_id, file_index, file_count, bytes_total, bytes_done, last_emit, cancel).await
        }
        FtpStream::Tls(st) => {
            run_transfer_op(st, sink, op, request_id, file_index, file_count, bytes_total, bytes_done, last_emit, cancel).await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_transfer_op<T>(
    s: &mut ImplAsyncFtpStream<T>,
    sink: &dyn EventSink,
    op: TransferOp,
    request_id: &str,
    file_index: usize,
    file_count: usize,
    bytes_total: u64,
    bytes_done: &mut u64,
    last_emit: &mut Instant,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String>
where
    T: TokioTlsStream + Send + 'static,
{
    match op {
        TransferOp::Put {
            local_path,
            remote_path,
        } => {
            let name = basename(&local_path);
            let mut local = tokio::fs::File::open(&local_path)
                .await
                .map_err(|e| format!("{}: 打开本地文件失败: {}", name, e))?;
            let mut ds = s
                .put_with_stream(remote_path.as_str())
                .await
                .map_err(|e| format!("{}: 打开远端文件失败: {}", name, e))?;
            let mut buf = vec![0u8; TRANSFER_CHUNK];
            let result: Result<(), String> = loop {
                if cancel.load(Ordering::Relaxed) {
                    break Err("已取消".to_string());
                }
                let n = match local.read(&mut buf).await {
                    Ok(0) => break Ok(()),
                    Ok(n) => n,
                    Err(e) => break Err(format!("{}: 读取失败: {}", name, e)),
                };
                if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut ds, &buf[..n]).await {
                    break Err(format!("{}: 上传写入失败: {}", name, e));
                }
                *bytes_done = bytes_done.saturating_add(n as u64);
                if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
                    emit_transfer_progress(
                        sink,
                        request_id,
                        "upload",
                        &name,
                        file_index,
                        file_count,
                        *bytes_done,
                        bytes_total,
                    );
                    *last_emit = Instant::now();
                }
            };
            if result.is_err() {
                // ABOR the pending data command so the control channel is
                // usable for the next file.
                let _ = s.abort(ds).await;
                return result;
            }
            s.finalize_put_stream(ds)
                .await
                .map_err(|e| format!("{}: 结束上传失败: {}", name, e))?;
            Ok(())
        }
        TransferOp::Get {
            remote_path,
            local_path,
            size: _size,
        } => {
            let name = basename(&remote_path);
            // No symlink/reparse-point redirection out of the destination.
            if let Err(e) = crate::path_safety::ensure_no_symlink_components(&local_path).await {
                return Err(format!("{}: {}", name, e));
            }
            let mut local = tokio::fs::File::create(&local_path)
                .await
                .map_err(|e| format!("{}: 创建本地文件失败: {}", name, e))?;
            let mut ds = s
                .retr_as_stream(remote_path.as_str())
                .await
                .map_err(|e| format!("{}: 打开远端文件失败: {}", name, e))?;
            let mut buf = vec![0u8; TRANSFER_CHUNK];
            let result: Result<(), String> = loop {
                if cancel.load(Ordering::Relaxed) {
                    break Err("已取消".to_string());
                }
                let n = match tokio::io::AsyncReadExt::read(&mut ds, &mut buf).await {
                    Ok(0) => break Ok(()),
                    Ok(n) => n,
                    Err(e) => break Err(format!("{}: 下载读取失败: {}", name, e)),
                };
                if let Err(e) = local.write_all(&buf[..n]).await {
                    break Err(format!("{}: 写入本地失败: {}", name, e));
                }
                *bytes_done = bytes_done.saturating_add(n as u64);
                if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
                    emit_transfer_progress(
                        sink,
                        request_id,
                        "download",
                        &name,
                        file_index,
                        file_count,
                        *bytes_done,
                        bytes_total,
                    );
                    *last_emit = Instant::now();
                }
            };
            if result.is_err() {
                let _ = s.abort(ds).await;
                return result;
            }
            s.finalize_retr_stream(ds)
                .await
                .map_err(|e| format!("{}: 结束下载失败: {}", name, e))?;
            let _ = local.flush().await;
            Ok(())
        }
    }
}

fn format_pex(f: &FtpFile) -> String {
    use suppaftp::list::PosixPexQuery;
    let r = |who: PosixPexQuery| f.can_read(who);
    let w = |who: PosixPexQuery| f.can_write(who);
    let x = |who: PosixPexQuery| f.can_execute(who);
    let o = PosixPexQuery::Owner;
    let g = PosixPexQuery::Group;
    let ot = PosixPexQuery::Others;
    let bit = |cond: bool, chr: char| if cond { chr } else { '-' };
    format!(
        "{}{}{}{}{}{}{}{}{}",
        bit(r(o), 'r'),
        bit(w(o), 'w'),
        bit(x(o), 'x'),
        bit(r(g), 'r'),
        bit(w(g), 'w'),
        bit(x(g), 'x'),
        bit(r(ot), 'r'),
        bit(w(ot), 'w'),
        bit(x(ot), 'x'),
    )
}

fn format_time(t: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(t)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_args_pass() {
        assert!(validate_ftp_arg("路径", "/var/www").is_ok());
        assert!(validate_ftp_arg("用户名", "user@example.com").is_ok());
        assert!(validate_ftp_arg("密码", "p@ss w0rd!#$%^&*()").is_ok());
        assert!(validate_ftp_arg("路径", "数据目录/文件.txt").is_ok());
    }

    #[test]
    fn crlf_injection_rejected() {
        // RUSTSEC-2026-0271 payloads: a CRLF in an argument would terminate
        // the intended command and inject a second one.
        assert!(validate_ftp_arg("用户名", "user\r\nDELE /etc/passwd").is_err());
        assert!(validate_ftp_arg("路径", "/a\rPORT 1,2,3,4,5,6").is_err());
        assert!(validate_ftp_arg("密码", "pw\n").is_err());
        assert!(validate_ftp_arg("路径", "x\ry").is_err());
    }

    #[test]
    fn nul_rejected() {
        assert!(validate_ftp_arg("路径", "/a\0b").is_err());
    }

    #[test]
    fn join_remote_normalizes() {
        assert_eq!(join_remote("/var/www", "a.txt"), "/var/www/a.txt");
        assert_eq!(join_remote("/var/www/", "a.txt"), "/var/www/a.txt");
        assert_eq!(join_remote(".", "a.txt"), "a.txt");
        assert_eq!(join_remote("", "a.txt"), "a.txt");
    }
}
