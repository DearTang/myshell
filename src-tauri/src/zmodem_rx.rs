//! Native Rust ZMODEM receiver — eliminates the JS IPC round-trip bottleneck.
//!
//! When the remote runs `sz`, bytes flow: SSH channel → this state machine →
//! direct file write. Zero IPC crossing for data. Only control events (file
//! offer, progress, completion) cross to the frontend.
//!
//! Protocol reference: the zmodem.js source (node_modules/zmodem.js/src/) and
//! the lrzsz wire behavior. We implement the DOWNLOAD path only; uploads
//! (remote `rz`) fall back to the JS zmodem.js path unchanged.

// ── ZMODEM constants (byte values) ────────────────────────────────────────
pub(crate) const ZDLE: u8 = 0x18;
pub(crate) const ZPAD: u8 = b'*';
pub(crate) const ZBIN: u8 = b'A'; // binary16 header
pub(crate) const ZHEX: u8 = b'B'; // hex header
pub(crate) const ZBIN32: u8 = b'C'; // binary32 header
pub(crate) const XON: u8 = 0x11;
pub(crate) const XOFF: u8 = 0x13;

// Frame type numbers (the typenum byte in a header)
pub(crate) mod frame_type {
    pub const ZRQINIT: u8 = 0;
    pub const ZRINIT: u8 = 1;
    pub const ZSINIT: u8 = 2;
    pub const ZACK: u8 = 3;
    pub const ZFILE: u8 = 4;
    pub const ZSKIP: u8 = 5;
    pub const ZABORT: u8 = 7;
    pub const ZFIN: u8 = 8;
    pub const ZRPOS: u8 = 9;
    pub const ZDATA: u8 = 10;
    pub const ZEOF: u8 = 11;
    pub const ZFERR: u8 = 12;
}

// Subpacket frame-end types (follow a ZDLE)
pub(crate) mod subpkt_end {
    pub const ZCRCE: u8 = 0x68; // h — end of frame, no ack
    pub const ZCRCG: u8 = 0x69; // i — frame continues, no ack (firehose)
    pub const ZCRCQ: u8 = 0x6a; // j — frame continues, ack
    pub const ZCRCW: u8 = 0x6b; // k — end of frame, ack
}

/// Receiver capability flags for ZRINIT.
/// CANFDX=0x01 | CANOVIO=0x02 | CANFC32=0x20 = 0x23
const ZRINIT_FLAGS: u8 = 0x23;

/// Receiver buffer/window size advertised in ZRINIT (ZP0/ZP1, little-endian).
/// lrzsz `sz` uses this as its streaming window — how many bytes it sends
/// before pausing for a ZACK. Advertising 0 makes lrzsz fall back to a tiny
/// (~1 KiB) default window, which caps throughput at ~2.5 MB/s even on a LAN
/// (window / RTT). A large window lets `sz` stream continuously, reaching
/// near line-rate throughput. 32 KiB comfortably covers the bandwidth-delay
/// product of a fast LAN.
const ZRINIT_BUFSZ: u16 = 0x8000;

/// Build the 4 ZRINIT data bytes: [bufsz_lo, bufsz_hi, 0, flags].
fn zrinit_data() -> [u8; 4] {
    [
        (ZRINIT_BUFSZ & 0xff) as u8,
        (ZRINIT_BUFSZ >> 8) as u8,
        0,
        ZRINIT_FLAGS,
    ]
}

/// Pack a file offset as 4 little-endian bytes (ZMODEM ZACK/ZRPOS/ZEOF field).
///
/// The cast used to be a bare `off as u32`, silently discarding the top 32
/// bits. This is the ONLY encoder for the position field in both directions
/// (the transmitter emits it for ZDATA and ZEOF; the peer's ZRPOS handler reads
/// back 4 bytes), so a 5 GiB transfer would advertise a 1 GiB offset and no
/// resume/retransmit above the 4 GiB boundary was representable — with no
/// error anywhere. Clamping keeps the field self-consistent (the peer sees an
/// offset it can act on) instead of advertising a wrong one.
pub(crate) fn offset_bytes(off: u64) -> [u8; 4] {
    let o = off.min(u32::MAX as u64) as u32;
    [
        (o & 0xff) as u8,
        ((o >> 8) & 0xff) as u8,
        ((o >> 16) & 0xff) as u8,
        ((o >> 24) & 0xff) as u8,
    ]
}

/// State of the native ZMODEM receiver state machine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RxState {
    /// First frame not yet seen — deciding download vs upload.
    Probing,
    /// Sent ZRINIT, waiting for ZFILE (or ZSINIT).
    WaitingZfile,
    /// ZFILE received, waiting for the frontend to accept/skip.
    WaitingAccept,
    /// File accepted, ZRPOS sent — receiving ZDATA subpackets.
    Transferring,
    /// All files done — ZFIN received, waiting for "OO".
    Done,
}

/// What the receiver wants the caller to do after a `feed()`.
#[derive(Default)]
pub struct RxActions {
    /// Bytes to send back to the SSH peer (protocol responses).
    pub send: Vec<u8>,
    /// Events to emit to the frontend (offer / progress / complete / end / error).
    pub events: Vec<RxEvent>,
}

/// Frontend-facing events emitted by the native receiver.
#[derive(Clone, Debug)]
pub enum RxEvent {
    /// A file is being offered — frontend should prompt for a save directory.
    /// `path` will be the filename (basename) to save.
    Offer { name: String, size: u64 },
    /// Progress update for the current file.
    Progress { written: u64, total: u64 },
    /// A file finished downloading.
    FileComplete { name: String, written: u64 },
    /// The entire ZMODEM session ended (ZFIN + OO).
    SessionEnd,
    /// Fatal error — caller should reset to Normal mode.
    Error(String),
}

/// A parsed ZMODEM header.
struct ParsedHeader {
    typenum: u8,
    /// The 4 data bytes (ZF0–ZF3).
    data: [u8; 4],
    /// Whether this header used binary32 framing (CRC32 subpackets follow a
    /// ZDATA header of this type); false for hex/binary16 (CRC16 subpackets).
    crc32: bool,
}

pub struct ZmodemReceiver {
    state: RxState,
    /// Accumulates raw bytes across SSH Data messages until we can parse a frame.
    buf: Vec<u8>,
    /// Path of the file currently being written (for progress display).
    file_path: Option<String>,
    /// Decoded payload data accumulated since the last `take_pending_write()`.
    /// The receiver does NOT write to disk itself — it only decodes and
    /// collects. The caller drains this and writes asynchronously (e.g. on a
    /// spawn_blocking task) so the SSH reader loop never blocks on disk I/O.
    /// russh's connection task awaits a bounded queue send per inbound packet;
    /// a blocked reader collapses throughput into a burst/pause crawl.
    pending_write: Vec<u8>,
    bytes_written: u64,
    file_size: u64,
    current_filename: String,
    /// Number of ZFILE/ZRINIT cycles (for file counting).
    file_index: u32,
    /// Offset within `buf` already consumed by the subpacket scanner.
    scan_pos: usize,
    /// When in Transferring state, we've seen a ZDATA header and are scanning
    /// for subpackets delimited by ZDLE+{ZCRCE|ZCRCG|ZCRCQ|ZCRCW}.
    in_zdata: bool,
    /// CRC width of the current ZDATA subpacket stream (set from the ZDATA
    /// header format): true = CRC32 (4-byte CRC after each subpacket), false =
    /// CRC16 (2-byte). Needed to skip the trailing CRC bytes correctly.
    data_crc32: bool,
    /// Set when the first frame indicates this is NOT a download (e.g. remote
    /// `rz` sends ZRINIT). The caller should drain `buf` and hand the bytes to
    /// the JS zmodem.js path instead.
    passthrough: bool,
    /// Plain terminal text (no ZDLE ⇒ no possible frame) seen while waiting
    /// for the first ZFILE offer. A sz that fails to open its file prints the
    /// error straight to the PTY and exits without CAN/ZFERR — the text lands
    /// here so the reader can surface it and end the session quickly instead
    /// of swallowing it until the 30s idle timeout.
    noise_text: Vec<u8>,
    /// When the first plain-text chunk was seen. Combined with a grace period
    /// by the reader (noise_timeout) so interleaved verbose output that IS
    /// followed by a real frame doesn't false-positive.
    noise_seen_at: Option<std::time::Instant>,
    /// Whether the "native download started" signal has been taken by the caller
    /// (so zmodem_start is emitted exactly once).
    start_emitted: bool,
    /// Last time feed() was called with data. The reader loop uses this as an
    /// idle watchdog: a sender that goes silent (crashed / killed / aborted
    /// without a CAN burst) would otherwise leave the session in Zmodem mode
    /// forever, swallowing all terminal output.
    last_feed: Option<std::time::Instant>,
    /// When the receiver entered WaitingAccept (ZFILE offered, frontend must
    /// pick a save directory). Drives the accept-wait cap in `idle_timeout` —
    /// deliberately NOT keyed on `last_feed`, because a sender waiting for our
    /// ZRPOS emits nothing, and one stray frame would reset a `last_feed`
    /// based cap back to zero and let a stuck session live forever.
    accept_since: Option<std::time::Instant>,
}

impl ZmodemReceiver {
    pub fn new() -> Self {
        Self {
            state: RxState::Probing,
            buf: Vec::with_capacity(8192),
            pending_write: Vec::new(),
            file_path: None,
            bytes_written: 0,
            file_size: 0,
            current_filename: String::new(),
            file_index: 0,
            scan_pos: 0,
            in_zdata: false,
            data_crc32: false,
            passthrough: false,
            noise_text: Vec::new(),
            noise_seen_at: None,
            start_emitted: false,
            last_feed: None,
            accept_since: None,
        }
    }

    /// Feed a chunk of raw bytes from the SSH channel. Returns actions the
    /// caller must perform (send bytes to peer, emit events to frontend).
    pub fn feed(&mut self, data: &[u8]) -> RxActions {
        let mut actions = RxActions::default();
        self.last_feed = Some(std::time::Instant::now());
        self.buf.extend_from_slice(data);

        // Direction probe: the first frame decides native-download vs JS
        // passthrough. Remote `sz` opens with ZRQINIT (download); remote `rz`
        // opens with ZRINIT (upload → hand off to JS).
        if self.state == RxState::Probing && !self.passthrough {
            match self.peek_first_typenum() {
                Some(frame_type::ZRQINIT) => { /* download — proceed to parse loop */ }
                Some(_) => {
                    self.passthrough = true;
                    return actions; // caller drains via take_passthrough()
                }
                None => return actions, // need more bytes to see the first frame
            }
        }
        if self.passthrough {
            return actions;
        }

        // Abort detection: lrzsz's `canit()` on failure (unreadable file,
        // target is a directory, user kill) sends a burst of 10× CAN followed
        // by 10× backspace, then exits. CAN shares the value 0x18 with ZDLE,
        // but valid data always transmits a literal 0x18 as a ZDLE-escaped
        // pair, so ≥5 consecutive raw CAN bytes can only be an abort. Without
        // this check the burst parses as no header at all and the receiver
        // waits forever — the terminal stays in Zmodem mode and swallows all
        // subsequent shell output (looks frozen).
        if let Some(msg) = Self::find_abort_burst(&self.buf[self.scan_pos.min(self.buf.len())..]) {
            log::warn!("[zmodem_rx] abort burst detected — ending session: {}", msg);
            self.buf.clear();
            self.scan_pos = 0;
            self.in_zdata = false;
            self.cleanup_file();
            actions.events.push(RxEvent::Error(msg));
            return actions;
        }

        loop {
            let made_progress = self.try_step(&mut actions);
            if !made_progress {
                break;
            }
        }

        // Dying-sender text: while waiting for the first ZFILE offer, any
        // UNCONSUMED tail that contains no ZDLE byte cannot be a protocol
        // frame (headers always carry ZDLE). sz that failed to open its
        // file (permission denied, missing path) prints its error straight
        // to the PTY and exits without a CAN burst or ZFERR — buffer the
        // text so the reader can surface it and end the session instead of
        // silently waiting out the 30s idle timeout with input blocked.
        // Runs AFTER the parse loop so a ZRQINIT + error text landing in
        // the same SSH packet is also caught. The tail is consumed (it is
        // terminal text, not protocol) so a later feed can't re-track it.
        if self.state == RxState::WaitingZfile && !self.in_zdata {
            let start = self.scan_pos.min(self.buf.len());
            let tail = &self.buf[start..];
            if !tail.is_empty() && !tail.contains(&ZDLE) {
                let printable = tail
                    .iter()
                    .filter(|&&b| (0x20..0x7f).contains(&b) || b >= 0x80)
                    .count();
                if printable >= 4 {
                    self.noise_text.extend_from_slice(tail);
                    if self.noise_seen_at.is_none() {
                        self.noise_seen_at = Some(std::time::Instant::now());
                    }
                    self.scan_pos = self.buf.len();
                }
            }
        }
        // Compact the buffer — drop already-consumed prefix.
        if self.scan_pos > 0 && self.scan_pos >= self.buf.len() {
            self.buf.clear();
            self.scan_pos = 0;
        } else if self.scan_pos > 8192 {
            self.buf.drain(..self.scan_pos);
            self.scan_pos = 0;
        }

        actions
    }

    /// If the receiver decided this is an upload (passthrough), return all
    /// buffered bytes so the caller can forward them to the JS bridge.
    pub fn take_passthrough(&mut self) -> Option<Vec<u8>> {
        if self.passthrough {
            Some(std::mem::take(&mut self.buf))
        } else {
            None
        }
    }

    /// Returns true exactly once, when the receiver has committed to the native
    /// download path (past Probing). The caller uses this to emit
    /// `zmodem_start(direction="download")` a single time.
    pub fn take_start_signal(&mut self) -> bool {
        if !self.passthrough && self.state != RxState::Probing && !self.start_emitted {
            self.start_emitted = true;
            true
        } else {
            false
        }
    }

    /// Scan for a lrzsz abort burst: ≥5 consecutive CAN (0x18) bytes.
    /// Returns a user-facing message when found.
    fn find_abort_burst(buf: &[u8]) -> Option<String> {
        let mut run = 0usize;
        for &b in buf {
            if b == ZDLE {
                run += 1;
                if run >= 5 {
                    return Some(
                        "远端中止了 ZMODEM 传输（sz 发送了取消序列，常见原因：文件不可读/权限不足，或目标是目录）"
                            .to_string(),
                    );
                }
            } else {
                run = 0;
            }
        }
        None
    }

    /// True when the session should be force-ended: the sender has been
    /// silent for longer than `limit` and we are NOT waiting for the user to
    /// pick a save directory (the directory picker can legitimately stay open
    /// for minutes — no protocol bytes flow while it's open). Covers the
    /// failure mode where sz dies without a CAN burst (e.g. killed by a
    /// signal) and the receiver would otherwise wait forever.
    ///
    /// WaitingAccept is capped separately and generously (5 min): the picker
    /// staying open is legitimate, but "the frontend never answers" (dialog
    /// lost, tab crashed mid-transfer, IPC dropped) used to hang the session
    /// *forever* — terminal stuck in Zmodem mode, swallowing all shell output
    /// and eating every keystroke, with Ctrl+C as the only exit.
    pub fn idle_timeout(&self, limit: std::time::Duration) -> bool {
        const ACCEPT_WAIT_CAP: std::time::Duration = std::time::Duration::from_secs(300);
        if self.state == RxState::WaitingAccept {
            return matches!(self.accept_since, Some(t) if t.elapsed() > ACCEPT_WAIT_CAP);
        }
        match self.last_feed {
            Some(t) => t.elapsed() > limit,
            None => false,
        }
    }

    /// True when plain-text output was seen waiting for the first offer and
    /// no protocol frame has arrived within `grace` — the sender is dead and
    /// its error text should be shown to the user (see `noise_text`). The
    /// reader passes a grace of ~2s so interleaved verbose output followed by
    /// a real ZFILE (which clears the tracker) doesn't false-positive.
    pub fn noise_timeout(&self, grace: std::time::Duration) -> bool {
        match self.noise_seen_at {
            Some(t) => t.elapsed() >= grace,
            None => false,
        }
    }

    /// Drain buffered dying-sender text so the reader can emit it as
    /// terminal output when the noise timeout fires.
    pub fn take_noise_text(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.noise_text)
    }

    /// Drain decoded payload accumulated since the last call. The caller writes
    /// this to disk asynchronously (e.g. via spawn_blocking) so the SSH reader
    /// loop is never blocked by disk I/O — critical for keeping russh's
    /// internal queue drained and WINDOW_ADJUST flowing at line rate.
    pub fn take_pending_write(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending_write)
    }

    /// Return any unconsumed bytes after the current scan position (e.g. the
    /// shell prompt that follows "OO"). Called when the session ends so the
    /// caller can route trailing bytes to terminal output.
    pub fn drain_trailing(&mut self) -> Vec<u8> {
        if self.scan_pos < self.buf.len() {
            let tail = self.buf.split_off(self.scan_pos);
            self.scan_pos = 0;
            tail
        } else {
            Vec::new()
        }
    }

    /// Peek the typenum of the first header without consuming it.
    fn peek_first_typenum(&self) -> Option<u8> {
        let mut start = self.scan_pos;
        while start < self.buf.len()
            && matches!(self.buf[start], XON | XOFF | 0x91 | 0x93)
        {
            start += 1;
        }
        let remaining = &self.buf[start..];
        let rel = self.find_header_start(remaining)?;
        let abs = start + rel;
        let (header, _) = self.try_parse_header(abs)?;
        Some(header.typenum)
    }

    /// Called when the frontend has chosen a path for the offered file.
    /// `path` = full path to write, or `None` to skip this file.
    /// Returns (actions, Option<File>) — the caller owns the file handle and
    /// is responsible for all writes (e.g. on a background disk task). The
    /// receiver itself never touches the file, so it never blocks the reader.
    pub fn accept_offer(
        &mut self,
        path: &Option<String>,
    ) -> (RxActions, Option<std::fs::File>) {
        let mut actions = RxActions::default();
        if self.state != RxState::WaitingAccept {
            return (actions, None);
        }
        match path {
            None => {
                // User skipped — send ZSKIP to move to the next file.
                actions.send = self.build_hex_header(frame_type::ZSKIP, [0, 0, 0, 0]);
                self.state = RxState::WaitingZfile;
                (actions, None)
            }
            Some(p) => {
                // `sz -r` offers carry relative subpaths ("dir/sub/file.txt")
                // — create the parent hierarchy so tree downloads mirror intact.
                //
                // The offer name comes from the REMOTE host, and this function
                // is the only filesystem sink in the ZMODEM receiver. It used
                // to carry a comment saying "the caller is responsible for
                // sanitizing the path" — but nothing enforced that: the module
                // never referenced `path_safety` at all (only sftp.rs / ftp.rs
                // do), and `ZmodemReceiver::accept_offer` is `pub`, reachable
                // straight from the Tauri command `zmodem_accept_offer`, which
                // forwards a webview-supplied path untouched. Enforce the
                // invariant HERE, where the bytes actually land, rather than
                // trusting every caller to remember.
                for comp in std::path::Path::new(p).components() {
                    use std::path::Component;
                    match comp {
                        // `..` is the one component that climbs out of the
                        // directory the user picked. Everything else is fine:
                        // the path IS an absolute location chosen in a native
                        // folder dialog, so RootDir / Prefix / CurDir are the
                        // normal case, not an attack.
                        Component::ParentDir => {
                            actions.events.push(RxEvent::Error(
                                "拒绝含上级引用(..)的保存路径".to_string(),
                            ));
                            actions.send = self.build_hex_header(frame_type::ZSKIP, [0, 0, 0, 0]);
                            self.state = RxState::WaitingZfile;
                            return (actions, None);
                        }
                        Component::Normal(s) => {
                            if let Some(name) = s.to_str() {
                                if let Err(e) = crate::path_safety::validate_component(name) {
                                    actions.events.push(RxEvent::Error(format!(
                                        "拒绝非法路径 {}: {}",
                                        name, e
                                    )));
                                    actions.send =
                                        self.build_hex_header(frame_type::ZSKIP, [0, 0, 0, 0]);
                                    self.state = RxState::WaitingZfile;
                                    return (actions, None);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(parent) = std::path::Path::new(p).parent() {
                    if !parent.as_os_str().is_empty() {
                        // No symlink/junction redirection out of the destination.
                        if let Err(e) =
                            crate::path_safety::ensure_no_symlink_components_sync(parent)
                        {
                            actions
                                .events
                                .push(RxEvent::Error(format!("拒绝写入: {}", e)));
                            actions.send =
                                self.build_hex_header(frame_type::ZSKIP, [0, 0, 0, 0]);
                            self.state = RxState::WaitingZfile;
                            return (actions, None);
                        }
                        if let Err(e) = std::fs::create_dir_all(parent) {
                            actions
                                .events
                                .push(RxEvent::Error(format!("无法创建目录: {}", e)));
                            // Skip this file on error.
                            actions.send = self.build_hex_header(frame_type::ZSKIP, [0, 0, 0, 0]);
                            self.state = RxState::WaitingZfile;
                            return (actions, None);
                        }
                    }
                }
                match std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(p)
                {
                    Ok(f) => {
                        // The caller owns the file handle; the receiver only
                        // tracks the path + bytes for progress/offset.
                        self.file_path = Some(p.clone());
                        self.bytes_written = 0;
                        // Send ZRPOS at offset 0 (fresh download).
                        actions.send = self.build_hex_header(frame_type::ZRPOS, [0, 0, 0, 0]);
                        self.state = RxState::Transferring;
                        self.in_zdata = false;
                        (actions, Some(f))
                    }
                    Err(e) => {
                        actions.events.push(RxEvent::Error(format!("无法创建文件: {}", e)));
                        // Skip this file on error.
                        actions.send = self.build_hex_header(frame_type::ZSKIP, [0, 0, 0, 0]);
                        self.state = RxState::WaitingZfile;
                        (actions, None)
                    }
                }
            }
        }
    }

    /// Try to advance the state machine by one step. Returns true if progress
    /// was made (caller should loop), false if we need more bytes.
    fn try_step(&mut self, actions: &mut RxActions) -> bool {
        // ── Done state: ZFIN already handled, SessionEnd already emitted. ──
        // No more parsing; the caller drops the receiver and the reader's
        // oo_eaten mechanism strips the trailing "OO" bytes.
        if self.state == RxState::Done {
            return false;
        }

        // ── In Transferring state: scan for ZDATA subpackets ──
        if self.state == RxState::Transferring && self.in_zdata {
            return self.scan_subpackets(actions);
        }

        // ── Otherwise: try to parse a header ──
        // First, skip XON/XOFF noise.
        while self.scan_pos < self.buf.len()
            && matches!(self.buf[self.scan_pos], XON | XOFF | 0x91 | 0x93)
        {
            self.scan_pos += 1;
        }

        if self.scan_pos >= self.buf.len() {
            return false;
        }

        // Look for a header starting with ZPAD (optionally ZPAD ZPAD) then ZDLE.
        // Scan forward to find ZDLE at a header boundary.
        let remaining = &self.buf[self.scan_pos..];

        // Find the next ZDLE that's part of a header (preceded by ZPAD).
        // Headers start with: ZPAD [ZPAD] ZDLE {ZBIN|ZHEX|ZBIN32}
        let hdr_start = self.find_header_start(remaining);
        match hdr_start {
            None => return false, // need more bytes
            Some(rel_pos) => {
                let abs_pos = self.scan_pos + rel_pos;
                // Try to parse a header starting here.
                match self.try_parse_header(abs_pos) {
                    None => return false, // incomplete — wait for more bytes
                    Some((header, consumed)) => {
                        self.scan_pos = abs_pos + consumed;
                        // A real protocol frame parsed — the sender is alive.
                        // Drop any buffered "dying sz" text (verbose-mode
                        // output interleaved with frames would end up here).
                        self.noise_text.clear();
                        self.noise_seen_at = None;
                        if self.handle_header(header, actions) {
                            return true;
                        }
                        // Handler needs more bytes (e.g. a control subpacket
                        // split across data chunks). Rewind to the header start
                        // so we re-parse header+subpacket atomically once the
                        // remaining bytes arrive.
                        self.scan_pos = abs_pos;
                        return false;
                    }
                }
            }
        }
    }

    /// Find the start of a header: ZPAD [ZPAD] ZDLE. Returns the offset
    /// relative to `remaining` of the ZDLE byte (so caller knows the header
    /// format byte is at offset+1).
    fn find_header_start(&self, remaining: &[u8]) -> Option<usize> {
        // The ZPAD prefix is MANDATORY, and the old code never checked it —
        // it accepted any ZDLE followed by a format byte. That is ambiguous
        // with escaped payload: a data byte `x` goes on the wire as
        // `0x18, x ^ 0x40`, so payload bytes 0x01 / 0x02 / 0x03 appear as
        // `0x18 0x41` (ZBIN), `0x18 0x42` (ZHEX), `0x18 0x43` (ZBIN32) — all
        // three accepted. When such a pair lands where header parsing is
        // active, `try_parse_header` returns None (the following bytes aren't
        // hex), `scan_pos` never advances, and the receiver is wedged for the
        // rest of the session: `feed` keeps refreshing `last_feed` so the idle
        // timeout never trips, the terminal stays locked in Zmodem mode
        // swallowing all shell output, and `buf` grows without bound.
        //
        // Requiring the ZPAD prefix removes the ambiguity entirely.
        let mut i = 0usize;
        while i < remaining.len() {
            if remaining[i] == ZDLE && i + 1 < remaining.len() {
                let next = remaining[i + 1];
                if next == ZBIN || next == ZHEX || next == ZBIN32 {
                    // lrzsz emits "ZPAD ZDLE" or "ZPAD ZPAD ZDLE"; tolerate
                    // leading noise by allowing ZPAD(s) immediately before.
                    let preceded = if i >= 1 {
                        remaining[i - 1] == ZPAD
                    } else {
                        // At the very start of the scan window there is no
                        // preceding byte; the caller may have consumed the
                        // ZPAD already, so accept position 0.
                        true
                    };
                    if preceded {
                        return Some(i);
                    }
                }
            }
            i += 1;
        }
        None
    }

    /// Try to parse a header at `abs_pos` (pointing at ZDLE). Returns the
    /// parsed header and the number of bytes consumed (from ZDLE onwards).
    fn try_parse_header(&self, abs_pos: usize) -> Option<(ParsedHeader, usize)> {
        let b = &self.buf;

        if abs_pos >= b.len() {
            return None;
        }
        // abs_pos should be ZDLE. The byte after determines format.
        if b[abs_pos] != ZDLE {
            return None;
        }
        if abs_pos + 1 >= b.len() {
            return None;
        }
        let fmt = b[abs_pos + 1];
        match fmt {
            ZHEX => self.parse_hex_header(abs_pos),
            ZBIN => self.parse_bin16_header(abs_pos),
            ZBIN32 => self.parse_bin32_header(abs_pos),
            _ => None,
        }
    }

    /// Hex header: ZDLE ZHEX <2 hex: type> <8 hex: data> <4 hex: crc> CR LF [XON]
    fn parse_hex_header(&self, pos: usize) -> Option<(ParsedHeader, usize)> {
        let b = &self.buf;
        // We need: 2 (ZDLE+ZHEX) + 2 + 8 + 4 = 16 hex chars + CR LF [XON]
        let hex_start = pos + 2;
        let min_len = hex_start + 14 + 2; // 14 hex chars + CR LF
        if b.len() < min_len {
            return None;
        }
        let hex_bytes = &b[hex_start..hex_start + 14];
        let mut decoded = [0u8; 7]; // type + 4 data + 2 crc
        for i in 0..7 {
            let hi = hex_val(hex_bytes[i * 2])?;
            let lo = hex_val(hex_bytes[i * 2 + 1])?;
            decoded[i] = (hi << 4) | lo;
        }
        let typenum = decoded[0];
        let data = [decoded[1], decoded[2], decoded[3], decoded[4]];
        // Skip CR LF [XON]
        let mut consumed = (hex_start + 14) - pos;
        // Expect CR (0x0d or 0x8d) LF (0x0a or 0x8a) [XON]
        let after_hex = hex_start + 14;
        if after_hex < b.len() && matches!(b[after_hex], 0x0d | 0x8d) {
            consumed += 1;
            if after_hex + 1 < b.len() && matches!(b[after_hex + 1], 0x0a | 0x8a) {
                consumed += 1;
                // Optional XON
                if after_hex + 2 < b.len() && b[after_hex + 2] == XON {
                    consumed += 1;
                }
            }
        }
        Some((ParsedHeader { typenum, data, crc32: false }, consumed))
    }

    /// Binary16 header: ZDLE ZBIN <ZDLE-encoded: type + 4 data + 2 crc>
    /// 7 logical bytes, each possibly ZDLE-escaped.
    fn parse_bin16_header(&self, pos: usize) -> Option<(ParsedHeader, usize)> {
        let b = &self.buf;
        let start = pos + 2; // skip ZDLE ZBIN
        let mut decoded = [0u8; 7];
        let mut consumed_logical = 0usize;
        let mut raw_idx = start;
        while consumed_logical < 7 {
            if raw_idx >= b.len() {
                return None; // incomplete
            }
            let byte = b[raw_idx];
            if byte == ZDLE {
                if raw_idx + 1 >= b.len() {
                    return None;
                }
                decoded[consumed_logical] = b[raw_idx + 1] ^ 0x40;
                raw_idx += 2;
            } else {
                decoded[consumed_logical] = byte;
                raw_idx += 1;
            }
            consumed_logical += 1;
        }
        let typenum = decoded[0];
        let data = [decoded[1], decoded[2], decoded[3], decoded[4]];
        let consumed = raw_idx - pos;
        Some((ParsedHeader { typenum, data, crc32: false }, consumed))
    }

    /// Binary32 header: ZDLE ZBIN32 <ZDLE-encoded: type + 4 data + 4 crc>
    /// 9 logical bytes.
    fn parse_bin32_header(&self, pos: usize) -> Option<(ParsedHeader, usize)> {
        let b = &self.buf;
        let start = pos + 2;
        let mut decoded = [0u8; 9];
        let mut consumed_logical = 0usize;
        let mut raw_idx = start;
        while consumed_logical < 9 {
            if raw_idx >= b.len() {
                return None;
            }
            let byte = b[raw_idx];
            if byte == ZDLE {
                if raw_idx + 1 >= b.len() {
                    return None;
                }
                decoded[consumed_logical] = b[raw_idx + 1] ^ 0x40;
                raw_idx += 2;
            } else {
                decoded[consumed_logical] = byte;
                raw_idx += 1;
            }
            consumed_logical += 1;
        }
        let typenum = decoded[0];
        let data = [decoded[1], decoded[2], decoded[3], decoded[4]];
        let consumed = raw_idx - pos;
        Some((ParsedHeader { typenum, data, crc32: true }, consumed))
    }

    /// Handle a parsed header according to the current state. Returns true if
    /// progress was made; false if the handler needs more bytes (a control
    /// subpacket split across data chunks) — the caller then rewinds scan_pos.
    fn handle_header(&mut self, header: ParsedHeader, actions: &mut RxActions) -> bool {
        // The subpacket CRC width follows the header that introduces the frame.
        self.data_crc32 = header.crc32;
        match self.state {
            RxState::Probing => self.handle_probing_header(header, actions),
            RxState::WaitingZfile => self.handle_waiting_zfile(header, actions),
            RxState::Transferring => self.handle_transferring_header(header, actions),
            _ => {
                // In WaitingAccept / Done, headers are unexpected here but we
                // handle ZFIN defensively.
                if header.typenum == frame_type::ZFIN {
                    self.handle_zfin(actions);
                }
                true
            }
        }
    }

    fn handle_probing_header(&mut self, header: ParsedHeader, actions: &mut RxActions) -> bool {
        match header.typenum {
            frame_type::ZRQINIT => {
                // Remote is sz (sender) — activate native download.
                log::warn!("[zmodem_rx] ZRQINIT detected — native download mode");
                self.state = RxState::WaitingZfile;
                // Send ZRINIT.
                actions.send = self.build_hex_header(
                    frame_type::ZRINIT,
                    zrinit_data(),
                );
            }
            // ZRINIT means remote is rz — upload. We should NOT be here;
            // caller checks direction and routes to JS path.
            _ => {
                // Unexpected — emit error.
                actions
                    .events
                    .push(RxEvent::Error("非下载方向，原生接收器不适用".into()));
            }
        }
        true
    }

    fn handle_waiting_zfile(&mut self, header: ParsedHeader, actions: &mut RxActions) -> bool {
        match header.typenum {
            frame_type::ZSINIT => {
                // ZSINIT carries an attention-string subpacket (ZCRCW). Consume
                // it, then respond with ZACK. If the subpacket isn't fully
                // buffered yet, signal "need more data" so the caller rewinds.
                if self.consume_zcrcw_subpacket().is_none() {
                    return false;
                }
                actions.send = self.build_hex_header(frame_type::ZACK, [0, 0, 0, 0]);
                true
            }
            frame_type::ZFILE => {
                // ZFILE carries the filename+metadata subpacket (ZCRCW).
                let payload = match self.consume_zcrcw_subpacket() {
                    Some(p) => p,
                    None => return false, // incomplete — wait for more bytes
                };
                match Self::parse_zfile_fields(&payload) {
                    Some((name, size)) => {
                        log::warn!("[zmodem_rx] ZFILE offer: name={:?} size={}", name, size);
                        self.current_filename = name.clone();
                        self.file_size = size;
                        self.file_index += 1;
                        self.state = RxState::WaitingAccept;
                        self.accept_since = Some(std::time::Instant::now());
                        actions.events.push(RxEvent::Offer { name, size });
                    }
                    None => {
                        log::warn!("[zmodem_rx] ZFILE payload unparseable — sending ZSKIP");
                        actions.send = self.build_hex_header(frame_type::ZSKIP, [0, 0, 0, 0]);
                    }
                }
                true
            }
            frame_type::ZFIN => {
                self.handle_zfin(actions);
                true
            }
            frame_type::ZABORT | frame_type::ZFERR => {
                // Sender-side abort/error frame (e.g. unreadable file).
                // Respond with nothing — the sender is already going away —
                // and end the session so the reader returns to Normal mode.
                log::warn!("[zmodem_rx] ZABORT/ZFERR received in WaitingZfile");
                self.cleanup_file();
                actions
                    .events
                    .push(RxEvent::Error("远端中止了 ZMODEM 传输（收到 ZABORT/ZFERR 帧）".into()));
                true
            }
            _ => true,
        }
    }

    fn handle_transferring_header(&mut self, header: ParsedHeader, actions: &mut RxActions) -> bool {
        match header.typenum {
            frame_type::ZDATA => {
                // ZDATA marks the start of a data stream. The data field
                // contains the starting offset (4 LE bytes). The header format
                // determines the subpacket CRC width for this stream.
                self.in_zdata = true;
                self.data_crc32 = header.crc32;
                // Subpackets will be scanned in the next try_step iteration.
            }
            frame_type::ZEOF => {
                // File complete. The caller is responsible for flushing the
                // disk task — signaled via the RxEvent::FileComplete event
                // (the reader loop sends DiskJob::Flush / Close).
                self.in_zdata = false;
                log::warn!(
                    "[zmodem_rx] ZEOF received: file {:?} complete ({} bytes, expected {})",
                    self.current_filename, self.bytes_written, self.file_size
                );
                // Force a final progress event so the frontend shows 100%
                // even for small files that never hit the 512KB threshold.
                actions.events.push(RxEvent::Progress {
                    written: self.bytes_written,
                    total: self.file_size,
                });
                actions.events.push(RxEvent::FileComplete {
                    name: self.current_filename.clone(),
                    written: self.bytes_written,
                });
                self.cleanup_file();
                // Send ZRINIT to accept the next file.
                self.state = RxState::WaitingZfile;
                actions.send = self.build_hex_header(
                    frame_type::ZRINIT,
                    zrinit_data(),
                );
            }
            frame_type::ZFIN => {
                self.in_zdata = false;
                self.handle_zfin(actions);
            }
            frame_type::ZABORT | frame_type::ZFERR => {
                // Sender aborted mid-transfer (e.g. it hit an I/O error).
                // Keep the partial file (lrzsz-compatible) and end the session.
                log::warn!("[zmodem_rx] ZABORT/ZFERR received in Transferring");
                self.in_zdata = false;
                actions.events.push(RxEvent::Progress {
                    written: self.bytes_written,
                    total: self.file_size,
                });
                self.cleanup_file();
                actions
                    .events
                    .push(RxEvent::Error("远端中止了 ZMODEM 传输（收到 ZABORT/ZFERR 帧）".into()));
            }
            _ => {}
        }
        true
    }

    fn handle_zfin(&mut self, actions: &mut RxActions) {
        log::warn!("[zmodem_rx] ZFIN received — session ending");
        self.cleanup_file();
        self.state = RxState::Done;
        // Echo ZFIN back (hex, no XON termination), then signal session end.
        // The trailing "OO" (Over-and-Out) bytes lrzsz prints afterwards are
        // stripped by the reader's oo_eaten mechanism once we flip to Normal.
        actions.send = self.build_hex_header_no_xon(frame_type::ZFIN, [0, 0, 0, 0]);
        actions.events.push(RxEvent::SessionEnd);
    }

    /// Scan the buffer for ZDATA subpackets (after a ZDATA header).
    /// Each subpacket ends with ZDLE + {ZCRCE|ZCRCG|ZCRCQ|ZCRCW}.
    fn scan_subpackets(&mut self, actions: &mut RxActions) -> bool {
        let b = &self.buf;
        let mut pos = self.scan_pos;
        let mut made_progress = false;

        while pos < b.len() {
            // Look for ZDLE + frame-end marker.
            if b[pos] == ZDLE && pos + 1 < b.len() {
                let marker = b[pos + 1];
                if marker == subpkt_end::ZCRCE
                    || marker == subpkt_end::ZCRCG
                    || marker == subpkt_end::ZCRCQ
                    || marker == subpkt_end::ZCRCW
                {
                    // Found end of a subpacket. Before decoding the payload,
                    // verify the trailing CRC is fully buffered. CRC is 2 bytes
                    // (CRC16) or 4 bytes (CRC32), ZDLE-encoded. We skip CRC
                    // entirely (reliable SSH transport). If the CRC bytes
                    // aren't fully buffered yet (Data chunk boundary cut
                    // mid-CRC), DON'T consume this subpacket — wait for more
                    // data. Otherwise we'd misalign and write CRC fragments as
                    // file data.
                    //
                    // CRITICAL: the CRC check MUST happen before we touch
                    // pending_write. If we decode+write the payload first and
                    // then discover the CRC is incomplete (break), scan_pos
                    // won't advance — and the next feed() will find the same
                    // subpacket again, decoding and writing it a SECOND time.
                    // That double-write was the root cause of files growing by
                    // tens of KB on large transfers.
                    let after_marker = pos + 2;
                    let crc_consumed = match skip_zdle_crc(&b[after_marker..], self.data_crc32) {
                        Some(n) => n,
                        None => {
                            // CRC incomplete — stop scanning, wait for more data.
                            // No side effects: pending_write untouched, scan_pos
                            // stays put, so the next feed() re-finds this
                            // subpacket cleanly.
                            break;
                        }
                    };

                    // CRC complete — safe to decode and collect the payload.
                    let payload_start = self.scan_pos;
                    let payload_end = pos;
                    let payload = zdle_decode(&b[payload_start..payload_end]);

                    if !payload.is_empty() {
                        self.pending_write.extend_from_slice(&payload);
                        self.bytes_written += payload.len() as u64;
                    }

                    let new_pos = after_marker + crc_consumed;
                    self.scan_pos = new_pos;
                    pos = new_pos;
                    made_progress = true;

                    // Emit progress periodically (every ~512KB).
                    if self.bytes_written % (512 * 1024) < payload.len() as u64 {
                        actions.events.push(RxEvent::Progress {
                            written: self.bytes_written,
                            total: self.file_size,
                        });
                    }

                    // ZCRCE = end of ZDATA frame (a header follows, typically ZEOF).
                    // ZCRCG = firehose continues — keep scanning.
                    // ZCRCQ = ack expected — we send ZACK and keep scanning.
                    // ZCRCW = end of frame AND ack expected. The control-frame
                    //   path already honours it (`consume_zcrcw_subpacket`),
                    //   but the DATA scanner did not, so a sender that ends a
                    //   data frame with ZCRCW — which the module's own
                    //   subpkt_end doc calls the correct semantic — had the
                    //   FOLLOWING ZEOF header decoded as subpacket payload and
                    //   appended to the file. Treat it as a frame end here too.
                    if marker == subpkt_end::ZCRCE || marker == subpkt_end::ZCRCW {
                        if marker == subpkt_end::ZCRCW {
                            actions.send.extend_from_slice(
                                &self.build_hex_header(
                                    frame_type::ZACK,
                                    offset_bytes(self.bytes_written),
                                ),
                            );
                        }
                        self.in_zdata = false;
                        return true; // back to header-parsing mode
                    }
                    if marker == subpkt_end::ZCRCQ {
                        // In the ZDATA phase the ZACK position is the count of
                        // data bytes accepted so far — exactly `bytes_written`.
                        // Hard-coding 0 told a stop-and-wait sender that nothing
                        // arrived, so it either re-sent the whole window or
                        // stalled; the counter was in scope and unused.
                        actions.send.extend_from_slice(
                            &self
                                .build_hex_header(frame_type::ZACK, offset_bytes(self.bytes_written)),
                        );
                    }
                    continue;
                }
                // A ZDLE-escaped data byte (not a frame-end marker) — skip the
                // whole pair so its decoded value can't be mistaken for a marker.
                pos += 2;
                continue;
            }
            pos += 1;
        }

        // No more complete subpackets found. Emit progress if we wrote data.
        if made_progress {
            actions.events.push(RxEvent::Progress {
                written: self.bytes_written,
                total: self.file_size,
            });
        }
        // Stay in subpacket mode — we'll resume from scan_pos next feed().
        // But we need to NOT lose unconsumed payload. The scan_pos should
        // remain at the start of unconsumed payload.
        // Note: made_progress=true means we found at least one complete
        // subpacket and advanced scan_pos past it.
        made_progress
    }

    /// Consume a ZCRCW-terminated control subpacket (ZSINIT/ZFILE) starting at
    /// scan_pos. Decodes the ZDLE-encoded payload and advances scan_pos past
    /// the frame-end marker and trailing CRC. Returns the decoded payload, or
    /// None if the end marker isn't buffered yet (caller should wait for more
    /// bytes and retry).
    fn consume_zcrcw_subpacket(&mut self) -> Option<Vec<u8>> {
        let b = &self.buf;
        let start = self.scan_pos;
        let mut i = start;
        while i + 1 < b.len() {
            if b[i] == ZDLE {
                let nb = b[i + 1];
                if nb == subpkt_end::ZCRCW || nb == subpkt_end::ZCRCE {
                    let payload = zdle_decode(&b[start..i]);
                    let after_marker = i + 2;
                    let crc_consumed = skip_zdle_crc(&b[after_marker..], self.data_crc32)?;
                    self.scan_pos = after_marker + crc_consumed;
                    return Some(payload);
                }
                // An escaped data byte — skip the pair so its decoded value
                // can't be mistaken for a frame-end marker.
                i += 2;
            } else {
                i += 1;
            }
        }
        None
    }

    /// Parse a decoded ZFILE subpacket payload into (filename, size).
    /// Format: `<name>\0<size> <mtime> <mode> <serial> [<files_remaining> <bytes_remaining>]`
    fn parse_zfile_fields(payload: &[u8]) -> Option<(String, u64)> {
        let nul_pos = payload.iter().position(|&b| b == 0)?;
        let name = String::from_utf8_lossy(&payload[..nul_pos]).to_string();
        let rest = &payload[nul_pos + 1..];
        let rest_str = String::from_utf8_lossy(rest);
        let mut parts = rest_str.split_whitespace();
        let size_str = parts.next()?;
        let size: u64 = size_str.parse().ok()?;
        Some((name, size))
    }

    /// Reset per-file bookkeeping. The file handle itself is owned by the
    /// caller's background disk task — closing is signaled via DiskJob::Close.
    fn cleanup_file(&mut self) {
        self.file_path = None;
        self.bytes_written = 0;
        self.file_size = 0;
    }

    // ── Header builders ───────────────────────────────────────────────────

    /// Build a hex header with CR LF XON termination.
    fn build_hex_header(&self, typenum: u8, data: [u8; 4]) -> Vec<u8> {
        self.build_hex_header_impl(typenum, data, true)
    }

    /// Build a hex header with CR LF only (no XON) — for ZACK/ZFIN.
    fn build_hex_header_no_xon(&self, typenum: u8, data: [u8; 4]) -> Vec<u8> {
        self.build_hex_header_impl(typenum, data, false)
    }

    fn build_hex_header_impl(&self, typenum: u8, data: [u8; 4], with_xon: bool) -> Vec<u8> {
        // Compute CRC16 over [typenum, data...]
        let crc_bytes = [typenum, data[0], data[1], data[2], data[3]];
        let crc = crc16_xmodem(&crc_bytes);
        let crc_hi = (crc >> 8) as u8;
        let crc_lo = (crc & 0xff) as u8;

        let mut out = Vec::with_capacity(21);
        out.extend_from_slice(&[ZPAD, ZPAD, ZDLE, ZHEX]);
        // type as 2 hex chars
        out.push(hex_char(typenum >> 4));
        out.push(hex_char(typenum & 0x0f));
        // 4 data bytes as 8 hex chars
        for &d in &data {
            out.push(hex_char(d >> 4));
            out.push(hex_char(d & 0x0f));
        }
        // CRC as 4 hex chars (big-endian)
        out.push(hex_char(crc_hi >> 4));
        out.push(hex_char(crc_hi & 0x0f));
        out.push(hex_char(crc_lo >> 4));
        out.push(hex_char(crc_lo & 0x0f));
        // CR LF [XON]
        out.push(0x0d);
        out.push(0x0a);
        if with_xon {
            out.push(XON);
        }
        out
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

pub(crate) fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn hex_char(n: u8) -> u8 {
    if n < 10 {
        b'0' + n
    } else {
        b'a' + (n - 10)
    }
}

/// ZDLE decode: every 0x18 byte means XOR the next byte with 0x40.
///
/// XON/XOFF are stripped unconditionally — they are the classic modem flow
/// control pair and a ZMODEM sender will never carry them as data.
///
/// 0x91 / 0x93 are a different matter: per the protocol those two may only be
/// stripped when the sender negotiated ESCCTL. Our ZRINIT did NOT (see
/// `ZRINIT_FLAGS`), so a conforming sender is free to transmit 0x90–0x9F
/// literally. Stripping them unconditionally deleted real file bytes —
/// unrecoverably, since the subpacket CRC is never verified either. Our own
/// transmitter escapes the whole 0x80–0x9F range, which is why the bug was
/// invisible in local testing.
pub(crate) fn zdle_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == ZDLE && i + 1 < data.len() {
            out.push(data[i + 1] ^ 0x40);
            i += 2;
        } else {
            if !matches!(data[i], XON | XOFF) {
                out.push(data[i]);
            }
            i += 1;
        }
    }
    out
}

/// Skip the CRC bytes after a subpacket frame-end marker. The CRC is
/// ZDLE-encoded: 2 logical bytes for CRC16, 4 for CRC32 (each logical byte may
/// occupy 1 or 2 raw bytes if ZDLE-escaped). Returns Some(consumed) if the full
/// CRC was present, or None if the buffer doesn't yet contain all CRC bytes
/// (caller should wait for more data before consuming this subpacket).
fn skip_zdle_crc(after_marker: &[u8], crc32: bool) -> Option<usize> {
    let logical_crc_bytes = if crc32 { 4 } else { 2 };
    let mut consumed = 0;
    let mut logical = 0;
    while logical < logical_crc_bytes {
        if consumed >= after_marker.len() {
            // CRC incomplete — not enough bytes buffered yet.
            return None;
        }
        if after_marker[consumed] == ZDLE {
            if consumed + 1 >= after_marker.len() {
                return None; // ZDLE escape pair incomplete
            }
            consumed += 2;
        } else {
            consumed += 1;
        }
        logical += 1;
    }
    Some(consumed)
}

/// CRC-16 as ZMODEM uses it (CRC-CCITT/XModem variant). This replicates
/// zmodem.js's `zcrc.js` exactly: seed with the first byte, fold each
/// subsequent byte through a table-driven `_updcrc`, then finalize with two
/// zero bytes. NOTE: the naive "XOR byte into the high byte then shift"
/// XModem formulation produces a DIFFERENT result and is rejected by lrzsz —
/// the table form below (new byte folded into the low byte) is the one lrzsz
/// and zmodem.js agree on. Verified: ZRINIT [01,00,00,00,23] → 0xbe50.
fn crc16_tab() -> &'static [u16; 256] {
    static TAB: std::sync::OnceLock<[u16; 256]> = std::sync::OnceLock::new();
    TAB.get_or_init(|| {
        let mut tab = [0u16; 256];
        for d in 0..256u16 {
            let mut curr = (d << 8) & 0xffff;
            for _ in 0..8 {
                curr = if curr & 0x8000 != 0 {
                    (curr << 1) ^ 0x1021
                } else {
                    curr << 1
                };
                curr &= 0xffff;
            }
            tab[d as usize] = curr;
        }
        tab
    })
}

#[inline]
fn updcrc(cp: u8, crc: u16, tab: &[u16; 256]) -> u16 {
    tab[((crc >> 8) & 255) as usize] ^ ((crc & 255) << 8) ^ (cp as u16)
}

pub(crate) fn crc16_xmodem(bytes: &[u8]) -> u16 {
    if bytes.is_empty() {
        return 0;
    }
    let tab = crc16_tab();
    let mut crc = bytes[0] as u16;
    for &b in &bytes[1..] {
        crc = updcrc(b, crc, tab);
    }
    crc = updcrc(0, updcrc(0, crc, tab), tab);
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CAN — ZMODEM cancel byte (0x18, shares the value with ZDLE).
    const CAN: u8 = 0x18;

    /// Build a hex header with dummy (unverified) CRC. Reproduces the wire
    /// format lrzsz uses for control frames: ZPAD ZPAD ZDLE ZHEX <2 hex type>
    /// <8 hex data> <4 hex crc> CR LF.
    fn hex_header(typenum: u8) -> Vec<u8> {
        let mut out = vec![ZPAD, ZPAD, ZDLE, ZHEX];
        out.extend_from_slice(format!("{:02x}", typenum).as_bytes());
        out.extend_from_slice(b"00000000"); // data
        out.extend_from_slice(b"0000"); // crc (parser skips, doesn't verify)
        out.push(0x0d);
        out.push(0x0a);
        out
    }

    /// ZFILE header + filename/metadata subpacket terminated by ZDLE ZCRCW.
    fn zfile_frame(name: &str, size: u64) -> Vec<u8> {
        let mut out = hex_header(frame_type::ZFILE);
        let mut payload = name.as_bytes().to_vec();
        payload.push(0);
        payload.extend_from_slice(format!("{} 0 0 0", size).as_bytes());
        // Printable ASCII + NUL need no ZDLE escaping on the wire.
        out.extend_from_slice(&payload);
        out.extend_from_slice(&[ZDLE, subpkt_end::ZCRCW]);
        out.extend_from_slice(&[0x00, 0x00]); // CRC16 (skipped by parser)
        out
    }

    /// lrzsz canit(): 10× CAN followed by 10× backspace.
    fn canit_burst() -> Vec<u8> {
        let mut v = vec![CAN; 10];
        v.extend_from_slice(&[0x08; 10]);
        v
    }

    #[test]
    fn abort_burst_after_zrqinit_ends_session() {
        let mut rx = ZmodemReceiver::new();
        let actions = rx.feed(&hex_header(frame_type::ZRQINIT));
        assert!(
            !actions.send.is_empty(),
            "ZRQINIT must be answered with ZRINIT"
        );
        assert!(rx.take_start_signal());

        let actions = rx.feed(&canit_burst());
        assert!(actions.send.is_empty(), "no protocol reply to a dead sender");
        assert!(
            matches!(actions.events.last(), Some(RxEvent::Error(_))),
            "abort burst must surface as an Error event (→ session end), got {:?}",
            actions.events
        );
    }

    #[test]
    fn abort_burst_split_across_feeds() {
        let mut rx = ZmodemReceiver::new();
        let _ = rx.feed(&hex_header(frame_type::ZRQINIT));
        let actions = rx.feed(&[CAN; 3]);
        assert!(actions.events.is_empty(), "below threshold — keep waiting");
        let actions = rx.feed(&[CAN; 3]);
        assert!(
            matches!(actions.events.last(), Some(RxEvent::Error(_))),
            "6 total consecutive CANs must trip the detector"
        );
    }

    #[test]
    fn abort_burst_in_same_feed_as_zrqinit() {
        // sz can fail so fast that the burst lands in the same SSH packet as
        // the handshake — the session must end without ever emitting
        // zmodem_start (take_start_signal stays false).
        let mut rx = ZmodemReceiver::new();
        let mut data = hex_header(frame_type::ZRQINIT);
        data.extend_from_slice(&canit_burst());
        let actions = rx.feed(&data);
        assert!(
            matches!(actions.events.last(), Some(RxEvent::Error(_))),
            "got {:?}",
            actions.events
        );
        assert!(!rx.take_start_signal());
    }

    #[test]
    fn zabort_frame_ends_session() {
        let mut rx = ZmodemReceiver::new();
        let _ = rx.feed(&hex_header(frame_type::ZRQINIT));
        let actions = rx.feed(&hex_header(frame_type::ZABORT));
        assert!(
            matches!(actions.events.last(), Some(RxEvent::Error(_))),
            "got {:?}",
            actions.events
        );
    }

    #[test]
    fn escaped_zdle_data_is_not_mistaken_for_abort() {
        // During a healthy transfer every literal 0x18 data byte travels as a
        // ZDLE pair (0x18, x^0x40) — alternating bytes, never a raw run of
        // 5+ CANs. Feed a ZFILE + ZDATA + escaped payload + ZEOF sequence and
        // assert no error fires.
        let mut rx = ZmodemReceiver::new();
        let _ = rx.feed(&hex_header(frame_type::ZRQINIT));
        let actions = rx.feed(&zfile_frame("t.txt", 4));
        assert!(actions.events.iter().any(|e| matches!(e, RxEvent::Offer { .. })));
        // Save path in the system temp dir so accept_offer can create it.
        let save_path = std::env::temp_dir().join("zmodem_rx_test_escaped_zdle.bin");
        let (a2, file) = rx.accept_offer(&Some(save_path.to_string_lossy().to_string()));
        assert!(file.is_some(), "accept_offer must create the temp file");
        assert!(!a2.send.is_empty(), "accept must send ZRPOS");

        // ZDATA header + subpacket "a<0x18>b" (0x18 escaped as ZDLE 0x58) + ZCRCE + crc.
        let mut zdata = vec![ZPAD, ZPAD, ZDLE, ZBIN32];
        // type ZDATA=10, offset 0, dummy crc32 bytes — parser skips crc.
        zdata.extend_from_slice(&[0x0a, 0x00, 0x00, 0x00, 0x00, 1, 2, 3, 4]);
        zdata.extend_from_slice(b"a");
        zdata.extend_from_slice(&[ZDLE, 0x18 ^ 0x40]); // escaped 0x18 data byte
        zdata.extend_from_slice(b"b");
        zdata.extend_from_slice(&[ZDLE, subpkt_end::ZCRCE, 0x00, 0x00, 0x00, 0x00]);
        let actions = rx.feed(&zdata);
        assert!(
            !actions.events.iter().any(|e| matches!(e, RxEvent::Error(_))),
            "healthy escaped stream must not trip the abort detector"
        );
        let pending = rx.take_pending_write();
        assert_eq!(pending, vec![b'a', 0x18, b'b'], "decoded payload mismatch");
        let _ = std::fs::remove_file(save_path);
    }

    #[test]
    fn idle_timeout_exempts_waiting_accept_but_caps_it() {
        let mut rx = ZmodemReceiver::new();
        let _ = rx.feed(&hex_header(frame_type::ZRQINIT));
        // Just fed — a 1-hour idle limit must not trip.
        assert!(!rx.idle_timeout(std::time::Duration::from_secs(3600)));

        // Offer arrives → WaitingAccept: the directory picker may stay open
        // for minutes, so the normal 30s watchdog must not fire, not even
        // with a zero limit.
        let _ = rx.feed(&zfile_frame("a.txt", 1));
        assert_eq!(rx.state, RxState::WaitingAccept);
        assert!(!rx.idle_timeout(std::time::Duration::ZERO));

        // …but "the frontend never answers" must not hang the session
        // forever either. Past the cap the receiver force-ends so the
        // terminal leaves ZMODEM mode without the user reaching for Ctrl+C.
        rx.accept_since = Some(std::time::Instant::now() - std::time::Duration::from_secs(301));
        assert!(rx.idle_timeout(std::time::Duration::ZERO));
    }

    #[test]
    fn accept_wait_cap_is_not_reset_by_stray_bytes() {
        // The cap must run from "entered WaitingAccept", not from the last
        // byte received: the sender is waiting for our ZRPOS and emits
        // nothing, and a stray frame must not buy the session another 5 min.
        let mut rx = ZmodemReceiver::new();
        let _ = rx.feed(&hex_header(frame_type::ZRQINIT));
        let _ = rx.feed(&zfile_frame("a.txt", 1));
        let stale = Some(std::time::Instant::now() - std::time::Duration::from_secs(301));

        // A fresh feed() would normally refresh last_feed.
        rx.feed(b"");
        assert_eq!(rx.state, RxState::WaitingAccept);
        rx.accept_since = stale;
        assert!(rx.idle_timeout(std::time::Duration::ZERO));
    }

    #[test]
    fn plain_text_before_offer_trips_noise_timeout() {
        // sz that can't read its file prints the error to the PTY and exits
        // without CAN/ZFERR. The receiver must surface that text via
        // take_noise_text() once the grace period passes.
        let mut rx = ZmodemReceiver::new();
        let _ = rx.feed(&hex_header(frame_type::ZRQINIT));
        let actions = rx.feed(b"sz: postgresql-01.log: Permission denied\r\n");
        assert!(
            actions.events.is_empty(),
            "text alone must not end the session instantly (grace period)"
        );
        assert!(rx.noise_timeout(std::time::Duration::ZERO));
        assert_eq!(
            rx.take_noise_text(),
            b"sz: postgresql-01.log: Permission denied\r\n".to_vec()
        );
    }

    #[test]
    fn noise_in_same_packet_as_zrqinit_is_tracked() {
        // sz can fail so fast that ZRQINIT and its error text land in the
        // same SSH packet — the post-parse tail scan must still catch the
        // text once the header has been consumed.
        let mut rx = ZmodemReceiver::new();
        let mut data = hex_header(frame_type::ZRQINIT);
        data.extend_from_slice(b"rz: /var/log/x.log: Permission denied\r\n");
        let _ = rx.feed(&data);
        assert!(rx.noise_timeout(std::time::Duration::ZERO));
        assert_eq!(
            rx.take_noise_text(),
            b"rz: /var/log/x.log: Permission denied\r\n".to_vec()
        );
    }

    #[test]
    fn real_frame_clears_noise_tracker() {
        // Verbose sz may interleave text with real frames — a subsequent
        // ZFILE must clear the noise tracker so the session continues.
        let mut rx = ZmodemReceiver::new();
        let _ = rx.feed(&hex_header(frame_type::ZRQINIT));
        let _ = rx.feed(b"some verbose line\r\n");
        assert!(rx.noise_timeout(std::time::Duration::ZERO));
        let actions = rx.feed(&zfile_frame("ok.txt", 5));
        assert!(actions.events.iter().any(|e| matches!(e, RxEvent::Offer { .. })));
        assert!(
            !rx.noise_timeout(std::time::Duration::from_secs(3600)),
            "a parsed frame must clear the noise tracker"
        );
        assert!(rx.take_noise_text().is_empty());
    }

    #[test]
    fn noise_not_tracked_in_other_states() {
        // Text during a transfer (in_zdata) must not be buffered as noise.
        let mut rx = ZmodemReceiver::new();
        let _ = rx.feed(&hex_header(frame_type::ZRQINIT));
        let _ = rx.feed(&zfile_frame("t.bin", 3));
        let save_path = std::env::temp_dir().join("zmodem_rx_test_noise_states.bin");
        let (_, file) = rx.accept_offer(&Some(save_path.to_string_lossy().to_string()));
        assert!(file.is_some());
        let _ = rx.feed(b"random mid-transfer text??");
        assert!(
            !rx.noise_timeout(std::time::Duration::ZERO),
            "noise tracker only applies to WaitingZfile"
        );
        let _ = std::fs::remove_file(save_path);
    }

    #[test]
    fn accept_offer_creates_parent_dirs_for_sz_r_subpath() {
        // `sz -r dir` offers arrive as "dir/sub/file.txt" — the receiver must
        // create the missing parent hierarchy, not fail the open.
        let mut rx = ZmodemReceiver::new();
        let _ = rx.feed(&hex_header(frame_type::ZRQINIT));
        let _ = rx.feed(&zfile_frame("some/dir/tree.txt", 3));
        let base = std::env::temp_dir().join("zmodem_rx_test_subpath");
        let _ = std::fs::remove_dir_all(&base);
        let target = base.join("some").join("dir").join("tree.txt");
        let (actions, file) = rx.accept_offer(&Some(target.to_string_lossy().to_string()));
        assert!(file.is_some(), "nested offer must open after mkdir -p");
        assert!(
            !actions
                .events
                .iter()
                .any(|e| matches!(e, RxEvent::Error(_))),
            "got {:?}",
            actions.events
        );
        assert!(target.exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
