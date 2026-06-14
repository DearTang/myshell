// In release builds, hide the console window. Debug builds keep it so
// eprintln! / env_logger output is visible during development. We move
// release-mode logs into a rotating file (`<config_dir>/myshell/logs/`)
// so users can still ship us diagnostics.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandHistoryItem {
    pub id: i64,
    pub command: String,
    pub pinned: bool,
    pub created_at: String,
}

// ============ App State ============

pub struct AppState {
    /// Arc-wrapped so per-session SshClient handlers can clone a reference
    /// for `check_server_key` lookups without borrowing State.
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub ssh_sessions: Arc<Mutex<std::collections::HashMap<String, ssh::SshSession>>>,
    pub ftp_sessions: Arc<Mutex<std::collections::HashMap<String, ftp::FtpSession>>>,
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
    // existence, so a missing credential is not a failure.
    let _ = secrets::delete_password(&id);
    let _ = secrets::delete_proxy_password(&id);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::delete_connection(&db, &id).map_err(|e| e.to_string())
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

    // Generate salt + derive master_key from passphrase
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

    // Persist salt + verifier + encrypted_dek
    vault::write_vault_files(&salt, &verifier)?;
    vault::write_encrypted_dek(&encrypted_dek)?;

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
    let encrypted_dek = vault::read_encrypted_dek().ok_or_else(|| "Vault 未初始化".to_string())?;

    // Derive master_key from passphrase and verify
    let master_key = crypto::derive_master_key(&passphrase, &salt);
    if !crypto::check_verifier(&master_key, &verifier) {
        // Record failed attempt
        return Err(lockout.record_failure());
    }

    // Decrypt DEK with master_key
    let dek_bytes = crypto::decrypt_with_key(&master_key, &encrypted_dek)?;
    let dek: [u8; 32] = dek_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "DEK 长度错误".to_string())?;

    // Success - reset failure counters
    lockout.record_success();

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

/// Verify login password (for viewing plaintext passwords in UI)
#[tauri::command]
fn verify_password(passphrase: String) -> Result<bool, String> {
    let salt = vault::read_salt().ok_or_else(|| "Vault 未初始化".to_string())?;
    let verifier = vault::read_verifier().ok_or_else(|| "Vault 未初始化".to_string())?;

    let master_key = crypto::derive_master_key(&passphrase, &salt);
    Ok(crypto::check_verifier(&master_key, &verifier))
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
/// 1. Verify old passphrase
/// 2. Re-encrypt DEK with new passphrase
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

    // Verify old passphrase
    let old_master_key = crypto::derive_master_key(&old_passphrase, &salt);
    if !crypto::check_verifier(&old_master_key, &verifier) {
        return Err("原密码错误".into());
    }

    // Decrypt DEK with old master_key
    let dek_bytes = crypto::decrypt_with_key(&old_master_key, &encrypted_dek)?;
    let dek: [u8; 32] = dek_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "DEK 长度错误".to_string())?;

    // Derive new master_key and re-encrypt DEK
    let new_master_key = crypto::derive_master_key(&new_passphrase, &salt);
    let new_encrypted_dek = crypto::encrypt_with_key(&new_master_key, &dek)?;
    let new_verifier = crypto::make_verifier(&new_master_key)?;

    // Persist
    vault::write_encrypted_dek(&new_encrypted_dek)?;
    vault::write_vault_files(&salt, &new_verifier)?;

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

/// Read a text file (PEM, etc.) at an absolute path. Used by the private-
/// key picker in ConnectionDialog: the user picks a path via the dialog
/// plugin, and this reads the content so we can stash it (encrypted) in
/// the vault rather than keeping a reference to a plaintext file on disk.
#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("读取 {} 失败: {}", path, e))
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

// ============ Command History Commands ============

#[tauri::command]
fn add_command_history(
    state: State<AppState>,
    connection_id: String,
    command: String,
) -> Result<i64, String> {
    if command.trim().is_empty() {
        return Ok(0);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::add_command_history(&db, &connection_id, &command, &now).map_err(|e| e.to_string())
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

// ============ SSH Commands ============

#[tauri::command]
async fn ssh_connect(
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    mut config: ConnectionConfig,
) -> Result<String, String> {
    // Resolve password from keyring here, not in ssh.rs, so the SSH module
    // doesn't need to know about the vault. Key auth uses private_key_pem
    // which is already in the config (populated by get_connections).
    if config.auth_method != "key" && config.password.is_none() {
        let key = require_dek(&state)?;
        config.password = secrets::get_password(&config.id, &key)?;
    }
    // Same for proxy password: pull from keyring so ssh.rs sees a complete
    // proxy config and can hand it to proxy.rs without knowing about the
    // vault.
    if config.proxy_type != "none" && config.proxy_password.is_none() {
        let key = require_dek(&state)?;
        config.proxy_password = secrets::get_proxy_password(&config.id, &key)?;
    }
    ssh::connect(state, window, config).await
}

#[tauri::command]
async fn ssh_send(state: State<'_, AppState>, session_id: String, data: String) -> Result<(), String> {
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
    ssh::disconnect(&state, &session_id).await
}

#[tauri::command]
async fn ssh_send_zmodem(
    state: State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    ssh::zmodem_send_bytes(&state, &session_id, &data).await
}

#[tauri::command]
async fn ssh_send_zmodem_abort(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    ssh::zmodem_abort(&state, &session_id).await
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

// ============ FTP Commands ============
//
// FTP sessions live in `AppState::ftp_sessions`, keyed by a fresh UUID that
// the frontend treats as a Tab ID. The frontend's SftpPanel reuses the same
// surface by passing `source: "ftp"`.

#[tauri::command]
async fn ftp_connect(mut config: ConnectionConfig, state: State<'_, AppState>) -> Result<String, String> {
    // Resolve password from keyring here (same pattern as ssh_connect).
    if config.password.is_none() {
        let key = require_dek(&state)?;
        config.password = secrets::get_password(&config.id, &key)?;
    }
    if config.proxy_type != "none" && config.proxy_password.is_none() {
        let key = require_dek(&state)?;
        config.proxy_password = secrets::get_proxy_password(&config.id, &key)?;
    }
    let session = ftp::connect(&config).await?;
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
#[tauri::command]
fn rz_open_read(path: String, state: State<'_, AppState>) -> Result<ZmodemReadOpenResult, String> {
    let meta = std::fs::metadata(&path).map_err(|e| format!("stat {}: {}", path, e))?;
    if !meta.is_file() {
        return Err(format!("Not a regular file: {}", path));
    }
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let file = std::fs::File::open(&path).map_err(|e| format!("open {}: {}", path, e))?;
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
#[tauri::command]
fn sz_open_write(path: String, state: State<'_, AppState>) -> Result<ZmodemWriteOpenResult, String> {
    let existing_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("open {}: {}", path, e))?;
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
#[tauri::command]
fn sz_write_chunk(
    id: String,
    offset: u64,
    bytes: Vec<u8>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use std::io::{Seek, SeekFrom, Write};
    let mut files = state.zmodem_files.lock().map_err(|e| e.to_string())?;
    let handle = files
        .get_mut(&id)
        .ok_or_else(|| "Unknown zmodem transfer id".to_string())?;
    if handle.kind != ZmodemFileKind::Write {
        return Err("Not a write handle".to_string());
    }
    let file = handle.writer.as_mut().ok_or("File closed")?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("seek: {}", e))?;
    file.write_all(&bytes).map_err(|e| format!("write: {}", e))?;
    let end = offset + bytes.len() as u64;
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
/// subsystem. Old logs (>14 days) are pruned on each launch.
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

    // Prune logs older than 14 days. Best-effort — ignore errors.
    if let Ok(entries) = std::fs::read_dir(&log_dir) {
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(60 * 60 * 24 * 14);
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if let Ok(mt) = meta.modified() {
                    if mt < cutoff && entry.path().extension().and_then(|e| e.to_str()) == Some("log") {
                        let _ = std::fs::remove_file(entry.path());
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

    eprintln!(
        "[startup] === MyShell starting at {} ===",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
}

pub fn run() {
    env_logger::init();
    #[cfg(not(debug_assertions))]
    setup_file_logging();

    // Check for version upgrade and backup if needed
    if let Err(e) = backup::check_and_backup() {
        eprintln!("[startup] backup check failed: {}", e);
    }

    let conn = db::init_db().expect("Failed to initialize database");
    // v0.1 → v0.2 schema migration (group_name rename, conn_type/ftp_*
    // columns, drop plaintext password column). v0.2 → vault migration
    // happens later inside setup_vault, once the user provides a master
    // password and we have a derived key to encrypt with.
    if let Err(e) = db::migrate_legacy_schema(&conn) {
        eprintln!("[startup] legacy schema migration failed: {}", e);
    }

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        ssh_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        ftp_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
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
            get_connection_password,
            get_connection_proxy_password,
            list_backups,
            rollback_backup,
            get_app_version,
            get_previous_version,
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
            add_command_history,
            list_command_history,
            set_command_history_pinned,
            delete_command_history,
            clear_command_history,
            ssh_connect,
            ssh_send,
            ssh_resize,
            ssh_disconnect,
            ssh_send_zmodem,
            ssh_send_zmodem_abort,
            ssh_get_server_info,
            sftp_list_dir,
            sftp_mkdir,
            sftp_remove,
            sftp_rename,
            ftp_connect,
            ftp_list_dir,
            ftp_mkdir,
            ftp_remove,
            ftp_rename,
            ftp_disconnect,
            rz_open_read,
            rz_read_chunk,
            rz_close,
            sz_open_write,
            sz_write_chunk,
            sz_close,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            drain_all_sessions(app_handle);
        }
    });
}

/// Fire `Disconnect` on every live SSH session and drop all FTP sessions,
/// then give them a brief grace period to close their TCP connections
/// cleanly. Without this, app close leaks open sockets — they sit in the
/// OS's TIME_WAIT until reaped. Called once on `ExitRequested`; safe to
/// call when no sessions exist.
fn drain_all_sessions(app_handle: &tauri::AppHandle) {
    let Some(state) = app_handle.try_state::<AppState>() else {
        return;
    };
    let senders: Vec<_> = {
        let Ok(sessions) = state.ssh_sessions.lock() else {
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
    if senders.is_empty() && ftp_count == 0 {
        return;
    }
    eprintln!(
        "[exit] draining {} SSH + {} FTP session(s)",
        senders.len(),
        ftp_count
    );
    for tx in &senders {
        let _ = tx.send(ssh::SessionCommand::Disconnect);
    }
    // Brief synchronous wait so the reader tasks can flush a graceful channel
    // close + EOF before the process dies. Tauri's exit is gated on this
    // callback returning, so the sleep is bounded and intentional.
    std::thread::sleep(std::time::Duration::from_millis(500));
}

fn main() {
    run();
}
