// MyShell MCP Server — exposes SSH/SFTP operations as MCP tools for AI agents
// (Claude Desktop, Cursor, ZCode, etc.) via the Model Context Protocol.
//
// Transport: stdio (Content-Length framed JSON-RPC 2.0, same as LSP).
// Auth: NONE — the MCP server does NOT hold the vault passphrase or DEK.
// All credential access is delegated to the GUI: ssh_exec runs in a GUI
// terminal tab, SFTP tools ask the GUI to decrypt connection credentials
// via IPC. If the GUI is not running, the MCP server AUTO-LAUNCHES it.
// Every tool (except the task-status trio) is gated behind the GUI vault:
// locked vault → immediate error + the GUI window is focused so the user
// can type the master password. No silent waiting anywhere.
//
// Configuration example (Claude Desktop claude_desktop_config.json):
//   { "mcpServers": { "myshell": {
//       "command": "myshell-mcp"
//   }}}

use myshell_core::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

// ============ Human confirmation for dangerous operations ============
//
// MCP is driven by AI agents — they can request destructive operations
// (rm -rf,-format, overwrite files, etc.). We MUST get an explicit human
// "yes" via a native OS dialog before executing any high-risk tool. The AI
// cannot bypass this: clicking Cancel / closing the dialog returns an error
/// Pop a Windows MessageBox asking the user to confirm a dangerous operation.
/// Returns true only when the user explicitly clicks Yes.
#[cfg(windows)]
fn confirm_dangerous_operation(tool: &str, detail: &str) -> bool {
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;

    let title: Vec<u16> = OsStr::new("MyShell MCP — 高危操作确认")
        .encode_wide()
        .chain(once(0))
        .collect();
    let message = format!(
        "AI 尝试执行高危操作：{}\n\n详情：{}\n\n点击「确认」允许执行，点击「取消」拒绝。",
        tool, detail
    );
    let message_w: Vec<u16> = OsStr::new(&message)
        .encode_wide()
        .chain(once(0))
        .collect();

    unsafe {
        winapi::um::winuser::MessageBoxW(
            std::ptr::null_mut(),
            message_w.as_ptr(),
            title.as_ptr(),
            winapi::um::winuser::MB_YESNO
                | winapi::um::winuser::MB_ICONWARNING
                | winapi::um::winuser::MB_SYSTEMMODAL
                | winapi::um::winuser::MB_SETFOREGROUND,
        ) == winapi::um::winuser::IDYES
    }
}

#[cfg(not(windows))]
fn confirm_dangerous_operation(_tool: &str, _detail: &str) -> bool {
    // Non-Windows: log and deny (MCP server is Windows-only for now)
    log("高危操作确认需要 Windows 环境，已拒绝");
    false
}

/// Tool-result text returned when the user DENIES a dangerous operation
/// (GUI React dialog or headless MessageBoxW). A denial is a HARD STOP by
/// project policy: this text is a direct instruction to the AI agent —
/// halt the whole task (no retry, no workaround), summarize what was in
/// flight, and hand the continue/abort decision back to the human.
fn denied_by_user_text(tool: &str, detail: &str) -> String {
    format!(
        "⛔ 用户已拒绝高危操作：{}（{}）。\n\n\
         【硬性停止】请立即停止当前任务的所有后续操作：不要重试该命令，\
         不要改用其他工具、路径或连接绕过，也不要继续任务中的下一步。\n\
         请立即做两件事：\n\
         1. 向用户输出当前任务说明：你正在执行什么任务、已完成哪些步骤、\
         被拒绝的是哪一步、剩余计划是什么；\n\
         2. 停止并等待用户明确答复。只有用户明确表示继续后才可以恢复操作，\
         用户答复前不要发起任何 MCP 调用。",
        tool, detail
    )
}

// ============ MCP Protocol (JSON-RPC 2.0 over stdio, Content-Length framed) ============

/// Read one JSON-RPC message from stdin.
///
/// The MCP stdio spec (2025-06-18) mandates **newline-delimited JSON** — one
/// JSON-RPC message per line. However, older clients (notably Claude Desktop
/// and some early SDKs) send **LSP-style `Content-Length: <n>\r\n\r\n<bytes>`
/// framing**. To stay compatible with both, we peek at the first non-empty
/// line: if it looks like a Content-Length header, switch into framed mode
/// for this message; otherwise parse the whole line as NDJSON.
fn read_message_raw(stdin: &mut impl BufRead) -> Option<Value> {
    loop {
        let mut line = String::new();
        let n = match stdin.read_line(&mut line) {
            Ok(n) => n,
            Err(e) => {
                log(&format!("read_line error: {}", e));
                return None;
            }
        };
        if n == 0 {
            log("read_line returned 0 (EOF on stdin)");
            return None;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        // Skip keep-alive / blank lines (NDJSON separators, or the blank line
        // after a Content-Length header that never came).
        if trimmed.is_empty() {
            continue;
        }
        // LSP-style framing: `Content-Length: <n>` (case-insensitive).
        if let Some(len_str) = trimmed.to_lowercase().strip_prefix("content-length:") {
            let len: usize = len_str.trim().parse().unwrap_or(0);
            if len == 0 {
                continue;
            }
            // Consume the `\r\n\r\n` separator (we already ate one `\r\n`;
            // there may be a single empty line next).
            let mut sep = String::new();
            let _ = stdin.read_line(&mut sep);
            let mut body = vec![0u8; len];
            if stdin.read_exact(&mut body).is_err() {
                log("read_exact failed for body");
                return None;
            }
            return match serde_json::from_slice(&body) {
                Ok(v) => Some(v),
                Err(e) => {
                    log(&format!("JSON parse failed (framed): {}", e));
                    None
                }
            };
        }
        // NDJSON: one JSON object per line.
        return match serde_json::from_str(trimmed) {
            Ok(v) => Some(v),
            Err(e) => {
                log(&format!("JSON parse failed (ndjson): {} — line: {}", e, trimmed.chars().take(120).collect::<String>()));
                // Keep reading the next line instead of dying — one bad line
                // shouldn't take down the whole session.
                continue;
            }
        };
    }
}

/// Write one message. Always flushes synchronously — MCP clients (notably
/// ZCode Desktop) wait on the response to `initialize` with a 30s timeout,
/// and any buffering here translates directly into client-side timeouts.
///
/// We emit **newline-delimited JSON** (the MCP 2025-06-18 spec format). Older
/// clients that framed with `Content-Length` headers will still accept NDJSON
/// on read because they also handle line-based input, and every modern MCP
/// client (opencode, ZCode, Cursor, Claude Code ≥ 1.0) now uses NDJSON.
fn write_message_raw(stdout: &mut impl Write, msg: &Value) {
    let body = serde_json::to_string(msg).unwrap_or_else(|_| "{}".to_string());
    // NDJSON: one JSON object followed by a single `\n`. No `\r` (spec says
    // messages must not contain embedded newlines, and the line terminator
    // is just `\n`).
    if write!(stdout, "{}\n", body).is_err() {
        log("write body failed");
        std::process::exit(0);
    }
    if stdout.flush().is_err() {
        log("flush failed");
        std::process::exit(0);
    }
}

fn jsonrpc_response(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn jsonrpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

// ============ Tool definitions ============
//
// Each `description` is the primary signal an AI agent uses to decide *when*
// to invoke a tool. Write them as if explaining to a junior engineer:
//   - WHEN to use this tool (and, crucially, when NOT to)
//   - WHAT the input means (connection name format, path conventions)
//   - WHAT the output looks like
//   - SIDE EFFECTS / dangers (anything the agent should flag to the user)

/// Shared description for the `connection` parameter across all tools.
/// Explains the three accepted forms (name / group-path / host-IP) and the
/// automatic conn_type disambiguation so the agent doesn't have to guess.
const CONNECTION_PARAM_DESC: &str = "How to identify the target connection. Accepts three forms:\n\
1. The connection `name` exactly as shown in list_connections (e.g. 'prod-db').\n\
2. The group-prefixed path (e.g. '/production/prod-db') — use this when the same name exists in multiple groups.\n\
3. The bare host or IP (e.g. '135.32.64.30' or 'nas.example.com') — useful when the user said 'ssh to <ip>'. MyShell will look it up automatically.\n\
\n\
DISAMBIGUATION: if multiple saved connections match the same name/host, this tool automatically filters by its expected type (ssh_exec→ssh, sftp_*→sftp). If STILL ambiguous (e.g. two ssh connections with the same host in different groups), the tool returns an error listing the candidates — surface them to the user and let them pick by re-calling with the full group-path form.";

/// Global instructions sent to the AI client in the `initialize` response
/// (MCP 2025-06-18). The client prepends this to the system prompt / tool
/// preamble so the agent understands *when* these tools are relevant at all.
///
/// Keep it short and directive — agents ignore long essays. Lead with the
/// trigger conditions (what user intent maps to myshell), then the workflow,
/// then the safety model.
const SERVER_INSTRUCTIONS: &str = "\
MyShell MCP — SSH/SFTP gateway to this user's pre-saved server connections.\n\
\n\
USE THESE TOOLS WHEN: the user references a remote server by name/nickname/alias/IP ('on prod-db', 'check web1', 'ssh to 135.32.64.30', 'restart nginx on the api box'), or asks to run a command / read a file / upload a file / list files on a remote host they've previously saved in MyShell. \
The connection details (host, port, credentials) are stored encrypted in MyShell — you never need and never will receive passwords. Pass the connection NAME or group-path. IP-based lookup requires the GUI to be running and the vault unlocked (IPs are encrypted).\n\
\n\
DO NOT USE THESE TOOLS WHEN: the user wants to run a command locally on their own machine (use your built-in shell tool instead), or references a server they haven't saved in MyShell (suggest they add it in the MyShell GUI first).\n\
\n\
WORKFLOW:\n\
1. If the user gave an unambiguous name or IP you trust, skip to step 2. If unsure, call `list_connections` first to see what's saved (there may be group prefixes like '/production/prod-db').\n\
2. Pass the name / group-path / host to `ssh_exec` / `sftp_list` / etc. The tool auto-disambiguates by type: ssh_exec prefers ssh connections, sftp_* prefers sftp.\n\
3. For `ssh_exec`: each invocation is a fresh non-interactive session — no cwd/memory between calls. Use absolute paths or chain commands with && if state matters.\n\
\n\
SAFETY: ssh_exec uses a **configurable whitelist/blacklist** to decide which commands need human confirmation. Read-only commands (ps, ls, cat, grep, df, ...) run WITHOUT a dialog. Dangerous commands (rm, kill, sudo, shutdown, write-redirects, pipe-to-shell, ...) trigger a NATIVE OS confirmation dialog the USER must click — you cannot bypass it. Before calling a dangerous command, briefly tell the user a dialog is coming. If they click Cancel, the tool returns a HARD STOP error ('⛔ 用户已拒绝高危操作'): immediately stop ALL further operations — do NOT retry the command, do NOT switch tools/paths/connections to work around it, do NOT continue to the next step of the task. Instead, output a summary of the current task (what you were doing, which steps are done, which step was rejected, what remains) and WAIT for the user to explicitly decide whether to continue. The sftp tools (sftp_upload/remove/rename) ALWAYS confirm regardless of rules, and the same hard-stop policy applies to every confirmation dialog.\n\
\n\
FILE TRANSFER (strict two-step policy):\n\
1. ALWAYS try `sftp_download`/`sftp_upload` FIRST for any file upload/download request. It is faster and more reliable.\n\
2. If the sftp tool FAILS for ANY reason, fall back to `zmodem_download`/`zmodem_upload` (ZMODEM over the remote `sz`/`rz` programs). Common failure reasons: the connection is saved as SSH type (no SFTP), the SFTP subsystem is disabled, restricted/chroot shells, jump hosts (堡垒机), embedded devices, or the remote refuses the SFTP channel. Tell the user: 'sftp 失败，改用 ZMODEM (lrzsz) 方式传输'.\n\
3. NEVER use `ssh_exec` to transfer file content via base64/echo/heredoc/cat workaround — it is fragile, slow, and breaks on binary/large files. If both sftp and zmodem fail, tell the user and stop; do NOT invent a workaround.\n\
\n\
VAULT GATE (applies to EVERY tool except ssh_status/ssh_cancel/zmodem_status): MyShell stores all connection credentials in an encrypted vault. If the MyShell GUI isn't running, the tool AUTO-LAUNCHES it. If the vault is locked, the tool FAILS IMMEDIATELY (no waiting) with '保险库未解锁' and brings the MyShell window to the front. When you see this error: tell the user to enter their master password in the MyShell window, then RETRY the same tool — do NOT switch to a different approach. The sftp → zmodem fallback only applies when the failure is about SFTP availability, NOT about the vault.\n\
\n\
ZMODEM IS ASYNC: `zmodem_upload`/`zmodem_download` return IMMEDIATELY with a `task_id` (status=`running`); the transfer (including GUI launch, vault-unlock wait, and the confirmation dialog) runs in the background. You MUST then poll `zmodem_status` with the `task_id` every 5-10 seconds until `status` is `done` or `failed`. Do not report success until you see `done`.\n\
\n\
ENCOUNTERING ERRORS: '保险库未解锁' means the user must enter their master password in the (now foregrounded) MyShell window — tell them, wait for them to confirm, then RETRY the same tool. '保险库尚未初始化' means first-time setup is needed in the GUI. Both are instant errors by design: the tools never hang waiting for the vault.";

fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "list_connections",
                "description": "List all SSH/SFTP/FTP connections saved in this user's MyShell client.\n\nWHEN TO USE: Always call this FIRST when the user mentions a remote server by name, nickname, alias, OR IP address — even if you think you know the name. Call this when the user asks 'what servers do I have', 'show my connections', or wants to know if a specific host is already configured. Knowing the full list helps you pick the right `connection` value for other tools.\n\nWHEN NOT TO USE: Don't use this for connections the user is defining on the fly (e.g. 'ssh to user@1.2.3.4'). MyShell tools only work with pre-saved connections.\n\nOUTPUT: JSON array, each item has `name`, `conn_type` (ssh/sftp/ftp/local), `group_path`. For security, host/port/username are NOT included - they are encrypted.\n\nIMPORTANT: other tools accept the connection `name` or the group-prefixed path ('/production/prod-db'). IP-based lookup requires the vault unlocked (IPs are encrypted). Passwords are never included, for security.\n\nVAULT REQUIRED: this call FAILS INSTANTLY with '保险库未解锁' when the MyShell vault is locked (the MyShell window is brought to the front for the user) — tell the user to enter their master password, then retry. The GUI auto-launches when it isn't running.",
                "inputSchema": { "type": "object", "properties": {}, "required": [] }
            },
            {
                "name": "ssh_exec",
                "description": "Run a shell command on a remote SSH server (non-interactive, one-shot). Returns stdout, stderr, and the process exit code.\n\nWHEN TO USE: The user wants to run a command on a remote server they've saved in MyShell — e.g. 'check disk usage on prod-db', 'restart nginx on web1', 'tail the log on api-server'. Prefer this over opening an interactive shell when the task is a single command with a defined end.\n\nWHEN NOT TO USE:\n- Interactive sessions (top, vim, less, mysql prompt) — this tool times out and won't stream output. Suggest the user run these in MyShell's GUI terminal instead.\n- Operations on a server NOT saved in MyShell — this tool can only reach pre-saved connections.\n- Operations the agent should do locally (read/write local files, run local commands) — use your own tools for those.\n- Long-running tasks (>5min): `apt upgrade`, `git clone` of large repos, `docker pull` of big images, full `npm install`, big `tar`/`rsync`. For these, use `ssh_run` instead — it returns a task_id immediately and you poll `ssh_status` to follow progress. `ssh_exec` is the wrong tool here because even max `timeout=3600` can still hit the call's overall budget on very long jobs.\n\n⚠️ HUMAN CONFIRMATION REQUIRED: A native OS dialog pops up asking the user to approve. The command WILL NOT run until the user clicks 'Yes'. This is by design — AI-initiated remote execution is dangerous. Tell the user a confirmation dialog is coming. If they click Cancel the tool returns a HARD STOP error: stop the whole task, summarize progress for the user, and wait for their explicit decision to continue (see SAFETY).\n\nOUTPUT: JSON `{exit_code, stdout, stderr}`. `exit_code` is 0 on success. stdout/stderr are truncated at 4MB each (silently — if you suspect truncation, redirect to a file on the server and `sftp_download` it instead). Commands that don't exit within `timeout` seconds return an error.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "command": { "type": "string", "description": "Shell command to run. Executed via `bash -c` semantics on the remote. Avoid commands that read from stdin or wait for input." },
                        "timeout": { "type": "integer", "description": "Max seconds to wait before killing the command (default 60, max 3600=1h). For tasks longer than ~5min, prefer `ssh_run` + `ssh_status` polling instead of raising this.", "default": 60, "minimum": 1, "maximum": 3600 }
                    },
                    "required": ["connection", "command"]
                }
            },
            {
                "name": "ssh_run",
                "description": "Run a shell command on a remote SSH server asynchronously (returns a task_id immediately, then poll `ssh_status` to follow progress). Designed for long-running tasks where blocking the conversation for minutes is impractical.\n\nWHEN TO USE:\n- Commands expected to take >1min: `apt upgrade`, `apt install`, `docker pull`, `docker compose up`, `git clone` of large repos, `npm/pnpm install`, `tar`/`rsync` of large trees, full system backups, long-running `find`/`grep`.\n- Any command where you'd otherwise need to set `ssh_exec` timeout >120s.\n\nWHEN NOT TO USE:\n- Quick status checks — use `ssh_exec` (synchronous, returns immediately on completion).\n- Interactive sessions — same as `ssh_exec`, doesn't stream.\n- Tasks you want the user to watch live — open a GUI terminal tab instead via `open_in_gui`.\n\nWORKFLOW:\n1. Call `ssh_run` → returns `{ task_id, status: \"started\" }` immediately (well under 1s, the actual command kicks off in background).\n2. Poll `ssh_status` with the same `task_id` every 10-30s. Each call returns the current phase + accumulated stdout/stderr so far (last 4MB each; full output is preserved on the server — `sftp_download` the log file if you need more).\n3. When `status` becomes `done` or `failed`, the task is finished and the final `exit_code` + full output is in the response.\n4. Tasks are kept in memory for 1h after completion, then auto-cleaned. Stale `task_id`s (older than 1h) return an error.\n\n⚠️ HUMAN CONFIRMATION REQUIRED (same as `ssh_exec`): A native OS dialog pops up before the command starts. The user MUST click 'Yes' for the command to run.\n\nOUTPUT on start: `{ task_id, status: \"started\", description: \"<conn>: <cmd prefix>\" }`. On poll: see `ssh_status` description.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "command": { "type": "string", "description": "Shell command to run. Executed via `bash -c` semantics on the remote. Avoid commands that read from stdin or wait for input. For long tasks, consider redirecting output to a log file (`> /tmp/run.log 2>&1`) and then `sftp_download` it after — keeps the live output small." }
                    },
                    "required": ["connection", "command"]
                }
            },
            {
                "name": "ssh_status",
                "description": "Poll the status of a command launched by `ssh_run`. Returns current phase, accumulated stdout/stderr (last 4MB each), and the task_id.\n\nPHASES:\n- `Confirming`: waiting for the user to click 'Yes' on the OS confirmation dialog. If stuck here, remind the user to check for the dialog.\n- `Connecting`: SSH handshake + auth in progress.\n- `Running`: command is executing on the remote.\n- `Done`: finished successfully. `exit_code` is 0 (usually).\n- `Failed`: command exited non-zero, the connection died, the OS confirmation was cancelled, or the user denied the dialog. `error` field explains. If the error contains '⛔ 用户已拒绝高危操作', that is a HARD STOP: stop all further operations, summarize the current task for the user, and wait for their explicit decision to continue.\n\nRECOMMENDED POLL INTERVAL: 10-30s. Don't poll faster than 5s — the task state only updates on actual progress, and aggressive polling wastes your tool budget.\n\nWORKFLOW:\n1. After `ssh_run` returns, store the `task_id`.\n2. Loop: call `ssh_status` with the task_id, sleep 10-30s, repeat until `status` is `done` or `failed`.\n3. If you no longer need a task (e.g. you found a better way), call `ssh_status` once and ignore — the task is harmless and will auto-clean after 1h. There's no explicit `kill` yet (use `ssh_exec kill <pid>` on the remote if you really need to stop a runaway process).\n\nOUTPUT: `{ task_id, status, phase?, progress_pct?, bytes_done?, bytes_total?, stdout, stderr, exit_code?, error?, result? }`. `stdout`/`stderr` are the accumulated output so far (or final output if `done`/`failed`).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": { "type": "string", "description": "The task_id returned by `ssh_run`." }
                    },
                    "required": ["task_id"]
                }
            },
            {
                "name": "ssh_cancel",
                "description": "Cancel a running `ssh_run` task. The remote SSH channel is closed (which interrupts the running process), the task is marked failed with `error` containing '用户通过 ssh_cancel 取消', and any stdout/stderr captured up to the cancel point is preserved.\n\nWHEN TO USE: A `ssh_run` task is taking too long or has gone sideways, and you want to stop it without waiting for the natural exit. Useful for runaway `docker pull`, `npm install` on the wrong repo, mis-targeted `find`, etc.\n\nWHEN NOT TO USE:\n- The task is already `done` or `failed` — returns an error. Use `ssh_status` to check first.\n- You want to kill a process on the REMOTE that `ssh_run` isn't directly managing — use `ssh_exec 'kill <pid>'` instead, which targets the remote's process tree more precisely. `ssh_cancel` closes the SSH channel; the remote shell may still have child processes that survive.\n\nBEHAVIOR:\n- The cancel signal is sent immediately and is idempotent (calling twice is safe).\n- The task transitions from its current phase straight to `failed` with `cancelled: true` in the result. Future `ssh_status` calls see this state until the 1h eviction.\n- No OS confirmation dialog — cancellation is non-destructive (the remote is left with whatever state the command had reached), so we don't pester the user with a Yes/No prompt.\n\nOUTPUT: `{ task_id, ok: true, status: \"cancelling\" }` on success, or an error string if the task_id is unknown / already finalized.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": { "type": "string", "description": "The task_id returned by `ssh_run` (must still be in Running/Connecting/Confirming state — not yet Done/Failed)." }
                    },
                    "required": ["task_id"]
                }
            },
            {
                "name": "sftp_list",
                "description": "List files and directories at a remote path on a saved SFTP/SSH server.\n\nWHEN TO USE: The user wants to browse remote files — e.g. 'what's in /var/log on the server', 'show me the files in /home/deploy/app'. Safe read-only operation, no confirmation needed.\n\nOUTPUT: JSON array of entries, each with `name`, `path` (full remote path), `is_dir` (bool), `size` (bytes), `permissions` (e.g. '-rw-r--r--'), `modified` (mtime). Sorted: directories first, then files alphabetically.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "path": { "type": "string", "description": "Remote directory path. Supports '~' and '~/subdir' (resolved via SFTP canonicalize). Defaults to '~' (home directory).", "default": "~" }
                    },
                    "required": ["connection"]
                }
            },
            {
                "name": "sftp_download",
                "description": "Download a single file from a remote SSH/SFTP server to the local filesystem.\n\nWHEN TO USE: User wants to fetch a remote file — e.g. 'download /etc/nginx/nginx.conf from web1', 'grab yesterday's log from /var/log/myapp.log'. Read-only on the server; writes to local disk.\n\nWHEN NOT TO USE: For reading a file's content into the conversation, you usually want to download it first then read it locally. Don't try to 'stream' large files — download to disk.\n\nOUTPUT: Confirmation message on success. The local file is overwritten if it exists.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "remote_path": { "type": "string", "description": "Absolute remote file path. Must be a file, not a directory." },
                        "local_path": { "type": "string", "description": "Absolute local path where the file will be saved (e.g. 'C:\\\\Users\\\\me\\\\downloads\\\\nginx.conf'). Parent directory must exist." }
                    },
                    "required": ["connection", "remote_path", "local_path"]
                }
            },
            {
                "name": "sftp_upload",
                "description": "Upload a single local file to a remote SSH/SFTP server.\n\nWHEN TO USE: User wants to push a file — e.g. 'deploy this config to /etc/nginx/nginx.conf', 'copy the new build to /var/www/html'. If `remote_path` is an existing file, it WILL be overwritten (no backup).\n\n⚠️ HUMAN CONFIRMATION REQUIRED: A native OS dialog pops up warning the user about the upload (potential overwrite). Won't proceed until the user clicks 'Yes'.\n\nOUTPUT: Confirmation message. The remote filename is taken from the local file's basename; only the remote *directory* path is used from `remote_path`.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "local_path": { "type": "string", "description": "Absolute local file path to upload. Must exist and be readable." },
                        "remote_path": { "type": "string", "description": "Remote TARGET DIRECTORY (not full file path). The uploaded file keeps its local basename. Must exist on the server." }
                    },
                    "required": ["connection", "local_path", "remote_path"]
                }
            },
            {
                "name": "sftp_mkdir",
                "description": "Create a new directory on a remote SSH/SFTP server.\n\nWHEN TO USE: User wants to create a remote folder before uploading files, set up a deployment path, etc. Errors if the path already exists or the parent doesn't.\n\nOUTPUT: Confirmation message. Does NOT create intermediate parent directories (like `mkdir -p` would) — if you need nested dirs, create them one level at a time.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "path": { "type": "string", "description": "Remote directory path to create (absolute)." }
                    },
                    "required": ["connection", "path"]
                }
            },
            {
                "name": "sftp_remove",
                "description": "Delete a file or directory on a remote SSH/SFTP server.\n\nWHEN TO USE: Rarely. The user explicitly asks to delete a remote file or empty directory.\n\n⚠️ HUMAN CONFIRMATION REQUIRED: A native OS dialog pops up warning about irreversible deletion. Won't proceed until the user clicks 'Yes'.\n\n⚠️ DANGEROUS: Cannot be undone. If `path` is a directory, it must be empty (this tool does NOT do recursive delete). If you find yourself wanting to delete a non-empty directory or use wildcards, STOP and ask the user to confirm explicitly — a wrong path can wipe important files.\n\nOUTPUT: Confirmation message on success.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "path": { "type": "string", "description": "Remote path to delete. Can be a file or an EMPTY directory." }
                    },
                    "required": ["connection", "path"]
                }
            },
            {
                "name": "sftp_rename",
                "description": "Rename or move a file/directory on a remote SSH/SFTP server.\n\nWHEN TO USE: User wants to rename a remote file or move it to a different directory on the SAME server — e.g. 'rename app.log to app.log.old', 'move /tmp/upload.zip to /opt/deploy/'.\n\n⚠️ HUMAN CONFIRMATION REQUIRED: A native OS dialog pops up warning that this may overwrite the destination. Won't proceed until the user clicks 'Yes'.\n\nWHEN NOT TO USE: Cross-server moves — this tool only works within one server. To move between servers, download then upload.\n\nOUTPUT: Confirmation message on success.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "old_path": { "type": "string", "description": "Current remote path (source)." },
                        "new_path": { "type": "string", "description": "New remote path (destination)." }
                    },
                    "required": ["connection", "old_path", "new_path"]
                }
            },
            {
                "name": "upload_project",
                "description": "打包并上传整个项目目录到远程服务器，自动解压。\n\nWHEN TO USE: 用户要把一个本地项目目录（含子目录）整体部署到远程服务器时 — 例如 '把 ./myapp 部署到 prod-server 的 /opt/myapp'、'上传这个文件夹到服务器并解压'。工具会自动执行：(1) 把本地目录打成 tar.gz 压缩包，(2) 通过 SSH 流式传输到服务器，(3) 在远程解压到指定路径。比逐个 sftp_upload 文件高效得多。\n\nWHEN NOT TO USE:\n- 只上传单个文件 — 用 sftp_upload 更快\n- 服务器未保存在 MyShell — 只能操作已保存的连接\n- 需要增量同步 — 此工具每次都会传整个目录\n\n⚠️ HUMAN CONFIRMATION REQUIRED: 上传可能覆盖远程文件，会弹出原生 OS 确认对话框，用户必须点击 'Yes' 才能继续。\n\nOUTPUT: 成功时返回确认消息（含传输大小和解压路径），失败返回错误。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "local_dir": { "type": "string", "description": "本地项目目录的绝对路径（如 'F:\\\\dev\\\\myapp'）。目录必须存在且可读。" },
                        "remote_dir": { "type": "string", "description": "远程目标目录的绝对路径（如 '/opt/myapp'）。工具会在 remote_dir 下自动创建与本地目录同名的子目录并解压。" }
                    },
                    "required": ["connection", "local_dir", "remote_dir"]
                }
            },
            {
                "name": "download_project",
                "description": "下载远程服务器上的整个项目目录到本地，自动解压。\n\nWHEN TO USE: 用户要把远程服务器上的整个项目目录下载到本地时 — 例如 '把 /opt/myapp 下载到本地'、'备份服务器上的项目'。工具会自动执行：(1) 在服务器上把目录打成 tar.gz 压缩包，(2) 通过 SFTP 下载到本地，(3) 在本地解压到指定路径。比逐个 sftp_download 文件高效得多。\n\nWHEN NOT TO USE:\n- 只下载单个文件 — 用 sftp_download 更快\n- 服务器未保存在 MyShell — 只能操作已保存的连接\n\n⚠️ HUMAN CONFIRMATION REQUIRED: 会弹出原生 OS 确认对话框，用户必须点击 'Yes' 才能继续。\n\nOUTPUT: 成功时返回确认消息（含传输大小和解压路径），失败返回错误。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "remote_dir": { "type": "string", "description": "远程项目目录的绝对路径（如 '/opt/myapp'）。目录必须存在且可读。" },
                        "local_dir": { "type": "string", "description": "本地目标目录的绝对路径（如 'F:\\\\dev\\\\myapp_backup'）。工具会在此目录下创建与远程目录同名的子目录并解压。" }
                    },
                    "required": ["connection", "remote_dir", "local_dir"]
                }
            },
            {
                "name": "test_connection",
                "description": "Verify that a saved connection still works (TCP dial + SSH handshake + auth + channel open). No commands run.\n\nWHEN TO USE: The user reports connection issues ('I can't reach prod-db', 'is the staging server up?') or you got an unexpected error from ssh_exec/sftp_list and want to diagnose. Quick, read-only, no confirmation needed.\n\nDISAMBIGUATION: If a host is saved as both an SSH and an SFTP connection (same IP/name), this tool probes the SSH one automatically — so passing a bare IP like '135.32.64.30' just works without needing the group path. Only a genuine ambiguity within SSH connections (two SSH entries, same host) returns an error listing the candidates.\n\nOUTPUT: Success message including the actual host:port that was contacted, or an error explaining which step failed (unreachable / auth refused / channel blocked).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC }
                    },
                    "required": ["connection"]
                }
            },
            {
                "name": "screenshot_terminal",
                "description": "Ask the user to capture a PNG screenshot of the terminal viewport for a saved connection, and have it saved to the attachment directory.\n\nWHEN TO USE: The user wants a visual record of what's on the terminal — e.g. 'screenshot the terminal', 'capture this screen', 'save what I'm seeing'. Also useful when you (the AI) need to see visual output that can't be expressed as text: full-screen TUIs (htop, vim), rendered charts/boxes, ASCII art, color-dependent output, or error screens with colored highlighting.\n\nWHEN NOT TO USE:\n- To read text command output — use `ssh_exec` instead and parse stdout.\n- To capture non-terminal UI (the GUI's own panels, dialogs, etc.) — this tool only captures the terminal viewport.\n- If the user isn't currently looking at the terminal — the capture is of whatever is on screen RIGHT NOW.\n\n⚠️ ARCHITECTURE LIMITATION (IMPORTANT): This MCP server runs as a separate process from the GUI and CANNOT directly access the GUI's terminal DOM. When you call this tool, it returns a message you should relay VERBATIM to the user — the user must then click the 📷 button in the MyShell GUI's CommandBar (next to the 快捷 button) themselves. The screenshot is saved automatically to their configured attachment directory. Tell the user:\n  - The connection they should have open in a tab\n  - That they should click 📷 in the terminal's toolbar\n  - That the PNG will be saved to the directory they configured in Settings → MCP 支持 → 附件目录\n\nIf they haven't configured the attachment directory yet, clicking 📷 will show an error telling them to configure it first.\n\nOUTPUT: A user-facing instruction message (relay it to the user). This tool does NOT return the image data — the capture happens in the GUI, not here.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC }
                    },
                    "required": ["connection"]
                }
            },
            {
                "name": "open_in_gui",
                "description": "Open a saved connection in the MyShell GUI as a tab (terminal or SFTP file browser). The GUI window is brought to the foreground and the connection is established automatically — the user sees a live, interactive tab.\n\nWHEN TO USE: The user wants to SEE and INTERACT with the connection themselves — e.g. 'open prod-db in MyShell', 'connect so I can watch', 'show me the terminal', 'let me browse files on web1'. Unlike ssh_exec/sftp_* (headless, return text), this opens a visible GUI tab the user can take over.\n\nTAB TYPES:\n- Default (omit `tab_type`): opens the connection's natural tab — a terminal for SSH connections, a file browser for SFTP connections.\n- `tab_type: \"terminal\"`: force a terminal (shell) tab.\n- `tab_type: \"sftp\"`: force an SFTP file-browser tab (useful to browse files on an SSH connection).\n\nFOCUS BEHAVIOR: If a matching tab for this connection is ALREADY open, the tool focuses (switches to) it instead of opening a duplicate. So calling this twice is safe — the second call just brings the existing tab to front.\n\nWHEN NOT TO USE:\n- For headless command execution where you just need the output — use `ssh_exec` instead.\n- When the MyShell GUI isn't running — this tool returns an error. Suggest the user open MyShell first, or use ssh_exec/sftp_* as a fallback.\n\nREQUIRES: The MyShell GUI application must be running on the same machine (communicates over a localhost IPC channel).\n\nOUTPUT: Confirmation of what happened (focused an existing tab, or opened a new terminal/SFTP tab) and that the window was focused.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "tab_type": {
                            "type": "string",
                            "enum": ["auto", "terminal", "sftp"],
                            "description": "Which kind of tab to open. 'auto' (default) uses the connection's natural type (terminal for SSH, file browser for SFTP). 'terminal' forces a shell tab. 'sftp' forces a file-browser tab.",
                            "default": "auto"
                        }
                    },
                    "required": ["connection"]
                }
            },
            {
                "name": "zmodem_download",
                "description": "Download a file from a remote SSH server using the ZMODEM protocol (remote `sz` → local). This is the FALLBACK transfer method — use it when `sftp_download` fails or is unavailable.\n\nWHEN TO USE (strict two-step policy):\n1. First call `sftp_download` for the same file. In most cases it succeeds and is faster.\n2. If `sftp_download` fails for ANY reason (connection is SSH-only with no SFTP saved, SFTP subsystem disabled, restricted/chroot shell, jump host, embedded device, etc.), call THIS tool as the fallback. Tell the user: 'sftp 失败，改用 ZMODEM (lrzsz) 方式传输'.\n3. If the user explicitly asked for ZMODEM/sz, use this tool directly.\n\nNEVER use `ssh_exec` + base64/cat/heredoc to transfer file content — it breaks on binary/large files. If both sftp and zmodem fail, tell the user and stop.\n\nPREREQUISITE: The remote server must have `sz` installed (part of the `lrzsz` package). If it's missing, the task fails after ~20s with a hint to install lrzsz.\n\nASYNC: returns IMMEDIATELY with {task_id, status:\"running\"}; the transfer runs in the background. The GUI auto-launches if it isn't running; a locked vault fails the tool call INSTANTLY with '保险库未解锁' (the MyShell window is brought to the front for the user) — tell the user to unlock, then retry. No human confirmation dialog (read-only on the remote).\n\nFINAL RESULT: when `zmodem_status` returns status=\"done\", its `result` field has `{files:[{name, local_path, bytes}]}`.\n\nNOTE: the `timeout` param (default 120s) caps the background transfer, not the tool-call window (which is <1s).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "remote_path": { "type": "string", "description": "Absolute remote file path to download (passed to `sz`). Must be a file, not a directory." },
                        "local_dir": { "type": "string", "description": "Absolute local directory where the file will be saved. Must exist. The file is named after the remote offer's basename." },
                        "timeout": { "type": "integer", "description": "Max seconds to wait for the transfer (default 120). The first 20s is an initial handshake timeout before the transfer is considered stalled.", "default": 120 }
                    },
                    "required": ["connection", "remote_path", "local_dir"]
                }
            },
            {
                "name": "zmodem_upload",
                "description": "Upload a local file to a remote SSH server using the ZMODEM protocol (local → remote `rz`). This is the FALLBACK transfer method — use it when `sftp_upload` fails or is unavailable.\n\nWHEN TO USE (strict two-step policy):\n1. First call `sftp_upload` for the same file. In most cases it succeeds and is faster.\n2. If `sftp_upload` fails for ANY reason (connection is SSH-only with no SFTP saved, SFTP subsystem disabled, restricted/chroot shell, jump host, embedded device, etc.), call THIS tool as the fallback. Tell the user: 'sftp 失败，改用 ZMODEM (lrzsz) 方式传输'.\n3. If the user explicitly asked for ZMODEM/rz, use this tool directly.\n\nNEVER use `ssh_exec` + base64/cat/heredoc/echo to transfer file content — it breaks on binary/large files. If both sftp and zmodem fail, tell the user and stop.\n\nPREREQUISITE: The remote server must have `rz` installed (part of the `lrzsz` package). If missing, the task fails after ~20s.\n\nASYNC: returns IMMEDIATELY with {task_id, status:\"running\"}; the transfer runs in the background. The GUI auto-launches if it isn't running; a locked vault fails the tool call INSTANTLY with '保险库未解锁' (the MyShell window is brought to the front for the user) — tell the user to unlock, then retry. The OS confirmation dialog pops AFTER the tool returns — remind the user to click it if the first `zmodem_status` poll shows phase=`Confirming`. You MUST then poll `zmodem_status(task_id)` every 5-10s until status is `done` or `failed`.\n\n⚠️ HUMAN CONFIRMATION REQUIRED: A native OS dialog pops up (writing to the remote server). Won't proceed until the user clicks 'Yes'. This happens AFTER the tool returns, in the background.\n\nFINAL RESULT: when `zmodem_status` returns status=\"done\", its `result` field has `{remote_path, bytes}`.\n\nNOTE: the `timeout` param (default 120s) caps the background transfer, not the tool-call window (which is <1s).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "local_path": { "type": "string", "description": "Absolute local file path to upload. Must exist and be a regular file." },
                        "remote_dir": { "type": "string", "description": "Remote target DIRECTORY (absolute). rz receives into this directory; the file keeps its local basename. `cd <remote_dir>` is run before rz, so the directory must exist." },
                        "timeout": { "type": "integer", "description": "Max seconds to wait for the transfer (default 120).", "default": 120 }
                    },
                    "required": ["connection", "local_path", "remote_dir"]
                }
            },
            {
                "name": "zmodem_status",
                "description": "Poll the status of a background ZMODEM upload/download task started by `zmodem_upload` or `zmodem_download`.\n\nWHEN TO USE: After calling `zmodem_upload`/`zmodem_download` (which return immediately with a `task_id`), call THIS tool with that `task_id` to check progress. Poll every 5-10 seconds until the response `status` is `done` or `failed`.\n\nOUTPUT: JSON with `task_id`, `status` (running/done/failed), `phase` (Confirming/Connecting/Transferring for running tasks), `progress_pct`, `bytes_done`, `bytes_total`. When `status` is `done`, `result` contains the transfer outcome (files for download, remote_path+bytes for upload). When `failed`, `error` has the message. If the error contains '⛔ 用户已拒绝高危操作', that is a HARD STOP — stop all further operations, summarize the current task for the user, and wait for their explicit decision to continue.\n\nIMPORTANT: this is a lightweight read-only poll — no confirmation dialog, fast. Keep calling it (every 5-10s) until you see done/failed. If `phase` is `Confirming`, tell the user a confirmation dialog is waiting for them to click.\n\nERROR '未找到任务': the task was lost (likely the MCP process restarted). The transfer did not complete — tell the user and suggest retrying.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task_id": { "type": "string", "description": "The task_id returned by zmodem_upload/zmodem_download." }
                    },
                    "required": ["task_id"]
                }
            }
        ]
    })
}

// ============ Async transfer task table ============
//
// ZMODEM transfers outlast any single MCP tool-call window: the client (ZCode,
// Cursor, etc.) imposes a 30s hard timeout on each tools/call, but a transfer
// needs a human-confirmation dialog + the actual byte streaming. So we run
// transfers as detached tokio tasks and let the AI poll for completion via
// `zmodem_status`. Each task writes its progress into this shared table.

/// Lifecycle of an async transfer task.
#[derive(Clone, Debug, PartialEq, Eq)]
enum TaskPhase {
    /// Waiting for the user to click the OS confirmation dialog (upload only).
    Confirming,
    /// Connecting + negotiating ZMODEM (pre-handshake). The first-frame
    /// timeout (lrzsz installed? path exists?) happens here.
    Connecting,
    /// Bytes are flowing.
    Transferring,
    /// Finished successfully.
    Done,
    /// Failed.
    Failed,
}

/// A snapshot of one transfer task, readable by `zmodem_status`.
#[derive(Clone, Debug)]
struct TransferTask {
    phase: TaskPhase,
    /// "download" or "upload".
    direction: String,
    /// Human-readable description for the status tool output.
    description: String,
    /// Bytes transferred so far (0 until handshake completes).
    bytes_done: u64,
    /// Total bytes (file size), 0 if unknown.
    bytes_total: u64,
    /// Set when phase == Done: the result payload (files / remote_path).
    result: Option<Value>,
    /// Set when phase == Failed.
    error: Option<String>,
    /// When the task was created (for staleness / cleanup decisions).
    started_at: std::time::Instant,
}

impl TransferTask {
    fn new(direction: &str, description: String, bytes_total: u64) -> Self {
        Self {
            phase: TaskPhase::Confirming,
            direction: direction.to_string(),
            description,
            bytes_done: 0,
            bytes_total,
            result: None,
            error: None,
            started_at: std::time::Instant::now(),
        }
    }

    /// Render the task as the MCP tool result text the AI sees.
    fn to_status_text(&self, task_id: &str) -> Value {
        let phase_str = match self.phase {
            TaskPhase::Confirming => "等待用户确认（请点击弹出的对话框）",
            TaskPhase::Connecting => "连接中（握手）",
            TaskPhase::Transferring => "传输中",
            TaskPhase::Done => "已完成",
            TaskPhase::Failed => "失败",
        };
        let pct = if self.bytes_total > 0 {
            (self.bytes_done * 100 / self.bytes_total).min(100)
        } else {
            0
        };
        let text = format!(
            "ZMODEM {} 任务 [{}]：{}\n状态：{}（{}% / {} 字节）",
            self.direction, task_id, self.description, phase_str, pct, self.bytes_done
        );
        match &self.phase {
            TaskPhase::Done => json!({
                "content": [{ "type": "text", "text": text }],
                "task_id": task_id,
                "status": "done",
                "progress_pct": pct,
                "bytes_done": self.bytes_done,
                "bytes_total": self.bytes_total,
                "result": self.result.clone().unwrap_or(json!(null))
            }),
            TaskPhase::Failed => json!({
                "content": [{ "type": "text", "text": format!("{}\n错误：{}", text, self.error.as_deref().unwrap_or("未知")) }],
                "task_id": task_id,
                "status": "failed",
                "error": self.error.clone().unwrap_or_default()
            }),
            _ => json!({
                "content": [{ "type": "text", "text": text }],
                "task_id": task_id,
                "status": "running",
                "phase": format!("{:?}", self.phase),
                "progress_pct": pct,
                "bytes_done": self.bytes_done,
                "bytes_total": self.bytes_total
            }),
        }
    }
}

// ============ Shared state ============

struct McpState {
    app: AppState,
    /// Async transfer tasks keyed by id. Written by background tokio tasks (the
    /// actual transfer drivers), read by the synchronous `zmodem_status` tool.
    /// Each entry is updated in place as the transfer progresses.
    tasks: Arc<Mutex<HashMap<String, TransferTask>>>,
    /// Per-exec-task cancel control plane. Inserted by `ssh_run`, looked up
    /// by `ssh_cancel` to flip a `watch::Sender` that the collect loop in
    /// `run_ssh_exec_task` is subscribed to via `tokio::select!`. Entries
    /// are removed in the same place the `TransferTask` entry is finalized
    /// (the spawn closure), so a stale task_id won't get a phantom cancel.
    exec_controls: Arc<Mutex<HashMap<String, ExecControl>>>,
}

/// Control handle for a running ssh_run task. The collect loop in
/// `run_ssh_exec_task` subscribes to `cancel_tx` and bails out of the
/// select! when a value arrives; `ssh_cancel` is what flips it.
struct ExecControl {
    /// Single-value watch channel: starts at `false`, `ssh_cancel` sends
    /// `true` to request termination. Subscribers re-check on every
    /// `changed()` wakeup.
    cancel_tx: tokio::sync::watch::Sender<bool>,
}

/// Resolve a connection name/group-path to its id WITHOUT needing the DEK
/// (plaintext-only DB lookup). Independent of `McpState` so background tasks
/// (which only hold an `AppState` clone) can call it.
fn resolve_connection_id(
    app: &AppState,
    query: &str,
    expected_conn_type: Option<&str>,
) -> Result<String, String> {
    let db = app.db.lock().map_err(|e| e.to_string())?;
    let connections = db::get_all_connections_plaintext(&db).map_err(|e| e.to_string())?;

    let type_ok = |c: &ConnectionConfig| match expected_conn_type {
        Some(t) => c.conn_type == t,
        None => true,
    };

    // Pass 1: exact name match.
    let by_name: Vec<_> = connections.iter().filter(|c| c.name == query && type_ok(c)).collect();
    if by_name.len() == 1 {
        return Ok(by_name[0].id.clone());
    }
    if by_name.len() > 1 {
        return Err(ambiguous_error(query, expected_conn_type, &by_name));
    }

    // Pass 2: group-prefixed path match.
    let by_path: Vec<_> = connections
        .iter()
        .filter(|c| format!("{}/{}", c.group_path.trim_end_matches('/'), c.name) == query && type_ok(c))
        .collect();
    if by_path.len() == 1 {
        return Ok(by_path[0].id.clone());
    }
    if by_path.len() > 1 {
        return Err(ambiguous_error(query, expected_conn_type, &by_path));
    }

    // Type-mismatch hint.
    if let Some(t) = expected_conn_type {
        let any_match: Vec<_> = connections
            .iter()
            .filter(|c| c.name == query
                || format!("{}/{}", c.group_path.trim_end_matches('/'), c.name) == query)
            .collect();
        if !any_match.is_empty() {
            let types: Vec<&str> = any_match.iter().map(|c| c.conn_type.as_str()).collect();
            return Err(format!(
                "找到了连接 '{}'，但类型是 {}，而当前操作需要 {} 类型。请确认连接配置或换一个连接。",
                query, types.join("/"), t
            ));
        }
    }

    Err(format!(
        "未找到连接: {}。注意：按 IP 查找需要先打开 GUI 并解锁保险库（IP 是加密存储的）。调用 list_connections 查看所有可用连接（按名称）。",
        query
    ))
}

/// Full credential resolution for background tasks: ensure the GUI is running
/// and the vault is unlocked (fail-fast, auto-launching the GUI if needed),
/// then ask the GUI to decrypt the connection. Each failure is reported as a
/// task error via `fail`.
async fn resolve_config_for_task(
    app: &AppState,
    conn_name: &str,
    expected_conn_type: Option<&str>,
    fail: &impl Fn(String),
) -> Option<ConnectionConfig> {
    // This is mostly a race guard: the synchronous vault gate at the top of
    // call_tool has already run, so the common case is an instant Ok. If the
    // vault got locked in between (auto-lock timer), we fail the task fast
    // instead of silently waiting.
    if let Err(e) = ensure_vault_ready() {
        fail(e);
        return None;
    }
    // Resolve the connection id (plaintext DB lookup).
    let conn_id = match resolve_connection_id(app, conn_name, expected_conn_type) {
        Ok(id) => id,
        Err(e) => {
            fail(e);
            return None;
        }
    };
    // Ask the GUI to decrypt the connection config.
    match get_config_from_gui(&conn_id) {
        Ok(c) => Some(c),
        Err(e) => {
            fail(format!("解析连接凭证失败: {}", e));
            None
        }
    }
}

impl McpState {
    /// Plaintext-only connection lookup. Delegates to the standalone
    /// `resolve_connection_id` (shared with background tasks).
    fn find_connection_id(&self, query: &str, expected_conn_type: Option<&str>) -> Result<String, String> {
        resolve_connection_id(&self.app, query, expected_conn_type)
    }

    /// Resolve a connection name/group-path to a fully decrypted
    /// ConnectionConfig by asking the GUI to decrypt it. This replaces the
    /// old find_connection + resolve_secrets combo (which needed the DEK).
    /// The MCP server no longer holds the DEK — the GUI does (user unlocked).
    fn resolve_via_gui(&self, query: &str, expected_conn_type: Option<&str>) -> Result<ConnectionConfig, String> {
        let conn_id = self.find_connection_id(query, expected_conn_type)?;
        get_config_from_gui(&conn_id)
    }
}

/// Format an "ambiguous match" error that tells the AI exactly which
/// connections collided so it can surface them to the user.
fn ambiguous_error(
    query: &str,
    expected_conn_type: Option<&str>,
    candidates: &[&ConnectionConfig],
) -> String {
    let list: Vec<String> = candidates
        .iter()
        .map(|c| {
            format!(
                "{} (type={}, group={}, user={})",
                c.name, c.conn_type, c.group_path, c.username
            )
        })
        .collect();
    format!(
        "连接 '{}'{} 匹配到 {} 条，请让用户明确指定其中一条：\n{}",
        query,
        expected_conn_type.map(|t| format!("（{} 类型）", t)).unwrap_or_default(),
        candidates.len(),
        list.join("\n")
    )
}

// ============ Tool dispatch ============

async fn call_tool(state: &McpState, name: &str, args: &Value) -> Result<Value, String> {
    // ── Vault-unlock gate (fail-fast, applies to EVERY tool except task
    // status/cancel) ────────────────────────────────────────────────────
    // Every tool requires the GUI vault to be unlocked — INCLUDING
    // list_connections: without the master password the AI must not even
    // learn WHICH servers exist (connection names/groups are metadata worth
    // protecting too). The gate:
    //   - auto-launches the GUI when it isn't running;
    //   - checks the vault state ONCE and fails immediately when locked,
    //     focusing the GUI's password gate so the user sees it right away.
    // The three exemptions (ssh_status / ssh_cancel / zmodem_status) never
    // touch credentials and must stay reachable so the AI can observe and
    // clean up tasks that failed ON this very gate.
    match name {
        "ssh_status" | "ssh_cancel" | "zmodem_status" => {}
        _ => {
            ensure_vault_ready()?;
        }
    }

    match name {
        "list_connections" => {
            // SECURITY: the vault gate at the top of call_tool has already
            // verified the GUI is running and the vault is unlocked — without
            // it, the AI must not even see which servers exist. The output is
            // still kept minimal: only name/conn_type/group_path. Host, port,
            // username and passwords are NEVER returned to the AI.
            let db = state.app.db.lock().map_err(|e| e.to_string())?;
            let connections = db::get_all_connections_plaintext(&db).map_err(|e| e.to_string())?;
            let items: Vec<Value> = connections
                .iter()
                .map(|c| {
                    json!({
                        "name": c.name,
                        "conn_type": c.conn_type,
                        "group_path": c.group_path,
                    })
                })
                .collect();
            Ok(json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&items).unwrap() }] }))
        }

            "ssh_exec" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let command = args["command"].as_str().ok_or("缺少 command 参数")?;
            // Default 60s (up from 30s) — gives 2x headroom for typical ops
            // (apt update, system health checks, log scans) without forcing
            // every AI call to override. For genuinely long tasks (>5min) use
            // the `ssh_run` tool (async + poll) instead of raising this.
            // Clamped to 1h max — anything longer should be `ssh_run`.
            let timeout = args["timeout"].as_u64().unwrap_or(60).min(3600);

            // Command confirmation: read the user's whitelist/blacklist rules
            // (regex) from the config file. Only commands that look dangerous
            // pop the OS confirmation dialog — read-only commands (ps, ls,
            // cat, grep, ...) run freely. See command_rules module for the
            // fail-safe decision order.
            let rules = load_command_rules();

            // ── show_in_gui path: run the command in a visible GUI tab ──
            // When enabled (default), the command is sent to a terminal tab in
            // the MyShell GUI so the user can watch it in real time. The output
            // is captured from the tab's PTY (via sentinel mechanism) and
            // returned to the AI — same result as the headless path, but the
            // user sees everything.
            //
            // GUI-attempted flag: if the GUI path was attempted AND the GUI was
            // reachable (i.e. the confirmation dialog was shown or could have
            // been shown), we must NOT fall back to headless + re-confirm for
            // dangerous commands — that would pop a SECOND confirmation (the OS
            // MessageBoxW) on top of the GUI's React dialog, forcing the user
            // to click twice. So when gui_attempted && needs_confirmation, we
            // return the error directly instead of falling through.
            if rules.show_in_gui {
                match exec_in_gui_tab(conn_name, command, timeout, &state).await {
                    Ok(result_json) => return Ok(result_json),
                    Err(e) => {
                        let needs_confirm = command_rules::command_needs_confirmation(command, &rules);
                        // Detect whether the GUI was actually reachable. If
                        // ensure_gui_running failed (GUI not installed / won't
                        // start), the error message mentions myshell.exe or
                        // startup timeout — in that case the GUI dialog was
                        // never shown, so headless fallback (with its own OS
                        // confirm) is legitimate.
                        let gui_unreachable = e.contains("未找到 myshell.exe")
                            || e.contains("GUI 启动超时")
                            || e.contains("无法定位");
                        let gui_attempted = !gui_unreachable;

                        if gui_attempted && needs_confirm {
                            // GUI was reachable but the exec failed (user didn't
                            // confirm in time, cancelled, or a session error).
                            // Don't fall through to headless — that would pop a
                            // second confirmation dialog. Surface the error.
                            log(&format!(
                                "show_in_gui 失败且命令需确认，不再回退 headless（避免重复弹窗）: {}",
                                e
                            ));
                            return Err(e);
                        }
                        // Either the GUI was unreachable (legitimate fallback)
                        // or the command doesn't need confirmation (headless
                        // won't pop anything). Fall through.
                        log(&format!("show_in_gui 失败，回退到静默模式: {}", e));
                    }
                }
            }

            // ── Confirmation check (headless path) ──
            if command_rules::command_needs_confirmation(command, &rules) {
                let detail = format!("在服务器 [{}] 执行命令: {}", conn_name, command);
                if !confirm_dangerous_operation("ssh_exec（远程命令执行）", &detail) {
                    return Ok(json!({ "content": [{ "type": "text", "text": denied_by_user_text("ssh_exec", &detail) }], "isError": true }));
                }
            } else {
                log(&format!("ssh_exec 免确认（白名单/非危险）: {}", command));
            }

            let config = state.resolve_via_gui(conn_name, Some("ssh"))?;

            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;
            let mut channel = handle
                .channel_open_session()
                .await
                .map_err(|e| format!("打开通道失败: {}", e))?;
            channel.exec(true, command).await.map_err(|e| format!("exec 失败: {}", e))?;

            let collect = async {
                use russh::ChannelMsg;
                let mut stdout: Vec<u8> = Vec::new();
                let mut stderr: Vec<u8> = Vec::new();
                let mut exit_code: Option<u32> = None;
                // Per-stream cap is configurable via MCP_SSH_MAX_OUTPUT_BYTES;
                // reads from an atomic so we can adjust at startup without
                // paying a mutex on every chunk.
                let max = ssh_max_output_bytes();
                loop {
                    match channel.wait().await {
                        Some(ChannelMsg::Data { ref data }) => {
                            if stdout.len() < max {
                                let room = max - stdout.len();
                                stdout.extend_from_slice(&data[..data.len().min(room)]);
                            }
                        }
                        Some(ChannelMsg::ExtendedData { ref data, ext: 1 }) => {
                            if stderr.len() < max {
                                let room = max - stderr.len();
                                stderr.extend_from_slice(&data[..data.len().min(room)]);
                            }
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status),
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        Some(_) => {}
                    }
                }
                (stdout, stderr, exit_code)
            };

            let (stdout, stderr, code) = tokio::time::timeout(
                std::time::Duration::from_secs(timeout),
                collect,
            )
            .await
            .map_err(|_| format!("命令超时（{}秒）", timeout))?;

            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            let result = json!({
                "exit_code": code.unwrap_or(0),
                "stdout": String::from_utf8_lossy(&stdout),
                "stderr": String::from_utf8_lossy(&stderr),
            });
            Ok(json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result).unwrap() }] }))
        }

        "ssh_run" => {
            // Async start of a long-running command. Returns task_id immediately
            // (well under the client's call timeout) and a background task does
            // the dial + exec + collect. The AI polls `ssh_status` with the
            // task_id to follow progress. Output accumulation is bounded at 4MB
            // per stream; for tasks with more output, redirect to a file
            // server-side and sftp_download it.
            //
            // Reuses the TransferTask table (McpState.tasks) — same machinery
            // as zmodem_*. direction="exec" tells ssh_status how to render the
            // status text.
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?.to_string();
            let command = args["command"].as_str().ok_or("缺少 command 参数")?.to_string();

            // GUI/vault pre-flight has already run at the top of call_tool
            // (auto-launch + fail-fast unlock check), so there is nothing
            // slow left to do here — the confirmation dialog is popped in
            // the BACKGROUND task below so the tool call returns instantly.
            let rules = load_command_rules();
            let needs_confirm = command_rules::command_needs_confirmation(&command, &rules);

            // Build a short description (full command can be very long; truncate).
            let cmd_preview: String = command.chars().take(60).collect();
            let cmd_preview = if command.chars().count() > 60 {
                format!("{}…", cmd_preview)
            } else {
                cmd_preview
            };
            let description = format!("[{}] {}", conn_name, cmd_preview);

            // Register the task in Confirming phase. If the command doesn't
            // need confirmation, we still leave it in Confirming briefly (the
            // bg task flips to Connecting immediately) — keeping the model
            // consistent means the AI sees the same phases either way.
            let task_id = format!("ssh-{}", chrono_like_ts());
            // Cancel control plane: created here so the task_id is bound to
            // a cancel handle BEFORE the background task starts. If the user
            // hits `ssh_cancel` before the task gets to its select!, the
            // signal sits in the watch channel buffer and fires the moment
            // the loop starts polling it — no race window.
            let (cancel_tx, _cancel_rx_init) = tokio::sync::watch::channel(false);
            {
                let mut tasks = state.tasks.lock().map_err(|e| e.to_string())?;
                tasks.insert(
                    task_id.clone(),
                    TransferTask::new("exec", description.clone(), 0),
                );
                let mut controls = state.exec_controls.lock().map_err(|e| e.to_string())?;
                controls.insert(
                    task_id.clone(),
                    ExecControl { cancel_tx },
                );
            }

            // Spawn the background driver. cancel_rx is created here (in the
            // spawning task, not in the bg task) so the bg task can move it
            // into its select! without any extra locking — watch::Sender is
            // cheap to clone, and the receiver stays valid for the bg task's
            // entire lifetime.
            let cancel_rx = {
                let controls = state.exec_controls.lock().expect("controls mutex");
                controls.get(&task_id).expect("just inserted").cancel_tx.subscribe()
            };
            let app = state.app.clone();
            let tasks_table = Arc::clone(&state.tasks);
            let controls_table = Arc::clone(&state.exec_controls);
            let tid = task_id.clone();
            tokio::spawn(async move {
                run_ssh_exec_task(
                    &app, &tasks_table, &controls_table, &tid,
                    &conn_name, &command, needs_confirm, cancel_rx,
                ).await;
            });

            log(&format!("ssh_run [{}] 已启动: {}", task_id, cmd_preview));

            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!(
                        "⏳ ssh_run 已启动（任务 {}）。\n命令：{}\n请用 ssh_status 查询进度（建议每 10-30 秒轮询一次）。如命令需要确认，会有系统弹窗；如不需要则自动开始。",
                        task_id, description
                    )
                }],
                "task_id": task_id,
                "status": "started",
                "poll_with": "ssh_status"
            }))
        }

        "ssh_status" => {
            // Poll a ssh_run task. Returns the current phase + accumulated
            // output (truncated to 4MB per stream). Tasks are kept in memory
            // for 1h after completion, then auto-evicted; stale task_ids
            // return an error.
            let task_id = args["task_id"].as_str().ok_or("缺少 task_id 参数")?;
            // Drop any tasks older than TTL before lookup. Cheap (mutex-held
            // microseconds) and keeps the map from growing unbounded across
            // a long session.
            evict_stale_tasks(&state.tasks);
            let tasks = state.tasks.lock().map_err(|e| e.to_string())?;
            match tasks.get(task_id) {
                Some(task) => {
                    // For exec tasks, render a status text tailored to command
                    // semantics (no "X% / N bytes" wording — exec doesn't
                    // track bytes). zmodem tasks still get the original
                    // "X% / N bytes" rendering.
                    if task.direction == "exec" {
                        Ok(exec_status_to_json(task_id, task))
                    } else {
                        Ok(task.to_status_text(task_id))
                    }
                }
                None => Err(format!(
                    "任务 {} 不存在或已过期（任务完成后 1 小时会被清理）",
                    task_id
                )),
            }
        }

        "ssh_cancel" => {
            // Cancel a running ssh_run task. Idempotent — second call on the
            // same task_id is harmless (the watch::Sender::send succeeds
            // even if no one's listening). We refuse to cancel a finalized
            // task (Done/Failed) because the bg task has already torn down
            // its control entry — a cancel call at that point would just
            // confuse the operator into thinking something went wrong.
            let task_id = args["task_id"].as_str().ok_or("缺少 task_id 参数")?;

            // First: confirm the task still exists and is in a cancellable
            // phase. We do this BEFORE touching the controls map so a
            // typo'd task_id doesn't silently no-op.
            {
                let tasks = state.tasks.lock().map_err(|e| e.to_string())?;
                match tasks.get(task_id) {
                    Some(t) if matches!(t.phase, TaskPhase::Done | TaskPhase::Failed) => {
                        return Err(format!(
                            "任务 {} 已经结束（phase={:?}），无需取消",
                            task_id, t.phase
                        ));
                    }
                    None => {
                        return Err(format!(
                            "任务 {} 不存在或已过期",
                            task_id
                        ));
                    }
                    _ => {} // fall through to cancel
                }
            }

            // Flip the watch channel. The collect loop in run_ssh_exec_task
            // is subscribed via cancel_rx.changed() in its tokio::select!,
            // and will exit on the next iteration. The watch semantics
            // guarantee the signal is delivered even if no one is awaiting
            // it RIGHT NOW — the value sits in the channel until the
            // receiver wakes up.
            {
                let controls = state.exec_controls.lock().map_err(|e| e.to_string())?;
                match controls.get(task_id) {
                    Some(c) => {
                        let _ = c.cancel_tx.send(true);
                        log(&format!("ssh_cancel [{}] signal sent", task_id));
                    }
                    None => {
                        // Race: the bg task already finished between our
                        // existence check and the cancel attempt (the map
                        // entries are torn down in the bg task's Phase 5
                        // cleanup). Surface this as a soft error so the
                        // AI knows the task is gone.
                        return Err(format!(
                            "任务 {} 的控制句柄已被清理（任务可能刚结束），取消信号未发送",
                            task_id
                        ));
                    }
                }
            }

            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!(
                        "🛑 ssh_cancel 已发送（任务 {}）。后台任务会在下一次循环迭代时检测到并退出，stdout/stderr 截至取消点的部分会被保留。请稍候用 ssh_status 确认 cancelled 状态。",
                        task_id
                    )
                }],
                "task_id": task_id,
                "ok": true,
                "status": "cancelling"
            }))
        }

        "sftp_list" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let path = args["path"].as_str().unwrap_or("~");

            let config = state.resolve_via_gui(conn_name, Some("sftp"))?;
            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;
            let sftp = open_sftp(&handle).await?;
            let entries = sftp_list_dir(&sftp, path).await?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            Ok(json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&entries).unwrap() }] }))
        }

        "sftp_download" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let remote = args["remote_path"].as_str().ok_or("缺少 remote_path 参数")?;
            let local = args["local_path"].as_str().ok_or("缺少 local_path 参数")?;

            let config = state.resolve_via_gui(conn_name, Some("sftp"))?;
            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;
            let sftp = open_sftp(&handle).await?;
            sftp_download_file(&sftp, remote, local).await?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            Ok(json!({ "content": [{ "type": "text", "text": format!("已下载 {} → {}", remote, local) }] }))
        }

        "sftp_upload" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let local = args["local_path"].as_str().ok_or("缺少 local_path 参数")?;
            let remote = args["remote_path"].as_str().ok_or("缺少 remote_path 参数")?;

            // 高危操作：上传可能覆盖远程文件
            let detail = format!("上传本地文件 [{}] → 服务器 [{}] 路径: {}", local, conn_name, remote);
            if !confirm_dangerous_operation("sftp_upload（上传文件）", &detail) {
                return Ok(json!({ "content": [{ "type": "text", "text": denied_by_user_text("sftp_upload", &detail) }], "isError": true }));
            }

            let config = state.resolve_via_gui(conn_name, Some("sftp"))?;
            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;
            let sftp = open_sftp(&handle).await?;
            sftp_upload_file(&sftp, local, remote).await?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            Ok(json!({ "content": [{ "type": "text", "text": format!("已上传 {} → {}", local, remote) }] }))
        }

        "sftp_mkdir" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let path = args["path"].as_str().ok_or("缺少 path 参数")?;

            let config = state.resolve_via_gui(conn_name, Some("sftp"))?;
            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;
            let sftp = open_sftp(&handle).await?;
            sftp.create_dir(path).await.map_err(|e| format!("创建目录失败: {}", e))?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            Ok(json!({ "content": [{ "type": "text", "text": format!("已创建目录 {}", path) }] }))
        }

        "sftp_remove" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let path = args["path"].as_str().ok_or("缺少 path 参数")?;

            // 高危操作：删除不可恢复
            let detail = format!("删除服务器 [{}] 上的: {}", conn_name, path);
            if !confirm_dangerous_operation("sftp_remove（删除文件/目录）", &detail) {
                return Ok(json!({ "content": [{ "type": "text", "text": denied_by_user_text("sftp_remove", &detail) }], "isError": true }));
            }

            let config = state.resolve_via_gui(conn_name, Some("sftp"))?;
            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;
            let sftp = open_sftp(&handle).await?;
            if sftp.remove_file(path).await.is_err() {
                sftp.remove_dir(path).await.map_err(|e| format!("删除失败: {}", e))?;
            }
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            Ok(json!({ "content": [{ "type": "text", "text": format!("已删除 {}", path) }] }))
        }

        "sftp_rename" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let old = args["old_path"].as_str().ok_or("缺少 old_path 参数")?;
            let new = args["new_path"].as_str().ok_or("缺少 new_path 参数")?;

            // 高危操作：重命名/移动可能覆盖目标
            let detail = format!("服务器 [{}] 上: {} → {}", conn_name, old, new);
            if !confirm_dangerous_operation("sftp_rename（重命名/移动）", &detail) {
                return Ok(json!({ "content": [{ "type": "text", "text": denied_by_user_text("sftp_rename", &detail) }], "isError": true }));
            }

            let config = state.resolve_via_gui(conn_name, Some("sftp"))?;
            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;
            let sftp = open_sftp(&handle).await?;
            sftp.rename(old, new).await.map_err(|e| format!("重命名失败: {}", e))?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            Ok(json!({ "content": [{ "type": "text", "text": format!("已重命名 {} → {}", old, new) }] }))
        }

        "upload_project" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let local_dir = args["local_dir"].as_str().ok_or("缺少 local_dir 参数")?;
            let remote_dir = args["remote_dir"].as_str().ok_or("缺少 remote_dir 参数")?;

            // 高危操作：上传可能覆盖远程文件
            let detail = format!(
                "打包本地目录 [{}] → 服务器 [{}] 路径: {}",
                local_dir, conn_name, remote_dir
            );
            if !confirm_dangerous_operation("upload_project（上传项目目录）", &detail) {
                return Ok(json!({ "content": [{ "type": "text", "text": denied_by_user_text("upload_project", &detail) }], "isError": true }));
            }

            // 1. 验证本地目录存在
            let local_path = std::path::Path::new(local_dir);
            if !local_path.is_dir() {
                return Err(format!("本地目录不存在: {}", local_dir));
            }
            let dir_name = local_path.file_name()
                .and_then(|n| n.to_str())
                .ok_or("无法提取目录名")?;

            // 2. 创建 tar.gz（排除常见不必要的目录）
            log(&format!("[upload_project] 打包 {} → {}.tar.gz", local_dir, dir_name));
            let tar_output = std::process::Command::new("tar")
                .args([
                    "-czf", "-",
                    "--exclude=.venv", "--exclude=venv", "--exclude=.env",
                    "--exclude=__pycache__", "--exclude=*.pyc",
                    "--exclude=.git", "--exclude=.svn",
                    "--exclude=node_modules", "--exclude=.next", "--exclude=dist", "--exclude=build",
                    "--exclude=target", "--exclude=.idea", "--exclude=.vscode",
                    "-C", local_dir, ".",
                ])
                .output()
                .map_err(|e| format!("打包失败: {}（确保系统有 tar 命令）", e))?;

            if !tar_output.status.success() {
                let stderr = String::from_utf8_lossy(&tar_output.stderr);
                return Err(format!("tar 打包失败: {}", stderr));
            }

            let tar_bytes = tar_output.stdout;
            let tar_size = tar_bytes.len();
            log(&format!("[upload_project] 打包完成: {} bytes", tar_size));

            // 3. 写入本地临时文件（sftp_upload_file 需要文件路径）
            let tmp_tar = std::env::temp_dir().join(format!("_ul_project_{}.tar.gz", dir_name));
            let tmp_tar_str = tmp_tar.to_str().ok_or("临时路径无效")?;
            std::fs::write(&tmp_tar, &tar_bytes)
                .map_err(|e| format!("写入临时文件失败: {}", e))?;

            // 4. 用 SFTP 上传到 home 目录（可靠），再 sudo 移动到目标位置
            let remote_target = format!("{}/{}", remote_dir.trim_end_matches('/'), dir_name);

            // 先获取连接配置（需要 username 构造 home 路径）
            let config = state.resolve_via_gui(conn_name, None)?;
            let home_dir = format!("/home/{}", config.username);
            let remote_tar_name = format!("_ul_project_{}.tar.gz", dir_name);
            let remote_tar_path = format!("{}/{}", home_dir, remote_tar_name);

            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;
            let sftp = open_sftp(&handle).await?;
            log(&format!("[upload_project] SFTP 上传 {} → {}", tmp_tar_str, home_dir));
            sftp_upload_file(&sftp, tmp_tar_str, &home_dir).await?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            // 5. 清理本地临时文件
            std::fs::remove_file(&tmp_tar).ok();

            // 4. 在服务器上：创建目标目录 + sudo 移动 + 解压 + 清理。
            // SECURITY: every interpolated path goes through shell_quote() —
            // they contain user/AI-controlled segments (remote_dir, the local
            // dir basename) and run under sudo, so an unescaped quote would
            // mean arbitrary command execution as root.
            let ssh_config2 = state.resolve_via_gui(conn_name, Some("ssh"))?;
            let ssh_handle2 = ssh::dial_and_authenticate(&state.app, &ssh_config2, false).await?;
            {
                let channel = ssh_handle2.channel_open_session().await.map_err(|e| format!("打开通道失败: {}", e))?;
                let extract_cmd = format!(
                    "sudo mkdir -p {} && sudo mv {} {} && cd {} && sudo tar -xzf {} && sudo rm -f {} && echo 'UPLOAD_OK'",
                    shell_quote(&remote_target),
                    shell_quote(&remote_tar_path),
                    shell_quote(&remote_target),
                    shell_quote(&remote_target),
                    shell_quote(&remote_tar_name),
                    shell_quote(&remote_tar_name),
                );
                channel.exec(true, extract_cmd).await.map_err(|e| format!("解压失败: {}", e))?;
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let _ = ssh_handle2.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            log(&format!("[upload_project] 完成! {} → {}", local_dir, remote_target));

            Ok(json!({ "content": [{ "type": "text", "text": format!("✅ 项目上传成功！\n本地: {}\n远程: {}\n大小: {} bytes (tar.gz)", local_dir, remote_target, tar_size) }] }))
        }

        "download_project" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let remote_dir = args["remote_dir"].as_str().ok_or("缺少 remote_dir 参数")?;
            let local_dir = args["local_dir"].as_str().ok_or("缺少 local_dir 参数")?;

            let detail = format!("从服务器 [{}] 下载目录 {} → 本地 {}", conn_name, remote_dir, local_dir);
            if !confirm_dangerous_operation("download_project（下载项目目录）", &detail) {
                return Ok(json!({ "content": [{ "type": "text", "text": denied_by_user_text("download_project", &detail) }], "isError": true }));
            }

            // 1. 验证远程目录存在（通过 SFTP，支持 ssh/sftp 连接）
            let config = state.resolve_via_gui(conn_name, None)?;
            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;
            let sftp = open_sftp(&handle).await?;
            let remote_path = std::path::Path::new(remote_dir);
            let dir_name = remote_path.file_name()
                .and_then(|n| n.to_str())
                .ok_or("无法提取远程目录名")?;

            // 验证远程目录存在（通过 SFTP canonicalize）
            let _ = sftp.canonicalize(remote_dir).await
                .map_err(|e| format!("远程目录不存在: {} ({})", remote_dir, e))?;

            // 2. 在服务器上打包到临时文件（用 Python tarfile 处理中文文件名）。
            // SECURITY: paths are passed via environment variables instead of
            // string interpolation — remote_dir can contain quotes/backslashes
            // that would otherwise break out of the old inline Python literal
            // into arbitrary code execution. The Python body itself uses only
            // double quotes so it can sit inside a single-quoted shell string.
            let tmp_tar = format!("/tmp/_dl_project_{}.tar.gz", dir_name);
            let tar_cmd = format!(
                "MYTAR={} MYDIR={} python3 -c '{}' 2>&1",
                shell_quote(&tmp_tar),
                shell_quote(remote_dir),
                "import tarfile, os\n\
                 exclude = {\".venv\", \"venv\", \"__pycache__\", \".git\", \"node_modules\", \"dist\", \"build\", \"target\"}\n\
                 with tarfile.open(os.environ[\"MYTAR\"], \"w:gz\") as tf:\n\
                     for root, dirs, files in os.walk(os.environ[\"MYDIR\"]):\n\
                         dirs[:] = [d for d in dirs if d not in exclude and not d.startswith(\".\")]\n\
                         for f in files:\n\
                             if f.endswith(\".pyc\"): continue\n\
                             full = os.path.join(root, f)\n\
                             arcname = os.path.relpath(full, os.environ[\"MYDIR\"])\n\
                             tf.add(full, arcname=arcname)\n\
                 print(\"TAR_OK\")"
            );
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            // 用 ssh_exec 执行打包
            let ssh_config = state.resolve_via_gui(conn_name, Some("ssh"))?;
            let ssh_handle = ssh::dial_and_authenticate(&state.app, &ssh_config, false).await?;
            {
                let channel = ssh_handle.channel_open_session().await.map_err(|e| format!("打开通道失败: {}", e))?;
                channel.exec(true, tar_cmd).await.map_err(|e| format!("打包命令执行失败: {}", e))?;
            }
            // 等待打包完成
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let _ = ssh_handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            // 3. 用 SFTP 下载 tar.gz 到本地临时文件
            let local_tar = std::env::temp_dir().join(format!("_dl_project_{}.tar.gz", dir_name));
            let local_tar_str = local_tar.to_str().ok_or("临时路径无效")?;

            let sftp_config = state.resolve_via_gui(conn_name, None)?;
            let sftp_handle = ssh::dial_and_authenticate(&state.app, &sftp_config, false).await?;
            let sftp2 = open_sftp(&sftp_handle).await?;
            sftp_download_file(&sftp2, &tmp_tar, local_tar_str).await?;
            let _ = sftp_handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            // 4. 清理服务器上的临时文件（dir_name 来自远端路径，需转义）
            let cleanup_config = state.resolve_via_gui(conn_name, Some("ssh"))?;
            let cleanup_handle = ssh::dial_and_authenticate(&state.app, &cleanup_config, false).await?;
            {
                let channel = cleanup_handle.channel_open_session().await.map_err(|e| format!("打开通道失败: {}", e))?;
                channel.exec(true, format!("sudo rm -f {}", shell_quote(&tmp_tar))).await.ok();
            }
            let _ = cleanup_handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            // 5. 在本地解压
            let local_path = std::path::Path::new(local_dir);
            std::fs::create_dir_all(local_path).map_err(|e| format!("创建本地目录失败: {}", e))?;
            let extract_output = std::process::Command::new("tar")
                .args(["-xzf", local_tar_str, "-C", local_dir])
                .output()
                .map_err(|e| format!("解压失败: {}（确保系统有 tar 命令）", e))?;

            // Record the size BEFORE removing the temp file (the old order
            // always reported 0 because metadata was read after the delete).
            let tar_size = std::fs::metadata(local_tar_str).map(|m| m.len()).unwrap_or(0);
            // 清理本地临时文件
            std::fs::remove_file(&local_tar).ok();

            if !extract_output.status.success() {
                let stderr = String::from_utf8_lossy(&extract_output.stderr);
                return Err(format!("解压失败: {}", stderr));
            }

            Ok(json!({ "content": [{ "type": "text", "text": format!("✅ 项目下载成功！\n远程: {}\n本地: {}\n大小: {} bytes (tar.gz)", remote_dir, local_dir, tar_size) }] }))
        }

        "zmodem_download" => zmodem_download_tool(state, args).await,

        "zmodem_upload" => zmodem_upload_tool(state, args).await,

        "zmodem_status" => {
            let task_id = args["task_id"].as_str().ok_or("缺少 task_id 参数")?;
            let task = {
                let tasks = state.tasks.lock().map_err(|e| e.to_string())?;
                tasks.get(task_id).cloned().ok_or_else(|| {
                    format!("未找到任务 {}。任务可能在 MCP 进程重启后丢失，或 task_id 错误。", task_id)
                })?
            };
            Ok(task.to_status_text(task_id))
        }

        "test_connection" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            // Prefer an SSH connection for reachability probing: ssh exercises
            // the full dial + auth + channel path and is the most common probe
            // target. This also auto-disambiguates hosts saved as BOTH ssh and
            // sftp (e.g. "135.32.64.30") — the ssh entry wins instead of
            // erroring with "匹配到 2 条". If no ssh connection matches the
            // query, fall back to any type so ftp / local / sftp-only hosts
            // still work. A genuine ambiguity WITHIN the ssh type (two ssh
            // connections, same host) is still surfaced to the caller.
            let config = match state.resolve_via_gui(conn_name, Some("ssh")) {
                Ok(c) => c,
                Err(e) if e.contains("未找到") => state.resolve_via_gui(conn_name, None)?,
                Err(e) => return Err(e),
            };

            let result = match config.conn_type.as_str() {
                "ssh" | "sftp" => ssh::test_connection(&state.app, &config).await,
                "ftp" => ftp::test_connection(&config).await,
                "local" => local::test_connection(&config).await,
                _ => Err(format!("未知类型: {}", config.conn_type)),
            };

            match result {
                Ok(msg) => Ok(json!({ "content": [{ "type": "text", "text": msg }] })),
                Err(e) => Err(e),
            }
        }

        "screenshot_terminal" => {
            // Architecture note: this MCP server runs as an independent
            // process from the GUI. It has no access to the GUI's xterm DOM
            // tree, so it cannot capture the screenshot itself. Instead it
            // resolves the connection (to confirm it exists and is the one
            // the user means) and returns a verbatim instruction the AI
            // should relay to the user — the user clicks 📷 in the GUI.
            //
            // This honest contract avoids pretending the tool can do what it
            // can't. A future IPC-backed variant (MCP writes a trigger file,
            // GUI watches and captures) is tracked as future work.
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            // Full decrypted lookup — the vault gate at the top of call_tool
            // already verified the GUI is running and unlocked. (The old
            // plaintext lookup left host/username EMPTY because those columns
            // are encrypted, so the instruction text showed a bare "@".)
            let config = state.resolve_via_gui(conn_name, None)?;
            let attachment_dir = secrets_attachment_dir();

            let attachment_hint = match attachment_dir {
                Some(d) => format!("截图将保存到：{}", d),
                None => "⚠️ 尚未配置附件目录。请先在「设置 → MCP 支持 → 附件目录」中配置。".to_string(),
            };

            let instruction = format!(
                "请用户在 MyShell GUI 中完成截图操作：\n\n\
                 1. 切换到连接 [{name}]（{user}@{host}）所在的终端标签页\n\
                 2. 在终端上方的工具栏点击 📷 截图按钮（位于「快捷」按钮旁边）\n\
                 3. 截图会自动保存，底部会显示保存路径\n\n\
                 {hint}\n\n\
                 注意：截图仅包含终端区域，不含工具栏/输入栏/标签栏。\n\
                 截图完成后，用户可以告诉你保存路径，或直接把截图拖到对话里分享给你。",
                name = config.name,
                user = config.username,
                host = config.host,
                hint = attachment_hint,
            );

            Ok(json!({ "content": [{ "type": "text", "text": instruction }] }))
        }

        "open_in_gui" => {
            // Resolve the connection id via plaintext lookup — NO DEK needed.
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let conn_id = state.find_connection_id(conn_name, None)?;
            // Also grab the plaintext record for the success message (name/conn_type).
            let db = state.app.db.lock().map_err(|e| e.to_string())?;
            let conns = db::get_all_connections_plaintext(&db).map_err(|e| e.to_string())?;
            drop(db);
            let info = conns.iter().find(|c| c.id == conn_id);

            // tab_type: "auto" (default) | "terminal" | "sftp".
            let tab_type = match args["tab_type"].as_str().unwrap_or("auto") {
                "terminal" => "terminal",
                "sftp" => "sftp",
                _ => "auto",
            };

            // Discover the GUI's IPC endpoint (port + auth token).
            let ep = read_gui_ipc_endpoint().ok_or_else(|| {
                "MyShell GUI 未运行（找不到 IPC 端口文件）。请先打开 MyShell 桌面应用，然后重试。".to_string()
            })?;

            // Send the open command over localhost TCP. The GUI focuses an
            // existing matching tab if one is open (focus_existing=true),
            // otherwise opens a new tab of the requested type.
            let result = send_gui_open_command(&ep, &conn_id, tab_type, true).map_err(|e| {
                format!("无法与 MyShell GUI 通信（{}）。请确认 MyShell 桌面应用正在运行。", e)
            })?;

            if result {
                let conn_type = info.map(|c| c.conn_type.as_str()).unwrap_or("");
                let display_name = info.map(|c| c.name.as_str()).unwrap_or(conn_name);
                let kind = match tab_type {
                    "sftp" => "SFTP 文件浏览标签页",
                    "terminal" => "终端标签页",
                    _ => if conn_type == "sftp" { "SFTP 文件浏览标签页" } else { "终端标签页" },
                };
                Ok(json!({ "content": [{ "type": "text", "text": format!(
                    "✅ 已在 MyShell GUI 中打开连接 [{}] 的{}，窗口已聚焦到前台。若该连接已有打开的标签页，则直接切换过去（不重复打开）。用户现在可以直接交互操作。",
                    display_name, kind
                ) }] }))
            } else {
                Err("GUI 收到了命令但未能打开连接（可能连接配置有误）。".to_string())
            }
        }

        _ => Err(format!("未知工具: {}", name)),
    }
}

/// Read the configured attachment directory from the same marker file the GUI
/// uses (`<config_dir>/myshell/attachment-dir`). Returns None if not set.
fn secrets_attachment_dir() -> Option<String> {
    let mut path = dirs::config_dir()?;
    path.push("myshell");
    path.push("attachment-dir");
    let raw = std::fs::read_to_string(&path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

/// Read the command confirmation rules from the JSON config file written by
/// the GUI (`<config_dir>/myshell/mcp-command-rules.json`). Returns the
/// built-in defaults if the file is missing or unparseable, so the MCP server
/// always has sane rules even on first run.
fn load_command_rules() -> command_rules::CommandRules {
    let mut path = match dirs::config_dir() {
        Some(d) => d,
        None => return command_rules::CommandRules::default(),
    };
    path.push("myshell");
    path.push("mcp-command-rules.json");
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => command_rules::CommandRules::default(),
    }
}

// ============ GUI IPC client (open_in_gui) ============
//
// The GUI writes its IPC listener port to `<config_dir>/myshell/gui-ipc-port`
// on startup and deletes it on exit. We read that file to discover the port,
// then open a localhost TCP connection and send a one-line JSON command.

/// GUI IPC endpoint discovered from `<config_dir>/myshell/gui-ipc-port`.
/// File format: `"<port>\n<token>"` — the per-session random token the GUI
/// requires on every command (auth for the machine-reachable localhost
/// listener; the file itself is protected by user-profile ACLs). The token
/// line is absent in pre-token GUI versions — requests then omit the field,
/// which an old GUI accepts.
struct GuiIpcEndpoint {
    port: u16,
    token: Option<String>,
}

/// Attach the auth token (when the GUI wrote one) to an IPC request.
fn ipc_cmd(ep: &GuiIpcEndpoint, mut v: Value) -> Value {
    if let Some(t) = &ep.token {
        v["token"] = Value::String(t.clone());
    }
    v
}

fn read_gui_ipc_endpoint() -> Option<GuiIpcEndpoint> {
    let mut path = dirs::config_dir()?;
    path.push("myshell");
    path.push("gui-ipc-port");
    let raw = std::fs::read_to_string(&path).ok()?;
    let mut parts = raw.split_whitespace();
    let port = parts.next()?.parse::<u16>().ok()?;
    let token = parts.next().map(|s| s.to_string());
    Some(GuiIpcEndpoint { port, token })
}

/// Send an "open_connection" command to the GUI over localhost TCP and wait
/// for the one-line JSON response. Returns true if the GUI acknowledged
/// success (`{"ok":true}`), false otherwise.
///
/// `tab_type` is "auto" | "terminal" | "sftp". `focus_existing` tells the GUI
/// to switch to an already-open matching tab instead of opening a duplicate.
fn send_gui_open_command(ep: &GuiIpcEndpoint, connection_id: &str, tab_type: &str, focus_existing: bool) -> Result<bool, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    let mut stream = TcpStream::connect(("127.0.0.1", ep.port))
        .map_err(|e| format!("TCP 连接失败: {}", e))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("设置超时失败: {}", e))?;

    // Send the command as one NDJSON line.
    let cmd = ipc_cmd(ep, serde_json::json!({
        "action": "open_connection",
        "connection_id": connection_id,
        "tab_type": tab_type,
        "focus_existing": focus_existing,
    }));
    writeln!(stream, "{}", cmd).map_err(|e| format!("发送命令失败: {}", e))?;
    stream.flush().map_err(|e| format!("flush 失败: {}", e))?;

    // Read the one-line response.
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).map_err(|e| format!("读取响应失败: {}", e))?;

    let resp: serde_json::Value = serde_json::from_str(resp_line.trim())
        .map_err(|e| format!("响应解析失败: {} (raw: {})", e, resp_line.trim()))?;

    Ok(resp["ok"].as_bool().unwrap_or(false))
}

/// Ask the GUI to decrypt a connection's credentials and return the full
/// decrypted ConnectionConfig. The MCP server no longer holds the DEK or a
/// stored passphrase — this is the ONLY way to obtain credentials for
/// headless SFTP operations. Requires the GUI to be running and the vault
/// to be unlocked (user must have entered the master password in the GUI).
fn get_config_from_gui(conn_id: &str) -> Result<ConnectionConfig, String> {
    let ep = read_gui_ipc_endpoint().ok_or_else(|| {
        "MyShell GUI 未运行。请先打开 MyShell 桌面应用并输入主密码解锁保险库，然后重试。".to_string()
    })?;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    let mut stream = TcpStream::connect(("127.0.0.1", ep.port))
        .map_err(|e| format!("连接 GUI 失败: {e}"))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("设置超时失败: {}", e))?;
    let cmd = ipc_cmd(&ep, serde_json::json!({
        "action": "get_connection_secrets",
        "connection_id": conn_id,
    }));
    writeln!(stream, "{}", cmd).map_err(|e| format!("发送命令失败: {e}"))?;
    stream.flush().map_err(|e| format!("flush 失败: {e}"))?;
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).map_err(|e| format!("读取响应失败: {e}"))?;
    let resp: Value = serde_json::from_str(resp_line.trim())
        .map_err(|e| format!("响应解析失败: {e} (raw: {})", resp_line.trim()))?;
    if resp["ok"].as_bool() == Some(true) {
        let config: ConnectionConfig = serde_json::from_value(resp["config"].clone())
            .map_err(|e| format!("配置反序列化失败: {e}"))?;
        Ok(config)
    } else {
        let err = resp["error"].as_str().unwrap_or("未知错误");
        Err(err.to_string())
    }
}

// ============ GUI exec_in_tab (show_in_gui mode) ============

/// Ensure the MyShell GUI is running. If the IPC port file exists, VERIFY the
/// port actually responds (a crashed / force-killed GUI leaves a stale port
/// file behind, which used to make every tool call die with a confusing IPC
/// timeout). Otherwise, spawn `myshell.exe` from the same directory as this
/// MCP binary and poll for the port file to appear (up to 30s).
fn ensure_gui_running() -> Result<GuiIpcEndpoint, String> {
    if let Some(ep) = read_gui_ipc_endpoint() {
        // Distinguish two failure modes by whether ANYTHING is listening:
        //
        //   connect refused  → the listener is gone, so the GUI process is
        //     gone too (its exit cleanup never ran — crash / task-manager
        //     kill). The port file is stale: remove it and relaunch. This
        //     is safe — a live GUI always has its listener bound.
        //
        //   connect ok but no IPC response → a process IS listening but not
        //     answering. Do NOT relaunch: blindly starting a second GUI would
        //     pop the "覆盖启动?" dialog and could force-kill the (possibly
        //     just busy) running instance, losing the user's open sessions.
        //     Surface an error and let the user decide.
        let listener_alive = std::net::TcpStream::connect(("127.0.0.1", ep.port)).is_ok();
        if listener_alive {
            if query_gui_vault_status(&ep).is_some() {
                return Ok(ep);
            }
            return Err(
                "MyShell GUI 正在运行但 IPC 无响应（进程可能已挂起）。请在任务管理器中检查 myshell.exe，必要时手动重启 MyShell 后重试。"
                    .to_string(),
            );
        }
        log(&format!(
            "[gui] 陈旧的 gui-ipc-port 文件（端口 {} 无监听）— 删除并重新启动 GUI",
            ep.port
        ));
        remove_gui_ipc_port_file();
    }

    // Find myshell.exe — it lives in the same directory as myshell-mcp.exe.
    let mcp_exe = std::env::current_exe()
        .map_err(|e| format!("无法定位自身路径: {e}"))?;
    let gui_exe = mcp_exe
        .parent()
        .ok_or("无法定位安装目录")?
        .join("myshell.exe");

    if !gui_exe.exists() {
        return Err(format!(
            "未找到 myshell.exe（期望位置: {}）。请手动打开 MyShell。",
            gui_exe.display()
        ));
    }

    log(&format!("启动 GUI: {}", gui_exe.display()));
    std::process::Command::new(&gui_exe)
        .spawn()
        .map_err(|e| format!("启动 myshell.exe 失败: {e}"))?;

    // Poll for the port file (GUI writes it on startup once the IPC listener
    // binds). Give it up to 30 seconds — the GUI needs to boot WebView2.
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Some(ep) = read_gui_ipc_endpoint() {
            log(&format!("GUI 就绪，IPC port = {}", ep.port));
            return Ok(ep);
        }
    }
    Err("GUI 启动超时（30 秒内未就绪）。请手动打开 MyShell 后重试。".to_string())
}

/// Delete the GUI's IPC port-discovery file. Only called when we have proven
/// nothing is listening on that port (stale file from a crashed GUI), so a
/// freshly launched GUI can publish its real port without confusion.
fn remove_gui_ipc_port_file() {
    let mut path = match dirs::config_dir() {
        Some(d) => d,
        None => return,
    };
    path.push("myshell");
    path.push("gui-ipc-port");
    let _ = std::fs::remove_file(&path);
}

/// Query the GUI's vault state via localhost IPC. Returns:
///   Some((initialized, unlocked)) — got a valid response
///   None                           — GUI unreachable or malformed response
///
/// `initialized` = a vault salt file exists on disk (user has set up a master
/// password before). `unlocked` = the DEK is currently loaded in the GUI's
/// AppState (user has entered the master password this session).
fn query_gui_vault_status(ep: &GuiIpcEndpoint) -> Option<(bool, bool)> {
    use std::io::{BufRead, BufReader, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", ep.port)).ok()?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok()?;
    let cmd = ipc_cmd(ep, serde_json::json!({ "action": "vault_status" }));
    writeln!(stream, "{}", cmd).ok()?;
    stream.flush().ok()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    // The GUI always sets `ok=true` for vault_status (it can't fail short of
    // the IPC itself failing, which we handle via None above).
    let initialized = v["initialized"].as_bool().unwrap_or(false);
    let unlocked = v["unlocked"].as_bool().unwrap_or(false);
    Some((initialized, unlocked))
}

/// Ask the GUI to bring its window (and thus the master-password gate) to the
/// front. Fire-and-forget: any failure is ignored — the error text we return
/// to the AI already tells the user what to do, focusing is just a courtesy.
fn send_gui_focus_unlock(ep: &GuiIpcEndpoint) {
    use std::io::{BufRead, BufReader, Write};
    let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", ep.port)) else {
        return;
    };
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let cmd = ipc_cmd(ep, serde_json::json!({ "action": "focus_unlock" }));
    if writeln!(stream, "{}", cmd).is_ok() {
        let _ = stream.flush();
        // Drain the one-line {"ok":true} response so the connection closes cleanly.
        let mut buf = String::new();
        let _ = BufReader::new(stream).read_line(&mut buf);
    }
}

/// Fail-fast vault gate. Called at the top of every tool handler that needs
/// anything beyond task-status polling (ssh_exec, sftp_*, list_connections,
/// open_in_gui, screenshot_terminal, and the async starters' pre-flight).
///
/// Behavior:
///   1. Ensure the GUI is running — AUTO-LAUNCH myshell.exe if it isn't
///      (waits up to 30s for the IPC port file while it boots).
///   2. Query the GUI's vault state ONCE (a few retries only to cover a
///      freshly launched GUI still binding its IPC listener).
///   3. Unlocked → Ok, proceed with the tool.
///      Not initialized → immediate actionable error.
///      Locked → focus the GUI so the password gate is in the user's face,
///      then return an immediate error telling the AI (and the user) to
///      unlock and RETRY the same tool.
///
/// DELIBERATELY no 30-second silent poll anymore: the old wait was the #1
/// source of "the tool hung forever, then vaguely suggested the password
/// might be missing" reports. Failing fast surfaces the prompt within
/// milliseconds; the AI relays it, the user unlocks, the retry succeeds.
fn ensure_vault_ready() -> Result<(), String> {
    let ep = ensure_gui_running().map_err(|e| {
        format!("MyShell GUI 未运行且无法自动启动：{}。请手动打开 MyShell 桌面应用，然后重试。", e)
    })?;

    // Right after auto-launch the IPC listener may need a moment; retry the
    // status query briefly before concluding the GUI is unreachable.
    let mut status = None;
    for _ in 0..5 {
        if let Some(s) = query_gui_vault_status(&ep) {
            status = Some(s);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    match status {
        Some((_, true)) => Ok(()),
        Some((false, _)) => Err(
            "保险库尚未初始化：请先在 MyShell GUI 中设置主密码（首次启动时会引导设置），然后重新调用此工具。"
                .to_string(),
        ),
        Some((true, false)) => {
            send_gui_focus_unlock(&ep);
            Err(
                "保险库未解锁：MyShell 窗口已置顶，请在其中输入主密码解锁保险库，解锁后重新调用此工具即可。"
                    .to_string(),
            )
        }
        None => Err(
            "无法查询 MyShell GUI 的保险库状态（IPC 无响应）。请确认 MyShell 桌面应用正在运行后重试。"
                .to_string(),
        ),
    }
}

/// Send an "exec_in_tab" command to the GUI: run `command` in a visible
/// terminal tab for `connection_id`, block until the GUI returns the result
/// (stdout + exit_code), and format it as an MCP tool response.
///
/// This is the "show_in_gui" path for ssh_exec. The GUI opens a tab, sends the
/// command + a sentinel marker to the PTY, captures the output, and sends it
/// back over the IPC connection.
async fn exec_in_gui_tab(
    conn_name: &str,
    command: &str,
    timeout: u64,
    state: &McpState,
) -> Result<Value, String> {
    // Resolve the connection id via plaintext lookup — NO DEK needed.
    // The actual SSH connection + credential use happens inside the GUI
    // process (the user has unlocked the vault there).
    let conn_id = state.find_connection_id(conn_name, Some("ssh"))?;

    // Ensure the GUI is running (auto-start if needed).
    let ep = ensure_gui_running()?;

    // 直接发送 exec_in_tab。前端的 handler 自己会处理三种情况：
    //   1. 该连接已有 connected 的 terminal tab → 直接复用（秒级）
    //   2. 该连接有 tab 但 status 是 disconnected/error → 原地重连（复用同 tab）
    //   3. 没有 tab → 开新 tab + 连接
    // 所以这里不再提前 open_connection，也不 sleep —— 那两个操作在会话已存在时
    // 是纯浪费（旧实现因此每次多耗 5-6 秒），在会话不存在时会和 exec_in_tab
    // 内部的连接逻辑产生竞态（同服务器被连两次）。
    log(&format!("[exec_in_gui_tab] exec_in_tab: {} (timeout={}s)", conn_name, timeout));

    // Send the exec_in_tab command and read the response.
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    let mut stream = TcpStream::connect(("127.0.0.1", ep.port))
        .map_err(|e| format!("TCP 连接 GUI 失败: {e}"))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(timeout + 15)))
        .map_err(|e| format!("设置超时失败: {e}"))?;

    let cmd = ipc_cmd(&ep, serde_json::json!({
        "action": "exec_in_tab",
        "connection_id": conn_id,
        "command": command,
        "timeout": timeout,
    }));
    writeln!(stream, "{}", cmd).map_err(|e| format!("发送命令失败: {e}"))?;
    stream.flush().map_err(|e| format!("flush 失败: {e}"))?;

    // Read the response — a JSON object with {ok, stdout?, exit_code?, error?}.
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).map_err(|e| format!("读取响应失败: {e}"))?;

    let resp: Value = serde_json::from_str(resp_line.trim())
        .map_err(|e| format!("响应解析失败: {e} (raw: {})", resp_line.trim()))?;

    if resp["ok"].as_bool() == Some(true) {
        let stdout = resp["stdout"].as_str().unwrap_or("");
        let exit_code = resp["exit_code"].as_i64().unwrap_or(0);
        let result = json!({
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": "",
        });
        Ok(json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result).unwrap_or_default() }] }))
    } else {
        let err_msg = resp["error"].as_str().unwrap_or("未知错误");
        // If there's partial stdout, include it.
        let stdout = resp["stdout"].as_str().unwrap_or("");
        if !stdout.is_empty() {
            Err(format!("{}（部分输出: {}）", err_msg, stdout))
        } else {
            Err(err_msg.to_string())
        }
    }
}

// ============ ZMODEM (lrzsz) helpers ============
//
// These two tools (zmodem_download / zmodem_upload) provide ZMODEM file
// transfers against the remote `sz` / `rz` programs — useful when the SFTP
// subsystem isn't available (restricted shells, jump hosts, embedded devices).
// They open a real interactive SSH session (PTY), invoke `sz`/`rz`, and drive
// the ZMODEM protocol via the core library's `zmodem_rx` (download) /
// `zmodem_tx` (upload) state machines.
//
// Both reuse the GUI-vault credential flow: the MCP server never holds the DEK;
// `resolve_via_gui` asks the running GUI to decrypt the connection config.

/// A captured terminal event used to coordinate the ZMODEM state machines with
/// the SSH channel reader (`ssh::connect`'s background task, which emits events
/// via the `EventSink`).
#[derive(Clone, Debug)]
#[allow(dead_code)]
enum ZmodemEvent {
    /// Raw bytes produced by the remote (sz/rz) — feed into the state machine.
    Raw(Vec<u8>),
    /// `zmodem_start`: the reader detected ZRINIT (upload) or a sz offer
    /// (download). For uploads this is the cue to hand the local file to the
    /// reader's NATIVE pump (the fast path the GUI uses).
    Start { direction: String },
    /// A file offer from remote `sz` (download): { name, size }.
    Offer { name: String, size: u64 },
    /// Progress: { bytes_transferred, bytes_total }. Currently informational
    /// only — the driver loops don't act on it (throughput comes back via the
    /// sender's own Progress events for uploads). Kept for future surfacing.
    #[allow(dead_code)]
    Progress { transferred: u64, total: u64 },
    /// A file finished downloading.
    FileComplete { name: String, bytes: u64 },
    /// The remote signaled the ZMODEM session has ended.
    End,
    /// A protocol error.
    Error(String),
    /// Plain terminal output (sz/rz error text, shell noise). Captured so we
    /// can include it in a diagnostic on failure (e.g. "lrzsz not installed").
    Terminal(Vec<u8>),
    /// The underlying SSH session closed.
    Closed,
}

/// `EventSink` that funnels every ssh.rs event into an mpsc channel as a
/// `ZmodemEvent`. Built per-tool-invocation; dropped when the tool returns.
struct McpZmodemSink {
    tx: std::sync::Mutex<tokio::sync::mpsc::UnboundedSender<ZmodemEvent>>,
}

impl McpZmodemSink {
    fn new(tx: tokio::sync::mpsc::UnboundedSender<ZmodemEvent>) -> Self {
        Self {
            tx: std::sync::Mutex::new(tx),
        }
    }
}

impl EventSink for McpZmodemSink {
    fn emit_raw(&self, event: &str, payload: Value) {
        // DEBUG: log every event to the MCP log file for upload-path diagnosis.
        let preview = if event == "zmodem_raw" || event == "ssh_output" {
            let len = payload["data"].as_array().map(|a| a.len()).unwrap_or(0);
            format!("<{} bytes>", len)
        } else {
            format!("{}", payload)
        };
        log(&format!("[McpZmodemSink] event={} {}", event, preview));

        let parsed = match event {
            "zmodem_raw" => payload["data"]
                .as_array()
                .map(|arr| {
                    let bytes: Vec<u8> = arr.iter().filter_map(|v| v.as_u64().map(|n| n as u8)).collect();
                    ZmodemEvent::Raw(bytes)
                }),
            "zmodem_start" => Some(ZmodemEvent::Start {
                direction: payload["direction"].as_str().unwrap_or("").to_string(),
            }),
            "zmodem_offer" => Some(ZmodemEvent::Offer {
                name: payload["fileName"].as_str().unwrap_or("unknown").to_string(),
                size: payload["fileSize"].as_u64().unwrap_or(0),
            }),
            "zmodem_progress" => Some(ZmodemEvent::Progress {
                transferred: payload["bytesTransferred"].as_u64().unwrap_or(0),
                total: payload["bytesTotal"].as_u64().unwrap_or(0),
            }),
            "zmodem_file_complete" => Some(ZmodemEvent::FileComplete {
                name: payload["fileName"].as_str().unwrap_or("").to_string(),
                bytes: payload["bytesWritten"].as_u64().unwrap_or(0),
            }),
            "zmodem_end" => Some(ZmodemEvent::End),
            "zmodem_error" => Some(ZmodemEvent::Error(
                payload["message"].as_str().unwrap_or("zmodem error").to_string(),
            )),
            "ssh_output" => payload["data"]
                .as_array()
                .map(|arr| {
                    let bytes: Vec<u8> = arr.iter().filter_map(|v| v.as_u64().map(|n| n as u8)).collect();
                    ZmodemEvent::Terminal(bytes)
                }),
            "ssh_closed" => Some(ZmodemEvent::Closed),
            _ => None,
        };
        if let Some(ev) = parsed {
            // best-effort send — if the receiver was dropped (tool returned),
            // there's nobody to handle the event anyway.
            let _ = self.tx.lock().map(|tx| tx.send(ev));
        }
    }
}

/// Sanitize an untrusted filename (from a remote `sz` offer) into a safe local
/// basename. Strips directory components, `..`, drive letters, and characters
/// illegal on Windows. Returns None if nothing usable remains.
fn sanitize_remote_basename(name: &str) -> Option<String> {
    // Take the last path component (handles both / and \).
    let leaf = name.rsplit(['/', '\\']).next()?.trim();
    if leaf.is_empty() || leaf == "." || leaf == ".." {
        return None;
    }
    let cleaned: String = leaf
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    // Trim trailing dots/spaces (Windows quirk).
    let cleaned = cleaned.trim_end_matches(['.', ' ']).to_string();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Join a directory and a basename into an absolute local path, producing a
/// platform-appropriate separator.
fn join_local_path(dir: &str, name: &str) -> String {
    let sep = if dir.contains('\\') && !dir.contains('/') {
        '\\'
    } else {
        std::path::MAIN_SEPARATOR
    };
    let dir = dir.trim_end_matches(['/', '\\']);
    format!("{}{}{}", dir, sep, name)
}

/// Single-quote `s` for safe interpolation into a POSIX shell command.
/// Anything inside single quotes is literal; an embedded quote uses the
/// standard close-quote/escaped-quote/reopen dance (`'\''`).
///
/// SECURITY: every remote path that ends up inside a shell command string
/// (upload_project's sudo chain, download_project's tar/cleanup, zmodem's
/// sz/rz/cd) MUST go through this — the paths originate from the AI/user and
/// may contain quotes, which would otherwise break out into arbitrary command
/// execution (with sudo in the worst case). Note the older `replace('\'',
/// "'\"\"'")` trick used elsewhere DROPPED the apostrophe instead of escaping
/// it — this helper replaces it everywhere.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// zmodem_download: run `sz <remote_path>` on the server, receive the file via
/// the native ZMODEM receiver, and write it to `local_dir`.
///
/// ASYNC: returns immediately with a task_id. The actual transfer runs in a
/// background task. Poll progress with `zmodem_status(task_id)`.
async fn zmodem_download_tool(state: &McpState, args: &Value) -> Result<Value, String> {
    let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?.to_string();
    let remote_path = args["remote_path"].as_str().ok_or("缺少 remote_path 参数")?.to_string();
    let local_dir = args["local_dir"].as_str().ok_or("缺少 local_dir 参数")?.to_string();
    let timeout = args["timeout"].as_u64().unwrap_or(120);

    // Validate the local destination directory exists (fail fast, before spawn).
    std::fs::metadata(&local_dir)
        .map_err(|e| format!("本地目录不存在或不可访问 [{}]: {}", local_dir, e))?;

    // NOTE: the GUI/vault gate has already run synchronously at the top of
    // call_tool (auto-launch + fail-fast unlock check), so by the time we get
    // here the vault is unlocked. Only the credential decryption and the
    // actual transfer are deferred to the background task — doing THOSE here
    // would block the tool-call path and risk the client's 30s timeout. The
    // background task re-checks the vault as a race guard (auto-lock may fire
    // mid-flight) and reports failures via the task table.

    // Create the task entry.
    let task_id = format!("zm-dl-{}", chrono_like_ts());
    let description = format!("下载 {} 的 {} → {}", conn_name, remote_path, local_dir);
    {
        let mut tasks = state.tasks.lock().map_err(|e| e.to_string())?;
        tasks.insert(
            task_id.clone(),
            TransferTask::new("download", description.clone(), 0),
        );
    }

    // Spawn the background driver.
    let app = state.app.clone();
    let tasks_table = Arc::clone(&state.tasks);
    let tid = task_id.clone();
    let bg_conn = conn_name.clone();
    let bg_remote = remote_path.clone();
    let bg_local = local_dir.clone();
    tokio::spawn(async move {
        run_download_task(&app, &tasks_table, &tid, &bg_conn, &bg_remote, &bg_local, timeout).await;
    });

    // Return immediately with the task handle so the client's 30s window is
    // never exceeded.
    Ok(json!({
        "content": [{
            "type": "text",
            "text": format!(
                "⏳ ZMODEM 下载已启动（任务 {}）。正在后台传输：{} → {}\n请用 zmodem_status 查询进度（建议每 5-10 秒轮询一次）。文件名和大小在收到远端 sz 的 offer 后才能确定。",
                task_id, remote_path, local_dir
            )
        }],
        "task_id": task_id,
        "status": "running",
        "poll_with": "zmodem_status"
    }))
}

/// Background driver for a zmodem download. Updates the task table in place.
async fn run_download_task(
    app: &AppState,
    tasks: &Arc<Mutex<HashMap<String, TransferTask>>>,
    task_id: &str,
    conn_name: &str,
    remote_path: &str,
    local_dir: &str,
    timeout: u64,
) {
    let mark = |phase: TaskPhase, bytes_done: u64, bytes_total: u64| {
        let _ = tasks.lock().map(|mut t| {
            if let Some(task) = t.get_mut(task_id) {
                task.phase = phase;
                task.bytes_done = bytes_done;
                task.bytes_total = bytes_total;
            }
        });
    };
    let fail = |msg: String| {
        let _ = tasks.lock().map(|mut t| {
            if let Some(task) = t.get_mut(task_id) {
                task.phase = TaskPhase::Failed;
                task.error = Some(msg.clone());
            }
        });
        log(&format!("zmodem_download [{}] failed: {}", task_id, msg));
    };

    mark(TaskPhase::Connecting, 0, 0);

    // Resolve credentials (ensure GUI running + vault unlocked + decrypt).
    // This can take a while (up to 30s waiting for vault unlock, or launching
    // the GUI), which is why it runs in the background task, not the tool call.
    let config = match resolve_config_for_task(app, conn_name, Some("ssh"), &fail).await {
        Some(c) => c,
        None => return, // fail() already recorded the error
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ZmodemEvent>();
    let sink = Arc::new(McpZmodemSink::new(tx));

    let session_id = match ssh::connect(app, sink.clone(), config).await {
        Ok(id) => id,
        Err(e) => {
            fail(format!("SSH 连接失败: {}", e));
            return;
        }
    };

    let cmd = format!("sz {}\r", shell_quote(remote_path));
    if let Err(e) = ssh::send_input(app, &session_id, cmd.as_bytes()).await {
        let _ = ssh::disconnect(app, &session_id).await;
        fail(format!("发送 sz 命令失败: {}", e));
        return;
    }

    let mut saved: Vec<(String, String, u64)> = Vec::new();
    let mut terminal_noise = Vec::<u8>::new();
    let mut got_offer = false;

    let result: Result<(), String> = loop {
        let next = if !got_offer {
            match tokio::time::timeout(std::time::Duration::from_secs(20), rx.recv()).await {
                Ok(v) => v,
                Err(_) => break Err(format!(
                    "等待 sz 启动超时（20秒未收到文件）。远端可能未安装 lrzsz，或路径不存在。\n终端输出：\n{}",
                    String::from_utf8_lossy(&terminal_noise)
                )),
            }
        } else {
            match tokio::time::timeout(std::time::Duration::from_secs(timeout), rx.recv()).await {
                Ok(v) => v,
                Err(_) => break Err(format!("传输超时（{}秒）", timeout)),
            }
        };

        match next {
            None => break Err("SSH 会话意外关闭".to_string()),
            Some(ev) => match ev {
                ZmodemEvent::Offer { name, size } => {
                    got_offer = true;
                    mark(TaskPhase::Transferring, 0, size);
                    let local_name = sanitize_remote_basename(&name).unwrap_or_else(|| {
                        format!("zmodem_download_{}", chrono_like_ts())
                    });
                    let local_path = join_local_path(local_dir, &local_name);
                    log(&format!(
                        "zmodem_download [{}]: offer {:?} ({} bytes) → {}",
                        task_id, name, size, local_path
                    ));
                    if let Err(e) =
                        ssh::zmodem_accept_offer(app, &session_id, Some(local_path.clone())).await
                    {
                        break Err(format!("接受文件失败: {}", e));
                    }
                }
                ZmodemEvent::FileComplete { name, bytes } => {
                    let local_name = sanitize_remote_basename(&name).unwrap_or_default();
                    let local_path = join_local_path(local_dir, &local_name);
                    saved.push((name, local_path, bytes));
                    mark(TaskPhase::Transferring, bytes, 0);
                }
                ZmodemEvent::Progress { transferred, total } => {
                    mark(TaskPhase::Transferring, transferred, total);
                }
                ZmodemEvent::End => break Ok(()),
                ZmodemEvent::Error(m) => break Err(m),
                ZmodemEvent::Closed => {
                    if saved.is_empty() {
                        break Err(format!(
                            "SSH 会话在传输完成前关闭。\n终端输出：\n{}",
                            String::from_utf8_lossy(&terminal_noise)
                        ));
                    }
                    break Ok(());
                }
                ZmodemEvent::Terminal(bytes) => {
                    terminal_noise.extend_from_slice(&bytes);
                    if terminal_noise.len() > 8 * 1024 {
                        terminal_noise.drain(..terminal_noise.len() - 8 * 1024);
                    }
                }
                ZmodemEvent::Raw(_) | ZmodemEvent::Start { .. } => {}
            },
        }
    };

    if result.is_err() {
        let _ = ssh::zmodem_abort(app, &session_id).await;
    }
    let _ = ssh::disconnect(app, &session_id).await;

    match result {
        Ok(()) => {
            let files: Vec<Value> = saved
                .iter()
                .map(|(name, path, bytes)| json!({ "name": name, "local_path": path, "bytes": bytes }))
                .collect();
            let total: u64 = saved.iter().map(|(_, _, b)| *b).sum();
            let _ = tasks.lock().map(|mut t| {
                if let Some(task) = t.get_mut(task_id) {
                    task.phase = TaskPhase::Done;
                    task.bytes_done = total;
                    task.result = Some(json!({ "files": files }));
                }
            });
        }
        Err(e) => fail(e),
    }
}

/// zmodem_upload: run `rz -y` on the server, then drive the native ZMODEM
/// sender to push `local_path` into the remote `remote_dir`.
async fn zmodem_upload_tool(state: &McpState, args: &Value) -> Result<Value, String> {
    let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?.to_string();
    let local_path = args["local_path"].as_str().ok_or("缺少 local_path 参数")?.to_string();
    let remote_dir = args["remote_dir"].as_str().ok_or("缺少 remote_dir 参数")?.to_string();
    let timeout = args["timeout"].as_u64().unwrap_or(120);

    // Validate the local file up front (fail fast).
    let meta = std::fs::metadata(&local_path)
        .map_err(|e| format!("本地文件不存在或不可访问 [{}]: {}", local_path, e))?;
    if !meta.is_file() {
        return Err(format!("本地路径不是普通文件: {}", local_path));
    }
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let basename = std::path::Path::new(&local_path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "无法从本地路径提取文件名".to_string())?
        .to_string();

    // NOTE: the GUI/vault gate has already run synchronously at the top of
    // call_tool (auto-launch + fail-fast unlock check), so by the time we get
    // here the vault is unlocked. Only the confirmation dialog, credential
    // decryption and the actual transfer are deferred to the background task
    // (the dialog blocks on the user's click, which must not hold the
    // tool-call window). The background task re-checks the vault as a race
    // guard.

    // Create the task entry (starts in Confirming — the OS dialog is popped
    // inside the background task so the tool call returns instantly).
    let task_id = format!("zm-ul-{}", chrono_like_ts());
    let description = format!("上传 {} → {}:{}/{}", local_path, conn_name, remote_dir, basename);
    {
        let mut tasks = state.tasks.lock().map_err(|e| e.to_string())?;
        tasks.insert(
            task_id.clone(),
            TransferTask::new("upload", description.clone(), size),
        );
    }

    // Spawn the background driver (confirmation dialog + transfer).
    let app = state.app.clone();
    let tasks_table = Arc::clone(&state.tasks);
    let tid = task_id.clone();
    let bg_conn = conn_name.clone();
    let bg_local = local_path.clone();
    let bg_basename = basename.clone();
    let bg_remote_dir = remote_dir.clone();
    tokio::spawn(async move {
        run_upload_task(
            &app, &tasks_table, &tid, &bg_conn, &bg_local, &bg_basename,
            size, mtime, &bg_remote_dir, timeout,
        ).await;
    });

    Ok(json!({
        "content": [{
            "type": "text",
                "text": format!(
                    "⏳ ZMODEM 上传已启动（任务 {}）。正在后台准备（随后会弹出系统确认对话框）。文件：{} ({} 字节) → {}/{}\n请用 zmodem_status 查询进度（建议每 5-10 秒轮询一次）。",
                    task_id, local_path, size, remote_dir, basename
                )
        }],
        "task_id": task_id,
        "status": "running",
        "poll_with": "zmodem_status"
    }))
}

/// Background driver for a zmodem upload. Pops the confirmation dialog, then
/// runs the ZMODEM sender. Updates the task table in place.
async fn run_upload_task(
    app: &AppState,
    tasks: &Arc<Mutex<HashMap<String, TransferTask>>>,
    task_id: &str,
    conn_name: &str,
    local_path: &str,
    basename: &str,
    size: u64,
    _mtime: u64,
    remote_dir: &str,
    timeout: u64,
) {
    let mark = |phase: TaskPhase, bytes_done: u64| {
        let _ = tasks.lock().map(|mut t| {
            if let Some(task) = t.get_mut(task_id) {
                task.phase = phase;
                task.bytes_done = bytes_done;
            }
        });
    };
    let fail = |msg: String| {
        let _ = tasks.lock().map(|mut t| {
            if let Some(task) = t.get_mut(task_id) {
                task.phase = TaskPhase::Failed;
                task.error = Some(msg.clone());
            }
        });
        log(&format!("zmodem_upload [{}] failed: {}", task_id, msg));
    };

    // The task is in Confirming. Pop the OS dialog here (blocks the background
    // task, NOT the tool call that already returned).
    let detail = format!(
        "ZMODEM 上传本地文件 [{}] ({} 字节) → {}",
        local_path, size, remote_dir
    );
    if !confirm_dangerous_operation("zmodem_upload（ZMODEM 上传）", &detail) {
        fail(denied_by_user_text("zmodem_upload", &detail));
        return;
    }

    mark(TaskPhase::Connecting, 0);

    // Resolve credentials (ensure GUI running + vault unlocked + decrypt).
    // This can take a while (GUI launch / vault-unlock wait), so it runs in the
    // background task, not the tool call.
    let config = match resolve_config_for_task(app, conn_name, Some("ssh"), &fail).await {
        Some(c) => c,
        None => return, // fail() already recorded the error
    };

    // Pre-validate the local file is readable. The reader task's native pump
    // opens its OWN handle for streaming (see ssh.rs ZmodemStartUpload), so we
    // just fail fast here with a clear message instead of letting ZMODEM stall.
    if let Err(e) = std::fs::File::open(local_path) {
        fail(format!("打开本地文件失败: {}", e));
        return;
    }

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ZmodemEvent>();
    let sink = Arc::new(McpZmodemSink::new(tx));

    let session_id = match ssh::connect(app, sink.clone(), config).await {
        Ok(id) => id,
        Err(e) => {
            fail(format!("SSH 连接失败: {}", e));
            return;
        }
    };

    // Build the `cd` clause. `~` and `~/...` must NOT be single-quoted — the
    // shell won't expand `~` inside quotes, so `cd '~'` fails with "No such
    // file or directory". For paths starting with `~`, emit them unquoted
    // (tilde expansion is safe; `~` has no shell metacharacters). For all
    // other paths, shell_quote single-quotes with proper escaping.
    let cd_target = if remote_dir == "~" || remote_dir.starts_with("~/") {
        remote_dir.to_string()
    } else {
        shell_quote(remote_dir)
    };
    // Launch `rz` directly — no manual `stty`. lrzsz's rz puts the PTY into
    // raw/no-echo mode itself before sending ZRINIT (same as when a user runs
    // Launch rz exactly as a user would in the interactive terminal — NO manual
    // stty. lrzsz's rz manages termios itself (sets raw on entry, restores on
    // exit). Forcing stty raw beforehand conflicts with rz's own setup and the
    // cooked-mode echo leaks into the byte stream.
    let cmd = format!("cd {} && rz -y\r", cd_target);
    if let Err(e) = ssh::send_input(app, &session_id, cmd.as_bytes()).await {
        let _ = ssh::disconnect(app, &session_id).await;
        fail(format!("发送 rz 命令失败: {}", e));
        return;
    }

    // Give rz a moment to start before we respond to its ZRINIT.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    let mut bytes_sent: u64 = 0;
    let mut terminal_noise = Vec::<u8>::new();
    let mut upload_started = false;

    let result: Result<(), String> = loop {
        let wait = if !upload_started || bytes_sent == 0 {
            std::time::Duration::from_secs(20)
        } else {
            std::time::Duration::from_secs(timeout.max(30))
        };
        let next = match tokio::time::timeout(wait, rx.recv()).await {
            Ok(v) => v,
            Err(_) => {
                break Err(format!(
                    "等待 rz 响应超时（{}秒）。远端可能未安装 lrzsz，或目录不可写。\n终端输出：\n{}",
                    wait.as_secs(),
                    String::from_utf8_lossy(&terminal_noise)
                ));
            }
        };

        match next {
            None => break Err("SSH 会话意外关闭".to_string()),
            Some(ev) => match ev {
                // ZRINIT received: hand the local file to the reader task's
                // NATIVE pump. It streams file data with 512 KB batched
                // `channel.data()` calls INLINE in the reader loop — zero
                // cross-task round-trips per packet, the same fast path the GUI
                // uses. This replaces the old per-subpacket feed/poll loop that
                // pushed every 128 KB chunk through the command queue (global
                // Mutex lock + 128 KB Vec clone + mpsc round-trip per packet),
                // which capped throughput at roughly 1/10 of the native path.
                ZmodemEvent::Start { direction }
                    if direction == "upload" && !upload_started =>
                {
                    upload_started = true;
                    mark(TaskPhase::Transferring, 0);
                    if let Err(e) =
                        ssh::zmodem_start_upload(app, &session_id, vec![local_path.to_string()]).await
                    {
                        break Err(format!("启动 native 上传失败: {}", e));
                    }
                }
                ZmodemEvent::Progress { transferred, .. } => {
                    bytes_sent = transferred;
                    mark(TaskPhase::Transferring, bytes_sent);
                }
                ZmodemEvent::FileComplete { bytes, .. } => {
                    bytes_sent = bytes;
                    mark(TaskPhase::Transferring, bytes_sent);
                }
                ZmodemEvent::End => break Ok(()),
                ZmodemEvent::Error(m) => break Err(m),
                ZmodemEvent::Closed => {
                    break Err(format!(
                        "SSH 会话在传输完成前关闭。\n终端输出：\n{}",
                        String::from_utf8_lossy(&terminal_noise)
                    ));
                }
                ZmodemEvent::Terminal(bytes) => {
                    terminal_noise.extend_from_slice(&bytes);
                    if terminal_noise.len() > 8 * 1024 {
                        terminal_noise.drain(..terminal_noise.len() - 8 * 1024);
                    }
                }
                // Native pump owns the stream now; ignore passthrough noise
                // (legacy zmodem_raw ZRINIT bytes) and stray duplicates.
                ZmodemEvent::Raw(_)
                | ZmodemEvent::Offer { .. }
                | ZmodemEvent::Start { .. } => {}
            },
        }
    };

    if result.is_err() {
        let _ = ssh::zmodem_abort(app, &session_id).await;
    }
    let _ = ssh::zmodem_finish(app, &session_id).await;
    let _ = ssh::disconnect(app, &session_id).await;

    match result {
        Ok(()) => {
            let remote_path = if remote_dir.ends_with('/') {
                format!("{}{}", remote_dir, basename)
            } else {
                format!("{}/{}", remote_dir, basename)
            };
            let _ = tasks.lock().map(|mut t| {
                if let Some(task) = t.get_mut(task_id) {
                    task.phase = TaskPhase::Done;
                    task.bytes_done = size;
                    task.result = Some(json!({ "remote_path": remote_path, "bytes": size }));
                }
            });
        }
        Err(e) => fail(e),
    }
}

/// Cheap UTC-ish timestamp string for fallback filenames (no chrono dep).
fn chrono_like_ts() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}

// ============ ssh_run / ssh_status (async exec) ============
//
// Long-running commands (apt upgrade, git clone, docker pull, etc.) can't fit
// inside `ssh_exec`'s timeout — even max 1h can be hit, and a 30-minute wait
// inside a single tool call is bad UX for the AI. ssh_run launches the
// command in a background task and returns a task_id immediately; the AI
// polls ssh_status to follow progress.
//
// Reuses the same McpState.tasks table and TransferTask struct as zmodem_*,
// just with direction = "exec" and a different status renderer. Output is
// stored in the task's `result` field (json {exit_code, stdout, stderr})
// after completion; while running, the latest 4MB per stream is in
// `result_in_progress` (a custom field on TransferTask — see below).

/// One hour. Tasks older than this (since completion) are evicted on the
/// next ssh_status call or any new ssh_run, to keep the map from growing
/// unbounded.
const EXEC_TASK_TTL_SECS: u64 = 3600;

/// Background driver for an ssh_run task. Owns the SSH handle, runs the
/// command to completion, and updates the task entry in place as it
/// progresses. Spawned by the `ssh_run` tool call; never called directly.
async fn run_ssh_exec_task(
    app: &AppState,
    tasks: &Arc<Mutex<HashMap<String, TransferTask>>>,
    controls: &Arc<Mutex<HashMap<String, ExecControl>>>,
    task_id: &str,
    conn_name: &str,
    command: &str,
    needs_confirm: bool,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
) {
    // Helper closures: mark() and fail() mutate the shared task entry.
    // fail() mirrors the zmodem_*-style closure signature (just `String`) so
    // it slots directly into resolve_config_for_task's `&impl Fn(String)`
    // parameter — the partial-output tracking lives in the structured
    // result field of the task entry, updated by the streaming loop.
    let fail = |msg: String| {
        log(&format!("ssh_run [{}] failed: {}", task_id, msg));
        let mut tasks = tasks.lock().expect("tasks mutex");
        if let Some(t) = tasks.get_mut(task_id) {
            t.phase = TaskPhase::Failed;
            t.error = Some(msg);
        }
        // Always release the cancel control plane so a subsequent
        // ssh_cancel on this task_id (or a stale one) doesn't try to
        // touch a dropped watch::Sender.
        let mut controls = controls.lock().expect("controls mutex");
        controls.remove(task_id);
    };

    // Phase 1: confirmation (if needed). Skipped for safe commands.
    if needs_confirm {
        let detail = format!("ssh_run 在服务器 [{}] 上执行: {}", conn_name, command);
        if !confirm_dangerous_operation("ssh_run（后台执行）", &detail) {
            fail(denied_by_user_text("ssh_run", &detail));
            return;
        }
    } else {
        log(&format!("ssh_run [{}] 免确认: {}", task_id, command));
    }

    // Phase 2: resolve credentials. resolve_config_for_task handles GUI
    // launch + vault wait + decryption, and reports failures via fail().
    let config = match resolve_config_for_task(app, conn_name, Some("ssh"), &fail).await {
        Some(c) => c,
        None => return, // fail() already recorded
    };

    // Phase 3: dial + exec.
    {
        let mut tasks = tasks.lock().expect("tasks mutex");
        if let Some(t) = tasks.get_mut(task_id) {
            t.phase = TaskPhase::Connecting;
        }
    }

    let handle = match ssh::dial_and_authenticate(app, &config, false).await {
        Ok(h) => h,
        Err(e) => {
            fail(format!("SSH 连接失败: {}", e));
            return;
        }
    };

    let mut channel = match handle.channel_open_session().await {
        Ok(c) => c,
        Err(e) => {
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "fail", "en").await;
            fail(format!("打开通道失败: {}", e));
            return;
        }
    };
    if let Err(e) = channel.exec(true, command).await {
        let _ = handle.disconnect(russh::Disconnect::ByApplication, "fail", "en").await;
        fail(format!("exec 失败: {}", e));
        return;
    }

    // Phase 4: collect. No timeout here — the WHOLE POINT of ssh_run is to
    // allow commands that take longer than ssh_exec's max 1h. Cancellation
    // is via `ssh_cancel` (flips cancel_rx below); a runaway remote process
    // not responding to cancel falls back to closing the SSH channel which
    // makes channel.wait() return Eof/Close.
    {
        let mut tasks = tasks.lock().expect("tasks mutex");
        if let Some(t) = tasks.get_mut(task_id) {
            t.phase = TaskPhase::Transferring;
        }
    }

    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();
    let mut exit_code: Option<u32> = None;
    // Per-stream cap is configurable via MCP_SSH_MAX_OUTPUT_BYTES — see
    // the matching note in the ssh_exec collect block.
    let max = ssh_max_output_bytes();

    // Track whether the loop exited via the cancel arm of select!. We can't
    // use the exit_code from the channel cleanly after a cancel (the
    // remote process may or may not have flushed its exit status before we
    // yanked the channel), so we mark the task accordingly.
    let mut cancelled = false;

    use russh::ChannelMsg;
    loop {
        tokio::select! {
            // Bias toward the channel: if a channel frame AND a cancel are
            // both ready at the same time, the cancel wins (we'd rather
            // stop than process one more batch of bytes).
            biased;
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() {
                    cancelled = true;
                    break;
                }
            }
            msg = channel.wait() => match msg {
                Some(ChannelMsg::Data { ref data }) => {
                    if stdout.len() < max {
                        let room = max - stdout.len();
                        stdout.extend_from_slice(&data[..data.len().min(room)]);
                    }
                }
                Some(ChannelMsg::ExtendedData { ref data, ext: 1 }) => {
                    if stderr.len() < max {
                        let room = max - stderr.len();
                        stderr.extend_from_slice(&data[..data.len().min(room)]);
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status),
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            }
        }
    }

    let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

    // Phase 5: finalize. Store final exit_code + accumulated output in
    // result so the AI sees it on the last poll. error stays None for
    // non-zero exit (that's not a task failure — the command ran to
    // completion; just returned non-zero). Cancelled tasks DO get an error
    // because the work didn't complete.
    {
        let mut tasks = tasks.lock().expect("tasks mutex");
        if let Some(t) = tasks.get_mut(task_id) {
            if cancelled {
                t.phase = TaskPhase::Failed;
                t.error = Some(format!(
                    "用户通过 ssh_cancel 取消（已收到 {} 字节 stdout / {} 字节 stderr）",
                    stdout.len(),
                    stderr.len()
                ));
                t.result = Some(json!({
                    "cancelled": true,
                    "stdout": String::from_utf8_lossy(&stdout).into_owned(),
                    "stderr": String::from_utf8_lossy(&stderr).into_owned(),
                }));
            } else {
                let code = exit_code.unwrap_or(0);
                t.phase = TaskPhase::Done;
                t.result = Some(json!({
                    "exit_code": code,
                    "stdout": String::from_utf8_lossy(&stdout).into_owned(),
                    "stderr": String::from_utf8_lossy(&stderr).into_owned(),
                }));
            }
        }
    }
    // Release the cancel control entry — last thing the bg task does, so
    // even a panic in earlier code paths would (eventually) leave a stale
    // entry that only gets cleared by the next ssh_run on the same id
    // (effectively never — but harmless). Putting it here under the
    // non-cancelled branch means a cancelled task still cleans up.
    {
        let mut controls = controls.lock().expect("controls mutex");
        controls.remove(task_id);
    }
    log(&format!(
        "ssh_run [{}] 完成: cancelled={} exit={:?} stdout={}B stderr={}B",
        task_id,
        cancelled,
        exit_code,
        stdout.len(),
        stderr.len()
    ));
}

/// Render a TransferTask (direction="exec") as the JSON the AI sees on
/// `ssh_status` polls. Skips the "X% / N bytes" wording that zmodem uses —
/// exec has no byte stream. The real payload (stdout/stderr/exit_code) is
/// in the `result` field; we also surface it as top-level keys for the
/// common case where the AI just wants the strings.
fn exec_status_to_json(task_id: &str, task: &TransferTask) -> Value {
    let phase_str = match task.phase {
        TaskPhase::Confirming => "等待用户确认（请点击弹出的对话框）",
        TaskPhase::Connecting => "连接中（SSH 握手）",
        TaskPhase::Transferring => "运行中（命令在远端执行）",
        TaskPhase::Done => "已完成",
        TaskPhase::Failed => "失败",
    };
    let elapsed = task.started_at.elapsed().as_secs();
    let text = format!(
        "ssh_run 任务 [{}]：{}\n状态：{}（已运行 {} 秒）",
        task_id, task.description, phase_str, elapsed
    );
    let (status, error) = match &task.phase {
        TaskPhase::Done => ("done", None),
        TaskPhase::Failed => ("failed", task.error.clone()),
        _ => ("running", None),
    };

    // Pull result fields to top level for easy AI consumption. `result`
    // shape is { stdout, stderr } while running and { exit_code, stdout,
    // stderr } when done — we surface the fields we know about and stash
    // the whole object in result for completeness.
    let result_obj = task.result.clone().unwrap_or(json!({}));
    let stdout = result_obj.get("stdout").cloned().unwrap_or(json!(""));
    let stderr = result_obj.get("stderr").cloned().unwrap_or(json!(""));
    let exit_code = result_obj.get("exit_code").cloned();

    let mut payload = json!({
        "content": [{ "type": "text", "text": text }],
        "task_id": task_id,
        "status": status,
        "elapsed_secs": elapsed,
        "stdout": stdout,
        "stderr": stderr,
        "result": result_obj,
    });
    if let Some(ec) = exit_code {
        payload["exit_code"] = ec;
    }
    if let Some(err) = error {
        payload["status"] = json!("failed");
        let prev_text = payload["content"][0]["text"].as_str().unwrap_or("").to_string();
        payload["content"][0]["text"] = json!(format!("{}\n错误：{}", prev_text, err));
        payload["error"] = json!(err);
    }
    payload
}

/// Periodic eviction of completed exec/zmodem tasks older than TTL.
/// Called lazily on each `ssh_status`/`zmodem_status` call so we don't
/// need a background timer task.
fn evict_stale_tasks(tasks: &Mutex<HashMap<String, TransferTask>>) {
    let mut tasks = match tasks.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let now = std::time::Instant::now();
    let before = tasks.len();
    tasks.retain(|_, t| {
        // Only evict done/failed; running tasks are never stale.
        if matches!(t.phase, TaskPhase::Done | TaskPhase::Failed) {
            now.duration_since(t.started_at).as_secs() < EXEC_TASK_TTL_SECS
        } else {
            true
        }
    });
    let after = tasks.len();
    if before != after {
        log(&format!("evict_stale_tasks: removed {} stale tasks ({} → {})", before - after, before, after));
    }
}

// ============ SFTP helpers (same as CLI) ============

async fn open_sftp(
    handle: &russh::client::Handle<ssh::SshClient>,
) -> Result<russh_sftp::client::SftpSession, String> {
    let channel = handle.channel_open_session().await.map_err(|e| format!("SFTP 通道失败: {}", e))?;
    channel.request_subsystem(true, "sftp").await.map_err(|e| format!("SFTP 子系统失败: {}", e))?;
    russh_sftp::client::SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("SFTP 会话失败: {}", e))
}

async fn sftp_list_dir(sftp: &russh_sftp::client::SftpSession, path: &str) -> Result<Vec<FileEntry>, String> {
    let resolved = if path == "~" || path.starts_with("~/") {
        let home = sftp.canonicalize(".").await.map_err(|e| format!("解析主目录失败: {}", e))?;
        let trimmed = home.trim_end_matches('/');
        match path.strip_prefix('~').unwrap_or("").trim_start_matches('/') {
            "" => trimmed.to_string(),
            suffix => format!("{}/{}", trimmed, suffix),
        }
    } else {
        path.to_string()
    };

    let entries = sftp.read_dir(&resolved).await.map_err(|e| format!("读取目录失败: {}", e))?;
    let mut files = Vec::new();
    for entry in entries {
        let name = entry.file_name().to_string();
        if name == "." || name == ".." { continue; }
        let file_type = entry.file_type();
        let full_path = if resolved.ends_with('/') { format!("{}{}", resolved, name) } else { format!("{}/{}", resolved, name) };
        files.push(FileEntry {
            name,
            path: full_path,
            is_dir: file_type.is_dir(),
            size: entry.metadata().size.unwrap_or(0),
            permissions: format!("{}", entry.metadata().permissions()),
            modified: entry.metadata().mtime.map(|t| t.to_string()).unwrap_or_default(),
        });
    }
    files.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(files)
}

async fn sftp_download_file(sftp: &russh_sftp::client::SftpSession, remote: &str, local: &str) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut rf = sftp.open(remote).await.map_err(|e| format!("打开远程文件失败: {}", e))?;
    let mut lf = tokio::fs::File::create(local).await.map_err(|e| format!("创建本地文件失败: {}", e))?;
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        let n = rf.read(&mut buf).await.map_err(|e| format!("读取失败: {}", e))?;
        if n == 0 { break; }
        lf.write_all(&buf[..n]).await.map_err(|e| format!("写入失败: {}", e))?;
    }
    Ok(())
}

async fn sftp_upload_file(sftp: &russh_sftp::client::SftpSession, local: &str, remote_dir: &str) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let filename = std::path::Path::new(local).file_name().and_then(|n| n.to_str()).ok_or("无法提取文件名")?;
    let remote_path = if remote_dir.ends_with('/') { format!("{}{}", remote_dir, filename) } else { format!("{}/{}", remote_dir, filename) };
    let mut lf = tokio::fs::File::open(local).await.map_err(|e| format!("打开本地文件失败: {}", e))?;
    let mut rf = sftp.create(&remote_path).await.map_err(|e| format!("创建远程文件失败: {}", e))?;
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        let n = lf.read(&mut buf).await.map_err(|e| format!("读取失败: {}", e))?;
        if n == 0 { break; }
        rf.write_all(&buf[..n]).await.map_err(|e| format!("写入失败: {}", e))?;
    }
    let _ = rf.flush().await;
    Ok(())
}

// ============ Logging to file (stdout is reserved for JSON-RPC only) ============

use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

static LOG_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Per-stream stdout/stderr cap for ssh_exec and ssh_run. Configurable via
/// the `MCP_SSH_MAX_OUTPUT_BYTES` env var (read once at startup); defaults
/// to 4 MiB. The atomic makes it cheap to read on every channel-data chunk
/// without a lock — writes only happen at startup (or via the env-var
/// setter below if we ever expose it).
///
/// Why configurable: deploys of big projects (`docker pull` of a 1GB image,
/// `npm install` on a huge monorepo) generate output streams well past
/// 4 MiB. The previous hardcoded cap silently truncated and the AI had no
/// way to know — the fix is to bump this via env var, redirect to a
/// server-side log file (recommended) and `sftp_download` it, or just
/// raise the cap for the one-off job.
static SSH_MAX_OUTPUT_BYTES: AtomicUsize = AtomicUsize::new(4 * 1024 * 1024);

/// Read the current per-stream output cap. Cheap (atomic load) — called on
/// every channel-data chunk in the collect loops.
fn ssh_max_output_bytes() -> usize {
    SSH_MAX_OUTPUT_BYTES.load(Ordering::Relaxed)
}

fn log_init() {
    if let Some(dir) = dirs::data_dir() {
        let log_dir = dir.join("myshell").join("logs");
        let _ = std::fs::create_dir_all(&log_dir);
        let path = log_dir.join("mcp.log");
        let _ = LOG_PATH.set(path);
    }

    // Pick up the per-stream output cap from MCP_SSH_MAX_OUTPUT_BYTES. Env
    // var is read once at startup so the read path stays a cheap atomic
    // load. Clamped to [64 KiB, 1 GiB] — below 64 KiB is useless for any
    // real command, above 1 GiB would risk OOM on a chatty job. Invalid
    // values are silently ignored (the default sticks); we log a warning
    // so the operator knows.
    if let Ok(raw) = std::env::var("MCP_SSH_MAX_OUTPUT_BYTES") {
        match raw.trim().parse::<usize>() {
            Ok(n) if n >= 64 * 1024 && n <= 1024 * 1024 * 1024 => {
                SSH_MAX_OUTPUT_BYTES.store(n, Ordering::Relaxed);
                log(&format!("MCP_SSH_MAX_OUTPUT_BYTES = {} bytes", n));
            }
            Ok(_) => log(&format!(
                "MCP_SSH_MAX_OUTPUT_BYTES={} 越界，保持默认 4 MiB（合法范围 64 KiB ~ 1 GiB）",
                raw
            )),
            Err(e) => log(&format!(
                "MCP_SSH_MAX_OUTPUT_BYTES={:?} 解析失败，保持默认 4 MiB：{}",
                raw, e
            )),
        }
    }
}

fn log(msg: &str) {
    if let Some(path) = LOG_PATH.get() {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "[{}] {}", chrono::Local::now().format("%H:%M:%S"), msg)
            });
    }
}

// ============ Main ============

#[tokio::main]
async fn main() {
    log_init();
    log("MCP server starting");

    // Initialize database
    let conn = match db::init_db() {
        Ok(c) => c,
        Err(e) => {
            log(&format!("数据库初始化失败: {}", e));
            std::process::exit(1);
        }
    };
    let _ = db::migrate_legacy_schema(&conn);

    let app = AppState {
        db: Arc::new(Mutex::new(conn)),
        ssh_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        ftp_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        local_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        zmodem_files: Arc::new(Mutex::new(std::collections::HashMap::new())),
        dek: Arc::new(Mutex::new(None)),
        transfer_cancels: Arc::new(Mutex::new(std::collections::HashMap::new())),
    };

    log("AppState initialized");

    // SECURITY: The MCP server no longer reads the vault passphrase from the
    // OS keyring or auto-unlocks the vault. Instead, all credential access is
    // delegated to the GUI: ssh_exec runs in a GUI terminal tab (the user has
    // unlocked the vault there), and SFTP tools ask the GUI to decrypt the
    // connection config via IPC (get_connection_secrets). The DEK here stays
    // None permanently — the MCP server cannot access any server on its own.

    let state = McpState {
        app,
        tasks: Arc::new(Mutex::new(std::collections::HashMap::new())),
        exec_controls: Arc::new(Mutex::new(std::collections::HashMap::new())),
    };
    log("MCP server ready — waiting for first message on stdin");

    // MCP stdio loop.
    //
    // We hold the stdin lock for the entire process lifetime (single-threaded
    // loop anyway). For stdout we wrap in BufWriter for efficiency but flush
    // explicitly after every write_message call — buffering without flush is
    // exactly what causes client-side initialize timeouts.
    let stdin = std::io::stdin();
    let stdin_lock = stdin.lock();
    let mut reader = std::io::BufReader::new(stdin_lock);
    let stdout = std::io::stdout();
    let mut writer = std::io::BufWriter::new(stdout.lock());

    while let Some(msg) = read_message_raw(&mut reader) {
        let method = msg["method"].as_str().unwrap_or("").to_string();
        let id = msg["id"].clone();
        log(&format!("recv: method={:?} id={}", method, id));

        match method.as_str() {
            "initialize" => {
                // MCP 2025-06-18 introduced `instructions` in the InitializeResult:
                // a global preamble the client prepends to the conversation so the
                // agent knows when/why to prefer these tools over its own. Older
                // clients (2024-11-05) ignore the field, so it's safe to always
                // send it. protocolVersion stays at 2024-11-05 for max compat —
                // the spec is bidirectional within the same major version.
                let response = jsonrpc_response(&id, json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "myshell-mcp",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "instructions": SERVER_INSTRUCTIONS
                }));
                write_message_raw(&mut writer, &response);
                log(&format!("sent: initialize response id={}", id));
            }

            "notifications/initialized" => {
                log("recv: notifications/initialized (no response needed)");
            }

            "tools/list" => {
                let response = jsonrpc_response(&id, tool_definitions());
                write_message_raw(&mut writer, &response);
                log(&format!("sent: tools/list response id={}", id));
            }

            "tools/call" => {
                let tool_name = msg["params"]["name"].as_str().unwrap_or("").to_string();
                let args = msg["params"]["arguments"].clone();
                log(&format!("tool call: {} args={}", tool_name, args));

                match call_tool(&state, &tool_name, &args).await {
                    Ok(result) => {
                        let response = jsonrpc_response(&id, result);
                        write_message_raw(&mut writer, &response);
                    }
                    Err(e) => {
                        let response = jsonrpc_response(&id, json!({
                            "content": [{ "type": "text", "text": format!("错误: {}", e) }],
                            "isError": true
                        }));
                        write_message_raw(&mut writer, &response);
                    }
                }
            }

            "ping" => {
                let response = jsonrpc_response(&id, json!({}));
                write_message_raw(&mut writer, &response);
                log(&format!("sent: ping response id={}", id));
            }

            _ => {
                // Unknown method — respond with MethodNotFound for requests (has id),
                // ignore unknown notifications (no id).
                if !id.is_null() {
                    let response = jsonrpc_error(&id, -32601, &format!("Method not found: {}", method));
                    write_message_raw(&mut writer, &response);
                    log(&format!("sent: method-not-found id={} method={}", id, method));
                }
            }
        }
    }
    log("stdin loop ended — exiting");
}
