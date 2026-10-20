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
// cannot bypass this: clicking Cancel / closing the dialog returns an error.
//
// SESSION-LEVEL ALLOWANCE. The GUI confirmation dialog offers a third choice
// besides confirm/deny: "本轮会话均允许" — the user, seeing what the AI wants
// to do, decides to stop being asked for the rest of THIS session. The GUI
// reports it back as `session_allowed: true` on the exec result; we then flip
// the process-wide flag below and every subsequent confirmation point (GUI
// path, headless ssh_exec, ssh_run, and the always-confirm file tools) is
// short-circuited. The MCP server process lifetime IS one AI session, so the
// grant naturally expires with it — no persistence, no cross-session leak.
// Denials are unaffected: a deny remains a hard stop.

/// Session-wide "allow all dangerous operations" grant. Flipped once by a GUI
/// "allow this session" click; never reset (the process exits with the session).
static SESSION_ALLOWED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the user has granted session-wide confirmation bypass.
fn session_allowed() -> bool {
    SESSION_ALLOWED.load(std::sync::atomic::Ordering::SeqCst)
}

/// Grant session-wide bypass (called when the GUI reports `session_allowed`).
fn grant_session_allowed() {
    if !session_allowed() {
        SESSION_ALLOWED.store(true, std::sync::atomic::Ordering::SeqCst);
        log("已授权：本轮会话内所有高危操作不再确认（用户在确认框中选择了「本轮会话均允许」）");
    }
}

/// Revoke session-wide bypass. The GUI is the authoritative party: when the
/// user clicks the topbar chip to revoke, the next exec request carries
/// `session_revoked: true` and we clear our flag so the following dangerous
/// operation confirms again as usual.
fn revoke_session_allowed() {
    if session_allowed() {
        SESSION_ALLOWED.store(false, std::sync::atomic::Ordering::SeqCst);
        log("已撤销会话放行：后续高危操作恢复确认弹窗");
    }
}

/// Central confirmation gate: returns true when the operation may proceed.
///
/// - Session grant active → immediately true (no dialog, no question).
/// - Otherwise pops the GUI/OS confirmation and returns the user's answer.
///
/// EVERY dangerous-operation confirmation goes through this function so the
/// session grant cannot be bypassed by omission at any call site.
fn confirm_or_session_allowed(tool: &str, detail: &str) -> bool {
    if session_allowed() {
        log(&format!("会话已授权，跳过确认: {}", tool));
        return true;
    }
    // Prefer the GUI-rendered dialog: the native MB_SYSTEMMODAL box raised
    // from THIS process is invisible to a user working inside MyShell — it
    // steals focus from behind their window, and for irreversible file
    // operations (overwrite / delete / rename) a prompt nobody saw is a
    // confirmation nobody gave. Falls back to the native box only when the
    // GUI cannot render it at all (not installed / wedged), which is the one
    // case where an invisible modal is still the best available option.
    match confirm_via_gui(tool, detail) {
        Some(allowed) => allowed,
        None => {
            log(&format!(
                "[confirm] GUI 不可用，回退原生弹窗: {}",
                tool
            ));
            confirm_dangerous_operation(tool, detail)
        }
    }
}

/// Ask the user to confirm `tool`/`detail` inside the GUI. Returns `None` when
/// the GUI cannot render the prompt (no IPC endpoint, no reply) — the caller
/// then decides on the headless fallback. A user who does not answer inside
/// the GUI's own 180s budget counts as **deny**: for irreversible operations,
/// "the user did not say yes" must read as "no".
fn confirm_via_gui(tool: &str, detail: &str) -> Option<bool> {
    use std::io::{BufRead, BufReader, Write};
    let ep = ensure_gui_running().ok()?;
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", ep.port)).ok()?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(200))).ok()?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(5))).ok()?;
    // The connection name is part of `detail` already; the GUI uses this
    // field for the dialog title, so pass the tool label when we have no
    // separate connection name (these call sites only carry a detail string).
    let cmd = ipc_cmd(
        &ep,
        serde_json::json!({
            "action": "mcp_confirm",
            "operation": tool,
            "connection_name": "MyShell",
            "detail": detail,
        }),
    );
    writeln!(stream, "{}", cmd).ok()?;
    stream.flush().ok()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v["ok"].as_bool() != Some(true) {
        return None;
    }
    match v["decision"].as_str() {
        Some("allow") | Some("session") => Some(true),
        // Explicit deny, and the GUI's own timeout, both mean "no".
        _ => Some(false),
    }
}

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
    log_err("高危操作确认需要 Windows 环境，已拒绝");
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

/// Danger notes for `command` (from the built-in knowledge base in
/// command_rules), formatted as a "检测到的危险点" block appended to the
/// confirmation detail. Makes the MessageBoxW specific — the user sees WHICH
/// rule matched and WHAT it destroys, not a bare "高危". Empty string when
/// nothing matched (confirmation is still driven by command_needs_confirmation).
fn danger_reasons_text(command: &str) -> String {
    let rules = load_command_rules();
    let reasons = command_rules::command_danger_reasons(command, &rules);
    if reasons.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = reasons
        .iter()
        .enumerate()
        .map(|(i, r)| format!("  {}. {}", i + 1, r))
        .collect();
    format!("\n\n⚠ 检测到的危险点:\n{}", lines.join("\n"))
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
                log_err(&format!("read_line error: {}", e));
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
                log_err("read_exact failed for body");
                return None;
            }
            return match serde_json::from_slice(&body) {
                Ok(v) => Some(v),
                Err(e) => {
                    log_err(&format!("JSON parse failed (framed): {}", e));
                    None
                }
            };
        }
        // NDJSON: one JSON object per line.
        return match serde_json::from_str(trimmed) {
            Ok(v) => Some(v),
            Err(e) => {
                log_err(&format!("JSON parse failed (ndjson): {} — line: {}", e, trimmed.chars().take(120).collect::<String>()));
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
        log_err("write body failed");
        std::process::exit(0);
    }
    if stdout.flush().is_err() {
        log_err("flush failed");
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
ARGUMENT ENCODING — NORMATIVE (follow this by default, not only when something breaks):\n\
Pass shell commands as `command_b64` (standard base64 of the UTF-8 command) rather than `command`, whenever the command contains ANY of: double quote, single quote, backslash, `$`, backtick, `{`, `}`, `|`, `;`, newline. Rationale: a shell command embedded in a JSON string has two escaping layers that do not compose — the JSON string layer and the shell layer — and a single mis-escaped character silently truncates the argument. Base64 has no character that requires escaping in either layer, so this class of failure disappears at the source. `command` (plain) remains supported for trivial commands (e.g. `uptime`, `ls /var/log`) — it is not deprecated.\n\
To read a file, use `ssh_read_text` — never build `cat`/`echo`/heredoc pipelines for that.\n\
NOTE ON FAILURES: malformed-JSON errors (\"Unterminated string\", \"Invalid escape character\") are raised by the MCP CLIENT before the request reaches this server, so no server-side error can explain them. Because of that, this server cannot diagnose them — the fix belongs in the caller (switch to `command_b64`, or transport the arguments as a real JSON object instead of a stringified one).\n\
\n\
TIMEOUTS — three different deadlines exist, do not confuse them:\n\
- Per-call transport timeout (client-side, often 30-60s): a \"Request timeout ... stage: receive\" means the CLIENT gave up — the remote command may still be running. Use `ssh_run` for anything that could exceed ~30s.\n\
- SSH handshake/dial timeout (~10s): connecting to the host failed. Retry once; if it repeats, call `test_connection` (TCP + handshake + auth only, runs no command).\n\
- Command timeout (`ssh_exec` `timeout`, default 60s): the result carries `timed_out: true` and `possible_remote_still_running: true`. The remote process was NOT killed — poll its log or use `ssh_run` (then `ssh_cancel` to stop it).\n\
\n\
SAFETY: ssh_exec uses a **configurable whitelist/blacklist** to decide which commands need human confirmation. Read-only commands (ps, ls, cat, grep, df, ...) run WITHOUT a dialog. Dangerous commands (rm, kill, sudo, shutdown, write-redirects, pipe-to-shell, ...) trigger a NATIVE OS confirmation dialog the USER must click — you cannot bypass it. Before calling a dangerous command, briefly tell the user a dialog is coming. If they click Cancel, the tool returns a HARD STOP error ('⛔ 用户已拒绝高危操作'): immediately stop ALL further operations — do NOT retry the command, do NOT switch tools/paths/connections to work around it, do NOT continue to the next step of the task. Instead, output a summary of the current task (what you were doing, which steps are done, which step was rejected, what remains) and WAIT for the user to explicitly decide whether to continue. The sftp tools (sftp_upload/remove/rename) ALWAYS confirm regardless of rules, and the same hard-stop policy applies to every confirmation dialog.\n\
\n\
FILE TRANSFER (strict two-step policy):\n\
1. ALWAYS try `zmodem_download`/`zmodem_upload` FIRST for any file upload/download request (ZMODEM over the remote `sz`/`rz` programs). It works in restricted environments where the SFTP channel is unavailable.\n\
2. If the zmodem task FAILS for a TECHNICAL reason, fall back to `sftp_download`/`sftp_upload`. Common zmodem failure reasons: the remote lacks lrzsz (`sz`/`rz` not installed — the task fails after ~20s), transfer timeout, or the remote refuses the SSH channel. Tell the user: 'zmodem 失败，改用 SFTP 方式传输'.\n\
3. To READ a file's content into the conversation, use `ssh_read_text` (it handles quoting and encoding internally) — never build a `cat`/`echo`/base64 workaround by hand. To TRANSFER a file (upload/download to local disk), use the two-step transfer policy above; NEVER — it is fragile, slow, and breaks on binary/large files. If both zmodem and sftp fail, tell the user and stop; do NOT invent a workaround.\n\
VAULT GATE (applies to EVERY tool except ssh_status/ssh_cancel/zmodem_status): MyShell stores all connection credentials in an encrypted vault. If the MyShell GUI isn't running, the tool AUTO-LAUNCHES it. If the vault is locked, the tool brings the MyShell window to the front and then WAITS UP TO 20 SECONDS in-call for the user to type the master password — if they unlock in time, this very call proceeds automatically. Only when the wait expires does it fail with '保险库未解锁'. When you see that error: the user was too slow, tell them the window is in the foreground, wait for their confirmation, then RETRY the same tool — do NOT switch to a different approach. The zmodem → sftp fallback only applies when the failure is about ZMODEM availability (e.g. lrzsz missing on the remote), NOT about the vault and NOT about a user-denied confirmation dialog.\n\
\n\
ZMODEM IS ASYNC: `zmodem_upload`/`zmodem_download` return IMMEDIATELY with a `task_id` (status=`running`); the transfer (including GUI launch, vault-unlock wait, and the confirmation dialog) runs in the background. You MUST then poll `zmodem_status` with the `task_id` every 5-10 seconds until `status` is `done` or `failed`. Do not report success until you see `done`.\n\
\n\
EVERY tool failure carries a machine-readable `error_kind` (`permission_denied` | `user_denied` | `timeout` | `busy` | `connection_lost` | `connect_failed` | `vault_locked` | `bad_request` | `not_found` | `internal`) plus a `hint` naming the next action. Branch on `error_kind`, not on the Chinese prose.\n\
\n\
SERIALIZATION: a connection's terminal runs one command at a time. A second `ssh_exec` on the same connection while the first is still running returns `error_kind: \"busy\"` with how long it has been running and the longest remaining wait — do NOT blind-retry; wait that long, or switch to `ssh_run` (asynchronous, exempt from this lock). Different connections run fully in parallel.\n\
\n\
ENCOUNTERING ERRORS: '保险库未解锁' means the tool already parked for 20 seconds waiting for the user and they didn't unlock in time — tell them the MyShell window is in the foreground, wait for their confirmation, then RETRY the same tool. '保险库尚未初始化' means first-time setup is needed in the GUI. Do not poll vault_status as the FIRST response to a locked vault: the in-call wait already covered the short case; poll it only when a retry fails the same way.";

fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "vault_status",
                "description": "Check whether the MyShell vault is unlocked, WITHOUT executing anything and WITHOUT exposing any connection data. Returns only booleans (`gui_running`, `initialized`, `unlocked`) and a `vault_state` string.\n\nWHEN TO USE: a gated tool already parks up to 20s in-call waiting for the user to unlock — so in the common case you never need this tool. Reach for it only when a gated tool FAILED with `error_kind: vault_locked` despite that wait AND a retry fails the same way: then poll THIS tool every ~5 seconds until `vault_state` is `unlocked`, and retry THE SAME tool once it is. Do not blind-retry the gated tool in a tight loop (each retry can re-trigger a confirmation dialog), and do not switch to a different tool (switching changes semantics).\n\nSECURITY: returns booleans only. While the vault is locked you still cannot see any connection names, hosts or credentials — that gate is unchanged.",
                "inputSchema": { "type": "object", "properties": {}, "required": [] }
            },
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
                        "command": { "type": "string", "description": "Shell command to run. Executed via `bash -c` semantics on the remote. Avoid commands that read from stdin or wait for input (sudo without -n will hang until the timeout; use `sudo -n` to fail fast instead)." },
                        "command_b64": { "type": "string", "description": "Base64-encoded shell command. PREFERRED when the command contains many quotes, backslashes, `$`, backticks or regex braces — base64 has no character that needs JSON escaping, so this eliminates the entire class of malformed-JSON failures. Provide EITHER `command` or `command_b64` (if both, `command_b64` wins)." },
                        "timeout": { "type": "integer", "description": "Max seconds to wait before killing the command (default 60, max 3600=1h). For tasks longer than ~5min, prefer `ssh_run` + `ssh_status` polling instead of raising this. On timeout the result carries `timed_out: true` and `possible_remote_still_running: true` — the command is NOT killed remotely.", "default": 60, "minimum": 1, "maximum": 3600 }
                    },
                    "required": ["connection"],
                    "anyOf": [{ "required": ["command"] }, { "required": ["command_b64"] }]
                }
            },
            {
                "name": "ssh_run",
                "description": "Run a shell command on a remote SSH server asynchronously (returns a task_id immediately, then poll `ssh_status` to follow progress). Designed for long-running tasks where blocking the conversation for minutes is impractical.\n\nWHEN TO USE:\n- Commands expected to take >1min: `apt upgrade`, `apt install`, `docker pull`, `docker compose up`, `git clone` of large repos, `npm/pnpm install`, `tar`/`rsync` of large trees, full system backups, long-running `find`/`grep`.\n- Any command where you'd otherwise need to set `ssh_exec` timeout >120s.\n\nWHEN NOT TO USE:\n- Quick status checks — use `ssh_exec` (synchronous, returns immediately on completion).\n- Interactive sessions — same as `ssh_exec`, doesn't stream.\n- Tasks you want the user to watch live — open a GUI terminal tab instead via `open_in_gui`.\n\nWORKFLOW:\n1. Call `ssh_run` → returns `{ task_id, status: \"started\" }` immediately (well under 1s, the actual command kicks off in background).\n2. Poll `ssh_status` with the same `task_id` every 10-30s. Each call returns the current phase + accumulated stdout/stderr so far (last 4MB each; full output is preserved on the server — `sftp_download` the log file if you need more).\n3. When `status` becomes `done` or `failed`, the task is finished and the final `exit_code` + full output is in the response.\n4. Tasks are kept in memory for 1h after completion, then auto-cleaned. Stale `task_id`s (older than 1h) return an error.\n\n⚠️ HUMAN CONFIRMATION REQUIRED (same as `ssh_exec`): A native OS dialog pops up before the command starts. The user MUST click 'Yes' for the command to run.\n\nOUTPUT on start: `{ task_id, status: \"started\", description: \"<conn>: <cmd prefix>\" }`. On poll: see `ssh_status` description.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "command": { "type": "string", "description": "Shell command to run. Executed via `bash -c` semantics on the remote. Avoid commands that read from stdin or wait for input. For long tasks, consider redirecting output to a log file (`> /tmp/run.log 2>&1`) and then `sftp_download` it after — keeps the live output small." },
                        "command_b64": { "type": "string", "description": "Base64-encoded shell command (same semantics as on `ssh_exec`). Use when the command has heavy quoting/escaping; mutually exclusive with `command`." }
                    },
                    "required": ["connection"],
                    "anyOf": [{ "required": ["command"] }, { "required": ["command_b64"] }]
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
                "name": "ssh_read_text",
                "description": "Read a remote text file's content, returned directly as a string — NO shell quoting, NO base64, NO SFTP round-trip to your side. Prefer this over `ssh_exec 'cat file'` for inspecting configs and logs.\n\nWHEN TO USE: You need to SEE a remote file's content — config files (sshd_config, nginx.conf, my.cnf, application.yaml), log excerpts, small scripts. This is the escape-free path: the path is passed as a JSON string parameter and handled entirely inside MyShell, so no shell metacharacter in the path or the content can break the call.\n\nWHEN NOT TO USE:\n- Binary files — this tool decodes UTF-8 and replaces invalid bytes; use `zmodem_download` / `sftp_download` instead.\n- Very large files (>1 MB by default) — set `max_bytes`, or download the file and read it locally.\n- Searching across many files (`grep -r`) — `ssh_exec` is the right tool there.\n\nIMPLEMENTATION NOTE: reads via the SFTP subsystem when available; if the server has no SFTP, it transparently falls back to a shell read with correct quoting. You do not need to care which path was used — the result reports it in `method`.\n\nOUTPUT: JSON with `content` (string), `bytes` (total file size), `truncated` (true if only the head was returned), `lines` (number of lines returned), `method` (`sftp` or `shell`).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "remote_path": { "type": "string", "description": "Absolute path to the remote text file (e.g. '/etc/ssh/sshd_config'). '~' is expanded." },
                        "max_bytes": { "type": "integer", "description": "Maximum bytes to return (default 1048576 = 1 MiB, max 8 MiB). Larger files are truncated from the end; `truncated: true` and `bytes` tell you the real size.", "default": 1048576 }
                    },
                    "required": ["connection", "remote_path"]
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
                "description": "Download a single file from a remote SSH/SFTP server to the local filesystem.\n\nWHEN TO USE: User wants to fetch a remote file — e.g. 'download /etc/nginx/nginx.conf from web1', 'grab yesterday's log from /var/log/myapp.log'. Read-only on the server; writes to local disk.\n\nWHEN NOT TO USE: For reading a file's content into the conversation, you usually want to download it first then read it locally. Don't try to 'stream' large files — download to disk.\n\nFALLBACK ORDER: `zmodem_download` is tried FIRST; use this tool when the ZMODEM task fails (e.g. lrzsz missing on the remote) or the user explicitly asked for SFTP.\n\nOUTPUT: Confirmation message on success. The local file is overwritten if it exists.",
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
                "description": "Upload a single local file to a remote SSH/SFTP server.\n\nWHEN TO USE: User wants to push a file — e.g. 'deploy this config to /etc/nginx/nginx.conf', 'copy the new build to /var/www/html'. If `remote_path` is an existing file, it WILL be overwritten (no backup).\n\nFALLBACK ORDER: `zmodem_upload` is tried FIRST; use this tool when the ZMODEM task fails (e.g. lrzsz missing on the remote) or the user explicitly asked for SFTP.\n\n⚠️ HUMAN CONFIRMATION REQUIRED: A native OS dialog pops up warning the user about the upload (potential overwrite). Won't proceed until the user clicks 'Yes'.\n\nOUTPUT: Confirmation message. The remote filename is taken from the local file's basename; only the remote *directory* path is used from `remote_path`.",
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
                "description": "Download a file from a remote SSH server using the ZMODEM protocol (remote `sz` → local). This is the PREFERRED transfer method — try it first for file downloads.\n\nWHEN TO USE (strict two-step policy):\n1. First call THIS tool for the download. ZMODEM works in restricted environments where SFTP is unavailable (connection saved as SSH-only with no SFTP, SFTP subsystem disabled, restricted/chroot shell, jump host, embedded device, etc.).\n2. If the ZMODEM task fails for a TECHNICAL reason (most commonly `sz` missing on the remote — the task fails after ~20s with a hint to install lrzsz, or transfer timeout), fall back to `sftp_download`. Tell the user: 'zmodem 失败，改用 SFTP 方式传输'. Do NOT fall back after a user-denied confirmation dialog — that is a hard stop.\n3. If the user explicitly asked for SFTP, use `sftp_download` directly.\n\nNEVER use `ssh_exec` + base64/cat/heredoc to transfer file content — it breaks on binary/large files. If both zmodem and sftp fail, tell the user and stop.\n\nPREREQUISITE: The remote server must have `sz` installed (part of the `lrzsz` package). If it's missing, the task fails after ~20s with a hint to install lrzsz.\n\nASYNC: returns IMMEDIATELY with {task_id, status:\"running\"}; the transfer runs in the background. The GUI auto-launches if it isn't running; a locked vault fails the tool call INSTANTLY with '保险库未解锁' (the MyShell window is brought to the front for the user) — tell the user to unlock, then retry. No human confirmation dialog (read-only on the remote).\n\nFINAL RESULT: when `zmodem_status` returns status=\"done\", its `result` field has `{files:[{name, local_path, bytes}]}`.\n\nNOTE: the `timeout` param (default 120s) caps the background transfer, not the tool-call window (which is <1s).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "remote_path": { "type": "string", "description": "Absolute remote path of ONE FILE to download (passed verbatim to `sz -r`).\n\n⚠️ NOT a directory. `sz` cannot open a directory — lrzsz answers 'Can't open any requested files' and exits 128, and the path is shell-quoted so no glob is expanded either. To move a whole tree, use `download_project` (tar over SFTP) instead.\n\nThis param's description previously claimed directories were recursed and their subtree mirrored. That was never true: nothing in the download path walks a tree. (`zmodem_upload` DOES recurse, because its uploader expands folders into relative ZFILE offers — the two directions are deliberately asymmetric.)" },
                        "local_dir": { "type": "string", "description": "Absolute local directory where the file lands. Must exist. The local name comes from the remote offer." },
                        "timeout": { "type": "integer", "description": "Max seconds to wait for the transfer (default 120). The first 20s is an initial handshake timeout before the transfer is considered stalled.", "default": 120 }
                    },
                    "required": ["connection", "remote_path", "local_dir"]
                }
            },
            {
                "name": "zmodem_upload",
                "description": "Upload a local file to a remote SSH server using the ZMODEM protocol (local → remote `rz`). This is the PREFERRED transfer method — try it first for file uploads.\n\nWHEN TO USE (strict two-step policy):\n1. First call THIS tool for the upload. ZMODEM works in restricted environments where SFTP is unavailable (connection saved as SSH-only with no SFTP, SFTP subsystem disabled, restricted/chroot shell, jump host, embedded device, etc.).\n2. If the ZMODEM task fails for a TECHNICAL reason (most commonly `rz` missing on the remote — the task fails after ~20s, or transfer timeout), fall back to `sftp_upload`. Tell the user: 'zmodem 失败，改用 SFTP 方式传输'. Do NOT fall back after a user-denied confirmation dialog — that is a hard stop.\n3. If the user explicitly asked for SFTP, use `sftp_upload` directly.\n\nNEVER use `ssh_exec` + base64/cat/heredoc/echo to transfer file content — it breaks on binary/large files. If both zmodem and sftp fail, tell the user and stop.\n\nPREREQUISITE: The remote server must have `rz` installed (part of the `lrzsz` package). If missing, the task fails after ~20s.\n\nASYNC: returns IMMEDIATELY with {task_id, status:\"running\"}; the transfer runs in the background. The GUI auto-launches if it isn't running; a locked vault fails the tool call INSTANTLY with '保险库未解锁' (the MyShell window is brought to the front for the user) — tell the user to unlock, then retry. The OS confirmation dialog pops AFTER the tool returns — remind the user to click it if the first `zmodem_status` poll shows phase=`Confirming`. You MUST then poll `zmodem_status(task_id)` every 5-10s until status is `done` or `failed`.\n\n⚠️ HUMAN CONFIRMATION REQUIRED: A native OS dialog pops up (writing to the remote server). Won't proceed until the user clicks 'Yes'. This happens AFTER the tool returns, in the background.\n\nFINAL RESULT: when `zmodem_status` returns status=\"done\", its `result` field has `{remote_path, bytes}`.\n\nNOTE: the `timeout` param (default 120s) caps the background transfer, not the tool-call window (which is <1s).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "local_path": { "type": "string", "description": "Absolute local path to upload. Must exist. May be a file OR a directory — directories are expanded recursively and their tree mirrored under remote_dir (ZFILE offers carry relative paths; lrzsz rz creates the subdirectories)." },
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

/// Running byte accounting for one ZMODEM transfer.
///
/// `sz -r` on a directory emits one `Offer` per file, and each file's
/// `Progress` / `FileComplete` events only speak for *that* file. Reporting
/// an event's own numbers straight into `TransferTask` therefore resets the
/// bar to 0 at every new offer, and the last event to arrive decides what
/// `bytes_total` ends up as. That is not hypothetical: the per-file-complete
/// branch used to pass a literal `0` as the total, so a **successful** 2 MiB
/// download reported "0% / 2097152 bytes" forever.
///
/// This type keeps the two accumulators and is deliberately pure so the
/// behaviour can be unit-tested without a live SSH session.
#[derive(Default, Debug, PartialEq)]
struct TransferProgress {
    /// Sum of every offer's declared size (the whole transfer's total).
    grand_total: u64,
    /// Completed files + the current file's progress.
    done_bytes: u64,
}

impl TransferProgress {
    /// A new file was offered at `size` bytes.
    fn offer(&mut self, size: u64) -> (u64, u64) {
        self.grand_total = self.grand_total.saturating_add(size);
        (self.done_bytes, self.grand_total)
    }

    /// `transferred` bytes of the current file are done; `total` is that
    /// file's own size. It may only *widen* the transfer total — replacing
    /// it would drop every previously-seen file back out of the denominator.
    fn progress(&mut self, transferred: u64, total: u64) -> (u64, u64) {
        if total > self.grand_total {
            self.grand_total = total;
        }
        (
            self.done_bytes.saturating_add(transferred),
            self.grand_total,
        )
    }

    /// The current file finished with `bytes` bytes actually written.
    fn file_complete(&mut self, bytes: u64) -> (u64, u64) {
        self.done_bytes = self.done_bytes.saturating_add(bytes);
        (self.done_bytes, self.grand_total)
    }

    /// Final (done, total). `actual` is the summed size of everything that
    /// really landed, which wins over the offer-derived total so a file that
    /// arrived short still reports truthfully.
    fn finish(&self, actual: u64) -> (u64, u64) {
        (actual, if self.grand_total > 0 { self.grand_total } else { actual })
    }
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
    // This is mostly a race guard: the vault gate at the top of call_tool
    // has already run, so the common case is an instant Ok. If the vault
    // got locked in between (auto-lock timer), we fail the task fast
    // instead of silently waiting. spawn_blocking keeps the gate's internal
    // std::thread::sleep off the async worker (same rationale as call_tool).
    let gate = tokio::task::spawn_blocking(ensure_vault_ready)
        .await
        .unwrap_or_else(|e| Err(VaultGateError::GuiUnavailable(format!("门禁探测任务失败: {e}"))));
    if let Err(e) = gate {
        fail(e.to_string());
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

/// Strip terminal escape sequences from remote output before it reaches the
/// AI. The GUI path captures a real PTY, so stdout carries the shell's prompt,
/// window-title OSC, grep's SGR colors and OSC-8 hyperlinks. Those bytes are
/// meaningless to an agent (it cannot render them), inflate the payload, and
/// used to break callers whose JSON pipeline chokes on raw ESC bytes.
///
/// Covers: CSI (`ESC [ ... final`), OSC (`ESC ] ... BEL|ESC \`), and short
/// `ESC <one byte>` sequences. Tab/newline/CR are preserved (they carry
/// structure a human reading the transcript still wants).
fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // ESC [ ... final byte in 0x40..=0x7E
            if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                i += 2;
                while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                    i += 1;
                }
                i += 1; // consume the final byte
                continue;
            }
            // ESC ] ... BEL or ESC \
            if i + 1 < bytes.len() && bytes[i + 1] == b']' {
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == 0x07 {
                        i += 1;
                        break;
                    }
                    if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            // Any other ESC: drop the escape plus the following byte.
            i += 2;
            continue;
        }
        // Walk one UTF-8 char so multibyte text (中文) survives intact.
        let ch_len = utf8_len(bytes[i]);
        let end = (i + ch_len).min(bytes.len());
        out.push_str(&input[i..end]);
        i = end;
    }
    out
}

/// Byte length of the UTF-8 char starting with `b` (1 on malformed input).
fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

/// Decode a base64-encoded command. Callers that generate JSON
/// programmatically can send `command_b64` instead of `command` — base64
/// contains none of `"` `'` `\` `$` `` ` `` so the entire class of
/// escaping failures (an unescaped quote or backslash silently truncating the
/// JSON string) disappears at the source.
fn decode_command_b64(raw: &str) -> Result<String, String> {
    use base64::Engine as _;
    let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .map_err(|e| {
            format!(
                "command_b64 不是合法 base64（原始内容无引号/反斜杠，无需转义）: {}",
                e
            )
        })?;
    String::from_utf8(bytes).map_err(|e| format!("command_b64 解码结果不是合法 UTF-8: {}", e))
}

/// Resolve the command argument, accepting either `command` or `command_b64`.
///
/// The base64 form exists because agents that build tool arguments by string
/// interpolation keep mutilating shell commands: every quote, backslash,
/// `$` or regex brace inside the JSON string is a chance to produce invalid
/// JSON, and the resulting parse error names neither the field nor the offset.
fn resolve_command_arg(args: &Value) -> Result<String, String> {
    if let Some(b64) = args["command_b64"].as_str() {
        if !b64.trim().is_empty() {
            return decode_command_b64(b64);
        }
    }
    if let Some(cmd) = args["command"].as_str() {
        return Ok(cmd.to_string());
    }
    Err("缺少 command 参数（或改用 command_b64，base64 编码，免转义）".to_string())
}

/// Machine-readable failure class, so a caller can branch without regex-ing
/// Chinese prose. Every tool error carries one of these.
mod error_kind {
    /// Remote/relay refused for lack of privilege (sudo, polkit, dmesg).
    pub const PERMISSION_DENIED: &str = "permission_denied";
    /// The human clicked Cancel / Deny on a confirmation dialog.
    pub const USER_DENIED: &str = "user_denied";
    /// Our own deadline elapsed; the remote may still be running it.
    pub const TIMEOUT: &str = "timeout";
    /// The connection's single-exec slot is occupied by an earlier command.
    pub const BUSY: &str = "busy";
    /// SSH session dropped, or reconnected but the channel was gone.
    pub const CONNECTION_LOST: &str = "connection_lost";
    /// Could not reach/authenticate the host at all.
    pub const CONNECT_FAILED: &str = "connect_failed";
    /// The vault is locked / not initialised (user action required).
    pub const VAULT_LOCKED: &str = "vault_locked";
    /// Bad arguments (missing field, malformed base64, unknown tool).
    pub const BAD_REQUEST: &str = "bad_request";
    /// Remote path/file missing.
    pub const NOT_FOUND: &str = "not_found";
    /// Anything else.
    pub const INTERNAL: &str = "internal";
}

/// Classify a free-text failure message into an `error_kind`.
///
/// The MCP protocol has no error-code slot in a tool result, and every
/// meaningful failure in this codebase flows through `Err(String)`. Rather
/// than re-plumb hundreds of `?` sites into a typed enum, we map the message
/// once, at the boundary where the result is rendered. Order matters: the
/// more specific patterns are tested first.
fn classify_error(msg: &str) -> &'static str {
    let m = msg;
    if m.contains("用户已拒绝") || m.contains("用户取消") || m.contains("用户通过 ssh_cancel") {
        return error_kind::USER_DENIED;
    }
    if m.contains("保险库未解锁") || m.contains("保险库尚未初始化") {
        return error_kind::VAULT_LOCKED;
    }
    if m.contains("Permission denied")
        || m.contains("permission denied")
        || m.contains("Authorization failed")
        || m.contains("polkit")
        || m.contains("Operation not permitted")
        || m.contains("拒绝访问")
    {
        return error_kind::PERMISSION_DENIED;
    }
    if m.contains("上一条命令仍在该服务器") || m.contains("一次只能跑一条命令") {
        return error_kind::BUSY;
    }
    if m.contains("超时") || m.contains("timeout") || m.contains("timed out") {
        return error_kind::TIMEOUT;
    }
    if m.contains("会话已断开") || m.contains("重连") || m.contains("Session command channel closed") {
        return error_kind::CONNECTION_LOST;
    }
    if m.contains("连接失败") || m.contains("连接超时") || m.contains("dial") || m.contains("握手") {
        return error_kind::CONNECT_FAILED;
    }
    if m.contains("缺少") || m.contains("不存在或已过期") || m.contains("未知工具") || m.contains("不是合法") {
        return error_kind::BAD_REQUEST;
    }
    if m.contains("No such file") || m.contains("not found") || m.contains("不存在") {
        return error_kind::NOT_FOUND;
    }
    error_kind::INTERNAL
}

/// Wrap a tool failure as an MCP tool result carrying a machine-readable
/// `error_kind` plus a `hint` telling the caller what to try next.
fn tool_error(kind_override: Option<&str>, msg: String) -> Value {
    let kind = kind_override.unwrap_or_else(|| classify_error(&msg));
    let hint = match kind {
        error_kind::PERMISSION_DENIED => {
            "远端拒绝了该操作（权限/策略原因，不是确认弹窗被取消）。可尝试 `sudo -n <命令>`（非交互式，失败会立刻返回而不是卡住），或换一个有权限的连接。"
        }
        error_kind::USER_DENIED => {
            "用户明确拒绝了该操作，属硬性停止：不要重试、不要换路径绕过，输出当前任务进展后等待用户决定。"
        }
        error_kind::TIMEOUT => {
            "命令可能仍在远端运行。用 `ssh_run` + `ssh_status` 轮询（可 `ssh_cancel` 取消），或在命令里重定向到日志并轮询该文件。"
        }
        error_kind::BUSY => {
            "该连接的终端正被上一条命令占用。等待上面提示的秒数后重试，或改用 `ssh_run`（异步，不受此锁限制）。"
        }
        error_kind::CONNECTION_LOST => {
            "会话已断开。重试一次通常会自动重连；仍失败则调 `test_connection` 确认可达性。"
        }
        error_kind::CONNECT_FAILED => {
            "建议先调 `test_connection` 确认该连接可达（它只做 TCP+握手+认证，不执行任何命令）。"
        }
        error_kind::VAULT_LOCKED => {
            "请在 MyShell 窗口中输入主密码解锁保险库，然后重试同一工具（不要改用其他工具）。"
        }
        error_kind::BAD_REQUEST => {
            "检查参数名与取值；命令若含大量引号/反斜杠，改用 `command_b64`（base64，免转义）。"
        }
        error_kind::NOT_FOUND => "确认远端路径拼写；可用 `sftp_list` 查看目录实际内容。",
        _ => "可查看 MyShell 的 MCP 错误日志（`mcp-error-<日期>.log`）获取完整上下文。",
    };
    json!({
        "content": [{ "type": "text", "text": format!("错误: {}", msg) }],
        "isError": true,
        "error_kind": kind,
        "hint": hint,
    })
}

async fn call_tool(state: &McpState, name: &str, args: &Value) -> Result<Value, String> {
    // ── Vault-unlock gate (applies to EVERY tool except task
    // status/cancel) ────────────────────────────────────────────────────
    // Every tool requires the GUI vault to be unlocked — INCLUDING
    // list_connections: without the master password the AI must not even
    // learn WHICH servers exist (connection names/groups are metadata worth
    // protecting too). The gate:
    //   - auto-launches the GUI when it isn't running;
    //   - when locked: focuses the GUI's password gate, then parks up to 20s
    //     IN-CALL waiting for the user to unlock (阶段 198) — unlock makes
    //     this very call proceed; only the expiry returns vault_locked.
    // The three exemptions (ssh_status / ssh_cancel / zmodem_status) never
    // touch credentials and must stay reachable so the AI can observe and
    // clean up tasks that failed ON this very gate.
    match name {
        // `vault_status` joins the exemptions deliberately: the AI needs a
        // way to check the vault state WITHOUT triggering the gate — gating
        // it would make the state unqueryable exactly while locked. With the
        // in-call wait it is a FALLBACK poll (not the first response): use it
        // only when a gated tool failed with vault_locked AND the retry fails
        // the same way. It returns booleans only, never credentials or
        // connection metadata.
        "ssh_status" | "ssh_cancel" | "zmodem_status" | "vault_status" => {}
        _ => {
            // spawn_blocking: the gate parks on std::thread::sleep for up to
            // 20s (vault-unlock wait) and up to ~27s more in launch paths —
            // blocking a tokio worker for that long stalls other async tasks
            // (ssh_run progress, zmodem transfers) whenever several gated
            // calls park at once.
            let gate = tokio::task::spawn_blocking(ensure_vault_ready)
                .await
                .unwrap_or_else(|e| {
                    Err(VaultGateError::GuiUnavailable(format!("门禁探测任务失败: {e}")))
                });
            if let Err(e) = gate {
                // Structured so the caller can branch on `vault_state` and knows
                // retrying the SAME tool after unlocking is the expected move,
                // instead of blind-retrying or switching tools.
                return Ok(json!({
                    "content": [{ "type": "text", "text": format!("错误: {}", e.message()) }],
                    "isError": true,
                    "error_kind": error_kind::VAULT_LOCKED,
                    "vault_state": e.state(),
                    "hint": format!(
                        "{} 解锁后请重试【同一个工具】—— 不要改用其他工具、也不要反复盲调。可先用 `vault_status` 查询状态（{}）。",
                        match e.state() {
                            "locked" => "请在 MyShell 窗口中输入主密码解锁保险库。",
                            "not_initialized" => "请在 MyShell 中设置主密码完成初始化。",
                            "gui_unavailable" => "请先手动打开 MyShell 桌面应用。",
                            _ => "请确认 MyShell 桌面应用正在运行。",
                        },
                        e.state()
                    ),
                }));
            }
        }
    }

    match name {
        "vault_status" => {
            // Read-only state probe for the unlock handshake. Exempt from the
            // gate above (it can never succeed while locked, so gating it would
            // be self-defeating), and it deliberately exposes ONLY two booleans
            // — never connection names, hosts, or anything else gated behind
            // the master password.
            let probe = ensure_gui_running();
            let (gui_up, initialized, unlocked) = match probe {
                Err(_) => (false, false, false),
                Ok(ep) => {
                    let mut st = None;
                    for _ in 0..3 {
                        if let Some(s) = query_gui_vault_status(&ep) {
                            st = Some(s);
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                    match st {
                        Some((i, u)) => (true, i, u),
                        None => (true, false, false),
                    }
                }
            };
            let state = if !gui_up {
                "gui_unavailable"
            } else if !initialized {
                "not_initialized"
            } else if !unlocked {
                "locked"
            } else {
                "unlocked"
            };
            let next = match state {
                "locked" => "请用户在 MyShell 窗口输入主密码解锁，然后重试原工具。",
                "not_initialized" => "请用户在 MyShell 中完成主密码初始化。",
                "gui_unavailable" => "请先打开 MyShell 桌面应用。",
                _ => "保险库已解锁，可直接执行原工具。",
            };
            Ok(json!({
                "content": [{ "type": "text", "text": format!("vault_status = {}；{}", state, next) }],
                "vault_state": state,
                "gui_running": gui_up,
                "initialized": initialized,
                "unlocked": unlocked,
                "hint": "门禁工具会先就地等待解锁（最长 20 秒）；只有工具已返回 vault_locked 后才需要本工具兜底轮询 —— 每 5 秒一次、约 2 分钟上限，期间不要反复重试原工具（会重复弹确认窗）。",
            }))
        }

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
            let command = resolve_command_arg(args)?;
            let command = command.as_str();
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
                        // A session grant means NO dialog is shown on either path
                        // (GUI skips it, headless short-circuits), so the
                        // double-popup concern below does not apply — treat the
                        // command as no-confirm and let the headless fallback run.
                        let needs_confirm = !session_allowed()
                            && command_rules::command_needs_confirmation(command, &rules);
                        // Whether the GUI was actually reachable comes from
                        // the error TYPE now, not from matching its text.
                        let gui_attempted = e.gui_reachable;

                        // P1-15: the command may ALREADY have run in the visible
                        // tab — the GUI writes it to the PTY first and only then
                        // reports a hard timeout / lost sentinel / IPC teardown.
                        // Falling back to headless would run the SAME command a
                        // second time on a fresh SSH session, invisibly to the
                        // user. Surface the original error instead.
                        if e.command_sent {
                            log_err(&format!(
                                "GUI 已把命令送入终端后才报错（耗时 {}ms），不再无头重跑（避免同一命令执行两次）: {}",
                                e.elapsed_ms, e.message
                            ));
                            return Err(format!(
                                "{}（命令已发送到远端终端，耗时约 {} 秒；它可能仍在运行，请先确认再决定是否重试）",
                                e.message,
                                e.elapsed_ms / 1000
                            ));
                        }

                        if gui_attempted && needs_confirm {
                            // GUI was reachable but the exec failed (user didn't
                            // confirm in time, cancelled, or a session error).
                            // Don't fall through to headless — that would pop a
                            // second confirmation dialog. Surface the error.
                            log_err(&format!(
                                "show_in_gui 失败且命令需确认（耗时 {}ms），不再回退 headless（避免重复弹窗）: {}",
                                e.elapsed_ms, e.message
                            ));
                            return Err(format!(
                                "{}（未在 {} 秒内完成；命令已发送到远端：{}）",
                                e.message,
                                e.elapsed_ms / 1000,
                                if e.command_sent { "是" } else { "否" }
                            ));
                        }
                        // Either the GUI was unreachable (legitimate fallback)
                        // or the command doesn't need confirmation (headless
                        // won't pop anything). Fall through.
                        log(&format!(
                            "show_in_gui 失败（耗时 {}ms），回退到静默模式: {}",
                            e.elapsed_ms, e.message
                        ));
                    }
                }
            }

            // ── Confirmation check (headless path) ──
            if command_rules::command_needs_confirmation(command, &rules) {
                let detail = format!(
                    "在服务器 [{}] 执行命令: {}{}",
                    conn_name,
                    command,
                    danger_reasons_text(command)
                );
                if !confirm_or_session_allowed("ssh_exec（远程命令执行）", &detail) {
                    return Ok(json!({ "content": [{ "type": "text", "text": denied_by_user_text("ssh_exec", &detail) }], "isError": true }));
                }
            } else {
                log(&format!("ssh_exec 免确认（白名单/非危险）: {}", command));
            }

            let started = std::time::Instant::now();
            let config = state.resolve_via_gui(conn_name, Some("ssh"))?;

            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;
            let mut channel = handle
                .channel_open_session()
                .await
                .map_err(|e| format!("打开通道失败: {}", e))?;
            channel.exec(true, command).await.map_err(|e| format!("exec 失败: {}", e))?;

            // Per-stream output cap — declared outside the collect block so the
            // `truncated` flag below can compare against it.
            let max_output = ssh_max_output_bytes();
            let collect = async {
                use russh::ChannelMsg;
                let mut stdout: Vec<u8> = Vec::new();
                let mut stderr: Vec<u8> = Vec::new();
                let mut exit_code: Option<u32> = None;
                let mut truncated = false;
                loop {
                    match channel.wait().await {
                        Some(ChannelMsg::Data { ref data }) => {
                            if stdout.len() < max_output {
                                let room = max_output - stdout.len();
                                stdout.extend_from_slice(&data[..data.len().min(room)]);
                                if data.len() > room {
                                    truncated = true;
                                }
                            } else {
                                truncated = true;
                            }
                        }
                        Some(ChannelMsg::ExtendedData { ref data, ext: 1 }) => {
                            if stderr.len() < max_output {
                                let room = max_output - stderr.len();
                                stderr.extend_from_slice(&data[..data.len().min(room)]);
                                if data.len() > room {
                                    truncated = true;
                                }
                            } else {
                                truncated = true;
                            }
                        }
                        Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status),
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        Some(_) => {}
                    }
                }
                (stdout, stderr, exit_code, truncated)
            };

            let (stdout, stderr, code, truncated) = match tokio::time::timeout(
                std::time::Duration::from_secs(timeout),
                collect,
            )
            .await
            {
                Ok(v) => v,
                Err(_) => {
                    // Timeout is a first-class outcome, not a bare string: the
                    // caller needs to know the remote may STILL be running this
                    // command (so it can poll a log or use ssh_run + kill)
                    // rather than blindly retrying and stacking a second copy.
                    let _ = handle.disconnect(russh::Disconnect::ByApplication, "timeout", "en").await;
                    let payload = json!({
                        "error": format!("命令超时（{}秒未完成）", timeout),
                        "error_kind": error_kind::TIMEOUT,
                        "hint": "命令可能仍在远端运行。用 `ssh_run` + `ssh_status` 轮询（可 `ssh_cancel` 取消），或在命令里重定向到日志并轮询该文件。",
                        "timed_out": true,
                        "possible_remote_still_running": true,
                        "command": command,
                        "duration_ms": started.elapsed().as_millis() as u64,
                        "timeout_secs": timeout,
                        "exit_code": serde_json::Value::Null,
                    });
                    return Ok(json!({
                        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&payload).unwrap_or_default() }],
                        "isError": true,
                        "error_kind": error_kind::TIMEOUT,
                        "timed_out": true,
                    }));
                }
            };

            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;
            let stdout = strip_ansi(&String::from_utf8_lossy(&stdout));
            let stderr = strip_ansi(&String::from_utf8_lossy(&stderr));
            let duration_ms = started.elapsed().as_millis() as u64;

            // A missing ExitStatus is NOT exit code 0. It means the channel
            // ended without one: the connection dropped, or the remote process
            // was terminated by a signal. `code.unwrap_or(0)` reported those —
            // along with whatever truncated stdout we managed to capture — as a
            // clean success, which is exactly how an AI ends up building on a
            // result that never happened.
            let Some(code) = code else {
                let result = json!({
                    "error": "远端未返回退出码：连接中断或进程被信号终止，结果未知（不能视为成功）",
                    "error_kind": error_kind::CONNECTION_LOST,
                    "hint": "不要把这个结果当成成功。重试一次；仍失败则调 `test_connection` 确认连接状态。",
                    "exit_code": serde_json::Value::Null,
                    "command": command,
                    "duration_ms": duration_ms,
                    "timed_out": false,
                    "truncated": truncated,
                    "stdout": stdout,
                    "stderr": stderr,
                });
                return Ok(json!({
                    "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result).unwrap_or_default() }],
                    "isError": true,
                    "error_kind": error_kind::CONNECTION_LOST,
                    "exit_code": serde_json::Value::Null,
                }));
            };

            let result = json!({
                "exit_code": code,
                "stdout": stdout,
                "stderr": stderr,
                // The caller must be able to tell an exit code the SHELL
                // reported (which is the last pipeline stage's status, not the
                // whole command's verdict) from one the transport produced.
                "exit_code_source": "remote_process",
                "command": command,
                "duration_ms": duration_ms,
                "timed_out": false,
                "truncated": truncated,
            });
            Ok(json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result).unwrap_or_default() }],
                "exit_code": code,
                "duration_ms": duration_ms,
                "timed_out": false,
                "truncated": truncated,
            }))
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
            let command = resolve_command_arg(args)?;

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
            let task_id = unique_task_id("ssh");
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
                let controls = state.exec_controls.lock().unwrap_or_else(|e| e.into_inner());
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

        "ssh_read_text" => {
            // Escape-free file read. Two paths, picked by what the server
            // offers: SFTP when the subsystem is available (no shell at all,
            // so nothing to quote), else a plain `cat` over an exec channel —
            // which is safe here because the only variable part is the path,
            // and we single-quote it ourselves in `shell_quote`.
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let remote_path = args["remote_path"].as_str().ok_or("缺少 remote_path 参数")?;
            let max_bytes = args["max_bytes"]
                .as_u64()
                .unwrap_or(1024 * 1024)
                .clamp(1024, 8 * 1024 * 1024) as usize;

            let config = state.resolve_via_gui(conn_name, Some("ssh")); 
            let config = match config {
                Ok(c) => c,
                // SFTP-only connections are legitimate targets for a file read.
                Err(e) => match state.resolve_via_gui(conn_name, Some("sftp")) {
                    Ok(c) => c,
                    Err(_) => return Err(e),
                },
            };

            let started = std::time::Instant::now();
            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;

            // Preferred: SFTP. Falls back to a shell read when the subsystem
            // is unavailable (restricted shells, jump hosts, embedded devices).
            let (bytes, content, method) = match open_sftp(&handle).await {
                Ok(sftp) => match sftp.metadata(remote_path).await {
                    Ok(meta) => {
                        let size = meta.size.unwrap_or(0);
                        match sftp.open(remote_path).await {
                            Ok(mut file) => {
                                use tokio::io::AsyncReadExt;
                                let mut buf = Vec::new();
                                let take = (size as usize).min(max_bytes);
                                let mut chunk = vec![0u8; 32 * 1024];
                                let mut read_total = 0usize;
                                loop {
                                    if read_total >= take {
                                        break;
                                    }
                                    let limit = (take - read_total).min(chunk.len());
                                    match file.read(&mut chunk[..limit]).await {
                                        Ok(0) => break,
                                        Ok(n) => {
                                            buf.extend_from_slice(&chunk[..n]);
                                            read_total += n;
                                        }
                                        Err(e) => {
                                            return Err(format!("读取远端文件失败: {}", e));
                                        }
                                    }
                                }
                                (size, String::from_utf8_lossy(&buf).into_owned(), "sftp")
                            }
                            Err(e) => return Err(format!("打开远端文件失败: {}", e)),
                        }
                    }
                    Err(e) => return Err(format!("读取远端文件信息失败（文件不存在?）: {}", e)),
                },
                Err(_) => {
                    // Shell fallback: `wc -c` for the true size + `head -c` for
                    // the content. The path is the only interpolated value and
                    // it goes through a proper single-quote escape.
                    let quoted = shell_quote(remote_path);
                    let cmd = format!(
                        "wc -c < {q} 2>/dev/null | tr -d ' \\t'; printf '\\n'; head -c {n} {q}",
                        q = quoted,
                        n = max_bytes
                    );
                    let mut channel = handle
                        .channel_open_session()
                        .await
                        .map_err(|e| format!("打开通道失败: {}", e))?;
                    channel
                        .exec(true, cmd.as_str())
                        .await
                        .map_err(|e| format!("exec 失败: {}", e))?;
                    let (out, err, code) = collect_channel_output(&mut channel, ssh_max_output_bytes())
                        .await;
                    if code != Some(0) && out.is_empty() {
                        return Err(format!(
                            "读取失败（shell 回退路径）: {}",
                            if err.trim().is_empty() { "文件不存在或无读取权限".to_string() } else { err }
                        ));
                    }
                    // First line is the true size from wc; the rest is content.
                    let (size_line, body) = match out.split_once('\n') {
                        Some((a, b)) => (a.trim().to_string(), b.to_string()),
                        None => (String::new(), out.clone()),
                    };
                    let size: u64 = size_line.parse().unwrap_or(body.len() as u64);
                    (size, strip_ansi(&body), "shell")
                }
            };

            let truncated = (bytes as usize) > content.len();
            let lines = content.lines().count();
            let payload = json!({
                "content": content,
                "bytes": bytes,
                "truncated": truncated,
                "lines": lines,
                "method": method,
                "duration_ms": started.elapsed().as_millis() as u64,
                "remote_path": remote_path,
            });
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;
            Ok(json!({
                "content": [{ "type": "text", "text": serde_json::to_string_pretty(&payload).unwrap_or_default() }],
                "bytes": bytes,
                "truncated": truncated,
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
            if !confirm_or_session_allowed("sftp_upload（上传文件）", &detail) {
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
            if !confirm_or_session_allowed("sftp_remove（删除文件/目录）", &detail) {
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
            if !confirm_or_session_allowed("sftp_rename（重命名/移动）", &detail) {
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
            if !confirm_or_session_allowed("upload_project（上传项目目录）", &detail) {
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
            let extract_cmd = format!(
                "sudo mkdir -p {} && sudo mv {} {} && cd {} && sudo tar -xzf {} && sudo rm -f {} && echo 'UPLOAD_OK'",
                shell_quote(&remote_target),
                shell_quote(&remote_tar_path),
                shell_quote(&remote_target),
                shell_quote(&remote_target),
                shell_quote(&remote_tar_name),
                shell_quote(&remote_tar_name),
            );
            // Read the exit status AND the sentinel the command prints. The old
            // sleep(3)-and-drop-channel version reported success even when
            // sudo was waiting for a password or the filesystem was read-only,
            // so the AI would report a deployment that never happened.
            let (code, out) = exec_collect(&ssh_handle2, &extract_cmd, "远端解压", 300).await?;
            let _ = ssh_handle2.disconnect(russh::Disconnect::ByApplication, "done", "en").await;
            if code != 0 || !out.contains("UPLOAD_OK") {
                return Err(format!(
                    "远端解压失败（退出码 {code}）——项目**未**部署到 {remote_target}。\n远端输出：\n{}",
                    out.trim()
                ));
            }

            log(&format!("[upload_project] 完成! {} → {}", local_dir, remote_target));

            Ok(json!({ "content": [{ "type": "text", "text": format!("✅ 项目上传成功！\n本地: {}\n远程: {}\n大小: {} bytes (tar.gz)", local_dir, remote_target, tar_size) }] }))
        }

        "download_project" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let remote_dir = args["remote_dir"].as_str().ok_or("缺少 remote_dir 参数")?;
            let local_dir = args["local_dir"].as_str().ok_or("缺少 local_dir 参数")?;

            let detail = format!("从服务器 [{}] 下载目录 {} → 本地 {}", conn_name, remote_dir, local_dir);
            if !confirm_or_session_allowed("download_project（下载项目目录）", &detail) {
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
                // Delete any leftover from a previous run FIRST. The temp path
                // is deterministic, so without this a failed tar leaves the
                // previous run's archive in place — and since the download step
                // simply fetches whatever sits at that path, the user silently
                // gets a stale directory tree with a success message.
                "rm -f {} && MYTAR={} MYDIR={} python3 -c '{}' 2>&1",
                shell_quote(&tmp_tar),
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
            // Verify the remote tar actually succeeded before downloading it.
            // The old code slept 3s and moved on, so a missing python3, an
            // unreadable directory, or a tar slower than 3s still produced a
            // "✅ 下载成功" — over a stale or absent archive.
            let (code, out) = exec_collect(&ssh_handle, &tar_cmd, "远端打包", 600).await?;
            if code != 0 || !out.contains("TAR_OK") {
                let _ = ssh_handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;
                return Err(format!(
                    "远端打包失败（退出码 {code}）——已中止下载，未解压任何内容。\n远端输出：\n{}",
                    out.trim()
                ));
            }
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
            // The doc on `evict_stale_tasks` says it runs "lazily on each
            // ssh_status/zmodem_status call", but only ssh_status actually
            // called it. A session that used ONLY zmodem_* — the documented
            // preferred transfer path — therefore kept every completed task,
            // including its full result payload, resident for the whole
            // process lifetime. Mirrors the ssh_status call site.
            evict_stale_tasks(&state.tasks);
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

        _ => { log_err(&format!("未知工具: {}", name)); Err(format!("未知工具: {}", name)) }
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

/// Run `channel` to completion, returning (stdout, stderr, exit_code).
/// (`shell_quote`, defined near the zmodem helpers, is the one canonical
/// quoting helper — reuse it, don't add a second one.)
/// Output is capped at `max` bytes per stream; the cap is reported by the
/// caller via its own `truncated` computation where it matters.
async fn collect_channel_output(
    channel: &mut russh::Channel<russh::client::Msg>,
    max: usize,
) -> (String, String, Option<u32>) {
    use russh::ChannelMsg;
    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();
    let mut exit_code: Option<u32> = None;
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
    (
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
        exit_code,
    )
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
fn ensure_gui_running() -> Result<GuiIpcEndpoint, GuiLaunchError> {
    // Serialize the whole discover-or-launch sequence. Tool calls now run
    // concurrently, so three parallel calls on a not-yet-running GUI used to
    // each observe "no port file" and each spawn their own myshell.exe — the
    // later two hit the single-instance "覆盖启动?" dialog. One lock, one
    // launch; the others re-enter and find the file the winner just wrote.
    static GUI_LAUNCH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = GUI_LAUNCH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ensure_gui_running_locked()
}

/// Why the GUI could not be reached.
///
/// SECURITY: the caller uses `gui_reachable()` to decide whether the visible
/// confirmation dialog was ever on screen — which decides whether falling
/// back to a headless run is legitimate. That decision used to be made by
/// substring-matching the Chinese error text, which makes the *wording* of an
/// error message a load-bearing part of the confirmation gate: reword a string
/// and a dangerous command silently stops asking for confirmation. The
/// property now travels in the type instead of the prose.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GuiLaunchError {
    /// `myshell.exe` not found next to `myshell-mcp.exe` (not installed).
    NotInstalled(std::path::PathBuf),
    /// Could not resolve our own path / install directory.
    LocateFailed(String),
    /// The process exists but could not be spawned.
    SpawnFailed(String),
    /// Launched, but no IPC port file appeared within 30s.
    StartupTimeout,
    /// A listener is bound on the recorded port but does not answer IPC.
    /// The GUI may be alive-but-wedged; we must NOT start a second one.
    IpcUnresponsive,
    /// A myshell.exe process is alive, but there is no usable port file, so
    /// we have no IPC endpoint to talk to. Spawning another GUI would only
    /// trigger the single-instance guard and exit — so this is reported
    /// rather than retried in a loop.
    NoPortFile,
}

impl GuiLaunchError {
    /// True when a GUI process was confirmed up and answering IPC, i.e. the
    /// frontend was reachable and any confirmation dialog it would have shown
    /// actually got a chance to appear.
    fn gui_reachable(&self) -> bool {
        match self {
            // A bound-but-wedged GUI: nothing was shown, but we also must not
            // relaunch — treat as reachable so the headless fallback asks the
            // user rather than proceeding silently.
            //
            // NoPortFile is the same call: a GUI process exists (so we must
            // not start a second one), and no IPC dialog was shown — so the
            // headless fallback must not be taken either. Unreachable from
            // the caller's perspective but never silently executable.
            GuiLaunchError::IpcUnresponsive | GuiLaunchError::NoPortFile => true,
            // Everything else means we never got a working GUI.
            GuiLaunchError::NotInstalled(_)
            | GuiLaunchError::LocateFailed(_)
            | GuiLaunchError::SpawnFailed(_)
            | GuiLaunchError::StartupTimeout => false,
        }
    }
}

impl std::fmt::Display for GuiLaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GuiLaunchError::NotInstalled(p) => write!(
                f,
                "未找到 myshell.exe（期望位置: {}）。请手动打开 MyShell。",
                p.display()
            ),
            GuiLaunchError::LocateFailed(e) => write!(f, "无法定位自身路径或安装目录: {}", e),
            GuiLaunchError::SpawnFailed(e) => write!(f, "启动 myshell.exe 失败: {}", e),
            GuiLaunchError::StartupTimeout => {
                write!(f, "GUI 启动超时（30 秒内未就绪）。请手动打开 MyShell 后重试。")
            }
            GuiLaunchError::IpcUnresponsive => write!(
                f,
                "MyShell GUI 正在运行但 IPC 无响应（进程可能已挂起）。请在任务管理器中检查 myshell.exe，必要时手动重启 MyShell 后重试。"
            ),
            GuiLaunchError::NoPortFile => write!(
                f,
                "MyShell GUI 正在运行，但 IPC 端口文件（gui-ipc-port）缺失，无法建立连接。请完全退出 MyShell 后重新打开（任务管理器结束 myshell.exe 亦可），然后重试。"
            ),
        }
    }
}

fn ensure_gui_running_locked() -> Result<GuiIpcEndpoint, GuiLaunchError> {
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
            return Err(GuiLaunchError::IpcUnresponsive);
        }
        // No listener on the recorded port. Before deleting the file and
        // spawning, check whether a GUI process is actually alive: a live GUI
        // whose port file went missing (deleted externally, or lost to a
        // crash-race) must NOT get a second instance launched at it — the new
        // process would hit the single-instance guard and exit, so this call
        // would burn its whole 30s budget and report a misleading timeout
        // while the real cause is an unreachable port file.
        if gui_process_running() {
            log(&format!(
                "[gui] myshell.exe 正在运行但端口 {} 无监听，端口文件不可用 — 不重复启动 GUI",
                ep.port
            ));
            remove_gui_ipc_port_file(Some(ep.port));
            if let Some(ep) = wait_for_port_file_republish() {
                return Ok(ep);
            }
            return Err(GuiLaunchError::NoPortFile);
        }
        log(&format!(
            "[gui] 陈旧的 gui-ipc-port 文件（端口 {} 无监听）— 删除并重新启动 GUI",
            ep.port
        ));
        remove_gui_ipc_port_file(Some(ep.port));
    }

    // No port file at all (or it was just deleted as stale). A GUI process
    // may still be alive and healthy on a port nobody recorded — spawning a
    // second one in that state can only produce a single-instance rejection
    // that exits instantly, so this call would sit out the full 30s and then
    // report StartupTimeout while the user actually has a running GUI.
    // Report the real condition instead.
    if gui_process_running() {
        log("[gui] myshell.exe 正在运行但缺少 gui-ipc-port 文件 — 不重复启动 GUI");
        if let Some(ep) = wait_for_port_file_republish() {
            return Ok(ep);
        }
        return Err(GuiLaunchError::NoPortFile);
    }

    // Find myshell.exe — it lives in the same directory as myshell-mcp.exe.
    let mcp_exe = std::env::current_exe()
        .map_err(|e| GuiLaunchError::LocateFailed(e.to_string()))?;
    let gui_exe = mcp_exe
        .parent()
        .ok_or_else(|| GuiLaunchError::LocateFailed("无法定位安装目录".to_string()))?
        .join("myshell.exe");

    if !gui_exe.exists() {
        return Err(GuiLaunchError::NotInstalled(gui_exe));
    }

    log(&format!("启动 GUI: {}", gui_exe.display()));
    std::process::Command::new(&gui_exe)
        .spawn()
        .map_err(|e| GuiLaunchError::SpawnFailed(e.to_string()))?;

    // Poll for the port file (GUI writes it on startup once the IPC listener
    // binds). Cap at 20s — the slowest observed cold start (WebView2 init) is
    // ~14s, and polling any longer would run into the ~30s budget most MCP
    // clients give a request, turning a diagnosable StartupTimeout into an
    // opaque client-side transport timeout.
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Some(ep) = read_gui_ipc_endpoint() {
            log(&format!("GUI 就绪，IPC port = {}", ep.port));
            return Ok(ep);
        }
    }
    Err(GuiLaunchError::StartupTimeout)
}

/// Compare a NUL-terminated wide process name from `PROCESSENTRY32W` against
/// a target name. Exact match only — `myshell.exe` must not match
/// `myshell-mcp.exe` (which is this very binary).
#[cfg(windows)]
fn exe_name_eq(a: *const u16, b: &[u16]) -> bool {
    if a.is_null() {
        return false;
    }
    // SAFETY: two bounds on the scan. (1) szExeFile is NUL-terminated inside
    // its [u16; 260] buffer per the API contract; test buffers passed here
    // are NUL-terminated too. (2) Regardless, we stop as soon as the name is
    // longer than `b` (such a name can never compare equal), so the deepest
    // index ever read is b.len() — 11 for our target — far inside the buffer.
    unsafe {
        let mut n = 0usize;
        while *a.add(n) != 0 {
            n += 1;
            if n > b.len() {
                return false;
            }
        }
        n == b.len() && (0..n).all(|i| *a.add(i) == b[i])
    }
}

/// Is a MyShell GUI process running right now?
///
/// The port file is the only channel we have to reach the GUI, and it is
/// written once at startup — so a live GUI whose file went missing is
/// invisible to us while the process itself is still detectable. Checking the
/// process table lets us tell "no GUI, safe to launch one" apart from "GUI is
/// up, but unreachable", which the old code conflated and answered by
/// launching a second instance that the single-instance guard instantly
/// rejects. That made every tool call burn its full 30s budget and then
/// report a misleading startup timeout.
///
/// Non-Windows: false (always launch) — same convention as the other
/// Windows-only probes in this file.
#[cfg(windows)]
fn gui_process_running() -> bool {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let target: Vec<u16> = std::ffi::OsStr::new("myshell.exe").encode_wide().collect();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == winapi::um::handleapi::INVALID_HANDLE_VALUE {
            // Can't tell — assume a GUI may be running so we never launch a
            // duplicate on an unknown state.
            return true;
        }
        // SAFETY: PROCESSENTRY32W is an all-zero-valid POD struct (pointers
        // are not dereferenced until Process32FirstW fills them); dwSize is
        // set immediately after, as the API requires.
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                if exe_name_eq(entry.szExeFile.as_ptr(), &target) {
                    found = true;
                    break;
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        winapi::um::handleapi::CloseHandle(snap);
        found
    }
}

#[cfg(not(windows))]
fn gui_process_running() -> bool {
    false
}

/// Delete the GUI's IPC port-discovery file — but ONLY if it still holds the
/// exact endpoint we judged stale.
///
/// The check is a compare-and-delete, not an unconditional remove. Between
/// "I read the file and found port P dead" and "I delete the file", the
/// GUI's watchdog may have re-published a FRESH, LIVE port (P2) — e.g. the
/// GUI was merely slow to bind, or another MCP instance raced us. An
/// unconditional delete would then destroy a healthy GUI's discovery file:
/// every later MCP call finds no port file, spawns a doomed second instance
/// and burns its whole budget. Deleting only the file that still names the
/// port we proved dead cannot touch a newer publication.
///
/// `stale_port` = the port we found with no listener. `None` means "no
/// endpoint to compare against" — then the file is left alone and the caller
/// reports the real condition (NoPortFile).
fn remove_gui_ipc_port_file(stale_port: Option<u16>) {
    let mut path = match dirs::config_dir() {
        Some(d) => d,
        None => return,
    };
    path.push("myshell");
    path.push("gui-ipc-port");
    remove_port_file_if_stale(&path, stale_port);
}

/// Compare-and-delete core of [`remove_gui_ipc_port_file`], with the target
/// path injected so the behavior can be tested against a temp dir instead of
/// the real `<config>/myshell/gui-ipc-port`.
fn remove_port_file_if_stale(path: &std::path::Path, stale_port: Option<u16>) {
    if let Some(port) = stale_port {
        // Re-read: if the file now names a DIFFERENT port, a newer
        // publication replaced ours — leave it untouched.
        if let Ok(raw) = std::fs::read_to_string(path) {
            let current = raw
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<u16>().ok());
            if current != Some(port) {
                log(&format!(
                    "[gui] 端口文件已被更新（现为 {:?}，判定陈旧的为 {}）— 不删除",
                    current, port
                ));
                return;
            }
        }
    }
    let _ = std::fs::remove_file(path);
}

/// Wait briefly for a live GUI's port-file watchdog to re-publish
/// `gui-ipc-port`, then hand back the endpoint once it actually answers IPC.
///
/// GUI builds with the watchdog re-assert their port file every ~5s whenever
/// it is missing or no longer carries their token, so "GUI alive, file gone"
/// is a TRANSIENT state for them — waiting one watchdog cycle here recovers
/// it with no user action and no second instance. GUI builds without the
/// watchdog (e.g. installed 2.18.0) never republish, the ~7s wait simply
/// expires, and the caller reports NoPortFile exactly as before: degraded to
/// the pre-existing error, never worse.
fn wait_for_port_file_republish() -> Option<GuiIpcEndpoint> {
    for _ in 0..7 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if let Some(ep) = read_gui_ipc_endpoint() {
            if std::net::TcpStream::connect(("127.0.0.1", ep.port)).is_ok()
                && query_gui_vault_status(&ep).is_some()
            {
                log(&format!(
                    "[gui] 端口文件已由 GUI 看门狗重新发布（端口 {}）— 复用现有 GUI",
                    ep.port
                ));
                return Some(ep);
            }
        }
    }
    None
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

/// How many times a parked tool call re-checks the vault while waiting for
/// the user to type the master password. 10 × 2s = 20s total — deliberately
/// UNDER the ~30s budget most MCP clients give one request, so the failure
/// path still returns a structured `vault_locked` error in time instead of
/// dying to a transport timeout (the failure mode that made the original
/// 30s poll, removed in 阶段 187, unrecoverable).
const VAULT_UNLOCK_WAIT_PROBES: usize = 10;
const VAULT_UNLOCK_WAIT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Outcome of the in-call unlock wait. Three states, not a bool: if the GUI
/// died mid-wait (every probe unreachable), reporting "still locked" would
/// send the user to type a password in a window that no longer exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnlockWaitOutcome {
    /// The vault got unlocked while we waited — this very call proceeds.
    Unlocked,
    /// Probes exhausted; the GUI stayed reachable and locked the whole time.
    StillLocked,
    /// Not one probe reached the GUI — it likely died mid-wait.
    Unreachable,
}

/// Sleep, then ask `probe` for the vault state; repeat until it reports
/// unlocked or `probes` runs out. `probe` is injected so the loop itself is
/// unit-testable without a live GUI (the GUI-backed closure is a one-liner
/// at the call site).
fn wait_for_unlock(
    probes: usize,
    interval: std::time::Duration,
    mut probe: impl FnMut() -> Option<(bool, bool)>,
) -> UnlockWaitOutcome {
    let mut ever_reachable = false;
    for _ in 0..probes {
        std::thread::sleep(interval);
        match probe() {
            Some((_, true)) => return UnlockWaitOutcome::Unlocked,
            Some(_) => ever_reachable = true,
            None => {}
        }
    }
    if ever_reachable {
        UnlockWaitOutcome::StillLocked
    } else {
        UnlockWaitOutcome::Unreachable
    }
}

/// Fail-fast-with-a-grace-period vault gate. Called at the top of every tool
/// handler that needs anything beyond task-status polling (ssh_exec, sftp_*,
/// list_connections, open_in_gui, screenshot_terminal, and the async
/// starters' pre-flight).
///
/// Behavior:
///   1. Ensure the GUI is running — AUTO-LAUNCH myshell.exe if it isn't
///      (waits up to 30s for the IPC port file while it boots).
///   2. Query the GUI's vault state ONCE (a few retries only to cover a
///      freshly launched GUI still binding its IPC listener).
///   3. Unlocked → Ok, proceed with the tool.
///      Not initialized → immediate actionable error (first-run setup is a
///      long flow; waiting for it inside the call is pointless).
///      Locked → focus the GUI so the password gate is in the user's face,
///      then WAIT IN-CALL for them to type the master password (up to
///      [VAULT_UNLOCK_WAIT_PROBES] probes). If they unlock, this very call
///      proceeds — the AI does not need to relay a "now retry" round-trip,
///      and the user never has to announce "我已经解锁了" (阶段 198).
///      If the wait expires, return the same structured error as before.
///
/// Why an in-call wait is now safe when 阶段 187 removed the old 30s poll:
/// the old poll was UNBOUNDED-in-spirit (30s) and burned the client's whole
/// request budget, so the outcome was always a transport timeout. The wait
/// below is capped at 20s — under the ~30s clients allow — so the failure
/// path still returns a structured `vault_locked` error in time, while the
/// success path (user unlocks while the AI's request is parked) now closes
/// the whole flow in ONE call. Each tools/call runs on its own tokio task
/// (阶段 185), so parked calls never block ssh_status or other connections.
fn ensure_vault_ready() -> Result<(), VaultGateError> {
    let ep = ensure_gui_running().map_err(|e| VaultGateError::GuiUnavailable(e.to_string()))?;

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
        Some((false, _)) => Err(VaultGateError::NotInitialized),
        Some((true, false)) => {
            send_gui_focus_unlock(&ep);
            log("[vault] 保险库锁定 — 调用内等待用户解锁（最长 20 秒）");
            match wait_for_unlock(
                VAULT_UNLOCK_WAIT_PROBES,
                VAULT_UNLOCK_WAIT_INTERVAL,
                || query_gui_vault_status(&ep),
            ) {
                UnlockWaitOutcome::Unlocked => {
                    log("[vault] 等待期间已解锁 — 继续执行本次调用");
                    return Ok(());
                }
                UnlockWaitOutcome::StillLocked => Err(VaultGateError::Locked),
                // GUI died mid-wait: "locked" would point the user at a
                // password window that no longer exists.
                UnlockWaitOutcome::Unreachable => Err(VaultGateError::IpcUnresponsive),
            }
        }
        None => Err(VaultGateError::IpcUnresponsive),
    }
}

/// Outcome of the vault gate, as a type rather than prose.
///
/// The `error_kind` returned to the caller (`vault_locked`) and the
/// `vault_state` hint are both derived from this variant, so a rewording can
/// never silently reclassify a gate failure — and `vault_status` can report
/// the same state to a polling caller without re-deriving it from text.
#[derive(Debug, Clone, PartialEq, Eq)]
enum VaultGateError {
    /// No working GUI at all (not installed / won't launch / wedged).
    GuiUnavailable(String),
    /// The GUI is up but has no vault configured yet.
    NotInitialized,
    /// Vault exists and the user has not entered the master password.
    Locked,
    /// GUI reachable at launch, then IPC stopped answering.
    IpcUnresponsive,
}

impl VaultGateError {
    /// Machine-readable state for the caller. `error_kind` stays `vault_locked`
    /// for all variants — from the caller's perspective the fix is always
    /// "the user must unlock in MyShell, then retry".
    fn state(&self) -> &'static str {
        match self {
            VaultGateError::Locked => "locked",
            VaultGateError::NotInitialized => "not_initialized",
            VaultGateError::GuiUnavailable(_) => "gui_unavailable",
            VaultGateError::IpcUnresponsive => "ipc_unresponsive",
        }
    }

    fn message(&self) -> String {
        match self {
            VaultGateError::GuiUnavailable(e) => format!(
                "MyShell GUI 未运行且无法自动启动：{}。请手动打开 MyShell 桌面应用，然后重试。",
                e
            ),
            VaultGateError::NotInitialized =>
                "保险库尚未初始化：请先在 MyShell GUI 中设置主密码（首次启动时会引导设置），然后重新调用此工具。"
                    .to_string(),
            VaultGateError::Locked =>
                "保险库未解锁：MyShell 窗口已置顶，本次调用已就地等待 20 秒仍未解锁。请在 MyShell 中输入主密码，然后重新调用此工具。"
                    .to_string(),
            VaultGateError::IpcUnresponsive =>
                "无法查询 MyShell GUI 的保险库状态（IPC 无响应）。请确认 MyShell 桌面应用正在运行后重试。"
                    .to_string(),
        }
    }
}

impl std::fmt::Display for VaultGateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
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
) -> Result<Value, GuiExecError> {
    // Resolve the connection id via plaintext lookup — NO DEK needed.
    // The actual SSH connection + credential use happens inside the GUI
    // process (the user has unlocked the vault there).
    let conn_id = state.find_connection_id(conn_name, Some("ssh"))?;

    // Ensure the GUI is running (auto-start if needed).
    let ep = ensure_gui_running()?;
    let started = std::time::Instant::now();

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
        // Session grant: when the user already chose "allow this session",
        // tell the GUI to skip its confirmation dialog entirely (the grant is
        // authoritative in this process; the GUI dialog is advisory UX).
        "session_allowed": session_allowed(),
    }));
    writeln!(stream, "{}", cmd).map_err(|e| format!("发送命令失败: {e}"))?;
    stream.flush().map_err(|e| format!("flush 失败: {e}"))?;

    // Read the response — a JSON object with {ok, stdout?, exit_code?, error?,
    // session_allowed?}. The GUI sets session_allowed when the user picked
    // "本轮会话均允许" at its confirmation dialog; we honor it from here on.
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line).map_err(|e| format!("读取响应失败: {e}"))?;

    let resp: Value = serde_json::from_str(resp_line.trim())
        .map_err(|e| format!("响应解析失败: {e} (raw: {})", resp_line.trim()))?;

    if resp["session_allowed"].as_bool() == Some(true) {
        grant_session_allowed();
    }
    if resp["session_revoked"].as_bool() == Some(true) {
        revoke_session_allowed();
    }

    if resp["ok"].as_bool() == Some(true) {
        // ANSI/OSC stripping happens here, at the boundary: the GUI captured
        // this from a real PTY (prompt, window-title OSC, grep colors), and
        // raw escape bytes are noise for an agent that cannot render them.
        let stdout = strip_ansi(resp["stdout"].as_str().unwrap_or(""));
        let exit_code = resp["exit_code"].as_i64().unwrap_or(0);
        // Snapshot once: two `started.elapsed()` calls a few microseconds
        // apart would report different durations for the same execution.
        let duration_ms = started.elapsed().as_millis() as u64;
        let result = json!({
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": "",
            // Honest signalling: the caller must be able to tell a real exit
            // code from "the shell printed $? of the last pipeline stage".
            "exit_code_source": "shell_last_status",
            "duration_ms": duration_ms,
            "timed_out": false,
            "truncated": false,
            "command": command,
            "executed_in": "gui_terminal",
        });
        Ok(json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&result).unwrap_or_default() }],
            "exit_code": exit_code,
            "duration_ms": duration_ms,
            "timed_out": false,
            "command": command,
        }))
    } else {
        let err_msg = resp["error"].as_str().unwrap_or("未知错误");
        // If there's partial stdout, include it (stripped, same as the OK path).
        let stdout = strip_ansi(resp["stdout"].as_str().unwrap_or(""));
        let message = if !stdout.is_empty() {
            format!("{}（部分输出: {}）", err_msg, stdout)
        } else {
            err_msg.to_string()
        };
        Err(GuiExecError {
            message,
            command_sent: resp["command_sent"].as_bool() == Some(true),
            elapsed_ms: started.elapsed().as_millis() as u64,
            // We got a real IPC response, so the GUI is up and reachable.
            gui_reachable: true,
        })
    }
}

/// Failure from the GUI-side exec path, carrying whether the command had
/// ALREADY been written to the terminal when it failed.
///
/// This distinction is load-bearing: the GUI sends `sshSend(command + "\n")`
/// first and only then can report failure (hard timeout, lost sentinel, IPC
/// teardown). Re-running the command headlessly in that case executes it a
/// SECOND time, invisibly — the user watches one command in the visible tab
/// while an identical one runs behind their back.
#[derive(Debug)]
struct GuiExecError {
    message: String,
    command_sent: bool,
    /// Wall-clock time spent inside `exec_in_gui_tab` before the failure —
    /// lets the caller tell "failed instantly" from "failed after burning the
    /// whole timeout", which decides whether retrying is worthwhile.
    elapsed_ms: u64,
    /// Whether a working GUI was confirmed up when this failed. SECURITY:
    /// this gates the headless fallback — `false` means the visible
    /// confirmation dialog could not have appeared, so falling back is only
    /// legitimate for commands that would not have needed one. Derived from
    /// the error TYPE (never from its wording).
    gui_reachable: bool,
}

impl From<GuiLaunchError> for GuiExecError {
    fn from(e: GuiLaunchError) -> Self {
        let gui_reachable = e.gui_reachable();
        GuiExecError {
            message: e.to_string(),
            command_sent: false,
            elapsed_ms: 0,
            gui_reachable,
        }
    }
}

impl From<String> for GuiExecError {
    /// Every `?` inside `exec_in_gui_tab` fails BEFORE the command can reach
    /// the terminal (connection lookup, GUI auto-start, TCP connect, write,
    /// response read). `command_sent` therefore stays false — which is exactly
    /// what keeps a headless retry legitimate for those paths.
    fn from(message: String) -> Self {
        GuiExecError {
            message,
            command_sent: false,
            elapsed_ms: 0,
            // Every `?` that produces a plain String runs AFTER
            // ensure_gui_running succeeded, so the frontend was reachable and
            // could have shown its dialog.
            gui_reachable: true,
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
        // Only low-frequency, decision-bearing events are logged. The byte
        // firehose (zmodem_raw / ssh_output) and per-chunk progress used to be
        // logged verbatim — 1332 progress lines in one session, and a
        // `bytesTransferred` line per network chunk on top; that noise is what
        // made the old mcp.log unreadable when diagnosing a failure. Start /
        // offer / file-complete / end / error still land, and those are the
        // ones a transfer post-mortem actually needs.
        match event {
            "zmodem_raw" | "ssh_output" | "zmodem_progress" => {}
            _ => {
                let preview = format!("{}", payload);
                log(&format!("[McpZmodemSink] event={} {}", event, preview));
            }
        }

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

/// Sanitize an untrusted `sz -r` offer name ("dir/sub/file.txt") into a safe
/// RELATIVE path under the local dir. Each segment is cleaned of characters
/// illegal on Windows (':' → '_', so drive letters and NTFS alternate data
/// streams are neutralized); traversal (".", "..", absolute prefixes, empty)
/// segments are dropped so the tree can never escape `local_dir`. Returns
/// None if nothing usable remains.
fn sanitize_relative_path(name: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for raw in name.split(['/', '\\']) {
        let cleaned: String = raw
            .chars()
            .map(|c| match c {
                '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
                c if (c as u32) < 0x20 => '_',
                c => c,
            })
            .collect();
        // Trim edges (Windows quirk: trailing dots/spaces are ignored by the
        // filesystem but defeat the ".." check below).
        let cleaned = cleaned.trim().trim_end_matches(['.', ' ']);
        if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
            continue;
        }
        parts.push(cleaned.to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(std::path::MAIN_SEPARATOR_STR))
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_relative_path;
    use super::TransferProgress;
    use super::{compose_line, open_log, prune_old_logs, strip_ansi, write_line, LogFiles, LOG_LINE_MAX};

    /// 调用方反馈的头号问题：JSON 转义。`command_b64` 必须能在不破坏任何字符的
    /// 前提下还原 shell 命令 —— 尤其是引号、反斜杠、`$` 与花括号。
    #[test]
    fn base64_command_round_trips_awkward_shell() {
        use super::decode_command_b64;
        use base64::Engine as _;
        let original = r#"grep -n 'HeartbeatThread\|Controller' /x.conf; awk '{for(i=2;i<=NF;i++) print $i}' | sed 's/\$$//'"#;
        let b64 = base64::engine::general_purpose::STANDARD.encode(original.as_bytes());
        assert_eq!(decode_command_b64(&b64).unwrap(), original);
        // 空白/换行容错（调用方常按 76 列折行）
        let wrapped = format!("{}\n{}", &b64[..10], &b64[10..]);
        assert_eq!(decode_command_b64(&wrapped).unwrap(), original);
        // 非法输入必须报错而不是静默给空串
        assert!(decode_command_b64("not base64!!!").is_err());
    }

    /// 采集层剥 ANSI：CSI 颜色、OSC 标题/超链接、裸 ESC 全清掉，中文与
    /// 制表/换行必须原样保留（AI 读的是内容，不是终端渲染）。
    #[test]
    fn strip_ansi_removes_escapes_but_keeps_text() {
        let cases = [
            ("\u{1b}]0;ustc@bigdata62:~\u{7}$ ls", "$ ls"),
            ("\u{1b}[01;31m\u{1b}[K60018\u{1b}[m\u{1b}[K", "60018"),
            ("\u{1b}]8;;http://x/y\u{1b}\\link\u{1b}]8;;\u{1b}\\", "link"),
            ("中文\t保留\n第二行", "中文\t保留\n第二行"),
            ("no escapes at all", "no escapes at all"),
        ];
        for (input, want) in cases {
            assert_eq!(strip_ansi(input), want, "input={input:?}");
        }
    }

    /// 错误分类要落在正确的机器可读类别上 —— 调用方靠它分支，不靠中文散文。
    #[test]
    fn classify_error_maps_known_failures() {
        use super::{classify_error, error_kind};
        assert_eq!(classify_error("⛔ 用户已拒绝高危操作：ssh_exec"), error_kind::USER_DENIED);
        assert_eq!(classify_error("保险库未解锁：..."), error_kind::VAULT_LOCKED);
        assert_eq!(classify_error("bash: dmesg: Permission denied"), error_kind::PERMISSION_DENIED);
        assert_eq!(classify_error("Interactive authentication required. polkit"), error_kind::PERMISSION_DENIED);
        assert_eq!(classify_error("命令超时（60秒未完成）"), error_kind::TIMEOUT);
        assert_eq!(classify_error("exec timeout (60s)"), error_kind::TIMEOUT);
        assert_eq!(
            classify_error("上一条命令仍在该服务器的终端中执行。"),
            error_kind::BUSY
        );
        assert_eq!(classify_error("会话已断开且重连失败：..."), error_kind::CONNECTION_LOST);
        assert_eq!(classify_error("缺少 connection 参数"), error_kind::BAD_REQUEST);
        assert_eq!(classify_error("无法提取文件名"), error_kind::INTERNAL);
        // 顺序敏感：权限拒绝消息里若也含「超时」二字，必须仍判成权限类别
        assert_eq!(
            classify_error("Permission denied（读取超时）"),
            error_kind::PERMISSION_DENIED
        );
    }

    /// schema 与实现必须一致：服务端接受 `command_b64`，因此 `ssh_exec` /
    /// `ssh_run` 的 required 不能写死 `command` —— 否则做严格校验的客户端会
    /// 拒掉一个完全合法的调用。（这个不一致真实存在过，见阶段 186。）
    #[test]
    fn command_tools_accept_either_command_form() {
        let defs = super::tool_definitions();
        for tool in ["ssh_exec", "ssh_run"] {
            let t = defs["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == tool)
                .unwrap_or_else(|| panic!("{tool} 未定义"));
            let schema = &t["inputSchema"];
            let props = &schema["properties"];
            assert!(
                props.get("command").is_some() && props.get("command_b64").is_some(),
                "{tool} 应同时暴露 command 与 command_b64"
            );
            let required: Vec<&str> = schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            assert!(
                !required.contains(&"command"),
                "{tool} 不得把 command 列为必填（command_b64 也是合法形态）"
            );
            assert!(required.contains(&"connection"), "{tool} 必须要求 connection");
            assert!(
                schema["anyOf"].as_array().map(|a| a.len() == 2).unwrap_or(false),
                "{tool} 应声明 anyOf: [command, command_b64]"
            );
        }
    }

    /// 编码规范必须真的出现在下发给客户端的 instructions 里 —— 规范写进
    /// 文档但没写进 instructions，对运行时 agent 等于不存在。
    #[test]
    fn instructions_state_the_encoding_norm() {
        let text = super::SERVER_INSTRUCTIONS;
        for needle in ["command_b64", "ARGUMENT ENCODING", "ssh_read_text", "error_kind"] {
            assert!(text.contains(needle), "instructions 缺少 {needle}");
        }
        // 「客户端抛出、服务端收不到」这一事实必须写明，否则接入方会去找服务端日志
        assert!(text.contains("before the request reaches this server") || text.contains("CLIENT"));
    }

    /// 阶段 198：调用内等待解锁的循环语义 —— 迟到解锁要能等到，
    /// 全程锁定返回 StillLocked；GUI 中途死亡（全程 None）必须与「锁定」
    /// 区分开，否则会把用户指向一个已经不存在的密码窗口。
    #[test]
    fn unlock_wait_polls_until_unlocked() {
        use super::{UnlockWaitOutcome, wait_for_unlock};
        use std::time::Duration;
        let tick = Duration::from_millis(1);

        // 第 3 次探测时用户解锁
        let mut calls = 0;
        let unlocked_late = || {
            calls += 1;
            if calls >= 3 { Some((true, true)) } else { Some((true, false)) }
        };
        assert_eq!(wait_for_unlock(10, tick, unlocked_late), UnlockWaitOutcome::Unlocked);
        assert_eq!(calls, 3, "解锁后应立即停止探测");

        // 始终未解锁：耗尽次数后如实返回 StillLocked
        let mut probes = 0;
        let never = || {
            probes += 1;
            Some((true, false))
        };
        assert_eq!(wait_for_unlock(5, tick, never), UnlockWaitOutcome::StillLocked);
        assert_eq!(probes, 5, "应恰好探测满次数");

        // GUI 中途死亡（全程 None）：不得误报 StillLocked
        assert_eq!(
            wait_for_unlock(2, tick, || None),
            UnlockWaitOutcome::Unreachable
        );

        // 先可达后失联（GUI 在等待中崩溃）：以最后已知状态 StillLocked 报告
        let mut seq_calls = 0;
        let flaky = || {
            seq_calls += 1;
            if seq_calls == 1 { Some((true, false)) } else { None }
        };
        assert_eq!(wait_for_unlock(3, tick, flaky), UnlockWaitOutcome::StillLocked);
    }

    /// 审计不变量（阶段 187）：GUI 可达性必须来自错误**类型**，不能来自
    /// 错误**文案**。这条闸门锁住「改一个字就可能让高危命令静默执行」的结构。
    #[test]
    fn gui_reachability_comes_from_type_not_wording() {
        use super::{GuiLaunchError, GuiExecError};
        // 不可达 ⇒ 可见确认框从未出现 ⇒ headless 回退是合法的
        for e in [
            GuiLaunchError::NotInstalled("C:\\x\\myshell.exe".into()),
            GuiLaunchError::LocateFailed("boom".into()),
            GuiLaunchError::SpawnFailed("boom".into()),
            GuiLaunchError::StartupTimeout,
        ] {
            assert!(!e.gui_reachable(), "{e:?} 应判定为 GUI 不可达");
            let ge: GuiExecError = e.into();
            assert!(!ge.gui_reachable, "转换后仍应为不可达: {ge:?}");
        }
        // 监听存在但无响应 ⇒ 也算「可达」：绝不能因为无响应就静默改走无头
        assert!(GuiLaunchError::IpcUnresponsive.gui_reachable());
        // 进程在但端口文件丢失同理：不能启动第二个实例，更不能静默执行
        assert!(GuiLaunchError::NoPortFile.gui_reachable());
        // 普通字符串错误全部发生在 GUI 就绪之后
        assert!(GuiExecError::from("whatever".to_string()).gui_reachable);
    }

    /// 端口文件丢失时必须能通过进程表发现「GUI 还活着」——否则
    /// ensure_gui_running_locked 会启动第二个 myshell.exe，被单实例守卫
    /// 立刻拒绝退出，于是每次调用都耗尽 30s 并报 StartupTimeout
    /// （阶段 192 观测到的 MCP 工具全部 30s 超时的直接原因）。
    ///
    /// 这里锁住名称匹配规则而非结果：进程表里有无 myshell.exe 取决于
    /// 跑测试的机器，断言它等于 true/false 都会让测试不稳定。
    #[test]
    #[cfg(windows)]
    fn gui_process_probe_matches_only_the_gui() {
        use super::exe_name_eq;
        use std::os::windows::ffi::OsStrExt;

        // 模拟 PROCESSENTRY32W.szExeFile 的 NUL 终止形态
        fn cstr(s: &str) -> Vec<u16> {
            let mut v: Vec<u16> = std::ffi::OsStr::new(s).encode_wide().collect();
            v.push(0);
            v
        }
        fn chars(s: &str) -> Vec<u16> {
            std::ffi::OsStr::new(s).encode_wide().collect()
        }

        let gui = cstr("myshell.exe");
        let mcp = cstr("myshell-mcp.exe");
        let cli = cstr("myshell-cli.exe");
        let target = chars("myshell.exe");
        assert!(exe_name_eq(gui.as_ptr(), &target));
        // 关键：不能把 MCP/CLI 自身误判成 GUI，否则永远不会去启动 GUI
        assert!(!exe_name_eq(mcp.as_ptr(), &target));
        assert!(!exe_name_eq(cli.as_ptr(), &target));
        assert!(!exe_name_eq(std::ptr::null(), &target));
    }

    /// 门禁状态必须能脱离文案被机器读取（vault_status 轮询契约的基础）。
    #[test]
    fn vault_gate_states_are_machine_readable() {
        use super::VaultGateError;
        assert_eq!(VaultGateError::Locked.state(), "locked");
        assert_eq!(VaultGateError::NotInitialized.state(), "not_initialized");
        assert_eq!(VaultGateError::GuiUnavailable("x".into()).state(), "gui_unavailable");
        assert_eq!(VaultGateError::IpcUnresponsive.state(), "ipc_unresponsive");
        // 四种状态的消息都必须是可行动的中文提示，而非内部代号
        for e in [
            VaultGateError::Locked,
            VaultGateError::NotInitialized,
            VaultGateError::GuiUnavailable("x".into()),
            VaultGateError::IpcUnresponsive,
        ] {
            let m = e.message();
            assert!(m.chars().count() > 10, "message 太短: {m}");
        }
    }

    /// 每个 error_kind 都要有非空 hint —— 只说「失败了」的错误等于没信息。
    #[test]
    fn every_error_kind_carries_a_hint() {
        use super::{error_kind, tool_error};
        for kind in [
            error_kind::PERMISSION_DENIED, error_kind::USER_DENIED, error_kind::TIMEOUT,
            error_kind::BUSY, error_kind::CONNECTION_LOST, error_kind::CONNECT_FAILED,
            error_kind::VAULT_LOCKED, error_kind::BAD_REQUEST, error_kind::NOT_FOUND,
            error_kind::INTERNAL,
        ] {
            let v = tool_error(Some(kind), "boom".to_string());
            assert_eq!(v["error_kind"], kind);
            assert_eq!(v["isError"], true);
            let hint = v["hint"].as_str().unwrap_or("");
            assert!(hint.len() > 10, "kind {kind} 的 hint 太短: {hint:?}");
        }
    }

    /// 基础日志与错误日志必须落到两个文件，且互不串写 —— 用户拿到报障时
    /// 只需要给 error 文件，base 文件仍是干净的会话轨迹。
    #[test]
    fn base_and_error_go_to_separate_files() {
        let dir = std::env::temp_dir().join(format!("myshell-mcp-logtest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut files = LogFiles::default();
        write_line(&mut files, &dir, false, "正常流程：MCP server starting");
        write_line(&mut files, &dir, true, "错误：数据库初始化失败");
        drop(files);

        let day = super::log_day();
        let base = std::fs::read_to_string(dir.join(format!("mcp-{}.log", day))).unwrap();
        let err = std::fs::read_to_string(dir.join(format!("mcp-error-{}.log", day))).unwrap();
        assert!(base.contains("正常流程"), "base 应含普通日志: {base}");
        assert!(!base.contains("数据库初始化失败"), "base 不得混入错误: {base}");
        assert!(err.contains("数据库初始化失败"), "error 文件应含错误: {err}");
        assert!(!err.contains("正常流程"), "error 文件不得混入普通日志: {err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 超过保留窗口的 mcp*.log 要被清掉，窗口内的必须留下；非 mcp 日志
    /// （GUI 的 myshell-<day>.log）绝不能被 MCP 删。
    #[test]
    fn prune_removes_only_expired_mcp_logs() {
        let dir = std::env::temp_dir().join(format!("myshell-mcp-prunetest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let stale = dir.join("mcp-1.log");
        let stale_err = dir.join("mcp-error-1.log");
        let fresh = dir.join("mcp-999999.log");
        let gui = dir.join("myshell-999999.log");
        for p in [&stale, &stale_err, &fresh, &gui] {
            std::fs::write(p, "x").unwrap();
        }
        // 把两个 mcp 文件的时间戳推到 30 天前。
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 24 * 60 * 60);
        for p in [&stale, &stale_err] {
            let f = std::fs::OpenOptions::new().write(true).open(p).unwrap();
            f.set_modified(old).unwrap();
        }

        prune_old_logs(&dir);

        assert!(!stale.exists(), "超期基础日志应被清理");
        assert!(!stale_err.exists(), "超期错误日志应被清理");
        assert!(fresh.exists(), "保留窗口内的日志不得删除");
        assert!(gui.exists(), "GUI 的日志不归 MCP 清理");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 超长行必须被截断且不撕裂多字节字符 —— 旧实现会把整条 heredoc 命令
    /// 原样写盘，一次会话就能把日志撑到数 MB。
    #[test]
    fn long_line_is_truncated_on_char_boundary() {
        let msg = format!("命令：{}", "中文".repeat(LOG_LINE_MAX));
        let line = compose_line(&msg);
        assert!(line.contains("truncated"), "应标注截断: {}", &line[..80.min(line.len())]);
        assert!(line.len() < LOG_LINE_MAX + 200, "截断后长度应受控，实际 {}", line.len());
        // 仍是合法 UTF-8（String 保证），且保留原文开头。
        assert!(line.contains("命令：中"));
    }

    /// 目录不可写时不能 panic 也不能卡住：log 是 best-effort 的旁路。
    #[test]
    fn unwritable_dir_disables_logging_without_panic() {
        let missing = std::env::temp_dir().join(format!("myshell-mcp-nodir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&missing);
        let mut files = LogFiles::default();
        write_line(&mut files, &missing, false, "这条无法写盘");
        write_line(&mut files, &missing, false, "第二条也应被静默跳过");
        assert!(files.disabled, "打开失败后应闩死，避免每行都做一次系统调用");
        assert!(open_log(&missing, super::log_day(), false).is_none());
    }

    /// 真机验证抓到的那个 bug 的最小复现：单文件、2 MiB，走
    /// Offer → Progress → FileComplete → End。
    ///
    /// 修复前，FileComplete 分支把 `bytes_total` 写成字面量 0，完成后又
    /// 没有补回，于是 zmodem_status 按 0/0 算出 **0%**——一次成功的下载
    /// 被报成"传输 0%"。2 MiB / 32665 个转义字节，真机双向 md5 一致，
    /// 但进度是 0。
    #[test]
    fn single_file_download_reports_100_percent_when_done() {
        let mut p = TransferProgress::default();

        assert_eq!(p.offer(2_097_152), (0, 2_097_152));
        assert_eq!(p.progress(1_048_576, 2_097_152), (1_048_576, 2_097_152));
        // 关键：FileComplete 之后 total 仍然是 2 MiB，不能变成 0。
        assert_eq!(p.file_complete(2_097_152), (2_097_152, 2_097_152));
        assert_eq!(p.finish(2_097_152), (2_097_152, 2_097_152));
    }

    /// 多文件（`sz -r` 传目录）：第二个 offer 不能把进度条清零。
    /// 修复前每个 Offer 都会 `mark(0, size)`，于是每来一个新文件，
    /// 已完成的前一个文件就从分子里消失了。
    #[test]
    fn multi_file_download_accumulates_across_offers() {
        let mut p = TransferProgress::default();

        p.offer(100);
        p.progress(50, 100);
        p.file_complete(100);
        // 文件 B 到来 —— 进度应保留 A 的 100 字节。
        assert_eq!(p.offer(200), (100, 300), "新 offer 不能把已完成的部分清零");
        p.progress(200, 200);
        p.file_complete(200);

        assert_eq!(p.finish(300), (300, 300));
    }

    /// 每个文件的 Progress 只代表**它自己**，所以 total 只能用来**放大**
    /// 总量，绝不能替换——否则先传的大文件会被后传的小文件挤出分母。
    #[test]
    fn per_file_progress_total_cannot_shrink_the_total() {
        let mut p = TransferProgress::default();
        p.offer(5_000);
        // 第二个文件的 total 只有 10，若直接替换总量就会变成 10。
        let (d, t) = p.progress(10, 10);
        assert_eq!(t, 5_000, "per-file total 不得缩小整个传输的总量");
        assert_eq!(d, 10);
    }

    /// 从没收到过 offer（对端一个文件都没发）时不能除零——`finish`
    /// 退回到实际字节数。
    #[test]
    fn finish_without_any_offer_falls_back_to_actual() {
        let p = TransferProgress::default();
        assert_eq!(p.finish(0), (0, 0));
        assert_eq!(p.finish(42), (42, 42));
    }

    fn sep() -> char {
        std::path::MAIN_SEPARATOR
    }

    #[test]
    fn preserves_subpath() {
        let got = sanitize_relative_path("dir/sub/file.txt").unwrap();
        let want = format!("dir{}sub{}file.txt", sep(), sep());
        assert_eq!(got, want);
    }

    #[test]
    fn plain_file_stays_flat() {
        assert_eq!(sanitize_relative_path("a.bin").unwrap(), "a.bin");
    }

    #[test]
    fn drops_traversal_segments() {
        // ".." anywhere in the chain is dropped — never escapes local_dir.
        let got = sanitize_relative_path("a/../../etc/passwd").unwrap();
        let want = format!("a{}etc{}passwd", sep(), sep());
        assert_eq!(got, want);
        // Trailing dots/spaces (Windows quirk) must not smuggle a "..".
        assert_eq!(sanitize_relative_path("x/.. ./y").unwrap(), format!("x{}y", sep()));
    }

    #[test]
    fn drops_absolute_prefix_and_drive() {
        // Absolute path → leading separator yields an empty first segment
        // which is dropped; "C:" maps to "C_" (illegal-char rule).
        let got = sanitize_relative_path("/var/log/app.log").unwrap();
        let want = format!("var{}log{}app.log", sep(), sep());
        assert_eq!(got, want);
        let got = sanitize_relative_path("C:/evil").unwrap();
        assert_eq!(got, format!("C_{}evil", sep()));
    }

    #[test]
    fn nothing_usable_returns_none() {
        assert!(sanitize_relative_path("").is_none());
        assert!(sanitize_relative_path("/").is_none());
        assert!(sanitize_relative_path("..").is_none());
        assert!(sanitize_relative_path("./..\\").is_none());
    }

    /// 端口文件的删除必须是「比对后删」：只有仍写着我们判定为陈旧的那个端口
    /// 时才删。GUI 看门狗（或另一个 MCP 实例）刚重写的新端口文件必须毫发无损
    /// —— 否则一次误删就让活着的 GUI 彻底不可发现，之后每次调用都去拉起
    /// 一个注定被单实例拦下的第二实例，把 30 秒预算耗光。
    #[test]
    fn stale_port_file_removal_preserves_a_newer_publication() {
        use super::remove_port_file_if_stale;

        let dir = std::env::temp_dir().join("myshell_port_file_stale_test");
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("gui-ipc-port");

        // 文件现在写着 5555，而我们判定陈旧的是 1234 → 不得删除。
        std::fs::write(&f, "5555\ntok-new").unwrap();
        remove_port_file_if_stale(&f, Some(1234));
        assert!(
            f.exists(),
            "端口文件已被更新为新端口时，删除陈旧判定必须放行（不得删除活实例的发现文件）"
        );

        // 文件仍是我们判定陈旧的那个端口 → 应当删除。
        std::fs::write(&f, "1234\ntok-old").unwrap();
        remove_port_file_if_stale(&f, Some(1234));
        assert!(
            !f.exists(),
            "文件仍是判定为陈旧的那份时必须删除，否则 MCP 永远读不到新端口"
        );

        let _ = std::fs::remove_dir_all(&dir);
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
    let task_id = unique_task_id("zm-dl");
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
        log_err(&format!("zmodem_download [{}] failed: {}", task_id, msg));
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

    // hold_startup=false: headless ZMODEM task — a startup hold would swallow
    // early ZMODEM protocol bytes until a FrontendReady that never comes.
    let session_id = match ssh::connect(app, sink.clone(), config, false).await {
        Ok(id) => id,
        Err(e) => {
            fail(format!("SSH 连接失败: {}", e));
            return;
        }
    };

    // `-r` is lrzsz's `--resume` (continue an interrupted transfer). It is
    // NOT recursion: `sz` only ever opens regular files, and it does not
    // glob either — it relies on the shell having expanded the argument
    // before it runs. Since we shell_quote the path, a directory or a
    // wildcard reaches sz literally and it aborts with "Can't open any
    // requested files" (exit 128), which surfaces here as a ZABORT. This
    // path is single-file by construction; whole trees go through
    // `download_project` (tar over SFTP).
    let cmd = format!("sz -r {}\r", shell_quote(remote_path));
    if let Err(e) = ssh::send_input(app, &session_id, cmd.as_bytes()).await {
        let _ = ssh::disconnect(app, &session_id).await;
        fail(format!("发送 sz 命令失败: {}", e));
        return;
    }

    let mut saved: Vec<(String, String, u64)> = Vec::new();
    let mut terminal_noise = Vec::<u8>::new();
    let mut got_offer = false;
    // 见 TransferProgress 的文档：sz -r 会逐个文件发 Offer，每个文件的
    // Progress/FileComplete 只代表自己，必须自己累加。
    let mut prog = TransferProgress::default();

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
                    let (d, t) = prog.offer(size);
                    mark(TaskPhase::Transferring, d, t);
                    let local_name = sanitize_relative_path(&name).unwrap_or_else(|| {
                        unique_task_id("zmodem_download")
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
                    let local_name = sanitize_relative_path(&name).unwrap_or_default();
                    let local_path = join_local_path(local_dir, &local_name);
                    saved.push((name, local_path, bytes));
                    let (d, t) = prog.file_complete(bytes);
                    mark(TaskPhase::Transferring, d, t);
                }
                ZmodemEvent::Progress { transferred, total } => {
                    let (d, t) = prog.progress(transferred, total);
                    mark(TaskPhase::Transferring, d, t);
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
            let (done, full) = prog.finish(total);
            let _ = tasks.lock().map(|mut t| {
                if let Some(task) = t.get_mut(task_id) {
                    task.phase = TaskPhase::Done;
                    task.bytes_done = done;
                    // 完成后把 bytes_total 也钉成实际总量。少这一步，
                    // zmodem_status 会按 0/0 算出 0%，把一次**成功**的
                    // 下载报成"传输 0%"——这正是真机验证抓到的那个 bug。
                    task.bytes_total = full;
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

    // Validate the local path up front (fail fast). Files upload as-is;
    // directories are expanded recursively by the native uploader (ZFILE
    // offers carry relative paths, lrzsz rz recreates the tree remotely).
    let meta = std::fs::metadata(&local_path)
        .map_err(|e| format!("本地文件不存在或不可访问 [{}]: {}", local_path, e))?;
    if !meta.is_file() && !meta.is_dir() {
        return Err(format!("本地路径不是文件或目录: {}", local_path));
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
    let task_id = unique_task_id("zm-ul");
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
        log_err(&format!("zmodem_upload [{}] failed: {}", task_id, msg));
    };

    // The task is in Confirming. Pop the OS dialog here (blocks the background
    // task, NOT the tool call that already returned).
    let detail = format!(
        "ZMODEM 上传本地文件 [{}] ({} 字节) → {}",
        local_path, size, remote_dir
    );
    if !confirm_or_session_allowed("zmodem_upload（ZMODEM 上传）", &detail) {
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

    // hold_startup=false: headless ZMODEM task — a startup hold would swallow
    // early ZMODEM protocol bytes until a FrontendReady that never comes.
    let session_id = match ssh::connect(app, sink.clone(), config, false).await {
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

/// Process-wide monotonic counter, used to make task ids collision-proof.
static TASK_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Build a task id that CANNOT collide within this process.
///
/// The ids used to be `ssh-<unix_seconds>` / `zm-dl-<unix_seconds>`, and the
/// task table is keyed by that string. AI clients routinely fan out parallel
/// tool calls, so two tasks starting inside the same wall-clock second got the
/// SAME id: the second `insert` destroyed the first task's entry while both
/// background drivers kept writing phase/progress/result into one record. For
/// `ssh_run` the collision also clobbered `exec_controls`, so `ssh_cancel`
/// could only reach the newer task and the older remote command became
/// uncancellable — and the older task's cleanup then deleted the newer
/// task's control handle.
fn unique_task_id(prefix: &str) -> String {
    use std::sync::atomic::Ordering;
    let seq = TASK_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{seq}", chrono_like_ts())
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
        log_err(&format!("ssh_run [{}] failed: {}", task_id, msg));
        let mut tasks = tasks.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = tasks.get_mut(task_id) {
            t.phase = TaskPhase::Failed;
            t.error = Some(msg);
        }
        // Always release the cancel control plane so a subsequent
        // ssh_cancel on this task_id (or a stale one) doesn't try to
        // touch a dropped watch::Sender.
        let mut controls = controls.lock().unwrap_or_else(|e| e.into_inner());
        controls.remove(task_id);
    };

    // Phase 1: confirmation (if needed). Skipped for safe commands.
    if needs_confirm {
        let detail = format!(
            "ssh_run 在服务器 [{}] 上执行: {}{}",
            conn_name,
            command,
            danger_reasons_text(command)
        );
        if !confirm_or_session_allowed("ssh_run（后台执行）", &detail) {
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
        let mut tasks = tasks.lock().unwrap_or_else(|e| e.into_inner());
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
        let mut tasks = tasks.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = tasks.get_mut(task_id) {
            t.phase = TaskPhase::Transferring;
        }
    }

    let mut stdout: Vec<u8> = Vec::new();
    let mut stderr: Vec<u8> = Vec::new();
    let mut exit_code: Option<u32> = None;
    let mut truncated = false;
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
                        if data.len() > room {
                            truncated = true;
                        }
                    } else {
                        truncated = true;
                    }
                }
                Some(ChannelMsg::ExtendedData { ref data, ext: 1 }) => {
                    if stderr.len() < max {
                        let room = max - stderr.len();
                        stderr.extend_from_slice(&data[..data.len().min(room)]);
                        if data.len() > room {
                            truncated = true;
                        }
                    } else {
                        truncated = true;
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
        let mut tasks = tasks.lock().unwrap_or_else(|e| e.into_inner());
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
                    "stdout": strip_ansi(&String::from_utf8_lossy(&stdout)),
                    "stderr": strip_ansi(&String::from_utf8_lossy(&stderr)),
                }));
            } else if let Some(code) = exit_code {
                t.phase = TaskPhase::Done;
                t.result = Some(json!({
                    "exit_code": code,
                    "stdout": strip_ansi(&String::from_utf8_lossy(&stdout)),
                    "stderr": strip_ansi(&String::from_utf8_lossy(&stderr)),
                    "truncated": truncated,
                    "duration_ms": t.started_at.elapsed().as_millis() as u64,
                }));
            } else {
                // No ExitStatus: the channel ended without one — connection
                // dropped or the remote was signal-terminated. Reporting
                // phase=Done with exit_code 0 told the AI the long job
                // succeeded. `ssh_status` documents a Failed state for exactly
                // this ("the connection died") which the old code could never
                // actually produce.
                t.phase = TaskPhase::Failed;
                t.error = Some(
                    "远端未返回退出码：连接中断或进程被信号终止，任务结果未知（不能视为成功）"
                        .to_string(),
                );
                t.result = Some(json!({
                    "exit_code": serde_json::Value::Null,
                    "stdout": strip_ansi(&String::from_utf8_lossy(&stdout)),
                    "stderr": strip_ansi(&String::from_utf8_lossy(&stderr)),
                    "truncated": truncated,
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
        let mut controls = controls.lock().unwrap_or_else(|e| e.into_inner());
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

// ============ Remote exec helpers ============

/// Run `cmd` on a fresh channel over `handle` and collect stdout + exit status.
///
/// The project-transfer tools used to `exec`, `sleep(3s)`, then drop the
/// channel — so BOTH the exit status and the `UPLOAD_OK` / `TAR_OK` sentinel
/// the command itself printed were thrown away. Every failure mode (sudo
/// prompting for a password, a read-only filesystem, python3 missing, a tar
/// that simply took longer than 3 seconds) was therefore reported to the AI
/// as a success. This returns both so the caller can actually check.
///
/// A missing `ExitStatus` is an error, never `0`: it means the connection
/// dropped or the remote process was signal-terminated, which is not a
/// successful run.
async fn exec_collect(
    handle: &russh::client::Handle<ssh::SshClient>,
    cmd: &str,
    what: &str,
    timeout_secs: u64,
) -> Result<(u32, String), String> {
    use russh::ChannelMsg;
    let mut channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("{what}: 打开通道失败: {}", e))?;
    channel
        .exec(true, cmd)
        .await
        .map_err(|e| format!("{what}: 执行失败: {}", e))?;

    let collect = async {
        let mut out = String::new();
        let mut exit: Option<u32> = None;
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { ref data } => out.push_str(&String::from_utf8_lossy(data)),
                ChannelMsg::ExtendedData { ref data, ext: 1 } => {
                    out.push_str(&String::from_utf8_lossy(data))
                }
                ChannelMsg::ExitStatus { exit_status } => exit = Some(exit_status as u32),
                ChannelMsg::Eof | ChannelMsg::Close => break,
                _ => {}
            }
        }
        (out, exit)
    };

    let (out, exit) = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        collect,
    )
    .await
    .map_err(|_| format!("{what}: 远端命令超时（{timeout_secs}秒），已中止。"))?;

    let code = exit.ok_or_else(|| {
        format!("{what}: 远端未返回退出码（连接中断或进程被信号终止），结果未知。")
    })?;
    Ok((code, out))
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
    // 本地句柄也要显式冲刷并在失败时报错 —— 否则「已下载」可能在磁盘满 /
    // 权限问题时对一份残缺文件成立。
    lf.flush().await.map_err(|e| format!("本地落盘失败（磁盘空间?）: {}", e))?;
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
    // flush() 等待所有在途写确认。旧代码写的是 `let _ = rf.flush().await;` ——
    // 远端磁盘满 / 超配额时这个错误被吞掉，工具照样回「已上传」，AI 与用户都
    // 以为文件完整。落盘失败必须冒泡成全量失败。
    rf.flush()
        .await
        .map_err(|e| format!("远端落盘失败（磁盘空间或配额?）: {}", e))?;
    Ok(())
}

// ============ Logging to file (stdout is reserved for JSON-RPC only) ============
//
// Two files, both rotated daily by epoch-day (same day formula as the GUI's
// logger) and pruned on startup to a 7-day retention window:
//
//   <config_dir>/myshell/logs/mcp-<day>.log        — base (info/正常流程)
//   <config_dir>/myshell/logs/mcp-error-<day>.log  — errors only
//
// Error entries go to the error file ONLY (never duplicated into the base
// file): a support report can then be sent as just the error file, and the
// base file keeps a clean trace of what the session actually did.
//
// The directory is `dirs::config_dir()` — the same one the GUI uses — so on
// Linux the two processes no longer log into different roots (the previous
// `dirs::data_dir()` only coincided with it on Windows).
//
// Every write is best-effort: a failed log must never break the JSON-RPC
// session (a panic here kills the AI client's connection). The writer is kept
// open across calls (a day rollover reopens it) instead of the old
// open-append-close per line.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Retention window, mirroring the GUI's 7-day policy.
const LOG_RETENTION_SECS: u64 = 7 * 24 * 60 * 60;

/// Cap on a single log line. Tool-call args can embed multi-KB heredocs; with
/// no cap a full session produced a 3.1 MB file. Tail dropped, length reported
/// (git-style).
const LOG_LINE_MAX: usize = 4000;

/// Log directory resolved once: `<config_dir>/myshell/logs` — the same root
/// the GUI logger uses (the previous `dirs::data_dir()` only coincided with it
/// on Windows). `None` = logging disabled (dir uncreatable). Creation also
/// prunes files past the retention window, so a long-idle install cleans up on
/// its next start.
static LOG_DIR: LazyLock<Option<std::path::PathBuf>> = LazyLock::new(|| {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("myshell")
        .join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    prune_old_logs(&dir);
    Some(dir)
});

/// Best-effort deletion of `mcp*.log` files (base, error, and the pre-rotation
/// bare `mcp.log`) last modified more than the retention window ago.
fn prune_old_logs(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let cutoff = match std::time::SystemTime::now().checked_sub(std::time::Duration::from_secs(LOG_RETENTION_SECS)) {
        Some(c) => c,
        None => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("mcp") || !name.ends_with(".log") {
            continue;
        }
        if entry.metadata().and_then(|m| m.modified()).map(|mt| mt < cutoff).unwrap_or(false) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Epoch-day index (seconds / 86400) — the shared day formula, and the suffix
/// in the file names (`mcp-<day>.log` / `mcp-error-<day>.log`).
fn log_day() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86400)
        .unwrap_or(0)
}

/// Open (append/create) one of the two daily files. `error=false` → base file.
fn open_log(dir: &std::path::Path, day: u64, error: bool) -> Option<std::fs::File> {
    let name = if error {
        format!("mcp-error-{}.log", day)
    } else {
        format!("mcp-{}.log", day)
    };
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(name))
        .ok()
}

/// The two open handles for the current day. A day rollover drops and reopens
/// them. `disabled` latches after an open failure so a read-only disk doesn't
/// cost an open() syscall per log line.
#[derive(Default)]
struct LogFiles {
    day: u64,
    base: Option<std::fs::File>,
    error: Option<std::fs::File>,
    disabled: bool,
}

static LOG_FILES: LazyLock<Mutex<LogFiles>> = LazyLock::new(|| Mutex::new(LogFiles::default()));

/// Write one line, timestamped, to the daily base or error file under `dir`.
/// Errors go to the error file exclusively — a support report can then be just
/// that file, and the base file stays a clean trace of the session. Unwritable
/// log files are fatal to nothing: this is called from the JSON-RPC loop and a
/// panic here would take the client's connection down with it.
fn write_line(files: &mut LogFiles, dir: &std::path::Path, error: bool, msg: &str) {
    if files.disabled {
        return;
    }
    let day = log_day();
    if files.day != day {
        files.day = day;
        files.base = None;
        files.error = None;
    }
    let slot = if error { &mut files.error } else { &mut files.base };
    if slot.is_none() {
        match open_log(dir, day, error) {
            Some(f) => *slot = Some(f),
            None => {
                files.disabled = true;
                return;
            }
        }
    }
    let line = compose_line(msg);
    let Some(target) = slot.as_mut() else { return };
    let _ = target.write_all(line.as_bytes());
    let _ = target.write_all(b"\n");
}

fn log_to(error: bool, msg: &str) {
    let Some(dir) = LOG_DIR.as_ref() else { return };
    let mut files = LOG_FILES.lock().unwrap_or_else(|e| e.into_inner());
    write_line(&mut files, dir, error, msg);
}

/// `[HH:MM:SS] <msg>`, truncated on a UTF-8 boundary when over `LOG_LINE_MAX`.
fn compose_line(msg: &str) -> String {
    let head = chrono::Local::now().format("%H:%M:%S");
    if msg.len() <= LOG_LINE_MAX {
        return format!("[{}] {}", head, msg);
    }
    let mut cut = LOG_LINE_MAX;
    while cut > 0 && !msg.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("[{}] {}… [truncated, {} chars total]", head, &msg[..cut], msg.len())
}

/// Base log: normal lifecycle, tool calls, decisions.
fn log(msg: &str) {
    log_to(false, msg);
}

/// Error log: failures the user should be able to hand over for diagnosis.
fn log_err(msg: &str) {
    log_to(true, msg);
}

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
    // Force the log dir + retention prune now so startup is deterministic
    // (LazyLock would otherwise initialize on the first log call).
    LazyLock::force(&LOG_DIR);

    // Pick up the per-stream output cap from MCP_SSH_MAX_OUTPUT_BYTES. Env
    // var is read once at startup so the read path stays a cheap atomic
    // load. Clamped to [64 KiB, 1 GiB] — below 64 KiB is useless for any
    // real command, above 1 GiB would risk OOM on a chatty job. Invalid
    // values are silently ignored (the default sticks); we log the reason
    // so the operator knows.
    if let Ok(raw) = std::env::var("MCP_SSH_MAX_OUTPUT_BYTES") {
        match raw.trim().parse::<usize>() {
            Ok(n) if n >= 64 * 1024 && n <= 1024 * 1024 * 1024 => {
                SSH_MAX_OUTPUT_BYTES.store(n, Ordering::Relaxed);
                log(&format!("MCP_SSH_MAX_OUTPUT_BYTES = {} bytes", n));
            }
            Ok(_) => log_err(&format!(
                "MCP_SSH_MAX_OUTPUT_BYTES={} 越界，保持默认 4 MiB（合法范围 64 KiB ~ 1 GiB）",
                raw
            )),
            Err(e) => log_err(&format!(
                "MCP_SSH_MAX_OUTPUT_BYTES={:?} 解析失败，保持默认 4 MiB：{}",
                raw, e
            )),
        }
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
            log_err(&format!("数据库初始化失败: {}", e));
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

    let state = Arc::new(McpState {
        app,
        tasks: Arc::new(Mutex::new(std::collections::HashMap::new())),
        exec_controls: Arc::new(Mutex::new(std::collections::HashMap::new())),
    });
    log("MCP server ready — waiting for first message on stdin");

    // MCP stdio loop.
    //
    // stdin is read inline (blocking); each `tools/call` runs in its own task
    // so a slow command no longer deafens the server. Previously the loop
    // awaited every call, so while one `ssh_exec` ran (up to an hour), a
    // second request — including `ssh_status` / `ssh_cancel` — sat unread in
    // the pipe until it finished. Clients that issue parallel tool calls now
    // get real parallelism, and cancellation stays reachable.
    //
    // stdout is shared behind a Mutex: responses from concurrent tasks must
    // never interleave mid-line. We still flush inside write_message_raw —
    // buffering without flush is exactly what causes client-side initialize
    // timeouts.
    let stdin = std::io::stdin();
    let stdin_lock = stdin.lock();
    let mut reader = std::io::BufReader::new(stdin_lock);
    let writer = Arc::new(Mutex::new(std::io::BufWriter::new(std::io::stdout())));
    // In-flight tool calls: on EOF we let them deliver their responses instead
    // of exiting out from under a client that half-closed stdin.
    let in_flight = Arc::new(std::sync::atomic::AtomicUsize::new(0));

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
                write_out(&writer, &response);
                log(&format!("sent: initialize response id={}", id));
            }

            "notifications/initialized" => {
                log("recv: notifications/initialized (no response needed)");
            }

            "tools/list" => {
                let response = jsonrpc_response(&id, tool_definitions());
                write_out(&writer, &response);
                log(&format!("sent: tools/list response id={}", id));
            }

            "tools/call" => {
                let tool_name = msg["params"]["name"].as_str().unwrap_or("").to_string();
                let args = msg["params"]["arguments"].clone();
                log(&format!("tool call: {} args={}", tool_name, args));

                let state = Arc::clone(&state);
                let writer = Arc::clone(&writer);
                let in_flight = Arc::clone(&in_flight);
                in_flight.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let result = match call_tool(&state, &tool_name, &args).await {
                        Ok(v) => v,
                        Err(e) => {
                            // The AI sees the error inline; the error log keeps
                            // the record after the conversation is gone.
                            log_err(&format!("tool {} 失败: {}", tool_name, e));
                            tool_error(None, e)
                        }
                    };
                    write_out(&writer, &jsonrpc_response(&id, result));
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                });
            }

            "ping" => {
                let response = jsonrpc_response(&id, json!({}));
                write_out(&writer, &response);
                log(&format!("sent: ping response id={}", id));
            }

            _ => {
                // Unknown method — respond with MethodNotFound for requests (has id),
                // ignore unknown notifications (no id).
                if !id.is_null() {
                    let response = jsonrpc_error(&id, -32601, &format!("Method not found: {}", method));
                    write_out(&writer, &response);
                    log(&format!("sent: method-not-found id={} method={}", id, method));
                }
            }
        }
    }
    // stdin closed. Give in-flight calls a bounded grace period to deliver
    // their responses (a client that half-closes stdin but still waits for a
    // result would otherwise lose it), then exit. The cap keeps a hung remote
    // command from pinning this process forever after the client is gone.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while in_flight.load(Ordering::SeqCst) > 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    if in_flight.load(Ordering::SeqCst) > 0 {
        log_err("stdin 已关闭，仍有未完成的工具调用；超过 30 秒宽限期后退出");
    }
    log("stdin loop ended — exiting");
}

/// Serialize concurrent responses through one writer. Held only for the
/// duration of a single write+flush, so it never becomes a bottleneck.
fn write_out(writer: &Arc<Mutex<std::io::BufWriter<std::io::Stdout>>>, msg: &Value) {
    match writer.lock() {
        Ok(mut w) => write_message_raw(&mut *w, msg),
        Err(_) => {}
    }
}
