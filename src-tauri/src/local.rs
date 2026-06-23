//! Local terminal sessions.
//!
//! Spawns a shell under a PTY (ConPTY on Windows, openpty on Unix) and bridges
//! its stdin/stdout to the frontend exactly like an SSH session: incoming PTY
//! bytes are emitted as `ssh_output`, and shell exit fires `ssh_closed`. The
//! frontend's `TerminalPanel` reuses the same event subscriptions it uses for
//! SSH, so a local tab is indistinguishable from an SSH tab at the renderer.
//!
//! Why two tasks instead of SSH's single `select!` loop: portable-pty's reader
//! is a blocking `std::io::Read`, which would pin a whole thread if awaited.
//! So the reader runs on a `spawn_blocking` thread, and writes/resizes run on
//! an async task that owns the master + writer + child handle.
//!
//! Encoding note: bytes are forwarded verbatim (no re-encoding) — same as the
//! SSH path. On Windows, output encoding follows the shell's console codepage:
//! PowerShell 7 (pwsh) emits UTF-8, Windows PowerShell 5.1 may emit the system
//! ANSI codepage (GBK on zh-CN) and render mojibake in xterm. Mitigation is a
//! follow-up (inject `chcp 65001` / set `[Console]::OutputEncoding`, or decode
//! in the reader via encoding_rs). v1 keeps it simple and consistent with SSH.

use crate::{AppState, ConnectionConfig};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use tauri::{Emitter, State, WebviewWindow};
use tauri::async_runtime;
use tokio::sync::mpsc;

/// Commands the frontend pushes into a live local session. A trimmed mirror
/// of `ssh::SessionCommand` — local terminals have no ZMODEM.
#[derive(Debug)]
pub enum LocalCommand {
    Input(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Disconnect,
}

/// A live local PTY session. Owns the command channel the IPC commands push
/// into; the reader thread + writer task are spawned in [`connect`].
pub struct LocalSession {
    pub command_tx: mpsc::UnboundedSender<LocalCommand>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalOutputPayload {
    session_id: String,
    data: Vec<u8>,
}

/// Per-read chunk size forwarded to the frontend. Bounded so a chatty command
/// (`cat /dev/urandom`, a tight loop) can't push unbounded bytes through IPC
/// in one go — the SSH side applies the same philosophy.
const READ_CHUNK: usize = 8192;

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Returns a UTF-8 setup sequence to feed the shell at startup, for shells
/// that otherwise emit the system ANSI console codepage (e.g. GBK on zh-CN
/// Windows) and would render as mojibake in xterm. Returns `None` for shells
/// that are already UTF-8 (pwsh 7, bash, zsh, …) so we don't clutter them.
///
/// Detection is by the executable's file stem, so it works for both bare
/// names (`powershell`) and full paths (`C:\...\powershell.exe`).
fn shell_utf8_prelude(shell_path: &str) -> Option<Vec<u8>> {
    let stem = Path::new(shell_path)
        .file_stem()?
        .to_string_lossy()
        .to_lowercase();
    match stem.as_str() {
        // `@` suppresses echo of the line; `>nul` suppresses the codepage
        // report. All subsequent command output is then UTF-8.
        "cmd" => Some(b"@chcp 65001>nul\r".to_vec()),
        // Windows PowerShell 5.1: force the console I/O encoding to UTF-8 and
        // switch the codepage. pwsh (7) is already UTF-8 and is handled by the
        // `_` arm below.
        "powershell" => Some(
            // concat! joins the pieces at compile time (adjacent b"..." byte
            // strings do NOT auto-concatenate in Rust, unlike "..." str lit).
            concat!(
                "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8;",
                "[Console]::InputEncoding=[System.Text.Encoding]::UTF8;",
                "chcp 65001 > $null\r",
            )
            .as_bytes()
            .to_vec(),
        ),
        _ => None,
    }
}

pub async fn connect(
    state: State<'_, AppState>,
    window: WebviewWindow,
    config: ConnectionConfig,
    cols: u16,
    rows: u16,
) -> Result<String, String> {
    let shell_path = config
        .shell_path
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "未配置启动 shell 路径".to_string())?;

    let session_id = uuid::Uuid::new_v4().to_string();
    let sid = session_id.clone();

    let pair = native_pty_system()
        .openpty(pty_size(cols, rows))
        .map_err(|e| format!("打开 PTY 失败: {}", e))?;

    // Clone the reader before moving the master into the writer task.
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("克隆 PTY reader 失败: {}", e))?;

    let mut cmd = CommandBuilder::new(&shell_path);
    if let Some(args) = config.shell_args.as_ref() {
        // Whitespace-split args. Adequate for v1 — `-d Ubuntu`, `--login -i`.
        // A spaced argument would need a richer parser; acceptable limitation.
        for arg in args.split_whitespace() {
            cmd.arg(arg);
        }
    }
    // Advertise a capable terminal so prompt engines (Oh My Posh / Starship)
    // and color-aware tools render their full styling. The GUI process
    // inherits no TERM, which can make them dumb-down to a plain prompt or
    // drop colors. env() only overrides these keys — the rest of the parent
    // environment (PATH, profile, etc.) is still inherited below.
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "MyShell");
    // Inherit the parent environment so PATH / user profile resolve like a
    // shell the user opened themselves.
    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("启动 shell 失败: {}", e))?;

    // Drop our slave handle: the child keeps its end alive, and holding our
    // copy would prevent the master reader from observing EOF on shell exit.
    drop(pair.slave);

    let (command_tx, command_rx) = mpsc::unbounded_channel::<LocalCommand>();
    // Second sender so the reader thread can tell the writer task to stop once
    // the shell exits.
    let reader_tx = command_tx.clone();
    {
        let mut sessions = state.local_sessions.lock().map_err(|e| e.to_string())?;
        sessions.insert(sid.clone(), LocalSession { command_tx });
    }

    // ---- Reader thread (blocking) ----
    let reader_window = window.clone();
    let reader_sid = sid.clone();
    let sessions_arc = Arc::clone(&state.local_sessions);
    async_runtime::spawn_blocking(move || {
        let mut reader = reader;
        let mut buf = vec![0u8; READ_CHUNK];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF — shell exited
                Ok(n) => {
                    let payload = LocalOutputPayload {
                        session_id: reader_sid.clone(),
                        data: buf[..n].to_vec(),
                    };
                    if let Err(e) = reader_window.emit("ssh_output", payload) {
                        eprintln!("[local:{}] emit ssh_output FAILED: {}", reader_sid, e);
                    }
                }
                Err(e) => {
                    eprintln!("[local:{}] read error: {}", reader_sid, e);
                    break;
                }
            }
        }
        // Shell is gone: notify the frontend, stop the writer task, then drop
        // ourselves from the session map.
        let _ = reader_window.emit("ssh_closed", &reader_sid);
        let _ = reader_tx.send(LocalCommand::Disconnect);
        if let Ok(mut map) = sessions_arc.lock() {
            map.remove(&reader_sid);
        }
    });

    // ---- Writer task (owns master + writer + child) ----
    let writer_sid = sid.clone();
    let init_command = config.init_command.clone();
    // Per-shell UTF-8 setup (cmd / Windows PowerShell 5.1) fed as the very
    // first input so those shells emit UTF-8 instead of the system ANSI
    // codepage (GBK on zh-CN) which renders as mojibake in xterm. None for
    // pwsh / *nix shells that are already UTF-8.
    let utf8_prelude = shell_utf8_prelude(&shell_path);
    let master = pair.master;
    async_runtime::spawn(async move {
        let mut writer = match master.take_writer() {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[local:{}] take_writer failed: {}", writer_sid, e);
                return;
            }
        };
        let mut command_rx = command_rx;
        let mut child = child;

        // Force UTF-8 for shells that otherwise emit the system ANSI codepage.
        // Sent before the user's init command so any output that follows is
        // already UTF-8.
        if let Some(prelude) = utf8_prelude.as_ref() {
            if let Err(e) = writer.write_all(prelude) {
                eprintln!("[local:{}] utf8 prelude write failed: {}", writer_sid, e);
            }
            let _ = writer.flush();
        }

        // Inject the optional init command (e.g. "claude") right after the
        // writer is ready. The PTY stdin buffers it until the shell finishes
        // starting up, then the shell reads + echoes + executes it. We use
        // CR (\r) to trigger execution — same as how a user's Enter key is
        // forwarded in onData.
        //
        // NOTE: sent immediately at startup (NOT deferred to the first
        // resize). An earlier revision deferred it so a TUI like claude would
        // read the real cols instead of the PTY's startup 80×24 — but that
        // introduces a visible downside (the shell prompt flashes before the
        // TUI takes over), and it turned out NOT to fix the originally-
        // reported "shift left" (that's the ConPTY Chinese-IME upstream bug,
        // see progress.md 阶段23 — not fixable here). Reverted per user call;
        // the cols-layout benefit wasn't worth the prompt flash. Don't re-add
        // the deferral without re-evaluating that trade-off.
        if let Some(init) = init_command
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            let mut bytes = Vec::with_capacity(init.len() + 1);
            bytes.extend_from_slice(init.as_bytes());
            bytes.push(b'\r');
            if let Err(e) = writer.write_all(&bytes) {
                eprintln!("[local:{}] init_command write failed: {}", writer_sid, e);
            }
            let _ = writer.flush();
        }

        while let Some(cmd) = command_rx.recv().await {
            match cmd {
                LocalCommand::Input(data) => {
                    if let Err(e) = writer.write_all(&data) {
                        eprintln!("[local:{}] write failed: {}", writer_sid, e);
                        break;
                    }
                    let _ = writer.flush();
                }
                LocalCommand::Resize { cols, rows } => {
                    if let Err(e) = master.resize(pty_size(cols, rows)) {
                        eprintln!("[local:{}] resize failed: {}", writer_sid, e);
                    }
                }
                LocalCommand::Disconnect => {
                    // User closed the tab — kill the shell so the reader sees
                    // EOF promptly instead of lingering until natural exit.
                    let _ = child.kill();
                    break;
                }
            }
        }
    });

    log::info!("[local:{}] spawned shell {:?}", sid, shell_path);
    Ok(sid)
}

pub async fn send_input(
    state: &State<'_, AppState>,
    session_id: &str,
    data: &[u8],
) -> Result<(), String> {
    let sessions = state.local_sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| "Session not found".to_string())?;
    session
        .command_tx
        .send(LocalCommand::Input(data.to_vec()))
        .map_err(|_| "Session command channel closed".to_string())?;
    Ok(())
}

pub async fn resize_terminal(
    state: &State<'_, AppState>,
    session_id: &str,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let sessions = state.local_sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| "Session not found".to_string())?;
    session
        .command_tx
        .send(LocalCommand::Resize { cols, rows })
        .map_err(|_| "Session command channel closed".to_string())?;
    Ok(())
}

pub async fn disconnect(state: &State<'_, AppState>, session_id: &str) -> Result<(), String> {
    // Signal the writer task to kill the child + exit. The reader thread
    // removes the session from the map once it observes EOF.
    let sender = {
        let sessions = state.local_sessions.lock().map_err(|e| e.to_string())?;
        sessions.get(session_id).map(|s| s.command_tx.clone())
    };
    if let Some(tx) = sender {
        let _ = tx.send(LocalCommand::Disconnect);
    }
    Ok(())
}

/// Probe a local-shell config by spawning it under a PTY and killing it
/// immediately. Catches bad/missing executable paths and spawn-permission
/// errors without opening a tab or registering a session. The spawn itself is
/// the success signal — we don't wait for output (a shell that spawns but
/// hangs on profile load still proves the path is valid and executable, which
/// is what "测试" is really validating here). Used by `test_connection`.
pub async fn test_connection(config: &ConnectionConfig) -> Result<String, String> {
    let shell_path = config
        .shell_path
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "未配置启动 shell 路径".to_string())?;

    let started = std::time::Instant::now();
    let cfg = config.clone();
    let spawn = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let pair = native_pty_system()
            .openpty(pty_size(80, 24))
            .map_err(|e| format!("打开 PTY 失败: {}", e))?;
        let mut cmd = CommandBuilder::new(&shell_path);
        if let Some(args) = cfg.shell_args.as_ref() {
            for arg in args.split_whitespace() {
                cmd.arg(arg);
            }
        }
        cmd.env("TERM", "xterm-256color");
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("启动 shell 失败: {}（检查路径是否正确）", e))?;
        // Drop our slave handle (same reason as `connect`) then reap the child
        // so the test never leaves a lingering process.
        drop(pair.slave);
        let _ = child.kill();
        let _ = child.wait();
        Ok(())
    })
    .await
    .map_err(|e| format!("测试线程失败: {}", e))?;
    spawn?;
    let ms = started.elapsed().as_millis();
    Ok(format!("连接成功（{} ms，shell 可启动）", ms))
}