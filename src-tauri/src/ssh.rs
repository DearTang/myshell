use crate::{AppState, ConnectionConfig};
use crate::proxy;
use russh::client::{self, Handle, Msg};
use russh::{Channel, ChannelMsg};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, WebviewWindow};
use tauri::async_runtime;
use tokio::sync::mpsc;
use tauri::State;

#[derive(Debug)]
pub enum SessionCommand {
    Input(Vec<u8>),
    Resize { cols: u32, rows: u32 },
    /// ZMODEM bytes flowing from the frontend (zmodem.js socket.send) back to the
    /// SSH channel. Kept on the same mpsc as Input so the biased select! keeps
    /// protocol responses high priority.
    ZmodemBytes(Vec<u8>),
    /// User clicked cancel — send 8× CAN(0x18) to abort the in-flight transfer.
    ZmodemAbort,
    Disconnect,
}

pub struct SshSession {
    pub handle: Arc<Handle<SshClient>>,
    pub command_tx: mpsc::UnboundedSender<SessionCommand>,
}

/// Carries the DB handle so `check_server_key` can look up known_hosts.
/// Per-host: instantiated once per `connect()` call.
pub struct SshClient {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub host: String,
    pub port: u16,
}

impl client::Handler for SshClient {
    type Error = russh::Error;

    /// Validate the server's public key against the local known_hosts table.
    /// First connection to a (host, port): persist the fingerprint, accept.
    /// Subsequent connections: reject if the fingerprint changed (MITM
    /// defense). The key is scoped to the port so the same hostname on two
    /// different ports (e.g. 22 internal + 2222 jump host) doesn't silently
    /// overwrite each other's trust anchor.
    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key
            .fingerprint(Default::default())
            .to_string();
        let key_type = server_public_key.algorithm().to_string();

        let accepted = {
            let db = match self.db.lock() {
                Ok(g) => g,
                Err(_) => {
                    // Mutex poisoned = a previous command panicked mid-DB-op.
                    // Fail closed — accepting here would mean trusting a host
                    // without ever checking the fingerprint store.
                    eprintln!(
                        "[ssh] known_hosts mutex poisoned; rejecting key for {}:{}",
                        self.host, self.port
                    );
                    return Ok(false);
                }
            };
            match crate::db::get_known_host(&db, &self.host, self.port) {
                Ok(Some((known_fp, _))) => {
                    if known_fp == fingerprint {
                        true
                    } else {
                        eprintln!(
                            "[ssh] host key mismatch for {}:{}: stored={} got={}",
                            self.host, self.port, known_fp, fingerprint
                        );
                        false
                    }
                }
                Ok(None) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs().to_string())
                        .unwrap_or_else(|_| "0".to_string());
                    if let Err(e) = crate::db::set_known_host(
                        &db,
                        &self.host,
                        self.port,
                        &fingerprint,
                        &key_type,
                        &now,
                    ) {
                        // Couldn't persist new host → fail closed. Surfacing
                        // the error to the user is better than accepting and
                        // silently losing TOFU for this host.
                        eprintln!("[ssh] known_hosts insert failed: {}", e);
                        return Ok(false);
                    }
                    true
                }
                Err(e) => {
                    // DB query error — fail closed. Don't trust the host on
                    // an unreadable store, even though the read failure is
                    // likely transient.
                    eprintln!(
                        "[ssh] known_hosts query failed for {}:{}: {} — rejecting",
                        self.host, self.port, e
                    );
                    return Ok(false);
                }
            }
        };

        Ok(accepted)
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SshOutputPayload {
    session_id: String,
    data: Vec<u8>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ZmodemStartPayload {
    session_id: String,
    /// "upload" = remote ran `rz` (we push bytes to them),
    /// "download" = remote ran `sz` (they push bytes to us).
    direction: &'static str,
}

#[derive(Clone, Copy, PartialEq)]
enum TermMode {
    Normal,
    Zmodem,
}

/// CAN — ZMODEM cancel byte (lrzsz sends 5+ to abort).
const CAN: u8 = 0x18;

/// ZMODEM auto-start needles. The protocol uses three framing variants after
/// the ZDLE escape byte:
///   `ZBIN  = 'A'` — binary frame, CRC16
///   `ZHEX  = 'B'` — hex frame    (lrzsz's rz/sz both send this first)
///   `ZBIN32 = 'C'` — binary frame, CRC32
/// The canonical lead-in is `ZPAD ZPAD ZDLE` = `**\x18`, but PTY echo can strip
/// the leading `**`, so we also accept the bare `\x18[ABC]` form.
const ZMODEM_NEEDLES: [&[u8]; 6] = [
    b"**\x18A", b"**\x18B", b"**\x18C",  // canonical (prefer — earliest match wins)
    b"\x18A",   b"\x18B",   b"\x18C",    // bare ZDLE fallback
];

/// Scan `buf` for the earliest ZMODEM auto-start. Returns the index of the
/// first byte of the match so the caller can split: prefix → terminal, suffix
/// → zmodem.js.
fn find_zmodem_start(buf: &[u8]) -> Option<usize> {
    let mut earliest: Option<usize> = None;
    for needle in ZMODEM_NEEDLES {
        if needle.len() > buf.len() {
            continue;
        }
        if let Some(idx) = buf.windows(needle.len()).position(|w| w == needle) {
            match earliest {
                None => earliest = Some(idx),
                Some(cur) if idx < cur => earliest = Some(idx),
                _ => {}
            }
        }
    }
    earliest
}

/// Best-effort direction hint derived from the first frame's type byte.
/// Both rz and sz open with ZRINIT, so this is unreliable on the first frame —
/// the frontend's zmodem.js will figure out the real role from the full
/// handshake. We send "auto" by default and only commit to a direction when
/// the leading frame is unambiguous (ZFILE → download).
fn detect_direction(tail: &[u8]) -> &'static str {
    for &b in tail.iter().take(32) {
        match b {
            0x04 | 0x02 => return "download", // ZFILE / ZSINIT — sender (sz) talking
            _ => continue,
        }
    }
    "auto"
}

/// Detect end-of-ZMODEM signals: a ZFIN frame or a burst of 5+ CAN bytes
/// (lrzsz abort sequence).
///
/// ZFIN appears as type byte 0x08, but the wire format depends on framing:
///   ZHEX   `**\x18B` + ASCII "08" (0x30 0x38) — lrzsz's default for ZFIN
///   ZBIN   `**\x18A` + raw byte 0x08
///   ZBIN32 `**\x18C` + raw byte 0x08
/// PTY echo can strip the leading `**`, so we also check bare-ZDLE forms.
fn is_zmodem_end(buf: &[u8]) -> bool {
    // 5+ consecutive CAN bytes — lrzsz abort sequence
    let mut can_run = 0;
    for &b in buf {
        if b == CAN {
            can_run += 1;
            if can_run >= 5 {
                return true;
            }
        } else {
            can_run = 0;
        }
    }

    const ZFIN_NEEDLES: [&[u8]; 6] = [
        b"**\x18B08", // canonical ZHEX
        b"**\x18A\x08", // canonical ZBIN
        b"**\x18C\x08", // canonical ZBIN32
        b"\x18B08",   // bare ZDLE ZHEX (PTY echo stripped **)
        b"\x18A\x08", // bare ZDLE ZBIN
        b"\x18C\x08", // bare ZDLE ZBIN32
    ];
    for needle in ZFIN_NEEDLES {
        if needle.len() <= buf.len()
            && buf.windows(needle.len()).any(|w| w == needle)
        {
            return true;
        }
    }
    false
}

pub async fn connect(
    state: State<'_, AppState>,
    window: WebviewWindow,
    config: ConnectionConfig,
) -> Result<String, String> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let sid = session_id.clone();

    // Configure the russh client. `inactivity_timeout` ensures that a
    // server which goes silent (NAT timeout, suspended VM, dead WiFi) is
    // detected within ~3 minutes instead of holding the session in
    // `AppState::ssh_sessions` forever — without it, the frontend's
    // `ssh_closed` event never fires and the UI shows "connected but
    // unresponsive" indefinitely. The 3-minute window is long enough that
    // idle interactive shells won't trip it during normal use.
    let mut ssh_config = client::Config::default();
    ssh_config.inactivity_timeout = Some(Duration::from_secs(180));
    let ssh_config = Arc::new(ssh_config);

    let handler = SshClient {
        db: Arc::clone(&state.db),
        host: config.host.clone(),
        port: config.port,
    };

    // Branch on proxy config: if proxy_type is set, dial the proxy first
    // and hand the upgraded stream to russh's connect_stream variant. SFTP
    // reuses this same session (sftp.rs) so the proxy choice covers SFTP
    // automatically without a separate code path.
    let mut handle = match proxy::ProxyConfig::from_config(&config)? {
        Some(proxy_cfg) => {
            eprintln!(
                "[ssh:{}] connecting via {} proxy {}:{} → {}:{}",
                sid,
                config.proxy_type,
                proxy_cfg.host(),
                proxy_cfg.port(),
                config.host,
                config.port
            );
            let stream = proxy::connect_via_proxy(&proxy_cfg, &config.host, config.port).await?;
            client::connect_stream(ssh_config, stream, handler)
                .await
                .map_err(|e| format!("SSH connect via proxy failed: {}", e))?
        }
        None => {
            client::connect(ssh_config, (config.host.as_str(), config.port), handler)
                .await
                .map_err(|e| format!("SSH connect failed: {}", e))?
        }
    };

    // Authenticate
    let auth_result = match config.auth_method.as_str() {
        "key" => {
            let pem = config
                .private_key_pem
                .as_ref()
                .ok_or_else(|| "未导入私钥（vault 中无私钥内容）".to_string())?;
            let key_pair = russh::keys::decode_secret_key(pem, None)
                .map_err(|e| format!("解析私钥失败: {}", e))?;
            let key_with_hash = russh::keys::PrivateKeyWithHashAlg::new(
                Arc::new(key_pair),
                None,
            );
            handle
                .authenticate_publickey(&config.username, key_with_hash)
                .await
                .map_err(|e| format!("Key auth failed: {}", e))?
        }
        _ => {
            // ssh_connect command already resolved the password from keyring
            // before calling us, so config.password is the plaintext secret.
            let password = config
                .password
                .clone()
                .unwrap_or_default();
            handle
                .authenticate_password(&config.username, &password)
                .await
                .map_err(|e| format!("Password auth failed: {}", e))?
        }
    };

    if !auth_result.success() {
        return Err("Authentication failed".to_string());
    }

    // Open channel and request PTY
    eprintln!("[ssh:{}] opening session channel", sid);
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("Channel open failed: {}", e))?;
    eprintln!("[ssh:{}] channel opened; requesting PTY", sid);

    channel
        .request_pty(true, "xterm-256color", 80, 24, 640, 480, &[])
        .await
        .map_err(|e| format!("PTY request failed: {}", e))?;
    eprintln!("[ssh:{}] PTY granted; requesting shell", sid);

    channel
        .request_shell(true)
        .await
        .map_err(|e| format!("Shell request failed: {}", e))?;
    eprintln!("[ssh:{}] shell requested OK", sid);

    // Build command channel for the reader task
    let (command_tx, command_rx) = mpsc::unbounded_channel::<SessionCommand>();

    let session = SshSession {
        handle: Arc::new(handle),
        command_tx,
    };

    // Store session (handle is moved into SshSession above, so use it from there)
    {
        let mut sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        sessions.insert(sid.clone(), session);
    }

    // Spawn the channel reader task. It owns the Channel and multiplexes
    // incoming SSH data with commands from the frontend. Emits are scoped to
    // the originating window so a different webview can't read this session's
    // output.
    let sessions_arc = Arc::clone(&state.ssh_sessions);
    let reader_sid = sid.clone();

    async_runtime::spawn(async move {
        channel_reader(window, reader_sid, sessions_arc, channel, command_rx).await;
    });

    Ok(sid)
}

async fn channel_reader(
    window: WebviewWindow,
    session_id: String,
    sessions: Arc<Mutex<HashMap<String, SshSession>>>,
    mut channel: Channel<Msg>,
    mut command_rx: mpsc::UnboundedReceiver<SessionCommand>,
) {
    let mut buffer: Vec<u8> = Vec::with_capacity(8192);
    let mut last_flush = Instant::now();
    let mut flush_interval = tokio::time::interval(Duration::from_millis(16));
    // First tick completes immediately; consume it so we don't flush an empty buffer.
    flush_interval.tick().await;
    log::info!("[ssh:{}] channel_reader started", session_id);

    let mut mode = TermMode::Normal;
    // Count of OO bytes (0x4F) eaten from the start of the post-ZMODEM
    // stream. ZMODEM sessions end with ZFIN + "OO" (Over and Out). Rust
    // switches back to Normal on ZFIN, so the trailing OO would otherwise
    // render as visible glyphs on the terminal.
    let mut oo_eaten: u8 = 2; // start saturated = "not eating"

    // Cancel-safety note: `channel.wait()` is awaited inside `tokio::select!`.
    // We previously attempted to move Data bytes onto a dedicated reader task
    // via `channel.make_reader()`, but russh 0.50's `make_reader` borrows the
    // channel mutably and the spawned task requires `'static`, which makes the
    // borrow last for the task's lifetime and starves every other `channel.*`
    // call in this loop. The Channel itself is not Send, so we can't move the
    // whole channel into the reader task either.
    //
    // In practice `wait()` is backed by an internal tokio mpsc::Receiver,
    // whose `recv()` is cancel-safe — cancellation between polls leaves the
    // pending message in the channel for the next call. If we ever observe
    // dropped bytes under high traffic, the fix is to upstream a Send-wrapping
    // variant of Channel in russh or migrate to a fork that exposes the
    // underlying Receiver directly.

    loop {
        tokio::select! {
            biased;

            // Commands from the frontend take priority so input latency stays low.
            cmd = command_rx.recv() => match cmd {
                Some(SessionCommand::Input(data)) => {
                    if let Err(e) = channel.data(&data[..]).await {
                        eprintln!("[ssh] data send failed for {}: {}", session_id, e);
                        break;
                    }
                }
                Some(SessionCommand::Resize { cols, rows }) => {
                    if let Err(e) = channel.window_change(cols, rows, 0, 0).await {
                        eprintln!("[ssh] window_change failed for {}: {}", session_id, e);
                    }
                }
                Some(SessionCommand::ZmodemBytes(data)) => {
                    if let Err(e) = channel.data(&data[..]).await {
                        eprintln!("[ssh] zmodem send failed for {}: {}", session_id, e);
                    }
                }
                Some(SessionCommand::ZmodemAbort) => {
                    // lrzsz abort sequence: 8× CAN + a few backspaces to clean
                    // the remote's PTY line state.
                    let abort = [CAN; 8];
                    let _ = channel.data(&abort[..]).await;
                    let cleanup: &[u8] = b"\x08\x08\x08\x08\x08\x08\x08\x08";
                    let _ = channel.data(cleanup).await;
                }
                Some(SessionCommand::Disconnect) | None => {
                    let _ = channel.close().await;
                    break;
                }
            },

            // Incoming SSH messages from the server.
            msg = channel.wait() => match msg {
                Some(ChannelMsg::Data { ref data }) => {
                    eprintln!("[ssh:{}] Data {} bytes", session_id, data.len());
                    // Strip leading OO (Over and Out) bytes that follow a ZMODEM
                    // session end. These arrive in Normal mode but are protocol
                    // tail bytes, not user-visible output.
                    let data: &[u8] = if oo_eaten < 2 {
                        let mut i = 0;
                        while i < data.len() && oo_eaten < 2 && data[i] == 0x4F {
                            i += 1;
                            oo_eaten += 1;
                        }
                        if i > 0 {
                            &data[i..]
                        } else {
                            // First byte isn't 'O' — give up eating (lrzsz might
                            // omit OO, or shell printed prompt immediately).
                            oo_eaten = 2;
                            data
                        }
                    } else {
                        data
                    };
                    if !data.is_empty() {
                        handle_incoming_data(
                            &window, &session_id, data, &mut buffer, &mut last_flush,
                            &mut mode, &mut oo_eaten,
                        );
                    }
                }
                Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                    eprintln!("[ssh:{}] ExtendedData {} bytes", session_id, data.len());
                    // stderr — treat as ordinary terminal output, never as ZMODEM.
                    append_capped(&window, &session_id, &mut buffer, &data[..], &mut last_flush);
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    eprintln!("[ssh:{}] ExitStatus={}", session_id, exit_status);
                    let _ = window.emit("ssh_exit", serde_json::json!({
                        "sessionId": session_id,
                        "code": exit_status
                    }));
                }
                Some(ChannelMsg::Eof) => {
                    eprintln!("[ssh:{}] EOF", session_id);
                    flush_buffer(&window, &session_id, &mut buffer);
                    if mode == TermMode::Zmodem {
                        let _ = window.emit("zmodem_end", &session_id);
                    }
                    let _ = window.emit("ssh_closed", &session_id);
                    break;
                }
                Some(ChannelMsg::Close) => {
                    eprintln!("[ssh:{}] Close", session_id);
                    flush_buffer(&window, &session_id, &mut buffer);
                    if mode == TermMode::Zmodem {
                        let _ = window.emit("zmodem_end", &session_id);
                    }
                    let _ = window.emit("ssh_closed", &session_id);
                    break;
                }
                None => {
                    eprintln!("[ssh:{}] channel.wait returned None", session_id);
                    flush_buffer(&window, &session_id, &mut buffer);
                    if mode == TermMode::Zmodem {
                        let _ = window.emit("zmodem_end", &session_id);
                    }
                    let _ = window.emit("ssh_closed", &session_id);
                    break;
                }
                Some(other) => {
                    eprintln!("[ssh:{}] other msg: {:?}", session_id, std::mem::discriminant(&other));
                }
            },

            // Periodic flush so terminal output reaches the UI even when the
            // server trickles bytes. Coalesces bursts and mitigates tauri#13234.
            _ = flush_interval.tick() => {
                if !buffer.is_empty() && last_flush.elapsed() >= Duration::from_millis(16) {
                    flush_buffer(&window, &session_id, &mut buffer);
                    last_flush = Instant::now();
                }
            }
        }
    }

    // Remove ourselves from the session map (covers server-side EOF/kill).
    if let Ok(mut map) = sessions.lock() {
        map.remove(&session_id);
    }
    log::info!("[ssh:{}] channel_reader exited", session_id);
}

/// Routes incoming PTY bytes: terminal output in Normal mode, raw ZMODEM frames
/// in Zmodem mode. The Normal→Zmodem transition happens when we spot the
/// auto-start sequence anywhere in the accumulated buffer; the Zmodem→Normal
/// transition fires on ZFIN or 5× CAN. On Zmodem→Normal, `oo_eaten` is reset
/// to 0 so the caller can strip the trailing OO bytes from the next chunks.
fn handle_incoming_data(
    window: &WebviewWindow,
    session_id: &str,
    data: &[u8],
    buffer: &mut Vec<u8>,
    last_flush: &mut Instant,
    mode: &mut TermMode,
    oo_eaten: &mut u8,
) {
    match *mode {
        TermMode::Normal => {
            // Append to the shared terminal/ZMODEM-scan buffer. If no needle is
            // found the existing 16ms tick will coalesce-flush it as terminal
            // output via append_capped's threshold logic.
            append_capped(window, session_id, buffer, data, last_flush);

            // Re-scan the full buffer for the ZMODEM auto-start sequence. It can
            // land anywhere — typically right after a prompt — so we can't only
            // look at the tail.
            if let Some(idx) = find_zmodem_start(buffer) {
                // Split: bytes [0..idx] stay as terminal output (already emitted
                // by append_capped's threshold path or queued for the 16ms tick);
                // bytes [idx..] become the first raw ZMODEM frame.
                let tail: Vec<u8> = buffer[idx..].to_vec();
                buffer.truncate(idx);

                // Force-flush the terminal prefix so xterm shows what came before
                // the protocol switching point.
                flush_buffer(window, session_id, buffer);

                let direction = detect_direction(&tail);
                *mode = TermMode::Zmodem;
                let _ = window.emit(
                    "zmodem_start",
                    ZmodemStartPayload {
                        session_id: session_id.to_string(),
                        direction,
                    },
                );
                let _ = window.emit(
                    "zmodem_raw",
                    SshOutputPayload {
                        session_id: session_id.to_string(),
                        data: tail,
                    },
                );
            }
        }
        TermMode::Zmodem => {
            // Forward raw bytes to the frontend zmodem.js parser. Do NOT coalesce
            // on the 16ms tick — protocol framing is byte-precise and a delayed
            // flush would corrupt the parser's state machine.
            let _ = window.emit(
                "zmodem_raw",
                SshOutputPayload {
                    session_id: session_id.to_string(),
                    data: data.to_vec(),
                },
            );
            if is_zmodem_end(data) {
                *mode = TermMode::Normal;
                *oo_eaten = 0; // arm the OO-stripping for the next chunks
                let _ = window.emit("zmodem_end", session_id);
            }
        }
    }
}

fn flush_buffer(window: &WebviewWindow, session_id: &str, buffer: &mut Vec<u8>) {
    if buffer.is_empty() {
        return;
    }
    let payload = SshOutputPayload {
        session_id: session_id.to_string(),
        data: std::mem::take(buffer),
    };
    eprintln!("[ssh:{}] flush {} bytes → ssh_output", session_id, payload.data.len());
    if let Err(e) = window.emit("ssh_output", payload) {
        eprintln!("[ssh:{}] emit ssh_output FAILED: {}", session_id, e);
    }
}

/// Hard cap on the in-flight output buffer. Prevents a hostile or chatty
/// server from OOMing the frontend (xterm.js + IPC bridge) by piling up
/// data faster than `term.write` drains.
const MAX_BUFFER_SIZE: usize = 256 * 1024;

/// Inline flush trigger — when the buffer reaches this size we flush
/// immediately rather than waiting for the 16ms tick. Keeps the tick branch
/// from being starved under sustained output.
const FLUSH_THRESHOLD: usize = 16 * 1024;

const TRUNCATION_MARKER: &[u8] =
    b"\r\n\x1b[33m[output truncated: server sending too fast]\x1b[0m\r\n";

fn append_capped(
    window: &WebviewWindow,
    session_id: &str,
    buffer: &mut Vec<u8>,
    incoming: &[u8],
    last_flush: &mut Instant,
) {
    let space = MAX_BUFFER_SIZE.saturating_sub(buffer.len());
    if incoming.len() <= space {
        buffer.extend_from_slice(incoming);
    } else {
        // Buffer would overflow: flush what fits, mark truncation, drop the rest.
        if space > 0 {
            buffer.extend_from_slice(&incoming[..space]);
        }
        flush_buffer(window, session_id, buffer);
        buffer.extend_from_slice(TRUNCATION_MARKER);
        flush_buffer(window, session_id, buffer);
        *last_flush = Instant::now();
        return;
    }
    if buffer.len() >= FLUSH_THRESHOLD {
        flush_buffer(window, session_id, buffer);
        *last_flush = Instant::now();
    }
}

pub async fn send_input(
    state: &State<'_, AppState>,
    session_id: &str,
    data: &[u8],
) -> Result<(), String> {
    let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| "Session not found".to_string())?;

    session
        .command_tx
        .send(SessionCommand::Input(data.to_vec()))
        .map_err(|_| "Session command channel closed".to_string())?;

    Ok(())
}

pub async fn resize_terminal(
    state: &State<'_, AppState>,
    session_id: &str,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| "Session not found".to_string())?;

    session
        .command_tx
        .send(SessionCommand::Resize {
            cols: cols as u32,
            rows: rows as u32,
        })
        .map_err(|_| "Session command channel closed".to_string())?;

    Ok(())
}

pub async fn disconnect(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<(), String> {
    // Try to signal the reader task to close the channel gracefully.
    // The reader task itself removes the session from the map on exit.
    let sender = {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        sessions
            .get(session_id)
            .map(|s| s.command_tx.clone())
    };

    if let Some(tx) = sender {
        let _ = tx.send(SessionCommand::Disconnect);
    } else {
        // Already gone (server closed or never existed) — nothing to do.
    }

    Ok(())
}

/// Push bytes produced by the frontend's zmodem.js (ZRINIT/ZRPOS/ZDATA acks,
/// file payload during upload) into the SSH channel.
pub async fn zmodem_send_bytes(
    state: &State<'_, AppState>,
    session_id: &str,
    data: &[u8],
) -> Result<(), String> {
    let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| "Session not found".to_string())?;

    session
        .command_tx
        .send(SessionCommand::ZmodemBytes(data.to_vec()))
        .map_err(|_| "Session command channel closed".to_string())?;

    Ok(())
}

/// Tell the reader task to fire the lrzsz abort sequence (8× CAN + backspaces).
/// Used by the cancel button in the progress overlay.
pub async fn zmodem_abort(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<(), String> {
    let sender = {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        sessions
            .get(session_id)
            .map(|s| s.command_tx.clone())
    };

    if let Some(tx) = sender {
        let _ = tx.send(SessionCommand::ZmodemAbort);
    }

    Ok(())
}

/// Open a fresh exec channel on the existing SSH session, run `command`, and
/// collect stdout. Used for one-shot probes like server-info gathering. The
/// interactive PTY channel is untouched — russh multiplexes channels over the
/// same connection. Caller is responsible for timeout if needed.
pub async fn exec_once(
    state: &State<'_, AppState>,
    session_id: &str,
    command: &str,
) -> Result<String, String> {
    let handle = {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        sessions
            .get(session_id)
            .map(|s| Arc::clone(&s.handle))
            .ok_or_else(|| "Session not found".to_string())?
    };

    let mut channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("Open exec channel failed: {}", e))?;
    channel
        .exec(true, command)
        .await
        .map_err(|e| format!("exec failed: {}", e))?;

    // Cap collected output so a chatty/malicious server can't OOM the app
    // within the 20s probe window (e.g. `yes` / `cat /dev/zero` left running).
    // Matches the spirit of the interactive channel's append_capped cap.
    const MAX_EXEC_BYTES: usize = 4 * 1024 * 1024;
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    let mut truncated = false;
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Data { ref data }) => {
                if buf.len() < MAX_EXEC_BYTES {
                    let room = MAX_EXEC_BYTES - buf.len();
                    buf.extend_from_slice(&data[..data.len().min(room)]);
                    if buf.len() == MAX_EXEC_BYTES {
                        truncated = true;
                    }
                }
            }
            Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                if buf.len() < MAX_EXEC_BYTES {
                    let room = MAX_EXEC_BYTES - buf.len();
                    buf.extend_from_slice(&data[..data.len().min(room)]);
                    if buf.len() == MAX_EXEC_BYTES {
                        truncated = true;
                    }
                }
            }
            Some(ChannelMsg::ExitStatus { .. })
            | Some(ChannelMsg::Eof)
            | Some(ChannelMsg::Close)
            | None => break,
            Some(_) => {}
        }
    }
    let mut out = String::from_utf8_lossy(&buf).into_owned();
    if truncated {
        out.push_str("\n[output truncated]\n");
    }
    Ok(out)
}
