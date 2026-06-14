//! OS keyring wrapper for storing connection passwords out of the SQLite DB.
//!
//! Service name is fixed to `myshell`; the connection's UUID (`id`) is the
//! account key. The value stored is **not** the plaintext password — it's
//! `crypto::encrypt_with_key(master_key, password)`. So a compromise of the
//! OS keyring alone (e.g. another process running as the same user reads
//! Credential Manager) yields only AES-256-GCM ciphertext, useless without
//! the master password.

use crate::crypto;
use keyring::Entry;

const SERVICE: &str = "myshell";

/// Persist an encrypted password for the given connection id. Overwrites
/// any existing value. Empty passwords are treated as "delete if present".
pub fn set_password(id: &str, password: &str, key: &[u8; 32]) -> Result<(), String> {
    if password.is_empty() {
        return delete_password(id);
    }
    let blob = crypto::encrypt_with_key(key, password.as_bytes())?;
    Entry::new(SERVICE, id)
        .map_err(|e| format!("keyring entry: {}", e))?
        .set_password(&blob)
        .map_err(|e| format!("keyring set: {}", e))
}

/// Read and decrypt the password for the given id. Returns `Ok(None)` when
/// no entry exists (first run, never set, or post-migration). A corrupt or
/// wrong-key blob surfaces as an error — caller can decide whether to treat
/// it as "missing" or propagate.
pub fn get_password(id: &str, key: &[u8; 32]) -> Result<Option<String>, String> {
    let entry = Entry::new(SERVICE, id).map_err(|e| format!("keyring entry: {}", e))?;
    match entry.get_password() {
        Ok(blob) => {
            let pt = crypto::decrypt_with_key(key, &blob)?;
            String::from_utf8(pt).map(Some).map_err(|e| format!("password utf8: {}", e))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("keyring get: {}", e)),
    }
}

/// Remove the entry. Idempotent — a missing entry is reported as success.
/// No key required: deleting doesn't expose plaintext.
pub fn delete_password(id: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE, id).map_err(|e| format!("keyring entry: {}", e))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("keyring delete: {}", e)),
    }
}
