use crate::{AppState, FileEntry};
use crate::{EventSink, EventSinkExt};
use russh_sftp::client::SftpSession;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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

/// SFTP 子系统是否可用。真实开一次子系统并完成 INIT 握手，失败即不可用。
///
/// 用于拖拽上传的通道选择：SSH 连得上不等于 SFTP 用得了（restricted shell、
/// chroot、只开 exec 的跳板机都会拒绝 `request_subsystem("sftp")`）。实测
/// 失败信息是 `SFTP session init failed: Timeout`，没有任何下一步提示。
/// 刻意复用 `get_sftp_session` 的同一条路径——任何间接推断都可能与服务端
/// 的真实行为不一致。
pub async fn probe_sftp_subsystem(state: &AppState, session_id: &str) -> Result<(), String> {
    // 握手本身就会失败/超时；拿到 session 后立刻丢掉，不做任何真实操作。
    let _sftp = get_sftp_session(state, session_id).await?;
    Ok(())
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
        // Skip the dot entries. The FTP listing already filters these
        // (`ftp.rs` checks for "." and ".."), and the frontend does not filter
        // either — so any SFTP server whose READDIR returns them showed rows
        // that cannot be acted on: downloading one failed with
        // `非法文件名: "."`, and removing one issued an RMDIR against the
        // current directory. The two protocols simply drifted.
        if name == "." || name == ".." {
            continue;
        }
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

    // Try removing as file first, then as directory. Keep BOTH errors: the old
    // code discarded the file error unconditionally, so a permission-denied
    // file surfaced as the RMDIR failure instead — a message describing an
    // operation that was never intended, and one that hid the real cause.
    if let Err(file_err) = sftp.remove_file(resolved.as_str()).await {
        sftp.remove_dir(resolved.as_str()).await.map_err(|dir_err| {
            format!("Remove failed: 删文件失败({file_err})，删目录失败({dir_err})")
        })?;
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
pub(crate) struct TransferProgressPayload {
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
pub(crate) struct TransferDonePayload {
    pub(crate) request_id: String,
    pub(crate) errors: Vec<String>,
    /// 本次上传在远端创建的目录数（SFTP 拖拽上传的文件夹展开）。
    /// 下载与 FTP 传输恒为 0 —— 前端据此决定完成提示的措辞
    /// （「已创建 N 个目录」/「已上传 M 个文件」）。
    pub(crate) created_dirs: usize,
}

/// Shared with ftp.rs — FTP transfers emit the SAME event contract so the
/// frontend's transfer overlay works unchanged for either protocol.
pub(crate) fn emit_transfer_progress(
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
/// input so the UI still shows *something*. Shared with ftp.rs.
pub(crate) fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// 一个待上传的本地文件。
#[derive(Clone, Debug)]
struct UploadTask {
    local_path: String,
    /// 相对目标目录的 POSIX 路径（如 "sub/deep/c.bin"）。
    /// 远端拼接只用它，绝不让本地 `Path` 的分隔符进入远端路径。
    relative: String,
    size: u64,
}

/// 本地拖入项的展开结果。
#[derive(Debug, Default)]
struct UploadPlan {
    /// 需要创建的远端目录（相对路径，父目录一定排在子目录之前）。
    dirs: Vec<String>,
    files: Vec<UploadTask>,
    errors: Vec<String>,
}

/// 本地递归的深度熔断。
///
/// 这不是产品层数限制：正常目录树想多深就多深。它只为挡住符号链接成环
/// （`a -> ..` 这类软链会让 `std::fs::metadata` 每层都跟回祖先，无限递归）。
/// 与 `ssh.rs::walk_upload_dir`、`MAX_RECURSE_DEPTH` 同值。
const MAX_UPLOAD_DEPTH: usize = 64;

/// 远端绝对目录 + 相对路径 → 绝对路径，统一用 POSIX 分隔符。
fn join_remote(dest: &str, rel: &str) -> String {
    if dest.ends_with('/') {
        format!("{}{}", dest, rel)
    } else {
        format!("{}/{}", dest, rel)
    }
}

/// 把拖入的本地路径展开成「要建的目录」+「要传的文件」。
///
/// 每个拖入项以**自己的名字**成为目标目录下的一个根（`/a/src` → `<dest>/src`），
/// 与单文件上传、ZMODEM 上传、主流客户端一致；不做「保留上级目录」那种
/// 随场景变化的路径规则。
///
/// 同步实现（`std::fs`）：调用方用 `spawn_blocking` 包住整棵树的遍历，
/// 几十万文件的目录不该占住 tokio worker。逻辑纯本地，可单测。
fn plan_upload(paths: &[String]) -> UploadPlan {
    let mut plan = UploadPlan::default();
    for p in paths {
        match std::fs::metadata(p) {
            Ok(md) if md.is_file() => plan.files.push(UploadTask {
                local_path: p.clone(),
                relative: basename(p),
                size: md.len(),
            }),
            Ok(md) if md.is_dir() => {
                let root = Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("folder")
                    .to_string();
                plan.dirs.push(root.clone());
                expand_upload_dir(Path::new(p), &root, &mut plan, 0);
            }
            Ok(_) => plan
                .errors
                .push(format!("{}: 不是文件或目录（已跳过）", basename(p))),
            Err(e) => plan
                .errors
                .push(format!("{}: 读取本地信息失败: {}", basename(p), e)),
        }
    }
    plan
}

/// 递归遍历一个本地目录，把子目录与文件按相对路径压入 `plan`。
///
/// 深度守卫写在这里（真正递归的函数内），不是写在外层入口——下载侧的
/// `expand_dir_recursive` 记过这个教训：守卫放在只进入一次的外层等于从没生效。
fn expand_upload_dir(dir: &Path, rel_prefix: &str, plan: &mut UploadPlan, depth: usize) {
    if depth > MAX_UPLOAD_DEPTH {
        plan.errors.push(format!(
            "{}: 目录层级超过 {} 层，已跳过（可能是符号链接成环）",
            rel_prefix, MAX_UPLOAD_DEPTH
        ));
        return;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            plan.errors.push(format!("{}: 读取目录失败: {}", rel_prefix, e));
            return;
        }
    };
    // 名字排序：跨平台确定性（Windows 的 read_dir 顺序不保证稳定）。
    let mut names: Vec<std::ffi::OsString> =
        rd.filter_map(|e| e.ok().map(|e| e.file_name())).collect();
    names.sort();
    for fname in names {
        let full = dir.join(&fname);
        let disp = fname.to_string_lossy().into_owned();
        let rel = format!("{}/{}", rel_prefix, disp);
        // 跟随符号链接，与 ZMODEM 的 collect_upload_files 一致；环由深度熔断兜住。
        match std::fs::metadata(&full) {
            Ok(md) if md.is_dir() => {
                plan.dirs.push(rel.clone());
                expand_upload_dir(&full, &rel, plan, depth + 1);
            }
            Ok(md) if md.is_file() => plan.files.push(UploadTask {
                local_path: full.to_string_lossy().into_owned(),
                relative: rel,
                size: md.len(),
            }),
            Ok(_) => plan
                .errors
                .push(format!("{}: 不是文件或目录（已跳过）", rel)),
            Err(e) => plan
                .errors
                .push(format!("{}: 读取本地信息失败: {}", rel, e)),
        }
    }
}

/// 找出批次内重名的**目录**。
///
/// 只判目录：同名目录会在远端合并成一棵树（静默的本地数据混合），必须拦；
/// 同名文件是既有的覆盖语义（Q8 已确认保持），单文件/多文件都照旧。
/// 返回名字（用于提示），调用方另有 `duplicate_paths` 做精确剔除。
fn find_duplicate_dirs(entries: &[(String, bool)]) -> Vec<String> {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (name, is_dir) in entries {
        if *is_dir {
            *counts.entry(name.as_str()).or_insert(0) += 1;
        }
    }
    let mut out: Vec<String> = counts
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(name, _)| name.to_string())
        .collect();
    out.sort();
    out
}

/// 拖拽上传前的冲突预检结果（序列化给前端）。
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictReport {
    /// 解析后的远端目标绝对路径（`~` 已展开），上传直接用这个。
    pub resolved_dest: String,
    /// 目标目录是否存在且确实是目录。false 时前端应回退主目录。
    pub dest_exists: bool,
    /// 批次内重名的目录名（提示用）。
    pub duplicate_dirs: Vec<String>,
    /// 批次内重名、需要从本次上传中剔除的本地路径（精确到项）。
    pub duplicate_paths: Vec<String>,
    /// 远端已存在的同名目录名。
    pub existing_dirs: Vec<String>,
}

/// 预检：目标目录是否可用、批次内有无重名目录、远端有无同名目录。
///
/// 纯只读：不建目录、不写文件、不碰 transfer 取消表。任一远端查询失败都
/// 会在 `dest_exists` / 空列表上体现或直接返回 `Err`——调用方据此中止，
/// 而不是在「不知道会不会覆盖」的情况下继续写。
pub async fn check_dir_conflicts(
    state: &AppState,
    session_id: &str,
    local_paths: &[String],
    remote_dest_dir: &str,
) -> Result<ConflictReport, String> {
    let sftp = get_sftp_session(state, session_id).await?;
    let dest = resolve_path(&sftp, remote_dest_dir).await?;

    // 目标目录必须存在且是目录，否则后面每个 create_dir 都会失败。
    // stat 失败（不存在 / 无权限）一律视为不可用，由调用方回退并提示。
    let dest_exists = matches!(sftp.metadata(dest.as_str()).await, Ok(md) if md.is_dir());

    // 本地类型分类。stat 失败的项在这里不报错——plan_upload 稍后会把它
    // 变成一条 per-path error，不重复打扰。
    // (完整路径, basename, 是否目录)
    let mut entries: Vec<(String, String, bool)> = Vec::new();
    for p in local_paths {
        if let Ok(md) = tokio::fs::metadata(p).await {
            entries.push((p.clone(), basename(p), md.is_dir()));
        }
    }

    let dir_flags: Vec<(String, bool)> = entries
        .iter()
        .map(|(_, name, is_dir)| (name.clone(), *is_dir))
        .collect();
    let duplicate_dirs = find_duplicate_dirs(&dir_flags);
    // 只剔除重名**目录**：同名文件是既有的覆盖语义，不能连带删掉。
    let duplicate_paths: Vec<String> = entries
        .iter()
        .filter(|(_, name, is_dir)| *is_dir && duplicate_dirs.contains(name))
        .map(|(path, _, _)| path.clone())
        .collect();

    // 冲突 = 远端已经有同名**目录**。用 stat（跟随符号链接）：指向目录的
    // 软链在用户眼里就是个目录，提示与合并都该按这个事实工作。
    // 同名但类型是文件的远端项不在这里报，交给 create_dir 失败的 per-path 错误。
    let mut existing_dirs: Vec<String> = Vec::new();
    if dest_exists {
        for (_, name, is_dir) in &entries {
            if !*is_dir || duplicate_dirs.contains(name) {
                continue;
            }
            let remote = join_remote(&dest, name);
            if matches!(sftp.metadata(remote).await, Ok(md) if md.is_dir()) {
                existing_dirs.push(name.clone());
            }
        }
    }
    existing_dirs.sort();

    Ok(ConflictReport {
        resolved_dest: dest,
        dest_exists,
        duplicate_dirs,
        duplicate_paths,
        existing_dirs,
    })
}

/// Upload a batch of local files and/or directories into `remote_dest_dir`
/// (resolved through `resolve_path` so `~` works).
///
/// Directories are expanded locally into the tree they should recreate on the
/// remote side: each dropped item becomes `<dest>/<its own name>` and is walked
/// to the leaves. Existing remote directories are treated as already-created
/// (merge semantics); same-named files overwrite, matching the single-file
/// behavior that predates this function.
///
/// Returns `Err` only on fatal failures (no SFTP session); per-path failures
/// land in the `sftp_transfer_done` errors list.
pub async fn upload(
    state: &AppState,
    session_id: &str,
    local_paths: Vec<String>,
    remote_dest_dir: &str,
    request_id: &str,
    sink: &dyn EventSink,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let sftp = get_sftp_session(state, session_id).await?;
    let dest = resolve_path(&sftp, remote_dest_dir).await?;

    // 本地展开放到 blocking 线程：整棵树遍历可能很久，不能占住 tokio worker。
    let UploadPlan {
        dirs,
        files,
        mut errors,
    } = tokio::task::spawn_blocking(move || plan_upload(&local_paths))
        .await
        .map_err(|e| format!("本地目录展开失败: {}", e))?;

    let file_total = files.len();
    let bytes_total: u64 = files.iter().map(|t| t.size).sum();
    let dir_total = dirs.len();

    // ── 建目录阶段 ──────────────────────────────────────────────────
    // `dirs` 是深度优先前序（父目录一定在子目录之前），而 SFTP 的
    // create_dir 不递归——这个顺序是硬约束，不能改成并行。
    //
    // 父目录失败后，它的所有子目录与文件都必然失败。短路整棵子树，
    // 否则一个 3 层目录会刷出几十条重复错误。
    let mut failed_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut created_dirs = 0usize;
    let mut cancelled_in_dirs = false;
    for (i, rel) in dirs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            errors.push("已取消".to_string());
            cancelled_in_dirs = true;
            break;
        }
        if let Some((parent, _)) = rel.rsplit_once('/') {
            if failed_dirs.contains(parent) {
                failed_dirs.insert(rel.clone());
                continue;
            }
        }
        // 目录阶段也发进度：几千个目录时浮层不能一直停在「准备中…」。
        emit_transfer_progress(
            sink,
            request_id,
            "upload",
            &format!("创建目录 {}/{}", i + 1, dir_total),
            i,
            dir_total,
            0,
            0,
        );
        let remote = join_remote(&dest, rel);
        match sftp.create_dir(remote.as_str()).await {
            Ok(()) => created_dirs += 1,
            Err(e) => {
                // 已存在的同名目录是正常情况（重新上传 / 用户选了「合并」）。
                // create_dir 对它一定失败，所以失败后必须 stat 确认，否则
                // 合并路径整棵树都建不起来。
                if matches!(sftp.metadata(remote.as_str()).await, Ok(md) if md.is_dir()) {
                    // 已存在，不算错误也不算新建。
                } else {
                    log::warn!("[sftp] create dir {} failed: {}", rel, e);
                    errors.push(format!("{}: 创建目录失败: {}", rel, e));
                    failed_dirs.insert(rel.clone());
                }
            }
        }
    }

    // ── 传文件阶段 ──────────────────────────────────────────────────
    // 建目录阶段已经取消过就不再进这个循环：否则会再推一条「已取消」。
    let mut bytes_done: u64 = 0;
    let mut skipped_by_failed_dir = 0usize;
    for (i, task) in files.iter().enumerate() {
        if cancelled_in_dirs {
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            errors.push("已取消".to_string());
            break;
        }
        if let Some((parent, _)) = task.relative.rsplit_once('/') {
            if failed_dirs.contains(parent) {
                skipped_by_failed_dir += 1;
                continue;
            }
        }
        let name = basename(&task.local_path);
        let remote = join_remote(&dest, &task.relative);
        let mut last_emit = Instant::now();
        emit_transfer_progress(
            sink, request_id, "upload", &name, i, file_total, bytes_done, bytes_total,
        );
        if let Err(e) = upload_one(
            &sftp, sink, request_id, &task.local_path, &remote, i, file_total, bytes_total,
            &mut bytes_done, &mut last_emit, &cancel,
        )
        .await
        {
            errors.push(e);
            if cancel.load(Ordering::Relaxed) {
                break;
            }
        }
    }
    if skipped_by_failed_dir > 0 {
        errors.push(format!(
            "{} 个文件因父目录创建失败而跳过",
            skipped_by_failed_dir
        ));
    }

    // Final tick so the overlay completes cleanly at 100%.
    emit_transfer_progress(
        sink,
        request_id,
        "upload",
        "",
        file_total,
        file_total,
        bytes_done,
        bytes_total,
    );
    sink.emit(
        "sftp_transfer_done",
        &TransferDonePayload {
            request_id: request_id.to_string(),
            errors,
            created_dirs,
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
    cancel: &Arc<AtomicBool>,
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
        if cancel.load(Ordering::Relaxed) {
            return Err("已取消".to_string());
        }
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
    // A server-side flush failure means the remote never durably stored the
    // data (disk full, quota); the caller would otherwise be told the upload
    // succeeded.
    remote
        .flush()
        .await
        .map_err(|e| format!("{}: 远端落盘失败（磁盘空间或配额?）: {}", name, e))?;
    Ok(())
}

/// A single file to download, produced by expanding the user's selection
/// (plain files + recursive folder walk) into a flat list.
#[derive(Clone)]
struct DownloadTask {
    remote_path: String,
    /// Path relative to the local dest dir ("dir/sub/file.txt").
    relative: String,
    size: u64,
}

/// Recursively expand one selected remote path into download tasks.
/// Files map to `<dest>/<name>`; folders mirror their subtree under
/// `<dest>/<folder-name>/...` (empty folders preserved via `dirs`).
/// Depth-capped: `metadata` follows symlinks, so a symlink cycle looks like
/// infinite nesting — the cap turns it into a per-path error instead of a hang.
async fn expand_download_one(
    sftp: &SftpSession,
    remote_path: &str,
    tasks: &mut Vec<DownloadTask>,
    dirs: &mut Vec<String>,
    errors: &mut Vec<String>,
    depth: usize,
) {
    const MAX_DEPTH: usize = MAX_RECURSE_DEPTH;
    if depth > MAX_DEPTH {
        errors.push(format!(
            "{}: 目录层级过深（>{}，可能存在符号链接循环）",
            basename(remote_path),
            MAX_DEPTH
        ));
        return;
    }
    let md = match sftp.metadata(remote_path).await {
        Ok(md) => md,
        Err(e) => {
            errors.push(format!("{}: 读取远端信息失败: {}", basename(remote_path), e));
            return;
        }
    };
    if !md.is_dir() {
        // The remote name is attacker-controlled data. A POSIX-legal name like
        // `..\evil.txt` would become a Windows path separator client-side and
        // escape the download dir — reject instead of writing.
        let name = basename(remote_path);
        if let Err(reason) = crate::path_safety::validate_component(&name) {
            errors.push(format!("{}: {}", name, reason));
            return;
        }
        tasks.push(DownloadTask {
            remote_path: remote_path.to_string(),
            relative: name,
            size: md.size.unwrap_or(0),
        });
        return;
    }
    let root_name = {
        let n = basename(remote_path);
        if n.is_empty() { "root".to_string() } else { n }
    };
    if let Err(reason) = crate::path_safety::validate_component(&root_name) {
        errors.push(format!("{}: {}", root_name, reason));
        return;
    }
    dirs.push(root_name.clone());
    expand_dir_recursive(sftp, remote_path, &root_name, tasks, dirs, errors, depth).await;
}

/// Split a "/"-joined relative task path into validated components and build
/// the local absolute path under `dest`. Component-wise construction means
/// the result cannot escape `dest` — no remote-derived string is ever handed
/// to `Path::join`.
fn local_components(dest: &str, relative: &str) -> Result<std::path::PathBuf, String> {
    let parts: Vec<&str> = relative.split('/').collect();
    for p in &parts {
        crate::path_safety::validate_component(p)?;
    }
    Ok(crate::path_safety::build_path(Path::new(dest), &parts))
}

/// Maximum directory depth for a recursive download.
///
/// Module-level so BOTH the single-entry expansion and the recursive walker
/// enforce the SAME cap — they had drifted, and the check that lived only in
/// the single-entry path never guarded the recursion.
const MAX_RECURSE_DEPTH: usize = 64;

/// Walk one remote folder level, appending file tasks and relative sub-dirs.
async fn expand_dir_recursive(
    sftp: &SftpSession,
    remote_dir: &str,
    rel_prefix: &str,
    tasks: &mut Vec<DownloadTask>,
    dirs: &mut Vec<String>,
    errors: &mut Vec<String>,
    depth: usize,
) {
    // The depth guard belongs HERE, in the function that actually recurses.
    // It used to sit in `expand_download_one`, which is only entered once per
    // top-level selection and always with depth = 0 — so the cap never applied
    // to the recursive walk at all. A remote symlink pointing at an ancestor
    // (or simply a very deep tree) therefore recursed without bound, flooding
    // READDIR requests and memory. The FTP sibling checks at the top of its
    // recursive `expand_download` (ftp.rs) and has always been correct; the two
    // walkers had drifted apart.
    if depth > MAX_RECURSE_DEPTH {
        errors.push(format!(
            "{}: 目录层级超过 {} 层，已跳过（可能是符号链接成环）",
            rel_prefix, MAX_RECURSE_DEPTH
        ));
        return;
    }
    let entries = match sftp.read_dir(remote_dir).await {
        Ok(e) => e,
        Err(e) => {
            errors.push(format!("{}: 读取目录失败: {}", rel_prefix, e));
            return;
        }
    };
    for entry in entries {
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        // Reject names that can't form ONE local filename component (the
        // expand-download root validates the same way).
        if let Err(reason) = crate::path_safety::validate_component(&name) {
            errors.push(format!("{}/{}: {}", rel_prefix, name, reason));
            continue;
        }
        let full = if remote_dir.ends_with('/') {
            format!("{}{}", remote_dir, name)
        } else {
            format!("{}/{}", remote_dir, name)
        };
        let rel = format!("{}/{}", rel_prefix, name);
        // DirEntry attrs ride along with the listing (no extra round trip).
        let md = entry.metadata();
        if md.is_dir() {
            dirs.push(rel.clone());
            Box::pin(expand_dir_recursive(
                sftp, &full, &rel, tasks, dirs, errors, depth + 1,
            ))
            .await;
        } else {
            tasks.push(DownloadTask {
                remote_path: full,
                relative: rel,
                size: md.size.unwrap_or(0),
            });
        }
    }
}

/// Download a batch of remote files/folders into `local_dest_dir`.
/// Folders are expanded recursively (their subtree lands under
/// `<dest>/<folder-name>/...`); files download concurrently with
/// `concurrency` workers sharing one SFTP session (russh-sftp multiplexes
/// by request id, so parallel handles over one channel are protocol-legal).
/// Overwrites local files. Per-file failures land in the
/// `sftp_transfer_done` errors list.
pub async fn download(
    state: &AppState,
    session_id: &str,
    remote_paths: Vec<String>,
    local_dest_dir: &str,
    request_id: &str,
    concurrency: usize,
    sink: Arc<dyn EventSink>,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    // One shared session: RawSftpSession is Send+Sync (SendWrapper) and
    // matches replies by request id, so N concurrent file reads multiplex
    // over a single channel — no per-worker handshake cost.
    let sftp = Arc::new(get_sftp_session(state, session_id).await?);
    // Local destination — created if missing (covers a freshly-typed path).
    tokio::fs::create_dir_all(local_dest_dir)
        .await
        .map_err(|e| format!("创建本地目录失败: {}", e))?;

    // Phase 1: expand selection (recursive walk) into a flat file list.
    let mut errors: Vec<String> = Vec::new();
    let mut tasks: Vec<DownloadTask> = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    for rp in &remote_paths {
        expand_download_one(&sftp, rp, &mut tasks, &mut dirs, &mut errors, 0).await;
    }
    // Materialize the folder skeleton before transferring so files always
    // have a parent to land in — and empty folders survive the copy.
    // Paths are rebuilt from validated components (never `join`ed from remote
    // strings) and checked for symlinks BEFORE create_dir_all, so an
    // attacker-planted link can't redirect the directory creation.
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
    let file_count = tasks.len();
    let bytes_total = tasks.iter().fold(0u64, |acc, t| acc.saturating_add(t.size));

    // Phase 2: worker pool. Cursor + shared task list — no channel needed;
    // fetch_add hands each task to exactly one worker.
    let tasks = Arc::new(tasks);
    let cursor = Arc::new(AtomicUsize::new(0));
    let bytes_done = Arc::new(AtomicU64::new(0));
    let files_done = Arc::new(AtomicUsize::new(0));
    let worker_errors = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let workers = concurrency.clamp(1, 16).min(file_count.max(1));
    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let sftp = Arc::clone(&sftp);
        let sink = Arc::clone(&sink);
        let cancel = Arc::clone(&cancel);
        let tasks = Arc::clone(&tasks);
        let cursor = Arc::clone(&cursor);
        let bytes_done = Arc::clone(&bytes_done);
        let files_done = Arc::clone(&files_done);
        let worker_errors = Arc::clone(&worker_errors);
        let dest = local_dest_dir.to_string();
        let rid = request_id.to_string();
        handles.push(tokio::spawn(async move {
            let mut last_emit = Instant::now();
            loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let i = cursor.fetch_add(1, Ordering::Relaxed);
                if i >= tasks.len() {
                    break;
                }
                let task = tasks[i].clone();
                // Rebuild the local path from validated components — never
                // `Path::join` a remote-derived string (backslash trap).
                let local_path = match local_components(&dest, &task.relative) {
                    Ok(p) => p,
                    Err(reason) => {
                        if let Ok(mut g) = worker_errors.lock() {
                            g.push(format!("{}: {}", task.relative, reason));
                        }
                        continue;
                    }
                };
                // Progress slot = files completed so far — a monotonic
                // counter that reads naturally when files run in parallel.
                let slot = files_done.load(Ordering::Relaxed);
                match download_one(
                    &sftp,
                    &*sink,
                    &rid,
                    &task.remote_path,
                    &local_path.to_string_lossy(),
                    slot,
                    file_count,
                    bytes_total,
                    &bytes_done,
                    &mut last_emit,
                    &cancel,
                )
                .await
                {
                    Ok(()) => {
                        files_done.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        if let Ok(mut g) = worker_errors.lock() {
                            g.push(e);
                        }
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                }
            }
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    if let Ok(g) = worker_errors.lock() {
        errors.extend(g.iter().cloned());
    }
    if cancel.load(Ordering::Relaxed) {
        errors.push("已取消".to_string());
    }

    let bytes_done = bytes_done.load(Ordering::Relaxed);
    emit_transfer_progress(
        &*sink, request_id, "download", "", file_count, file_count, bytes_done, bytes_total,
    );
    sink.emit(
        "sftp_transfer_done",
        &TransferDonePayload {
            request_id: request_id.to_string(),
            errors,
            // 下载不建远端目录。
            created_dirs: 0,
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
    bytes_done: &AtomicU64,
    last_emit: &mut Instant,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    let name = basename(remote_path);
    // A pre-planted local symlink (leaf or any ancestor) must not redirect
    // the write outside the destination — check before open/create.
    if let Err(e) = crate::path_safety::ensure_no_symlink_components(Path::new(local_path)).await {
        log::warn!("[sftp] download {} blocked: {}", name, e);
        return Err(format!("{}: {}", name, e));
    }
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
        if cancel.load(Ordering::Relaxed) {
            return Err("已取消".to_string());
        }
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
        let done_now = bytes_done.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
        if last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
            emit_transfer_progress(
                sink, request_id, "download", &name, file_index, file_count, done_now,
                bytes_total,
            );
            *last_emit = Instant::now();
        }
    }
    // ENOSPC / quota failures surface HERE, at flush, not during write_all.
    // Discarding this Result ended the transfer with `errors` empty and the
    // progress bar at 100% while the file on disk was short.
    local
        .flush()
        .await
        .map_err(|e| format!("{}: 落盘失败（磁盘空间不足?）: {}", name, e))?;
    // `remote` (SFTP read handle) is closed by its Drop impl on scope exit —
    // a read-only handle has no pending writes to drain.
    Ok(())
}

#[cfg(test)]
mod upload_plan_tests {
    use super::{find_duplicate_dirs, plan_upload};

    /// 每个用例独立的临时根目录（按 tag 区分，跑完清理）。
    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "myshell_upload_plan_{}_{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(root: &std::path::Path) {
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn nested_tree_keeps_relative_paths_and_empty_dirs() {
        let root = temp_root("nested");
        std::fs::write(root.join("a.txt"), b"aaa").unwrap();
        std::fs::create_dir_all(root.join("sub").join("deep")).unwrap();
        std::fs::write(root.join("sub").join("b.txt"), b"bb").unwrap();
        std::fs::write(root.join("sub").join("deep").join("c.bin"), b"cccc").unwrap();
        std::fs::create_dir_all(root.join("empty")).unwrap();
        // 三层全空的目录链：三层都必须在 dirs 里，否则远端会缺层。
        std::fs::create_dir_all(root.join("e1").join("e2").join("e3")).unwrap();

        let plan = plan_upload(&[root.to_string_lossy().to_string()]);
        assert!(plan.errors.is_empty(), "不应有错误: {:?}", plan.errors);

        let name = root.file_name().unwrap().to_string_lossy().to_string();

        // 文件的相对路径一律 POSIX 分隔，且保留层级。
        let files: Vec<String> = plan.files.iter().map(|f| f.relative.clone()).collect();
        assert_eq!(
            files,
            vec![
                format!("{}/a.txt", name),
                format!("{}/sub/b.txt", name),
                format!("{}/sub/deep/c.bin", name),
            ]
        );
        // bytes_total 由调用方求和，这里核对单文件尺寸来源正确。
        let total: u64 = plan.files.iter().map(|f| f.size).sum();
        assert_eq!(total, 3 + 2 + 4);

        // 父目录一定排在子目录之前（create_dir 不递归，这是硬约束）。
        for (i, rel) in plan.dirs.iter().enumerate() {
            if let Some((parent, _)) = rel.rsplit_once('/') {
                let parent_pos = plan
                    .dirs
                    .iter()
                    .position(|d| d == parent)
                    .unwrap_or_else(|| panic!("目录 {} 的父目录 {} 不在 dirs 里", rel, parent));
                assert!(parent_pos < i, "父目录 {} 必须排在 {} 之前", parent, rel);
            }
        }
        // 空目录必须出现（否则远端重建出来会缺目录）。
        assert!(plan.dirs.iter().any(|d| d == &format!("{}/empty", name)));
        assert!(plan.dirs.iter().any(|d| d == &format!("{}/e1/e2/e3", name)));
        // 根目录自身也要建。
        assert_eq!(plan.dirs.first().map(String::as_str), Some(name.as_str()));

        cleanup(&root);
    }

    #[test]
    fn top_level_file_relative_is_just_its_name() {
        let root = temp_root("flat");
        let file = root.join("only.txt");
        std::fs::write(&file, b"x").unwrap();

        let plan = plan_upload(&[file.to_string_lossy().to_string()]);
        assert!(plan.errors.is_empty(), "{:?}", plan.errors);
        assert!(plan.dirs.is_empty());
        assert_eq!(plan.files.len(), 1);
        // 不能把本地的上级目录泄漏进远端路径。
        assert_eq!(plan.files[0].relative, "only.txt");
        assert_eq!(plan.files[0].size, 1);

        cleanup(&root);
    }

    #[test]
    fn missing_path_is_a_per_item_error_not_a_failure() {
        let plan = plan_upload(&["Z:\\definitely\\not\\here.abc".to_string()]);
        assert!(plan.files.is_empty());
        assert!(plan.dirs.is_empty());
        assert_eq!(plan.errors.len(), 1);
    }

    #[test]
    fn duplicate_dirs_detected_only_for_directories() {
        let entries = vec![
            ("src".to_string(), true),
            ("src".to_string(), true),
            ("README.md".to_string(), false),
            ("README.md".to_string(), false),
            ("only".to_string(), true),
        ];
        // 同名文件走既有覆盖语义，不拦；只报同名目录。
        assert_eq!(find_duplicate_dirs(&entries), vec!["src".to_string()]);
    }

    #[test]
    fn duplicate_dirs_ignores_unique_names() {
        let entries = vec![
            ("a".to_string(), true),
            ("b".to_string(), true),
            ("a.txt".to_string(), false),
        ];
        assert!(find_duplicate_dirs(&entries).is_empty());
    }
}

#[cfg(test)]
mod upload_conflict_tests {
    use super::{find_duplicate_dirs, plan_upload};

    /// 回归守卫：同名目录 + 同名文件混合时的剔除名单。
    ///
    /// 修前 bug：`check_dir_conflicts` 用「basename 命中 duplicate_dirs」筛
    /// 剔除名单，会把同名的**文件**一起删掉（`/x/README.md` 与 `/x/src`、
    /// `/y/src` 同时拖入时 README.md 被误剔）。修后只剔目录。
    #[test]
    fn duplicate_pruning_keeps_same_named_files() {
        let root = std::env::temp_dir().join("myshell_upload_conflict_prune");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("x").join("src")).unwrap();
        std::fs::create_dir_all(root.join("y").join("src")).unwrap();
        std::fs::write(root.join("x").join("README.md"), b"a").unwrap();

        let paths: Vec<String> = [
            root.join("x").join("src"),
            root.join("y").join("src"),
            root.join("x").join("README.md"),
        ]
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

        // 与 check_dir_conflicts 相同的本地分类逻辑。
        let mut entries: Vec<(String, String, bool)> = Vec::new();
        for p in &paths {
            if let Ok(md) = std::fs::metadata(p) {
                let name = super::basename(p);
                entries.push((p.clone(), name, md.is_dir()));
            }
        }
        let dir_flags: Vec<(String, bool)> = entries
            .iter()
            .map(|(_, name, is_dir)| (name.clone(), *is_dir))
            .collect();
        let duplicate_dirs = find_duplicate_dirs(&dir_flags);
        let duplicate_paths: Vec<String> = entries
            .iter()
            .filter(|(_, name, is_dir)| *is_dir && duplicate_dirs.contains(name))
            .map(|(path, _, _)| path.clone())
            .collect();

        assert_eq!(duplicate_dirs, vec!["src".to_string()]);
        // 只有两个目录被剔除，同名文件必须留下。
        assert_eq!(duplicate_paths.len(), 2);
        assert!(duplicate_paths.iter().all(|p| p.ends_with("src")));
        assert!(!duplicate_paths.iter().any(|p| p.ends_with("README.md")));

        // 剔除后仍应有一个文件要传。
        let remaining: Vec<&String> = paths
            .iter()
            .filter(|p| !duplicate_paths.contains(p))
            .collect();
        assert_eq!(remaining.len(), 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 回归守卫：顶层文件与顶层目录的 relative 语义不同。
    ///
    /// 修前 bug 的对照面：文件用 basename（不带任何目录），目录则要建出根
    /// 目录本身。这条锁住「别为了统一把文件的 relative 也加上父目录」。
    #[test]
    fn file_and_dir_roots_have_different_shapes() {
        let root = std::env::temp_dir().join("myshell_upload_conflict_shapes");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("proj")).unwrap();
        std::fs::write(root.join("proj").join("main.rs"), b"fn main(){}").unwrap();
        std::fs::write(root.join("top.txt"), b"t").unwrap();

        let plan = plan_upload(&[
            root.join("proj").to_string_lossy().to_string(),
            root.join("top.txt").to_string_lossy().to_string(),
        ]);
        assert!(plan.errors.is_empty(), "{:?}", plan.errors);

        // 拖入的目录项以「自己的名字」为根，不带上层路径。
        assert_eq!(plan.dirs, vec!["proj".to_string()]);
        let files: Vec<&str> = plan.files.iter().map(|f| f.relative.as_str()).collect();
        assert_eq!(files, vec!["proj/main.rs", "top.txt"]);

        let _ = std::fs::remove_dir_all(&root);
    }
}
