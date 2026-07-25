// MyShell MCP Server — exposes SSH/SFTP operations as MCP tools for AI agents
// (Claude Desktop, Cursor, ZCode, etc.) via the Model Context Protocol.
//
// Transport: stdio (Content-Length framed JSON-RPC 2.0, same as LSP).
// Auth: NONE — the MCP server does NOT hold the vault passphrase or DEK.
// All credential access is delegated to the GUI: ssh_exec runs in a GUI
// terminal tab, SFTP tools ask the GUI to decrypt connection credentials
// via IPC. The GUI must be running and the vault must be unlocked.
//
// Configuration example (Claude Desktop claude_desktop_config.json):
//   { "mcpServers": { "myshell": {
//       "command": "myshell-mcp"
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
The connection details (host, port, credentials) are stored encrypted in MyShell — you never need and never will receive passwords. Pass the connection NAME or group-path. IP-based lookup requires the GUI to be running and the vault unlocked (IPs are encrypted).\n\
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
ENCOUNTERING ERRORS: if a tool returns '保险库未解锁' or 'MyShell GUI 未运行', the user needs to open the MyShell desktop app and enter their master password to unlock the vault. Tell them this and stop — the MCP server cannot access any server without the GUI running and unlocked.";

fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "list_connections",
                "description": "List all SSH/SFTP/FTP connections saved in this user's MyShell client.\n\nWHEN TO USE: Always call this FIRST when the user mentions a remote server by name, nickname, alias, OR IP address — even if you think you know the name. Call this when the user asks 'what servers do I have', 'show my connections', or wants to know if a specific host is already configured. Knowing the full list helps you pick the right `connection` value for other tools.\n\nWHEN NOT TO USE: Don't use this for connections the user is defining on the fly (e.g. 'ssh to user@1.2.3.4'). MyShell tools only work with pre-saved connections.\n\nOUTPUT: JSON array, each item has `name`, `conn_type` (ssh/sftp/ftp/local), `group_path`. For security, host/port/username are NOT included - they are encrypted and only accessible when the GUI vault is unlocked.\n\nIMPORTANT: other tools accept the connection `name` or the group-prefixed path ('/production/prod-db'). IP-based lookup requires the GUI to be running and the vault unlocked (IPs are encrypted). Passwords are never included, for security.",
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
            }
        ]
    })
}

// ============ Shared state ============

struct McpState {
    app: AppState,
}

impl McpState {
    /// Plaintext-only connection lookup: resolves a name/group-path to a
    /// connection id WITHOUT needing the DEK. Used by the show_in_gui path
    /// (ssh_exec, open_in_gui) where the actual credential use happens inside
    /// the GUI process. Host-IP matching is NOT possible here (host is
    /// encrypted) — only name and group-path matching.
    fn find_connection_id(&self, query: &str, expected_conn_type: Option<&str>) -> Result<String, String> {
        let db = self.app.db.lock().map_err(|e| e.to_string())?;
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
    match name {
        "list_connections" => {
            // SECURITY: use plaintext-only lookup - do NOT decrypt host/username.
            // The MCP server must not expose server IPs or credentials to the AI
            // without the GUI being unlocked. Only name/conn_type/group_path
            // (non-sensitive plaintext columns) are returned.
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
                return Ok(json!({ "content": [{ "type": "text", "text": "❌ 用户取消了高危操作：sftp_upload" }], "isError": true }));
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
                return Ok(json!({ "content": [{ "type": "text", "text": "❌ 用户取消了高危操作：sftp_remove" }], "isError": true }));
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
                return Ok(json!({ "content": [{ "type": "text", "text": "❌ 用户取消了高危操作：sftp_rename" }], "isError": true }));
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
                return Ok(json!({ "content": [{ "type": "text", "text": "❌ 用户取消了高危操作：upload_project" }], "isError": true }));
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

            // 4. 在服务器上：创建目标目录 + sudo 移动 + 解压 + 清理
            let ssh_config2 = state.resolve_via_gui(conn_name, Some("ssh"))?;
            let ssh_handle2 = ssh::dial_and_authenticate(&state.app, &ssh_config2, false).await?;
            {
                let channel = ssh_handle2.channel_open_session().await.map_err(|e| format!("打开通道失败: {}", e))?;
                let extract_cmd = format!(
                    "sudo mkdir -p '{}' && sudo mv '{}' '{}' && cd '{}' && sudo tar -xzf '{}' && sudo rm -f '{}' && echo 'UPLOAD_OK'",
                    remote_target, remote_tar_path, remote_target, remote_target, remote_tar_name, remote_tar_name
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
                return Ok(json!({ "content": [{ "type": "text", "text": "❌ 用户取消了高危操作：download_project" }], "isError": true }));
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

            // 2. 在服务器上打包到临时文件（用 Python tarfile 处理中文文件名）
            let tmp_tar = format!("/tmp/_dl_project_{}.tar.gz", dir_name);
            let tar_cmd = format!(
                "python3 -c \"\
import tarfile, os, sys
exclude = {{'.venv', 'venv', '__pycache__', '.git', 'node_modules', 'dist', 'build', 'target'}}
with tarfile.open('{}', 'w:gz') as tf:
    for root, dirs, files in os.walk('{}'):
        dirs[:] = [d for d in dirs if d not in exclude and not d.startswith('.')]
        for f in files:
            if f.endswith('.pyc'): continue
            full = os.path.join(root, f)
            arcname = os.path.relpath(full, '{}')
            tf.add(full, arcname=arcname)
print('TAR_OK')
\" 2>&1",
                tmp_tar, remote_dir, remote_dir
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

            // 4. 清理服务器上的临时文件
            let cleanup_config = state.resolve_via_gui(conn_name, Some("ssh"))?;
            let cleanup_handle = ssh::dial_and_authenticate(&state.app, &cleanup_config, false).await?;
            {
                let channel = cleanup_handle.channel_open_session().await.map_err(|e| format!("打开通道失败: {}", e))?;
                channel.exec(true, format!("sudo rm -f '{}'", tmp_tar)).await.ok();
            }
            let _ = cleanup_handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            // 5. 在本地解压
            let local_path = std::path::Path::new(local_dir);
            std::fs::create_dir_all(local_path).map_err(|e| format!("创建本地目录失败: {}", e))?;
            let extract_output = std::process::Command::new("tar")
                .args(["-xzf", local_tar_str, "-C", local_dir])
                .output()
                .map_err(|e| format!("解压失败: {}（确保系统有 tar 命令）", e))?;

            // 清理本地临时文件
            std::fs::remove_file(&local_tar).ok();

            if !extract_output.status.success() {
                let stderr = String::from_utf8_lossy(&extract_output.stderr);
                return Err(format!("解压失败: {}", stderr));
            }

            let tar_size = std::fs::metadata(local_tar_str).map(|m| m.len()).unwrap_or(0);
            Ok(json!({ "content": [{ "type": "text", "text": format!("✅ 项目下载成功！\n远程: {}\n本地: {}\n大小: {} bytes (tar.gz)", remote_dir, local_dir, tar_size) }] }))
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
            // Plaintext lookup only — screenshot tool doesn't need credentials.
            let db = state.app.db.lock().map_err(|e| e.to_string())?;
            let conns = db::get_all_connections_plaintext(&db).map_err(|e| e.to_string())?;
            drop(db);
            let config = conns.into_iter().find(|c| c.name == conn_name
                || format!("{}/{}", c.group_path.trim_end_matches('/'), c.name) == conn_name)
                .ok_or_else(|| format!("未找到连接: {}", conn_name))?;
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

            // Discover the GUI's IPC port.
            let port = read_gui_ipc_port().ok_or_else(|| {
                "MyShell GUI 未运行（找不到 IPC 端口文件）。请先打开 MyShell 桌面应用，然后重试。".to_string()
            })?;

            // Send the open command over localhost TCP. The GUI focuses an
            // existing matching tab if one is open (focus_existing=true),
            // otherwise opens a new tab of the requested type.
            let result = send_gui_open_command(port, &conn_id, tab_type, true).map_err(|e| {
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

/// Ask the GUI to decrypt a connection's credentials and return the full
/// decrypted ConnectionConfig. The MCP server no longer holds the DEK or a
/// stored passphrase — this is the ONLY way to obtain credentials for
/// headless SFTP operations. Requires the GUI to be running and the vault
/// to be unlocked (user must have entered the master password in the GUI).
fn get_config_from_gui(conn_id: &str) -> Result<ConnectionConfig, String> {
    let port = read_gui_ipc_port().ok_or_else(|| {
        "MyShell GUI 未运行。请先打开 MyShell 桌面应用并输入主密码解锁保险库，然后重试。".to_string()
    })?;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("连接 GUI 失败: {e}"))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| format!("设置超时失败: {e}"))?;
    let cmd = serde_json::json!({
        "action": "get_connection_secrets",
        "connection_id": conn_id,
    });
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
    // Resolve the connection id via plaintext lookup — NO DEK needed.
    // The actual SSH connection + credential use happens inside the GUI
    // process (the user has unlocked the vault there).
    let conn_id = state.find_connection_id(conn_name, Some("ssh"))?;

    // Ensure the GUI is running (auto-start if needed).
    let port = ensure_gui_running()?;

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

    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("TCP 连接 GUI 失败: {e}"))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(timeout + 15)))
        .map_err(|e| format!("设置超时失败: {e}"))?;

    let cmd = serde_json::json!({
        "action": "exec_in_tab",
        "connection_id": conn_id,
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

    // SECURITY: The MCP server no longer reads the vault passphrase from the
    // OS keyring or auto-unlocks the vault. Instead, all credential access is
    // delegated to the GUI: ssh_exec runs in a GUI terminal tab (the user has
    // unlocked the vault there), and SFTP tools ask the GUI to decrypt the
    // connection config via IPC (get_connection_secrets). The DEK here stays
    // None permanently — the MCP server cannot access any server on its own.

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
