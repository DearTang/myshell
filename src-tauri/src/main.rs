// In release builds, hide the console window. Debug builds keep it so
// eprintln! / env_logger output is visible during development. We move
// release-mode logs into a rotating file (`<config_dir>/myshell/logs/`)
// so users can still ship us diagnostics.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use myshell_core::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Listener, Manager, State};
use rand::RngCore;

// ============ Tauri EventSink adapter ============

/// Bridges the core `EventSink` trait to Tauri's `WebviewWindow::emit`.
/// Constructed per-command-call where a core function needs to emit events
/// back to the frontend (ssh_output, sftp progress, ai tokens, etc.).
struct WindowSink(tauri::WebviewWindow);

impl EventSink for WindowSink {
    fn emit_raw(&self, event: &str, payload: serde_json::Value) {
        let _ = self.0.emit(event, payload);
    }
}

// ============ MCP exec_in_tab: pending request registry ============
//
// When the MCP server asks the GUI to run a command in a visible terminal tab,
// the IPC listener emits a "mcp-gui-command" event and then BLOCKS waiting for
// the frontend to call the `mcp_exec_result` Tauri command with the result.
// We use oneshot channels keyed by request_id to connect the waiting IPC
// thread to the Tauri command handler.
use tokio::sync::oneshot;
type PendingExecMap = std::sync::Mutex<std::collections::HashMap<String, oneshot::Sender<serde_json::Value>>>;
static PENDING_EXEC: std::sync::LazyLock<PendingExecMap> = std::sync::LazyLock::new(|| {
    std::sync::Mutex::new(std::collections::HashMap::new())
});

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
    // Soft-delete: the row is kept (stamped deleted_at) so it can be restored
    // from the recycle bin, and its keyring credentials are KEPT so a restore
    // works without re-entering the password. The db layer returns the ids of
    // any connections hard-purged to respect the 30-row cap — only those get a
    // keyring cleanup (they're gone for good).
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let purged_ids = db::delete_connection(&db, &id).map_err(|e| e.to_string())?;
    for pid in &purged_ids {
        if let Err(e) = secrets::delete_password(pid) {
            log::warn!("[delete_connection] keyring purge password failed for {}: {}", pid, e);
        }
        if let Err(e) = secrets::delete_proxy_password(pid) {
            log::warn!("[delete_connection] keyring purge proxy password failed for {}: {}", pid, e);
        }
    }
    log::info!(
        "[delete_connection] soft-deleted {} (purged {} overflow recycle rows)",
        id,
        purged_ids.len()
    );
    Ok(())
}

/// Forget the stored host key for (host, port) so the next connect re-runs
/// trust-on-first-use. Recovery path for a legitimate server-side host-key
/// change (OS reinstall / regenerated host keys) that the MITM defense would
/// otherwise reject forever as "Unknown server key". Scoped to host+port
/// exactly like the known_hosts store, so resetting one connection doesn't
/// touch another host's trust anchor.
#[tauri::command]
fn reset_known_host(state: State<AppState>, host: String, port: u16) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::delete_known_host(&db, &host, port).map_err(|e| e.to_string())?;
    log::info!(
        "[reset_known_host] cleared stored host key for {} (port {})",
        redact::host(&host),
        port
    );
    Ok(())
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

// require_dek is now in myshell_core (takes &AppState). Deref coercion
// from &State<AppState> → &AppState makes existing call sites work unchanged.

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

/// Gitee releases list endpoint for this repo. Returns a JSON array of all
/// releases (newest-first by creation time), each with `tag_name`, `assets[]`,
/// `body`, `created_at`, `html_url`, etc. Public, no auth needed (subject to
/// Gitee's unauthenticated rate limits).
///
/// We intentionally do NOT use `/releases/latest`: Gitee's "latest" marker is
/// unreliable for API-published releases — it kept pointing at v1.11.2 (created
/// 2026-07-17) long after v2.x was published, so the in-app update check never
/// saw anything newer. Fetching the full list and picking the max version
/// client-side (via `is_newer`) is immune to that marker drifting. per_page=100
/// is well above the ~30 releases published to date.
const GITEE_RELEASES_LIST: &str =
    "https://gitee.com/api/v5/repos/argustang/myshell/releases?per_page=100&page=1";

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
    /// `"auto"` = built-in download + installer launch (Windows NSIS path).
    /// `"browser"` = open the release page in the default browser and let the
    /// user download/install manually (Linux/macOS — no auto-update pipeline
    /// there; the `.exe`-only `install_update` would refuse a `.deb` anyway).
    /// Empty string on the error path (field is meaningless when `error` is set).
    update_strategy: String,
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
        update_strategy: String::new(),
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

    let resp = match client.get(GITEE_RELEASES_LIST).send().await {
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

    // The list endpoint returns an array of releases. Pick the one with the
    // highest semver tag (via `is_newer`) rather than relying on Gitee's
    // creation-order or "latest" marker — both have been observed wrong.
    let releases = match json.as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => {
            return update_info_error(&current_version, "未找到任何发布版本".to_string())
        }
    };
    let latest_json = releases
        .iter()
        .max_by(|a, b| {
            let av = a
                .get("tag_name")
                .and_then(|v| v.as_str())
                .or_else(|| a.get("name").and_then(|v| v.as_str()))
                .unwrap_or("0");
            let bv = b
                .get("tag_name")
                .and_then(|v| v.as_str())
                .or_else(|| b.get("name").and_then(|v| v.as_str()))
                .unwrap_or("0");
            if is_newer(av, bv) {
                std::cmp::Ordering::Greater
            } else if is_newer(bv, av) {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .expect("non-empty array checked above");

    // tag_name is the canonical version source on a Gitee release. Fall back
    // to the deprecated `name` field only if tag is absent.
    let tag = latest_json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            latest_json
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        });
    let Some(latest_version) = tag else {
        return update_info_error(&current_version, "未找到版本信息".to_string());
    };

    let release_url = latest_json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://gitee.com/argustang/myshell/releases")
        .to_string();

    // Prefer the first asset's download URL; otherwise the release page.
    // Platform-aware asset selection. On Windows we look for a `.exe` asset
    // (the NSIS installer); on Linux a `.deb`; other platforms have no
    // installer yet. Falling back to the release page means the UI's
    // download button always points somewhere useful rather than grabbing
    // `assets[0]` which could be the wrong platform's installer when both
    // are uploaded to the same release.
    let asset_suffix = if cfg!(target_os = "windows") {
        ".exe"
    } else if cfg!(target_os = "linux") {
        ".deb"
    } else {
        // macOS / others: no auto-installable asset; let UI fall back to
        // the release page (browser mode).
        ""
    };
    let download_url = latest_json
        .get("assets")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|asset| {
                let url = asset
                    .get("browser_download_url")
                    .and_then(|v| v.as_str())?;
                if !asset_suffix.is_empty() {
                    // Match by suffix (case-insensitive) so we pick the
                    // right-platform installer when a release carries both
                    // .exe and .deb.
                    if url.to_ascii_lowercase().ends_with(asset_suffix) {
                        Some(url.to_string())
                    } else {
                        None
                    }
                } else {
                    Some(url.to_string())
                }
            })
        })
        .unwrap_or_else(|| release_url.clone());

    // Linux/macOS have no built-in installer-launch pipeline (install_update
    // rejects non-.exe and would exec a wrong-arch binary). Route the UI to
    // "open the release page in the default browser" instead — user
    // downloads + runs the installer manually. Windows keeps the built-in
    // download-and-launch flow.
    let update_strategy = if cfg!(target_os = "windows") {
        "auto"
    } else {
        "browser"
    }
    .to_string();

    let published_at = latest_json
        .get("created_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let notes_raw = latest_json
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
        update_strategy,
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
    if !(lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:"))
    {
        return Err("仅支持 http(s) 链接和 mailto:".to_string());
    }
    use tauri_plugin_shell::ShellExt;
    app.shell()
        .open(url, None)
        .map_err(|e| format!("打开链接失败: {e}"))
}

/// Returns the canonical log directory path, mirroring `setup_file_logging`.
/// Kept as a free function so both `setup_file_logging` (release startup) and
/// `get_feedback_log` (user-triggered) agree on the location without sharing
/// mutable state.
fn log_dir_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("myshell")
        .join("logs")
}

/// Payload for `get_feedback_log` — what the feedback dialog needs to show
/// the user their log content and to reveal the folder in the file explorer.
#[derive(Clone, Serialize)]
struct FeedbackLogInfo {
    /// Absolute path to the logs dir, for the "open folder" button.
    log_dir: String,
    /// Today's (+ optionally yesterday's) log content, already scrubbed by
    /// `redact::scrub_log_text` so point-of-origin misses / third-party
    /// crate output can't leak hosts or IPs into a feedback email.
    content: String,
    /// True if the content was truncated from its full size.
    truncated: bool,
}

/// Read the current (and previous day's) log file for the feedback dialog.
///
/// Security: the content is scrubbed a second time via `redact::scrub_log_text`
/// — even if a code path forgot to mask a host at its log site, or a log file
/// predates the redaction work, no IP / user@host survives into the feedback
/// payload. Capped at 200 KiB (tail) so a runaway log can't OOM the email or
/// the webview.
#[tauri::command]
fn get_feedback_log() -> Result<FeedbackLogInfo, String> {
    const MAX_BYTES: usize = 200 * 1024;

    let log_dir = log_dir_path();
    let log_dir_str = log_dir.to_string_lossy().to_string();

    if !log_dir.exists() {
        // Debug builds don't set up file logging (setup_file_logging is
        // #[cfg(not(debug_assertions))]). Return empty rather than erroring —
        // the feedback dialog shows a "logs unavailable in dev build" note.
        return Ok(FeedbackLogInfo {
            log_dir: log_dir_str,
            content: String::new(),
            truncated: false,
        });
    }

    // Day index since epoch, same formula as setup_file_logging.
    let day = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86400)
        .unwrap_or(0);

    // Read today + yesterday so a late-night report written just past
    // midnight still has context. Yesterday first so chronology reads top-down.
    let mut combined = String::new();
    for offset in [1u64, 0] {
        let path = log_dir.join(format!("myshell-{}.log", day.saturating_sub(offset)));
        if let Ok(text) = std::fs::read_to_string(&path) {
            if !combined.is_empty() {
                combined.push_str("\n\n");
            }
            combined.push_str(&format!("===== {} =====\n", path.display()));
            combined.push_str(&text);
        }
    }

    // Tail-truncate if oversized (keep the most recent entries — those are
    // what's relevant to a just-encountered bug).
    let truncated = combined.len() > MAX_BYTES;
    if truncated {
        // Walk forward from the byte cut point to the next UTF-8 boundary so
        // we never split a multibyte codepoint (logs may contain Chinese).
        let mut start = combined.len() - MAX_BYTES;
        while !combined.is_char_boundary(start) {
            start += 1;
        }
        combined = format!("[…earlier log truncated, showing last {} KiB…]\n", MAX_BYTES / 1024)
            + &combined[start..];
    }

    // Defence-in-depth scrub: mask any IP / user@host that slipped through.
    let content = redact::scrub_log_text(&combined);

    Ok(FeedbackLogInfo {
        log_dir: log_dir_str,
        content,
        truncated,
    })
}

/// Windows: find an existing Explorer window showing `target` and bring it to
/// the foreground. Returns true if a matching window was found and focused,
/// false if no window exists (caller should spawn a new explorer).
///
/// Explorer exposes the current folder path via the address bar, but reading
/// it reliably requires UI Automation (complex). Instead we compare window
/// titles — Explorer sets the title to the folder name (e.g. "feedback"), so
/// we match on the last path segment. This is good enough for the feedback /
/// log folder use case where we only ever open these two specific dirs.
#[cfg(target_os = "windows")]
fn try_focus_existing_explorer(target: &std::path::Path) -> bool {
    use winapi::um::winuser::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
        SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOWNORMAL,
    };

    let folder_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    if folder_name.is_empty() {
        return false;
    }

    // Enumerate all top-level windows and look for an Explorer window whose
    // title matches our folder name. Explorer titles are just the folder name
    // (e.g. "feedback"), so this is a reliable match.
    struct EnumCtx {
        folder: String,
        found: u32, // HWND as u32
    }

    let mut ctx = EnumCtx {
        folder: folder_name.to_lowercase(),
        found: 0,
    };

    extern "system" fn enum_proc(hwnd: winapi::shared::windef::HWND, lparam: winapi::shared::minwindef::LPARAM) -> winapi::shared::minwindef::BOOL {
        unsafe {
            // Only consider visible windows.
            if IsWindowVisible(hwnd) == 0 {
                return 1; // continue
            }

            let len = GetWindowTextLengthW(hwnd);
            if len == 0 {
                return 1;
            }

            let mut buf = vec![0u16; (len as usize) + 1];
            GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            let title = String::from_utf16_lossy(&buf[..len as usize]);

            // Explorer titles are just the folder name (e.g. "feedback").
            // Also match "feedback" inside longer paths in the title.
            let ctx = &mut *(lparam as *mut EnumCtx);
            if title.to_lowercase() == ctx.folder {
                ctx.found = hwnd as u32;
                return 0; // stop enumerating
            }
            1 // continue
        }
    }

    unsafe {
        EnumWindows(
            Some(enum_proc),
            &mut ctx as *mut _ as winapi::shared::minwindef::LPARAM,
        );
    }

    if ctx.found == 0 {
        return false;
    }

    let hwnd = ctx.found as winapi::shared::windef::HWND;
    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        ShowWindow(hwnd, SW_SHOWNORMAL);
        SetForegroundWindow(hwnd);
    }
    true
}

/// Reveal a path in the OS file explorer. Whitelisted to the MyShell log dir
/// ONLY — arbitrary path opening would be a footgun (could be tricked into
/// running a malicious explorer registered for a path type). The feedback
/// dialog uses this for its "open feedback / log folder" button.
#[tauri::command]
#[allow(deprecated)]
fn reveal_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    // Canonicalize both sides so the allowlist isn't defeated by `..`, symlinks,
    // or differing separators. A non-existent path canonicalizes to itself on
    // Windows, which still fails the contains() check for a bogus path.
    let target = std::fs::canonicalize(&path).unwrap_or_else(|_| std::path::PathBuf::from(&path));
    // Allow the entire myshell config dir (logs/, feedback/, etc.) — not just
    // the logs subdir. The feedback dialog needs to open the feedback folder
    // which is a sibling of logs/ under the same parent.
    let allowed_root = log_dir_path()
        .parent() // …/myshell/
        .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()))
        .unwrap_or_else(|| log_dir_path());

    if !target.starts_with(&allowed_root) {
        return Err("仅允许打开 MyShell 配置目录".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        // Check if an explorer window is already open at this path; if so,
        // bring it to the foreground instead of spawning a duplicate.
        if !try_focus_existing_explorer(&target) {
            use tauri_plugin_shell::ShellExt;
            app.shell()
                .command("explorer.exe")
                .args([target.to_string_lossy().to_string()])
                .spawn()
                .map_err(|e| format!("打开文件夹失败: {e}"))?;
        }
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        use tauri_plugin_shell::ShellExt;
        app.shell()
            .command("open")
            .args([target.to_string_lossy().to_string()])
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {e}"))?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use tauri_plugin_shell::ShellExt;
        app.shell()
            .command("xdg-open")
            .args([target.to_string_lossy().to_string()])
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {e}"))?;
        Ok(())
    }
}

/// Save a feedback package (zip) built by the frontend (fflate) into a
/// `feedback/` subfolder of the log dir. Returns the full path so the UI can
/// show it and offer "open folder". This avoids pulling in a Tauri fs plugin
/// — the write is sandboxed to the feedback dir, not an arbitrary path.
///
/// The filename is supplied by the caller but sanitized: only alphanumerics,
/// `-`, `_`, `.` survive; a `.zip` extension is enforced. A collision
/// (unlikely with the timestamp) appends a counter.
#[tauri::command]
fn save_feedback_zip(filename: String, data: Vec<u8>) -> Result<String, String> {
    // Sanitize filename — strip anything that could escape the dir or confuse
    // the filesystem (path separators, control chars, etc.).
    let safe_name: String = filename
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe_name = if safe_name.ends_with(".zip") {
        safe_name
    } else {
        format!("{safe_name}.zip")
    };

    let dir = log_dir_path()
        .parent() // …/myshell/  (logs/ is one level down)
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("feedback");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("创建反馈目录失败: {e}"))?;

    // Disambiguate collisions with a counter.
    let mut path = dir.join(&safe_name);
    let mut counter = 1;
    while path.exists() {
        let stem = safe_name.trim_end_matches(".zip");
        path = dir.join(format!("{stem}-{counter}.zip"));
        counter += 1;
    }

    std::fs::write(&path, &data).map_err(|e| format!("写入反馈包失败: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

/// Clear all files in the feedback directory. Called when the feedback dialog
/// closes (after the user has had a chance to email the zip) to prevent
/// accumulation of old feedback zips on disk.
#[tauri::command]
fn clear_feedback_dir() -> Result<(), String> {
    let dir = log_dir_path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("feedback");

    if !dir.exists() {
        return Ok(());
    }

    let entries = std::fs::read_dir(&dir)
        .map_err(|e| format!("读取反馈目录失败: {e}"))?;

    for entry in entries.flatten() {
        let path = entry.path();
        // Only remove files (the zip packages), leave subdirs untouched.
        if path.is_file() {
            if let Err(e) = std::fs::remove_file(&path) {
                log::warn!("[feedback] failed to remove {}: {e}", path.display());
            }
        }
    }

    Ok(())
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
async fn download_update(app: tauri::AppHandle, window: tauri::WebviewWindow, url: String) -> Result<String, String> {
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
    // to track per-version temp files. Use the app config dir instead of
    // std::env::temp_dir() because some Windows environments return a temp
    // path that doesn't actually exist or is on a network share.
    let dest = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("获取应用目录失败: {e}"))?
        .join("myshell-update-setup.exe");

    use tokio::io::AsyncWriteExt;
    // Ensure the parent dir exists (it normally does, but be defensive).
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建目录失败: {e}"))?;
    }
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
        // Throttle events to ~every 32 KB so the progress bar updates
        // smoothly (~190 events for a 6 MB installer) without flooding the
        // webview. 256 KB caused only ~24 events which looked jerky.
        if downloaded - last_emitted >= 32 * 1024 || total == 0 {
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
/// replace the running files. The NSIS installer (perMachine mode) requires
/// admin privileges, so we use ShellExecuteW with the "runas" verb to trigger
/// a UAC elevation prompt — that's the standard Windows install UX.
///
/// `std::process::Command::spawn()` inherits the caller's token (os error 740
/// when the App isn't elevated), while ShellExecuteW+runas asks Windows to
/// elevate the child process even from a non-admin parent.
///
/// Security: the path must be a local `.exe` file — the `.exe` suffix check
/// and file-existence check are the only gates. "runas" is Windows-only
/// (no-op on other platforms in the broader sense, but this project only
/// builds for Windows NSIS).
#[tauri::command]
fn install_update(app: tauri::AppHandle, path: String) -> Result<(), String> {
    if path.bytes().any(|b| b == 0) {
        return Err("无效路径".to_string());
    }
    let meta = std::fs::metadata(&path).map_err(|_| "安装包不存在".to_string())?;
    if !meta.is_file() {
        return Err("安装包路径无效".to_string());
    }
    let lower = path.to_ascii_lowercase();
    if !lower.ends_with(".exe") {
        return Err("仅支持 .exe 安装包".to_string());
    }

    #[cfg(windows)]
    {
        use std::ptr::null_mut;
        use winapi::um::shellapi::ShellExecuteW;
        use winapi::um::winuser::SW_SHOWNORMAL;

        // Convert path to a wide-string (NUL-terminated) for the Windows API.
        let path_wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        // "runas\0" — ask Windows to elevate the process.
        let verb: Vec<u16> = "runas\0".encode_utf16().collect();
        let empty: Vec<u16> = vec![0]; // empty NUL-terminated string

        // SAFETY: ShellExecuteW is a Windows API; all pointers are to valid
        // NUL-terminated wide strings; null_mut() for unused optional params.
        let ret = unsafe {
            ShellExecuteW(
                null_mut(),        // hwnd
                verb.as_ptr(),     // lpOperation = "runas" (elevate)
                path_wide.as_ptr(),// lpFile = the installer exe
                empty.as_ptr(),    // lpParameters = none
                empty.as_ptr(),    // lpDirectory = none
                SW_SHOWNORMAL,     // nShowCmd
            )
        };
        // ShellExecuteW returns a value > 32 on success, <= 32 on failure.
        if ret as usize <= 32 {
            return Err(format!("启动安装器失败 (ShellExecuteW code: {})", ret as usize));
        }
    }

    #[cfg(not(windows))]
    {
        std::process::Command::new(&path)
            .spawn()
            .map_err(|e| format!("启动安装器失败: {e}"))?;
    }

    // Exit so the installer can overwrite our binaries. exit(0) runs the
    // RunEvent::Exit cleanups then terminates.
    app.exit(0);
    Ok(())
}

// ============ GPU acceleration toggle ============
//
// WebView2 (Windows) / WebKitGTK (Linux) render via the GPU. On some GPU +
// driver combinations the WebGL/canvas compositing misbehaves — the root cause
// behind the "terminal cursor invisible / selection highlight invisible"
// reports. We let the user disable GPU acceleration as an escape hatch.
//
// On Windows this is achieved by seeding the WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
// environment variable with --disable-gpu BEFORE WebView2 initializes (i.e.
// before the first window is created). Because the env var must be set that
// early, the flag is persisted to a plain file the Rust side reads at the very
// top of run(), so it takes effect on the NEXT app launch — not the current
// one. The frontend surfaces this "restart to apply" caveat in the UI.
//
// The flag lives at <config_dir>/myshell/gpu-disabled (presence = disabled),
// deliberately OUTSIDE the versioned backup set so an emergency GPU-off isn't
// rolled back by a version restore.

/// Path of the GPU-off marker file. Presence of the file (regardless of
/// contents) means "disable GPU on next launch".
fn gpu_disabled_flag_path() -> Option<std::path::PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("myshell");
    path.push("gpu-disabled");
    Some(path)
}

/// Read the persisted GPU-off flag. `true` when the marker file exists (GPU
/// should be disabled). Defaults to `false` (GPU on — the historical default)
/// on any IO error, since disabling GPU has a perf cost and shouldn't happen
/// accidentally.
#[tauri::command]
fn get_gpu_acceleration_disabled() -> bool {
    gpu_disabled_flag_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Set the persisted GPU-off flag. `disabled = true` creates the marker file
/// (GPU off on next launch); `false` removes it. Idempotent. The change takes
/// effect only on the next app restart.
#[tauri::command]
fn set_gpu_acceleration_disabled(disabled: bool) -> Result<(), String> {
    let path = gpu_disabled_flag_path().ok_or_else(|| "无法定位配置目录".to_string())?;
    // Ensure the parent dir exists (it normally does, but be defensive — a
    // first-ever launch with no DB yet has never created it).
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    if disabled {
        // Write a tiny, human-readable note so a user poking around the config
        // dir understands what this mystery file is.
        std::fs::write(&path, "1\n").map_err(|e| format!("写入标志文件失败: {e}"))?;
    } else {
        // RemoveFile is fine if the file is absent (no-op-ish); only treat a
        // non-NotFound error as real.
        if let Err(e) = std::fs::remove_file(&path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(format!("删除标志文件失败: {e}"));
            }
        }
    }
    Ok(())
}

// ============ Attachment directory + screenshot save ============
//
// User-configurable directory where terminal screenshots (and other AI/MCP
// attachments) are saved. Stored as a plain-text file containing the absolute
// path the user picked via the directory picker. Defaults to None (not
// configured) — the GUI prompts the user to set it the first time they try to
// take a screenshot.

/// Path of the marker file that stores the user's chosen attachment directory.
/// The FILE's location is fixed (always <config_dir>/myshell/attachment-dir);
/// the FILE's *contents* are the user-chosen directory path.
fn attachment_dir_setting_path() -> Option<std::path::PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("myshell");
    path.push("attachment-dir");
    Some(path)
}

/// Read the user's configured attachment directory. `None` if not yet set.
#[tauri::command]
fn get_attachment_dir() -> Option<String> {
    let path = attachment_dir_setting_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Persist the user's chosen attachment directory. Validates that the path
/// exists (or creates it) before saving.
#[tauri::command]
fn set_attachment_dir(dir: String) -> Result<String, String> {
    // Validate / create the target directory first — never remember a path
    // that doesn't work.
    let p = std::path::Path::new(&dir);
    std::fs::create_dir_all(p).map_err(|e| format!("创建目录失败: {e}"))?;
    let canonical = p.canonicalize()
        .map_err(|e| format!("无法解析目录路径: {e}"))?
        .to_string_lossy()
        .to_string();
    // Persist the canonical path.
    let setting_path = attachment_dir_setting_path().ok_or_else(|| "无法定位配置目录".to_string())?;
    if let Some(parent) = setting_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    std::fs::write(&setting_path, &canonical).map_err(|e| format!("写入配置失败: {e}"))?;
    Ok(canonical)
}

/// Save a PNG screenshot to the attachment directory.
///
/// `data_url` is a `data:image/png;base64,...` URL (what canvas.toDataURL
/// produces). `connection_name` is used to build a human-friendly filename.
///
/// Returns the absolute path of the saved file.
#[tauri::command]
fn save_screenshot(data_url: String, connection_name: String) -> Result<String, String> {
    use base64::Engine as _;

    // Strip the `data:image/png;base64,` prefix.
    let b64_payload = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| "期望 data:image/png;base64,... 格式".to_string())?;
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64_payload)
        .map_err(|e| format!("base64 解码失败: {e}"))?;

    // Resolve the attachment dir.
    let dir_str = get_attachment_dir()
        .ok_or_else(|| "未配置附件目录。请在「设置 → MCP 支持」中配置附件目录后重试。".to_string())?;
    let dir = std::path::Path::new(&dir_str);
    std::fs::create_dir_all(dir).map_err(|e| format!("附件目录不存在且无法创建: {e}"))?;

    // Build filename: 截图_<conn>_<YYYYMMDD-HHmmss>.png
    // Sanitize connection_name — Windows forbids \ / : * ? " < > |
    let safe_name: String = connection_name
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    // %Y%m%d-%H%M%S%.3f = YYYYMMDD-HHMMSS.fff (3 millisecond digits). The
    // fractional part prevents filename collisions when the user rapid-fires
    // the 📷 button — multiple screenshots within the same second would
    // otherwise overwrite each other.
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
    let filename = format!("截图_{}_{}.png", safe_name, ts);
    let filepath = dir.join(filename);

    std::fs::write(&filepath, &png_bytes).map_err(|e| format!("写入截图文件失败: {e}"))?;

    Ok(filepath.to_string_lossy().to_string())
}

// ============ MCP command confirmation rules ============
//
// User-configurable whitelist + blacklist (regex) that control which `ssh_exec`
// commands skip the human-confirmation dialog. Stored as JSON in the config
// dir so both the GUI (these commands) and the MCP server (separate process,
// reads the same file) stay in sync.

fn command_rules_path() -> Option<std::path::PathBuf> {
    let mut path = dirs::config_dir()?;
    path.push("myshell");
    path.push("mcp-command-rules.json");
    Some(path)
}

/// Read the configured command rules. Returns the built-in defaults if the
/// file doesn't exist yet (first launch) or fails to parse.
#[tauri::command]
fn get_command_rules() -> Result<myshell_core::command_rules::CommandRules, String> {
    let path = command_rules_path().ok_or_else(|| "无法定位配置目录".to_string())?;
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|e| format!("解析命令规则失败: {e}")),
        Err(_) => {
            // File doesn't exist — return defaults (not an error).
            Ok(myshell_core::command_rules::CommandRules::default())
        }
    }
}

/// Persist the command rules to the JSON config file.
#[tauri::command]
fn set_command_rules(rules: myshell_core::command_rules::CommandRules) -> Result<(), String> {
    let path = command_rules_path().ok_or_else(|| "无法定位配置目录".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&rules)
        .map_err(|e| format!("序列化命令规则失败: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("写入命令规则失败: {e}"))?;
    Ok(())
}

/// Called by the frontend to deliver the result of an `exec_in_tab` request
/// back to the waiting IPC listener thread. The listener registered a oneshot
/// sender under `request_id`; we look it up, send the result, and clean up.
///
/// `result` is a JSON object: `{ok: bool, stdout?: string, exit_code?: int, error?: string}`.
#[tauri::command]
fn mcp_exec_result(request_id: String, result: serde_json::Value) -> Result<(), String> {
    let mut pending = PENDING_EXEC.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = pending.remove(&request_id) {
        // Send the result to the waiting IPC thread. Ignore send errors — the
        // thread may have already timed out and dropped the receiver.
        let _ = tx.send(result);
        Ok(())
    } else {
        // No pending request for this ID — likely a duplicate call or the
        // request already timed out. Not an error (idempotent).
        Ok(())
    }
}

/// Open a file or folder in the OS file manager. On Windows, selects the file
/// in Explorer (like "Show in Folder"); on macOS, reveals in Finder; on Linux,
/// opens the containing directory in xdg-open.
#[tauri::command]
fn show_in_folder(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(format!("路径不存在: {}", path));
    }

    #[cfg(target_os = "windows")]
    {
        // `explorer.exe /select,<path>` selects the file in Explorer. For a
        // directory, just open the directory.
        use std::process::Command;
        let arg = if p.is_dir() {
            path.clone()
        } else {
            // /select, expects backslashes — canonicalize already gave us native
            // separators on Windows.
            format!("/select,{}", path)
        };
        Command::new("explorer.exe")
            .arg(&arg)
            .spawn()
            .map_err(|e| format!("无法打开文件管理器: {e}"))?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        if p.is_dir() {
            Command::new("open").arg(&path).spawn()
        } else {
            Command::new("open").args(["-R", &path]).spawn()
        }
        .map_err(|e| format!("无法打开 Finder: {e}"))?;
        Ok(())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use std::process::Command;
        // xdg-open only opens directories, not "reveal file", so open the parent.
        let target = if p.is_dir() { path.clone() } else {
            p.parent().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| path.clone())
        };
        Command::new("xdg-open").arg(&target).spawn()
            .map_err(|e| format!("无法打开文件管理器: {e}"))?;
        Ok(())
    }
}

// ============ Frontend log forwarding ============
//
// The webview's JS runtime (console) isn't persisted on the user's machine —
// they'd have to open devtools, which they won't. To diagnose frontend-side
// anomalies (the "cursor invisible" class of report, a render crash, an
// unhandled promise rejection), the frontend forwards its errors here so they
// land in the SAME daily log file as the Rust backend output. The two then
// share a timeline, so a frontend render error can be correlated with the
// backend SSH/PTY event that preceded it.
//
// Every line is tagged `[frontend]` so a reader can tell at a glance which
// side of the IPC boundary produced it.

/// Severity the frontend may forward. Anything outside this set is coerced to
/// `info` so a typo can't be used to suppress an error via an invalid level.
fn frontend_log_level(raw: &str) -> log::Level {
    match raw.to_ascii_lowercase().as_str() {
        "error" => log::Level::Error,
        "warn" => log::Level::Warn,
        "info" => log::Level::Info,
        "debug" => log::Level::Debug,
        _ => log::Level::Info,
    }
}

/// Write a single frontend log line into the shared log file. `message` should
/// already be a single line (the frontend joins multi-line stacks); we replace
/// any residual newlines so one event = one log line, keeping the file greppable.
#[tauri::command]
fn write_frontend_log(level: String, message: String) {
    let one_line = message.replace(['\n', '\r'], " ⏎ ");
    let lvl = frontend_log_level(&level);
    match lvl {
        log::Level::Error => log::error!("[frontend] {}", one_line),
        log::Level::Warn => log::warn!("[frontend] {}", one_line),
        log::Level::Info => log::info!("[frontend] {}", one_line),
        log::Level::Debug => log::debug!("[frontend] {}", one_line),
        // trace isn't reachable from frontend_log_level, but match must be exhaustive.
        log::Level::Trace => log::trace!("[frontend] {}", one_line),
    }
}

/// Read the GPU-off flag at the very top of run() and, when set, seed the
/// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS env var so WebView2 boots without GPU
/// compositing. Runs before any window is created — that ordering is what makes
/// it actually work (the env var is read once during WebView2 environment
/// creation). Idempotent + silent on non-Windows (the env var is harmless
/// there but also pointless, so we skip it to avoid surprises).
fn apply_gpu_pref_if_disabled() {
    let disabled = gpu_disabled_flag_path()
        .map(|p| p.exists())
        .unwrap_or(false);
    if !disabled {
        return;
    }
    // Only meaningful on Windows (WebView2). WebKitGTK on Linux/macOS doesn't
    // honor this env var, so don't set it there.
    #[cfg(windows)]
    {
        // Append rather than overwrite: respect any user/system value already in
        // the environment (e.g. a proxy flag set externally) and just tack
        // --disable-gpu on if it isn't already present.
        const FLAG: &str = "--disable-gpu";
        let existing = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS")
            .unwrap_or_default();
        let merged = if existing.split_whitespace().any(|f| f == FLAG) {
            existing
        } else if existing.trim().is_empty() {
            FLAG.to_string()
        } else {
            format!("{existing} {FLAG}")
        };
        // SAFETY: setenv of a thread-local-ish env var before any WebView2
        // thread spawns. We're still single-threaded at the top of run().
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", merged);
        log::info!("[startup] GPU acceleration disabled via gpu-disabled flag (applies to WebView2 on next window creation)");
    }
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
fn delete_folder(state: State<AppState>, path: String) -> Result<String, String> {
    let trimmed = normalize_folder_path(&path);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    // Cascade delete: soft-deletes child connections into the recycle bin
    // (restoreable) and physically drops this folder + all descendants. The
    // frontend has already asked the user to confirm when the folder is
    // non-empty (showing the exact counts), so we don't block here. Overflow
    // connections (beyond the 30-row cap) are hard-purged and their keyring
    // entries cleaned here.
    let outcome = db::delete_folder_recursive(&db, &trimmed).map_err(|e| e.to_string())?;
    for pid in &outcome.purged_conn_ids {
        if let Err(e) = secrets::delete_password(pid) {
            log::warn!("[delete_folder] keyring purge password failed for {}: {}", pid, e);
        }
        if let Err(e) = secrets::delete_proxy_password(pid) {
            log::warn!("[delete_folder] keyring purge proxy password failed for {}: {}", pid, e);
        }
    }
    Ok(format!(
        "已删除 {} 个文件夹，{} 个连接移入回收站",
        outcome.folders_deleted,
        outcome.soft_deleted_conn_ids.len()
    ))
}

/// A connection in the recycle bin: its config + when it was soft-deleted.
/// Used by the RecycleDialog so the user can see name/host and deletion time.
#[derive(serde::Serialize)]
pub struct DeletedConnection {
    #[serde(flatten)]
    pub config: ConnectionConfig,
    pub deleted_at: String,
}

#[tauri::command]
fn get_deleted_connections(state: State<AppState>) -> Result<Vec<DeletedConnection>, String> {
    let key = require_dek(&state)?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let rows = db::get_deleted_connections(&db, &key).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(config, deleted_at)| DeletedConnection { config, deleted_at })
        .collect())
}

#[tauri::command]
fn restore_connection(state: State<AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::restore_connection(&db, &id).map_err(|e| e.to_string())
}

/// Permanently delete a single recycled connection (from the RecycleDialog's
/// per-row "彻底删除"). Hard-deletes the row + quick_commands and purges its
/// keyring entries.
#[tauri::command]
fn purge_connection(state: State<AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::hard_delete_connection(&db, &id).map_err(|e| e.to_string())?;
    if let Err(e) = secrets::delete_password(&id) {
        log::warn!("[purge_connection] keyring purge password failed for {}: {}", id, e);
    }
    if let Err(e) = secrets::delete_proxy_password(&id) {
        log::warn!("[purge_connection] keyring purge proxy password failed for {}: {}", id, e);
    }
    Ok(())
}

/// Empty the recycle bin (from the RecycleDialog's "清空回收站"). Hard-deletes
/// every soft-deleted row + its quick_commands, then purges their keyring.
#[tauri::command]
fn purge_all_deleted_connections(state: State<AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let ids = db::purge_all_deleted(&db).map_err(|e| e.to_string())?;
    for id in &ids {
        if let Err(e) = secrets::delete_password(id) {
            log::warn!("[purge_all_deleted_connections] keyring purge password failed for {}: {}", id, e);
        }
        if let Err(e) = secrets::delete_proxy_password(id) {
            log::warn!("[purge_all_deleted_connections] keyring purge proxy password failed for {}: {}", id, e);
        }
    }
    Ok(())
}

#[tauri::command]
fn rename_connection(
    state: State<AppState>,
    id: String,
    new_name: String,
) -> Result<(), String> {
    let trimmed = new_name.trim().to_string();
    if trimmed.is_empty() {
        return Err("名称不能为空".to_string());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db::rename_connection(&db, &id, &trimmed).map_err(|e| e.to_string())
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
    let result = ssh::connect(&state, Arc::new(WindowSink(window)), config).await;
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
    match ssh::disconnect(&state, &session_id).await {
        Ok(()) => {
            log::info!("[ssh:{}] disconnected", session_id);
            Ok(())
        }
        Err(e) => {
            log::warn!("[ssh:{}] disconnect failed: {}", session_id, e);
            Err(e)
        }
    }
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

/// Frontend signals the ZMODEM session is over (zmodem.js parsed ZFIN or user
/// aborted). Switches the reader task back to Normal mode authoritatively.
#[tauri::command]
async fn ssh_zmodem_finish(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    ssh::zmodem_finish(&state, &session_id).await
}

/// Native ZMODEM download: frontend chose a save path for the offered file
/// (`path` = full path) or passes `null` to skip this file.
#[tauri::command]
async fn zmodem_accept_offer(
    state: State<'_, AppState>,
    session_id: String,
    path: Option<String>,
) -> Result<(), String> {
    ssh::zmodem_accept_offer(&state, &session_id, path).await
}

/// Native ZMODEM upload: frontend selected local file(s) to upload. The reader
/// loop opens the first file, creates a ZmodemSender, and streams data directly
/// over the SSH channel (zero IPC, symmetric to the native download path).
#[tauri::command]
async fn zmodem_start_upload(
    state: State<'_, AppState>,
    session_id: String,
    paths: Vec<String>,
) -> Result<(), String> {
    ssh::zmodem_start_upload(&state, &session_id, paths).await
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
    match local::connect(&state, Arc::new(WindowSink(window)), config, 80, 24).await {
        Ok(sid) => {
            log::info!("[local:{}] connected", sid);
            Ok(sid)
        }
        Err(e) => Err(e),
    }
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
    match local::disconnect(&state, &session_id).await {
        Ok(()) => {
            log::info!("[local:{}] disconnected", session_id);
            Ok(())
        }
        Err(e) => {
            log::warn!("[local:{}] disconnect failed: {}", session_id, e);
            Err(e)
        }
    }
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
    let sink = WindowSink(window);
    ai::chat_stream(
        &state,
        &sink,
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
    let sink = WindowSink(window);
    ai::inspect_health_ssh(&state, &sink, &session_id, &request_id).await
}

/// Run the read-only diagnostic script on the local machine, then stream an
/// AI health report. No PTY session needed — it's the user's own machine.
#[tauri::command]
async fn ai_inspect_health_local(
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    request_id: String,
) -> Result<(), String> {
    let sink = WindowSink(window);
    ai::inspect_health_local(&state, &sink, &request_id).await
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

// ── Multi-model management commands ──────────────────────────────────────

#[tauri::command]
fn list_ai_models(state: State<'_, AppState>) -> Result<Vec<ai::AiModelInfo>, String> {
    ai::list_ai_models_cmd(&state)
}

#[tauri::command]
fn get_active_ai_model_id(state: State<'_, AppState>) -> Result<Option<i64>, String> {
    ai::get_active_model_id(&state)
}

#[tauri::command]
fn get_active_ai_selection(state: State<'_, AppState>) -> Result<ai::ActiveAiSelection, String> {
    ai::get_active_selection_cmd(&state)
}

#[tauri::command]
fn save_ai_model(
    state: State<'_, AppState>,
    id: Option<i64>,
    name: String,
    provider: String,
    model_id: String,
    base_url: Option<String>,
    api_key: Option<String>,
    proxy_url: Option<String>,
    temperature: f64,
    models: Option<Vec<SupplierModelPayload>>,
) -> Result<i64, String> {
    let models_arg = models.map(|list| {
        list.into_iter()
            .map(|m| (m.model_id, m.label))
            .collect::<Vec<_>>()
    });
    ai::save_ai_model_cmd(
        &state,
        id,
        name,
        provider,
        model_id,
        base_url,
        api_key,
        proxy_url,
        temperature,
        models_arg,
    )
}

#[tauri::command]
fn delete_ai_model(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    ai::delete_ai_model_cmd(&state, id)
}

#[tauri::command]
fn set_active_ai_model(
    state: State<'_, AppState>,
    id: i64,
    model_string: Option<String>,
) -> Result<(), String> {
    ai::set_active_ai_model_cmd(&state, id, model_string)
}

#[tauri::command]
fn init_ai_presets(state: State<'_, AppState>) -> Result<(), String> {
    ai::init_ai_presets_cmd(&state)
}

/// Fetch available models from a provider's /models endpoint (OpenAI format).
#[tauri::command]
async fn fetch_provider_models(
    provider: String,
    base_url: String,
    api_key: String,
) -> Result<Vec<String>, String> {
    ai::fetch_provider_models(&provider, &base_url, &api_key).await
}

/// Fetch available models for a specific supplier by id. Decrypts the stored
/// key server-side — used after save so the frontend doesn't handle plaintext.
#[tauri::command]
async fn fetch_models_for_supplier(
    state: State<'_, AppState>,
    supplier_id: i64,
    override_key: Option<String>,
) -> Result<Vec<String>, String> {
    ai::fetch_models_for_supplier(&state, supplier_id, override_key).await
}

// ── Supplier model management ─────────────────────────────────────────────

/// Payload for a single model entry when saving a supplier (model_id + label).
#[derive(serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SupplierModelPayload {
    pub model_id: String,
    pub label: Option<String>,
}

#[tauri::command]
fn list_supplier_models(
    state: State<'_, AppState>,
    supplier_id: i64,
) -> Result<Vec<ai::SupplierModelInfo>, String> {
    ai::list_supplier_models_cmd(&state, supplier_id)
}

#[tauri::command]
fn add_supplier_model(
    state: State<'_, AppState>,
    supplier_id: i64,
    model_id: String,
    label: Option<String>,
) -> Result<i64, String> {
    ai::add_supplier_model_cmd(&state, supplier_id, model_id, label)
}

#[tauri::command]
fn remove_supplier_model(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    ai::remove_supplier_model_cmd(&state, id)
}

#[tauri::command]
fn toggle_ai_model_enabled(
    state: State<'_, AppState>,
    id: i64,
    enabled: bool,
) -> Result<(), String> {
    ai::toggle_ai_model_enabled_cmd(&state, id, enabled)
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
    /// MemTotal from `free -b`.
    mem_total_bytes: u64,
    /// Used memory = `total − MemAvailable` (the kernel's usable-memory
    /// estimate; matches htop 3.x / node_exporter). Falls back to procps's
    /// corrected `used` on legacy systems without an available column.
    mem_used_bytes: u64,
    cpu_usage_pct: f32,
    /// `mem_used_bytes / mem_total_bytes * 100`. 0 when total is 0.
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

    // free -b Mem line (procps ≥ 3.3.10):
    // "Mem:  total  used  free  shared  buff/cache  available"
    // Usage is computed as `total − available`: MemAvailable is the kernel's
    // own estimate of usable memory (what htop 3.x / node_exporter / k8s
    // report), whereas procps's derived `used` column overstates pressure on
    // shmem/tmpfs-heavy boxes. Old `free` output also has 7 tokens on the
    // Mem: line (… buffers cached), so the available column must be detected
    // from the header, not the token count.
    let mem_section = section(raw, "=M=");
    let has_available = mem_section
        .lines()
        .next()
        .map(|header| header.contains("available"))
        .unwrap_or(false);
    let (mem_total, mem_used) = mem_section
        .lines()
        .find(|l| l.starts_with("Mem:"))
        .and_then(|l| {
            let v: Vec<&str> = l.split_whitespace().collect();
            let total = v.get(1)?.parse::<u64>().ok()?;
            let used = if has_available && v.len() >= 7 {
                // Saturate against accounting weirdness (available > total).
                total.saturating_sub(v.get(6)?.parse::<u64>().ok()?)
            } else {
                // Legacy free (< procps 3.3.10): no available column, and the
                // Mem: used there counts buffers/cache as consumed. Prefer the
                // corrected "-/+ buffers/cache:" row when present.
                mem_section
                    .lines()
                    .find(|l| l.starts_with("-/+ buffers/cache:"))
                    .and_then(|l| l.split_whitespace().nth(2))
                    .and_then(|s| s.parse::<u64>().ok())
                    .or_else(|| v.get(2).and_then(|s| s.parse::<u64>().ok()))?
            };
            Some((total, used))
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

#[cfg(test)]
mod server_info_tests {
    use super::*;

    /// Modern procps (≥ 3.3.10): header has an `available` column, so
    /// used = total − available, NOT the procps-derived `used` column.
    #[test]
    fn mem_usage_prefers_total_minus_available() {
        let raw = "=OS=\nDebian GNU/Linux 12 (bookworm)\n\
                   =K=\n6.1.0-18-amd64\n\
                   =C=\n8\n\
                   =M=\n               total        used        free      shared  buff/cache   available\n\
                   Mem:     16266356000  4194304000  2097152000   268435456  8589934592  11534336000\n\
                   Swap:     2147483648           0  2147483648\n\
                   =DT=\n1000202043392 412345678901\n\
                   =DM=\n/dev/sda1 916G 384G 45% /\n\
                   =S1=\ncpu  10000 0 5000 80000 1000 0 500 0 0 0\n\
                   =S2=\ncpu  10100 0 5100 80700 1010 0 510 0 0 0\n";
        let info = parse_server_info(raw);
        assert_eq!(info.mem_total_bytes, 16266356000);
        // total − available, not the 4194304000 `used` column
        assert_eq!(info.mem_used_bytes, 16266356000 - 11534336000);
        assert!((info.mem_usage_pct - 29.09).abs() < 0.1);
    }

    /// Legacy procps (< 3.3.10): 7 tokens on the Mem: line too (last is
    /// `cached`), but no `available` in the header — fall back to the
    /// corrected "-/+ buffers/cache:" row, not Mem:'s inflated used.
    #[test]
    fn mem_usage_legacy_uses_buffers_cache_row() {
        let raw = "=M=\n             total       used       free     shared    buffers     cached\n\
                   Mem:       8192000000 7000000000 1192000000          0  500000000 3500000000\n\
                   -/+ buffers/cache: 3000000000 5192000000\n\
                   Swap:      4095996000          0 4095996000\n";
        let info = parse_server_info(raw);
        assert_eq!(info.mem_total_bytes, 8192000000);
        assert_eq!(info.mem_used_bytes, 3000000000);
        assert!((info.mem_usage_pct - 36.62).abs() < 0.1);
    }

    /// Missing/garbage sections degrade to zeros instead of panicking.
    #[test]
    fn mem_usage_missing_section_is_zero() {
        let info = parse_server_info("=OS=\nLinux\n=M=\ngarbage\n");
        assert_eq!(info.mem_total_bytes, 0);
        assert_eq!(info.mem_used_bytes, 0);
        assert_eq!(info.mem_usage_pct, 0.0);
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
    let sink = WindowSink(window);
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    state.transfer_cancels.lock().unwrap().insert(request_id.clone(), cancel.clone());
    let result = sftp::upload(
        &state,
        &session_id,
        local_paths,
        &remote_dest_dir,
        &request_id,
        &sink,
        cancel,
    )
    .await;
    state.transfer_cancels.lock().unwrap().remove(&request_id);
    result
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
    let sink = WindowSink(window);
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    state.transfer_cancels.lock().unwrap().insert(request_id.clone(), cancel.clone());
    let result = sftp::download(
        &state,
        &session_id,
        remote_paths,
        &local_dest_dir,
        &request_id,
        &sink,
        cancel,
    )
    .await;
    state.transfer_cancels.lock().unwrap().remove(&request_id);
    result
}

/// Cancel an in-flight SFTP transfer by request_id. Sets the atomic flag that
/// the download/upload chunk loop checks between 32 KB reads.
#[tauri::command]
async fn sftp_cancel_transfer(
    request_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    let cancels = state.transfer_cancels.lock().unwrap();
    match cancels.get(&request_id) {
        Some(flag) => {
            flag.store(true, Ordering::Relaxed);
            Ok(())
        }
        None => Err("传输不存在或已结束".to_string()),
    }
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

	// ============ MCP Server Management ============

	/// Get the absolute path to myshell-mcp.exe.
	#[tauri::command]
	fn mcp_get_binary_path() -> Result<String, String> {
	    mcp_tools::mcp_binary_path()
	}

	/// Detect installed AI tools and whether they have myshell configured.
	#[tauri::command]
	fn mcp_detect_tools() -> Vec<mcp_tools::AiToolInfo> {
	    mcp_tools::mcp_detect_tools()
	}

	/// Write myshell MCP config to a specific tool. Returns true if written, false if already configured.
	#[tauri::command]
	fn mcp_write_config(tool_id: String) -> Result<bool, String> {
	    let binary_path = mcp_tools::mcp_binary_path()?;
	    mcp_tools::mcp_write_config(&tool_id, &binary_path)
	}

	/// Remove myshell from a tool's MCP config.
	#[tauri::command]
	fn mcp_remove_config(tool_id: String) -> Result<(), String> {
	    mcp_tools::mcp_remove_config(&tool_id)
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
#[cfg(all(not(debug_assertions), windows))]
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

/// Unix release builds: no stderr redirection. The Windows version uses
/// MSVC CRT symbols (`_open_osfhandle` / `_dup2` / `as_raw_handle`) that
/// don't exist on Linux, so on Unix we simply leave stderr attached to the
/// parent process (the desktop launcher / shell). The log-directory
/// creation and 7-day pruning logic from the Windows path are intentionally
/// NOT duplicated here yet — Linux logging support is deferred until a
/// later release; for now the priority is producing a working .deb.
#[cfg(all(not(debug_assertions), unix))]
fn setup_file_logging() {}

/// Debug builds don't redirect stderr (neither Windows nor Unix) — keep
/// `eprintln!` visible in the console for development.
#[cfg(debug_assertions)]
fn setup_file_logging() {}

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

// ============ Single-instance guard ============
//
// Detect a running MyShell GUI at startup, before any window/DB work. If one
// is found, pop a native dialog: [是] restart (kill the old instance, keep
// starting this one) / [否] quit (leave the old instance alone, exit now).
//
// Detection uses a Global named mutex (CreateMutexW). The OS releases it when
// the owning process dies — crash, TerminateProcess, anything — so there is
// no stale-lock problem (unlike the gui-ipc-port file, which survives
// crashes). The `Global\` prefix makes the check span all sessions/users on
// the machine, matching single-instance semantics.
//
// Non-Windows: no-op, startup always proceeds (same convention as
// confirm_dangerous_operation in myshell-mcp.rs).

/// Try to become the only MyShell GUI instance.
/// - Ok(true): we hold the mutex — proceed with startup.
/// - Ok(false): another instance exists and the user chose "quit" — return
///   from run() immediately.
/// - Err: the user chose "restart" but the old instance refused to die —
///   surfaced by the caller as a startup failure.
#[cfg(windows)]
fn acquire_single_instance_lock() -> Result<bool, String> {
    use std::os::windows::ffi::OsStrExt;
    use winapi::shared::winerror::ERROR_ALREADY_EXISTS;
    use winapi::um::errhandlingapi::GetLastError;
    use winapi::um::synchapi::{CreateMutexW, WaitForSingleObject};
    use winapi::um::winbase::{WAIT_ABANDONED, WAIT_OBJECT_0};
    use winapi::um::winuser::{MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_SYSTEMMODAL, MessageBoxW};

    const MUTEX_NAME: &str = "Global\\MyShellSingleInstanceMutex";

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    // Block on the mutex for up to `secs`, polling in 500ms slices so we can
    // log progress and respect the deadline. WAIT_ABANDONED counts as
    // acquired: it means the previous owner died without releasing — exactly
    // the "old instance exited" transition we are waiting for.
    unsafe fn wait_mutex_free(mutex: winapi::um::winnt::HANDLE, secs: u64) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        loop {
            let r = WaitForSingleObject(mutex, 500);
            if r == WAIT_OBJECT_0 || r == WAIT_ABANDONED {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
        }
    }

    unsafe {
        let name = wide(MUTEX_NAME);
        let mutex = CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr());
        if mutex.is_null() || mutex == winapi::um::handleapi::INVALID_HANDLE_VALUE {
            return Err("无法创建单实例互斥体（CreateMutexW 失败）".into());
        }
        // Intentionally leak the handle — it must stay alive for the whole
        // process lifetime so other starters can detect us. The kernel cleans
        // it up on process exit.
        if GetLastError() != ERROR_ALREADY_EXISTS {
            return Ok(true); // we are the first instance
        }

        log::info!("[single-instance] 检测到已有实例，弹窗询问用户");

        if !ask_restart_or_quit() {
            log::info!("[single-instance] 用户选择退出，保持现有实例");
            return Ok(false);
        }
        log::info!("[single-instance] 用户选择重启，尝试结束旧实例");

        // Phase 1: graceful — ask the old instance to exit via the localhost
        // IPC bridge. Its shutdown handler calls app.exit(0), which flows
        // through RunEvent::ExitRequested (drains SSH/local-PTY sessions,
        // deletes the port file). This is the clean path.
        shutdown_existing_via_ipc();

        if wait_mutex_free(mutex, 5) {
            log::info!("[single-instance] 旧实例已优雅退出");
            return Ok(true);
        }

        // Phase 2: fallback — the old instance didn't exit in time (hung,
        // IPC port stale, older build without the shutdown action). Force-
        // terminate every myshell.exe that isn't us. This skips the old
        // instance's ExitRequested cleanup — local-PTY child shells may be
        // orphaned — but leaves the machine in a working state, which wins.
        log::warn!("[single-instance] 优雅退出超时，强杀旧实例进程");
        kill_other_myshell_processes();

        if wait_mutex_free(mutex, 5) {
            log::info!("[single-instance] 旧实例已被强制结束");
            return Ok(true);
        }

        // Both phases failed (e.g. the old instance runs elevated under
        // another session and we lack PROCESS_TERMINATE rights). Tell the
        // user and bail — starting anyway would corrupt the single-instance
        // invariant.
        let msg = wide("无法结束已运行的 MyShell 实例（可能权限不足或进程无响应）。\n\
                       请手动在任务管理器中结束 myshell.exe 后重试。");
        MessageBoxW(
            std::ptr::null_mut(),
            msg.as_ptr(),
            wide("MyShell 启动失败").as_ptr(),
            MB_OK | MB_ICONERROR | MB_SYSTEMMODAL | MB_SETFOREGROUND,
        );
        Err("无法结束已运行的 MyShell 实例".into())
    }
}

/// The "already running" dialog. true = user chose overwrite/restart, false
/// = user chose quit (or dismissed with X). Uses the Win32 TaskDialog API
/// so the buttons can carry our own labels ("覆盖启动" / "退出") instead of
/// the fixed MB_YESNO labels — gives the modern Windows 11 rounded-button
/// styling automatically.
#[cfg(windows)]
fn ask_restart_or_quit() -> bool {
    use std::os::raw::{c_int, c_void};
    use std::os::windows::ffi::OsStrExt;

    const BTN_QUIT: c_int = 100;
    const BTN_OVERWRITE: c_int = 101;

    #[repr(C)]
    struct TASKDIALOG_BUTTON {
        nButtonID: c_int,
        pszButtonText: *const u16,
    }

    #[repr(C)]
    union TaskDialogIcon {
        hMainIcon: *mut c_void,
        pszMainIcon: *const u16,
    }

    #[repr(C)]
    union TaskDialogFooterIcon {
        hFooterIcon: *mut c_void,
        pszFooterIcon: *const u16,
    }

    #[repr(C)]
    struct TASKDIALOGCONFIG {
        cbSize: u32,
        hwndParent: *mut c_void,
        hInstance: *mut c_void,
        dwFlags: u32,
        dwCommonButtons: u32,
        pszWindowTitle: *const u16,
        mainIcon: TaskDialogIcon,
        pszMainInstruction: *const u16,
        pszContent: *const u16,
        cButtons: u32,
        pButtons: *const TASKDIALOG_BUTTON,
        nDefaultButton: c_int,
        cRadioButtons: u32,
        pRadioButtons: *const TASKDIALOG_BUTTON,
        nDefaultRadioButton: c_int,
        pszVerificationText: *const u16,
        pszExpandedInformation: *const u16,
        pszExpandedControlText: *const u16,
        pszCollapsedControlText: *const u16,
        footerIcon: TaskDialogFooterIcon,
        pszFooter: *const u16,
        pfCallback: *const c_void,
        lpCallbackData: *mut c_void,
        cxWidth: u32,
    }

    #[link(name = "comctl32")]
    extern "system" {
        fn TaskDialogIndirect(
            pTaskConfig: *const TASKDIALOGCONFIG,
            pnButton: *mut c_int,
            pnRadioButton: *mut c_int,
            pfVerificationFlagChecked: *mut c_int,
        ) -> c_int;
    }

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    // All wide() Vecs must outlive the call — TaskDialogIndirect only reads
    // them during the call itself, but Rust borrow checker can't see that,
    // so we bind them to locals and let them drop at function end.
    let title = wide("MyShell");
    let main_instruction = wide("应用已在运行");
    let content = wide(
        "检测到另一次启动。覆盖启动会结束当前实例并重新启动应用；\
         退出则保持当前实例继续运行（本次启动已自动结束）。",
    );
    let quit_label = wide("退出");
    let overwrite_label = wide("覆盖启动");

    // Buttons render left-to-right in array order. "退出" on the left
    // (less destructive, lighter visual weight), "覆盖启动" on the right
    // (primary action — nDefaultButton makes Enter trigger it and gives
    // it the accent colour).
    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: BTN_QUIT,
            pszButtonText: quit_label.as_ptr(),
        },
        TASKDIALOG_BUTTON {
            nButtonID: BTN_OVERWRITE,
            pszButtonText: overwrite_label.as_ptr(),
        },
    ];

    let cfg = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: std::ptr::null_mut(),
        hInstance: std::ptr::null_mut(),
        dwFlags: 0,
        dwCommonButtons: 0,
        pszWindowTitle: title.as_ptr(),
        mainIcon: TaskDialogIcon {
            pszMainIcon: std::ptr::null(),
        },
        pszMainInstruction: main_instruction.as_ptr(),
        pszContent: content.as_ptr(),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: BTN_OVERWRITE,
        cRadioButtons: 0,
        pRadioButtons: std::ptr::null(),
        nDefaultRadioButton: 0,
        pszVerificationText: std::ptr::null(),
        pszExpandedInformation: std::ptr::null(),
        pszExpandedControlText: std::ptr::null(),
        pszCollapsedControlText: std::ptr::null(),
        footerIcon: TaskDialogFooterIcon {
            pszFooterIcon: std::ptr::null(),
        },
        pszFooter: std::ptr::null(),
        pfCallback: std::ptr::null(),
        lpCallbackData: std::ptr::null_mut(),
        cxWidth: 0,
    };

    let mut clicked: c_int = 0;
    log::info!(
        "[single-instance] TaskDialog cbSize={} (expect 176), buttons_ptr={:p}, n_buttons={}",
        std::mem::size_of::<TASKDIALOGCONFIG>(),
        buttons.as_ptr(),
        buttons.len()
    );
    let hr = unsafe {
        TaskDialogIndirect(
            &cfg,
            &mut clicked,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if hr < 0 {
        log::warn!(
            "[single-instance] TaskDialogIndirect 失败 hr={}，默认按退出处理",
            hr
        );
        return false;
    }
    // Cancel (X button) → clicked == 0 → falls through to "false" = quit,
    // which is the safe choice: never overwrite on an ambiguous result.
    clicked == BTN_OVERWRITE
}

/// Ask the running instance to exit gracefully through the localhost IPC
/// bridge (`{"action":"shutdown"}`). Best-effort: any failure just falls
/// through to the force-kill fallback — the caller re-checks the mutex.
#[cfg(windows)]
fn shutdown_existing_via_ipc() {
    let Some(dir) = dirs::config_dir() else { return };
    let port_file = dir.join("myshell").join("gui-ipc-port");
    let Ok(raw) = std::fs::read_to_string(&port_file) else {
        log::warn!("[single-instance] 无 gui-ipc-port 文件，跳过优雅退出");
        return;
    };
    let Ok(port) = raw.trim().parse::<u16>() else {
        log::warn!("[single-instance] gui-ipc-port 内容无效: {:?}", raw.trim());
        return;
    };
    let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
        log::warn!("[single-instance] IPC 端口 {} 连接失败（stale port file?）", port);
        return;
    };
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    use std::io::{Read, Write};
    if stream.write_all(b"{\"action\":\"shutdown\"}\n").is_err() {
        log::warn!("[single-instance] 发送 shutdown 指令失败");
        return;
    }
    // Read the ack (best-effort — the old instance exits right after
    // writing it, so a reset here is normal).
    let mut buf = [0u8; 128];
    let _ = stream.read(&mut buf);
    log::info!("[single-instance] shutdown 指令已送达旧实例");
}

/// Terminate every myshell.exe process except ourselves. Fallback when the
/// graceful IPC shutdown doesn't complete in time.
#[cfg(windows)]
fn kill_other_myshell_processes() {
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use winapi::um::winnt::PROCESS_TERMINATE;

    let me = std::process::id();
    let mut killed = 0usize;
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            log::error!("[single-instance] CreateToolhelp32Snapshot 失败");
            return;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let name_len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..name_len]);
                if name.eq_ignore_ascii_case("myshell.exe") && entry.th32ProcessID != me {
                    let proc = OpenProcess(PROCESS_TERMINATE, 0, entry.th32ProcessID);
                    if !proc.is_null() && proc != INVALID_HANDLE_VALUE {
                        if TerminateProcess(proc, 1) != 0 {
                            killed += 1;
                            log::warn!(
                                "[single-instance] 已强制结束 myshell.exe (pid={})",
                                entry.th32ProcessID
                            );
                        }
                        CloseHandle(proc);
                    } else {
                        log::warn!(
                            "[single-instance] OpenProcess(pid={}) 失败（权限不足）",
                            entry.th32ProcessID
                        );
                    }
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    if killed == 0 {
        log::warn!("[single-instance] 未找到可结束的 myshell.exe 进程");
    }
}

/// Non-Windows: single-instance detection is Windows-only (the project's
/// convention — see confirm_dangerous_operation in myshell-mcp.rs). Startup
/// always proceeds.
#[cfg(not(windows))]
fn acquire_single_instance_lock() -> Result<bool, String> {
    Ok(true)
}

pub fn run() {
    // GPU acceleration escape hatch — MUST run before any window is created.
    // WebView2 reads WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS once during its
    // environment creation (triggered by the first window), so seeding it here
    // at the very top of run() is what actually makes --disable-gpu take
    // effect. See apply_gpu_pref_if_disabled.
    apply_gpu_pref_if_disabled();

    // Default INFO so log::info!/warn!/error! surface in the daily log file
    // (release dup2's stderr → file) / console (debug). RUST_LOG still wins
    // for ad-hoc verbose debugging (e.g. RUST_LOG=myshell=debug).
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();
    #[cfg(not(debug_assertions))]
    setup_file_logging();

    // Single-instance guard — MUST run before any window/DB/session work so
    // a "quit" decision doesn't leave half-initialized state behind, and a
    // "restart" decision doesn't race the old instance's teardown.
    match acquire_single_instance_lock() {
        Ok(true) => {}
        Ok(false) => {
            log::info!("[startup] another instance is running; user chose to quit");
            return;
        }
        Err(e) => {
            log::error!("[startup] single-instance guard failed: {}", e);
            return;
        }
    }

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
        zmodem_files: Arc::new(Mutex::new(HashMap::new())),
        dek: Arc::new(Mutex::new(None)),
        transfer_cancels: Arc::new(Mutex::new(HashMap::new())),
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
            get_gpu_acceleration_disabled,
            set_gpu_acceleration_disabled,
            get_attachment_dir,
            set_attachment_dir,
            save_screenshot,
            show_in_folder,
            get_command_rules,
            set_command_rules,
            mcp_exec_result,
            write_frontend_log,
            get_connections,
            save_connection,
            delete_connection,
            reset_known_host,
            copy_connection,
            export_connections,
            import_connections,
            get_deleted_connections,
            restore_connection,
            purge_connection,
            purge_all_deleted_connections,
            list_folders,
            save_folder,
            delete_folder,
            rename_folder,
            rename_connection,
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
            ssh_zmodem_finish,
            zmodem_accept_offer,
            zmodem_start_upload,
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
            sftp_cancel_transfer,
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
            list_ai_models,
            get_active_ai_model_id,
            get_active_ai_selection,
            save_ai_model,
            delete_ai_model,
            set_active_ai_model,
            list_supplier_models,
            add_supplier_model,
            remove_supplier_model,
            toggle_ai_model_enabled,
            init_ai_presets,
            fetch_provider_models,
            fetch_models_for_supplier,
            get_feedback_log,
            reveal_path,
            save_feedback_zip,
            clear_feedback_dir,
            mcp_get_binary_path,
            mcp_detect_tools,
            mcp_write_config,
            mcp_remove_config,
        ])
        .setup(|app| {
            // Seed built-in AI model presets on first launch (idempotent).
            {
                let state = app.state::<AppState>();
                if let Err(e) = ai::init_ai_presets_cmd(&state) {
                    log::warn!("[startup] init_ai_presets failed: {}", e);
                }
            }

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

            // ── MCP ↔ GUI IPC bridge ──────────────────────────────────────
            // The MCP server (myshell-mcp.exe) is a separate process that
            // cannot touch the GUI's DOM or window. To let AI agents open
            // connections in the GUI ("open prod-db in MyShell"), we expose
            // a localhost-only TCP listener. The MCP server discovers the
            // port via a file in the config dir, connects, and sends a
            // one-line JSON command. We emit a Tauri event that the frontend
            // listens for, which triggers handleConnect() — the same code
            // path as double-clicking a connection in the sidebar.
            //
            // Security: binds 127.0.0.1 only (no external access). The port
            // file lives in the user's config dir (same ACL as the DB).
            {
                let ipc_handle = app.handle().clone();
                std::thread::spawn(move || {
                    use std::io::{BufRead, BufReader, Write};
                    use std::net::TcpListener;

                    let listener = match TcpListener::bind("127.0.0.1:0") {
                        Ok(l) => l,
                        Err(e) => {
                            log::error!("[ipc] failed to bind TCP listener: {}", e);
                            return;
                        }
                    };
                    let port = match listener.local_addr() {
                        Ok(addr) => addr.port(),
                        Err(e) => {
                            log::error!("[ipc] failed to get local addr: {}", e);
                            return;
                        }
                    };

                    // Write the port file so the MCP server can discover us.
                    if let Some(dir) = dirs::config_dir() {
                        let myshell_dir = dir.join("myshell");
                        let _ = std::fs::create_dir_all(&myshell_dir);
                        let port_file = myshell_dir.join("gui-ipc-port");
                        if let Err(e) = std::fs::write(&port_file, port.to_string()) {
                            log::error!("[ipc] failed to write port file: {}", e);
                        } else {
                            log::info!("[ipc] GUI IPC listener on 127.0.0.1:{}", port);
                        }
                    }

                    // Accept loop — one command per connection (the MCP server
                    // opens a fresh TCP connection for each tool call).
                    for stream in listener.incoming() {
                        let stream = match stream {
                            Ok(s) => s,
                            Err(e) => {
                                log::warn!("[ipc] accept error: {}", e);
                                continue;
                            }
                        };
                        // Set a short timeout so a misbehaving client can't
                        // hang the listener thread forever.
                        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                        let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(5)));

                        let mut reader = BufReader::new(stream.try_clone().unwrap_or(stream));
                        let mut line = String::new();
                        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                            continue;
                        }

                        // Parse the command JSON.
                        let cmd: serde_json::Value = match serde_json::from_str(line.trim()) {
                            Ok(v) => v,
                            Err(e) => {
                                let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"invalid JSON: {}\"}}", e);
                                continue;
                            }
                        };

                        let action = cmd["action"].as_str().unwrap_or("");
                        match action {
                            // Single-instance restart path: a freshly started
                            // MyShell asks us to exit so it can take over.
                            // app.exit(0) flows through RunEvent::ExitRequested
                            // (drain_all_sessions + port-file cleanup). Ack
                            // before exiting — the starter may be gone by the
                            // time we write, which is fine (best-effort).
                            "shutdown" => {
                                let _ = writeln!(reader.get_mut(), "{{\"ok\":true,\"shutting_down\":true}}");
                                log::info!("[ipc] shutdown 收到退出指令（单实例重启）");
                                ipc_handle.exit(0);
                            }
                            "open_connection" => {
                                let conn_id = cmd["connection_id"].as_str().unwrap_or("").to_string();
                                if conn_id.is_empty() {
                                    let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"missing connection_id\"}}");
                                    continue;
                                }
                                // Optional fields the frontend uses to pick tab
                                // type and decide whether to focus an existing tab.
                                let tab_type = cmd["tab_type"].as_str().unwrap_or("auto").to_string();
                                let focus_existing = cmd["focus_existing"].as_bool().unwrap_or(true);

                                // Bring the window to the foreground.
                                if let Some(window) = ipc_handle.get_webview_window("main") {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }

                                // Emit to the frontend — App.tsx listens for
                                // "mcp-gui-command" and calls handleConnect().
                                let payload = serde_json::json!({
                                    "action": "open_connection",
                                    "connection_id": conn_id,
                                    "tab_type": tab_type,
                                    "focus_existing": focus_existing,
                                });
                                match ipc_handle.emit("mcp-gui-command", payload) {
                                    Ok(_) => {
                                        let _ = writeln!(reader.get_mut(), "{{\"ok\":true}}");
                                    }
                                    Err(e) => {
                                        let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"emit failed: {}\"}}", e);
                                    }
                                }
                            }
                            "exec_in_tab" => {
                                // MCP wants to run a command in a visible GUI
                                // terminal tab and get the output back. Unlike
                                // open_connection (fire-and-forget), this blocks
                                // until the frontend reports the result.
                                let conn_id = cmd["connection_id"].as_str().unwrap_or("").to_string();
                                let command = cmd["command"].as_str().unwrap_or("").to_string();
                                let timeout_secs = cmd["timeout"].as_u64().unwrap_or(30);
                                if conn_id.is_empty() || command.is_empty() {
                                    let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"missing connection_id or command\"}}");
                                    continue;
                                }

                                // Generate a unique request_id for this exec.
                                let request_id = uuid::Uuid::new_v4().to_string();

                                // Register a oneshot channel so the
                                // `mcp_exec_result` Tauri command can deliver
                                // the result back to this waiting thread.
                                let (tx, rx) = oneshot::channel::<serde_json::Value>();
                                {
                                    let mut pending = PENDING_EXEC.lock().unwrap();
                                    pending.insert(request_id.clone(), tx);
                                }

                                // Emit to the frontend. We deliberately do NOT
                                // call window.show()/set_focus() here: exec_in_tab
                                // usually reuses an already-connected tab, and
                                // forcing focus on every (often high-frequency)
                                // ssh_exec call yanks the window to the foreground
                                // while the user is interacting with it — the main
                                // cause of the "GUI freezes during MCP use" reports.
                                // open_connection still focuses the window (it is
                                // the action that actually opens a new tab).
                                let payload = serde_json::json!({
                                    "action": "exec_in_tab",
                                    "request_id": request_id,
                                    "connection_id": conn_id,
                                    "command": command,
                                    "timeout": timeout_secs,
                                });
                                if let Err(e) = ipc_handle.emit("mcp-gui-command", payload) {
                                    // Clean up the pending entry on emit failure.
                                    let mut pending = PENDING_EXEC.lock().unwrap();
                                    pending.remove(&request_id);
                                    let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"emit failed: {}\"}}", e);
                                    continue;
                                }

                                // Detach the blocking wait into its own thread so
                                // this accept loop can immediately accept the next
                                // connection. Previously block_on() ran inline and
                                // a single long/hung exec froze every subsequent
                                // IPC request (open_connection, vault_status,
                                // other execs) behind it for up to timeout+10s.
                                let writer = match reader.get_ref().try_clone() {
                                    Ok(w) => w,
                                    Err(e) => {
                                        // Extremely rare (OS can't dup the socket).
                                        // Fall back to inline wait so the caller
                                        // still gets a response instead of a hang.
                                        log::warn!("[ipc] exec_in_tab cannot clone stream ({}); sync wait", e);
                                        let timeout = std::time::Duration::from_secs(timeout_secs + 10);
                                        let result = match tokio::runtime::Runtime::new() {
                                            Ok(rt) => rt.block_on(async move {
                                                match tokio::time::timeout(timeout, rx).await {
                                                    Ok(Ok(val)) => Some(val),
                                                    _ => None,
                                                }
                                            }),
                                            Err(e) => {
                                                log::error!("[ipc] exec_in_tab runtime: {e}");
                                                None
                                            }
                                        };
                                        match result {
                                            Some(result) => {
                                                let _ = writeln!(reader.get_mut(), "{}", result);
                                            }
                                            None => {
                                                let mut pending = PENDING_EXEC.lock().unwrap();
                                                pending.remove(&request_id);
                                                let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"exec timeout ({}s)\"}}", timeout_secs);
                                            }
                                        }
                                        continue;
                                    }
                                };
                                std::thread::spawn(move || {
                                    let timeout = std::time::Duration::from_secs(timeout_secs + 10);
                                    let result = match tokio::runtime::Runtime::new() {
                                        Ok(rt) => rt.block_on(async move {
                                            match tokio::time::timeout(timeout, rx).await {
                                                Ok(Ok(val)) => Some(val),
                                                _ => None,
                                            }
                                        }),
                                        Err(e) => {
                                            log::error!("[ipc] exec_in_tab runtime: {e}");
                                            None
                                        }
                                    };
                                    match result {
                                        Some(result) => {
                                            let _ = writeln!(&writer, "{}", result);
                                        }
                                        None => {
                                            let mut pending = PENDING_EXEC.lock().unwrap();
                                            pending.remove(&request_id);
                                            let _ = writeln!(&writer, "{{\"ok\":false,\"error\":\"exec timeout ({}s)\"}}", timeout_secs);
                                        }
                                    }
                                });
                            }

                            // MCP detected a locked vault and is about to
                            // return a "请解锁" error to the AI. Bring the
                            // window to the front so the password gate is
                            // right in the user's face — without this the
                            // gate can sit in the background while the user
                            // wonders why the AI seems stuck.
                            "focus_unlock" => {
                                if let Some(window) = ipc_handle.get_webview_window("main") {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                                let _ = writeln!(reader.get_mut(), "{{\"ok\":true}}");
                            }

                            // MCP polls the GUI's vault state. Used by the
                            // MCP server's ensure_vault_ready gate: when a
                            // tool is called while the vault is still locked,
                            // the MCP server fails FAST (no silent waiting)
                            // after asking us to focus the unlock gate.
                            // Returns {ok, initialized, unlocked}.
                            "vault_status" => {
                                let app_state = ipc_handle.state::<AppState>();
                                let initialized = vault::is_initialized();
                                let unlocked = app_state
                                    .dek
                                    .lock()
                                    .map(|k| k.is_some())
                                    .unwrap_or(false);
                                let _ = writeln!(
                                    reader.get_mut(),
                                    "{{\"ok\":true,\"initialized\":{},\"unlocked\":{}}}",
                                    initialized, unlocked
                                );
                            }

                            // MCP asks the GUI to decrypt a connection's
                            // credentials (host, username, password, proxy,
                            // keys) using the vault DEK that the user has
                            // unlocked in the GUI. The MCP server itself no
                            // longer holds the DEK or a stored passphrase —
                            // this is the only way it can obtain credentials
                            // for headless SFTP operations. Returns the full
                            // decrypted ConnectionConfig as JSON.
                            "get_connection_secrets" => {
                                let conn_id = cmd["connection_id"].as_str().unwrap_or("").to_string();
                                if conn_id.is_empty() {
                                    let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"missing connection_id\"}}");
                                    continue;
                                }
                                // Get AppState from the Tauri handle.
                                let app_state = ipc_handle.state::<AppState>();
                                // Decrypt using the GUI's DEK.
                                let dek_guard = match app_state.dek.lock() {
                                    Ok(g) => g,
                                    Err(e) => {
                                        let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"DEK 锁定: {}\"}}", e);
                                        continue;
                                    }
                                };
                                let key = match dek_guard.as_ref() {
                                    Some(k) => k,
                                    None => {
                                        let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"保险库未解锁，请在 MyShell GUI 中输入主密码解锁\"}}");
                                        continue;
                                    }
                                };
                                let db_guard = app_state.db.lock();
                                let db_conn = match db_guard {
                                    Ok(db) => db,
                                    Err(e) => {
                                        let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"数据库锁定: {}\"}}", e);
                                        continue;
                                    }
                                };
                                match db::get_connection(&db_conn, key, &conn_id) {
                                    Ok(Some(mut config)) => {
                                        // Resolve password + proxy password from keyring.
                                        if config.auth_method != "key" && config.password.is_none() {
                                            if let Ok(pw) = secrets::get_password(&config.id, key) {
                                                config.password = pw;
                                            }
                                        }
                                        if config.proxy_type != "none" && config.proxy_password.is_none() {
                                            if let Ok(pw) = secrets::get_proxy_password(&config.id, key) {
                                                config.proxy_password = pw;
                                            }
                                        }
                                        let json = serde_json::to_string(&config).unwrap_or_else(|_| "{}".to_string());
                                        let _ = writeln!(reader.get_mut(), "{{\"ok\":true,\"config\":{}}}", json);
                                    }
                                    Ok(None) => {
                                        let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"未找到连接: {}\"}}", conn_id);
                                    }
                                    Err(e) => {
                                        let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"连接查找失败: {}\"}}", e);
                                    }
                                }
                            }

                            // MCP asks the GUI to capture a screenshot of a
                            // terminal tab. Emits an event to the frontend,
                            // which finds the terminal for this connection,
                            // captures the xterm buffer to a PNG data URL, and
                            // calls mcp_exec_result with the data URL. Blocks
                            // until the frontend responds (same pattern as
                            // exec_in_tab).
                            "screenshot_terminal" => {
                                let conn_id = cmd["connection_id"].as_str().unwrap_or("").to_string();
                                if conn_id.is_empty() {
                                    let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"missing connection_id\"}}");
                                    continue;
                                }
                                let request_id = uuid::Uuid::new_v4().to_string();
                                let (tx, rx) = oneshot::channel::<serde_json::Value>();
                                {
                                    let mut pending = PENDING_EXEC.lock().unwrap();
                                    pending.insert(request_id.clone(), tx);
                                }
                                if let Some(window) = ipc_handle.get_webview_window("main") {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                                let payload = serde_json::json!({
                                    "action": "screenshot_terminal",
                                    "request_id": request_id,
                                    "connection_id": conn_id,
                                });
                                if let Err(e) = ipc_handle.emit("mcp-gui-command", payload) {
                                    let mut pending = PENDING_EXEC.lock().unwrap();
                                    pending.remove(&request_id);
                                    let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"emit failed: {}\"}}", e);
                                    continue;
                                }
                                // Detach the wait into its own thread, matching the
                                // exec_in_tab pattern, so a slow screenshot capture
                                // (e.g. a tab that's still connecting) doesn't stall
                                // the accept loop and block other IPC requests.
                                let timeout_secs_sc = 20u64;
                                let writer = match reader.get_ref().try_clone() {
                                    Ok(w) => w,
                                    Err(e) => {
                                        // Fall back to inline wait (rare).
                                        log::warn!("[ipc] screenshot cannot clone stream ({}); sync wait", e);
                                        let timeout = std::time::Duration::from_secs(timeout_secs_sc);
                                        let result = match tokio::runtime::Runtime::new() {
                                            Ok(rt) => rt.block_on(async move {
                                                match tokio::time::timeout(timeout, rx).await {
                                                    Ok(Ok(val)) => Some(val),
                                                    _ => None,
                                                }
                                            }),
                                            Err(e) => {
                                                log::error!("[ipc] screenshot runtime: {e}");
                                                None
                                            }
                                        };
                                        match result {
                                            Some(result) => {
                                                let _ = writeln!(reader.get_mut(), "{}", result);
                                            }
                                            None => {
                                                let mut pending = PENDING_EXEC.lock().unwrap();
                                                pending.remove(&request_id);
                                                let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"screenshot timeout ({}s)\"}}", timeout_secs_sc);
                                            }
                                        }
                                        continue;
                                    }
                                };
                                std::thread::spawn(move || {
                                    let timeout = std::time::Duration::from_secs(timeout_secs_sc);
                                    let result = match tokio::runtime::Runtime::new() {
                                        Ok(rt) => rt.block_on(async move {
                                            match tokio::time::timeout(timeout, rx).await {
                                                Ok(Ok(val)) => Some(val),
                                                _ => None,
                                            }
                                        }),
                                        Err(e) => {
                                            log::error!("[ipc] screenshot runtime: {e}");
                                            None
                                        }
                                    };
                                    match result {
                                        Some(result) => {
                                            let _ = writeln!(&writer, "{}", result);
                                        }
                                        None => {
                                            let mut pending = PENDING_EXEC.lock().unwrap();
                                            pending.remove(&request_id);
                                            let _ = writeln!(&writer, "{{\"ok\":false,\"error\":\"screenshot timeout ({}s)\"}}", timeout_secs_sc);
                                        }
                                    }
                                });
                            }

                            _ => {
                                let _ = writeln!(reader.get_mut(), "{{\"ok\":false,\"error\":\"unknown action: {}\"}}", action);
                            }
                        }
                    }
                });
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            drain_all_sessions(app_handle);
            // Clean up the IPC port file so a stale file doesn't mislead the
            // MCP server into thinking the GUI is still running.
            if let Some(dir) = dirs::config_dir() {
                let port_file = dir.join("myshell").join("gui-ipc-port");
                let _ = std::fs::remove_file(port_file);
            }
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
