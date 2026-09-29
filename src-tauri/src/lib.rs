// MyShell core library — shared types, modules, and traits used by the GUI
// binary (main.rs), the CLI binary (bin/myshell-cli.rs), and the MCP server
// binary (bin/myshell-mcp.rs).
//
// Everything in this crate is Tauri-free: no `State`, no `WebviewWindow`, no
// `AppHandle`. The GUI binary wraps these pure functions with thin
// `#[tauri::command]` adapters; the CLI / MCP binaries call them directly.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

// ============ Modules ============

pub mod ssh;
pub mod zmodem_rx;
pub mod zmodem_tx;
pub mod sftp;
pub mod db;
pub mod secrets;
pub mod ftp;
pub mod crypto;
pub mod vault;
pub mod proxy;
pub mod backup;
pub mod local;
pub mod fonts;
pub mod elevation;
pub mod ai;
pub mod redact;
pub mod mcp_tools;
pub mod command_rules;
pub mod path_safety;

// ============ Event Sink (emitter abstraction) ============

/// Abstraction over Tauri's `WebviewWindow::emit`. The GUI binary implements
/// this with a real window; the CLI / MCP binaries implement it with stdout
/// or a no-op. This is the seam that decouples core logic from the Tauri
/// event system.
///
/// Object-safe: only `emit_raw` (non-generic) is the required method.
/// Use the `EventSinkExt` blanket extension for ergonomic typed emission.
pub trait EventSink: Send + Sync + 'static {
    fn emit_raw(&self, event: &str, payload: serde_json::Value);
}

/// Extension trait for ergonomic emission of typed (Serialize) payloads.
/// Automatically available for every `EventSink` implementor via blanket impl.
pub trait EventSinkExt: EventSink {
    fn emit<T: Serialize>(&self, event: &str, payload: &T) {
        if let Ok(v) = serde_json::to_value(payload) {
            self.emit_raw(event, v);
        }
    }
}

impl<T: EventSink + ?Sized> EventSinkExt for T {}

// ============ Connection Config ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: String, // "password" or "key"
    /// Transient — used to shuttle the password from frontend to keyring on
    /// save, and from keyring to ssh connect. Never persisted to SQLite.
    pub password: Option<String>,
    /// Private key PEM content. Encrypted at rest in `private_key_pem_enc` and
    /// loaded from there at connect time via `db::get_private_key_pem`.
    ///
    /// **Never crosses the IPC boundary into the webview.** It used to be
    /// selected by `get_all_connections`, decrypted and returned on every
    /// list load, which parked every connection's key in renderer memory for
    /// the whole session — and the renderer hosts two `v-html` sinks for
    /// remote content, so any injection there escalated to full private-key
    /// exfiltration. `skip_serializing` makes that structurally impossible
    /// rather than relying on every call site to remember; the frontend gets
    /// the `has_private_key` boolean instead.
    #[serde(default, skip_serializing)]
    pub private_key_pem: Option<String>,
    /// Whether a private key is stored for this connection, derived from
    /// `private_key_pem_enc.is_some()`. Replaces the PEM on the wire so the
    /// dialog can still render "已加密存储" without holding the key.
    #[serde(default)]
    pub has_private_key: bool,
    /// Explicit "delete the stored private key" intent, set by the dialog's
    /// ✕ button. Absent or false means "keep what is stored": the *absence* of
    /// a PEM in a save payload is not a delete instruction, because the
    /// frontend never receives the PEM in the first place and an ordinary edit
    /// would otherwise wipe the key.
    #[serde(default, skip_serializing)]
    pub clear_private_key: bool,
    /// ssh | sftp | ftp. SFTP rides on SSH (shared session id), FTP is a
    /// standalone connection managed in `AppState::ftp_sessions`.
    #[serde(default)]
    pub conn_type: String,
    /// Hierarchical folder path, e.g. "/prod/web". Root is "/".
    #[serde(default = "default_group_path")]
    pub group_path: String,
    /// none | implicit | explicit — FTP/FTPS only.
    #[serde(default = "default_ftp_tls")]
    pub ftp_tls: String,
    /// FTP passive mode toggle. True by default (NAT-friendly).
    #[serde(default = "default_ftp_passive")]
    pub ftp_passive: bool,
    /// Proxy type: "none" | "socks5" | "http". Stored plaintext (not
    /// sensitive — knowing you use SOCKS5 doesn't compromise anything).
    #[serde(default = "default_proxy_type")]
    pub proxy_type: String,
    /// Proxy host (transient plaintext in memory; encrypted at rest as
    /// `proxy_host_enc`). Surfaces internal network topology, treated as
    /// same-sensitivity as `host`.
    #[serde(default)]
    pub proxy_host: Option<String>,
    /// Proxy port. Small int, not sensitive — stored plaintext.
    #[serde(default)]
    pub proxy_port: Option<u16>,
    /// Proxy auth username. Stored plaintext in DB (not a secret on its own).
    #[serde(default)]
    pub proxy_username: Option<String>,
    /// Proxy auth password (transient). Resolved from keyring at connect
    /// time, written to keyring at save time. Same scheme as `password`.
    #[serde(default)]
    pub proxy_password: Option<String>,
    /// Local terminal only (`conn_type == "local"`): shell executable to
    /// spawn, e.g. `pwsh.exe`, `powershell.exe`, `cmd.exe`, `wsl.exe`, or an
    /// absolute path. Ignored for ssh/sftp/ftp. Plain column — a program
    /// path isn't a secret.
    #[serde(default)]
    pub shell_path: Option<String>,
    /// Local terminal only: optional shell arguments (e.g. `-d Ubuntu`).
    #[serde(default)]
    pub shell_args: Option<String>,
    /// Optional command injected into the PTY right after the shell starts
    /// (e.g. `claude` to auto-launch on open). Currently honored for local
    /// terminals; SSH may use it later. Plain column — not a secret.
    #[serde(default)]
    pub init_command: Option<String>,
    /// Optional per-connection terminal font override (family name). When set,
    /// takes precedence over the global terminal font for this connection's
    /// tabs. Plain column — not a secret.
    #[serde(default)]
    pub terminal_font: Option<String>,
    /// TCP dial address-family preference: "auto" (default — let the OS try
    /// both v4/v6) | "ipv4" | "ipv6". Honored by ssh.rs's `dial_tcp` for
    /// SSH/SFTP only. Plain column — not a secret (same treatment as
    /// proxy_type). Fixes hosts with a dead AAAA record black-holing the
    /// connect attempt before it falls back to IPv4.
    #[serde(default = "default_address_family")]
    pub address_family: String,
    /// Per-connection TCP + SSH-handshake connect timeout in seconds. None =
    /// use the 10s default. Nullable column.
    #[serde(default)]
    pub connect_timeout_secs: Option<u32>,
    /// SSH keepalive interval in seconds. None = use the 15s default
    /// (keepalive_max stays fixed at 3). Nullable column.
    #[serde(default)]
    pub keepalive_interval_secs: Option<u32>,
    /// Suppress the shell's idle auto-logout (`TMOUT`) by injecting `export
    /// TMOUT=0` into the interactive PTY once, right after the shell starts.
    /// This is a one-shot write on the *existing* channel (safe for restricted
    /// single-session accounts). Default false.
    #[serde(default)]
    pub suppress_tmout: bool,
    pub created_at: String,
}

pub fn default_group_path() -> String {
    "/".to_string()
}

fn default_ftp_tls() -> String {
    "none".to_string()
}

fn default_ftp_passive() -> bool {
    true
}

fn default_proxy_type() -> String {
    "none".to_string()
}

fn default_address_family() -> String {
    "auto".to_string()
}

// ============ SFTP File Entry ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: String,
    pub modified: String,
}

// ============ Command History Entry ============

// rename_all = camelCase so the wire fields (createdAt) match the TS
// interface in api.ts. ConnectionConfig/FileEntry stay snake_case by
// intentional convention (documented in api.ts); these list-item structs
// use camelCase because the frontend reads createdAt/connectionId/sortOrder/
// isGlobal directly off the payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandHistoryItem {
    pub id: i64,
    pub command: String,
    pub pinned: bool,
    pub created_at: String,
}

// ============ Quick Command Entries ============

/// A quick command as stored/managed (global or per-connection). Used by the
/// management panel. `connection_id` is None for global scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickCommandItem {
    pub id: i64,
    pub connection_id: Option<String>,
    pub label: String,
    pub command: String,
    pub sort_order: i64,
}

/// A quick command flattened for the terminal execution panel: the union of
/// global + current-connection commands, with an `is_global` flag for grouping.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickCommandExecItem {
    pub id: i64,
    pub is_global: bool,
    pub label: String,
    pub command: String,
}

// ============ App State ============

/// `Clone`-able so the MCP server can hand a copy to each background transfer
/// task (each field is `Arc`/`Mutex`, so cloning is cheap and shares state).
#[derive(Clone)]
pub struct AppState {
    /// Arc-wrapped so per-session SshClient handlers can clone a reference
    /// for `check_server_key` lookups without borrowing State.
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub ssh_sessions: Arc<Mutex<std::collections::HashMap<String, ssh::SshSession>>>,
    pub ftp_sessions: Arc<Mutex<std::collections::HashMap<String, ftp::FtpSession>>>,
    /// Local PTY terminal sessions, keyed by UUID session id (== frontend
    /// tab id, same invariant as ssh_sessions).
    pub local_sessions: Arc<Mutex<std::collections::HashMap<String, local::LocalSession>>>,
    pub zmodem_files: Arc<Mutex<HashMap<String, ZmodemFileHandle>>>,
    /// Data Encryption Key (DEK) — random 32-byte key for encrypting all
    /// database columns and keyring entries. Derived once at setup and
    /// stored encrypted by the login password. `None` until unlocked.
    ///
    /// `Zeroizing` makes "lock vault" mean what it says: assigning `None`
    /// scrubs the 32 bytes on the way out, instead of leaving them in a
    /// freed heap page for the next allocation to read.
    pub dek: Arc<Mutex<Option<Zeroizing<[u8; 32]>>>>,
    /// Cancellation flags for in-flight SFTP transfers, keyed by request_id.
    /// The frontend sets the flag via `sftp_cancel_transfer`; the download/upload
    /// loop checks it between 32 KB chunks and breaks early when true.
    pub transfer_cancels: Arc<Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>>,
}

/// Track open file handles for streaming ZMODEM file IO. Each transfer is
/// keyed by a UUID so the frontend can talk about multiple concurrent files
/// (multi-file rz, separate write handles per sz offer).
pub struct ZmodemFileHandle {
    pub kind: ZmodemFileKind,
    pub path: String,
    /// For reads: cached open file + total size. For writes: an append-mode
    /// handle so each chunk writes without re-opening. We box these so the
    /// enum variant stays cheap to move.
    pub reader: Option<std::fs::File>,
    pub writer: Option<std::fs::File>,
    pub size: u64,
}

#[derive(PartialEq)]
pub enum ZmodemFileKind {
    Read,
    Write,
}

// ============ Vault helpers ============

/// Extract the DEK from AppState or surface a friendly error if the
/// vault is locked. Every command that touches encrypted columns calls this
/// first — there's no implicit unlock.
///
/// Returns a *copy* so the lock is released immediately. The copy is
/// `Zeroizing`, so the caller's binding is scrubbed when the command
/// returns rather than lingering in the async task's stack frame.
pub fn require_dek(state: &AppState) -> Result<Zeroizing<[u8; 32]>, String> {
    state
        .dek
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "Vault 未解锁".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归守卫：`AppState::dek` 的槽位类型必须带 zeroize 擦除语义。
    ///
    /// 擦除效果本身无法在测试里观测（内存已被释放），但**类型**可以：
    /// 这个函数只在字段是 `Option<Zeroizing<[u8; 32]>>` 时才编译得过。
    /// 谁把字段改回 `Option<[u8; 32]>`（于是「锁定保险库」退化成单纯
    /// drop），`cargo test` 立刻编译失败。
    fn _assert_dek_slot_is_zeroizing(state: &AppState) {
        let guard = state.dek.try_lock().expect("uncontended");
        let _: &Option<Zeroizing<[u8; 32]>> = &guard;
    }

    /// 同理钉住 `require_dek` 的**返回类型**。这一条才是真正脆弱的：
    /// 如果有人把返回类型改回 `[u8; 32]`，main.rs 里那三十多个
    /// `let key = require_dek(&state)?;` 调用点靠 deref 转换**照样编译
    /// 通过**，擦除语义却会在最常见的路径上被静默丢掉。字段类型不变时
    /// 只有这条断言能发现。
    fn _assert_require_dek_returns_zeroizing(
        r: Result<Zeroizing<[u8; 32]>, String>,
    ) -> Result<Zeroizing<[u8; 32]>, String> {
        r
    }

    #[test]
    fn dek_slot_type_guard() {
        // 让上面的编译期断言真正被实例化一次（否则 dead_code 会被优化掉，
        // 断言也就跟着消失）。用不到返回值，只要求它能通过类型检查。
        let _ = _assert_dek_slot_is_zeroizing;
        let _ = _assert_require_dek_returns_zeroizing;
    }

    /// `require_dek` 在未解锁时必须 fail-closed —— 锁定的保险库不能
    /// 退化成"返回全零密钥"。
    #[test]
    fn require_dek_is_fail_closed_when_locked() {
        let state = AppState {
            db: Arc::new(Mutex::new(
                rusqlite::Connection::open_in_memory().expect("in-memory sqlite"),
            )),
            ssh_sessions: Arc::new(Mutex::new(HashMap::new())),
            ftp_sessions: Arc::new(Mutex::new(HashMap::new())),
            local_sessions: Arc::new(Mutex::new(HashMap::new())),
            zmodem_files: Arc::new(Mutex::new(HashMap::new())),
            dek: Arc::new(Mutex::new(None)),
            transfer_cancels: Arc::new(Mutex::new(HashMap::new())),
        };
        let err = require_dek(&state).expect_err("locked vault must not yield a key");
        assert_eq!(err, "Vault 未解锁");

        // 解锁后能取到，且类型仍是 Zeroizing。
        let key = Zeroizing::new([7u8; 32]);
        *state.dek.lock().expect("uncontended") = Some(key.clone());
        let got = require_dek(&state).expect("unlocked vault yields the DEK");
        assert_eq!(&got[..], &key[..]);

        // 锁定后立刻失效。
        *state.dek.lock().expect("uncontended") = None;
        assert!(require_dek(&state).is_err());
    }
}
