use crate::{AppState, ConnectionConfig, EventSink, EventSinkExt};
use crate::proxy;
use crate::zmodem_rx::{ZmodemReceiver, RxActions, RxEvent};

/// Jobs for the background ZMODEM disk-write task.
enum DiskJob {
    /// Take ownership of a newly opened file for writing.
    Open(std::fs::File),
    /// Append decoded payload bytes to the open file.
    Write(Vec<u8>),
    /// Flush the open file (e.g. at ZEOF).
    Flush,
    /// Flush and release the file handle.
    Close,
}
use russh::client::{self, Handle, Msg};
use russh::{Channel, ChannelMsg, Disconnect};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

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
    /// Frontend signals the ZMODEM session is definitively over (zmodem.js
    /// parsed ZFIN or the user aborted). Switch TermMode back to Normal so
    /// subsequent SSH data renders as terminal output. This is the AUTHORITATIVE
    /// end signal — the Rust-side `is_zmodem_end` byte-pattern check is only a
    /// fallback for 5×CAN (abort) and should NOT detect ZFIN (false-positives
    /// on binary file data break multi-file transfers).
    ZmodemFinish,
    /// Native ZMODEM receiver: frontend chose a save path for the offered
    /// file. `Some(path)` = accept and write there; `None` = skip this file.
    ZmodemAcceptOffer { path: Option<String> },
    Disconnect,
}

pub struct SshSession {
    pub handle: Arc<Handle<SshClient>>,
    pub command_tx: mpsc::UnboundedSender<SessionCommand>,
    /// Original connection config (incl. credentials). Kept in memory so that
    /// `exec_once` can transparently reconnect (dial + auth + replace handle)
    /// when the underlying SSH transport dies mid-session — without this, a
    /// dead-connection error during AI health inspection / one-shot exec
    /// surfaces to the user as "please reconnect manually". The config is
    /// already in memory for the lifetime of the session (connect() received
    /// it); storing the Arc here doesn't extend exposure.
    pub config: Arc<ConnectionConfig>,
}

/// Carries the DB handle so `check_server_key` can look up known_hosts.
/// Per-host: instantiated once per `connect()` call.
pub struct SshClient {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub host: String,
    pub port: u16,
    /// Records a host-key mismatch detected during `check_server_key` so the
    /// connect flow can surface a specific, actionable error instead of
    /// russh's opaque "Unknown server key". Holds (stored_fp, presented_fp).
    /// Shared with `dial_and_authenticate` (which keeps a clone) because russh
    /// consumes the handler on connect and doesn't hand it back on failure.
    pub host_key_mismatch: Arc<Mutex<Option<(String, String)>>>,
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
                    log::warn!(
                        "[ssh] known_hosts mutex poisoned; rejecting key for {}:{}",
                        crate::redact::host(&self.host), self.port
                    );
                    return Ok(false);
                }
            };
            match crate::db::get_known_host(&db, &self.host, self.port) {
                Ok(Some((known_fp, _))) => {
                    if known_fp == fingerprint {
                        true
                    } else {
                        log::warn!(
                            "[ssh] host key mismatch for {}:{}: stored={} got={}",
                            crate::redact::host(&self.host), self.port, known_fp, fingerprint
                        );
                        // Stash both fingerprints so dial_and_authenticate can
                        // turn russh's opaque "Unknown server key" into a
                        // specific "host key changed — reset trust" error.
                        if let Ok(mut slot) = self.host_key_mismatch.lock() {
                            *slot = Some((known_fp.clone(), fingerprint.clone()));
                        }
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
                        log::warn!("[ssh] known_hosts insert failed: {}", e);
                        return Ok(false);
                    }
                    true
                }
                Err(e) => {
                    // DB query error — fail closed. Don't trust the host on
                    // an unreadable store, even though the read failure is
                    // likely transient.
                    log::warn!(
                        "[ssh] known_hosts query failed for {}:{}: {} — rejecting",
                        crate::redact::host(&self.host), self.port, e
                    );
                    return Ok(false);
                }
            }
        };

        Ok(accepted)
    }

    /// Dynamically grow the SSH channel receive window target. russh's
    /// default behavior tops up the window to a fixed target when it drops
    /// below target/2; on high-RTT links this limits throughput because the
    /// WINDOW_ADJUST round-trip gates the sender. By returning an
    /// ever-larger target we keep the advertised window deep enough that the
    /// sender never stalls waiting for an adjust.
    fn adjust_window(&mut self, _channel: russh::ChannelId, window: u32) -> u32 {
        window.max(16 * 1024 * 1024)
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

/// Strip the lrzsz "auto-start" noise that `sz` prints to stdout immediately
/// before the protocol frame: the literal `rz` (+ optional CR/LF). This is a
/// legacy convention — `sz` emits `rz\r\n` so a ZMODEM-aware terminal emulator
/// on the other end auto-launches its local `rz`. MyShell (like most modern
/// clients) doesn't need it; it just shows up as a stray "rz" line in the
/// terminal. Only safe to call at the exact ZMODEM switch point, where we know
/// a protocol frame immediately follows — so a trailing `rz` here can only be
/// the lrzsz trigger, never real user output.
fn strip_zmodem_autostart_noise(buf: &mut Vec<u8>) {
    // The text is at the tail, possibly preceded by \r and/or \n. Walk back
    // over trailing CR/LF/whitespace, then expect exactly "rz".
    let mut end = buf.len();
    while end > 0 && matches!(buf[end - 1], b'\r' | b'\n' | b' ' | b'\t') {
        end -= 1;
    }
    if end >= 2 && &buf[end - 2..end] == b"rz" {
        // Keep nothing of the trigger — but preserve any newline that separated
        // the command echo from the (now-removed) noise so the prompt flow
        // still reads naturally. Trim the "rz" plus trailing CR/LF only.
        let mut trim_to = end - 2;
        // Also drop one preceding CR/LF if present so we don't leave a blank
        // line where the noise used to be.
        if trim_to > 0 && matches!(buf[trim_to - 1], b'\r' | b'\n') {
            trim_to -= 1;
        }
        buf.truncate(trim_to);
    }
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

/// Detect end-of-ZMODEM signals: a burst of 5+ CAN bytes (lrzsz abort).
///
/// ZFIN detection was REMOVED: naive byte-pattern matching on the raw stream
/// false-positives on binary file data during multi-file transfers (the
/// compressed payload of a 323 MB tar.gz easily contains ZFIN-like sequences).
/// The frontend's zmodem.js properly parses ZFIN with full ZDLE-escaping
/// awareness and signals the end via `SessionCommand::ZmodemFinish`. This
/// function remains only as a fallback for the abort case (5×CAN), which is
/// unambiguous — CAN (0x18) is always ZDLE-escaped in data subpackets, so 5
/// consecutive raw CANs can only be an intentional abort sequence.
fn is_zmodem_end(buf: &[u8]) -> bool {
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
    false
}

/// Dial the SSH server (direct or via proxy) and authenticate, returning the
/// authenticated `Handle` without registering a session, spawning a reader,
/// requesting a PTY/shell, or emitting any events.
///
/// Extracted from `connect` so both the real connect path and the "test"
/// probe share the exact same dial + auth + known_hosts TOFU logic — the test
/// catches the same auth/network/host-key failures a real connect would.
///
/// `open_channel` opens one session channel and immediately drops it, which
/// validates the multiplexed-channel path that real SFTP relies on (catches
/// "authenticated but server rejects channels"). The real connect path passes
/// `false` and opens its own channel afterwards instead.
/// Connection phase timeout (TCP dial + SSH handshake). If the server is
/// unresponsive (dead host, firewall dropping SYN, suspended VM), the initial
/// Default connect timeout when a connection doesn't override it via
/// `connect_timeout_secs`. `client::connect()` can block indefinitely waiting
/// for TCP retransmits, so we always cap the wait so the user gets a clear
/// error quickly instead of a hung UI.
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Wraps a connection future with a timeout. On timeout, returns a localized
/// error so the user sees "连接超时" instead of hanging forever. The timeout
/// is per-connection (`config.connect_timeout_secs`, resolved by the caller).
async fn with_connect_timeout<F, T, E>(
    fut: F,
    timeout: Duration,
    ctx: &str,
    map_err: impl FnOnce(E) -> String,
) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, E>>,
{
    match tokio::time::timeout(timeout, fut).await {
        Ok(Ok(handle)) => Ok(handle),
        Ok(Err(e)) => Err(map_err(e)),
        Err(_) => Err(format!(
            "{}超时（{:?} 无响应），请检查服务器地址和网络",
            ctx, timeout
        )),
    }
}

/// Resolve `host:port` and dial a TCP stream, honoring the per-connection
/// address-family preference. `family` is "ipv4" / "ipv6" / anything-else
/// (= auto: try whatever the OS returns, in order). We resolve ourselves
/// (instead of letting `client::connect` do it) so we can filter the address
/// family — this is what fixes hosts with a dead AAAA record black-holing the
/// connect before it falls back to IPv4. The dial is capped by `timeout`.
async fn dial_tcp(
    host: &str,
    port: u16,
    family: &str,
    timeout: Duration,
) -> Result<TcpStream, String> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("DNS 解析失败 ({}): {}", host, e))?
        .filter(|a| match family {
            "ipv4" => a.is_ipv4(),
            "ipv6" => a.is_ipv6(),
            _ => true,
        })
        .collect();
    if addrs.is_empty() {
        return Err(match family {
            "ipv4" => format!("{} 无可用 IPv4 地址", host),
            "ipv6" => format!("{} 无可用 IPv6 地址", host),
            _ => format!("{} 无可用地址", host),
        });
    }
    match tokio::time::timeout(timeout, TcpStream::connect(addrs.as_slice())).await {
        Ok(Ok(stream)) => {
            // Disable Nagle so small, latency-sensitive packets — especially
            // SSH CHANNEL_WINDOW_ADJUST (~13 bytes) — go out immediately.
            // russh never sets this itself; with Nagle active the window
            // adjust can be held ~200ms, which collapses the receive window
            // and throttles bulk downloads to a burst/pause crawl.
            let _ = stream.set_nodelay(true);
            Ok(stream)
        }
        Ok(Err(e)) => Err(format!("TCP 连接失败 ({}:{}): {}", host, port, e)),
        Err(_) => Err(format!(
            "连接超时（{:?} 无响应），请检查服务器地址和网络",
            timeout
        )),
    }
}

pub async fn dial_and_authenticate(
    state: &AppState,
    config: &ConnectionConfig,
    open_channel: bool,
) -> Result<Handle<SshClient>, String> {
    // Configure the russh client. We use keepalives (not inactivity_timeout)
    // to detect dead servers. Rationale: an interactive shell legitimately
    // stays silent for a long time while a command runs — e.g.
    // `find . | sort | uniq -c` blocks until find finishes (sort is a
    // blocking consumer of its stdin), so the PTY emits nothing for
    // minutes. inactivity_timeout is a *connection-level* idle timer in
    // russh: if NO packets arrive from the server within the window it
    // returns InactivityTimeout, killing the connection mid-command —
    // which is exactly the bug we hit (channel.wait() → None → ssh_closed,
    // terminal shows "[Connection closed]" while the command was still
    // running). MobaXterm/OpenSSH don't have this problem because they
    // keep the connection alive with periodic heartbeats instead of a
    // hard idle cutoff.
    //
    // keepalive_interval sends a no-op request every 15s; keepalive_max=3
    // means the connection is only dropped after ~45s of total silence
    // (no response to 3 consecutive keepalives). A healthy server — even
    // one busy running a long command — responds to keepalives, so the
    // connection survives arbitrary command durations. A genuinely dead
    // server (NAT timeout, suspended VM, dead WiFi) stops responding and
    // is detected within ~45s, close to the old 30s intent but without
    // the false-positive on long-running commands.
    // Per-connection connect timeout (None = 10s default).
    let connect_timeout = config
        .connect_timeout_secs
        .map(|s| Duration::from_secs(s as u64))
        .unwrap_or(DEFAULT_CONNECT_TIMEOUT);

    let mut ssh_config = client::Config::default();
    // Keepalive interval is per-connection (None = 15s default); max stays 3.
    ssh_config.keepalive_interval = Some(Duration::from_secs(
        config.keepalive_interval_secs.unwrap_or(15) as u64,
    ));
    ssh_config.keepalive_max = 3;
    // Prefer AES-GCM ciphers over ChaCha20-Poly1305. russh's Rust ChaCha20
    // implementation lacks the AVX2/SIMD optimizations that C-based clients
    // (Xshell, OpenSSH) use, making it CPU-bound on bulk transfers. AES-GCM
    // leverages AES-NI hardware instructions, which are dramatically faster
    // on x86_64 and directly translate to higher throughput for both SFTP
    // and ZMODEM file transfers.
    ssh_config.preferred.cipher = std::borrow::Cow::Borrowed(&[
        russh::cipher::AES_256_GCM,
        russh::cipher::CHACHA20_POLY1305,
        russh::cipher::AES_256_CTR,
        russh::cipher::AES_192_CTR,
        russh::cipher::AES_128_CTR,
    ]);
    // Large SSH channel window so a fast sender (e.g. `sz` streaming a file)
    // can keep the pipe full instead of stalling on window exhaustion. The
    // russh default (~2 MB) throttles bulk transfers to a few MB/s; a 16 MB
    // window lets throughput reach the link rate. Memory cost is per-channel
    // and acceptable for a desktop client.
    ssh_config.window_size = 16 * 1024 * 1024;
    // Deepen the per-channel message queue (default 100). russh's connection
    // task `await`s a bounded send into this queue for every inbound packet;
    // if it fills (because our reader is momentarily busy, e.g. a disk flush),
    // the connection task blocks — stopping TCP reads AND window-adjust writes,
    // which collapses throughput into a burst/pause crawl. A deeper queue
    // absorbs that jitter so the reader's brief stalls don't cascade.
    ssh_config.channel_buffer_size = 1024;
    let ssh_config = Arc::new(ssh_config);

    // Shared slot for a host-key mismatch detected mid-handshake. The handler
    // fills it; we keep a clone to read after connect fails (russh consumes
    // the handler and doesn't return it on error).
    let host_key_mismatch: Arc<Mutex<Option<(String, String)>>> = Arc::new(Mutex::new(None));
    let handler = SshClient {
        db: Arc::clone(&state.db),
        host: config.host.clone(),
        port: config.port,
        host_key_mismatch: Arc::clone(&host_key_mismatch),
    };

    // Branch on proxy config: if proxy_type is set, dial the proxy first
    // and hand the upgraded stream to russh's connect_stream variant. SFTP
    // reuses this same session (sftp.rs) so the proxy choice covers SFTP
    // automatically without a separate code path.
    //
    // Both paths are wrapped with the per-connection `connect_timeout` so a
    // dead server fails fast instead of letting the UI spin forever. The
    // direct path resolves + dials itself (via `dial_tcp`) so it can honor
    // the address-family preference, then hands the stream to connect_stream.
    //
    // The whole dial+handshake runs in an async block so a failure surfaces
    // as a single Result we can post-process: if `check_server_key` recorded
    // a host-key mismatch, we replace russh's opaque "Unknown server key"
    // with a specific, actionable error (pointing at the reset-trust action).
    let connect_result: Result<Handle<SshClient>, String> = async {
        match proxy::ProxyConfig::from_config(config)? {
            Some(proxy_cfg) => {
                log::info!(
                    "[ssh] dialing via {} proxy {}:{} → {}:{}",
                    config.proxy_type,
                    crate::redact::host(proxy_cfg.host()),
                    proxy_cfg.port(),
                    crate::redact::host(&config.host),
                    config.port
                );
                let stream =
                    proxy::connect_via_proxy(&proxy_cfg, &config.host, config.port).await?;
                with_connect_timeout(
                    client::connect_stream(ssh_config, stream, handler),
                    connect_timeout,
                    "通过代理连接",
                    |e| format!("SSH connect via proxy failed: {}", e),
                )
                .await
            }
            None => {
                let stream = dial_tcp(
                    &config.host,
                    config.port,
                    &config.address_family,
                    connect_timeout,
                )
                .await?;
                with_connect_timeout(
                    client::connect_stream(ssh_config, stream, handler),
                    connect_timeout,
                    "连接",
                    |e| format!("SSH connect failed: {}", e),
                )
                .await
            }
        }
    }
    .await;

    let mut handle = connect_result.map_err(|e| {
        if let Ok(slot) = host_key_mismatch.lock() {
            if let Some((stored, got)) = slot.as_ref() {
                return format!(
                    "服务器主机密钥已变更，与本地信任记录不符（通常是服务器重装或重新生成了密钥所致）。\n· 本地记录: {}\n· 当前服务器: {}\n如确认是服务器侧变更，请右键该连接选择「重置主机密钥信任」后重连。",
                    stored, got
                );
            }
        }
        e
    })?;

    // Authenticate
    let auth_result = match config.auth_method.as_str() {
        "key" => {
            let pem = config
                .private_key_pem
                .as_ref()
                .ok_or_else(|| "未导入私钥（vault 中无私钥内容）".to_string())?;
            let key_pair = russh::keys::decode_secret_key(pem, None)
                .map_err(|e| format!("解析私钥失败: {}", e))?;
            let key_with_hash = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key_pair), None);
            handle
                .authenticate_publickey(&config.username, key_with_hash)
                .await
                .map_err(|e| format!("Key auth failed: {}", e))?
        }
        _ => {
            // ssh_connect command already resolved the password from keyring
            // before calling us, so config.password is the plaintext secret.
            let password = config.password.clone().unwrap_or_default();
            handle
                .authenticate_password(&config.username, &password)
                .await
                .map_err(|e| format!("Password auth failed: {}", e))?
        }
    };

    if !auth_result.success() {
        return Err("Authentication failed".to_string());
    }

    if open_channel {
        // Validates that the server will hand us a session channel (the path
        // real SFTP depends on). No PTY/shell — we drop it immediately so the
        // test leaves no lingering session on the server.
        let _ = handle
            .channel_open_session()
            .await
            .map_err(|e| format!("Channel open failed: {}", e))?;
    }

    Ok(handle)
}

/// Probe an SSH/SFTP connection end-to-end WITHOUT registering a session.
/// Dials, authenticates, opens one channel to validate reachability, then
/// sends a graceful SSH disconnect (`ByApplication` = "we're done, not an
/// error"). Returns a human-readable success message with the wall-clock
/// latency. Used by the `test_connection` command for the dialog "测试" button.
pub async fn test_connection(
    state: &AppState,
    config: &ConnectionConfig,
) -> Result<String, String> {
    let started = Instant::now();
    let handle = dial_and_authenticate(state, config, true).await?;
    // Best-effort graceful disconnect — the test already passed; a teardown
    // hiccup must not turn a success into an error.
    let _ = handle
        .disconnect(Disconnect::ByApplication, "test complete", "en")
        .await;
    let ms = started.elapsed().as_millis();
    Ok(format!(
        "连接成功（{} ms，认证方式={}）",
        ms,
        if config.auth_method == "key" {
            "私钥"
        } else {
            "密码"
        }
    ))
}

pub async fn connect(
    state: &AppState,
    sink: Arc<dyn EventSink>,
    config: ConnectionConfig,
) -> Result<String, String> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let sid = session_id.clone();

    // Dial + authenticate. The shared dial_and_authenticate helper is also
    // used by `test_connection`, so a real connect and a connection test
    // exercise the exact same dial/auth/host-key path.
    let handle = dial_and_authenticate(state, &config, false).await?;

    // Open channel and request PTY
    log::info!("[ssh:{}] opening session channel", sid);
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| {
            log::error!("[ssh:{}] channel open failed: {}", sid, e);
            format!("Channel open failed: {}", e)
        })?;

    // SFTP-only sessions don't use an interactive shell — russh-sftp opens its
    // own fresh subsystem channel via `get_sftp_session` on the same `Handle`.
    // Requesting a PTY + shell here is pointless AND actively harmful on
    // SFTP-only / nologin accounts: the login shell exits immediately
    // (ExitStatus=1), which used to make `channel_reader` emit `ssh_closed`,
    // which the frontend's SftpPanel interpreted as the whole connection dying
    // → false "连接已断开 / 重连" overlay even though SFTP works fine. So for
    // SFTP we open the channel (validates the connection) but skip PTY/shell.
    let is_sftp = config.conn_type == "sftp";
    if is_sftp {
        log::info!("[ssh:{}] SFTP session — skipping PTY/shell request", sid);
    } else {
        log::info!("[ssh:{}] channel opened; requesting PTY", sid);
        channel
            .request_pty(true, "xterm-256color", 80, 24, 640, 480, &[])
            .await
            .map_err(|e| {
                log::error!("[ssh:{}] PTY request failed: {}", sid, e);
                format!("PTY request failed: {}", e)
            })?;
        log::info!("[ssh:{}] PTY granted; requesting shell", sid);

        channel
            .request_shell(true)
            .await
            .map_err(|e| {
                log::error!("[ssh:{}] shell request failed: {}", sid, e);
                format!("Shell request failed: {}", e)
            })?;
        log::info!("[ssh:{}] shell requested OK", sid);
    }

    // Build command channel for the reader task
    let (command_tx, command_rx) = mpsc::unbounded_channel::<SessionCommand>();

    // Extract before config is moved into Arc.
    let suppress_tmout = config.suppress_tmout;

    let session = SshSession {
        handle: Arc::new(handle),
        command_tx,
        config: Arc::new(config),
    };

    // Store session (handle is moved into SshSession above, so use it from there)
    {
        let mut sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        sessions.insert(sid.clone(), session);
    }

    // Spawn the channel reader task. It owns the Channel and multiplexes
    // incoming SSH data with commands from the frontend. Emits are scoped to
    // the originating webview so a different webview can't read this session's
    // output. The reader intentionally does NOT remove the session from the
    // map on exit — see the note in `channel_reader` and the removal in
    // `disconnect()`.
    let reader_sid = sid.clone();

    tokio::spawn(async move {
        channel_reader(sink, reader_sid, channel, command_rx, suppress_tmout).await;
    });

    Ok(sid)
}

async fn channel_reader(
    sink: Arc<dyn EventSink>,
    session_id: String,
    mut channel: Channel<Msg>,
    mut command_rx: mpsc::UnboundedReceiver<SessionCommand>,
    suppress_tmout: bool,
) {
    let mut buffer: Vec<u8> = Vec::with_capacity(8192);
    let mut last_flush = Instant::now();
    let mut flush_interval = tokio::time::interval(Duration::from_millis(16));
    // First tick completes immediately; consume it so we don't flush an empty buffer.
    flush_interval.tick().await;
    log::info!("[ssh:{}] channel_reader started", session_id);

    // One-shot shell TMOUT suppression — gated on the dedicated `suppress_tmout`
    // toggle (independent of the exec keepalive below). The interactive login
    // shell inherits `TMOUT` (set in /etc/profile, /etc/profile.d/*) which
    // force-logs-out the session after N seconds of no *keyboard* input —
    // regardless of whether the SSH connection itself is alive. The exec
    // keepalive below runs in a *separate* non-interactive process, so it does
    // NOT reset TMOUT; and on restricted single-session accounts (MaxSessions=1)
    // opening that extra channel makes the server DROP the whole connection, so
    // the two mechanisms must stay decoupled. To defeat TMOUT we touch the
    // interactive PTY itself: injecting `export TMOUT=0` right after the shell
    // starts (before the user can run anything, so it lands in the login shell,
    // not vim/less), on the EXISTING channel — no new channel, safe everywhere.
    // `2>/dev/null` swallows the error when an admin locked it `readonly
    // TMOUT=...`; in that locked case there is no safe workaround (any periodic
    // PTY byte injection would corrupt editors like vim), so we leave it be.
    // Leading/trailing newlines make it a standalone line, not merged with the
    // prompt.
    if suppress_tmout {
        let _ = channel.data(&b"\nexport TMOUT=0 2>/dev/null\n"[..]).await;
    }

    let mut mode = TermMode::Normal;
    // Post-ZMODEM suppression window. After a ZMODEM session ends (normally
    // or via abort), lrzsz emits protocol tail bytes: CAN bursts, error text
    // ("Transfer incomplete"), ZFERR/ZFIN frames, and "OO". All of this is
    // noise that would render as terminal garbage. We suppress ALL incoming
    // data until the deadline passes (500 ms covers lrzsz's full abort
    // response; the shell prompt appears later once the process exits).
    let mut suppress_until: Option<Instant> = None;

    // Native ZMODEM receiver — active only for downloads (remote `sz`).
    // When Some, incoming data is fed to the receiver instead of being
    // emitted as `zmodem_raw` to the JS bridge. Uploads (remote `rz`) stay
    // on the JS zmodem.js path (receiver is None).
    //
    // The receiver does NO disk I/O itself — it only decodes and collects
    // payload into pending_write. After each feed(), the reader drains it
    // onto an unbounded channel; a background task owns the actual file
    // writes via spawn_blocking. This keeps the reader loop (and thus
    // russh's connection task, which awaits a bounded send per inbound
    // packet) free of disk work, so WINDOW_ADJUST flows at line rate.
    let mut zmodem_rx: Option<ZmodemReceiver> = None;
    // Channel carrying write jobs to the background disk task.
    // Each job is either a new file handle (FileOpen) or a data buffer (Write).
    let (disk_tx, mut disk_rx): (
        tokio::sync::mpsc::UnboundedSender<DiskJob>,
        tokio::sync::mpsc::UnboundedReceiver<DiskJob>,
    ) = mpsc::unbounded_channel();
    let disk_sink = Arc::clone(&sink);
    let disk_sid = session_id.clone();
    tokio::spawn(async move {
        use std::io::Write;
        let mut file: Option<std::fs::File> = None;
        while let Some(job) = disk_rx.recv().await {
            match job {
                DiskJob::Open(f) => {
                    file = Some(f);
                }
                DiskJob::Write(buf) => {
                    if let Some(ref mut f) = file {
                        // This runs on its OWN task, not the reader loop, so a
                        // blocking write only stalls THIS task — the reader
                        // keeps draining russh's queue and emitting
                        // WINDOW_ADJUST at line rate.
                        if let Err(e) = f.write_all(&buf) {
                            log::warn!("[ssh:{}] disk write failed: {}", disk_sid, e);
                        }
                    }
                }
                DiskJob::Flush => {
                    if let Some(ref mut f) = file {
                        let _ = f.flush();
                    }
                }
                DiskJob::Close => {
                    if let Some(ref mut f) = file {
                        let _ = f.flush();
                    }
                    file = None;
                }
            }
        }
        let _ = disk_sink;
    });

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
                        log::warn!("[ssh:{}] data send failed: {}", session_id, e);
                        break;
                    }
                }
                Some(SessionCommand::Resize { cols, rows }) => {
                    if let Err(e) = channel.window_change(cols, rows, 0, 0).await {
                        log::warn!("[ssh:{}] window_change failed: {}", session_id, e);
                    }
                }
                Some(SessionCommand::ZmodemBytes(data)) => {
                    if let Err(e) = channel.data(&data[..]).await {
                        log::warn!("[ssh:{}] zmodem send failed: {}", session_id, e);
                    }
                }
                Some(SessionCommand::ZmodemAbort) => {
                    // lrzsz abort sequence: 8× CAN + a few backspaces to clean
                    // the remote's PTY line state.
                    let abort = [CAN; 8];
                    let _ = channel.data(&abort[..]).await;
                    let cleanup: &[u8] = b"\x08\x08\x08\x08\x08\x08\x08\x08";
                    let _ = channel.data(cleanup).await;
                    // Close the disk task so any pending file is flushed.
                    let _ = disk_tx.send(DiskJob::Close);
                    // Immediately switch to Normal + suppress post-abort noise
                    // (CAN bursts, error text, ZFERR, OO) for 500 ms.
                    mode = TermMode::Normal;
                    suppress_until = Some(Instant::now() + Duration::from_millis(500));
                    sink.emit("zmodem_end", &session_id);
                    // Drop any native receiver so a subsequent transfer starts clean.
                    zmodem_rx = None;
                }
                Some(SessionCommand::ZmodemFinish) => {
                    // Frontend (zmodem.js) is the authority on session end.
                    // Switch back to Normal + suppress trailing protocol noise.
                    if mode == TermMode::Zmodem {
                        mode = TermMode::Normal;
                        suppress_until = Some(Instant::now() + Duration::from_millis(500));
                        sink.emit("zmodem_end", &session_id);
                    }
                    let _ = disk_tx.send(DiskJob::Close);
                    zmodem_rx = None;
                }
                Some(SessionCommand::ZmodemAcceptOffer { path }) => {
                    // Native receiver: frontend chose a save path (or skipped).
                    if let Some(rx) = zmodem_rx.as_mut() {
                        let (actions, file) = rx.accept_offer(&path);
                        // Hand the file handle to the background disk task.
                        if let Some(f) = file {
                            let _ = disk_tx.send(DiskJob::Open(f));
                        }
                        let (send, ended, disk) = dispatch_rx_actions(&*sink, &session_id, actions);
                        if disk.flush {
                            let _ = disk_tx.send(DiskJob::Flush);
                        }
                        if !send.is_empty() {
                            if let Err(e) = channel.data(&send[..]).await {
                                log::warn!("[ssh:{}] zmodem accept send failed: {}", session_id, e);
                            }
                        }
                        if ended {
                            if disk.close {
                                let _ = disk_tx.send(DiskJob::Close);
                            }
                            mode = TermMode::Normal;
                            suppress_until = Some(Instant::now() + Duration::from_millis(500));
                            zmodem_rx = None;
                        }
                    }
                }
                Some(SessionCommand::Disconnect) | None => {
                    let _ = channel.close().await;
                    break;
                }
            },

            // Incoming SSH messages from the server.
            msg = channel.wait() => match msg {
                Some(ChannelMsg::Data { ref data }) => {
                    log::debug!("[ssh:{}] Data {} bytes", session_id, data.len());
                    // Post-ZMODEM suppression: after a session ends (abort or
                    // normal ZFIN), lrzsz emits CAN bursts, error text, ZFERR
                    // frames, and "OO". Suppress ALL incoming data until the
                    // deadline passes so none of it renders as terminal garbage.
                    let data: &[u8] = if let Some(deadline) = suppress_until {
                        if Instant::now() < deadline {
                            &[][..] // still suppressing — discard
                        } else {
                            suppress_until = None;
                            data
                        }
                    } else {
                        data
                    };
                    if !data.is_empty() {
                        let to_send = handle_incoming_data(
                            &*sink, &session_id, data, &mut buffer, &mut last_flush,
                            &mut mode, &mut suppress_until, &mut zmodem_rx, &disk_tx,
                        );
                        // Drain decoded payload from the receiver to the
                        // background disk task — this is just an unbounded
                        // channel send, so the reader never blocks on disk I/O.
                        if let Some(ref mut rx) = zmodem_rx {
                            let pending = rx.take_pending_write();
                            if !pending.is_empty() {
                                let _ = disk_tx.send(DiskJob::Write(pending));
                            }
                        }
                        // Native receiver protocol responses (ZRINIT/ZRPOS/ZACK/
                        // ZFIN) go straight back to the peer — no JS round-trip.
                        if !to_send.is_empty() {
                            if let Err(e) = channel.data(&to_send[..]).await {
                                log::warn!("[ssh:{}] zmodem rx send failed: {}", session_id, e);
                            }
                        }
                    }
                }
                Some(ChannelMsg::ExtendedData { ref data, .. }) => {
                    log::debug!("[ssh:{}] ExtendedData {} bytes", session_id, data.len());
                    // stderr — treat as ordinary terminal output, never as ZMODEM.
                    append_capped(&*sink, &session_id, &mut buffer, &data[..], &mut last_flush);
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    log::info!("[ssh:{}] ExitStatus={}", session_id, exit_status);
                    sink.emit_raw("ssh_exit", serde_json::json!({
                        "sessionId": session_id,
                        "code": exit_status
                    }));
                }
                Some(ChannelMsg::Eof) => {
                    log::info!("[ssh:{}] EOF", session_id);
                    flush_buffer(&*sink, &session_id, &mut buffer);
                    if mode == TermMode::Zmodem {
                        sink.emit("zmodem_end", &session_id);
                    }
                    sink.emit("ssh_closed", &session_id);
                    break;
                }
                Some(ChannelMsg::Close) => {
                    log::info!("[ssh:{}] Close", session_id);
                    flush_buffer(&*sink, &session_id, &mut buffer);
                    if mode == TermMode::Zmodem {
                        sink.emit("zmodem_end", &session_id);
                    }
                    sink.emit("ssh_closed", &session_id);
                    break;
                }
                None => {
                    log::info!("[ssh:{}] channel.wait returned None", session_id);
                    flush_buffer(&*sink, &session_id, &mut buffer);
                    if mode == TermMode::Zmodem {
                        sink.emit("zmodem_end", &session_id);
                    }
                    sink.emit("ssh_closed", &session_id);
                    break;
                }
                Some(other) => {
                    log::debug!("[ssh:{}] other msg: {:?}", session_id, std::mem::discriminant(&other));
                }
            },

            // Periodic flush so terminal output reaches the UI even when the
            // server trickles bytes. Coalesces bursts and mitigates tauri#13234.
            // In Zmodem mode, flush as zmodem_raw (coalesced protocol bytes).
            _ = flush_interval.tick() => {
                if !buffer.is_empty() && last_flush.elapsed() >= Duration::from_millis(16) {
                    if mode == TermMode::Zmodem {
                        flush_zmodem_buffer(&*sink, &session_id, &mut buffer);
                    } else {
                        flush_buffer(&*sink, &session_id, &mut buffer);
                    }
                    last_flush = Instant::now();
                }
            }
        }
    }

    // NOTE: we intentionally do NOT remove the session from AppState here.
    // The shell channel closing (server EOF / `exit` / kill) is not the same
    // as the SSH *connection* dying — russh multiplexes many channels over a
    // single connection, so SFTP and exec can still open fresh channels on
    // the same `Arc<Handle>`. This matters for SFTP-only accounts whose login
    // shell exits immediately (ExitStatus=1, nologin/chroot): tearing the
    // session down here would drop that handle and make every later SFTP op
    // fail with "SSH session not found". Removal happens only on an explicit
    // `ssh_disconnect` (see `disconnect()`). A connection that is genuinely
    // dead lingers in the map until the user closes the tab, which is fine —
    // the terminal already shows "[Connection closed]" and SFTP ops on a dead
    // handle return a clean error instead of crashing.
    log::info!("[ssh:{}] channel_reader exited", session_id);
}

/// Routes incoming PTY bytes: terminal output in Normal mode, ZMODEM frames in
/// Zmodem mode. The Normal→Zmodem transition happens when we spot the auto-start
/// sequence; direction is then probed from the first frame:
///   - ZRQINIT (remote `sz`) → native Rust receiver (zero-IPC data path)
///   - otherwise (remote `rz`) → passthrough to the JS zmodem.js bridge
///
/// Returns bytes that must be sent back to the SSH peer (protocol responses
/// from the native receiver). The caller forwards them via `channel.data()`.
fn handle_incoming_data(
    sink: &dyn EventSink,
    session_id: &str,
    data: &[u8],
    buffer: &mut Vec<u8>,
    last_flush: &mut Instant,
    mode: &mut TermMode,
    suppress_until: &mut Option<Instant>,
    zmodem_rx: &mut Option<ZmodemReceiver>,
    disk_tx: &tokio::sync::mpsc::UnboundedSender<DiskJob>,
) -> Vec<u8> {
    match *mode {
        TermMode::Normal => {
            // Append to the shared terminal/ZMODEM-scan buffer. If no needle is
            // found the existing 16ms tick will coalesce-flush it as terminal
            // output via append_capped's threshold logic.
            append_capped(sink, session_id, buffer, data, last_flush);

            // Re-scan the full buffer for the ZMODEM auto-start sequence. It can
            // land anywhere — typically right after a prompt — so we can't only
            // look at the tail.
            if let Some(idx) = find_zmodem_start(buffer) {
                // Split: bytes [0..idx] stay as terminal output; bytes [idx..]
                // become the first ZMODEM frame.
                let tail: Vec<u8> = buffer[idx..].to_vec();
                buffer.truncate(idx);
                strip_zmodem_autostart_noise(buffer);
                flush_buffer(sink, session_id, buffer);

                // Probe direction with a native receiver.
                let mut rx = ZmodemReceiver::new();
                let actions = rx.feed(&tail);

                if let Some(passthrough_bytes) = rx.take_passthrough() {
                    // Upload (remote `rz`) — hand off to the JS zmodem.js path.
                    *mode = TermMode::Zmodem;
                    *zmodem_rx = None;
                    sink.emit(
                        "zmodem_start",
                        &ZmodemStartPayload {
                            session_id: session_id.to_string(),
                            direction: "upload",
                        },
                    );
                    sink.emit(
                        "zmodem_raw",
                        &SshOutputPayload {
                            session_id: session_id.to_string(),
                            data: passthrough_bytes,
                        },
                    );
                    return Vec::new();
                }

                // Download (remote `sz`) — native receiver active.
                *mode = TermMode::Zmodem;
                if rx.take_start_signal() {
                    sink.emit(
                        "zmodem_start",
                        &ZmodemStartPayload {
                            session_id: session_id.to_string(),
                            direction: "download",
                        },
                    );
                }
                let (send, ended, disk) = dispatch_rx_actions(sink, session_id, actions);
                if disk.flush || disk.close {
                    let pending = rx.take_pending_write();
                    if !pending.is_empty() {
                        let _ = disk_tx.send(DiskJob::Write(pending));
                    }
                }
                if disk.flush {
                    let _ = disk_tx.send(DiskJob::Flush);
                }
                if ended {
                    if disk.close {
                        let _ = disk_tx.send(DiskJob::Close);
                    }
                    *mode = TermMode::Normal;
                    *suppress_until = Some(Instant::now() + Duration::from_millis(500));
                    *zmodem_rx = None;
                    sink.emit("zmodem_end", &session_id.to_string());
                } else {
                    *zmodem_rx = Some(rx);
                }
                return send;
            }
            Vec::new()
        }
        TermMode::Zmodem => {
            if let Some(rx) = zmodem_rx.as_mut() {
                // Native download path — data goes straight to disk, no JS IPC.
                let actions = rx.feed(data);
                if rx.take_start_signal() {
                    sink.emit(
                        "zmodem_start",
                        &ZmodemStartPayload {
                            session_id: session_id.to_string(),
                            direction: "download",
                        },
                    );
                }
                let (send, ended, disk) = dispatch_rx_actions(sink, session_id, actions);
                // CRITICAL: drain any pending decoded payload BEFORE sending
                // Flush/Close. ZEOF triggers disk.flush, but the last
                // subpacket's bytes are still in pending_write — if we flush
                // before draining, the file is truncated and the transfer
                // hangs because sz never sees its ZEOF acknowledged correctly.
                if disk.flush || disk.close {
                    let pending = rx.take_pending_write();
                    if !pending.is_empty() {
                        let _ = disk_tx.send(DiskJob::Write(pending));
                    }
                }
                if disk.flush {
                    let _ = disk_tx.send(DiskJob::Flush);
                }
                if ended {
                    if disk.close {
                        let _ = disk_tx.send(DiskJob::Close);
                    }
                    *mode = TermMode::Normal;
                    *suppress_until = Some(Instant::now() + Duration::from_millis(500));
                    *zmodem_rx = None;
                    // Tell the frontend the session is over so the progress
                    // overlay hides. Without this the UI stays at 100% forever.
                    sink.emit("zmodem_end", &session_id.to_string());
                }
                return send;
            }

            // Passthrough (upload) path — coalesce and emit zmodem_raw to the
            // JS bridge. ZMODEM frames are self-delimiting (ZDLE-escaped), so
            // zmodem.js's consume() parses merged data correctly.
            buffer.extend_from_slice(data);
            if buffer.len() >= FLUSH_THRESHOLD {
                flush_zmodem_buffer(sink, session_id, buffer);
                *last_flush = Instant::now();
            }
            if is_zmodem_end(data) {
                flush_zmodem_buffer(sink, session_id, buffer);
                *mode = TermMode::Normal;
                *suppress_until = Some(Instant::now() + Duration::from_millis(500));
                sink.emit("zmodem_end", &session_id);
            }
            Vec::new()
        }
    }
}

/// Emit a native receiver's frontend events and collect the bytes that must be
/// sent back to the SSH peer. Returns `(send_bytes, session_ended)`.
/// What the reader should do with the disk task after dispatching events.
#[derive(Default)]
struct DiskSignal {
    flush: bool,
    close: bool,
}

fn dispatch_rx_actions(
    sink: &dyn EventSink,
    session_id: &str,
    actions: RxActions,
) -> (Vec<u8>, bool, DiskSignal) {
    let mut ended = false;
    let mut disk = DiskSignal::default();
    for event in actions.events {
        match event {
            RxEvent::Offer { name, size } => {
                sink.emit_raw(
                    "zmodem_offer",
                    serde_json::json!({
                        "sessionId": session_id,
                        "fileName": name,
                        "fileSize": size,
                    }),
                );
            }
            RxEvent::Progress { written, total } => {
                sink.emit_raw(
                    "zmodem_progress",
                    serde_json::json!({
                        "sessionId": session_id,
                        "bytesTransferred": written,
                        "bytesTotal": total,
                    }),
                );
            }
            RxEvent::FileComplete { name, written } => {
                // The file is done — flush the disk task so data lands before
                // we move to the next file or session end.
                disk.flush = true;
                sink.emit_raw(
                    "zmodem_file_complete",
                    serde_json::json!({
                        "sessionId": session_id,
                        "fileName": name,
                        "bytesWritten": written,
                    }),
                );
            }
            RxEvent::SessionEnd => {
                disk.close = true;
                ended = true;
            }
            RxEvent::Error(msg) => {
                disk.close = true;
                log::warn!("[ssh:{}] zmodem rx error: {}", session_id, msg);
                sink.emit_raw(
                    "zmodem_error",
                    serde_json::json!({
                        "sessionId": session_id,
                        "message": msg,
                    }),
                );
                ended = true;
            }
        }
    }
    (actions.send, ended, disk)
}

fn flush_buffer(sink: &dyn EventSink, session_id: &str, buffer: &mut Vec<u8>) {
    if buffer.is_empty() {
        return;
    }
    let payload = SshOutputPayload {
        session_id: session_id.to_string(),
        data: std::mem::take(buffer),
    };
    log::debug!("[ssh:{}] flush {} bytes → ssh_output", session_id, payload.data.len());
    sink.emit("ssh_output", &payload);
}

/// Flush the shared buffer as `zmodem_raw` — used while in TermMode::Zmodem.
/// Same buffer as flush_buffer but emits on the zmodem_raw channel so the
/// frontend's ZmodemBridge.feed() receives it instead of xterm.js.
fn flush_zmodem_buffer(sink: &dyn EventSink, session_id: &str, buffer: &mut Vec<u8>) {
    if buffer.is_empty() {
        return;
    }
    let payload = SshOutputPayload {
        session_id: session_id.to_string(),
        data: std::mem::take(buffer),
    };
    log::debug!("[ssh:{}] flush {} bytes → zmodem_raw", session_id, payload.data.len());
    sink.emit("zmodem_raw", &payload);
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
    sink: &dyn EventSink,
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
        flush_buffer(sink, session_id, buffer);
        buffer.extend_from_slice(TRUNCATION_MARKER);
        flush_buffer(sink, session_id, buffer);
        *last_flush = Instant::now();
        return;
    }
    if buffer.len() >= FLUSH_THRESHOLD {
        flush_buffer(sink, session_id, buffer);
        *last_flush = Instant::now();
    }
}

pub async fn send_input(
    state: &AppState,
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
    state: &AppState,
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
    state: &AppState,
    session_id: &str,
) -> Result<(), String> {
    // This is the single point that removes an SSH session from the map.
    // The channel_reader no longer removes on shell-channel close (see the
    // note there): the shell channel dying is not the same as the SSH
    // connection dying, and tearing the session down on shell close broke
    // SFTP for SFTP-only accounts whose shell exits right after connect.
    // Pulling the entry out drops the SshSession — and with it the
    // `Arc<Handle>`, which closes the SSH connection once any in-flight SFTP
    // clone of that Arc releases.
    let session = {
        let mut sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        sessions.remove(session_id)
    };

    if let Some(session) = session {
        // Signal the reader to close the shell channel gracefully. Best-effort:
        // for an SFTP-only account the reader already exited, so this send is a
        // no-op. `session` is dropped at end of scope, dropping the Arc<Handle>.
        let _ = session.command_tx.send(SessionCommand::Disconnect);
    }

    Ok(())
}

/// Push bytes produced by the frontend's zmodem.js (ZRINIT/ZRPOS/ZDATA acks,
/// file payload during upload) into the SSH channel.
pub async fn zmodem_send_bytes(
    state: &AppState,
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
    state: &AppState,
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

/// Frontend signals the ZMODEM session is definitively over. Switches the
/// reader task's TermMode back to Normal so subsequent SSH data renders as
/// terminal output. Called by the bridge on session_end (zmodem.js parsed
/// ZFIN) and on user abort — this is the AUTHORITATIVE end signal.
pub async fn zmodem_finish(
    state: &AppState,
    session_id: &str,
) -> Result<(), String> {
    let sender = {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        sessions
            .get(session_id)
            .map(|s| s.command_tx.clone())
    };

    if let Some(tx) = sender {
        let _ = tx.send(SessionCommand::ZmodemFinish);
    }

    Ok(())
}

/// Native ZMODEM receiver: the frontend chose a save path for the offered file
/// (or `None` to skip). Forwards the decision to the reader task's receiver.
pub async fn zmodem_accept_offer(
    state: &AppState,
    session_id: &str,
    path: Option<String>,
) -> Result<(), String> {
    let sender = {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        sessions
            .get(session_id)
            .map(|s| s.command_tx.clone())
    };

    if let Some(tx) = sender {
        let _ = tx.send(SessionCommand::ZmodemAcceptOffer { path });
    }

    Ok(())
}

/// Heuristic: does this russh error string indicate the underlying SSH
/// transport is dead (so a reconnect is worth trying)? russh surfaces these
/// as stringified errors — we match on substrings because the error types
/// aren't all public.
fn is_connection_dead(err: &str) -> bool {
    err.contains("ConnectFailed")
        || err.contains("disconnect")
        || err.contains("Connection reset")
        || err.contains("broken pipe")
        || err.contains("channel open")
        || err.contains("Session not found")
}

/// Open a new session channel on the existing handle. Extracted so exec_once
/// can retry it after a reconnect without duplicating the handle-lookup logic.
async fn open_exec_channel(
    state: &AppState,
    session_id: &str,
) -> Result<russh::Channel<russh::client::Msg>, String> {
    let handle = {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        sessions
            .get(session_id)
            .map(|s| Arc::clone(&s.handle))
            .ok_or_else(|| "Session not found".to_string())?
    };
    handle
        .channel_open_session()
        .await
        .map_err(|e| format!("Open exec channel failed: {}", e))
}

/// Re-establish the SSH connection for an existing session: dial + auth using
/// the stored config, then replace the handle in the sessions map. The
/// interactive PTY channel on the OLD handle is abandoned (its reader task
/// will observe the disconnect and emit ssh_closed — the frontend already
/// handles that by marking the tab disconnected, and our fresh handle keeps
/// exec_once working). Does NOT touch command_tx — the reader task for the
/// old channel winds down on its own.
async fn reconnect_session(state: &AppState, session_id: &str) -> Result<(), String> {
    let config = {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        let s = sessions
            .get(session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        Arc::clone(&s.config)
    };
    log::info!("[ssh:{}] reconnecting for exec_once (dial+auth)", session_id);
    let new_handle = dial_and_authenticate(state, &config, false).await?;
    // Replace the handle in the map. The old handle's Arc may still be held by
    // the reader task — that's fine, it'll drop when the reader exits.
    let mut sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
    if let Some(session) = sessions.get_mut(session_id) {
        session.handle = Arc::new(new_handle);
        log::info!("[ssh:{}] reconnect succeeded, handle replaced", session_id);
        Ok(())
    } else {
        // Session was removed concurrently — nothing to update, but the dial
        // already succeeded so we just drop the new handle.
        Err("Session removed during reconnect".to_string())
    }
}

/// Open a fresh exec channel on the existing SSH session, run `command`, and
/// collect stdout. Used for one-shot probes like server-info gathering. The
/// interactive PTY channel is untouched — russh multiplexes channels over the
/// same connection. Caller is responsible for timeout if needed.
pub async fn exec_once(
    state: &AppState,
    session_id: &str,
    command: &str,
) -> Result<String, String> {
    // First attempt with the current handle. On a connection-dead error
    // (ConnectFailed / channel open fail), transparently reconnect once:
    // dial + auth with the session's stored config, replace the handle in the
    // sessions map, then retry the channel open + exec. This makes one-shot
    // probes (AI health inspection, server-info) resilient to idle-disconnect
    // without forcing the user to manually reconnect the tab.
    let mut channel = match open_exec_channel(state, session_id).await {
        Ok(ch) => ch,
        Err(first_err) if is_connection_dead(&first_err) => {
            log::warn!(
                "[ssh:{}] exec channel open failed ({}), attempting one-time reconnect",
                session_id, first_err
            );
            // Reconnect: dial + auth + replace handle. Reuses the stored config.
            reconnect_session(state, session_id).await?;
            // Retry the channel open with the fresh handle.
            open_exec_channel(state, session_id)
                .await
                .map_err(|e| {
                    log::warn!("[ssh:{}] exec channel open failed after reconnect: {}", session_id, e);
                    format!("Open exec channel failed: {}", e)
                })?
        }
        Err(e) => {
            log::warn!("[ssh:{}] exec channel open failed: {}", session_id, e);
            return Err(format!("Open exec channel failed: {}", e));
        }
    };
    channel
        .exec(true, command)
        .await
        .map_err(|e| {
            log::warn!("[ssh:{}] exec failed: {}", session_id, e);
            format!("exec failed: {}", e)
        })?;

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
