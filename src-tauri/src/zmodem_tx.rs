//! Native Rust ZMODEM **sender** — the upload counterpart to `zmodem_rx`.
//!
//! When the remote runs `rz`, we drive a sender state machine that reads a
//! local file and emits ZFILE/ZDATA/ZEOF/ZFIN frames. Bytes the remote `rz`
//! produces (ZRINIT/ZRPOS/ZACK/ZSKIP/ZFIN) are fed in via `feed()`.
//!
//! Design mirrors `zmodem_rx.rs`: a pure synchronous state machine with no
//! async/Tauri coupling. The caller (MCP tool / CLI) owns the SSH channel and
//! the `fs::File`, shuttling bytes between them and this machine:
//!
//! ```text
//!   remote rz bytes ──► feed()  ──► TxActions.send ──► SSH channel
//!   poll(&mut file)   ──► TxActions.send ──► SSH channel
//! ```
//!
//! Protocol choices for maximum lrzsz compatibility:
//!   - Headers sent in HEX form (every receiver accepts hex).
//!   - CRC16 subpackets only (we may send CRC16 even if the receiver advertised
//!     CANFC32 — receivers always accept CRC16). Avoids implementing CRC32.
//!   - ZDLE escaping uses the full ESCCTL set (escape all bytes < 0x20, 0x7f,
//!     and ZDLE itself). lrzsz `rz` decodes this correctly regardless of its
//!     own escape advertising.
//!   - Data streamed with ZCRCG (fire-and-forget) subpackets terminated by a
//!     single ZCRCE (end-of-frame, no ack) before ZEOF. SSH is a reliable
//!     transport so we never need ZRPOS-driven retransmit for correctness —
//!     but we honour a ZRPOS the receiver sends (seek + resend from offset).

use std::io::{Read, Seek, SeekFrom};

use crate::zmodem_rx::{
    crc16_xmodem, frame_type, hex_char, hex_val, offset_bytes, subpkt_end, ZBIN, ZDLE, ZHEX, XOFF,
    XON,
};

/// ZPAD byte (also reused from rx constants conceptually; redefined locally to
/// keep the const block readable — value is b'*').
const ZPAD: u8 = b'*';

/// CAN — the ZMODEM abort byte. It shares the value 0x18 with ZDLE, but in an
/// abort context (≥5 consecutive bytes) it is unambiguously a cancel signal,
/// never part of a legal frame.
const CAN: u8 = 0x18;

/// Subpacket data size. Temporarily set to 1024 (lrzsz default MAXBLOCK) to
/// isolate whether the upload failure is caused by subpacket format/size or
/// by flow control. If rz accepts 1 KiB ZCRCW subpackets, we know the format
/// is correct and can tune size/frame-end back up for throughput.
const SUBPACKET_DATA: usize = 1024;

/// State of the ZMODEM sender state machine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TxState {
    /// Waiting for the remote `rz` to send ZRINIT.
    WaitingZrinit,
    /// ZSINIT sent; waiting for ZACK before sending ZFILE.
    WaitingZack,
    /// ZFILE sent; waiting for ZRPOS (accept at offset) or ZSKIP.
    WaitingZrpos,
    /// ZDATA header sent (or about to be); streaming file data subpackets.
    SendingData,
    /// ZEOF sent; waiting for ZRINIT (next file — none in our single-file case)
    /// or ZFIN.
    WaitingZrinit2,
    /// ZFIN sent; waiting for the receiver's ZFIN, then we emit "OO".
    WaitingZfin,
    /// Session complete.
    Done,
}

/// Actions the caller must perform after `feed()` / `poll()`.
#[derive(Default)]
pub struct TxActions {
    /// Bytes to write to the SSH channel (protocol frames + file data).
    pub send: Vec<u8>,
    /// Frontend/caller-facing events.
    pub events: Vec<TxEvent>,
}

/// Events emitted by the sender.
#[derive(Clone)]
pub enum TxEvent {
    /// The session has started (ZRINIT received, ZFILE being sent).
    Started,
    /// Upload progress: bytes sent so far, total file size.
    Progress { sent: u64, total: u64 },
    /// The file has been fully sent (ZEOF acknowledged).
    FileComplete { name: String, bytes: u64 },
    /// The entire ZMODEM session ended cleanly.
    SessionEnd,
    /// Fatal error — caller should abort.
    Error(String),
}

/// A header parsed from the remote receiver.
struct ParsedHeader {
    typenum: u8,
    data: [u8; 4],
}

pub struct ZmodemSender {
    state: TxState,
    /// Inbound buffer: raw bytes from the remote `rz` awaiting parse.
    buf: Vec<u8>,
    /// Read cursor into `buf`.
    scan_pos: usize,
    /// File metadata.
    name: String,
    size: u64,
    mtime: u64,
    /// Current send offset within the file (advances as we stream, rewinds on
    /// ZRPOS-driven retransmit).
    file_offset: u64,
    /// Total bytes acknowledged-sent (for progress). Advances with each subpacket.
    bytes_sent: u64,
    /// Receiver's advertised buffer size (ZBUFLEN from ZRINIT, bytes 2-3). Kept
    /// for diagnostics; `subpacket_size()` ignores it because over a reliable
    /// SSH transport we can safely stream full-size subpackets regardless.
    #[allow(dead_code)]
    recv_window: usize,
    /// Emitted `Started` exactly once.
    started_emitted: bool,
    /// Emitted `FileComplete` exactly once per file.
    file_complete_emitted: bool,
    /// Whether we have already emitted the ZDATA header for the current
    /// SendingData phase (so poll() knows to send it before data subpackets).
    zdata_header_sent: bool,
    /// Set when the file has been fully read and the final ZCRCE + ZEOF sent.
    data_stream_done: bool,
    /// Whether ZSINIT has already been sent in this ZMODEM session. ZSINIT is
    /// a per-session negotiation (ESCCTL caps), NOT per-file — resending it
    /// before the 2nd ZFILE makes lrzsz rz abort and return to the shell.
    /// Multi-file swaps set this to true on the fresh sender so it skips
    /// ZSINIT and sends ZFILE directly.
    zsinit_sent: bool,
}

impl ZmodemSender {
    pub fn new(name: String, size: u64, mtime: u64) -> Self {
        Self {
            state: TxState::WaitingZrinit,
            buf: Vec::with_capacity(512),
            scan_pos: 0,
            name,
            size,
            mtime,
            file_offset: 0,
            bytes_sent: 0,
            recv_window: SUBPACKET_DATA,
            started_emitted: false,
            file_complete_emitted: false,
            zdata_header_sent: false,
            data_stream_done: false,
            zsinit_sent: false,
        }
    }

    /// Create a sender for a **subsequent file** in an already-established
    /// ZMODEM session (multi-file upload). ZSINIT was already negotiated for
    /// the session by the first file's sender, so this one skips ZSINIT and
    /// sends ZFILE directly when the receiver signals readiness. Resending
    /// ZSINIT makes lrzsz rz abort to the shell.
    pub fn new_for_next_file(name: String, size: u64, mtime: u64) -> Self {
        let mut s = Self::new(name, size, mtime);
        s.zsinit_sent = true;
        s
    }

    pub fn is_done(&self) -> bool {
        self.state == TxState::Done
    }

    /// Debug helper: current state name for logging.
    pub fn state_name(&self) -> &'static str {
        match self.state {
            TxState::WaitingZrinit => "WaitingZrinit",
            TxState::WaitingZack => "WaitingZack",
            TxState::WaitingZrpos => "WaitingZrpos",
            TxState::SendingData => "SendingData",
            TxState::WaitingZrinit2 => "WaitingZrinit2",
            TxState::WaitingZfin => "WaitingZfin",
            TxState::Done => "Done",
        }
    }

    /// True when the current file's ZEOF has been sent and we're waiting for
    /// the receiver's ZRINIT (next file) or ZFIN. The caller uses this to
    /// decide whether to start the next queued file.
    pub fn is_waiting_for_next_file(&self) -> bool {
        self.state == TxState::WaitingZrinit2
    }

    /// Force the sender to emit ZFIN+OO (and SessionEnd) immediately,
    /// regardless of its current state. Used by the reader-side 3-second
    /// timeout when rz fails to send a reply (PTY in error state).
    /// Returns the bytes that must be sent to rz.
    pub fn force_finish_for_timeout(&mut self) -> Vec<u8> {
        log::warn!("[zmodem_tx] force_finish_for_timeout: state={}", self.state_name());
        let mut out = Vec::new();
        // Build ZFIN hex header + OO terminator, the same bytes the
        // handle_header WaitingZfin branch emits.
        out.extend_from_slice(&self.build_hex_header(frame_type::ZFIN, [0, 0, 0, 0]));
        out.extend_from_slice(b"OO");
        self.state = TxState::Done;
        out
    }

    /// Feed bytes received from the remote `rz`. Returns actions (bytes to send
    /// back + events).
    pub fn feed(&mut self, data: &[u8]) -> TxActions {
        let mut actions = TxActions::default();
        self.buf.extend_from_slice(data);

        // Repeatedly try to advance the state machine while we can parse frames.
        loop {
            let progressed = self.try_step(&mut actions);
            if !progressed {
                break;
            }
        }

        // Compact the buffer periodically.
        if self.scan_pos > 0 && self.scan_pos >= self.buf.len() {
            self.buf.clear();
            self.scan_pos = 0;
        } else if self.scan_pos > 2048 {
            self.buf.drain(..self.scan_pos);
            self.scan_pos = 0;
        }

        actions
    }

    /// Drive file-data streaming in the SendingData state. Reads up to one
    /// subpacket from `file` and emits the encoded frame. Returns actions; the
    /// caller should call this repeatedly until `is_done()` or an Error event.
    pub fn poll<R: Read + Seek>(&mut self, file: &mut R) -> TxActions {
        let mut actions = TxActions::default();

        if self.state != TxState::SendingData {
            return actions;
        }

        // If a previous feed() detected end-of-file, the ZEOF + final ZCRCE are
        // already emitted; nothing to poll.
        if self.data_stream_done {
            return actions;
        }

        // Emit the ZDATA header once per SendingData phase.
        if !self.zdata_header_sent {
            actions.send = self.build_bin16_header(frame_type::ZDATA, offset_bytes(self.file_offset));
            self.zdata_header_sent = true;
        }

        // Seek to the current offset (cheap no-op if already there) and read
        // one subpacket. On EOF, emit the closing ZCRCE empty subpacket + ZEOF.
        let chunk_size = self.subpacket_size();
        if let Err(e) = file.seek(SeekFrom::Start(self.file_offset)) {
            actions.events.push(TxEvent::Error(format!("seek 失败: {}", e)));
            self.state = TxState::Done;
            return actions;
        }

        let mut chunk = vec![0u8; chunk_size];
        let n = match file.read(&mut chunk) {
            Ok(n) => n,
            Err(e) => {
                actions.events.push(TxEvent::Error(format!("读文件失败: {}", e)));
                self.state = TxState::Done;
                return actions;
            }
        };

        if n == 0 {
            // File fully streamed — close the ZDATA stream with an empty ZCRCE
            // subpacket, then send ZEOF (offset = total size). The receiver
            // responds with ZRINIT (single-file: we treat as prelude to ZFIN)
            // or ZFIN.
            actions.send.extend_from_slice(&self.build_data_subpacket(&[], subpkt_end::ZCRCE));
            actions.send.extend_from_slice(&self.build_bin16_header(frame_type::ZEOF, offset_bytes(self.size)));
            self.data_stream_done = true;
            self.state = TxState::WaitingZrinit2;
            if !self.file_complete_emitted {
                self.file_complete_emitted = true;
                actions.events.push(TxEvent::Progress { sent: self.size, total: self.size });
                actions.events.push(TxEvent::FileComplete { name: self.name.clone(), bytes: self.size });
            }
            return actions;
        }

        // Stream the chunk as a ZCRCG (fire-and-forget) subpacket.
        let chunk = &chunk[..n];
        actions
            .send
            .extend_from_slice(&self.build_data_subpacket(chunk, subpkt_end::ZCRCG));

        self.file_offset += n as u64;
        self.bytes_sent += n as u64;
        actions.events.push(TxEvent::Progress {
            sent: self.bytes_sent.min(self.size),
            total: self.size,
        });

        actions
    }

    /// Try to advance the state machine by parsing one inbound frame. Returns
    /// true if progress was made.
    fn try_step(&mut self, actions: &mut TxActions) -> bool {
        if self.state == TxState::Done {
            return false;
        }

        // Detect the lrzsz abort sequence: 5+ consecutive CAN (0x18) bytes.
        if self.detect_abort() {
            log::warn!("[zmodem_tx] abort detected (≥5 CAN in buffer)");
            actions
                .events
                .push(TxEvent::Error("接收方中止了传输（CAN）".into()));
            self.state = TxState::Done;
            return true;
        }

        // Skip leading XON/XOFF noise.
        while self.scan_pos < self.buf.len()
            && matches!(self.buf[self.scan_pos], XON | XOFF | 0x91 | 0x93)
        {
            self.scan_pos += 1;
        }
        if self.scan_pos >= self.buf.len() {
            return false;
        }

        // Look for a header starting with ZPAD [ZPAD] ZDLE.
        let remaining = &self.buf[self.scan_pos..];
        let rel = match find_header_start(remaining) {
            Some(r) => r,
            None => return false,
        };
        let abs = self.scan_pos + rel;

        match parse_header(&self.buf, abs) {
            None => false, // incomplete — wait for more bytes
            Some((header, consumed)) => {
                self.scan_pos = abs + consumed;
                self.handle_header(header, actions)
            }
        }
    }

    /// Apply a parsed receiver header to the current state. Returns true if the
    /// state advanced.
    fn handle_header(&mut self, header: ParsedHeader, actions: &mut TxActions) -> bool {
        match self.state {
            TxState::WaitingZrinit | TxState::WaitingZrinit2 => {
                log::info!("[zmodem_tx] feed handle_header: state={:?}, typenum={}", self.state, header.typenum);
                if header.typenum == frame_type::ZRINIT {
                    // First ZRINIT → send ZSINIT (request escape-ctrl-chars) then ZFILE.
                    // Subsequent ZRINIT (after our ZEOF) → no more files, send ZFIN.
                    self.record_zrinit_caps(header.data);
                    if self.state == TxState::WaitingZrinit {
                        if !self.started_emitted {
                            self.started_emitted = true;
                            actions.events.push(TxEvent::Started);
                        }
                        if self.zsinit_sent {
                            // Multi-file: ZSINIT already negotiated for this
                            // session by the first file's sender. Skip it and
                            // send ZFILE directly — resending ZSINIT makes rz
                            // abort to the shell.
                            actions.send = self.build_zfile_frame();
                            self.state = TxState::WaitingZrpos;
                        } else {
                            // Send ZSINIT (hex) to negotiate ESCCTL — without it,
                            // rz uses its default escape rules and our binary16
                            // ZFILE fails CRC, yielding a ZNAK. The ZDLE escaping
                            // fix (0x80-0x9F range) makes our ZSINIT frame pass rz's
                            // validation, so the ZACK → ZFILE flow now completes.
                            // Format mirrors zmodem.js: hex header, ESCCTL=0x40 in
                            // data[3], attention-string subpacket payload [0].
                            let mut zsinit = self.build_hex_header(frame_type::ZSINIT, [0, 0, 0, 0x40]);
                            zsinit.extend_from_slice(&self.build_data_subpacket(&[0], subpkt_end::ZCRCW));
                            actions.send = zsinit;
                            self.state = TxState::WaitingZack;
                            self.zsinit_sent = true;
                        }
                    } else {
                        // After ZEOF the receiver's ZRINIT means "ready for next
                        // file". We have none → close the session.
                        actions.send = self.build_hex_header(frame_type::ZFIN, [0, 0, 0, 0]);
                        self.state = TxState::WaitingZfin;
                    }
                    true
                } else if header.typenum == frame_type::ZRPOS {
                    // ZRPOS in WaitingZrinit2 (post-ZEOF): lrzsz rz sends this
                    // to signal "ready for next file". The multi-file swap in
                    // ssh.rs's handle_incoming_data fires when queue is
                    // non-empty, replacing us with a fresh WaitingZrinit sender.
                    // If the queue is empty, the reader-side timeout sends ZFIN.
                    // Either way we just stay put here.
                    //
                    // ZRPOS in WaitingZrinit (fresh sender for a new file in a
                    // multi-file batch): lrzsz rz sends ZRPOS to signal readiness
                    // for the next file. Since ZSINIT was already negotiated for
                    // the session, send ZFILE directly (same as the ZRINIT path).
                    if self.state == TxState::WaitingZrinit {
                        log::info!("[zmodem_tx] ZRPOS in WaitingZrinit — sending ZFILE (multi-file next file)");
                        self.record_zrinit_caps(header.data);
                        if !self.started_emitted {
                            self.started_emitted = true;
                            actions.events.push(TxEvent::Started);
                        }
                        actions.send = self.build_zfile_frame();
                        self.state = TxState::WaitingZrpos;
                    } else {
                        log::info!("[zmodem_tx] ZRPOS off=0 in WaitingZrinit2 — letting reader handle (multi-file or timeout)");
                    }
                    true
                } else if header.typenum == frame_type::ZRINIT {
                    // Same reasoning as ZRPOS above — let the reader's multi-
                    // file swap drive the next file (or end the session).
                    log::info!("[zmodem_tx] ZRINIT in WaitingZrinit2 — letting reader handle");
                    true
                } else if header.typenum == frame_type::ZFIN {
                    // Receiver may send ZFIN directly after ZEOF.
                    actions.send = self.build_hex_header(frame_type::ZFIN, [0, 0, 0, 0]);
                    actions.send.extend_from_slice(b"OO");
                    actions.events.push(TxEvent::SessionEnd);
                    self.state = TxState::Done;
                    true
                } else {
                    // ZSINIT etc. — ignore, stay put.
                    true
                }
            }
            TxState::WaitingZack => {
                match header.typenum {
                    frame_type::ZACK => {
                        // Receiver acknowledged ZSINIT — now send ZFILE.
                        actions.send = self.build_zfile_frame();
                        self.state = TxState::WaitingZrpos;
                        true
                    }
                    frame_type::ZRINIT => {
                        // rz might re-send ZRINIT before processing our ZSINIT.
                        // Ignore it; we're waiting for ZACK.
                        true
                    }
                    _ => true,
                }
            }
            TxState::WaitingZrpos => {
                match header.typenum {
                    frame_type::ZRPOS => {
                        // Receiver wants us to start sending data at this offset.
                        let off = u32::from_le_bytes([
                            header.data[0],
                            header.data[1],
                            header.data[2],
                            header.data[3],
                        ]) as u64;
                        self.file_offset = off;
                        self.bytes_sent = off;
                        self.zdata_header_sent = false;
                        self.data_stream_done = false;
                        self.state = TxState::SendingData;
                        true
                    }
                    frame_type::ZSKIP => {
                        // Receiver declined the file.
                        actions
                            .events
                            .push(TxEvent::Error("接收方跳过了该文件（ZSKIP）".into()));
                        self.state = TxState::Done;
                        true
                    }
                    frame_type::ZFERR => {
                        actions
                            .events
                            .push(TxEvent::Error("接收方报告文件错误（ZFERR）".into()));
                        self.state = TxState::Done;
                        true
                    }
                    frame_type::ZFIN => {
                        actions.send = self.build_hex_header(frame_type::ZFIN, [0, 0, 0, 0]);
                        actions.send.extend_from_slice(b"OO");
                        actions.events.push(TxEvent::SessionEnd);
                        self.state = TxState::Done;
                        true
                    }
                    _ => true,
                }
            }
            TxState::SendingData => {
                // Mid-stream the receiver may send ZRPOS (retransmit from offset),
                // ZACK (acknowledgement — ignored, we firehose), ZFIN/ZFERR (abort).
                match header.typenum {
                    frame_type::ZRPOS => {
                        let off = u32::from_le_bytes([
                            header.data[0],
                            header.data[1],
                            header.data[2],
                            header.data[3],
                        ]) as u64;
                        self.file_offset = off;
                        self.bytes_sent = off;
                        self.zdata_header_sent = false;
                        self.data_stream_done = false;
                        true
                    }
                    frame_type::ZACK => true,
                    frame_type::ZFERR => {
                        actions
                            .events
                            .push(TxEvent::Error("接收方报告文件错误（ZFERR）".into()));
                        self.state = TxState::Done;
                        true
                    }
                    frame_type::ZFIN => {
                        actions.send = self.build_hex_header(frame_type::ZFIN, [0, 0, 0, 0]);
                        actions.send.extend_from_slice(b"OO");
                        actions.events.push(TxEvent::SessionEnd);
                        self.state = TxState::Done;
                        true
                    }
                    _ => true,
                }
            }
            TxState::WaitingZfin => {
                if header.typenum == frame_type::ZFIN {
                    actions.send.extend_from_slice(b"OO");
                    actions.events.push(TxEvent::SessionEnd);
                    self.state = TxState::Done;
                }
                true
            }
            TxState::Done => false,
        }
    }

    /// Detect ≥5 consecutive CAN (0x18) bytes in the buffer — the ZMODEM
    /// abort signal. Scans the unprocessed tail.
    fn detect_abort(&self) -> bool {
        let mut run = 0u32;
        for &b in &self.buf[self.scan_pos..] {
            if b == CAN {
                run += 1;
                if run >= 5 {
                    return true;
                }
            } else if matches!(b, XON | XOFF | 0x91 | 0x93) {
                // noise — don't reset the run
            } else {
                run = 0;
            }
        }
        false
    }

    /// Extract receiver capabilities from a ZRINIT's data bytes.
    ///
    /// Layout matches zmodem.js's ZRINIT header (which interoperates with
    /// lrzsz): buffer size is data[0..2] big-endian (0 = unlimited), and the
    /// capability flags (CANFDX/CANOVIO/CANFC32/ESCCTL/...) are in data[3].
    fn record_zrinit_caps(&mut self, data: [u8; 4]) {
        let bufsz = ((data[0] as u16) << 8) | (data[1] as u16);
        if bufsz > 0 {
            self.recv_window = (bufsz as usize).max(1024);
        }
        // bufsz == 0 means "unlimited" — keep recv_window at its default
        // (SUBPACKET_DATA) so we stream at full size.
    }

    fn subpacket_size(&self) -> usize {
        // Don't clamp below SUBPACKET_DATA on a reliable SSH transport: even a
        // receiver that advertised a small buffer is reading from a PTY/pipe
        // that never loses data, and lrzsz rz mallocs its decode buffer to
        // match the incoming subpacket size. Large subpackets are safe and
        // necessary for throughput.
        SUBPACKET_DATA
    }

    // ── Frame builders ────────────────────────────────────────────────────

    /// Build a hex-format header: ZPAD ZPAD ZDLE ZHEX <type><data><crc> CR LF XON.
    fn build_hex_header(&self, typenum: u8, data: [u8; 4]) -> Vec<u8> {
        let crc_body = [typenum, data[0], data[1], data[2], data[3]];
        let crc = crc16_xmodem(&crc_body);
        let crc_hi = (crc >> 8) as u8;
        let crc_lo = (crc & 0xff) as u8;

        let mut out = Vec::with_capacity(21);
        out.extend_from_slice(&[ZPAD, ZPAD, ZDLE, ZHEX]);
        out.push(hex_char(typenum >> 4));
        out.push(hex_char(typenum & 0x0f));
        for &d in &data {
            out.push(hex_char(d >> 4));
            out.push(hex_char(d & 0x0f));
        }
        out.push(hex_char(crc_hi >> 4));
        out.push(hex_char(crc_hi & 0x0f));
        out.push(hex_char(crc_lo >> 4));
        out.push(hex_char(crc_lo & 0x0f));
        out.push(0x0d);
        out.push(0x0a);
        // ZFILE/ZDATA producers send XON after control headers; lrzsz tolerates
        // it. We omit XON on ZFIN (matches rx convention) by special-casing.
        if typenum != frame_type::ZFIN {
            out.push(XON);
        }
        out
    }

    /// Build a binary16 header: ZPAD ZDLE ZBIN <ZDLE-encoded type+data+crc>.
    /// lrzsz's `rz` expects ZFILE/ZDATA/ZEOF in binary format (not hex) —
    /// confirmed by zmodem.js's SENDER_BINARY_HEADER table. Using hex for
    /// these frames causes rz to silently ignore them and re-send ZRINIT.
    fn build_bin16_header(&self, typenum: u8, data: [u8; 4]) -> Vec<u8> {
        let crc_body = [typenum, data[0], data[1], data[2], data[3]];
        let crc = crc16_xmodem(&crc_body);
        let crc_bytes = [(crc >> 8) as u8, (crc & 0xff) as u8];

        // The payload to ZDLE-encode: type + 4 data bytes + 2 CRC bytes.
        let mut body = Vec::with_capacity(7);
        body.push(typenum);
        body.extend_from_slice(&data);
        body.extend_from_slice(&crc_bytes);

        let mut out = Vec::with_capacity(20);
        out.push(ZPAD);
        out.push(ZDLE);
        out.push(ZBIN);
        zdle_append(&body, &mut out);
        out
    }

    /// Build the ZFILE header + its ZCRCW-terminated metadata subpacket.
    /// Metadata layout: `<name>\0<size> <mtime> <mode> <serial> <filesrem> <bytesrem>`.
    fn build_zfile_frame(&self) -> Vec<u8> {
        // Format matches lrzsz expectations (and zmodem.js's
        // _convert_params_to_offer_payload_array):
        //   - size:   decimal
        //   - mtime:  OCTAL (lrzsz parses st_mtime via strtoul base 8)
        //   - mode:   octal, with 0x8000 (S_IFREG) OR'd in — lrzsz rejects
        //             modes without a file-type bit, treating the ZFILE as
        //             invalid and silently falling back to XMODEM ("C").
        //             0x8000 | 0o100644 = 0o1000644.
        let mode = 0o100000 | 0o644; // S_IFREG | rw-r--r--
        let mut meta = Vec::new();
        meta.extend_from_slice(self.name.as_bytes());
        meta.push(0); // NUL separator
        // Match zmodem.js's _convert_params_to_offer_payload_array exactly for a
        // single-file offer: size (decimal), mtime (octal), mode (octal with the
        // 0x8000 regular-file bit), serial=0, files_remaining=1,
        // bytes_remaining=size. A trailing files_remaining=0 (what we sent
        // before) made lrzsz treat the offer as empty and ignore it.
        meta.extend_from_slice(
            format!("{} {:o} {:o} 0 1 {}", self.size, self.mtime, mode, self.size).as_bytes(),
        );

        // ZFILE uses binary16 header (matches zmodem.js's SENDER_BINARY_HEADER
        // table). Works once rz has acknowledged our ZSINIT (ESCCTL negotiation).
        let mut out = self.build_bin16_header(frame_type::ZFILE, [0, 0, 0, 0]);
        out.extend_from_slice(&self.build_data_subpacket(&meta, subpkt_end::ZCRCW));
        out
    }

    /// Build a data subpacket: ZDLE-encoded `payload` + ZDLE + `end_marker` +
    /// ZDLE-encoded CRC16 (big-endian). An empty payload is valid (used for the
    /// closing ZCRCE frame).
    ///
    /// IMPORTANT: the CRC covers `payload + end_marker` (NOT just payload).
    /// This matches zmodem.js's _encode (zsubpacket.js:137) and lrzsz's
    /// expectation. Computing CRC over payload alone makes every subpacket fail
    /// rz's validation (ZNAK).
    fn build_data_subpacket(&self, payload: &[u8], end_marker: u8) -> Vec<u8> {
        let mut crc_input = Vec::with_capacity(payload.len() + 1);
        crc_input.extend_from_slice(payload);
        crc_input.push(end_marker);
        let crc = crc16_xmodem(&crc_input);
        let crc_bytes = [(crc >> 8) as u8, (crc & 0xff) as u8];

        let mut out = Vec::with_capacity(payload.len() + 16);
        zdle_append(payload, &mut out);
        out.push(ZDLE);
        out.push(end_marker);
        zdle_append(&crc_bytes, &mut out);
        out
    }
}

// ── Parsing helpers (operate on an external buffer) ──────────────────────

/// Find the offset of a header start (ZDLE preceded by ≥1 ZPAD) in `remaining`.
fn find_header_start(remaining: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < remaining.len() {
        if remaining[i] == ZDLE && i + 1 < remaining.len() {
            let next = remaining[i + 1];
            if next == ZBIN || next == ZHEX {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Parse a header at `abs_pos` (pointing at ZDLE). Returns the typenum, the 4
/// data bytes, and the number of raw bytes consumed.
fn parse_header(buf: &[u8], abs_pos: usize) -> Option<(ParsedHeader, usize)> {
    if abs_pos >= buf.len() || buf[abs_pos] != ZDLE {
        return None;
    }
    if abs_pos + 1 >= buf.len() {
        return None;
    }
    match buf[abs_pos + 1] {
        ZHEX => parse_hex_header(buf, abs_pos),
        ZBIN => parse_bin16_header(buf, abs_pos),
        _ => None, // we don't send ZBIN32, and receivers rarely use it for control
    }
}

/// Hex header: ZDLE ZHEX + 14 hex chars (type + 4 data + 2 crc) + CR LF [XON].
fn parse_hex_header(buf: &[u8], pos: usize) -> Option<(ParsedHeader, usize)> {
    let hex_start = pos + 2;
    let min_len = hex_start + 14 + 2; // 14 hex + CR LF
    if buf.len() < min_len {
        return None;
    }
    let hex_bytes = &buf[hex_start..hex_start + 14];
    let mut decoded = [0u8; 7];
    for i in 0..7 {
        let hi = hex_val(hex_bytes[i * 2])?;
        let lo = hex_val(hex_bytes[i * 2 + 1])?;
        decoded[i] = (hi << 4) | lo;
    }
    let typenum = decoded[0];
    let data = [decoded[1], decoded[2], decoded[3], decoded[4]];
    let mut consumed = (hex_start + 14) - pos;
    let after_hex = hex_start + 14;
    if after_hex < buf.len() && matches!(buf[after_hex], 0x0d | 0x8d) {
        consumed += 1;
        if after_hex + 1 < buf.len() && matches!(buf[after_hex + 1], 0x0a | 0x8a) {
            consumed += 1;
            if after_hex + 2 < buf.len() && buf[after_hex + 2] == XON {
                consumed += 1;
            }
        }
    }
    Some((ParsedHeader { typenum, data }, consumed))
}

/// Binary16 header: ZDLE ZBIN + 7 ZDLE-encoded logical bytes (type + 4 data + 2 crc).
fn parse_bin16_header(buf: &[u8], pos: usize) -> Option<(ParsedHeader, usize)> {
    let start = pos + 2;
    let mut decoded = [0u8; 7];
    let mut raw_idx = start;
    for slot in decoded.iter_mut().take(7) {
        if raw_idx >= buf.len() {
            return None;
        }
        let byte = buf[raw_idx];
        if byte == ZDLE {
            if raw_idx + 1 >= buf.len() {
                return None;
            }
            *slot = buf[raw_idx + 1] ^ 0x40;
            raw_idx += 2;
        } else {
            *slot = byte;
            raw_idx += 1;
        }
    }
    let typenum = decoded[0];
    let data = [decoded[1], decoded[2], decoded[3], decoded[4]];
    Some((ParsedHeader { typenum, data }, raw_idx - pos))
}

// ── ZDLE encoding ─────────────────────────────────────────────────────────

/// Append `bytes` to `out`, ZDLE-escaping every byte that could be mangled by
/// the PTY/terminal layer: ZDLE itself, DEL (0x7f), and all C0 control bytes
/// (< 0x20). This is the ESCCTL superset — receivers that did not advertise
/// ESCCTL still decode it correctly because the inverse (XOR 0x40) is lossless.
fn zdle_append(bytes: &[u8], out: &mut Vec<u8>) {
    for &b in bytes {
        if needs_zdle_escape(b) {
            out.push(ZDLE);
            out.push(b ^ 0x40);
        } else {
            out.push(b);
        }
    }
}

#[inline]
fn needs_zdle_escape(b: u8) -> bool {
    // Must match zmodem.js's ZDLE table with escape_ctrl_chars enabled
    // (zdle.js _setup_zdle_table): escape exactly the bytes whose bits 5-6 are
    // clear — the C0 controls 0x00-0x1F AND their high-bit twins 0x80-0x9F.
    // lrzsz's `rz` validates incoming binary headers/subpackets against this
    // scheme; an unescaped high-bit control byte (e.g. a CRC byte 0x89) makes
    // it reply ZNAK and the handshake dies. ZDLE (0x18) falls inside the range.
    // DEL (0x7f) is NOT escaped: 0x7F & 0x60 = 0x60 (bits 5-6 set), so zmodem.js
    // leaves it alone. Escaping 0x7F injects spurious ZDLE bytes that corrupt
    // the subpacket frame structure and cause rz to reject every ZDATA block.
    (b & 0x60) == 0
}
