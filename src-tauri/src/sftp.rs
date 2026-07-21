use crate::{AppState, FileEntry};
use crate::{EventSink, EventSinkExt};
use russh_sftp::client::SftpSession;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn get_sftp_session(
    state: &AppState,
    session_id: &str,
) -> Result<SftpSession, String> {
    // Scope the MutexGuard so it's dropped before any await — std::sync::MutexGuard
    // is not Send, and the returned future must be Send.
    let handle = {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| "SSH session not found".to_string())?;
        Arc::clone(&session.handle)
    };

    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| {
            log::error!("[sftp:{}] channel open failed: {}", session_id, e);
            format!("SFTP channel open failed: {}", e)
        })?;

    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| {
            log::error!("[sftp:{}] subsystem request failed: {}", session_id, e);
            format!("SFTP subsystem request failed: {}", e)
        })?;

    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| {
            log::error!("[sftp:{}] session init failed: {}", session_id, e);
            format!("SFTP session init failed: {}", e)
        })?;

    Ok(sftp)
}

/// Expand a leading `~` to the user's absolute home directory.
///
/// The SFTP protocol (and russh-sftp) treats `~` as a **literal** directory
/// name — there is no shell to expand it — so `read_dir("~")` always fails
/// with `SSH_FX_NO_SUCH_FILE` on a standard server. We resolve the home dir
/// via SFTP REALPATH of "." (the server's default directory, i.e. the home),
/// then splice the remainder back on. Bare `~`, `~/`, and `~/foo/bar` all
/// resolve; absolute/relative paths pass through untouched.
async fn resolve_path(sftp: &SftpSession, path: &str) -> Result<String, String> {
    let needs_home = path == "~" || path == "~/" || path.starts_with("~/");
    if !needs_home {
        return Ok(path.to_string());
    }

    let home = sftp
        .canonicalize(".")
        .await
        .map_err(|e| format!("Resolve home failed: {}", e))?;
    let trimmed = home.trim_end_matches('/');

    let rest = path.strip_prefix('~').unwrap_or("");
    // rest is "", "/", or "/foo/bar"
    match rest.trim_start_matches('/') {
        "" => Ok(if trimmed.is_empty() {
            "/".to_string()
        } else {
            trimmed.to_string()
        }),
        suffix => Ok(format!("{}/{}", trimmed, suffix)),
    }
}

pub async fn list_dir(
    state: &AppState,
    session_id: &str,
    path: &str,
) -> Result<Vec<FileEntry>, String> {
    let sftp = get_sftp_session(state, session_id).await?;
    let resolved = resolve_path(&sftp, path).await?;

    let entries = sftp
        .read_dir(resolved.as_str())
        .await
        .map_err(|e| format!("Read dir failed: {}", e))?;

    let mut files = Vec::new();
    for entry in entries {
        let file_type = entry.file_type();
        let name = entry.file_name().to_string();
        // Build child paths from the resolved absolute path (not the raw `~`
        // the frontend sent) so navigation uses real paths from here on.
        let full_path = if resolved.ends_with('/') {
            format!("{}{}", resolved, name)
        } else {
            format!("{}/{}", resolved, name)
        };

        files.push(FileEntry {
            name,
            path: full_path,
            is_dir: file_type.is_dir(),
            size: entry.metadata().size.unwrap_or(0),
            permissions: format!("{}", entry.metadata().permissions()),
            modified: entry
                .metadata()
                .mtime
                .map(|t| t.to_string())
                .unwrap_or_default(),
        });
    }

    // Sort: dirs first, then files
    files.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(files)
}

pub async fn create_dir(
    state: &AppState,
    session_id: &str,
    path: &str,
) -> Result<(), String> {
    let sftp = get_sftp_session(state, session_id).await?;
    let resolved = resolve_path(&sftp, path).await?;
    sftp.create_dir(resolved)
        .await
        .map_err(|e| format!("Create dir failed: {}", e))?;
    Ok(())
}

pub async fn remove(
    state: &AppState,
    session_id: &str,
    path: &str,
) -> Result<(), String> {
    let sftp = get_sftp_session(state, session_id).await?;
    let resolved = resolve_path(&sftp, path).await?;

    // Try removing as file first, then as directory
    if sftp.remove_file(resolved.as_str()).await.is_err() {
        sftp.remove_dir(resolved.as_str())
            .await
            .map_err(|e| format!("Remove failed: {}", e))?;
    }
    Ok(())
}

pub async fn rename(
    state: &AppState,
    session_id: &str,
    old_path: &str,
    new_path: &str,
) -> Result<(), String> {
    let sftp = get_sftp_session(state, session_id).await?;
    let old_resolved = resolve_path(&sftp, old_path).await?;
    let new_resolved = resolve_path(&sftp, new_path).await?;
    sftp.rename(old_resolved, new_resolved)
        .await
        .map_err(|e| format!("Rename failed: {}", e))?;
    Ok(())
}

// ============ File transfer (upload / download) ============
//
// Batch, files-only, overwrite. Each file opens its own SFTP file handle on
// the shared `SftpSession` (one subsystem channel per batch). Progress streams
// to the originating event sink via `sftp_transfer_*` events, keyed by `request_id`
// so concurrent transfers in different panels don't cross.
//
// Per-file errors are collected, not fatal: one unreadable file must not abort
// a 100-file batch. The caller receives the list in `sftp_transfer_done`.

/// Read/write chunk. A common SFTP sweet spot — large enough to amortize
/// per-packet overhead, small enough to keep memory + progress granularity fine.
const TRANSFER_CHUNK: usize = 32 * 1024;

/// Throttle in-flight progress emits during a single large file so a multi-GB
/// transfer doesn't flood the IPC bridge.
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(120);

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferProgressPayload {
    request_id: String,
    /// "upload" | "download" — drives the overlay label.
    phase: &'static str,
    current_file: String,
    file_index: usize,
    file_count: usize,
    bytes_done: u64,
    bytes_total: u64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferDonePayload {
    request_id: String,
    errors: Vec<String>,
}

fn emit_transfer_progress(
    sink: &dyn EventSink,
    request_id: &str,
    phase: &'static str,
    current_file: &str,
    file_index: usize,
    file_count: usize,
    bytes_done: u64,
    bytes_total: u64,
) {
    sink.emit(
        "sftp_transfer_progress",
        &TransferProgressPayload {
            request_id: request_id.to_string(),
            phase,
            current_file: current_file.to_string(),
            file_index,
            file_count,
            bytes_done,
            bytes_total,
        },
    );
}

/// Best-effort basename. Using `Path::file_name` (not a naive rsplit on `/`)
/// avoids a Windows local path like `C:\Users\foo\bar.txt` leaking the
/// directory into the remote filename. Falls back to the full path on weird
/// input so the UI still shows *something*.
fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// Upload a batch of local files into `remote_dest_dir` (resolved through
/// `resolve_path` so `~` works). Overwrites existing remote files. Returns
/// `Err` only on fatal failures (no SFTP session); per-file failures land in
/// the `sftp_transfer_done` errors list.
pub async fn upload(
    state: &AppState,
    session_id: &str,
    local_paths: Vec<String>,
    remote_dest_dir: &str,
    request_id: &str,
    sink: &dyn EventSink,
) -> Result<(), String> {
    let sftp = get_sftp_session(state, session_id).await?;
    let dest = resolve_path(&sftp, remote_dest_dir).await?;

    // Pre-stat: total files + bytes so the overlay shows a real progress bar.
    // Non-files (dirs) violate the files-only contract — skip + record.
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
        let name = basename(lp);
        let remote_path = if dest.ends_with('/') {
            format!("{}{}", dest, name)
        } else {
            format!("{}/{}", dest, name)
        };

        let mut last_emit = Instant::now();
        emit_transfer_progress(
            sink, request_id, "upload", &name, i, file_count, bytes_done, bytes_total,
        );
        if let Err(e) = upload_one(
            &sftp, sink, request_id, lp, &remote_path, i, file_count, bytes_total,
            &mut bytes_done, &mut last_emit,
        )
        .await
        {
            errors.push(e);
        }
    }

    // Final tick so the overlay completes cleanly at 100%.
    emit_transfer_progress(
        sink, request_id, "upload", "", file_count, file_count, bytes_done, bytes_total,
    );
    sink.emit(
        "sftp_transfer_done",
        &TransferDonePayload {
            request_id: request_id.to_string(),
            errors,
        },
    );
    Ok(())
}

async fn upload_one(
    sftp: &SftpSession,
    sink: &dyn EventSink,
    request_id: &str,
    local_path: &str,
    remote_path: &str,
    file_index: usize,
    file_count: usize,
    bytes_total: u64,
    bytes_done: &mut u64,
    last_emit: &mut Instant,
) -> Result<(), String> {
    let name = basename(local_path);
    let mut local = tokio::fs::File::open(local_path)
        .await
        .map_err(|e| {
            log::warn!("[sftp] upload {} open local failed: {}", name, e);
            format!("{}: 打开本地文件失败: {}", name, e)
        })?;
    // create() = CREATE|TRUNCATE|WRITE → overwrite semantics.
    let mut remote = sftp
        .create(remote_path)
        .await
        .map_err(|e| {
            log::warn!("[sftp] upload {} open remote failed: {}", name, e);
            format!("{}: 打开远端文件失败: {}", name, e)
        })?;

    let mut buf = vec![0u8; TRANSFER_CHUNK];
    loop {
        let n = local
            .read(&mut buf)
            .await
            .map_err(|e| {
                log::warn!("[sftp] upload {} read failed: {}", name, e);
                format!("{}: 读取失败: {}", name, e)
            })?;
        if n == 0 {
            break;
        }
        remote
            .write_all(&buf[..n])
            .await
            .map_err(|e| {
                log::warn!("[sftp] upload {} write failed: {}", name, e);
                format!("{}: 写入失败: {}", name, e)
            })?;
        *bytes_done = bytes_done.saturating_add(n as u64);
        if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
            emit_transfer_progress(
                sink, request_id, "upload", &name, file_index, file_count, *bytes_done,
                bytes_total,
            );
            *last_emit = Instant::now();
        }
    }
    // flush() awaits all pending write acks so the data is durable on the
    // server before we release the handle. The SFTP close-handle packet itself
    // is sent by `File`'s Drop impl (close_nowait) — there is no explicit
    // close() method on russh-sftp 2.3's File.
    let _ = remote.flush().await;
    Ok(())
}

/// Download a batch of remote files into `local_dest_dir`. `remote_paths` are
/// taken absolute (they come from the listing). Overwrites local files.
pub async fn download(
    state: &AppState,
    session_id: &str,
    remote_paths: Vec<String>,
    local_dest_dir: &str,
    request_id: &str,
    sink: &dyn EventSink,
) -> Result<(), String> {
    let sftp = get_sftp_session(state, session_id).await?;
    // Local destination — created if missing (covers a freshly-typed path).
    tokio::fs::create_dir_all(local_dest_dir)
        .await
        .map_err(|e| format!("创建本地目录失败: {}", e))?;

    // Pre-stat remote files for the progress bar.
    let mut errors: Vec<String> = Vec::new();
    let mut tasks: Vec<String> = Vec::new();
    let mut bytes_total: u64 = 0;
    for rp in &remote_paths {
        match sftp.metadata(rp).await {
            Ok(md) => {
                bytes_total = bytes_total.saturating_add(md.size.unwrap_or(0));
                tasks.push(rp.clone());
            }
            Err(e) => errors.push(format!("{}: 读取远端信息失败: {}", basename(rp), e)),
        }
    }
    let file_count = tasks.len();

    let mut bytes_done: u64 = 0;
    for (i, rp) in tasks.iter().enumerate() {
        let name = basename(rp);
        let local_path = Path::new(local_dest_dir).join(&name);

        let mut last_emit = Instant::now();
        emit_transfer_progress(
            sink, request_id, "download", &name, i, file_count, bytes_done, bytes_total,
        );
        if let Err(e) = download_one(
            &sftp,
            sink,
            request_id,
            rp,
            &local_path.to_string_lossy(),
            i,
            file_count,
            bytes_total,
            &mut bytes_done,
            &mut last_emit,
        )
        .await
        {
            errors.push(e);
        }
    }

    emit_transfer_progress(
        sink, request_id, "download", "", file_count, file_count, bytes_done, bytes_total,
    );
    sink.emit(
        "sftp_transfer_done",
        &TransferDonePayload {
            request_id: request_id.to_string(),
            errors,
        },
    );
    Ok(())
}

async fn download_one(
    sftp: &SftpSession,
    sink: &dyn EventSink,
    request_id: &str,
    remote_path: &str,
    local_path: &str,
    file_index: usize,
    file_count: usize,
    bytes_total: u64,
    bytes_done: &mut u64,
    last_emit: &mut Instant,
) -> Result<(), String> {
    let name = basename(remote_path);
    // open() = READ.
    let mut remote = sftp
        .open(remote_path)
        .await
        .map_err(|e| {
            log::warn!("[sftp] download {} open remote failed: {}", name, e);
            format!("{}: 打开远端文件失败: {}", name, e)
        })?;
    // File::create = CREATE|TRUNCATE|WRITE → overwrite.
    let mut local = tokio::fs::File::create(local_path)
        .await
        .map_err(|e| {
            log::warn!("[sftp] download {} create local failed: {}", name, e);
            format!("{}: 创建本地文件失败: {}", name, e)
        })?;

    let mut buf = vec![0u8; TRANSFER_CHUNK];
    loop {
        let n = remote
            .read(&mut buf)
            .await
            .map_err(|e| {
                log::warn!("[sftp] download {} read failed: {}", name, e);
                format!("{}: 读取失败: {}", name, e)
            })?;
        if n == 0 {
            break;
        }
        local
            .write_all(&buf[..n])
            .await
            .map_err(|e| {
                log::warn!("[sftp] download {} write failed: {}", name, e);
                format!("{}: 写入失败: {}", name, e)
            })?;
        *bytes_done = bytes_done.saturating_add(n as u64);
        if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
            emit_transfer_progress(
                sink, request_id, "download", &name, file_index, file_count, *bytes_done,
                bytes_total,
            );
            *last_emit = Instant::now();
        }
    }
    let _ = local.flush().await;
    // `remote` (SFTP read handle) is closed by its Drop impl on scope exit —
    // a read-only handle has no pending writes to drain.
    Ok(())
}
