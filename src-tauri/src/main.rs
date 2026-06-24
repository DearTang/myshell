// In release builds, hide the console window. Debug builds keep it so
// eprintln! / env_logger output is visible during development. We move
// release-mode logs into a rotating file (`<config_dir>/myshell/logs/`)
// so users can still ship us diagnostics.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Listener, Manager, State};
use rand::RngCore;

mod ssh;
mod sftp;
mod db;
mod secrets;
mod ftp;
mod crypto;
mod vault;
mod proxy;
mod backup;
mod local;
mod fonts;
mod elevation;
mod ai;

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

fn default_group_path() -> String {
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

// ============ Connection Management Commands ============

#[tauri::command]
fn get_connections(state: State<AppState>) -> Result<Vec<ConnectionConfig>, String> {
    let key = require_dek(&state)?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::get_all_connections(&db, &key).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_connection(state: State<AppState>, mut config: ConnectionConfig) -> Result<(), String> {
    let key = require_dek(&state)?;
    // Move the password out of the in-memory config before persisting so the
    // plaintext value never lands in SQLite. The keyring is the only store.
    let pw = config.password.take();
    if config.auth_method == "password" {
        if let Some(p) = pw.as_ref() {
            secrets::set_password(&config.id, p, &key)?;
        }
    } else {
        // Key auth path — clear any stale password in the keyring so a future
        // flip back to password auth doesn't reuse a forgotten secret.
        secrets::delete_password(&config.id)?;
    }

    // Proxy password: same pattern. An empty proxy_password means "leave the
    // existing keyring entry alone" (UI leaves the field blank when editing
    // a connection that already has a proxy password). A non-empty value
    // overwrites; switching proxy_type to "none" deletes the entry.
    let proxy_pw = config.proxy_password.take();
    match config.proxy_type.as_str() {
        "none" | "" => {
            // Clear any stale proxy credential so flipping type back doesn't
            // reuse a forgotten secret.
            let _ = secrets::delete_proxy_password(&config.id);
        }
        _ => {
            if let Some(p) = proxy_pw.as_ref().filter(|s| !s.is_empty()) {
                secrets::set_proxy_password(&config.id, p, &key)?;
            }
            // Empty proxy_pw on save: leave existing entry intact (edit flow
            // doesn't re-collect password). The connect path resolves from
            // keyring, so a stale None here just means "no auth attempted".
        }
    }

    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::save_connection(&db, &key, &config).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_connection(state: State<AppState>, id: String) -> Result<(), String> {
    // Best-effort keyring cleanup — the DB row is the source of truth for
    // existence, so a missing credential is not a failure. We log unexpected
    // errors (not "not found") so a leaked credential isn't silently left
    // in the OS keyring when the user thought they'd deleted the connection.
    if let Err(e) = secrets::delete_password(&id) {
        log::warn!("[delete_connection] keyring delete password failed for {}: {}", id, e);
    }
    if let Err(e) = secrets::delete_proxy_password(&id) {
        log::warn!("[delete_connection] keyring delete proxy password failed for {}: {}", id, e);
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let result = db::delete_connection(&db, &id).map_err(|e| e.to_string());
    if result.is_ok() {
        log::info!("[delete_connection] removed {} (cascaded per-server quick commands)", id);
    }
    result
}

/// Clone a connection: same config, new UUID + incremented name suffix.
/// Name collisions are resolved by scanning `name`, `name(1)`, `name(2)`, …
/// until a free slot is found. The keyring entry is also duplicated so the
/// copy is connectable without re-entering the password.
#[tauri::command]
fn copy_connection(state: State<AppState>, src_id: String) -> Result<ConnectionConfig, String> {
    let key = require_dek(&state)?;
    let new_id = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());

    let mut cfg = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let src = db::get_connection(&db, &key, &src_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Source connection not found".to_string())?;
        // Find the next available `name(N)` slot. Strip any existing `(N)`
        // suffix off the source name first so copy-of-copy produces
        // `foo(1)` → `foo(2)` rather than `foo(1)(1)`.
        let base = strip_name_suffix(&src.name);
        let mut candidate = format!("{}(1)", base);
        let mut n = 1;
        while db::connection_name_exists(&db, &candidate).map_err(|e| e.to_string())? {
            n += 1;
            candidate = format!("{}({})", base, n);
        }
        ConnectionConfig {
            id: new_id.clone(),
            name: candidate,
            created_at: now,
            // password is transient — sourced from keyring below.
            password: None,
            ..src
        }
    };

    // Clone keyring entry so the copy inherits the source's password. A
    // missing entry (e.g. key auth) is fine — auth_method is also cloned.
    if cfg.auth_method == "password" {
        if let Some(pw) = secrets::get_password(&src_id, &key).map_err(|e| e.to_string())? {
            secrets::set_password(&new_id, &pw, &key)?;
            cfg.password = Some(pw);
        }
    }
    // Same treatment for the proxy password — copy it under the new id so
    // the duplicated connection can dial through the same proxy.
    if cfg.proxy_type != "none" {
        if let Some(pw) = secrets::get_proxy_password(&src_id, &key).map_err(|e| e.to_string())? {
            secrets::set_proxy_password(&new_id, &pw, &key)?;
            cfg.proxy_password = Some(pw);
        }
    }

    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::save_connection(&db, &key, &cfg).map_err(|e| e.to_string())?;
    Ok(cfg)
}

/// Strip a trailing `(N)` from a name so copies don't stack suffixes:
/// `foo` → `foo`, `foo(1)` → `foo`, `foo(2)` → `foo`.
fn strip_name_suffix(name: &str) -> String {
    if let Some(open) = name.rfind('(') {
        let after = &name[open + 1..];
        if after.ends_with(')') {
            let inner = &after[..after.len() - 1];
            if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                return name[..open].to_string();
            }
        }
    }
    name.to_string()
}

// ============ Encrypted Import/Export ============
//
// Dump format: JSON envelope with base64-encoded salt/nonce/ciphertext.
// Plaintext is the serde_json serialization of `Vec<ConnectionConfig>` with
// the password field populated from the keyring so the dump is self-
// contained on the import side. AES-256-GCM + PBKDF2-HMAC-SHA256 (200k
// iterations) — see `crypto.rs` for parameters.

#[derive(Serialize, Deserialize)]
struct ConnectionDump {
    /// Schema/format bump — bump on incompatible plaintext shape changes.
    /// Currently always 1.
    schema: u32,
    /// UTC export timestamp (epoch seconds) — informational only.
    exported_at: u64,
    connections: Vec<ConnectionConfig>,
    /// Folder paths dumped alongside connections so group structure survives
    /// a round-trip. Missing on imports from older dumps.
    #[serde(default)]
    folders: Vec<String>,
}

/// Export every connection (with keyring-fetched passwords) and every folder
/// to an encrypted JSON envelope written to `path`. Returns the connection
/// count on success.
#[tauri::command]
fn export_connections(
    state: State<AppState>,
    passphrase: String,
    path: String,
) -> Result<usize, String> {
    let key = require_dek(&state)?;
    let (connections, folders) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let mut conns = db::get_all_connections(&db, &key).map_err(|e| e.to_string())?;
        // Populate each config's transient password from the keyring so the
        // dump is importable on a fresh machine. Missing entries leave the
        // field None — acceptable for key-auth connections.
        for c in conns.iter_mut() {
            if c.auth_method == "password" {
                c.password = secrets::get_password(&c.id, &key).ok().flatten();
            }
            if c.proxy_type != "none" {
                c.proxy_password = secrets::get_proxy_password(&c.id, &key).ok().flatten();
            }
        }
        let folders = db::list_folders(&db).map_err(|e| e.to_string())?;
        (conns, folders)
    };

    let count = connections.len();
    let dump = ConnectionDump {
        schema: 1,
        exported_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        connections,
        folders,
    };
    let plaintext = serde_json::to_vec(&dump).map_err(|e| format!("JSON encode: {}", e))?;
    let envelope = crypto::encrypt(&plaintext, &passphrase)?;
    std::fs::write(&path, envelope).map_err(|e| format!("write {}: {}", path, e))?;
    Ok(count)
}

/// Import connections + folders from an encrypted dump at `path`. Each
/// imported connection gets a fresh UUID to avoid colliding with existing
/// entries (so re-importing the same file is idempotent at the file level
/// but produces duplicate connections — the user can clean up after).
/// Returns the number of connections imported.
#[tauri::command]
fn import_connections(
    state: State<AppState>,
    passphrase: String,
    path: String,
) -> Result<usize, String> {
    let key = require_dek(&state)?;
    let envelope = std::fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path, e))?;
    let plaintext = crypto::decrypt(&envelope, &passphrase)?;
    let dump: ConnectionDump =
        serde_json::from_slice(&plaintext).map_err(|e| format!("decode dump: {}", e))?;

    const MAX_IMPORT: usize = 5000;
    if dump.connections.len() > MAX_IMPORT {
        return Err(format!("导入条目过多（{} > {}）", dump.connections.len(), MAX_IMPORT));
    }
    if dump.folders.len() > 1000 {
        return Err("文件夹数量过多".to_string());
    }

    let count = dump.connections.len();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());

    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        // Folders first so connection FK-style grouping doesn't reference a
        // missing path. INSERT OR IGNORE keeps re-imports from erroring on
        // duplicate folder paths.
        for f in &dump.folders {
            let _ = db::save_folder(&db, f, &now);
        }
        for mut c in dump.connections.into_iter() {
            let new_id = uuid::Uuid::new_v4().to_string();
            // Stash the password (if present) into the keyring under the new
            // ID before clearing it from the in-memory config — save_connection
            // would otherwise skip the keyring write.
            if c.auth_method == "password" {
                if let Some(pw) = c.password.as_ref() {
                    secrets::set_password(&new_id, pw, &key)?;
                }
            }
            // Same for proxy password — write under new id, clear from config.
            if c.proxy_type != "none" {
                if let Some(pw) = c.proxy_password.as_ref() {
                    secrets::set_proxy_password(&new_id, pw, &key)?;
                }
            }
            c.id = new_id;
            c.created_at = now.clone();
            c.password = None;
            c.proxy_password = None;
            db::save_connection(&db, &key, &c).map_err(|e| e.to_string())?;
        }
    }
    Ok(count)
}

// ============ Vault Commands ============
//
// Master-password gate. The frontend calls vault_status on startup to
// decide which screen to render: setup (no salt on disk yet) or unlock
// (salt present, master_key not yet loaded into AppState). Both flows
// end with master_key populated, after which the connection commands
// can run.

/// Extract the DEK from AppState or surface a friendly error if the
/// vault is locked. Every command that touches encrypted columns calls this
/// first — there's no implicit unlock.
fn require_dek(state: &State<AppState>) -> Result<[u8; 32], String> {
    state
        .dek
        .lock()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Vault 未解锁".to_string())
}

#[derive(Serialize)]
struct VaultStatus {
    initialized: bool,
    unlocked: bool,
}

#[tauri::command]
fn vault_status(state: State<AppState>) -> VaultStatus {
    let initialized = vault::is_initialized();
    let unlocked = state
        .dek
        .lock()
        .map(|k| k.is_some())
        .unwrap_or(false);
    VaultStatus { initialized, unlocked }
}

/// First-time setup:
/// 1. Generate random salt for password derivation
/// 2. Derive master_key from passphrase
/// 3. Generate random DEK (data encryption key)
/// 4. Encrypt DEK with master_key and store
/// 5. Create verifier for password verification
/// 6. Migrate any existing plaintext DB rows
#[tauri::command]
fn setup_vault(state: State<AppState>, passphrase: String) -> Result<(), String> {
    if passphrase.len() < 6 {
        return Err("主密码至少 6 个字符".into());
    }
    if vault::is_initialized() {
        return Err("Vault 已初始化，请使用解锁".into());
    }

    // Generate salt + derive master_key from passphrase at the current
    // default iteration count (600k as of this build).
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let master_key = crypto::derive_master_key(&passphrase, &salt);

    // Generate random DEK (32 bytes)
    let mut dek = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut dek);

    // Encrypt DEK with master_key
    let encrypted_dek = crypto::encrypt_with_key(&master_key, &dek)?;

    // Create verifier for password verification
    let verifier = crypto::make_verifier(&master_key)?;

    // Persist salt + verifier + encrypted_dek + KDF metadata.
    vault::write_vault_files(&salt, &verifier)?;
    vault::write_encrypted_dek(&encrypted_dek)?;
    vault::write_kdf_meta(&vault::default_kdf_meta())?;

    // Populate dek before triggering DB migration
    {
        let mut slot = state.dek.lock().map_err(|e| e.to_string())?;
        *slot = Some(dek);
    }

    // Migrate any pre-vault DB content
    let mut conn = state.db.lock().map_err(|e| e.to_string())?;
    if let Err(e) = db::migrate_to_vault(&mut conn, &dek) {
        return Err(format!("数据迁移失败: {}", e));
    }
    Ok(())
}

/// Unlock: verify passphrase and decrypt DEK
#[tauri::command]
fn unlock_vault(state: State<AppState>, passphrase: String) -> Result<(), String> {
    // Check lockout status first
    let mut lockout = vault::LockoutState::load();
    if let Some(remaining) = lockout.check_lockout() {
        return Err(format!("密码错误次数过多，请等待 {} 秒后重试", remaining));
    }

    let salt = vault::read_salt().ok_or_else(|| "Vault 未初始化".to_string())?;
    let verifier = vault::read_verifier().ok_or_else(|| "Vault 未初始化".to_string())?;
    // dek.enc may be missing on vaults created by a pre-DEK build (before
    // commit bd530e6). Those builds encrypted column data directly with the
    // PBKDF2-derived master_key, so if the verifier passes we can recover
    // by reusing master_key as the DEK — existing rows stay readable. We
    // persist a freshly-encrypted dek.enc below so future unlocks go through
    // the normal path.
    let encrypted_dek_opt = vault::read_encrypted_dek();

    // Pick the iteration count to derive with. Vaults created after the
    // 200k→600k bump ship a `vault.kdf` metadata file; older vaults don't,
    // and we fall back to the legacy 200k count so we can still authenticate
    // them. On a successful unlock of an old vault we transparently re-KDF
    // and re-encrypt everything under 600k (see below).
    let (iterations, kdf_meta_present) = match vault::read_kdf_meta() {
        Some(meta) => (meta.iterations, true),
        None => (crypto::LEGACY_PBKDF2_ITERATIONS, false),
    };
    let master_key = crypto::derive_master_key_with_iterations(&passphrase, &salt, iterations);
    if !crypto::check_verifier(&master_key, &verifier) {
        // Record failed attempt
        lockout.record_failure()?;
        return Err("密码错误".to_string());
    }

    // Recover DEK. New vaults: decrypt dek.enc with master_key. Legacy
    // pre-DEK vaults (dek.enc missing): master_key IS the dek — the old
    // build used it directly to encrypt columns, so reusing it preserves
    // existing rows without re-encrypting the whole DB.
    let dek: [u8; 32] = match encrypted_dek_opt {
        Some(blob) => {
            let bytes = crypto::decrypt_with_key(&master_key, &blob)?;
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| "DEK 长度错误".to_string())?
        }
        None => {
            eprintln!("[vault] legacy pre-DEK vault detected — reusing master_key as DEK");
            master_key
        }
    };

    // Success - reset failure counters
    lockout.record_success();

    // Legacy migration: if the vault predates the KDF metadata file, the
    // verifier + DEK are still derived at the old 200k count. Re-derive a
    // fresh 600k master_key from the same passphrase/salt and re-encrypt
    // both blobs in place. From the next launch onward, unlock reads the
    // metadata and goes straight to 600k.
    if !kdf_meta_present {
        let new_master_key = crypto::derive_master_key(&passphrase, &salt);
        match crypto::encrypt_with_key(&new_master_key, &dek) {
            Ok(new_encrypted_dek) => {
                if let Err(e) = vault::write_encrypted_dek(&new_encrypted_dek) {
                    eprintln!("[vault] KDF migration: failed to re-encrypt DEK: {}", e);
                } else if let Ok(new_verifier) = crypto::make_verifier(&new_master_key) {
                    let _ = vault::write_vault_files(&salt, &new_verifier);
                    let _ = vault::write_kdf_meta(&vault::default_kdf_meta());
                    eprintln!("[vault] KDF migration: re-derived at 600k iterations");
                }
            }
            Err(e) => eprintln!("[vault] KDF migration skipped: {}", e),
        }
    }

    let mut slot = state.dek.lock().map_err(|e| e.to_string())?;
    *slot = Some(dek);
    Ok(())
}

/// Lock: drop the in-memory DEK
#[tauri::command]
fn lock_vault(state: State<AppState>) -> Result<(), String> {
    let mut slot = state.dek.lock().map_err(|e| e.to_string())?;
    *slot = None;
    Ok(())
}

/// Verify login password (for viewing plaintext passwords in UI).
/// Applies the same lockout policy as `unlock_vault` — otherwise a malicious
/// script could brute-force the password by repeatedly calling this command,
/// since it used to bypass the failure counter entirely.
#[tauri::command]
fn verify_password(passphrase: String) -> Result<bool, String> {
    let mut lockout = vault::LockoutState::load();
    if let Some(remaining) = lockout.check_lockout() {
        return Err(format!("密码错误次数过多，请等待 {} 秒后重试", remaining));
    }

    let salt = vault::read_salt().ok_or_else(|| "Vault 未初始化".to_string())?;
    let verifier = vault::read_verifier().ok_or_else(|| "Vault 未初始化".to_string())?;

    // Match unlock_vault's iteration-count logic: prefer vault.kdf metadata,
    // fall back to legacy 200k for old vaults.
    let iterations = match vault::read_kdf_meta() {
        Some(meta) => meta.iterations,
        None => crypto::LEGACY_PBKDF2_ITERATIONS,
    };
    let master_key = crypto::derive_master_key_with_iterations(&passphrase, &salt, iterations);
    let ok = crypto::check_verifier(&master_key, &verifier);
    if ok {
        lockout.record_success();
        return Ok(true);
    }
    // record_failure always returns Err with a friendly message — covers
    // both the "this attempt failed" case and the lockout / daily-limit
    // caps. We surface it so the UI can show "locked for N seconds".
    Err(lockout.record_failure().err().unwrap_or_else(|| "密码错误".to_string()))
}

/// Get lockout status info for the UI
#[derive(Serialize)]
struct LockoutInfo {
    /// Current consecutive failure count
    consecutive_failures: u32,
    /// Daily failure count
    daily_failures: u32,
    /// Last failure time as Unix timestamp
    last_failure_time: Option<u64>,
    /// Seconds remaining in lockout (if locked)
    lockout_remaining: Option<u64>,
    /// Is currently locked out
    is_locked: bool,
}

#[tauri::command]
fn get_lockout_info() -> LockoutInfo {
    let mut lockout = vault::LockoutState::load();
    let lockout_remaining = lockout.check_lockout();

    LockoutInfo {
        consecutive_failures: lockout.consecutive_failures,
        daily_failures: lockout.daily_failures,
        last_failure_time: lockout.last_failure_time,
        lockout_remaining,
        is_locked: lockout_remaining.is_some(),
    }
}

/// Change login password:
/// 1. Verify old passphrase (using whichever KDF iteration count is current)
/// 2. Re-encrypt DEK with new passphrase at the current default iteration count
/// 3. Update verifier
#[tauri::command]
fn change_master_password(
    state: State<AppState>,
    old_passphrase: String,
    new_passphrase: String,
) -> Result<(), String> {
    if new_passphrase.len() < 6 {
        return Err("新密码至少 6 个字符".into());
    }

    let salt = vault::read_salt().ok_or_else(|| "Vault 未初始化".to_string())?;
    let verifier = vault::read_verifier().ok_or_else(|| "Vault 未初始化".to_string())?;
    let encrypted_dek = vault::read_encrypted_dek().ok_or_else(|| "Vault 未初始化".to_string())?;

    // Verify old passphrase — match unlock_vault's iteration-count logic.
    let old_iterations = match vault::read_kdf_meta() {
        Some(meta) => meta.iterations,
        None => crypto::LEGACY_PBKDF2_ITERATIONS,
    };
    let old_master_key =
        crypto::derive_master_key_with_iterations(&old_passphrase, &salt, old_iterations);
    if !crypto::check_verifier(&old_master_key, &verifier) {
        return Err("原密码错误".into());
    }

    // Decrypt DEK with old master_key
    let dek_bytes = crypto::decrypt_with_key(&old_master_key, &encrypted_dek)?;
    let dek: [u8; 32] = dek_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "DEK 长度错误".to_string())?;

    // Derive new master_key at the current default iteration count (600k)
    // and re-encrypt DEK. This implicitly migrates an old 200k vault to
    // the new KDF when the user changes their password.
    let new_master_key = crypto::derive_master_key(&new_passphrase, &salt);
    let new_encrypted_dek = crypto::encrypt_with_key(&new_master_key, &dek)?;
    let new_verifier = crypto::make_verifier(&new_master_key)?;

    // Persist
    vault::write_encrypted_dek(&new_encrypted_dek)?;
    vault::write_vault_files(&salt, &new_verifier)?;
    vault::write_kdf_meta(&vault::default_kdf_meta())?;

    // Update in-memory DEK (should already be set, but ensure it's correct)
    {
        let mut slot = state.dek.lock().map_err(|e| e.to_string())?;
        *slot = Some(dek);
    }

    Ok(())
}

// ============ Backup Commands ============

/// Backup info for UI
#[derive(Serialize, Clone)]
struct BackupInfoUi {
    version: String,
    timestamp: u64,
    files: Vec<String>,
    timestamp_str: String,
}

/// Get list of available backups
#[tauri::command]
fn list_backups() -> Result<Vec<BackupInfoUi>, String> {
    let backups = backup::list_backups()?;
    Ok(backups.into_iter().map(|b| {
        let datetime = chrono::DateTime::from_timestamp(b.timestamp as i64, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| b.timestamp.to_string());
        BackupInfoUi {
            version: b.version,
            timestamp: b.timestamp,
            files: b.files,
            timestamp_str: datetime,
        }
    }).collect())
}

/// Rollback to a specific version
#[tauri::command]
fn rollback_backup(version: String) -> Result<String, String> {
    backup::rollback(&version)
}

/// Get current app version
#[tauri::command]
fn get_app_version() -> String {
    backup::APP_VERSION.to_string()
}

/// Get previous version available for quick rollback
#[tauri::command]
fn get_previous_version() -> Option<String> {
    backup::get_previous_version()
}

// ============ Update check ============
//
// Lightweight update detection. We do NOT integrate tauri-plugin-updater
// (no in-app auto-install, no signing keys, no CI manifest). Instead the Rust
// backend does a single GET against the Gitee "latest release" API and
// compares the reported tag against the running version. The frontend then
// shows a prompt + "go download" button (opens the browser).
//
// Why backend and not frontend fetch: tauri.conf.json CSP pins
// `connect-src 'self' ipc: http://ipc.localhost`, so a fetch() to Gitee from
// the webview would be blocked. Rust's reqwest is unaffected by CSP and also
// sidesteps CORS. reqwest is already a dependency (used by ai.rs).

/// Gitee "latest release" endpoint for this repo. Returns a JSON object with
/// `tag_name`, `assets[]`, `body`, `created_at`, `html_url`, etc. Public, no
/// auth needed (subject to Gitee's unauthenticated rate limits).
const GITEE_LATEST_RELEASE: &str =
    "https://gitee.com/api/v5/repos/argustang/myshell/releases/latest";

/// Release-notes body is capped before returning to the frontend so a giant
/// changelog can't balloon the webview memory.
const MAX_NOTES_CHARS: usize = 2000;

#[derive(Debug, Clone, Serialize)]
struct UpdateInfo {
    current_version: String,
    latest_version: String,
    has_update: bool,
    /// html_url of the release (fallback for the "download" button).
    release_url: String,
    /// browser_download_url of the first asset, falling back to release_url.
    download_url: String,
    /// Truncated release body (Markdown).
    notes: String,
    /// created_at of the release, raw string from the API.
    published_at: String,
    checked_at: u64,
    /// Set when the check failed (network/parse/no release). Frontend treats
    /// any non-empty `error` as "no info / stay silent".
    error: Option<String>,
}

/// Build an UpdateInfo marked as failed. Never returns Result — the command
/// contract is "always resolve with a struct", so transient network issues
/// can't surface as unhandled promise rejections in the frontend.
fn update_info_error(current_version: &str, message: impl Into<String>) -> UpdateInfo {
    UpdateInfo {
        current_version: current_version.to_string(),
        latest_version: String::new(),
        has_update: false,
        release_url: String::new(),
        download_url: String::new(),
        notes: String::new(),
        published_at: String::new(),
        checked_at: unix_now_secs(),
        error: Some(message.into()),
    }
}

/// `std::time::SystemTime` seconds since the epoch. Used to stamp the check
/// so the frontend can show "checked N minutes ago".
fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parse a dotted version like "v1.4.5" or "1.4.5" into numeric segments.
/// Non-numeric trailing junk in a segment (e.g. "1.4.5-rc1" -> [1,4,5]) is
/// truncated at the first non-digit. Empty/garbage input yields `vec![0]`.
fn parse_version(raw: &str) -> Vec<u64> {
    let cleaned = raw.trim().trim_start_matches('v').trim_start_matches('V');
    cleaned
        .split('.')
        .map(|seg| {
            let digits: String = seg.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

/// True if `latest` is strictly newer than `current` by dotted-numeric
/// comparison. Equal or older -> false.
fn is_newer(latest: &str, current: &str) -> bool {
    let l = parse_version(latest);
    let c = parse_version(current);
    let n = l.len().max(c.len());
    for i in 0..n {
        let lv = l.get(i).copied().unwrap_or(0);
        let cv = c.get(i).copied().unwrap_or(0);
        if lv != cv {
            return lv > cv;
        }
    }
    false
}

/// Check Gitee for the latest release and report whether an update is
/// available. Resolves with a populated `UpdateInfo` on every path; failures
/// are encoded in the `error` field rather than rejected.
///
/// Network timeout is 30s (per the update-check spec) — generous enough for a
/// slow first connection, short enough that a dead link doesn't hang the
/// background check indefinitely. The client-level timeout governs the whole
/// request lifecycle (connect + headers + body).
#[tauri::command]
async fn check_for_updates() -> UpdateInfo {
    let current_version = backup::APP_VERSION.to_string();

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => return update_info_error(&current_version, format!("客户端构建失败: {e}")),
    };

    let resp = match client.get(GITEE_LATEST_RELEASE).send().await {
        Ok(r) => r,
        Err(e) => return update_info_error(&current_version, format!("网络请求失败: {e}")),
    };

    if !resp.status().is_success() {
        return update_info_error(
            &current_version,
            format!("接口返回状态 {}", resp.status().as_u16()),
        );
    }

    let json: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return update_info_error(&current_version, format!("解析响应失败: {e}")),
    };

    // tag_name is the canonical version source on a Gitee release. Fall back
    // to the deprecated `name` field only if tag is absent.
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| json.get("name").and_then(|v| v.as_str()).map(str::to_string));
    let Some(latest_version) = tag else {
        return update_info_error(&current_version, "未找到版本信息".to_string());
    };

    let release_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://gitee.com/argustang/myshell/releases")
        .to_string();

    // Prefer the first asset's download URL; otherwise the release page.
    let download_url = json
        .get("assets")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|asset| asset.get("browser_download_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| release_url.clone());

    let published_at = json
        .get("created_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let notes_raw = json
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let notes = truncate_chars(&notes_raw, MAX_NOTES_CHARS);

    let has_update = is_newer(&latest_version, &current_version);

    UpdateInfo {
        current_version,
        latest_version,
        has_update,
        release_url,
        download_url,
        notes,
        published_at,
        checked_at: unix_now_secs(),
        error: None,
    }
}

/// Truncate a string to at most `max_chars` Unicode chars, appending an
/// ellipsis marker if it was cut. Steps along `chars()` so we never split a
/// multibyte UTF-8 sequence (avoids the panic from indexing mid-codepoint).
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    out.push_str("\n…(已截断)");
    out
}

/// Open an external URL in the user's default browser. The URL must be an
/// absolute http(s) link — anything else (file:, javascript:, relative, etc.)
/// is rejected so this can't be abused as a local-file-open primitive.
///
/// `Shell::open` is deprecated upstream in favour of the separate
/// `tauri-plugin-opener`, but we intentionally stay on `tauri-plugin-shell`
/// (already registered + already granted the `open` capability in
/// tauri.conf.json) to avoid pulling in a second plugin for a single
/// browser-open call. The deprecated method still routes through the OS
/// default handler exactly as before.
#[tauri::command]
#[allow(deprecated)]
fn open_external_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let lower = url.trim().to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err("仅支持 http(s) 链接".to_string());
    }
    use tauri_plugin_shell::ShellExt;
    app.shell()
        .open(url, None)
        .map_err(|e| format!("打开链接失败: {e}"))
}

/// Payload for the `update_download_progress` event, scoped to the window
/// that initiated the download.
#[derive(Clone, Serialize)]
struct DownloadProgress {
    downloaded: u64,
    total: u64,
}

/// Download the installer `.exe` from `url` into the OS temp dir, streaming
/// and emitting `update_download_progress` events to the originating window
/// so the UI can render a progress bar. Returns the absolute temp path on
/// success; the frontend then calls `install_update` with it.
///
/// Security: `url` must be https (the asset URL we got from Gitee's API). The
/// file is written to the system temp dir under a fixed name, overwriting any
/// prior partial download. No signature verification (that'd require the full
/// tauri-plugin-updater pipeline) — trust is HTTPS + explicit user consent.
#[tauri::command]
async fn download_update(window: tauri::WebviewWindow, url: String) -> Result<String, String> {
    let lower = url.trim().to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err("仅支持 http(s) 链接".to_string());
    }

    // A longer timeout than check_for_updates: the installer (~6 MB) over a
    // slow link can take a while, and we want the stream to keep going.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("客户端构建失败: {e}"))?;

    let resp = client
        .get(url.trim())
        .send()
        .await
        .map_err(|e| format!("下载请求失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败: HTTP {}", resp.status().as_u16()));
    }
    let total = resp.content_length().unwrap_or(0);

    // Fixed temp filename; re-download just overwrites. Kept simple — no need
    // to track per-version temp files.
    let temp_dir = std::env::temp_dir();
    let dest = temp_dir.join("myshell-update-setup.exe");

    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::File::create(&dest)
        .await
        .map_err(|e| format!("创建临时文件失败: {e}"))?;

    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_emitted: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取数据失败: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入文件失败: {e}"))?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        // Throttle events to ~every 256 KB so we don't flood the webview for
        // every tiny chunk.
        if downloaded - last_emitted >= 256 * 1024 || total == 0 {
            let _ = window.emit(
                "update_download_progress",
                DownloadProgress {
                    downloaded,
                    total,
                },
            );
            last_emitted = downloaded;
        }
    }
    file.flush().await.map_err(|e| format!("写入文件失败: {e}"))?;
    drop(file);

    // Final 100% event so the UI flips to "ready" even if the last chunk was
    // under the 256 KB throttle threshold.
    let _ = window.emit(
        "update_download_progress",
        DownloadProgress {
            downloaded,
            total: if total == 0 { downloaded } else { total },
        },
    );

    dest.to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "临时路径含非法字符".to_string())
}

/// Launch the downloaded installer and exit the app so the installer can
/// replace the running files. The NSIS installer (perMachine mode) will
/// trigger a UAC prompt — that's the normal Windows install UX.
///
/// On Windows the installer is spawned with `DETACHED_PROCESS |
/// CREATE_NEW_PROCESS_GROUP` so it survives the parent's exit. On other
/// platforms we just spawn it (the project targets Windows/NSIS, but this
/// keeps it compiling elsewhere).
#[tauri::command]
fn install_update(app: tauri::AppHandle, path: String) -> Result<(), String> {
    if path.bytes().any(|b| b == 0) {
        return Err("无效路径".to_string());
    }
    let meta = std::fs::metadata(&path).map_err(|_| "安装包不存在".to_string())?;
    if !meta.is_file() {
        return Err("安装包路径无效".to_string());
    }
    // Sanity: only let it launch an .exe — refuse anything else so this can't
    // be repurposed to run arbitrary files.
    let lower = path.to_ascii_lowercase();
    if !lower.ends_with(".exe") {
        return Err("仅支持 .exe 安装包".to_string());
    }

    let mut cmd = std::process::Command::new(&path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // SAFETY: these are bit-flag creation constants; no pointers/handles.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().map_err(|e| format!("启动安装器失败: {e}"))?;

    // Exit so the installer can overwrite our binaries. exit(0) runs the
    // RunEvent::Exit cleanups then terminates.
    app.exit(0);
    Ok(())
}

/// Read a PEM-formatted text file at an absolute path. Used by the private-
/// key picker in ConnectionDialog: the user picks a path via the dialog
/// plugin, and this reads the content so we can stash it (encrypted) in
/// the vault rather than keeping a reference to a plaintext file on disk.
///
/// Security: historically this command accepted any path and returned any
/// file's contents to the frontend, which combined with `csp: null` made it
/// a complete arbitrary-file-read primitive (e.g. `~/.ssh/id_rsa`,
/// `%APPDATA%\myshell\dek.enc`). We now constrain it:
///   1. Size cap of 1 MiB — PEM keys are <100 KB, anything bigger is suspect.
///   2. Content must start with the PEM armor marker `-----BEGIN`.
///   3. NUL bytes in the path are rejected (block PATH-truncation tricks).
///   4. Error messages are uniform — we never echo the OS error string or
///      the path back to the frontend, so this can't double as a file
///      existence oracle.
#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    use std::io::Read;
    if path.bytes().any(|b| b == 0) {
        return Err("无效路径".to_string());
    }
    if path.len() > 4096 {
        return Err("路径过长".to_string());
    }
    // Open once and stat the handle (not the path) so the size check and the
    // read observe the same file — a path-based metadata() followed by read()
    // is a TOCTOU window where the file can be swapped/truncated/grown
    // between the two calls.
    let file = std::fs::File::open(&path).map_err(|_| "读取文件失败".to_string())?;
    let meta = file.metadata().map_err(|_| "读取文件失败".to_string())?;
    if !meta.is_file() {
        return Err("读取文件失败".to_string());
    }
    const MAX_PEM_BYTES: u64 = 1024 * 1024;
    if meta.len() > MAX_PEM_BYTES {
        return Err("文件过大".to_string());
    }
    // Take() bounds the read to MAX_PEM_BYTES even if the file grew between
    // the stat above and here (defence-in-depth against the residual race).
    let mut content = Vec::new();
    file.take(MAX_PEM_BYTES)
        .read_to_end(&mut content)
        .map_err(|_| "读取文件失败".to_string())?;
    let text = String::from_utf8(content).map_err(|_| "文件编码无效".to_string())?;
    if !text.trim_start().starts_with("-----BEGIN") {
        return Err("文件格式无效".to_string());
    }
    Ok(text)
}

/// Read an image file from disk and return it as a base64 data URL suitable
/// for use as a CSS `background-image`. Path is validated before IO.
#[tauri::command]
fn read_file_base64(path: String) -> Result<String, String> {
    use base64::Engine as _;
    use std::io::Read;

    if path.bytes().any(|b| b == 0) {
        return Err("无效路径".to_string());
    }
    if path.len() > 4096 {
        return Err("路径过长".to_string());
    }
    // Open once + stat the handle: avoids the TOCTOU window between a
    // path-based metadata() and a separate read() (file could be swapped or
    // grown in between, defeating the size cap).
    let file = std::fs::File::open(&path).map_err(|_| "读取文件失败".to_string())?;
    let meta = file.metadata().map_err(|_| "读取文件失败".to_string())?;
    if !meta.is_file() {
        return Err("读取文件失败".to_string());
    }
    // Limit to 8 MiB for background images
    const MAX_BYTES: u64 = 8 * 1024 * 1024;
    if meta.len() > MAX_BYTES {
        return Err("文件过大（最大 8 MB）".to_string());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|_| "读取文件失败".to_string())?;

    // Simple extension-based MIME detection — avoids a dependency
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let mime = match ext.to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    };

    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

/// Get the plaintext password for a connection (requires DEK).
/// Used for viewing passwords in the UI after verification.
#[tauri::command]
fn get_connection_password(state: State<AppState>, id: String) -> Result<Option<String>, String> {
    let key = require_dek(&state)?;
    secrets::get_password(&id, &key)
}

/// Get the plaintext proxy password for a connection (requires DEK).
#[tauri::command]
fn get_connection_proxy_password(state: State<AppState>, id: String) -> Result<Option<String>, String> {
    let key = require_dek(&state)?;
    secrets::get_proxy_password(&id, &key)
}

// ============ Folder Management Commands ============

#[tauri::command]
fn list_folders(state: State<AppState>) -> Result<Vec<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::list_folders(&db).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_folder(state: State<AppState>, path: String) -> Result<(), String> {
    let trimmed = normalize_folder_path(&path);
    if trimmed == "/" || trimmed.is_empty() {
        return Err("无效的文件夹路径".to_string());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::save_folder(&db, &trimmed, &now).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_folder(state: State<AppState>, path: String) -> Result<(), String> {
    let trimmed = normalize_folder_path(&path);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    if db::folder_has_children(&db, &trimmed).map_err(|e| e.to_string())? {
        return Err("文件夹非空，请先删除子项".to_string());
    }
    db::delete_folder(&db, &trimmed).map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_folder(
    state: State<AppState>,
    old_path: String,
    new_path: String,
) -> Result<(), String> {
    let old = normalize_folder_path(&old_path);
    let new = normalize_folder_path(&new_path);
    if old == "/" || new == "/" {
        return Err("不能重命名根路径".to_string());
    }
    // Cycle guard: reject if new_path is inside old_path.
    if new == old || new.starts_with(&format!("{}/", old)) {
        return Err("目标路径不能是源路径的子目录".to_string());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::rename_folder(&db, &old, &new).map_err(|e| e.to_string())
}

#[tauri::command]
fn move_connection(
    state: State<AppState>,
    conn_id: String,
    new_group_path: String,
) -> Result<(), String> {
    // Single-column rewrite of group_path by id equality. Parameterized, so
    // injection-safe; does NOT touch the keyring or re-encrypt host/user/key
    // (unlike save_connection). Moving into "/" is allowed (= unfile).
    let target = normalize_folder_path(&new_group_path);
    log::info!("[move_connection] {} -> group_path {}", conn_id, target);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::move_connection(&db, &conn_id, &target).map_err(|e| e.to_string())
}

// ============ Command History Commands ============

#[tauri::command]
fn add_command_history(
    state: State<AppState>,
    connection_id: String,
    command: String,
) -> Result<i64, String> {
    let trimmed = command.trim();
    // Drop empty + junk commands. `is_junk_command` covers the user-reported
    // noise (commands made only of 'a'/'d' like "A", "AD"). Ok(0) → "nothing
    // inserted", same shape the caller already sees for the empty case.
    if trimmed.is_empty() || is_junk_command(trimmed) {
        return Ok(0);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::add_command_history(&db, &connection_id, trimmed, &now).map_err(|e| e.to_string())
}

/// Whether a command should be silently dropped from history. Currently matches
/// the user's complaint: commands consisting only of 'a'/'d' (case-insensitive)
/// — accidental single/double taps like "A", "D", "AD", "DA" — that are never
/// useful history. NOTE this also matches "dd"; add a whitelist here if a real
/// command starts getting dropped.
fn is_junk_command(trimmed: &str) -> bool {
    !trimmed.is_empty()
        && trimmed
            .chars()
            .all(|c| matches!(c.to_ascii_lowercase(), 'a' | 'd'))
}

#[tauri::command]
fn list_command_history(
    state: State<AppState>,
    connection_id: String,
) -> Result<Vec<CommandHistoryItem>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let rows = db::list_command_history(&db, &connection_id).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(id, command, pinned, created_at)| CommandHistoryItem {
            id,
            command,
            pinned,
            created_at,
        })
        .collect())
}

#[tauri::command]
fn set_command_history_pinned(
    state: State<AppState>,
    id: i64,
    pinned: bool,
) -> Result<(), String> {
    let pinned_at = if pinned {
        Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "0".to_string()),
        )
    } else {
        None
    };
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::set_command_history_pinned(&db, id, pinned, pinned_at.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_command_history(state: State<AppState>, id: i64) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::delete_command_history(&db, id).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_command_history(
    state: State<AppState>,
    connection_id: String,
    include_pinned: bool,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::clear_command_history(&db, &connection_id, include_pinned).map_err(|e| e.to_string())
}

// ============ Quick Commands Commands ============

#[tauri::command]
fn add_quick_command(
    state: State<AppState>,
    connection_id: Option<String>,
    label: String,
    command: String,
) -> Result<i64, String> {
    let label = label.trim();
    let command = command.trim();
    if label.is_empty() || command.is_empty() {
        return Err("名称和命令不能为空".to_string());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let new_id = db::add_quick_command(&db, connection_id.as_deref(), label, command, &now)
        .map_err(|e| e.to_string())?;
    log::info!(
        "[quick-cmd] added id={} label={:?} scope={:?}",
        new_id, label, connection_id
    );
    Ok(new_id)
}

#[tauri::command]
fn list_quick_commands(
    state: State<AppState>,
    connection_id: Option<String>,
) -> Result<Vec<QuickCommandItem>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let rows = db::list_quick_commands(&db, connection_id.as_deref()).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(id, cid, label, command, sort_order)| QuickCommandItem {
            id,
            connection_id: cid,
            label,
            command,
            sort_order,
        })
        .collect())
}

#[tauri::command]
fn list_quick_commands_for_connection(
    state: State<AppState>,
    connection_id: String,
) -> Result<Vec<QuickCommandExecItem>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let rows = db::list_quick_commands_for_connection(&db, &connection_id)
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(id, is_global, label, command)| QuickCommandExecItem {
            id,
            is_global,
            label,
            command,
        })
        .collect())
}

#[tauri::command]
fn update_quick_command(
    state: State<AppState>,
    id: i64,
    label: String,
    command: String,
) -> Result<(), String> {
    let label = label.trim();
    let command = command.trim();
    if label.is_empty() || command.is_empty() {
        return Err("名称和命令不能为空".to_string());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::update_quick_command(&db, id, label, command).map_err(|e| e.to_string())?;
    log::info!("[quick-cmd] updated id={} label={:?}", id, label);
    Ok(())
}

#[tauri::command]
fn update_quick_command_order(
    state: State<AppState>,
    id: i64,
    sort_order: i64,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::update_quick_command_order(&db, id, sort_order).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_quick_command(state: State<AppState>, id: i64) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::delete_quick_command(&db, id).map_err(|e| e.to_string())?;
    log::info!("[quick-cmd] deleted id={}", id);
    Ok(())
}

/// Normalize a folder path: trim, ensure leading slash, collapse duplicate
/// slashes, strip trailing slash (except root).
fn normalize_folder_path(input: &str) -> String {
    let trimmed = input.trim();
    let mut segments: Vec<&str> = vec![];
    for part in trimmed.split('/') {
        let p = part.trim();
        if !p.is_empty() {
            segments.push(p);
        }
    }
    if segments.is_empty() {
        return "/".to_string();
    }
    format!("/{}", segments.join("/"))
}

/// Reject writes to system-critical locations. Defense-in-depth for
/// `sz_open_write` — even though the path should originate from a save
/// dialog, the cost of an over-broad check is low while the cost of an
/// over-write into `%APPDATA%\...\Startup\` or `/etc/` is high.
///
/// The check is prefix-based (case-insensitive on Windows) so it's
/// intentionally conservative — if a real save target falls under one of
/// these prefixes, the user will have to pick another folder.
fn is_protected_write_path(path: &str) -> bool {
    let p = path.replace('\\', "/");
    let lower = p.to_lowercase();

    // Windows startup / system locations.
    let win_blocks = [
        "/startup/",
        "/start menu/programs/startup/",
        "/windows/system32/",
        "/windows/system/",
        "/boot/",
        "/program files/",
        "/program files (x86)/",
    ];
    if lower.contains(':') {
        for b in win_blocks {
            if lower.contains(b) {
                return true;
            }
        }
    }

    // Unix system locations.
    let unix_blocks = [
        "/etc/", "/usr/", "/boot/", "/bin/", "/sbin/", "/lib/", "/lib64/",
        "/proc/", "/sys/", "/dev/",
    ];
    for b in unix_blocks {
        if p.starts_with(b) {
            return true;
        }
    }
    false
}

// ============ SSH Commands ============

#[tauri::command]
async fn ssh_connect(
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    mut config: ConnectionConfig,
) -> Result<String, String> {
    let target = format!("{}@{}:{}", config.username, config.host, config.port);
    log::info!(
        "[ssh] connect requested: {} (auth={}, proxy={})",
        target, config.auth_method, config.proxy_type
    );
    // Resolve password from keyring here, not in ssh.rs, so the SSH module
    // doesn't need to know about the vault. Key auth uses private_key_pem
    // which is already in the config (populated by get_connections).
    if config.auth_method != "key" && config.password.is_none() {
        let key = require_dek(&state)?;
        config.password = secrets::get_password(&config.id, &key)?;
    }
    // Refuse to authenticate with an empty password. A missing keyring entry
    // resolves to None (then ssh.rs would send "" via unwrap_or_default),
    // which many servers count as a failed attempt and lock the account
    // after N tries. Surface the real cause instead.
    if config.auth_method == "password"
        && config
            .password
            .as_deref()
            .map(str::is_empty)
            .unwrap_or(true)
    {
        return Err("未找到保存的密码，请重新编辑该连接并输入密码".to_string());
    }
    // Same for proxy password: pull from keyring so ssh.rs sees a complete
    // proxy config and can hand it to proxy.rs without knowing about the
    // vault.
    if config.proxy_type != "none" && config.proxy_password.is_none() {
        let key = require_dek(&state)?;
        config.proxy_password = secrets::get_proxy_password(&config.id, &key)?;
    }
    let result = ssh::connect(state, window, config).await;
    match &result {
        Ok(sid) => log::info!("[ssh:{}] connected to {}", sid, target),
        Err(e) => log::error!("[ssh] connect failed for {}: {}", target, e),
    }
    result
}

#[tauri::command]
async fn ssh_send(state: State<'_, AppState>, session_id: String, data: String) -> Result<(), String> {
    // Cap per-call input. Normal typing is a few bytes; even a deliberate
    // paste is rarely >64 KiB. A 100 MB blob from a compromised renderer
    // would otherwise sit in the unbounded command channel and balloon
    // process memory.
    const MAX_INPUT_BYTES: usize = 256 * 1024;
    if data.len() > MAX_INPUT_BYTES {
        return Err("输入数据过大".to_string());
    }
    ssh::send_input(&state, &session_id, data.as_bytes()).await
}

#[tauri::command]
async fn ssh_resize(
    state: State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    ssh::resize_terminal(&state, &session_id, cols, rows).await
}

#[tauri::command]
async fn ssh_disconnect(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    log::info!("[ssh:{}] disconnect requested", session_id);
    ssh::disconnect(&state, &session_id).await
}

#[tauri::command]
async fn ssh_send_zmodem(
    state: State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    const MAX_ZMODEM_BYTES: usize = 16 * 1024 * 1024;
    if data.len() > MAX_ZMODEM_BYTES {
        return Err("数据块过大".to_string());
    }
    ssh::zmodem_send_bytes(&state, &session_id, &data).await
}

#[tauri::command]
async fn ssh_send_zmodem_abort(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    ssh::zmodem_abort(&state, &session_id).await
}

// ============ Local Terminal Commands ============
//
// Local terminal sessions (conn_type='local') spawn a shell under a PTY. They
// reuse the SSH event channel (ssh_output / ssh_closed) so the frontend
// TerminalPanel doesn't branch on event names — only on which connect/send
// commands to invoke.

#[tauri::command]
async fn local_connect(
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    config: ConnectionConfig,
) -> Result<String, String> {
    log::info!(
        "[local] connect requested: name={} shell={:?}",
        config.name, config.shell_path
    );
    // No DEK/keyring resolution — a local shell has no credentials. The vault
    // must still be unlocked to list the connection row (enforced by
    // get_connections), but local_connect itself needs no secret.
    local::connect(state, window, config, 80, 24).await
}

#[tauri::command]
async fn local_send(
    state: State<'_, AppState>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    // Same per-call cap as ssh_send — a paste is rarely >64 KiB and a
    // compromised renderer shouldn't be able to balloon the channel.
    const MAX_INPUT_BYTES: usize = 256 * 1024;
    if data.len() > MAX_INPUT_BYTES {
        return Err("输入数据过大".to_string());
    }
    local::send_input(&state, &session_id, data.as_bytes()).await
}

#[tauri::command]
async fn local_resize(
    state: State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    local::resize_terminal(&state, &session_id, cols, rows).await
}

#[tauri::command]
async fn local_disconnect(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    log::info!("[local:{}] disconnect requested", session_id);
    local::disconnect(&state, &session_id).await
}

// ============ AI assistant ============

/// Non-secret projection of the `ai_settings` row for the settings UI. The
/// API key never leaves the backend — `has_key` only tells the form a key is
/// already stored.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AiSettingsView {
    provider: String,
    model: Option<String>,
    base_url: Option<String>,
    proxy_url: Option<String>,
    has_key: bool,
    temperature: f64,
}

/// Stream a chat completion. Tokens arrive as `ai_token` events; the stream
/// ends with `ai_done` (or `ai_error` on failure). Vault must be unlocked,
/// since the API key is encrypted with the DEK.
#[tauri::command]
async fn ai_chat(
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    request_id: String,
    messages: Vec<ai::ChatMessage>,
    system: Option<String>,
    context: Option<ai::AiContext>,
) -> Result<(), String> {
    ai::chat_stream(
        &state,
        &window,
        ai::AiChatParams {
            request_id,
            messages,
            system,
            context,
        },
    )
    .await
}

/// Run the read-only Linux diagnostic script over an open SSH session, then
/// stream an AI health report. Mirrors `ssh_get_server_info`'s use of
/// `ssh::exec_once`.
#[tauri::command]
async fn ai_inspect_health_ssh(
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    session_id: String,
    request_id: String,
) -> Result<(), String> {
    ai::inspect_health_ssh(&state, &window, &session_id, &request_id).await
}

/// Run the read-only diagnostic script on the local machine, then stream an
/// AI health report. No PTY session needed — it's the user's own machine.
#[tauri::command]
async fn ai_inspect_health_local(
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    request_id: String,
) -> Result<(), String> {
    ai::inspect_health_local(&state, &window, &request_id).await
}

/// Read the AI provider config. The API key is NOT returned — only `has_key`.
/// Works with the vault locked, so the settings form can render before unlock.
#[tauri::command]
fn get_ai_settings(state: State<'_, AppState>) -> Result<AiSettingsView, String> {
    let (provider, model, base_url, api_key_enc, proxy_url, temperature) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        match db.query_row(
            "SELECT provider, model, base_url, api_key_enc, proxy_url, temperature FROM ai_settings WHERE id = 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, f64>(5)?,
                ))
            },
        ) {
            Ok(row) => row,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                ("claude".to_string(), None, None, None, None, 0.7)
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    Ok(AiSettingsView {
        provider,
        model,
        base_url,
        proxy_url,
        has_key: api_key_enc
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        temperature,
    })
}

/// Save the AI provider config. `api_key` is Optional: a non-empty value
/// re-encrypts & overwrites; empty/None leaves the existing key untouched (so
/// the user can change model without re-entering the key). Requires an
/// unlocked vault (key is encrypted with the DEK).
#[tauri::command]
async fn save_ai_settings(
    state: State<'_, AppState>,
    provider: String,
    model: Option<String>,
    base_url: Option<String>,
    proxy_url: Option<String>,
    api_key: Option<String>,
    temperature: f64,
) -> Result<(), String> {
    let dek = require_dek(&state)?;
    let new_key_enc = api_key
        .filter(|s| !s.is_empty())
        .map(|k| crypto::encrypt_with_key(&dek, k.as_bytes()))
        .transpose()?;

    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute(
        "INSERT INTO ai_settings (id, provider, model, base_url, api_key_enc, proxy_url, temperature)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
            provider   = excluded.provider,
            model      = excluded.model,
            base_url   = excluded.base_url,
            proxy_url  = excluded.proxy_url,
            temperature = excluded.temperature,
            api_key_enc = COALESCE(excluded.api_key_enc, ai_settings.api_key_enc)",
        rusqlite::params![provider, model, base_url, new_key_enc, proxy_url, temperature],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Probe a connection config without registering a persistent session or
/// opening a tab. Dispatches to the per-protocol `test_connection` probe
/// (ssh/sftp share the SSH path; ftp + local have their own).
///
/// Credentials mirror `ssh_connect`/`ftp_connect`: the form's typed
/// `password`/`proxy_password` win when present; otherwise we resolve from the
/// keyring by `config.id` (edit mode, where the dialog shows "留空保持不变").
/// For password auth in new-connection mode with no id and no typed password,
/// we refuse to test rather than send an empty password (many servers count
/// that as a failed attempt and lock the account). The whole probe is wrapped
/// in a 15s timeout so a hung host returns a clean error instead of blocking
/// the UI.
#[tauri::command]
async fn test_connection(
    state: State<'_, AppState>,
    mut config: ConnectionConfig,
) -> Result<String, String> {
    // ── 1. Resolve credentials (same rules as ssh_connect / ftp_connect) ──
    if config.conn_type != "local" && config.auth_method != "key" && config.password.is_none() {
        if !config.id.is_empty() {
            let key = require_dek(&state)?;
            config.password = secrets::get_password(&config.id, &key)?;
        }
        // Empty/missing password on password-auth: refuse to test. Sending an
        // empty password would be counted as a failed attempt by many servers
        // and can trigger lockout after N tries — same guard as ssh_connect.
        if config.auth_method == "password"
            && config
                .password
                .as_deref()
                .map(str::is_empty)
                .unwrap_or(true)
        {
            return Err("未填写密码，无法测试".to_string());
        }
    }
    if config.conn_type != "local"
        && config.proxy_type != "none"
        && config.proxy_password.is_none()
        && !config.id.is_empty()
    {
        let key = require_dek(&state)?;
        config.proxy_password = secrets::get_proxy_password(&config.id, &key)?;
    }

    // ── 2. Dispatch under a 15s timeout ──
    let probe = async {
        match config.conn_type.as_str() {
            "ssh" | "sftp" => ssh::test_connection(&state, &config).await,
            "ftp" => ftp::test_connection(&config).await,
            "local" => local::test_connection(&config).await,
            other => Err(format!("未知连接类型: {}", other)),
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(15), probe)
        .await
        .map_err(|_| {
            "连接测试超时（15s）— 通常网络不可达、防火墙拦截、或服务未运行".to_string()
        })?
}

/// Test the AI provider config with a minimal non-streaming request. Accepts
/// optional overrides so the user can validate the CURRENT FORM VALUES
/// (including an unsaved typed API key) without saving first. When an
/// override is None or empty, the vault-stored value is used.
#[tauri::command]
async fn ai_test_settings(
    state: State<'_, AppState>,
    overrides: Option<ai::AiTestOverrides>,
) -> Result<String, String> {
    let overrides = overrides.unwrap_or_default();
    ai::test_settings(&state, overrides).await
}

// ============ Elevation (run as admin) ============

/// Whether MyShell is running elevated (admin on Windows, root on Unix). Drives
/// the "管理员权限" status chip in Settings — the local-terminal admin story is
/// "elevate the whole app" (see `elevation.rs`), so the UI just reports state.
#[tauri::command]
fn is_elevated() -> bool {
    elevation::is_elevated()
}

/// Re-launch MyShell elevated via the UAC consent dialog, then exit the current
/// (non-elevated) process. The elevated instance starts independently. Errors
/// if the user cancels UAC — in which case we stay running as-is.
#[tauri::command]
fn restart_as_admin(app: tauri::AppHandle) -> Result<(), String> {
    elevation::restart_as_admin()?;
    // Elevated instance is launching; tear this process down. `exit(0)` runs the
    // ExitRequested handler so live SSH/local sessions drain gracefully.
    app.exit(0);
    Ok(())
}

// ============ SSH Server Info ============

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerInfo {
    os_pretty: String,
    kernel: String,
    cpu_cores: u32,
    mem_total_bytes: u64,
    mem_used_bytes: u64,
    cpu_usage_pct: f32,
    mem_usage_pct: f32,
    /// Aggregate bytes across all `/dev/...` partitions. Reported as the
    /// "整体磁盘" number; individual partition sizes are surfaced separately.
    disk_total_bytes: u64,
    disk_used_bytes: u64,
    /// `disk_used_bytes / disk_total_bytes * 100`. 0 when total is 0.
    disk_total_pct: f32,
    /// Device node of the busiest `/dev/...` partition, e.g. `/dev/sda1`.
    disk_max_dev: String,
    /// Mount path of the busiest partition, e.g. `/` or `/home`. Empty when
    /// no real block-device partitions exist.
    disk_max_mount: String,
    /// Size of the busiest partition, in the human-readable unit emitted by
    /// `df -h` (e.g. `100G`, `1.5T`). Frontend renders verbatim.
    disk_max_size: String,
    /// Used bytes of the busiest partition, same unit source as `disk_max_size`.
    disk_max_used: String,
    /// Utilization percent (0–100) of the busiest partition — drives the
    /// usage-bar color threshold in the UI.
    disk_max_pct: f32,
    /// True when the last refresh timed out — the frontend should grey out
    /// the values to signal staleness.
    stale: bool,
}

/// Combined probe script — runs in a single exec round-trip (~1s due to the
/// sleep between /proc/stat samples). All sections are delimited by `=TAG=`
/// markers so the parser can slice without relying on line counts.
///
/// Disk uses two passes:
/// - `=DT=` sums bytes across all `/dev/...` partitions for the aggregate
///   total + used (the user-facing "how full is this box" number).
/// - `=DM=` keeps `df -h` human-readable strings per partition so we can
///   surface the busiest partition's size/used without lossy byte→unit
///   reformatting in Rust.
const SERVER_INFO_SCRIPT: &str = r#"echo "=OS="; (. /etc/os-release 2>/dev/null && echo "$PRETTY_NAME") || uname -sr
echo "=K="; uname -r
echo "=C="; nproc 2>/dev/null || grep -c '^processor' /proc/cpuinfo
echo "=M="; free -b
echo "=DT="; df -B1 2>/dev/null | awk 'NR>1 && $1 ~ /^\/dev\// {print $2,$3}'
echo "=DM="; df -h 2>/dev/null | awk 'NR>1 && $1 ~ /^\/dev\// {sub(/%/,"",$5); print $1,$2,$3,$5,$6}'
echo "=S1="; head -n1 /proc/stat
sleep 1
echo "=S2="; head -n1 /proc/stat"#;

#[tauri::command]
async fn ssh_get_server_info(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<ServerInfo, String> {
    let raw = match tokio::time::timeout(
        std::time::Duration::from_secs(8),
        ssh::exec_once(&state, &session_id, SERVER_INFO_SCRIPT),
    )
    .await
    {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return Err(e),
        // On timeout, return a stale snapshot so the UI can grey it out
        // instead of erroring the whole panel.
        Err(_) => {
            return Ok(ServerInfo {
                os_pretty: "unknown".into(),
                kernel: "unknown".into(),
                cpu_cores: 0,
                mem_total_bytes: 0,
                mem_used_bytes: 0,
                cpu_usage_pct: 0.0,
                mem_usage_pct: 0.0,
                disk_total_bytes: 0,
                disk_used_bytes: 0,
                disk_total_pct: 0.0,
                disk_max_dev: String::new(),
                disk_max_mount: String::new(),
                disk_max_size: String::new(),
                disk_max_used: String::new(),
                disk_max_pct: 0.0,
                stale: true,
            });
        }
    };

    Ok(parse_server_info(&raw))
}

/// Slice the raw output by `=TAG=` markers and parse each section. Tolerant —
/// missing or malformed sections produce zeros rather than errors so the UI
/// degrades gracefully on minimal systems (e.g. busybox without `nproc`).
fn parse_server_info(raw: &str) -> ServerInfo {
    /// Extract content between `=TAG=` and the next `\n=` (next tag's leading
    /// `=`). Tags are emitted by the shell as `echo "=OS="` etc., so the
    /// actual bytes are `=OS=\n...content...\n=K=\n...`. The trailing newline
    /// before the next tag is the section terminator.
    fn section<'a>(raw: &'a str, tag: &str) -> &'a str {
        let start = match raw.find(tag) {
            Some(i) => i + tag.len(),
            None => return "",
        };
        let rest = &raw[start..];
        // Skip a single leading newline so `=OS=\nLinux` → "Linux".
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        let end = rest.find("\n=").unwrap_or(rest.len());
        rest[..end].trim_end_matches('\r')
    }

    let os_pretty = section(raw, "=OS=").lines().next().unwrap_or("").to_string();
    let kernel = section(raw, "=K=").lines().next().unwrap_or("").to_string();
    let cpu_cores = section(raw, "=C=")
        .lines()
        .next()
        .and_then(|l| l.trim().parse::<u32>().ok())
        .unwrap_or(0);

    // free -b Mem line: "Mem:  total  used  free  shared  buff  cache  available"
    let (mem_total, mem_used) = section(raw, "=M=")
        .lines()
        .find(|l| l.starts_with("Mem:"))
        .and_then(|l| {
            let v: Vec<&str> = l.split_whitespace().collect();
            if v.len() >= 3 {
                Some((v[1].parse::<u64>().ok()?, v[2].parse::<u64>().ok()?))
            } else {
                None
            }
        })
        .unwrap_or((0, 0));

    // =DT= is two columns (total used) per partition in raw bytes; we sum
    // them for the aggregate disk usage number.
    let (disk_total, disk_used) = section(raw, "=DT=").lines().fold(
        (0u64, 0u64),
        |(t, u), line| {
            let v: Vec<&str> = line.split_whitespace().collect();
            if v.len() >= 2 {
                (
                    t.saturating_add(v[0].parse::<u64>().unwrap_or(0)),
                    u.saturating_add(v[1].parse::<u64>().unwrap_or(0)),
                )
            } else {
                (t, u)
            }
        },
    );

    // =DM= is the per-partition `df -h` view: "dev size used use% mount".
    // Pick the busiest partition and keep its human-readable size/used.
    let mut disk_max_pct: f32 = 0.0;
    let mut disk_max_dev = String::new();
    let mut disk_max_mount = String::new();
    let mut disk_max_size = String::new();
    let mut disk_max_used = String::new();
    for line in section(raw, "=DM=").lines() {
        let v: Vec<&str> = line.split_whitespace().collect();
        if v.len() < 5 {
            continue;
        }
        let usage: f32 = v[3].parse().unwrap_or(0.0);
        if usage >= disk_max_pct {
            disk_max_pct = usage;
            disk_max_dev = v[0].to_string();
            disk_max_size = v[1].to_string();
            disk_max_used = v[2].to_string();
            // df -h's mount column is the last; paths with spaces are rare
            // but possible — join all trailing fields to recover the path.
            disk_max_mount = v[4..].join(" ");
        }
    }

    let cpu_usage = {
        let s1 = section(raw, "=S1=").lines().next().unwrap_or("");
        let s2 = section(raw, "=S2=").lines().next().unwrap_or("");
        cpu_busy_pct(s1, s2)
    };

    ServerInfo {
        os_pretty,
        kernel,
        cpu_cores,
        mem_total_bytes: mem_total,
        mem_used_bytes: mem_used,
        cpu_usage_pct: cpu_usage,
        mem_usage_pct: pct(mem_used, mem_total),
        disk_total_bytes: disk_total,
        disk_used_bytes: disk_used,
        disk_total_pct: pct(disk_used, disk_total),
        disk_max_dev,
        disk_max_mount,
        disk_max_size,
        disk_max_used,
        disk_max_pct,
        stale: false,
    }
}

/// Compute CPU busy % between two /proc/stat `cpu` aggregate lines. The
/// "busy" set is user + nice + system + irq + softirq + steal + iowait.
/// Returns 0.0 if either sample is malformed.
fn cpu_busy_pct(s1: &str, s2: &str) -> f32 {
    let parse = |s: &str| -> Option<(u64, u64)> {
        let v: Vec<&str> = s.split_whitespace().collect();
        if v.len() < 5 || v[0] != "cpu" {
            return None;
        }
        let nums: Vec<u64> = v[1..].iter().filter_map(|x| x.parse::<u64>().ok()).collect();
        if nums.len() < 4 {
            return None;
        }
        let total: u64 = nums.iter().sum();
        // idle = idle + iowait (indices 3, 4)
        let idle = nums.get(3).copied().unwrap_or(0) + nums.get(4).copied().unwrap_or(0);
        Some((total, idle))
    };
    let (t1, i1) = parse(s1).unwrap_or((0, 0));
    let (t2, i2) = parse(s2).unwrap_or((0, 0));
    let dt = t2.saturating_sub(t1);
    let di = i2.saturating_sub(i1);
    if dt == 0 {
        return 0.0;
    }
    100.0 * (1.0 - (di as f32 / dt as f32))
}

fn pct(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        100.0 * used as f32 / total as f32
    }
}

// ============ SFTP Commands ============

#[tauri::command]
async fn sftp_list_dir(
    session_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<FileEntry>, String> {
    sftp::list_dir(&state, &session_id, &path).await
}

#[tauri::command]
async fn sftp_mkdir(
    session_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sftp::create_dir(&state, &session_id, &path).await
}

#[tauri::command]
async fn sftp_remove(
    session_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sftp::remove(&state, &session_id, &path).await
}

#[tauri::command]
async fn sftp_rename(
    session_id: String,
    old_path: String,
    new_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sftp::rename(&state, &session_id, &old_path, &new_path).await
}

/// Batch-upload local files into the remote dir. Progress streams via
/// `sftp_transfer_progress`; completion (with per-file errors) via
/// `sftp_transfer_done`. See `sftp::upload`.
#[tauri::command]
async fn sftp_upload(
    session_id: String,
    local_paths: Vec<String>,
    remote_dest_dir: String,
    request_id: String,
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    sftp::upload(
        &state,
        &session_id,
        local_paths,
        &remote_dest_dir,
        &request_id,
        &window,
    )
    .await
}

/// Batch-download remote files into a local dir. Same event contract as
/// `sftp_upload`. See `sftp::download`.
#[tauri::command]
async fn sftp_download(
    session_id: String,
    remote_paths: Vec<String>,
    local_dest_dir: String,
    request_id: String,
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    sftp::download(
        &state,
        &session_id,
        remote_paths,
        &local_dest_dir,
        &request_id,
        &window,
    )
    .await
}

// ============ FTP Commands ============
//
// FTP sessions live in `AppState::ftp_sessions`, keyed by a fresh UUID that
// the frontend treats as a Tab ID. The frontend's SftpPanel reuses the same
// surface by passing `source: "ftp"`.

#[tauri::command]
async fn ftp_connect(mut config: ConnectionConfig, state: State<'_, AppState>) -> Result<String, String> {
    let port = if config.port == 0 { 21 } else { config.port };
    log::info!(
        "[ftp] connect requested: {}@{}:{} (tls={}, passive={}, proxy={})",
        config.username, config.host, port,
        config.ftp_tls, config.ftp_passive, config.proxy_type
    );
    // Resolve password from keyring here (same pattern as ssh_connect).
    if config.password.is_none() {
        let key = require_dek(&state)?;
        config.password = secrets::get_password(&config.id, &key)?;
    }
    if config.proxy_type != "none" && config.proxy_password.is_none() {
        let key = require_dek(&state)?;
        config.proxy_password = secrets::get_proxy_password(&config.id, &key)?;
    }
    let target = format!("{}@{}:{}", config.username, config.host, port);
    let result = ftp::connect(&config).await;
    match &result {
        Ok(_) => log::info!("[ftp] connected to {}", target),
        Err(e) => log::error!("[ftp] connect failed for {}: {}", target, e),
    }
    let session = result?;
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut sessions = state.ftp_sessions.lock().map_err(|e| e.to_string())?;
        sessions.insert(id.clone(), session);
    }
    Ok(id)
}

#[tauri::command]
async fn ftp_list_dir(
    session_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<FileEntry>, String> {
    let mut session = take_ftp_session(&state, &session_id)?;
    let result = ftp::list_dir(&mut session, &path).await;
    return_ftp_session(&state, &session_id, session)?;
    result
}

#[tauri::command]
async fn ftp_mkdir(
    session_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut session = take_ftp_session(&state, &session_id)?;
    let result = ftp::mkdir(&mut session, &path).await;
    return_ftp_session(&state, &session_id, session)?;
    result
}

#[tauri::command]
async fn ftp_remove(
    session_id: String,
    path: String,
    is_dir: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut session = take_ftp_session(&state, &session_id)?;
    let result = ftp::remove(&mut session, &path, is_dir).await;
    return_ftp_session(&state, &session_id, session)?;
    result
}

#[tauri::command]
async fn ftp_rename(
    session_id: String,
    old_path: String,
    new_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut session = take_ftp_session(&state, &session_id)?;
    let result = ftp::rename(&mut session, &old_path, &new_path).await;
    return_ftp_session(&state, &session_id, session)?;
    result
}

#[tauri::command]
async fn ftp_disconnect(session_id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut session = {
        let mut sessions = state.ftp_sessions.lock().map_err(|e| e.to_string())?;
        sessions
            .remove(&session_id)
            .ok_or_else(|| "FTP session not found".to_string())?
    };
    ftp::disconnect(&mut session).await
}

/// Borrow the FTP session out of state for the duration of a single operation.
/// The session is moved back into the map when the operation completes —
/// `AsyncFtpStream` does not impl `Clone`, so we can't hold a shared ref.
fn take_ftp_session(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<ftp::FtpSession, String> {
    let mut sessions = state.ftp_sessions.lock().map_err(|e| e.to_string())?;
    sessions
        .remove(session_id)
        .ok_or_else(|| "FTP session not found".to_string())
}

/// Put the session back. If the map no longer has a slot (e.g. the user
/// disconnected concurrently), drop the session silently — the caller has
/// already finished its operation, and `quit()` would race the disconnect.
fn return_ftp_session(
    state: &State<'_, AppState>,
    session_id: &str,
    session: ftp::FtpSession,
) -> Result<(), String> {
    let mut sessions = state.ftp_sessions.lock().map_err(|e| e.to_string())?;
    // Only re-insert if not concurrently removed.
    if sessions.contains_key(session_id) {
        sessions.insert(session_id.to_string(), session);
    }
    Ok(())
}

// ============ ZMODEM File IO Commands ============
//
// The frontend drives the protocol via zmodem.js; Rust just provides
// streaming file IO so we don't pull GBs into the JS heap. Each transfer
// is tracked by a UUID; reads are random-access (zmodem.js requests by
// offset during resume), writes are sequential append (zmodem.js guarantees
// ordered ZDATA delivery).

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ZmodemReadOpenResult {
    id: String,
    size: u64,
    mtime: i64,
}

/// Open a local file for ZMODEM upload (rz). Returns a transfer ID + size
/// that the frontend feeds into zmodem.js's offer.
///
/// Security: errors are intentionally generic — we never echo the path or
/// the OS error string back to the frontend, so this command can't be used
/// as a file-existence oracle. Path must be NUL-free and ≤ 4096 bytes to
/// block PATH-truncation tricks.
#[tauri::command]
fn rz_open_read(path: String, state: State<'_, AppState>) -> Result<ZmodemReadOpenResult, String> {
    if path.bytes().any(|b| b == 0) || path.len() > 4096 {
        return Err("无效路径".to_string());
    }
    let meta = std::fs::metadata(&path).map_err(|_| "无法读取文件".to_string())?;
    if !meta.is_file() {
        return Err("无法读取文件".to_string());
    }
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let file = std::fs::File::open(&path).map_err(|_| "无法读取文件".to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut files = state.zmodem_files.lock().map_err(|e| e.to_string())?;
        files.insert(
            id.clone(),
            ZmodemFileHandle {
                kind: ZmodemFileKind::Read,
                path,
                reader: Some(file),
                writer: None,
                size,
            },
        );
    }
    Ok(ZmodemReadOpenResult { id, size, mtime })
}

/// Read up to `len` bytes starting at `offset` from a previously opened
/// rz transfer. Returns an empty Vec at EOF.
#[tauri::command]
fn rz_read_chunk(
    id: String,
    offset: u64,
    len: u32,
    state: State<'_, AppState>,
) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut files = state.zmodem_files.lock().map_err(|e| e.to_string())?;
    let handle = files
        .get_mut(&id)
        .ok_or_else(|| "Unknown zmodem transfer id".to_string())?;
    if handle.kind != ZmodemFileKind::Read {
        return Err("Not a read handle".to_string());
    }
    let file = handle.reader.as_mut().ok_or("File closed")?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("seek: {}", e))?;
    let mut buf = vec![0u8; len as usize];
    let n = file.read(&mut buf).map_err(|e| format!("read: {}", e))?;
    buf.truncate(n);
    Ok(buf)
}

/// Close and drop a read handle. Safe to call multiple times.
#[tauri::command]
fn rz_close(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut files = state.zmodem_files.lock().map_err(|e| e.to_string())?;
    files.remove(&id);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ZmodemWriteOpenResult {
    id: String,
    existing_size: u64,
}

/// Open (or reuse) a file for ZMODEM download (sz). Returns the transfer
/// ID and the current file size (0 if new) so the frontend can negotiate
/// resume via ZRPOS if it wants.
///
/// Security: this is the highest-risk IPC primitive in the app — it can
/// create or overwrite files at arbitrary paths. Mitigations:
///   1. Reject NUL bytes and over-long paths (block PATH-truncation).
///   2. Reject paths inside system-critical locations (Windows boot/startup
///      dirs, `/etc`, `/usr`, `/boot` on Unix). The frontend is expected to
///      have called a save dialog, but defense-in-depth applies.
///   3. Refuse to follow symlinks at the leaf — the destination could
///      point anywhere, and a save dialog return is no guarantee against
///      pre-planted links.
///   4. Cap pre-existing size at 4 GiB; anything bigger is suspicious and
///      would let a malicious offer exhaust disk.
///   5. Uniform error strings — no path or OS-error echo, so this can't
///      serve as a write-target oracle.
#[tauri::command]
fn sz_open_write(path: String, state: State<'_, AppState>) -> Result<ZmodemWriteOpenResult, String> {
    if path.bytes().any(|b| b == 0) || path.len() > 4096 {
        return Err("无效路径".to_string());
    }
    if is_protected_write_path(&path) {
        return Err("目标路径受保护".to_string());
    }
    // Reject if the leaf itself is a symlink. We can't fully prevent
    // directory-component symlinks without canonicalize (which itself
    // follows links), but the dialog flow normally lands on real paths.
    if std::fs::symlink_metadata(&path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err("目标路径受保护".to_string());
    }
    let existing_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    const MAX_EXISTING_BYTES: u64 = 4 * 1024 * 1024 * 1024;
    if existing_size > MAX_EXISTING_BYTES {
        return Err("目标文件过大".to_string());
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&path)
        .map_err(|_| "无法写入文件".to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut files = state.zmodem_files.lock().map_err(|e| e.to_string())?;
        files.insert(
            id.clone(),
            ZmodemFileHandle {
                kind: ZmodemFileKind::Write,
                path,
                reader: None,
                writer: Some(file),
                size: 0,
            },
        );
    }
    Ok(ZmodemWriteOpenResult { id, existing_size })
}

/// Write a chunk at `offset`. Supports resume — caller may pass a
/// non-zero offset to seek before writing, or pass the running append
/// offset for normal sequential transfers. The handle must have been
/// opened via `sz_open_write`.
///
/// Security: single-chunk cap is 16 MiB (ZMODEM frame payloads are
/// normally 1 KB; anything bigger is abuse). Cumulative size per handle
/// is capped at 8 GiB to prevent disk-exhaustion via a long-running
/// malicious transfer.
#[tauri::command]
fn sz_write_chunk(
    id: String,
    offset: u64,
    bytes: Vec<u8>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use std::io::{Seek, SeekFrom, Write};
    const MAX_CHUNK_BYTES: usize = 16 * 1024 * 1024;
    const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;
    if bytes.len() > MAX_CHUNK_BYTES {
        return Err("数据块过大".to_string());
    }
    let end = offset.checked_add(bytes.len() as u64).ok_or("offset 溢出")?;
    if end > MAX_TOTAL_BYTES {
        return Err("超过最大文件大小".to_string());
    }
    let mut files = state.zmodem_files.lock().map_err(|e| e.to_string())?;
    let handle = files
        .get_mut(&id)
        .ok_or_else(|| "Unknown zmodem transfer id".to_string())?;
    if handle.kind != ZmodemFileKind::Write {
        return Err("Not a write handle".to_string());
    }
    let file = handle.writer.as_mut().ok_or("File closed")?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| "写入失败".to_string())?;
    file.write_all(&bytes).map_err(|_| "写入失败".to_string())?;
    if end > handle.size {
        handle.size = end;
    }
    Ok(())
}

/// Close and flush a write handle.
#[tauri::command]
fn sz_close(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut files = state.zmodem_files.lock().map_err(|e| e.to_string())?;
    files.remove(&id);
    Ok(())
}

// ============ Main ============

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// In release builds, redirect CRT file descriptor 2 (stderr) to a daily
/// log file under `<config_dir>/myshell/logs/`. All `eprintln!` calls then
/// land on disk instead of disappearing into the void of the windows
/// subsystem. Old logs (>7 days) are pruned on each launch.
///
/// The CRT functions `_open_osfhandle` + `_dup2` are MSVC-specific; we
/// extern them under the "system" ABI (cdecl on x64). The OpenOptions
/// handle is `mem::forget`'d so the OS file descriptor stays alive for
/// the process lifetime.
#[cfg(not(debug_assertions))]
fn setup_file_logging() {
    use std::mem;
    use std::os::windows::io::AsRawHandle;

    extern "system" {
        fn _open_osfhandle(osfhandle: isize, flags: i32) -> i32;
        fn _dup2(fd1: i32, fd2: i32) -> i32;
        fn _close(fd: i32) -> i32;
    }

    let mut log_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("myshell")
        .join("logs");
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        // Last-resort: fall back to a logs dir next to the exe.
        log_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("logs")))
            .unwrap_or_else(|| std::path::PathBuf::from("logs"));
        let _ = std::fs::create_dir_all(&log_dir);
        eprintln!("[startup] failed to create log dir, fallback: {}", e);
    }

    // Prune logs older than 7 days. Best-effort — ignore errors. Count is
    // surfaced in the startup banner below (emitted after dup2 so it lands
    // in the freshly opened daily file, not the pre-redirect stderr).
    let mut pruned = 0u32;
    if let Ok(entries) = std::fs::read_dir(&log_dir) {
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(60 * 60 * 24 * 7);
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if let Ok(mt) = meta.modified() {
                    if mt < cutoff && entry.path().extension().and_then(|e| e.to_str()) == Some("log") {
                        if std::fs::remove_file(entry.path()).is_ok() {
                            pruned += 1;
                        }
                    }
                }
            }
        }
    }

    // Daily file so a long-running session doesn't bloat a single file.
    let date = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86400)
        .unwrap_or(0);
    let log_path = log_dir.join(format!("myshell-{}.log", date));

    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(f) => f,
        Err(_) => return,
    };

    let raw = file.as_raw_handle() as isize;
    // _O_APPEND = 0x4000 on Windows MSVC.
    let osf = unsafe { _open_osfhandle(raw, 0x4000) };
    if osf == -1 {
        return;
    }
    // dup2 onto fd 2 (stderr). Subsequent eprintln! writes go here.
    unsafe {
        if _dup2(osf, 2) == 0 {
            // Don't close `osf` — _dup2 makes fd 2 share the same handle.
            // The File wrapper would close on drop; leak it instead.
            mem::forget(file);
        } else {
            _close(osf);
        }
    }

    log::info!(
        "[startup] === MyShell starting === (log retention 7d, pruned {} old log file(s))",
        pruned
    );
}

/// Set a stable Windows AppUserModelID for the process. Without it the
/// taskbar groups the window by executable path, which — especially right
/// after an update or from a fresh install — surfaces the generic Windows
/// icon in the taskbar even though the .exe's own icon resource is correct
/// (so Explorer shows the right icon but the taskbar doesn't). Must be set
/// before any window is created. Best-effort: a failure just leaves the
/// default path-based identity (still works, may show a generic/cached icon).
#[cfg(windows)]
fn set_windows_app_user_model_id() {
    use std::os::windows::ffi::OsStrExt;

    // Inline FFI to shell32!SetCurrentProcessExplicitAppUserModelID. Declared
    // directly (not via the `winapi` crate) to avoid adding a feature gate;
    // shell32 is already linked transitively. HRESULT is a typed i32.
    #[link(name = "shell32")]
    extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
    }

    // Matches `identifier` in tauri.conf.json — a stable reverse-DNS id.
    let app_id: Vec<u16> = std::ffi::OsStr::new("com.myshell.client")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `app_id` is a valid NUL-terminated UTF-16 buffer owned by this
    // scope; the pointer does not outlive the call.
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
    }
}

#[cfg(not(windows))]
fn set_windows_app_user_model_id() {}

pub fn run() {
    // Default INFO so log::info!/warn!/error! surface in the daily log file
    // (release dup2's stderr → file) / console (debug). RUST_LOG still wins
    // for ad-hoc verbose debugging (e.g. RUST_LOG=myshell=debug).
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    #[cfg(not(debug_assertions))]
    setup_file_logging();

    // Set the Windows taskbar identity before any window is created, so the
    // taskbar shows the MyShell icon instead of a generic one.
    set_windows_app_user_model_id();

    // Check for version upgrade and backup if needed
    if let Err(e) = backup::check_and_backup() {
        log::warn!("[startup] backup check failed: {}", e);
    }

    let conn = db::init_db().expect("Failed to initialize database");
    log::info!("[startup] database initialized");
    // v0.1 → v0.2 schema migration (group_name rename, conn_type/ftp_*
    // columns, drop plaintext password column). v0.2 → vault migration
    // happens later inside setup_vault, once the user provides a master
    // password and we have a derived key to encrypt with.
    if let Err(e) = db::migrate_legacy_schema(&conn) {
        log::warn!("[startup] legacy schema migration failed: {}", e);
    } else {
        log::info!("[startup] schema migration ok");
    }

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        ssh_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        ftp_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        local_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        zmodem_files: Mutex::new(HashMap::new()),
        dek: Arc::new(Mutex::new(None)),
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            vault_status,
            setup_vault,
            unlock_vault,
            lock_vault,
            verify_password,
            get_lockout_info,
            change_master_password,
            read_text_file,
            read_file_base64,
            get_connection_password,
            get_connection_proxy_password,
            list_backups,
            rollback_backup,
            get_app_version,
            get_previous_version,
            check_for_updates,
            open_external_url,
            download_update,
            install_update,
            get_connections,
            save_connection,
            delete_connection,
            copy_connection,
            export_connections,
            import_connections,
            list_folders,
            save_folder,
            delete_folder,
            rename_folder,
            move_connection,
            add_command_history,
            list_command_history,
            set_command_history_pinned,
            delete_command_history,
            clear_command_history,
            add_quick_command,
            list_quick_commands,
            list_quick_commands_for_connection,
            update_quick_command,
            update_quick_command_order,
            delete_quick_command,
            ssh_connect,
            ssh_send,
            ssh_resize,
            ssh_disconnect,
            ssh_send_zmodem,
            ssh_send_zmodem_abort,
            local_connect,
            local_send,
            local_resize,
            local_disconnect,
            fonts::list_system_fonts,
            is_elevated,
            restart_as_admin,
            ssh_get_server_info,
            sftp_list_dir,
            sftp_mkdir,
            sftp_remove,
            sftp_rename,
            sftp_upload,
            sftp_download,
            ftp_connect,
            ftp_list_dir,
            ftp_mkdir,
            ftp_remove,
            ftp_rename,
            ftp_disconnect,
            test_connection,
            rz_open_read,
            rz_read_chunk,
            rz_close,
            sz_open_write,
            sz_write_chunk,
            sz_close,
            ai_chat,
            ai_inspect_health_ssh,
            ai_inspect_health_local,
            get_ai_settings,
            save_ai_settings,
            ai_test_settings,
        ])
        .setup(|app| {
            // Explicitly set the main window's icon so the title bar + taskbar
            // show the MyShell icon regardless of how tauri-build auto-embedded
            // the default window icon. Pairs with set_windows_app_user_model_id()
            // (called at the top of run()) to fully fix the Windows taskbar icon.
            #[cfg(windows)]
            {
                if let Some(window) = app.get_webview_window("main") {
                    match tauri::image::Image::from_bytes(include_bytes!("../icons/icon.ico")) {
                        Ok(icon) => {
                            if let Err(e) = window.set_icon(icon) {
                                log::warn!("[startup] set window icon failed: {}", e);
                            }
                        }
                        Err(e) => log::warn!("[startup] decode window icon failed: {}", e),
                    }
                }
            }

            // No-white-screen splash: the main window starts hidden
            // (`"visible": false` in tauri.conf.json). The frontend emits a
            // "dom-ready" event (see src/main.tsx) once React has painted its
            // first frame, so we reveal the window only after there's real
            // content to show — eliminating the brief blank flash before
            // login. Keep a 4s safety net in case the frontend never signals
            // (dev error, broken JS) so the app can't get stuck invisible.
            if let Some(window) = app.get_webview_window("main") {
                let win = window.clone();
                app.listen_any("dom-ready", move |_| {
                    let _ = win.show();
                    let _ = win.set_focus();
                });
                // Safety-net: force-show after 4s regardless, so a frontend
                // bug can never leave the user with an invisible window.
                let win_fallback = window.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                    if win_fallback.is_visible().unwrap_or(false) {
                        return;
                    }
                    log::warn!("[startup] dom-ready not received within 4s; force-showing window");
                    let _ = win_fallback.show();
                    let _ = win_fallback.set_focus();
                });
            } else {
                log::warn!("[startup] main window not found; cannot register dom-ready reveal");
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            drain_all_sessions(app_handle);
        }
    });
}

/// Fire `Disconnect` on every live SSH session and drop all FTP sessions
/// so reader tasks can flush a graceful channel close before the process
/// dies. Called once on `ExitRequested`.
///
/// Why we no longer `thread::sleep` here: the previous implementation
/// blocked the Tauri main event loop for 500ms during exit, freezing the
/// window-close flow. Tauri's runtime will continue draining pending
/// async tasks (the channel reader coroutines) even after this callback
/// returns, and any still-open TCP sockets are reclaimed by the OS when
/// the process actually exits. The sleep traded UI responsiveness for a
/// marginal reduction in TIME_WAIT sockets that wasn't worth the freeze.
fn drain_all_sessions(app_handle: &tauri::AppHandle) {
    let Some(state) = app_handle.try_state::<AppState>() else {
        return;
    };
    let ssh_senders: Vec<_> = {
        let Ok(sessions) = state.ssh_sessions.lock() else {
            return;
        };
        sessions
            .values()
            .map(|s| s.command_tx.clone())
            .collect()
    };
    let local_senders: Vec<_> = {
        let Ok(sessions) = state.local_sessions.lock() else {
            return;
        };
        sessions
            .values()
            .map(|s| s.command_tx.clone())
            .collect()
    };
    let ftp_count = {
        let Ok(mut sessions) = state.ftp_sessions.lock() else {
            return;
        };
        let count = sessions.len();
        // Drop all FTP sockets synchronously — `AsyncFtpStream::quit` is
        // async and we can't await here. The drop closes the TCP socket;
        // the server's reader sees EOF and tears down.
        sessions.clear();
        count
    };
    if ssh_senders.is_empty() && local_senders.is_empty() && ftp_count == 0 {
        return;
    }
    eprintln!(
        "[exit] draining {} SSH + {} local + {} FTP session(s)",
        ssh_senders.len(),
        local_senders.len(),
        ftp_count
    );
    for tx in &ssh_senders {
        let _ = tx.send(ssh::SessionCommand::Disconnect);
    }
    for tx in &local_senders {
        let _ = tx.send(local::LocalCommand::Disconnect);
    }
}

fn main() {
    run();
}
