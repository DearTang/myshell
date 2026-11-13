use crate::{AppState, ConnectionConfig, EventSink, EventSinkExt};
use crate::proxy;
use crate::zmodem_rx::{ZmodemReceiver, RxActions, RxEvent};
use crate::zmodem_tx::{ZmodemSender, TxEvent};

/// A queued file for native multi-file ZMODEM upload.
struct UploadFile {
    path: String,
    name: String,
    size: u64,
    mtime: u64,
}

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

/// Hard ceiling on bytes handed to the disk task but not yet written.
///
/// The channel is unbounded, so a fast link feeding a slow disk (network
/// share, USB stick, spinning rust) used to grow the queue without limit —
/// tens of MB per second, never draining, until the process died. Exceeding
/// the ceiling is reported as a failed transfer, which is far better than an
/// OOM, and is orders of magnitude above any legitimate in-flight backlog.
const DISK_QUEUE_MAX_BYTES: usize = 64 * 1024 * 1024;

/// The reader's handle to the background disk task: the job queue plus the two
/// pieces of shared state the task writes back.
#[derive(Clone)]
struct DiskSink {
    tx: tokio::sync::mpsc::UnboundedSender<DiskJob>,
    /// The FIRST disk failure (write / flush / queue overflow).
    ///
    /// The receiver's ACK/ZRPOS path is driven by its own byte counter and is
    /// completely independent of the disk, so a write failure used to be a bare
    /// `log::warn!` while `sz` kept receiving acknowledgements for data that
    /// never reached the platter. The session then finished "successfully" and
    /// `zmodem_file_complete` reported the counter — not the file — so a
    /// truncated (ENOSPC, yanked USB, permission change) file looked perfect.
    err: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// Bytes queued but not yet written, for the ceiling above.
    queued: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl DiskSink {
    fn new() -> (Self, tokio::sync::mpsc::UnboundedReceiver<DiskJob>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<DiskJob>();
        (
            DiskSink {
                tx,
                err: std::sync::Arc::new(std::sync::Mutex::new(None)),
                queued: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            },
            rx,
        )
    }

    /// Queue a job, accounting for Write payloads against the ceiling.
    /// Returns false once the transfer has been marked failed — callers stop
    /// feeding the queue.
    fn send(&self, job: DiskJob) -> bool {
        use std::sync::atomic::Ordering;
        if let DiskJob::Write(ref buf) = job {
            let now = self.queued.fetch_add(buf.len(), Ordering::AcqRel) + buf.len();
            if now > DISK_QUEUE_MAX_BYTES {
                self.queued.fetch_sub(buf.len(), Ordering::AcqRel);
                self.fail(format!(
                    "写入队列积压超过 {} MB（磁盘写入速度跟不上网络），已中止传输以避免内存耗尽",
                    DISK_QUEUE_MAX_BYTES / (1024 * 1024)
                ));
                return false;
            }
        }
        if self.tx.send(job).is_err() {
            return false;
        }
        true
    }

    /// Record the first failure only — later ones are usually consequences.
    fn fail(&self, msg: String) {
        log::warn!("[zmodem] disk sink: {}", msg);
        if let Ok(mut slot) = self.err.lock() {
            if slot.is_none() {
                *slot = Some(msg);
            }
        }
    }

    fn failed(&self) -> Option<String> {
        self.err.lock().ok().and_then(|g| g.clone())
    }

    /// Called by the disk task once a Write payload has actually landed.
    fn written(&self, n: usize) {
        use std::sync::atomic::Ordering;
        self.queued.fetch_sub(n, Ordering::AcqRel);
    }

    /// Clear a failure before a new file starts.
    fn reset(&self) {
        if let Ok(mut slot) = self.err.lock() {
            *slot = None;
        }
    }
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
    /// Frontend attached its `ssh_output` listener for this session (sent via
    /// the `ssh_ready` command). Releases the reader's startup hold so the
    /// banner bytes captured during connect (MOTD / "Last login") are emitted
    /// instead of being dropped by the not-yet-subscribed webview.
    FrontendReady,
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
    /// Native ZMODEM sender: frontend selected local files to upload. The
    /// reader opens the first file, creates a `ZmodemSender`, and begins
    /// streaming. Remaining paths queue for multi-file batches.
    ZmodemStartUpload { paths: Vec<String> },
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
    /// Live SSH tunnels. Present so a server-initiated `forwarded-tcpip`
    /// channel (remote `-R` forward) can be routed to the rule that asked for
    /// it. `None` for connections that never use port forwarding.
    pub port_forwards: Option<Arc<crate::forward::ForwardManager>>,
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

    /// Server-initiated channel for a remote (`-R`) port forward: someone
    /// connected to the port the server is listening on for us.
    ///
    /// Routing is by `(connected_address, connected_port)` — the only pair
    /// the protocol gives us about which forward request the channel belongs
    /// to. An unmatched channel is closed rather than accepted-and-dropped:
    /// leaving it open would make the remote client hang until timeout.
    ///
    /// The pump runs as a detached task so this returns immediately and never
    /// stalls the session's read loop (a slow local target dial would
    /// otherwise freeze terminal output).
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        match self.port_forwards.as_ref() {
            Some(mgr) => mgr.handle_forwarded_channel(
                connected_address,
                connected_port,
                channel,
            ),
            None => {
                log::warn!(
                    "[ssh] forwarded-tcpip for {}:{} on a connection without \
                     port forwarding — dropping",
                    connected_address,
                    connected_port
                );
                let _ = channel.close().await;
            }
        }
        Ok(())
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
/// Note: CAN shares the value 0x18 with ZDLE, but in an abort context (≥5
/// consecutive bytes) it is unambiguously a cancel signal.
const CAN: u8 = 0x18;
/// ZPAD — ZMODEM frame padding byte ('*'), starts every frame header.
const ZPAD: u8 = b'*';

/// One-shot TMOUT suppression injected into the PTY right after the shell
/// starts (gated by the `suppress_tmout` connect flag). Extracted as a named
/// constant so a unit test can pin its shape: leading space (HISTCONTROL
/// isolation), `set +o/-o history` wrapper (un-set HISTCONTROL hosts), and a
/// `2>/dev/null` on EVERY command so non-bash login shells don't print
/// "Illegal option" garbage on connect.
const TMOUT_SUPPRESS_COMMAND: &[u8] =
    b"\n set +o history 2>/dev/null; export TMOUT=0 2>/dev/null; set -o history 2>/dev/null\n";
/// ZMODEM auto-start needles.
///
/// ONLY the canonical `ZPAD ZPAD ZDLE [ABC]` form is accepted. The bare
/// `\x18[ABC]` fallback that used to live here was removed deliberately: it
/// matched ANY byte stream containing ZDLE followed by 'A'/'B'/'C', which
/// includes the tail of a CAN abort burst, shell history, and binary output.
/// The observed symptoms were the upload chooser re-popping after a cancel
/// and the terminal freezing on arrow-key history lookup.
///
/// The fallback's original justification — "PTY echo can strip the leading
/// `**`" — does not hold: when the remote runs `rz`, the frames IT emits
/// always carry the full `ZPAD ZPAD ZDLE` prefix, and echo only affects what
/// WE type, never the peer's frames.
const ZMODEM_NEEDLES: [&[u8]; 3] = [
    b"**\x18A",
    b"**\x18B",
    b"**\x18C",
];

/// True when the buffer's tail looks like an interactive shell prompt.
///
/// Used to clear the post-cancel auto-start lockout: once rz has exited, the
/// remote prints a prompt (`user@host:~$ `, `# `, `> `) and a NEW `rz` typed by
/// the user must be honoured again. Deliberately narrow — matching too eagerly
/// would re-open the chooser on ordinary output.
fn looks_like_shell_prompt(buf: &[u8]) -> bool {
    // Only the tail matters, and only when it ends with the prompt's
    // trailing space (shells emit `$ ` / `# ` and then wait for input).
    let tail = &buf[buf.len().saturating_sub(64)..];
    let s = String::from_utf8_lossy(tail);
    let trimmed = s.trim_end();
    // Reject if the tail is mid-line (no trailing space in the raw bytes).
    if !buf.ends_with(b" ") && !buf.ends_with(b"\n") {
        return false;
    }
    trimmed.ends_with('$')
        || trimmed.ends_with('#')
        || trimmed.ends_with('>')
        || trimmed.ends_with("%") // zsh default
}

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
// `detect_direction` was removed: it had no call site. The direction is decided
// by the receiver's own handshake (`rx.take_passthrough()` /
// `rx.take_start_signal()`), and the ZMODEM start payload always carries an
// explicit direction. The old doc comment described a probe that no longer ran.

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
        // Every connection gets the manager so a remote forward can be
        // started on any of them later; connections that never use it simply
        // never see a `forwarded-tcpip` channel.
        port_forwards: Some(Arc::clone(&state.port_forwards)),
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

/// Open an interactive SSH session. `hold_startup` = GUI consumers (true):
/// the reader buffers early terminal data until the frontend signals
/// `FrontendReady`, so the login banner isn't emitted before the tab's
/// `ssh_output` listener exists. Headless consumers (CLI, MCP) pass false —
/// their sinks already exist, and holding would swallow ZMODEM protocol bytes.
pub async fn connect(
    state: &AppState,
    sink: Arc<dyn EventSink>,
    config: ConnectionConfig,
    hold_startup: bool,
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
        config: Arc::new(config),
        command_tx,
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
    let reader_handle = {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        sessions
            .get(&sid)
            .map(|s| Arc::clone(&s.handle))
            .ok_or_else(|| "Session not found after insert".to_string())?
    };

    tokio::spawn(async move {
        channel_reader(
            sink,
            reader_sid,
            channel,
            reader_handle,
            command_rx,
            suppress_tmout,
            hold_startup,
        )
        .await;
    });

    Ok(sid)
}

async fn channel_reader(
    sink: Arc<dyn EventSink>,
    session_id: String,
    mut channel: Channel<Msg>,
    handle: Arc<Handle<SshClient>>,
    mut command_rx: mpsc::UnboundedReceiver<SessionCommand>,
    suppress_tmout: bool,
    hold_startup: bool,
) {
    // Capture the channel id once. We use `handle.data(id, ...)` for upload
    // data instead of `channel.data(...)` so the pump's send doesn't share a
    // borrow with `channel.wait()` — avoiding a deadlock where channel.data()
    // blocks waiting for WINDOW_ADJUST while channel.wait() (the only way to
    // process that WINDOW_ADJUST) is starved by the pump.
    let channel_id = channel.id();
    let mut buffer: Vec<u8> = Vec::with_capacity(8192);
    let mut last_flush = Instant::now();
    let mut flush_interval = tokio::time::interval(Duration::from_millis(16));
    // First tick completes immediately; consume it so we don't flush an empty buffer.
    flush_interval.tick().await;

    // Spawn a dedicated sender task that drains a bounded mpsc and forwards
    // each batch to the SSH channel through a `ChannelTx` writer obtained via
    // `channel.make_writer_ext()`. This writer OWNS clones of the channel's
    // internal mpsc sender, window-size mutex, and Notify — so calling
    // `writer.write_all().await` does NOT borrow `channel` and therefore
    // never blocks `channel.wait()`. Crucially, `write_all` DOES apply the
    // real SSH send-window backpressure (poll_writable in russh's tx.rs
    // takes the window mutex and waits on Notify when the window is empty),
    // so the sender task naturally slows when the server hasn't ACKed.
    //
    // WINDOW_ADJUST is handled by russh's background connection task — it
    // updates the window-size mutex and notify_one()s the Notify, which
    // wakes any writer that's currently parked. No intervention from this
    // reader task or the sender task is needed.
    let writer = channel.make_writer_ext(None);
    // Channel capacity must stay large enough that the pump's send_data().await
    // NEVER blocks while inside the ChannelMsg::Data arm — blocking there stalls
    // the entire select! loop, preventing WINDOW_ADJUST from being processed,
    // which deadlocks the sender task (write_all awaits window → reader can't
    // process WINDOW_ADJUST → window never reopens). 64 is safe.
    let (data_tx, data_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let sender_sid = session_id.clone();
    let data_sender_task = tokio::spawn({
        async move {
            use tokio::io::AsyncWriteExt;
            let mut writer = writer;
            let mut rx = data_rx;
            while let Some(batch) = rx.recv().await {
                let t0 = Instant::now();
                if let Err(e) = writer.write_all(&batch[..]).await {
                    log::warn!("[ssh:{}] data sender task: write_all failed: {}", sender_sid, e);
                    break;
                }
                // NOTE: no flush() — russh 0.50's ChannelTx::poll_flush is a no-op
                // (returns Ready(Ok(())) unconditionally). write_all already pushes
                // each ChannelMsg::Data onto the channel's internal mpsc the moment
                // a permit is available; the background connection task drains it to
                // the TCP socket. Calling flush() here was pure async state-machine
                // overhead per batch with zero network effect.
                log::info!("[ssh:{}] [TX] stream: {} bytes in {:?} = {:.1} MB/s", sender_sid, batch.len(), t0.elapsed(), if t0.elapsed().as_secs_f64() > 0.0 { batch.len() as f64 / t0.elapsed().as_secs_f64() / (1024.0 * 1024.0) } else { 0.0 });
            }
            log::info!("[ssh:{}] data sender task exited", sender_sid);
        }
    });

    /// Hand a byte batch to the sender task. The mpsc has capacity 64, so
    /// `send` only awaits when the sender task has fallen behind — which
    /// means SSH backpressure is already engaged and the few ms wait is the
    /// pump slowing to line speed. Never blocks on network state.
    async fn send_data(data_tx: &tokio::sync::mpsc::Sender<Vec<u8>>, data: &[u8]) {
        if data_tx.send(Vec::from(data)).await.is_err() {
            // Sender task has exited (connection dropped). Log it: a silent
            // failure here is what made an earlier "bytes never reached the
            // remote" bug invisible in the logs.
            log::warn!("send_data: writer task gone, {} bytes dropped", data.len());
        }
    }
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
    // prompt. History isolation, two layers (same as the cwd probe in the
    // frontend): the leading SPACE covers HISTCONTROL=ignorespace, and the
    // `set +o history` / `set -o history` wrapper covers servers where
    // HISTCONTROL is unset — measured one such host where the space alone left
    // `export TMOUT=0` in the user's Up-arrow history. Every `set` also gets
    // its own `2>/dev/null`: on non-bash login shells (sh/dash/fish) the
    // history option doesn't exist and the bare `set` would print "Illegal
    // option" garbage into the user's terminal on every connect.
    if suppress_tmout {
        send_data(&data_tx, TMOUT_SUPPRESS_COMMAND).await;
    }

    // Startup hold (GUI only): the frontend registers its `ssh_output`
    // listener only after `ssh_connect` resolves AND React commits the tab's
    // TerminalPanel — but sshd streams the login banner ("Last login: …",
    // MOTD) within milliseconds of the shell request. Bytes emitted before
    // the listener exists are dropped by the webview, so the banner never
    // rendered (Xshell shows it; we showed nothing). Hold incoming terminal
    // data until `SessionCommand::FrontendReady` arrives (the frontend sends
    // it right after its listeners attach), then flush it as one `ssh_output`
    // batch and go live. Fallback deadline for a consumer that never signals
    // (version-skewed pairing): release late rather than never.
    let mut startup_hold = if hold_startup {
        Some(Vec::with_capacity(4096))
    } else {
        None
    };
    let hold_deadline = Instant::now() + Duration::from_secs(3);

    let mut mode = TermMode::Normal;
    // Post-ZMODEM suppression window. After a ZMODEM session ends (normally
    // or via abort), lrzsz emits protocol tail bytes: CAN bursts, error text
    // ("Transfer incomplete"), ZFERR/ZFIN frames, and "OO". All of this is
    // noise that would render as terminal garbage. We suppress ALL incoming
    // data until the deadline passes (500 ms covers lrzsz's full abort
    // response; the shell prompt appears later once the process exits).
    let mut suppress_until: Option<Instant> = None;

    // Set when the user cancels a ZMODEM session (ZmodemAbort). While true,
    // the auto-start scan refuses to re-enter Zmodem mode.
    //
    // Why a persistent flag instead of a longer suppress_until: rz does not
    // necessarily answer a CAN burst within any fixed deadline. Measured: the
    // reply arrived 18s after the abort, long past every window tried (0.5s,
    // 2s, 5s). When it lands it looks exactly like a fresh `rz` invocation —
    // same canonical `**\x18B` frame — so the scan re-enters Zmodem mode and
    // the chooser pops again. The user has already cancelled once, so they
    // will not cancel again; the terminal is stuck until reconnect.
    //
    // Cleared only by real user input or a shell prompt, i.e. evidence the
    // remote is back to an interactive shell and a NEW `rz` is plausible.
    let mut zmodem_cancelled = false;

    // Pending terminal-restore after a ZMODEM abort. `rz` sets termios to raw
    // mode (ECHO off, ICANON off) before reading; if it dies from our CAN burst
    // without running its cleanup, the remote tty stays raw: keystrokes stop
    // echoing, readline can't see complete lines, backspace/arrows misbehave,
    // and only Enter yields a prompt. That state is invisible to us — the
    // protocol layer is already clean — so we schedule a conditional repair:
    // after the grace period, if no shell prompt was seen, send Ctrl-C +
    // `stty sane`. Prompt detection is the gate that keeps a normally-exited
    // rz from getting a stray command injected.
    let mut tty_restore_deadline: Option<Instant> = None;
    // Set by the reader whenever a shell prompt is observed while a restore is
    // pending — that's proof the remote tty came back on its own, so the repair
    // command must not be injected.
    let mut tty_saw_prompt = false;

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

    // Native ZMODEM sender state (uploads). Symmetric to zmodem_rx: when
    // sender is Some, incoming rz bytes feed the sender and its output goes
    // straight to channel.data() with zero IPC. Bundled in TxSession to keep
    // the handle_incoming_data signature manageable.
    let mut tx_state = TxSession::new();
    // Queue for the background ZMODEM disk task. `DiskSink` owns the sender
    // plus the shared first-error slot and the in-flight byte count; the
    // receiver stays with the task.
    let (diskq, mut disk_rx) = DiskSink::new();
    let disk_sink = Arc::clone(&sink);
    let disk_errs = diskq.clone();
    let disk_queue = diskq.clone();
    tokio::spawn(async move {
        use std::io::Write;
        let mut file: Option<std::fs::File> = None;
        while let Some(job) = disk_rx.recv().await {
            match job {
                DiskJob::Open(f) => {
                    file = Some(f);
                }
                DiskJob::Write(buf) => {
                    let n = buf.len();
                    if let Some(ref mut f) = file {
                        // A failure here is NOT recoverable for this file and must
                        // not be swallowed: the receiver keeps ACKing on its own
                        // byte counter, so the session would otherwise complete
                        // "successfully" over a truncated file. Record it and let
                        // the reader abort.
                        if let Err(e) = f.write_all(&buf) {
                            disk_errs.fail(format!("写入文件失败（磁盘满 / 权限变更 / 设备拔出?）: {}", e));
                        }
                    } else {
                        disk_errs.fail("收到写入数据但没有打开的文件".to_string());
                    }
                    disk_queue.written(n);
                }
                DiskJob::Flush => {
                    if let Some(ref mut f) = file {
                        if let Err(e) = f.flush() {
                            disk_errs.fail(format!("落盘失败（磁盘空间不足?）: {}", e));
                        }
                    }
                }
                DiskJob::Close => {
                    if let Some(ref mut f) = file {
                        if let Err(e) = f.flush() {
                            disk_errs.fail(format!("落盘失败（磁盘空间不足?）: {}", e));
                        }
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

    let mut last_upload_error: Option<String> = None;
    // Last zmodem_progress emit for the native DOWNLOAD path (throttle state
    // for dispatch_rx_actions — symmetric to tx_state.last_progress_emit).
    let mut rx_last_progress: Option<Instant> = None;
    loop {
        // Promote any deferred upload-error cleanup into actual side-effects
        // BEFORE the select, so all borrows are fresh.
        if let Some(msg) = last_upload_error.take() {
            emit_zmodem_error(&*sink, &session_id, &msg);
            mode = TermMode::Normal;
            suppress_until = Some(Instant::now() + Duration::from_millis(500));
            tx_state.sender = None;
            tx_state.file = None;
            tx_state.queue.clear();
            emit_zmodem_end(&*sink, &session_id, "session ended");
            // Don't continue — fall through to select!; the new state is
            // already consistent.
        }

        tokio::select! {
            biased;

            // Commands from the frontend take priority so input latency stays low.
            cmd = command_rx.recv() => match cmd {
                Some(SessionCommand::Input(data)) => {
                    // Real user keystrokes mean the remote is interactive again
                    // — a subsequent `rz` typed by the user must work.
                    zmodem_cancelled = false;
                    // 只记字节数不记内容：sudo/passwd/mysql 的交互提示下，
                    // 用户敲的是密码 —— 秘密不分级，debug 也不落。
                    log::debug!("[ssh:{}] [IN] {} bytes", session_id, data.len());
                    send_data(&data_tx, &data[..]).await;
                }
                Some(SessionCommand::Resize { cols, rows }) => {
                    if let Err(e) = channel.window_change(cols, rows, 0, 0).await {
                        log::warn!("[ssh:{}] window_change failed: {}", session_id, e);
                    }
                }
                Some(SessionCommand::ZmodemBytes(data)) => {
                    send_data(&data_tx, &data[..]).await;
                }
                Some(SessionCommand::ZmodemAbort) => {
                    // lrzsz abort sequence: 8× CAN + a few backspaces to clean
                    // the remote's PTY line state.
                    //
                    // 顺序要紧：CAN 必须在清空 tx_state 之前发出去，且发送路上
                    // 不能有任何「丢弃后续批次」的开关。旧代码先在 reader 里
                    // store(true) 再 send，sender task 看到标志就把这 8 个 CAN
                    // 连同后续所有字节一起丢掉 —— 远端 rz 永远收不到取消，
                    // 继续发 ZRINIT，Rust 反复自动启动，前端 zmodem_start
                    // 无限循环（实测「rz -y 取消不可以退出，一直重复这个过程」，
                    // 日志 `sender task: aborted, dropping 8 bytes` 正是被丢
                    // 的 CAN）。同一标志还曾让 sender task 直接 break 退出，
                    // 之后用户敲的任何键都发不出去（「取消后无法输入命令」）。
                    let abort = [CAN; 8];
                    send_data(&data_tx, &abort[..]).await;
                    let cleanup: &[u8] = b"\x08\x08\x08\x08\x08\x08\x08\x08";
                    send_data(&data_tx, cleanup).await;
                    // Close the disk task so any pending file is flushed.
                    let _ = diskq.send(DiskJob::Close);
                    // Immediately switch to Normal + suppress post-abort noise
                    // (CAN bursts, error text, ZFERR, OO) for 2s. 500ms was
                    // too short: rz takes a moment to notice the CAN burst and
                    // exit, and whatever it prints on the way out (error text,
                    // the shell prompt) lands in Normal mode after the window
                    // expires. Bytes carrying `**\x18B` then re-trigger the
                    // auto-start scan and the chooser pops a second time.
                    mode = TermMode::Normal;
                    suppress_until = Some(Instant::now() + Duration::from_millis(2000));
                    // Refuse auto-start until the remote is demonstrably back
                    // at a shell prompt (or the user types). rz's reply to the
                    // CAN burst can arrive many seconds later — measured 18s —
                    // and it is byte-identical to a fresh `rz` invocation.
                    zmodem_cancelled = true;
                    // 1.2s 后若仍没见到 shell 提示符，说明 rz 死于我们的 CAN
                    // 而没跑自己的 termios 清理 —— 远端 tty 停在 raw 模式
                    // （不回显、readline 收不到整行、退格/方向键全乱，
                    // 只有回车能出东西）。见 tty_restore_deadline 的注释。
                    // 取 2.4s 而不是 1.2s：suppress_until 是 2s，早于它发送
                    // 的 stty sane 会被仍在退出的 rz 当成协议字节吃掉。
                    tty_restore_deadline = Some(Instant::now() + Duration::from_millis(2400));
                    tty_saw_prompt = false;
                    emit_zmodem_end(&*sink, &session_id, "aborted by user (CAN×8 sent)");
                    // Drop any native receiver/sender so a subsequent transfer starts clean.
                    zmodem_rx = None;
                    tx_state.sender = None;
                    tx_state.file = None;
                    tx_state.queue.clear();
                }
                Some(SessionCommand::ZmodemFinish) => {
                    // Frontend is the authority on session end. Switch back to
                    // Normal + suppress trailing protocol noise.
                    if mode == TermMode::Zmodem {
                        mode = TermMode::Normal;
                        suppress_until = Some(Instant::now() + Duration::from_millis(500));
                        emit_zmodem_end(&*sink, &session_id, "session ended");
                    }
                    let _ = diskq.send(DiskJob::Close);
                    zmodem_rx = None;
                    tx_state.sender = None;
                    tx_state.file = None;
                    tx_state.queue.clear();
                }
                Some(SessionCommand::ZmodemAcceptOffer { path }) => {
                    // Native receiver: frontend chose a save path (or skipped).
                    if let Some(rx) = zmodem_rx.as_mut() {
                        let (actions, file) = rx.accept_offer(&path);
                        // Hand the file handle to the background disk task.
                        if let Some(f) = file {
                            let _ = diskq.send(DiskJob::Open(f));
                        }
                        let (send, ended, disk_sig) =
                            dispatch_rx_actions(&*sink, &session_id, actions, &mut rx_last_progress, &diskq);
                        if disk_sig.flush {
                            let _ = diskq.send(DiskJob::Flush);
                        }
                        if !send.is_empty() {
                            send_data(&data_tx, &send[..]).await;
                        }
                        if ended {
                            if disk_sig.close {
                                let _ = diskq.send(DiskJob::Close);
                            }
                            mode = TermMode::Normal;
                            suppress_until = Some(Instant::now() + Duration::from_millis(500));
                            zmodem_rx = None;
                        }
                    }
                }
                Some(SessionCommand::ZmodemStartUpload { paths }) => {
                    // Native upload: frontend selected local files AND/OR
                    // folders. Folders are expanded recursively here; each
                    // file's ZFILE offer name is its path relative to the
                    // selected folder root ("root/sub/file") — lrzsz's rz
                    // recreates the directory tree from that path, which is
                    // how ZMODEM (a file-only protocol) carries a folder.
                    // Open the first, queue the rest, feed any buffered
                    // ZRINIT to kick off the handshake.
                    log::info!("[ssh:{}] ZmodemStartUpload: {} paths, pending={} bytes", session_id, paths.len(), tx_state.pending.len());
                    if paths.is_empty() {
                        log::warn!("[ssh:{}] zmodem_start_upload with empty paths", session_id);
                    } else {
                        let (mut files, walk_errors) = collect_upload_files(&paths);
                        for e in &walk_errors {
                            log::warn!("[ssh:{}] upload expansion: {}", session_id, e);
                        }
                        if let Some(first) = files.pop_front() {
                            tx_state.queue = files;
                            log::info!("[ssh:{}] [TX] first file: {:?} ({} bytes)", session_id, first.path, first.size);
                            match start_upload_file(&first) {
                                Ok((mut sender, file)) => {
                                    // Feed any buffered ZRINIT to start the handshake.
                                    let pending = std::mem::take(&mut tx_state.pending);
                                    log::info!("[ssh:{}] [TX] start_upload: pending={} bytes, sender state before feed={}", session_id, pending.len(), sender.state_name());
                                    let actions = if pending.is_empty() {
                                        sender.feed(&[])
                                    } else {
                                        sender.feed(&pending)
                                    };
                                    log::info!("[ssh:{}] [TX] start_upload: after feed, sender state={}, send={} bytes, events={}", session_id, sender.state_name(), actions.send.len(), actions.events.len());
                                    tx_state.sender = Some(sender);
                                    tx_state.file = Some(file);
                                    if !actions.send.is_empty() {
                                        send_data(&data_tx, &actions.send[..]).await;
                                    }
                                }
                                Err(e) => {
                                    emit_zmodem_error(&*sink, &session_id, &e);
                                }
                            }
                        } else {
                            // Selection expanded to nothing (e.g. only empty
                            // folders) — abort now so rz exits instead of
                            // waiting forever for an offer that never comes.
                            log::warn!("[ssh:{}] upload selection empty after expansion — aborting", session_id);
                            let detail = if walk_errors.is_empty() {
                                "所选内容没有可上传的文件（可能只包含空文件夹）".to_string()
                            } else {
                                walk_errors.join("; ")
                            };
                            emit_zmodem_error(&*sink, &session_id, &detail);
                            let abort = [CAN; 8];
                            send_data(&data_tx, &abort[..]).await;
                            mode = TermMode::Normal;
                            suppress_until = Some(Instant::now() + Duration::from_millis(500));
                            emit_zmodem_end(&*sink, &session_id, "session ended");
                            tx_state.sender = None;
                            tx_state.file = None;
                            tx_state.queue.clear();
                        }
                    }
                }
                Some(SessionCommand::FrontendReady) => {
                    // Release the startup hold: the tab's listeners are now
                    // attached, so the buffered banner can be delivered.
                    if let Some(held) = startup_hold.take() {
                        if !held.is_empty() {
                            sink.emit(
                                "ssh_output",
                                &SshOutputPayload {
                                    session_id: session_id.clone(),
                                    data: held,
                                },
                            );
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
                    // Startup hold: buffer instead of emitting (see the
                    // startup_hold comment above). Checked before ZMODEM
                    // handling — during login there is no ZMODEM traffic.
                    if let Some(held) = startup_hold.as_mut() {
                        // Cap bounds memory against a server that floods
                        // output before the frontend attaches (login scripts
                        // cat-ing huge files); past the cap, early bytes are
                        // dropped — same as the pre-fix behavior.
                        const MAX_HOLD_BYTES: usize = 1024 * 1024;
                        let space = MAX_HOLD_BYTES.saturating_sub(held.len());
                        held.extend_from_slice(&data[..data.len().min(space)]);
                        continue;
                    }
                    // Post-ZMODEM suppression: after a session ends (abort or
                    // normal ZFIN), lrzsz emits CAN bursts, error text, ZFERR
                    // frames, and "OO". Suppress ALL incoming data until the
                    // deadline passes so none of it renders as terminal garbage.
                    //
                    // 抑制之前先看原始字节：这段时间正是 rz 退出、shell
                    // 重新打印提示符的窗口，而 tty 恢复判定要靠它。若等数据
                    // 进了 handle_incoming_data 再检测，提示符早被这里丢掉了，
                    // 闸门永远不成立、每次都会多发一条 stty sane。
                    if tty_restore_deadline.is_some() && looks_like_shell_prompt(data) {
                        tty_saw_prompt = true;
                    }
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
                            &mut mode, &mut suppress_until, &mut zmodem_cancelled,
                            &mut zmodem_rx, &diskq,
                            &mut tx_state, &mut rx_last_progress, &mut tty_saw_prompt,
                        );
                        // Drain decoded payload from the receiver to the
                        // background disk task — this is just an unbounded
                        // channel send, so the reader never blocks on disk I/O.
                        if let Some(ref mut rx) = zmodem_rx {
                            let pending = rx.take_pending_write();
                            if !pending.is_empty() {
                                let _ = diskq.send(DiskJob::Write(pending));
                            }
                        }
                        // Native receiver protocol responses (ZRINIT/ZRPOS/ZACK/
                        // ZFIN) go straight back to the peer — no JS round-trip.
                        if !to_send.is_empty() {
                            send_data(&data_tx, &to_send[..]).await;
                        }
                        // Native upload: stream file data subpackets directly via
                        // channel.data(). INLINE (not a separate task): a pump task
                        // starved the PTY input buffer because it flooded rz's stdin
                        // faster than rz could drain — receiver saw byte loss and
                        // never stopped sending ZRPOS offset=0. The inline pump is
                        // naturally throttled by the reader's select! cadence.
                        //
                        // Batch subpackets into ~512 KB chunks: bigger than the
                        // old 128 KB (fewer channel.data() calls → less per-call
                        // SSH/PTY overhead → higher throughput), but still small
                        // enough that each await yields back to select! promptly,
                        // keeping the reader's other arms responsive.
                        if let Some(sender) = tx_state.sender.as_mut() {
                            let mut error_msg: Option<String> = None;
                            if let Some(file) = tx_state.file.as_mut() {
                                log::info!("[ssh:{}] [TX] pump enter: state={}", session_id, sender.state_name());
                                const INLINE_BATCH: usize = 512 * 1024;
                                // Cap batches per select! iteration. Without a
                                // cap, the pump drains the ENTIRE file in one
                                // pass (~6 ms for 10 MB), flooding the server's
                                // PTY kernel buffer before rz can drain — the
                                // kernel drops bytes, rz sees corrupt data,
                                // aborts with ZRPOS(0). 8 batches ≈ 4 MB per
                                // iteration keeps data flowing at network speed
                                // while yielding back to select! frequently
                                // enough to process WINDOW_ADJUST and user input.
                                const MAX_BATCHES_PER_ITER: usize = 8;
                                let mut batches_this_iter: usize = 0;
                                let mut batch: Vec<u8> = Vec::with_capacity(INLINE_BATCH);
                                let mut upload_error = false;
                                let mut total_sent: usize = 0;
                                let mut send_time: std::time::Duration = std::time::Duration::ZERO;
                                loop {
                                    let poll = sender.poll(file);
                                    if poll.send.is_empty() {
                                        break;
                                    }
                                    batch.extend_from_slice(&poll.send);
                                    let mut ended = false;
                                    let mut err: Option<String> = None;
                                    dispatch_tx_events_throttled(
                                        &*sink,
                                        &session_id,
                                        &poll.events,
                                        &mut ended,
                                        &mut err,
                                        &mut tx_state.last_progress_emit,
                                    );
                                    if let Some(msg) = err {
                                        // Defer cleanup to after the
                                        // sender/file borrows end below.
                                        upload_error = true;
                                        error_msg = Some(msg);
                                        break;
                                    }
                                    if ended {
                                        tx_state.file = None;
                                        break;
                                    }
                                    if batch.len() >= INLINE_BATCH {
                                        let t0 = Instant::now();
                                        send_data(&data_tx, &batch[..]).await;
                                        send_time += t0.elapsed();
                                        total_sent += batch.len();
                                        batch.clear();
                                        batches_this_iter += 1;
                                        if batches_this_iter >= MAX_BATCHES_PER_ITER {
                                            // Yield back to select! so it can
                                            // process WINDOW_ADJUST and other
                                            // messages. The next ChannelMsg::Data
                                            // from rz (or the 16ms flush tick)
                                            // will resume the pump.
                                            break;
                                        }
                                    }
                                }
                                if !upload_error && !batch.is_empty() {
                                    let t0 = Instant::now();
                                    send_data(&data_tx, &batch[..]).await;
                                    send_time += t0.elapsed();
                                    total_sent += batch.len();
                                }
                                if total_sent > 0 {
                                    log::info!("[ssh:{}] [TX] stream: {} bytes in {:?} = {:.1} MB/s", session_id, total_sent, send_time, if send_time.as_secs_f64() > 0.0 { total_sent as f64 / send_time.as_secs_f64() / (1024.0 * 1024.0) } else { 0.0 });
                                }

                                // Track when we entered WaitingZrinit2 (post-
                                // ZEOF). The reader-side timeout below forces a
                                // ZFIN if rz never acknowledges, which happens
                                // when its PTY enters an error state and
                                // streams garbage instead of a proper reply.
                                if sender.state_name() == "WaitingZrinit2" {
                                    if tx_state.waiting_zrinit2_since.is_none() {
                                        tx_state.waiting_zrinit2_since = Some(Instant::now());
                                    }
                                } else {
                                    tx_state.waiting_zrinit2_since = None;
                                }
                            }
                            // Deferred upload-error cleanup: capture the
                            // message here (after the inner pump borrows
                            // have ended) so the outer if-let-sender can
                            // release its borrow before we mutate
                            // tx_state.sender in the next-loop cleanup.
                            if let Some(msg) = error_msg.take() {
                                last_upload_error = Some(msg);
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
                        emit_zmodem_end(&*sink, &session_id, "session ended");
                    }
                    sink.emit("ssh_closed", &session_id);
                    break;
                }
                Some(ChannelMsg::Close) => {
                    log::info!("[ssh:{}] Close", session_id);
                    flush_buffer(&*sink, &session_id, &mut buffer);
                    if mode == TermMode::Zmodem {
                        emit_zmodem_end(&*sink, &session_id, "session ended");
                    }
                    sink.emit("ssh_closed", &session_id);
                    break;
                }
                None => {
                    log::info!("[ssh:{}] channel.wait returned None", session_id);
                    flush_buffer(&*sink, &session_id, &mut buffer);
                    if mode == TermMode::Zmodem {
                        emit_zmodem_end(&*sink, &session_id, "session ended");
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
                // Resume upload pump if sender is in SendingData. The pump
                // caps batches per select! iteration (MAX_BATCHES_PER_ITER)
                // to avoid PTY buffer overflow, so it needs to be re-triggered
                // by this periodic tick when rz sends no further data.
                if let Some(sender) = tx_state.sender.as_mut() {
                    if sender.state_name() == "SendingData" {
                        if let Some(file) = tx_state.file.as_mut() {
                            const INLINE_BATCH: usize = 512 * 1024;
                            const MAX_BATCHES_PER_ITER: usize = 8;
                            let mut batch: Vec<u8> = Vec::with_capacity(INLINE_BATCH);
                            let mut batches_this_iter: usize = 0;
                            loop {
                                let poll = sender.poll(file);
                                if poll.send.is_empty() { break; }
                                batch.extend_from_slice(&poll.send);
                                let mut ended = false;
                                let mut err: Option<String> = None;
                                dispatch_tx_events_throttled(
                                    &*sink, &session_id, &poll.events,
                                    &mut ended, &mut err, &mut tx_state.last_progress_emit,
                                );
                                if let Some(msg) = err {
                                    last_upload_error = Some(msg);
                                    break;
                                }
                                if ended { tx_state.file = None; break; }
                                if batch.len() >= INLINE_BATCH {
                                    send_data(&data_tx, &batch[..]).await;
                                    batch.clear();
                                    batches_this_iter += 1;
                                    if batches_this_iter >= MAX_BATCHES_PER_ITER { break; }
                                }
                            }
                            if !batch.is_empty() && last_upload_error.is_none() {
                                send_data(&data_tx, &batch[..]).await;
                            }
                            if sender.state_name() == "WaitingZrinit2" {
                                if tx_state.waiting_zrinit2_since.is_none() {
                                    tx_state.waiting_zrinit2_since = Some(Instant::now());
                                }
                            } else {
                                tx_state.waiting_zrinit2_since = None;
                            }
                        }
                    }
                }
                if !buffer.is_empty() && last_flush.elapsed() >= Duration::from_millis(16) {
                    if mode == TermMode::Zmodem {
                        flush_zmodem_buffer(&*sink, &session_id, &mut buffer);
                    } else {
                        flush_buffer(&*sink, &session_id, &mut buffer);
                    }
                    last_flush = Instant::now();
                }
                // Startup-hold fallback: a consumer that never signals
                // FrontendReady (version-skewed old frontend, unexpected path)
                // gets the banner after the deadline instead of never.
                if startup_hold.is_some() && Instant::now() >= hold_deadline {
                    if let Some(held) = startup_hold.take() {
                        if !held.is_empty() {
                            sink.emit(
                                "ssh_output",
                                &SshOutputPayload {
                                    session_id: session_id.clone(),
                                    data: held,
                                },
                            );
                        }
                    }
                }
                // Terminal-restore fallback after a ZMODEM abort. If the grace
                // period elapsed and no shell prompt arrived (prompt detection
                // clears the deadline), rz died from our CAN without restoring
                // termios — the remote tty is raw and every keystroke is
                // effectively lost. Ctrl-C first (中断可能残留的前台进程，并让
                // readline 放弃当前行), then `stty sane` (ECHO/ICANON/ISIG 恢复).
                // 前导空格：配了 HISTCONTROL=ignorespace 的机器上不进历史。
                if let Some(deadline) = tty_restore_deadline {
                    if Instant::now() >= deadline {
                        tty_restore_deadline = None;
                        if tty_saw_prompt {
                            log::info!(
                                "[ssh:{}] zmodem abort: shell prompt seen, tty looks healthy",
                                session_id
                            );
                        } else {
                            send_data(&data_tx, b"\x03").await;
                            send_data(&data_tx, b" stty sane\r").await;
                            log::warn!(
                                "[ssh:{}] zmodem abort: no prompt after grace period → sent Ctrl-C + stty sane",
                                session_id
                            );
                            sink.emit(
                                "ssh_output",
                                &SshOutputPayload {
                                    session_id: session_id.clone(),
                                    data: "\r\n\x1b[33m[ZMODEM] 已恢复远端终端设置（stty sane）\x1b[0m\r\n"
                                        .as_bytes()
                                        .to_vec(),
                                },
                            );
                        }
                        tty_saw_prompt = false;
                    }
                }
                // ZMODEM upload timeout: if the sender has been waiting in
                // WaitingZrinit2 (post-ZEOF) for too long without rz
                // acknowledging, force end the session. 30s covers large
                // files whose data is still flushing through the mpsc/sender
                // task to the network after poll() finished reading the file.
                if mode == TermMode::Zmodem {
                    if let Some(since) = tx_state.waiting_zrinit2_since {
                        if since.elapsed() > Duration::from_secs(30) {
                            log::warn!("[ssh:{}] zmodem upload timeout (30s in WaitingZrinit2, queue={}), forcing end", session_id, tx_state.queue.len());
                            // Synthesise ZFIN+OO so the session can close.
                            let zfin_bytes = if let Some(sender) = tx_state.sender.as_mut() {
                                Some(sender.force_finish_for_timeout())
                            } else {
                                None
                            };
                            if let Some(zfin) = zfin_bytes {
                                send_data(&data_tx, &zfin).await;
                            }
                            tx_state.waiting_zrinit2_since = None;
                            // Clean up and switch back to Normal so the UI
                            // stops showing the upload progress overlay.
                            // Suppress for 2s — rz may emit trailing text
                            // (progress, exit message) after the ZFIN.
                            mode = TermMode::Normal;
                            suppress_until = Some(Instant::now() + Duration::from_millis(2000));
                            tx_state.sender = None;
                            tx_state.file = None;
                            tx_state.queue.clear();
                            emit_zmodem_end(&*sink, &session_id, "session ended");
                            }
                        }
                    }
                // ZMODEM download idle timeout: a sender that died without a
                // CAN burst (killed by a signal, silent crash) would leave the
                // native receiver waiting forever — the terminal stays in
                // Zmodem mode and swallows all shell output, looking frozen.
                // 30s of total silence (exempting WaitingAccept, where the
                // save-directory picker can stay open indefinitely) means the
                // sender is gone.
                if mode == TermMode::Zmodem {
                    let rx_timed_out = zmodem_rx
                        .as_ref()
                        .map(|rx| rx.idle_timeout(Duration::from_secs(30)))
                        .unwrap_or(false);
                    if rx_timed_out {
                        log::warn!(
                            "[ssh:{}] zmodem rx idle timeout (30s without data), forcing end",
                            session_id
                        );
                        // Surface whatever terminal text arrived while the
                        // receiver still held the session in Zmodem mode —
                        // the dying sz's error output (and the shell prompt
                        // that followed) was swallowed into the rx buffer.
                        if let Some(rx) = zmodem_rx.as_mut() {
                            let tail = rx.drain_trailing();
                            if !tail.is_empty() {
                                sink.emit(
                                    "ssh_output",
                                    &SshOutputPayload {
                                        session_id: session_id.clone(),
                                        data: tail,
                                    },
                                );
                            }
                        }
                        // Abort sequence (same as the user-facing Cancel):
                        // if sz is still alive — hung on I/O, holding the PTY
                        // in raw mode — the 8×CAN makes lrzsz exit and restore
                        // the terminal. Without this the user cannot type
                        // anything after the error because every keystroke
                        // goes to the hung sz instead of the shell.
                        send_data(&data_tx, &[CAN; 8]).await;
                        send_data(&data_tx, b"\x08\x08\x08\x08\x08\x08\x08\x08").await;
                        emit_zmodem_error(
                            &*sink,
                            &session_id,
                            "ZMODEM 接收超时：远端 30 秒没有任何数据，会话已结束",
                        );
                        let _ = diskq.send(DiskJob::Close);
                        mode = TermMode::Normal;
                        suppress_until = Some(Instant::now() + Duration::from_millis(500));
                        zmodem_rx = None;
                        emit_zmodem_end(&*sink, &session_id, "session ended");
                    }
                }
                // ZMODEM download fast-fail on plain text: sz that fails to
                // open its file (no read permission) prints the error straight
                // to the PTY and exits without CAN/ZFERR. The receiver buffers
                // that text (noise_text); if no real frame follows within the
                // grace window the sender is dead — show the error text and
                // end the session immediately instead of blocking the
                // terminal for the full 30s idle timeout.
                if mode == TermMode::Zmodem {
                    let noise_timed_out = zmodem_rx
                        .as_ref()
                        .map(|rx| rx.noise_timeout(Duration::from_millis(2000)))
                        .unwrap_or(false);
                    if noise_timed_out {
                        log::warn!(
                            "[ssh:{}] zmodem rx noise timeout (plain text, no offer) — ending session",
                            session_id
                        );
                        let text = zmodem_rx
                            .as_mut()
                            .map(|rx| rx.take_noise_text())
                            .unwrap_or_default();
                        if !text.is_empty() {
                            sink.emit(
                                "ssh_output",
                                &SshOutputPayload {
                                    session_id: session_id.clone(),
                                    data: text,
                                },
                            );
                        }
                        send_data(&data_tx, &[CAN; 8]).await;
                        send_data(&data_tx, b"\x08\x08\x08\x08\x08\x08\x08\x08").await;
                        emit_zmodem_error(
                            &*sink,
                            &session_id,
                            "远端 sz 未发送文件即退出（常见原因：无读取权限），已退出 ZMODEM 模式",
                        );
                        let _ = diskq.send(DiskJob::Close);
                        mode = TermMode::Normal;
                        suppress_until = Some(Instant::now() + Duration::from_millis(500));
                        zmodem_rx = None;
                        emit_zmodem_end(&*sink, &session_id, "session ended");
                    }
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

/// Native ZMODEM sender state, bundled so it threads through handle_incoming_data
/// as a single &mut. Symmetric to the `zmodem_rx: Option<ZmodemReceiver>` slot.
///
/// Upload pump is INLINE in the reader's ChannelMsg::Data arm (not a separate
/// task): a separate pump task starved the PTY input buffer (rz's stdin got
/// flooded faster than it could drain, losing bytes and causing endless ZRPOS
/// retransmits). The inline pump naturally throttles — each channel.data() is
/// gated by the reader returning to select!, matching rz's read cadence.
struct TxSession {
    /// Active sender for the current file. None until the frontend calls
    /// zmodem_start_upload.
    sender: Option<ZmodemSender>,
    /// The file currently being sent.
    file: Option<std::fs::File>,
    /// Remaining files for multi-file batches.
    queue: std::collections::VecDeque<UploadFile>,
    /// rz bytes received before the sender was created (buffered ZRINIT etc.).
    pending: Vec<u8>,
    /// When the sender entered WaitingZrinit2 (post-ZEOF). Used by the
    /// reader to force a ZFIN timeout when rz fails to acknowledge (e.g.
    /// its PTY went into an error state and is streaming garbage instead
    /// of a ZRINIT/ZFIN reply).
    waiting_zrinit2_since: Option<std::time::Instant>,
    /// Last time we emitted a `zmodem_progress` IPC event. Throttles the
    /// per-subpacket Progress events so a fast upload (one Progress per
    /// 128 KiB subpacket) doesn't flood the WebView with IPC + React
    /// re-renders — the single biggest upload-throughput bottleneck.
    last_progress_emit: Option<std::time::Instant>,
}

impl TxSession {
    fn new() -> Self {
        Self {
            sender: None,
            file: None,
            queue: std::collections::VecDeque::new(),
            pending: Vec::new(),
            waiting_zrinit2_since: None,
            last_progress_emit: None,
        }
    }
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
    zmodem_cancelled: &mut bool,
    zmodem_rx: &mut Option<ZmodemReceiver>,
    disk: &DiskSink,
    tx_state: &mut TxSession,
    rx_last_progress: &mut Option<Instant>,
    tty_saw_prompt: &mut bool,
) -> Vec<u8> {
    match *mode {
        TermMode::Normal => {
            // Post-session suppress window: the CAN burst we sent (and rz's
            // reply) is still in flight. Dropping it here — rather than
            // letting it reach the terminal — is what keeps `cccccccc` from
            // appearing after a cancel, and keeps the abort tail from
            // re-triggering the auto-start scan below.
            if matches!(suppress_until, Some(t) if Instant::now() < *t) {
                buffer.clear();
                return Vec::new();
            }

            // Append to the shared terminal/ZMODEM-scan buffer. If no needle is
            // found the existing 16ms tick will coalesce-flush it as terminal
            // output via append_capped's threshold logic.
            append_capped(sink, session_id, buffer, data, last_flush);

            // A shell prompt means the remote is interactive again: rz has
            // exited and the user could legitimately start a new transfer.
            // This is the second (keystroke-free) way to clear the post-cancel
            // lockout — without it, a user who never types would be locked out
            // until they press a key.
            if looks_like_shell_prompt(buffer) {
                // 提示符出现 = 远端 tty 已被 rz 正常恢复（或从未坏掉），
                // 待办的 stty sane 不必再发。
                *tty_saw_prompt = true;
                if *zmodem_cancelled {
                    *zmodem_cancelled = false;
                }
            }

            // Re-scan the full buffer for the ZMODEM auto-start sequence. It can
            // land anywhere — typically right after a prompt — so we can't only
            // look at the tail.
            //
            // The suppress window is already handled by the early return above,
            // so by the time we get here the buffer holds genuine terminal data.
            // Only the canonical `ZPAD ZPAD ZDLE [ABC]` form counts as a start
            // (see ZMODEM_NEEDLES) — the bare-ZDLE fallback that used to live
            // here matched abort-burst tails and shell history.
            if !*zmodem_cancelled {
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
                    // Upload (remote `rz`). Enter Zmodem mode. Emit zmodem_start
                    // so the frontend prompts for files and calls
                    // zmodem_start_upload (native) - OR falls back to the JS
                    // zmodem.js bridge via zmodem_raw passthrough (legacy).
                    // Buffer the initial ZRINIT for the native sender.
                    *mode = TermMode::Zmodem;
                    *zmodem_rx = None;
                    log::info!(
                        "[ssh:{}] zmodem_start direction=upload (remote rz invited)",
                        session_id
                    );
                    sink.emit(
                        "zmodem_start",
                        &ZmodemStartPayload {
                            session_id: session_id.to_string(),
                            direction: "upload",
                        },
                    );
                    // Stash for the native sender (if the frontend uses it).
                    tx_state.pending.extend_from_slice(&passthrough_bytes);
                    // Passthrough fallback for legacy JS bridge.
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
                    log::info!(
                        "[ssh:{}] zmodem_start direction=download (remote sz started)",
                        session_id
                    );
                    sink.emit(
                        "zmodem_start",
                        &ZmodemStartPayload {
                            session_id: session_id.to_string(),
                            direction: "download",
                        },
                    );
                }
                let (send, ended, disk_sig) =
                    dispatch_rx_actions(sink, session_id, actions, rx_last_progress, disk);
                if disk_sig.flush || disk_sig.close {
                    let pending = rx.take_pending_write();
                    if !pending.is_empty() {
                        let _ = disk.send(DiskJob::Write(pending));
                    }
                }
                if disk_sig.flush {
                    let _ = disk.send(DiskJob::Flush);
                }
                if ended {
                    if disk_sig.close {
                        let _ = disk.send(DiskJob::Close);
                    }
                    *mode = TermMode::Normal;
                    *suppress_until = Some(Instant::now() + Duration::from_millis(500));
                    *zmodem_rx = None;
                    emit_zmodem_end(&*sink, &session_id, "session ended");
                } else {
                    *zmodem_rx = Some(rx);
                }
                return send;
                }
            }
            Vec::new()
        }
        TermMode::Zmodem => {
            if let Some(rx) = zmodem_rx.as_mut() {
                // Native download path — data goes straight to disk, no JS IPC.
                let actions = rx.feed(data);
                if rx.take_start_signal() {
                    log::info!(
                        "[ssh:{}] zmodem_start direction=download (receiver handshake)",
                        session_id
                    );
                    sink.emit(
                        "zmodem_start",
                        &ZmodemStartPayload {
                            session_id: session_id.to_string(),
                            direction: "download",
                        },
                    );
                }
                let (send, ended, disk_sig) =
                    dispatch_rx_actions(sink, session_id, actions, rx_last_progress, disk);
                // CRITICAL: drain any pending decoded payload BEFORE sending
                // Flush/Close. ZEOF triggers disk_sig.flush, but the last
                // subpacket's bytes are still in pending_write — if we flush
                // before draining, the file is truncated and the transfer
                // hangs because sz never sees its ZEOF acknowledged correctly.
                if disk_sig.flush || disk_sig.close {
                    let pending = rx.take_pending_write();
                    if !pending.is_empty() {
                        let _ = disk.send(DiskJob::Write(pending));
                    }
                }
                if disk_sig.flush {
                    let _ = disk.send(DiskJob::Flush);
                }
                if ended {
                    if disk_sig.close {
                        let _ = disk.send(DiskJob::Close);
                    }
                    *mode = TermMode::Normal;
                    *suppress_until = Some(Instant::now() + Duration::from_millis(500));
                    *zmodem_rx = None;
                    // Tell the frontend the session is over so the progress
                    // overlay hides. Without this the UI stays at 100% forever.
                    emit_zmodem_end(&*sink, &session_id, "session ended");
                }
                return send;
            }

            // Native upload path - sender active, feed rz bytes and stream file data.
            if let Some(sender) = tx_state.sender.as_mut() {
                let mut all_send = Vec::new();
                let mut tx_ended = false;
                let mut tx_error: Option<String> = None;

                // Multi-file: when the current file's ZEOF is sent, the sender
                // enters WaitingZrinit2. If rz sends ZRINIT (ready for next file)
                // and we have queued files, swap in a fresh sender BEFORE feeding
                // the ZRINIT — otherwise the old sender would send ZFIN and end
                // the session.
                if sender.is_waiting_for_next_file() && !tx_state.queue.is_empty() {
                    if let Some(next) = tx_state.queue.pop_front() {
                        log::info!("[ssh:{}] [TX] multi-file: starting next file '{}'", session_id, next.name);
                        match start_upload_next_file(&next) {
                            Ok((new_sender, new_file)) => {
                                // Drop old sender (already sent ZEOF). Replace
                                // with a fresh one in WaitingZrinit state so it
                                // processes the incoming ZRINIT normally.
                                *sender = new_sender;
                                tx_state.file = Some(new_file);
                            }
                            Err(e) => {
                                emit_zmodem_error(&*sink, &session_id, &e);
                                *mode = TermMode::Normal;
                                *suppress_until = Some(Instant::now() + Duration::from_millis(500));
                                tx_state.sender = None;
                                tx_state.file = None;
                                tx_state.queue.clear();
                                emit_zmodem_end(&*sink, &session_id, "session ended");
                                return Vec::new();
                            }
                        }
                    }
                }

                // Feed the incoming rz bytes (ZRPOS/ZACK/ZRINIT/ZFIN/CAN).
                log::info!("[ssh:{}] [TX] native path: {} bytes, sender state={}, hex: {:02x?}", session_id, data.len(), sender.state_name(), &data[..data.len().min(40)]);

                // Fast-finish optimization: in WaitingZrinit2 (file data fully
                // sent, waiting for rz to acknowledge), rz often emits non-
                // protocol output (progress text, file content echo, BEL bytes)
                // instead of a clean ZRINIT/ZFIN. Waiting 3s for the timeout
                // lets that garbage leak to the terminal after suppress expires.
                // If the data contains no ZMODEM frame header (ZPAD/ZDLE), end
                // immediately — the file was already sent successfully.
                if sender.is_waiting_for_next_file() && !data.iter().any(|&b| b == ZPAD || b == CAN) {
                    log::info!("[ssh:{}] [TX] WaitingZrinit2 + non-protocol data — fast finish", session_id);
                    let zfin = sender.force_finish_for_timeout();
                    all_send.extend_from_slice(&zfin);
                    *mode = TermMode::Normal;
                    *suppress_until = Some(Instant::now() + Duration::from_millis(2000));
                    tx_state.sender = None;
                    tx_state.file = None;
                    tx_state.queue.clear();
                    emit_zmodem_end(&*sink, &session_id, "session ended");
                    return all_send;
                }

                let actions = sender.feed(data);
                log::info!("[ssh:{}] [TX] feed: send={} events={}", session_id, actions.send.len(), actions.events.len());
                all_send.extend_from_slice(&actions.send);
                // Track ZSKIP separately: it needs the QUEUE to decide, and the
                // queue lives here, not in the single-file sender.
                let mut skipped_name: Option<String> = None;
                for ev in &actions.events {
                    if let TxEvent::FileSkipped { name } = ev {
                        skipped_name = Some(name.clone());
                    }
                }
                dispatch_tx_events(sink, session_id, &actions.events, &mut tx_ended, &mut tx_error);

                if let Some(msg) = tx_error {
                    emit_zmodem_error(&*sink, &session_id, &msg);
                    *mode = TermMode::Normal;
                    *suppress_until = Some(Instant::now() + Duration::from_millis(500));
                    tx_state.sender = None;
                    tx_state.file = None;
                    tx_state.queue.clear();
                    emit_zmodem_end(&*sink, &session_id, "session ended");
                    return Vec::new();
                }

                // Receiver skipped the current file (ZSKIP). Continue with the
                // next queued file if there is one; otherwise close the
                // session properly (ZFIN) so rz exits instead of sitting
                // there waiting for an offer that never comes — that wait is
                // what previously re-invited ZMODEM, popped the manual
                // chooser, and stranded the terminal in Zmodem mode.
                if let Some(name) = skipped_name {
                    if let Some(next) = tx_state.queue.pop_front() {
                        log::info!("[ssh:{}] [TX] '{}' skipped — offering next '{}'", session_id, name, next.name);
                        match start_upload_next_file(&next) {
                            Ok((mut new_sender, new_file)) => {
                                let offer = new_sender.send_zfile_offer();
                                *sender = new_sender;
                                tx_state.file = Some(new_file);
                                all_send.extend_from_slice(&offer);
                            }
                            Err(e) => {
                                emit_zmodem_error(&*sink, &session_id, &e);
                                *mode = TermMode::Normal;
                                *suppress_until = Some(Instant::now() + Duration::from_millis(500));
                                tx_state.sender = None;
                                tx_state.file = None;
                                tx_state.queue.clear();
                                emit_zmodem_end(&*sink, &session_id, "session ended");
                                return Vec::new();
                            }
                        }
                    } else {
                        log::warn!("[ssh:{}] [TX] last file '{}' skipped — closing session with ZFIN", session_id, name);
                        let zfin = sender.force_finish_for_timeout();
                        all_send.extend_from_slice(&zfin);
                        *mode = TermMode::Normal;
                        // 5s, not the usual 500ms/2s: this path KNOWS a ZFIN
                        // echo is coming back, and that echo's bytes
                        // (`**\x18B08…`) match the auto-start needle
                        // `**\x18B` byte-for-byte. A slow link can delay it
                        // past 2s, at which point the window has expired and
                        // the echo re-enters Zmodem mode (chooser pops a
                        // second time; cancelling strands the terminal).
                        *suppress_until = Some(Instant::now() + Duration::from_millis(5000));
                        tx_state.sender = None;
                        tx_state.file = None;
                        tx_state.queue.clear();
                        emit_zmodem_end(&*sink, &session_id, "session ended");
                        return all_send;
                    }
                }

                if tx_ended {
                    *mode = TermMode::Normal;
                    *suppress_until = Some(Instant::now() + Duration::from_millis(500));
                    tx_state.sender = None;
                    tx_state.file = None;
                    tx_state.queue.clear();
                    emit_zmodem_end(&*sink, &session_id, "session ended");
                    return all_send;
                }

                // File data streaming is handled by the caller (select! loop)
                // so each subpacket can be sent via channel.data() individually,
                // letting the SSH transport manage flow control between sends.

                return all_send;
            }

            // Passthrough (upload) fallback - emit zmodem_raw to the JS bridge.
            // Used when the frontend has not called zmodem_start_upload (legacy
            // mode) or for multi-file batches (not yet native).
            log::info!("[ssh:{}] [TX] passthrough fallback (sender=None): {} bytes, is_end={}", session_id, data.len(), is_zmodem_end(data));
            buffer.extend_from_slice(data);
            if !buffer.is_empty() {
                flush_zmodem_buffer(sink, session_id, buffer);
                *last_flush = Instant::now();
            }
            if is_zmodem_end(data) {
                *mode = TermMode::Normal;
                *suppress_until = Some(Instant::now() + Duration::from_millis(500));
                emit_zmodem_end(&*sink, &session_id, "session ended");
            }
            Vec::new()
        }
    }
}

/// Expand the user's upload selection (files and/or folders) into the flat
/// upload queue. Folders are walked depth-first: each contained file's ZFILE
/// offer name is `<folder-name>/<sub>/file` — lrzsz's rz creates the remote
/// directories from the path, which is how ZMODEM (a file-only protocol)
/// carries a tree. Directly-selected files keep their plain basename.
/// Depth-capped to survive symlink/junction cycles. Non-fatal problems land
/// in the returned error list (transfer continues with the rest).
fn collect_upload_files(
    paths: &[String],
) -> (std::collections::VecDeque<UploadFile>, Vec<String>) {
    let mut queue = std::collections::VecDeque::new();
    let mut errors = Vec::new();
    for p in paths {
        match std::fs::metadata(p) {
            Ok(md) if md.is_file() => push_upload_file(&mut queue, p, None, &md),
            Ok(md) if md.is_dir() => {
                let root = std::path::Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("folder")
                    .to_string();
                walk_upload_dir(std::path::Path::new(p), &root, &mut queue, &mut errors, 0);
            }
            Ok(_) => errors.push(format!("{}: 不是文件或目录（已跳过）", p)),
            Err(e) => errors.push(format!("{}: 读取本地信息失败: {}", p, e)),
        }
    }
    (queue, errors)
}

/// Append one file to the upload queue. `rel_prefix` is the file's path
/// relative to the selected folder root (None = directly-selected file).
fn push_upload_file(
    queue: &mut std::collections::VecDeque<UploadFile>,
    path: &str,
    rel_prefix: Option<&str>,
    md: &std::fs::Metadata,
) {
    let base = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    // POSIX separators in the offer name — remote rz splits on '/'.
    let name = match rel_prefix {
        Some(prefix) => format!("{}/{}", prefix, base),
        None => base,
    };
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    queue.push_back(UploadFile {
        path: path.to_string(),
        name,
        size: md.len(),
        mtime,
    });
}

fn walk_upload_dir(
    dir: &std::path::Path,
    rel_prefix: &str,
    queue: &mut std::collections::VecDeque<UploadFile>,
    errors: &mut Vec<String>,
    depth: usize,
) {
    const MAX_DEPTH: usize = 64;
    if depth > MAX_DEPTH {
        errors.push(format!(
            "{}: 目录层级过深（>{}，可能存在符号链接循环）",
            rel_prefix, MAX_DEPTH
        ));
        return;
    }
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            errors.push(format!("{}: 读取目录失败: {}", rel_prefix, e));
            return;
        }
    };
    // Deterministic order across platforms (name-sorted, dirs and files
    // interleaved — a stable sequence makes resume/debugging predictable).
    let mut names: Vec<std::ffi::OsString> =
        rd.filter_map(|e| e.ok().map(|e| e.file_name())).collect();
    names.sort();
    for fname in names {
        let full = dir.join(&fname);
        let disp = fname.to_string_lossy().into_owned();
        let rel = format!("{}/{}", rel_prefix, disp);
        // metadata() follows symlinks/junctions (OneDrive etc.) — the depth
        // cap above is the cycle guard.
        match std::fs::metadata(&full) {
            // rel_prefix is the DIRECTORY prefix; push_upload_file appends
            // the file's basename (rel here already contains it).
            Ok(md) if md.is_file() => {
                push_upload_file(queue, &full.to_string_lossy(), Some(rel_prefix), &md)
            }
            Ok(md) if md.is_dir() => {
                walk_upload_dir(&full, &rel, queue, errors, depth + 1)
            }
            Ok(_) => {} // special file (socket/device) — skip silently
            Err(e) => errors.push(format!("{}: {}", rel, e)),
        }
    }
}

/// Open a file for native upload and create the ZmodemSender. Returns
/// (sender, file) ready to stream.
fn start_upload_file(uf: &UploadFile) -> Result<(ZmodemSender, std::fs::File), String> {
    let file = std::fs::File::open(&uf.path).map_err(|e| format!("打开文件失败: {}", e))?;
    Ok((
        ZmodemSender::new(uf.name.clone(), uf.size, uf.mtime),
        file,
    ))
}

/// Open a **subsequent** file in a multi-file upload batch. Uses
/// `new_for_next_file` so the fresh sender skips ZSINIT (already negotiated
/// for the session) and sends ZFILE directly — resending ZSINIT makes rz
/// abort to the shell.
fn start_upload_next_file(uf: &UploadFile) -> Result<(ZmodemSender, std::fs::File), String> {
    let file = std::fs::File::open(&uf.path).map_err(|e| format!("打开文件失败: {}", e))?;
    Ok((
        ZmodemSender::new_for_next_file(uf.name.clone(), uf.size, uf.mtime),
        file,
    ))
}

/// Translate TxEvent batch into frontend Tauri events, mirroring
/// dispatch_rx_actions. Sets `ended` on SessionEnd, `error` on Error.
fn dispatch_tx_events(
    sink: &dyn EventSink,
    session_id: &str,
    events: &[TxEvent],
    ended: &mut bool,
    error: &mut Option<String>,
) {
    for ev in events {
        match ev {
            TxEvent::Started => {} // zmodem_start already emitted
            TxEvent::Progress { sent, total } => {
                sink.emit("zmodem_progress", &serde_json::json!({
                    "sessionId": session_id,
                    "bytesTransferred": sent,
                    "bytesTotal": total
                }));
            }
            TxEvent::FileComplete { name, bytes } => {
                sink.emit("zmodem_file_complete", &serde_json::json!({
                    "sessionId": session_id,
                    "fileName": name,
                    "bytesWritten": bytes
                }));
            }
            TxEvent::FileSkipped { name } => {
                // Receiver refused the file (ZSKIP). Not fatal by itself —
                // the reader continues with the next queued file or closes
                // the session cleanly. Surface it so the user knows why a
                // file from a multi-file batch never landed.
                sink.emit("zmodem_file_skipped", &serde_json::json!({
                    "sessionId": session_id,
                    "fileName": name
                }));
            }
            TxEvent::SessionEnd => {
                *ended = true;
            }
            TxEvent::Error(msg) => {
                *error = Some(msg.clone());
            }
        }
    }
}

/// Minimum interval between `zmodem_progress` IPC events during uploads.
/// The ZmodemSender emits one Progress per 128 KiB subpacket; without
/// throttling a 100 MB file fires ~800 IPC round-trips (JSON serialize +
/// WebView postMessage + React setState + re-render), all on the reader
/// thread, which is the single biggest upload-throughput bottleneck.
/// 100 ms keeps the progress bar smooth while cutting IPC count ~8×+.
const PROGRESS_THROTTLE: std::time::Duration = std::time::Duration::from_millis(100);

/// Throttled variant of dispatch_tx_events: skips `Progress` events that
/// arrive within `PROGRESS_THROTTLE` of the last emit, EXCEPT the final
/// one (`sent == total`) which always passes through so the progress bar
/// reliably reaches 100%. Non-Progress events are never throttled.
fn dispatch_tx_events_throttled(
    sink: &dyn EventSink,
    session_id: &str,
    events: &[TxEvent],
    ended: &mut bool,
    error: &mut Option<String>,
    last_emit: &mut Option<std::time::Instant>,
) {
    for ev in events {
        match ev {
            TxEvent::Started => {}
            TxEvent::Progress { sent, total } => {
                let now = std::time::Instant::now();
                // Force-flush the final progress (sent == total) so the UI
                // always shows 100% on completion, regardless of throttle.
                let is_final = *sent >= *total;
                let due = last_emit
                    .map(|t| now.duration_since(t) >= PROGRESS_THROTTLE)
                    .unwrap_or(true);
                if is_final || due {
                    sink.emit("zmodem_progress", &serde_json::json!({
                        "sessionId": session_id,
                        "bytesTransferred": sent,
                        "bytesTotal": total
                    }));
                    *last_emit = Some(now);
                }
            }
            TxEvent::FileComplete { name, bytes } => {
                sink.emit("zmodem_file_complete", &serde_json::json!({
                    "sessionId": session_id,
                    "fileName": name,
                    "bytesWritten": bytes
                }));
            }
            TxEvent::FileSkipped { name } => {
                sink.emit("zmodem_file_skipped", &serde_json::json!({
                    "sessionId": session_id,
                    "fileName": name
                }));
            }
            TxEvent::SessionEnd => {
                *ended = true;
            }
            TxEvent::Error(msg) => {
                *error = Some(msg.clone());
            }
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
    last_progress_emit: &mut Option<Instant>,
    disk_sink: &DiskSink,
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
                // Same 100ms throttle as uploads (see PROGRESS_THROTTLE): the
                // receiver emits Progress per 512KB crossing AND once per
                // feed() batch — at line rate that's 200+ IPC events/sec,
                // which floods the WebView with React re-renders. The flood
                // starved paint frames so the progress bar's CSS transition
                // crawled tens of percent behind the numbers, and delayed
                // UI clicks (cancel felt dead). The final progress event
                // (written >= total) always passes so the bar reaches 100%.
                let now = Instant::now();
                let is_final = total > 0 && written >= total;
                let due = last_progress_emit
                    .map(|t| now.duration_since(t) >= PROGRESS_THROTTLE)
                    .unwrap_or(true);
                if is_final || due {
                    sink.emit_raw(
                        "zmodem_progress",
                        serde_json::json!({
                            "sessionId": session_id,
                            "bytesTransferred": written,
                            "bytesTotal": total,
                        }),
                    );
                    *last_progress_emit = Some(now);
                }
            }
            RxEvent::FileComplete { name, written } => {
                // The file is done — flush the disk task so data lands before
                // we move to the next file or session end.
                disk.flush = true;
                // Did the bytes actually REACH the disk? The receiver's counter
                // is independent of the file, so a completed transfer over a
                // failed write used to be announced as a success with a
                // truncated file on disk. Report the disk error instead.
                if let Some(e) = disk_sink.failed() {
                    disk_sink.reset();
                    log::warn!("[ssh:{}] zmodem file {} 落盘失败: {}", session_id, name, e);
                    sink.emit_raw(
                        "zmodem_error",
                        serde_json::json!({
                            "sessionId": session_id,
                            "message": format!("文件 {} 传输失败：{}（文件可能不完整）", name, e),
                        }),
                    );
                } else {
                    sink.emit_raw(
                        "zmodem_file_complete",
                        serde_json::json!({
                            "sessionId": session_id,
                            "fileName": name,
                            "bytesWritten": written,
                        }),
                    );
                }
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

/// Emit `zmodem_error` and log it. Same rationale as `emit_zmodem_end`:
/// an error that only reaches the UI leaves no trace in the log, so a failed
/// transfer looks like it never happened when reading logs after the fact.
fn emit_zmodem_error(sink: &dyn EventSink, session_id: &str, message: &str) {
    log::warn!("[ssh:{}] zmodem_error: {}", session_id, message);
    sink.emit(
        "zmodem_error",
        &serde_json::json!({ "sessionId": session_id, "message": message }),
    );
}

/// Emit `zmodem_end` and log why. Every ZMODEM session exit (normal ZFIN,
/// abort, error, timeout) funnels through here so a session's end is ALWAYS
/// visible in the log. Without this the start/end events were invisible and a
/// real abort looked identical to "rz never started" during debugging.
fn emit_zmodem_end(sink: &dyn EventSink, session_id: &str, reason: &str) {
    log::info!("[ssh:{}] zmodem_end ({})", session_id, reason);
    sink.emit("zmodem_end", &session_id.to_string());
}

fn flush_buffer(sink: &dyn EventSink, session_id: &str, buffer: &mut Vec<u8>) {
    if buffer.is_empty() {
        return;
    }
    let payload = SshOutputPayload {
        session_id: session_id.to_string(),
        data: std::mem::take(buffer),
    };
    log::debug!("[ssh:{}] [OUT] flush {} bytes → ssh_output", session_id, payload.data.len());
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

/// Release the reader's startup hold (see `channel_reader`): the frontend has
/// attached its `ssh_output` listener, so buffered banner bytes can flush.
/// Unknown / already-gone sessions are a silent no-op — the hold dies with
/// the reader task anyway.
pub async fn frontend_ready(state: &AppState, session_id: &str) -> Result<(), String> {
    let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
    if let Some(session) = sessions.get(session_id) {
        session
            .command_tx
            .send(SessionCommand::FrontendReady)
            .map_err(|_| "Session command channel closed".to_string())?;
    }
    Ok(())
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

    // Tear down port-forwarding tunnels owned by this session BEFORE the
    // handle drops. A local rule's listener is a plain OS socket that would
    // keep accepting connections with no SSH connection left to carry them
    // — clients would hang until timeout instead of getting a fast refusal.
    // Remote rules must additionally tell the server to stop listening, which
    // needs the handle, so this ordering is required rather than cosmetic.
    let owned = state.port_forwards.stop_for_session(session_id);
    if !owned.is_empty() {
        log::info!(
            "[ssh:{}] stopped {} port-forward tunnel(s) with the session",
            session_id,
            owned.len()
        );
        if let Some(session) = session.as_ref() {
            for rule_id in &owned {
                state
                    .port_forwards
                    .release_remote(rule_id, &session.handle)
                    .await;
            }
        }
    }

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

/// Native ZMODEM upload: the frontend selected one or more local files. The
/// reader loop opens the first, creates a ZmodemSender, and streams the file
/// data directly over the SSH channel (zero IPC). Remaining files queue for
/// multi-file batches.
pub async fn zmodem_start_upload(
    state: &AppState,
    session_id: &str,
    paths: Vec<String>,
) -> Result<(), String> {
    let sender = {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        sessions
            .get(session_id)
            .map(|s| s.command_tx.clone())
    };

    if let Some(tx) = sender {
        let _ = tx.send(SessionCommand::ZmodemStartUpload { paths });
    }

    Ok(())
}

/// Heuristic: does this russh error string indicate the underlying SSH
/// transport is dead (so a reconnect is worth trying)? russh surfaces these
/// as stringified errors — we match on substrings because the error types
/// aren't all public.
///
/// Case-insensitive and whitespace-insensitive: russh's display text for the
/// same failure has varied in capitalisation *and* spacing across versions
/// (`SendError` vs "Channel send error"), and a missed match silently turns
/// into "no reconnect, error on every probe".
///
/// `SendError` (`channel_open_session` fails when the handle's session task has
/// already exited — the sender was dropped) is the *clearest* death signal
/// there is: no packets will ever flow again on this handle.
fn is_connection_dead(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    // Whitespace dropped so "Channel send error" and "SendError" are one match.
    let flat = e.replace(' ', "");
    flat.contains("connectfailed")
        || flat.contains("senderror")
        || flat.contains("disconnect")
        || flat.contains("connectionreset")
        || flat.contains("brokenpipe")
        || e.contains("channel open")
}

/// Open a new session channel on the existing handle. Extracted so exec_once
/// can retry it after a reconnect without duplicating the handle-lookup logic.
///
/// Returns the russh error string as-is: `exec_once` adds the single
/// "Open exec channel failed: " prefix for both the first attempt and the
/// post-reconnect retry (it used to prefix here too, producing
/// "Open exec channel failed: Open exec channel failed: …" to the user).
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
        .map_err(|e| e.to_string())
}

// Per-session single-flight locks for exec_once's transparent reconnect.
// Keyed by session UUID; entries are tiny and live for the process lifetime.
static RECONNECT_LOCKS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

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
    // Single-flight: concurrent exec_once calls that all notice the dead
    // connection must not each dial+auth (log evidence: 3 dials in the same
    // millisecond). Everyone snapshots the handle pointer, waits on a
    // per-session lock, and whoever finds the handle already replaced by the
    // winner skips its own dial and reuses the fresh connection.
    let lock = {
        let mut table = RECONNECT_LOCKS.lock().map_err(|e| e.to_string())?;
        table.entry(session_id.to_string()).or_default().clone()
    };
    // usize (not a raw pointer) so the future stays Send across the await.
    let handle_before: Option<usize> = {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        sessions.get(session_id).map(|s| Arc::as_ptr(&s.handle) as usize)
    };
    let _guard = lock.lock().await;
    {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        if sessions.get(session_id).map(|s| Arc::as_ptr(&s.handle) as usize) != handle_before {
            log::info!(
                "[ssh:{}] reconnect already done by a concurrent call, reusing handle",
                session_id
            );
            return Ok(());
        }
    }
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
    // (ConnectFailed / SendError / Disconnected / channel open fail),
    // transparently reconnect once: dial + auth with the session's stored
    // config, replace the handle in the sessions map, then retry the channel
    // open + exec. This makes one-shot probes (AI health inspection,
    // server-info) resilient to idle-disconnect without forcing the user to
    // manually reconnect the tab.
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

#[cfg(test)]
mod upload_expand_tests {
    use super::collect_upload_files;

    #[test]
    fn folder_expands_to_relative_offer_names() {
        let root = std::env::temp_dir().join("zmodem_upload_expand_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub").join("deep")).unwrap();
        std::fs::write(root.join("a.txt"), b"aaa").unwrap();
        std::fs::write(root.join("sub").join("b.txt"), b"bb").unwrap();
        std::fs::write(root.join("sub").join("deep").join("c.bin"), b"cccc").unwrap();
        std::fs::create_dir_all(root.join("empty")).unwrap(); // no files inside

        let (queue, errors) = collect_upload_files(&[root.to_string_lossy().to_string()]);
        assert!(errors.is_empty(), "{:?}", errors);
        // Offer names = path relative to the selected folder, POSIX seps.
        // Name-sorted: a.txt, sub/b.txt, sub/deep/c.bin ("empty" contributes
        // nothing — ZMODEM carries files only, empty dirs can't travel).
        let names: Vec<&str> = queue.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "zmodem_upload_expand_test/a.txt",
                "zmodem_upload_expand_test/sub/b.txt",
                "zmodem_upload_expand_test/sub/deep/c.bin",
            ]
        );
        assert_eq!(queue.iter().find(|f| f.name.ends_with("a.txt")).unwrap().size, 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plain_files_keep_basename() {
        let tmp = std::env::temp_dir().join("zmodem_upload_flat_test.txt");
        std::fs::write(&tmp, b"x").unwrap();
        let (queue, errors) = collect_upload_files(&[tmp.to_string_lossy().to_string()]);
        assert!(errors.is_empty());
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].name, "zmodem_upload_flat_test.txt");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn missing_path_lands_in_errors() {
        let (queue, errors) =
            collect_upload_files(&["Z:\\definitely\\not\\here.abc".to_string()]);
        assert!(queue.is_empty());
        assert_eq!(errors.len(), 1);
    }
}

#[cfg(test)]
mod tmout_inject_tests {
    use super::TMOUT_SUPPRESS_COMMAND;

    /// 注入命令的形状不变量。修前这个字符串没有对 set 做 stderr 重定向，
    /// 非 bash 登录 shell（sh/dash/fish）每次连接都打 "Illegal option" 垃圾行。
    #[test]
    fn tmout_command_silences_every_set_on_non_bash_shells() {
        let s = std::str::from_utf8(TMOUT_SUPPRESS_COMMAND).unwrap();
        // 每个 set 都必须自带重定向（不能只靠行尾那个）。
        assert!(s.contains("set +o history 2>/dev/null"));
        assert!(s.contains("set -o history 2>/dev/null"));
        // 前导空格保留 —— HISTCONTROL=ignorespace 的历史隔离层。
        assert!(s.starts_with("\n set "));
    }
}

#[cfg(test)]
mod connection_dead_tests {
    use super::is_connection_dead;
    #[test]
    fn session_not_found_is_not_connection_death() {
        // Map miss = the session was explicitly disconnected (ssh_disconnect
        // removed it) or never existed. Dialing cannot resurrect it — exec_once
        // must surface the error instead of attempting a one-time reconnect
        // log evidence: a polling panel retried every 5s against a removed
        // session, 7104 times in one day, each attempting a pointless dial.
        assert!(!is_connection_dead("Session not found"));
        // SendError = the handle's session task exited (sender dropped): no
        // packets will ever flow on this handle again. Missing this match made
        // dead connections error on every probe instead of reconnecting once.
        assert!(is_connection_dead("SendError"));
        assert!(is_connection_dead(
            "Open exec channel failed: Channel send error"
        ));
        // Capitalisation is not stable across russh versions.
        assert!(is_connection_dead("Disconnected"));
        assert!(is_connection_dead("disconnect"));
        assert!(is_connection_dead(
            "Open exec channel failed: Failed to open channel (ConnectFailed)"
        ));
        assert!(is_connection_dead("channel open failed"));
    }
}

#[cfg(test)]
mod zmodem_zfin_echo_tests {
    use super::{find_zmodem_start, looks_like_shell_prompt};

    /// 根因不变量：ZFIN 帧的字节与 ZMODEM 自动启动针**逐字节相同**。
    ///
    /// 这是「会话结束后选择器又弹一次」那个 bug 的根：我们发 ZFIN 关会话，
    /// 远端的回响字节 `**\x18B08…` 与 auto-start 针 `**\x18B` 匹配，于是被
    /// `find_zmodem_start` 当成「远端又敲了一次 rz」，重新进入 Zmodem 模式。
    /// 用户点取消后终端卡死（输入被吞），只能重连。
    ///
    /// 这条测试锁住「为什么抑制窗口内必须跳过自动启动检测」—— 若哪天有人
    /// 改了针或 ZFIN 的构造，这里会先失败，而不是等到用户再踩一次坑。
    #[test]
    fn zfin_frame_bytes_match_the_autostart_needle() {
        // 与 zmodem_tx::build_hex_header(ZFIN) 的字节布局一致：
        // ZPAD ZPAD ZDLE ZHEX + "08" + 8 个 0 + CRC + CR LF + XON
        let zfin_like: Vec<u8> = {
            let mut v = vec![b'*', b'*', 0x18, b'B'];
            v.extend_from_slice(b"0800000000000000");
            v.extend_from_slice(b"0000\r\n");
            v.push(0x11);
            v
        };
        assert!(
            find_zmodem_start(&zfin_like).is_some(),
            "ZFIN 帧必须匹配 auto-start 针 —— 这是抑制窗口存在的原因"
        );

        // 反向：普通终端文本不得匹配，否则每次 ls 都会误入 ZMODEM。
        assert!(find_zmodem_start(b"user@host:~$ ").is_none());
    }

    /// 裸 ZDLE 针（`\x18[ABC]`，不带 `**` 前导）曾被当作「PTY echo 吃掉
    /// 前导星号」的兜底而存在。实测代价远大于收益：它让任何含 ZDLE 后跟
    /// A/B/C 的字节流都命中，包括 CAN 中止序列的余波、shell 历史命令、
    /// 二进制输出 —— 表现为取消后选择器反复自启、上下方向键查历史卡死。
    ///
    /// 这条测试锁住「只认规范前导」：真实场景中远端 rz 发出的帧永远带完整
    /// `ZPAD ZPAD ZDLE`，而 echo 只影响我们自己的输入，不影响对端的帧。
    #[test]
    fn bare_zdle_form_is_not_an_autostart() {
        // CAN 中止序列的余波：一串 0x18 后跟任意字节
        let can_tail = vec![0x18, 0x18, 0x18, 0x42, 0x18, 0x18];
        assert!(
            find_zmodem_start(&can_tail).is_none(),
            "裸 \\x18B 不得算作自动启动 —— 这正是取消后自启的根因"
        );

        // 历史命令里恰好出现 ZDLE + 'B'（例如粘贴过的二进制/转义内容）
        assert!(find_zmodem_start(b"ls -la\x18Bfoo").is_none());
    }

    /// 规范形式仍然认 —— 别把收窄做成「再也不识别」。
    #[test]
    fn canonical_form_still_detected() {
        assert!(find_zmodem_start(b"prompt$ **\x18B08").is_some());
        assert!(find_zmodem_start(b"**\x18A").is_some());
        assert!(find_zmodem_start(b"**\x18C").is_some());
    }

    /// shell 提示符判定：取消后恢复自动启动的唯一无输入途径。
    /// 过宽会让 chooser 在普通输出上重开，过严则用户不敲字就永远锁死。
    #[test]
    fn shell_prompt_detection_is_narrow_but_sufficient() {
        // 常见提示符（bash / zsh / root / PS2 续行）
        assert!(looks_like_shell_prompt(b"user@host:~$ "));
        assert!(looks_like_shell_prompt(b"[root@ias34 /]# "));
        assert!(looks_like_shell_prompt(b"> ")); // PS2
        assert!(looks_like_shell_prompt(b"ias@34-prod ~% ")); // zsh

        // 普通输出不得判定为提示符
        assert!(!looks_like_shell_prompt(b"total 48\ndrwxr-xr-x 2 user user 4096 "));
        assert!(!looks_like_shell_prompt(b"a.txt\n"));
        // 行中间的 $ 不算（没有尾随空格）
        assert!(!looks_like_shell_prompt(b"echo $HOME"));
    }
}

// ============ Upload capability probe (drag-drop upload channel selection) ============
//
// 拖拽上传原本无条件走 SFTP 子系统。真实环境里「连得上 SSH」不等于「用得了
// SFTP」——restricted shell、chroot、只开 exec 的跳板机、嵌入式设备都会拒绝
// `request_subsystem("sftp")`，用户看到的是「无法检查远端目标目录，已取消上传：
// SFTP session init failed: Timeout」，没有任何可操作的下一步。
//
// 探测一次服务端的实际上传能力，让上传走能用的那条路，并在两条路都没有时
// 给出「装什么、怎么装」的具体指引，而不是一句无法执行的错误。
//
// 探测走 exec 通道（`exec_once`）而不是 PTY：PTY 会被用户正在跑的全屏程序
// 占用，exec 通道独立、且 exec_once 自带一次性重连。

/// 服务端上传能力的探测结果。
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadCapabilities {
    /// `request_subsystem("sftp")` 是否可用。
    pub sftp_available: bool,
    /// 远端是否同时有 `rz` 与 `sz`（lrzsz）。
    pub zmodem_available: bool,
    /// 探测是否跑完。false = exec 也不可用（或超时），此时两条路径都未确认。
    pub probe_completed: bool,
    /// 探测到的包管理器（用于生成安装命令）。
    pub package_manager: Option<String>,
    /// 装 lrzsz 的具体命令；探测到包管理器时才有。
    pub install_hint: Option<String>,
    /// 探测过程中的说明（失败原因等），展示给用户。
    pub message: Option<String>,
}

/// 探测脚本：一次性问完「有没有 rz/sz」与「用哪个包管理器」。
///
/// 每条命令单独一行输出，用 `;` 分隔而不是 `&&`：任一探测命令不存在时
/// `command -v` 返回非零，`&&` 会让后面全部不执行，最后两行永远拿不到。
/// POSIX sh 兼容（不用 bash 特性）：服务端可能是 dash / busybox。
const PROBE_SCRIPT: &str = "command -v rz >/dev/null 2>&1 && echo HAS_RZ; \
command -v sz >/dev/null 2>&1 && echo HAS_SZ; \
for pm in apt-get dnf yum apk zypper pacman; do command -v $pm >/dev/null 2>&1 && echo PM=$pm && break; done";

/// 探测超时。与 `ssh_get_server_info` 的 8 秒同量级——探测失败不该让拖拽
/// 卡住，未知就退回「上传时再判」。
const PROBE_TIMEOUT_SECS: u64 = 8;

/// 包管理器 → 安装 lrzsz 的命令。用于「两条上传路径都不可用」时的可操作提示。
fn install_hint_for(pm: &str) -> Option<&'static str> {
    match pm {
        "apt-get" => Some("sudo apt-get update && sudo apt-get install -y lrzsz"),
        "dnf" => Some("sudo dnf install -y lrzsz"),
        "yum" => Some("sudo yum install -y lrzsz"),
        "apk" => Some("sudo apk add lrzsz"),
        "zypper" => Some("sudo zypper install -y lrzsz"),
        "pacman" => Some("sudo pacman -S --noconfirm lrzsz"),
        _ => None,
    }
}

/// 探测服务端的上传能力。
///
/// SFTP 可用性单独试一次子系统请求（`get_sftp_session` 的同一路径）；lrzsz
/// 与包管理器走 exec 脚本。exec 不可用（restricted shell / 超时）时
/// `probe_completed = false`，调用方据此走「上传时失败再降级」。
pub async fn probe_upload_capabilities(
    state: &AppState,
    session_id: &str,
) -> UploadCapabilities {
    // SFTP：真实开一次子系统，失败即不可用。这比任何间接推断都可靠。
    // 必须带超时——挂起的服务器（收到子系统请求后不应答）会让握手永久
    // 卡住，探测死等，拖拽上传就卡在这一步，这正是要修的用户症状。
    let sftp_available = tokio::time::timeout(
        std::time::Duration::from_secs(PROBE_TIMEOUT_SECS),
        super::sftp::probe_sftp_subsystem(state, session_id),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false);

    let mut caps = UploadCapabilities {
        sftp_available,
        ..Default::default()
    };

    let output = match tokio::time::timeout(
        std::time::Duration::from_secs(PROBE_TIMEOUT_SECS),
        exec_once(state, session_id, PROBE_SCRIPT),
    )
    .await
    {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            // exec 也不可用：两条路都没确认。不明说「装 lrzsz」——用户可能
            // 装不了，而且真正缺的是 exec 通道本身。
            caps.probe_completed = false;
            caps.message = Some(format!("无法探测服务端能力：{}", e));
            return caps;
        }
        Err(_) => {
            caps.probe_completed = false;
            caps.message = Some("探测服务端能力超时".to_string());
            return caps;
        }
    };

    caps.probe_completed = true;
    parse_probe_output(&output, sftp_available, &mut caps);
    caps
}

/// 解析探测脚本的输出并填充 `caps`。纯函数，单测直接喂字符串。
///
/// zmodem 可用要求 rz 与 sz **同时**在：上传要 rz、（下载/完整性）要 sz，
/// 只有其中一个说明装的是残缺包，按不可用处理走安装指引。
fn parse_probe_output(output: &str, sftp_available: bool, caps: &mut UploadCapabilities) {
    let mut has_rz = false;
    let mut has_sz = false;
    for line in output.lines() {
        let line = line.trim();
        match line {
            "HAS_RZ" => has_rz = true,
            "HAS_SZ" => has_sz = true,
            _ => {
                if let Some(pm) = line.strip_prefix("PM=") {
                    let pm = pm.trim();
                    caps.package_manager = Some(pm.to_string());
                    caps.install_hint = install_hint_for(pm).map(|s| s.to_string());
                }
            }
        }
    }
    caps.zmodem_available = has_rz && has_sz;

    if !sftp_available && !caps.zmodem_available {
        caps.message = Some(match &caps.install_hint {
            Some(hint) => format!("服务端既不支持 SFTP 子系统，也没有 lrzsz。可安装：{}", hint),
            None => "服务端既不支持 SFTP 子系统，也没有 lrzsz。请安装 lrzsz 后重试。".to_string(),
        });
    }
}

#[cfg(test)]
mod upload_caps_tests {
    use super::{install_hint_for, parse_probe_output, UploadCapabilities};

    fn parse(output: &str, sftp: bool) -> UploadCapabilities {
        // probe_completed 由 probe_upload_capabilities 在调用解析前置位
        // （exec 探测已成功才会走到解析），这里复现同样的前置状态。
        let mut caps = UploadCapabilities {
            sftp_available: sftp,
            probe_completed: true,
            ..Default::default()
        };
        parse_probe_output(output, sftp, &mut caps);
        caps
    }

    /// lrzsz 齐全 + apt：ZMODEM 可用、给出 apt 安装命令。
    #[test]
    fn full_lrzsz_with_apt() {
        let caps = parse("HAS_RZ\nHAS_SZ\nPM=apt-get\n", false);
        assert!(caps.zmodem_available);
        assert!(!caps.sftp_available);
        assert_eq!(caps.package_manager.as_deref(), Some("apt-get"));
        assert_eq!(
            caps.install_hint.as_deref(),
            Some("sudo apt-get update && sudo apt-get install -y lrzsz")
        );
        // 有 ZMODEM 就不是死路，不生成「两条路都没有」的提示。
        assert!(caps.message.is_none());
    }

    /// 只有 rz 没有 sz（残缺安装）：按不可用处理，走安装指引。
    #[test]
    fn rz_without_sz_is_unavailable() {
        let caps = parse("HAS_RZ\nPM=dnf\n", false);
        assert!(!caps.zmodem_available);
        assert!(caps.message.is_some());
        assert!(caps.message.as_deref().unwrap().contains("lrzsz"));
    }

    /// 两条路都没有 + 探到包管理器：提示里带具体安装命令。
    #[test]
    fn nothing_available_gives_install_hint() {
        let caps = parse("PM=apk\n", false);
        assert!(!caps.zmodem_available && !caps.sftp_available);
        assert!(caps.probe_completed);
        let msg = caps.message.as_deref().unwrap();
        assert!(msg.contains("apk add lrzsz"), "提示应含安装命令：{}", msg);
    }

    /// 两条路都没有 + 没探到包管理器：提示仍可读，不 panic。
    #[test]
    fn nothing_available_without_pm() {
        let caps = parse("", false);
        assert!(caps.message.is_some());
        assert!(caps.install_hint.is_none());
    }

    /// SFTP 可用时即使没有 lrzsz 也不提示安装（不需要）。
    #[test]
    fn sftp_available_silences_hint() {
        let caps = parse("", true);
        assert!(caps.sftp_available);
        assert!(caps.message.is_none());
    }

    /// 输出可能带 \r（部分 shell 的 echo）：trim 后仍能识别。
    #[test]
    fn crlf_output_still_parses() {
        let caps = parse("HAS_RZ\r\nHAS_SZ\r\nPM=pacman\r\n", false);
        assert!(caps.zmodem_available);
        assert_eq!(caps.package_manager.as_deref(), Some("pacman"));
    }

    /// 脚本用 `;` 分隔而不是 `&&`：前一条失败后 PM= 仍会出现——这里锁住
    /// PM= 行独立于 HAS_RZ/HAS_SZ 的解析（不依赖顺序）。
    #[test]
    fn pm_line_parses_regardless_of_order() {
        let caps = parse("PM=zypper\nHAS_RZ\nHAS_SZ\n", false);
        assert!(caps.zmodem_available);
        assert_eq!(caps.package_manager.as_deref(), Some("zypper"));
    }

    #[test]
    fn install_hint_covers_all_known_pms() {
        for pm in ["apt-get", "dnf", "yum", "apk", "zypper", "pacman"] {
            assert!(install_hint_for(pm).is_some(), "{} 应有安装命令", pm);
        }
        assert!(install_hint_for("brew").is_none());
    }
}
