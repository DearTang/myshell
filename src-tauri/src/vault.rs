//! Vault file helpers — salt + verifier persistence for master-password
//! unlock. The vault directory sits next to the SQLite DB and holds:
//!
//! - `vault.salt` — 16 random bytes, generated at setup, public. Same salt
//!   feeds PBKDF2 on every unlock. Public is fine — its job is to defeat
//!   rainbow tables, not to be secret.
//! - `vault.verifier` — AES-256-GCM(master_key, VAULT_MAGIC). Used to prove
//!   an unlock attempt derived the right key without keeping the key (or
//!   any password-equivalent) on disk.
//!
//! Both files live in `<config_dir>/myshell/`. Losing both = data is gone
//! even with the correct passphrase (salt mismatch → different key → GCM
//! auth fails). The DB file (connections.db) sits alongside.

use std::path::PathBuf;

const SALT_FILE: &str = "vault.salt";
const VERIFIER_FILE: &str = "vault.verifier";

/// Resolve `<config_dir>/myshell/`. Falls back to `./myshell` if the dirs
/// crate can't find a config dir (rare; usually means running as SYSTEM).
fn vault_dir() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("myshell");
    std::fs::create_dir_all(&path).ok();
    path
}

fn salt_path() -> PathBuf {
    vault_dir().join(SALT_FILE)
}

fn verifier_path() -> PathBuf {
    vault_dir().join(VERIFIER_FILE)
}

/// True iff a salt file is on disk. The frontend uses this to decide
/// whether to render the setup gate or the unlock gate.
pub fn is_initialized() -> bool {
    salt_path().exists() && verifier_path().exists()
}

/// Read the persisted salt. Returns None if either file is missing
/// (treated as "not initialized" — setup flow should run).
pub fn read_salt() -> Option<[u8; 16]> {
    let bytes = std::fs::read(salt_path()).ok()?;
    if bytes.len() != 16 {
        return None;
    }
    let mut arr = [0u8; 16];
    arr.copy_from_slice(&bytes);
    Some(arr)
}

/// Read the persisted verifier blob (base64 string).
pub fn read_verifier() -> Option<String> {
    std::fs::read_to_string(verifier_path()).ok()
}

/// Atomically persist salt + verifier together. Writes go to a temp file
/// first, then rename, so a crash mid-setup can't leave one file present
/// and the other missing.
pub fn write_vault_files(salt: &[u8; 16], verifier: &str) -> Result<(), String> {
    let dir = vault_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;

    // Salt
    let salt_tmp = dir.join(format!("{}.tmp", SALT_FILE));
    std::fs::write(&salt_tmp, salt).map_err(|e| format!("write salt tmp: {}", e))?;
    std::fs::rename(&salt_tmp, salt_path()).map_err(|e| format!("rename salt: {}", e))?;

    // Verifier
    let ver_tmp = dir.join(format!("{}.tmp", VERIFIER_FILE));
    std::fs::write(&ver_tmp, verifier).map_err(|e| format!("write verifier tmp: {}", e))?;
    std::fs::rename(&ver_tmp, verifier_path()).map_err(|e| format!("rename verifier: {}", e))?;

    Ok(())
}

/// Wipe salt + verifier. Used by the "reset vault" flow (UI surfaces this
/// as a destructive action with double confirmation). The connections DB
/// is left in place — but without the right salt+key, it becomes inert.
pub fn wipe_vault_files() -> Result<(), String> {
    let _ = std::fs::remove_file(salt_path());
    let _ = std::fs::remove_file(verifier_path());
    Ok(())
}
