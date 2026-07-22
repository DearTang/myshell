// MyShell MCP Server — exposes SSH/SFTP operations as MCP tools for AI agents
// (Claude Desktop, Cursor, ZCode, etc.) via the Model Context Protocol.
//
// Transport: stdio (Content-Length framed JSON-RPC 2.0, same as LSP).
// Auth: reads MYSHELL_PASSPHRASE env var on startup to unlock the vault.
//
// Configuration example (Claude Desktop claude_desktop_config.json):
//   { "mcpServers": { "myshell": {
//       "command": "myshell-mcp",
//       "env": { "MYSHELL_PASSPHRASE": "your-master-password" }
//   }}}

use myshell_core::*;
use serde_json::{json, Value};
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
The connection details (host, port, credentials) are stored encrypted in MyShell — you never need and never will receive passwords. Pass the connection NAME, group-path, or host/IP — all three forms are accepted.\n\
\n\
DO NOT USE THESE TOOLS WHEN: the user wants to run a command locally on their own machine (use your built-in shell tool instead), or references a server they haven't saved in MyShell (suggest they add it in the MyShell GUI first).\n\
\n\
WORKFLOW:\n\
1. If the user gave an unambiguous name or IP you trust, skip to step 2. If unsure, call `list_connections` first to see what's saved (there may be group prefixes like '/production/prod-db').\n\
2. Pass the name / group-path / host to `ssh_exec` / `sftp_list` / etc. The tool auto-disambiguates by type: ssh_exec prefers ssh connections, sftp_* prefers sftp.\n\
3. For `ssh_exec`: each invocation is a fresh non-interactive session — no cwd/memory between calls. Use absolute paths or chain commands with && if state matters.\n\
\n\
SAFETY: ssh_exec uses a **configurable whitelist/blacklist** to decide which commands need human confirmation. Read-only commands (ps, ls, cat, grep, df, ...) run WITHOUT a dialog. Dangerous commands (rm, kill, sudo, shutdown, write-redirects, pipe-to-shell, ...) trigger a NATIVE OS confirmation dialog the USER must click — you cannot bypass it. Before calling a dangerous command, briefly tell the user a dialog is coming. If they click Cancel, the tool returns an error — surface it, don't retry silently. The sftp tools (sftp_upload/remove/rename) ALWAYS confirm regardless of rules.\n\
\n\
ENCOUNTERING ERRORS: if a tool returns 'Vault 未解锁' or '未找到保存的密码', the user hasn't synced their vault passphrase to the OS keyring via MyShell GUI (Settings → MCP 支持). Tell them this and stop.";

fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "list_connections",
                "description": "List all SSH/SFTP/FTP connections saved in this user's MyShell client.\n\nWHEN TO USE: Always call this FIRST when the user mentions a remote server by name, nickname, alias, OR IP address — even if you think you know the name. Call this when the user asks 'what servers do I have', 'show my connections', or wants to know if a specific host is already configured. Knowing the full list helps you pick the right `connection` value for other tools.\n\nWHEN NOT TO USE: Don't use this for connections the user is defining on the fly (e.g. 'ssh to user@1.2.3.4'). MyShell tools only work with pre-saved connections.\n\nOUTPUT: JSON array, each item has `name`, `host`, `port`, `username`, `conn_type` (ssh/sftp/ftp/local), `group_path`. \n\nIMPORTANT: other tools accept the connection `name`, the group-prefixed path ('/production/prod-db'), OR the bare `host`/IP interchangeably — so if the user said '135.32.64.30', you can pass '135.32.64.30' directly to ssh_exec without looking up the name first. But DO call this when (a) you're not sure the host is saved, (b) the user used an ambiguous nickname, or (c) a previous call returned an 'ambiguous match' error and you need to see the candidates. Passwords are never included, for security.",
                "inputSchema": { "type": "object", "properties": {}, "required": [] }
            },
            {
                "name": "ssh_exec",
                "description": "Run a shell command on a remote SSH server (non-interactive, one-shot). Returns stdout, stderr, and the process exit code.\n\nWHEN TO USE: The user wants to run a command on a remote server they've saved in MyShell — e.g. 'check disk usage on prod-db', 'restart nginx on web1', 'tail the log on api-server'. Prefer this over opening an interactive shell when the task is a single command with a defined end.\n\nWHEN NOT TO USE: \n- Interactive sessions (top, vim, less, mysql prompt) — this tool times out and won't stream output. Suggest the user run these in MyShell's GUI terminal instead.\n- Operations on a server NOT saved in MyShell — this tool can only reach pre-saved connections.\n- Operations the agent should do locally (read/write local files, run local commands) — use your own tools for those.\n\n⚠️ HUMAN CONFIRMATION REQUIRED: A native OS dialog pops up asking the user to approve. The command WILL NOT run until the user clicks 'Yes'. This is by design — AI-initiated remote execution is dangerous. Tell the user a confirmation dialog is coming.\n\nOUTPUT: JSON `{exit_code, stdout, stderr}`. `exit_code` is 0 on success. stdout/stderr are truncated at 4MB each. Commands that don't exit within `timeout` seconds return an error.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "connection": { "type": "string", "description": CONNECTION_PARAM_DESC },
                        "command": { "type": "string", "description": "Shell command to run. Executed via `bash -c` semantics on the remote. Avoid commands that read from stdin or wait for input." },
                        "timeout": { "type": "integer", "description": "Max seconds to wait before killing the command (default 30). Raise for long jobs like log scans, but note the human confirmation dialog is only shown once at the start.", "default": 30 }
                    },
                    "required": ["connection", "command"]
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
            }
        ]
    })
}

// ============ Shared state ============

struct McpState {
    app: AppState,
}

impl McpState {
    /// Resolve a user-provided connection reference into a `ConnectionConfig`.
    ///
    /// `query` may be:
    ///   - the exact connection `name` ("prod-db")
    ///   - the group-prefixed path ("/production/prod-db")
    ///   - a bare host/IP ("135.32.64.30") — resolved via the `host` field,
    ///     useful when the user said "ssh to <ip>" without remembering the
    ///     saved name.
    ///
    /// `expected_conn_type` is the caller's hint: "ssh" for ssh_exec,
    /// "sftp" for sftp_*, "ftp" for ftp, etc. It is used to disambiguate
    /// when multiple saved connections share the same name or host (common
    /// when the user saved both an SSH and an SFTP entry for the same box).
    ///
    /// Matching priority:
    ///   1. Exact `name` match, filtered by `expected_conn_type` if given
    ///   2. Exact `group_path/name` match, filtered by `expected_conn_type`
    ///   3. Exact `host` match, filtered by `expected_conn_type`
    ///   4. If still ambiguous (e.g. two ssh connections with the same host
    ///      in different groups), return an error listing the candidates so
    ///      the AI can ask the user to disambiguate.
    fn find_connection(&self, query: &str, expected_conn_type: Option<&str>) -> Result<ConnectionConfig, String> {
        let key = require_dek(&self.app)?;
        let db = self.app.db.lock().map_err(|e| e.to_string())?;
        let connections = db::get_all_connections(&db, &key).map_err(|e| e.to_string())?;

        // Helper: does this connection match the type filter?
        let type_ok = |c: &ConnectionConfig| match expected_conn_type {
            Some(t) => c.conn_type == t,
            None => true,
        };

        // Pass 1: exact name match (optionally type-filtered).
        let by_name: Vec<_> = connections.iter().filter(|c| c.name == query && type_ok(c)).collect();
        if by_name.len() == 1 {
            return Ok(by_name[0].clone());
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
            return Ok(by_path[0].clone());
        }
        if by_path.len() > 1 {
            return Err(ambiguous_error(query, expected_conn_type, &by_path));
        }

        // Pass 3: host match (this is the "user typed an IP" path).
        // A connection's `name` often equals its `host`, so pass 1 may have
        // matched already — but if not, try the host field directly. When
        // expected_conn_type is None we STILL try host match, but with higher
        // ambiguity risk (error will list candidates).
        let by_host: Vec<_> = connections.iter().filter(|c| c.host == query && type_ok(c)).collect();
        if by_host.len() == 1 {
            return Ok(by_host[0].clone());
        }
        if by_host.len() > 1 {
            return Err(ambiguous_error(query, expected_conn_type, &by_host));
        }

        // Nothing matched (or 0 matches after type filter). If the user had a
        // type filter, check whether ANY connection matched without the filter
        // — that lets us give a more helpful error ("found it, but it's an
        // SFTP connection, not SSH").
        if let Some(t) = expected_conn_type {
            let any_match: Vec<_> = connections
                .iter()
                .filter(|c| c.name == query || c.host == query
                    || format!("{}/{}", c.group_path.trim_end_matches('/'), c.name) == query)
                .collect();
            if !any_match.is_empty() {
                let types: Vec<&str> = any_match.iter().map(|c| c.conn_type.as_str()).collect();
                return Err(format!(
                    "找到了连接 '{}'，但类型是 {}，而当前操作需要 {} 类型。请确认连接配置或换一个连接。",
                    query,
                    types.join("/"),
                    t
                ));
            }
        }

        Err(format!("未找到连接: {}。调用 list_connections 查看所有可用连接。", query))
    }

    fn resolve_secrets(&self, config: &mut ConnectionConfig) -> Result<(), String> {
        if config.auth_method != "key" && config.password.is_none() {
            let key = require_dek(&self.app)?;
            config.password = secrets::get_password(&config.id, &key)?;
        }
        if config.auth_method == "password"
            && config.password.as_deref().map(str::is_empty).unwrap_or(true)
        {
            return Err("未找到保存的密码".to_string());
        }
        if config.proxy_type != "none" && config.proxy_password.is_none() {
            let key = require_dek(&self.app)?;
            config.proxy_password = secrets::get_proxy_password(&config.id, &key)?;
        }
        Ok(())
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
    match name {
        "list_connections" => {
            let key = require_dek(&state.app)?;
            let db = state.app.db.lock().map_err(|e| e.to_string())?;
            let connections = db::get_all_connections(&db, &key).map_err(|e| e.to_string())?;
            let items: Vec<Value> = connections
                .iter()
                .map(|c| {
                    json!({
                        "name": c.name,
                        "host": c.host,
                        "port": c.port,
                        "username": c.username,
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
            let timeout = args["timeout"].as_u64().unwrap_or(30);

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
            // user sees everything. Falls back to headless if the GUI isn't
            // reachable after an auto-start attempt.
            if rules.show_in_gui {
                match exec_in_gui_tab(conn_name, command, timeout, &state).await {
                    Ok(result_json) => return Ok(result_json),
                    Err(e) => {
                        // GUI not available or exec failed — fall through to
                        // headless mode so the tool still works.
                        log(&format!("show_in_gui 失败，回退到静默模式: {}", e));
                    }
                }
            }

            // ── Confirmation check (headless path) ──
            if command_rules::command_needs_confirmation(command, &rules) {
                let detail = format!("在服务器 [{}] 执行命令: {}", conn_name, command);
                if !confirm_dangerous_operation("ssh_exec（远程命令执行）", &detail) {
                    return Ok(json!({ "content": [{ "type": "text", "text": "❌ 用户取消了高危操作：ssh_exec" }], "isError": true }));
                }
            } else {
                log(&format!("ssh_exec 免确认（白名单/非危险）: {}", command));
            }

            let mut config = state.find_connection(conn_name, Some("ssh"))?;
            state.resolve_secrets(&mut config)?;

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
                const MAX: usize = 4 * 1024 * 1024;
                loop {
                    match channel.wait().await {
                        Some(ChannelMsg::Data { ref data }) => {
                            if stdout.len() < MAX {
                                let room = MAX - stdout.len();
                                stdout.extend_from_slice(&data[..data.len().min(room)]);
                            }
                        }
                        Some(ChannelMsg::ExtendedData { ref data, ext: 1 }) => {
                            if stderr.len() < MAX {
                                let room = MAX - stderr.len();
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

        "sftp_list" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let path = args["path"].as_str().unwrap_or("~");

            let mut config = state.find_connection(conn_name, Some("sftp"))?;
            state.resolve_secrets(&mut config)?;
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

            let mut config = state.find_connection(conn_name, Some("sftp"))?;
            state.resolve_secrets(&mut config)?;
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
                return Ok(json!({ "content": [{ "type": "text", "text": "❌ 用户取消了高危操作：sftp_upload" }], "isError": true }));
            }

            let mut config = state.find_connection(conn_name, Some("sftp"))?;
            state.resolve_secrets(&mut config)?;
            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;
            let sftp = open_sftp(&handle).await?;
            sftp_upload_file(&sftp, local, remote).await?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            Ok(json!({ "content": [{ "type": "text", "text": format!("已上传 {} → {}", local, remote) }] }))
        }

        "sftp_mkdir" => {
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let path = args["path"].as_str().ok_or("缺少 path 参数")?;

            let mut config = state.find_connection(conn_name, Some("sftp"))?;
            state.resolve_secrets(&mut config)?;
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
                return Ok(json!({ "content": [{ "type": "text", "text": "❌ 用户取消了高危操作：sftp_remove" }], "isError": true }));
            }

            let mut config = state.find_connection(conn_name, Some("sftp"))?;
            state.resolve_secrets(&mut config)?;
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
                return Ok(json!({ "content": [{ "type": "text", "text": "❌ 用户取消了高危操作：sftp_rename" }], "isError": true }));
            }

            let mut config = state.find_connection(conn_name, Some("sftp"))?;
            state.resolve_secrets(&mut config)?;
            let handle = ssh::dial_and_authenticate(&state.app, &config, false).await?;
            let sftp = open_sftp(&handle).await?;
            sftp.rename(old, new).await.map_err(|e| format!("重命名失败: {}", e))?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            Ok(json!({ "content": [{ "type": "text", "text": format!("已重命名 {} → {}", old, new) }] }))
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
            let mut config = match state.find_connection(conn_name, Some("ssh")) {
                Ok(c) => c,
                Err(e) if e.contains("未找到") => state.find_connection(conn_name, None)?,
                Err(e) => return Err(e),
            };
            state.resolve_secrets(&mut config)?;

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
            let config = state.find_connection(conn_name, None)?;
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
            // Resolve the connection first (confirms it exists + disambiguates).
            let conn_name = args["connection"].as_str().ok_or("缺少 connection 参数")?;
            let config = state.find_connection(conn_name, None)?;

            // tab_type: "auto" (default) | "terminal" | "sftp".
            let tab_type = match args["tab_type"].as_str().unwrap_or("auto") {
                "terminal" => "terminal",
                "sftp" => "sftp",
                _ => "auto",
            };

            // Discover the GUI's IPC port.
            let port = read_gui_ipc_port().ok_or_else(|| {
                "MyShell GUI 未运行（找不到 IPC 端口文件）。请先打开 MyShell 桌面应用，然后重试。如果只是想执行命令，可以改用 ssh_exec（无需 GUI）。".to_string()
            })?;

            // Send the open command over localhost TCP. The GUI focuses an
            // existing matching tab if one is open (focus_existing=true),
            // otherwise opens a new tab of the requested type.
            let result = send_gui_open_command(port, &config.id, tab_type, true).map_err(|e| {
                format!("无法与 MyShell GUI 通信（{}）。请确认 MyShell 桌面应用正在运行。如果只是想执行命令，可以改用 ssh_exec。", e)
            })?;

            if result {
                let kind = match tab_type {
                    "sftp" => "SFTP 文件浏览标签页",
                    "terminal" => "终端标签页",
                    _ => if config.conn_type == "sftp" { "SFTP 文件浏览标签页" } else { "终端标签页" },
                };
                Ok(json!({ "content": [{ "type": "text", "text": format!(
                    "✅ 已在 MyShell GUI 中打开连接 [{}]（{}@{}）的{}，窗口已聚焦到前台。若该连接已有打开的标签页，则直接切换过去（不重复打开）。用户现在可以直接交互操作。",
                    config.name, config.username, config.host, kind
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

/// Read the GUI's IPC port from the port-discovery file. Returns None if the
/// file is missing (GUI not running) or unparseable.
fn read_gui_ipc_port() -> Option<u16> {
    let mut path = dirs::config_dir()?;
    path.push("myshell");
    path.push("gui-ipc-port");
    let raw = std::fs::read_to_string(&path).ok()?;
    raw.trim().parse::<u16>().ok()
}

/// Send an "open_connection" command to the GUI over localhost TCP and wait
/// for the one-line JSON response. Returns true if the GUI acknowledged
/// success (`{"ok":true}`), false otherwise.
///
/// `tab_type` is "auto" | "terminal" | "sftp". `focus_existing` tells the GUI
/// to switch to an already-open matching tab instead of opening a duplicate.
fn send_gui_open_command(port: u16, connection_id: &str, tab_type: &str, focus_existing: bool) -> Result<bool, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("TCP 连接失败: {}", e))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("设置超时失败: {}", e))?;

    // Send the command as one NDJSON line.
    let cmd = serde_json::json!({
        "action": "open_connection",
        "connection_id": connection_id,
        "tab_type": tab_type,
        "focus_existing": focus_existing,
    });
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

// ============ GUI exec_in_tab (show_in_gui mode) ============

/// Ensure the MyShell GUI is running. If the IPC port file exists, assume it's
/// already up. Otherwise, spawn `myshell.exe` from the same directory as this
/// MCP binary and poll for the port file to appear (up to 30s).
fn ensure_gui_running() -> Result<u16, String> {
    // Already running?
    if let Some(port) = read_gui_ipc_port() {
        return Ok(port);
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
        if let Some(port) = read_gui_ipc_port() {
            log(&format!("GUI 就绪，IPC port = {}", port));
            return Ok(port);
        }
    }
    Err("GUI 启动超时（30 秒内未就绪）。请手动打开 MyShell 后重试。".to_string())
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
    // Resolve the connection to get its id (needed by the GUI to find/open tab).
    let config = state.find_connection(conn_name, Some("ssh"))?;

    // Ensure the GUI is running (auto-start if needed).
    let port = ensure_gui_running()?;

    // Send the exec_in_tab command and read the response.
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("TCP 连接 GUI 失败: {e}"))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(timeout + 15)))
        .map_err(|e| format!("设置超时失败: {e}"))?;

    let cmd = serde_json::json!({
        "action": "exec_in_tab",
        "connection_id": config.id,
        "command": command,
        "timeout": timeout,
    });
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

// ============ Vault unlock (same logic as CLI) ============

/// Unlock vault and return just the DEK (no AppState needed).
/// Used by the background unlock thread so the main async task can start
/// the JSON-RPC loop immediately without waiting for PBKDF2 (600k iterations,
/// ~1s on most CPUs) to finish.
fn try_unlock_dek(passphrase: &str) -> Result<[u8; 32], String> {
    let mut lockout = vault::LockoutState::load();
    if let Some(remaining) = lockout.check_lockout() {
        return Err(format!("锁定中，请等待 {} 秒", remaining));
    }
    let salt = vault::read_salt().ok_or("Vault 未初始化")?;
    let verifier = vault::read_verifier().ok_or("Vault 未初始化")?;
    let encrypted_dek_opt = vault::read_encrypted_dek();
    let (iterations, _) = match vault::read_kdf_meta() {
        Some(meta) => (meta.iterations, true),
        None => (crypto::LEGACY_PBKDF2_ITERATIONS, false),
    };
    let master_key = crypto::derive_master_key_with_iterations(passphrase, &salt, iterations);
    if !crypto::check_verifier(&master_key, &verifier) {
        lockout.record_failure()?;
        return Err("密码错误".to_string());
    }
    let dek: [u8; 32] = match encrypted_dek_opt {
        Some(blob) => {
            let bytes = crypto::decrypt_with_key(&master_key, &blob)?;
            bytes.as_slice().try_into().map_err(|_| "DEK 长度错误")?
        }
        None => master_key,
    };
    lockout.record_success();
    Ok(dek)
}

// ============ Logging to file (stdout is reserved for JSON-RPC only) ============

use std::sync::OnceLock;

static LOG_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();

fn log_init() {
    if let Some(dir) = dirs::data_dir() {
        let log_dir = dir.join("myshell").join("logs");
        let _ = std::fs::create_dir_all(&log_dir);
        let path = log_dir.join("mcp.log");
        let _ = LOG_PATH.set(path);
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
        zmodem_files: Mutex::new(std::collections::HashMap::new()),
        dek: Arc::new(Mutex::new(None)),
    };

    log("AppState initialized");

    // Unlock vault from OS keyring (DPAPI-encrypted, no plaintext in config).
    // Done in a non-blocking way — vault unlock happens AFTER we start the
    // JSON-RPC loop, so the server can respond to initialize/tools/list even
    // before unlock completes. Encrypted-data tools will return an error if
    // called before unlock finishes.
    if vault::is_initialized() {
        // Spawn unlock in background — don't block server startup
        let dek_clone = Arc::clone(&app.dek);
        std::thread::spawn(move || {
            match secrets::get_mcp_passphrase() {
                Ok(Some(passphrase)) => {
                    if passphrase.is_empty() {
                        log("警告: keyring 中密码为空");
                    } else {
                        // Re-create a minimal AppState reference for unlock
                        // We can't move the whole state, so we do unlock inline
                        // using just the dek slot.
                        match try_unlock_dek(&passphrase) {
                            Ok(dek) => {
                                if let Ok(mut slot) = dek_clone.lock() {
                                    *slot = Some(dek);
                                }
                                log("Vault 解锁成功（后台）");
                            }
                            Err(e) => log(&format!("Vault 解锁失败: {}", e)),
                        }
                    }
                }
                Ok(None) => log("keyring 中未存储 vault 密码"),
                Err(e) => log(&format!("读取 keyring 失败: {}", e)),
            }
        });
    } else {
        log("Vault 未初始化，跳过解锁");
    }

    let state = McpState { app };
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
