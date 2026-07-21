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

// ============ Modules ============

pub mod ssh;
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
    /// Private key PEM content (transient in memory). Encrypted at rest in
    /// the `private_key_pem_enc` column. ssh.rs loads via
    /// `russh::keys::decode_secret_key` from this string — no file IO.
    pub private_key_pem: Option<String>,
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

pub struct AppState {
    /// Arc-wrapped so per-session SshClient handlers can clone a reference
    /// for `check_server_key` lookups without borrowing State.
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub ssh_sessions: Arc<Mutex<std::collections::HashMap<String, ssh::SshSession>>>,
    pub ftp_sessions: Arc<Mutex<std::collections::HashMap<String, ftp::FtpSession>>>,
    /// Local PTY terminal sessions, keyed by UUID session id (== frontend
    /// tab id, same invariant as ssh_sessions).
    pub local_sessions: Arc<Mutex<std::collections::HashMap<String, local::LocalSession>>>,
    pub zmodem_files: Mutex<HashMap<String, ZmodemFileHandle>>,
    /// Data Encryption Key (DEK) — random 32-byte key for encrypting all
    /// database columns and keyring entries. Derived once at setup and
    /// stored encrypted by the login password. `None` until unlocked.
    pub dek: Arc<Mutex<Option<[u8; 32]>>>,
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
pub fn require_dek(state: &AppState) -> Result<[u8; 32], String> {
    state
        .dek
        .lock()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Vault 未解锁".to_string())
}
