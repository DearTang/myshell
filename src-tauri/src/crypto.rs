//! AES-256-GCM + PBKDF2-HMAC-SHA256 envelope for encrypting connection
//! dumps. The user supplies a passphrase; we derive a 256-bit key via PBKDF2
//! (200k iterations, 16-byte random salt) and use it with AES-GCM (12-byte
//! random nonce). Output is JSON with base64-encoded salt/nonce/ciphertext
//! so dumps are safe to email or paste into chat.
//!
//! Threat model: protects against an attacker who reads the exported file
//! but does not have the passphrase. A weak passphrase can still be brute-
//! forced offline — the KDF iterations only slow this down — so the UI
//! enforces a 12+ char minimum and nudges toward mixed char classes.

use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// PBKDF2 iteration count. 200k strikes a balance: ~80ms derive on a modern
/// CPU (tolerable for an interactive open/save), expensive enough to make
/// offline brute force annoying.
const PBKDF2_ITERATIONS: u32 = 200_000;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

#[derive(Serialize, Deserialize)]
struct Envelope {
    version: u32,
    kdf: KdfParams,
    cipher: CipherBlob,
}

#[derive(Serialize, Deserialize)]
struct KdfParams {
    algorithm: String,
    iterations: u32,
    salt: String,
}

#[derive(Serialize, Deserialize)]
struct CipherBlob {
    algorithm: String,
    nonce: String,
    ciphertext: String,
}

/// Encrypt `plaintext` under `passphrase`. Returns a pretty-printed JSON
/// string suitable for writing to disk.
pub fn encrypt(plaintext: &[u8], passphrase: &str) -> Result<String, String> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce);

    let key = derive_key(passphrase, &salt);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("AES key init: {}", e))?;
    let ciphertext = cipher
        .encrypt(&nonce.into(), plaintext)
        .map_err(|e| format!("AES encrypt: {}", e))?;

    let envelope = Envelope {
        version: 1,
        kdf: KdfParams {
            algorithm: "pbkdf2-hmac-sha256".into(),
            iterations: PBKDF2_ITERATIONS,
            salt: B64.encode(salt),
        },
        cipher: CipherBlob {
            algorithm: "aes-256-gcm".into(),
            nonce: B64.encode(nonce),
            ciphertext: B64.encode(&ciphertext),
        },
    };
    serde_json::to_string_pretty(&envelope).map_err(|e| format!("JSON encode: {}", e))
}

/// Decrypt a JSON envelope produced by [`encrypt`]. Returns the raw
/// plaintext bytes; the caller is responsible for deserializing.
///
/// Errors:
/// - `Bad envelope format` — JSON parse failure or missing fields
/// - `Unsupported KDF/cipher` — version drift or unknown algorithm
/// - `AES decrypt: ...` — wrong passphrase (GCM tag fails to verify)
pub fn decrypt(envelope_str: &str, passphrase: &str) -> Result<Vec<u8>, String> {
    let envelope: Envelope =
        serde_json::from_str(envelope_str).map_err(|e| format!("Bad envelope format: {}", e))?;
    if envelope.kdf.algorithm != "pbkdf2-hmac-sha256" {
        return Err(format!("Unsupported KDF: {}", envelope.kdf.algorithm));
    }
    if envelope.cipher.algorithm != "aes-256-gcm" {
        return Err(format!(
            "Unsupported cipher: {}",
            envelope.cipher.algorithm
        ));
    }
    let salt = B64
        .decode(&envelope.kdf.salt)
        .map_err(|e| format!("Bad salt: {}", e))?;
    let nonce_bytes = B64
        .decode(&envelope.cipher.nonce)
        .map_err(|e| format!("Bad nonce: {}", e))?;
    let ciphertext = B64
        .decode(&envelope.cipher.ciphertext)
        .map_err(|e| format!("Bad ciphertext: {}", e))?;
    // AES-GCM's nonce is fixed at 12 bytes — aes-gcm exposes a `Nonce` type
    // backed by a GenericArray<U12>. Convert via try_into to surface length
    // errors cleanly rather than panicking inside the type-system convert.
    let nonce_arr: [u8; NONCE_LEN] = nonce_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Bad nonce length".to_string())?;

    let key = derive_key(passphrase, &salt);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("AES key init: {}", e))?;
    cipher
        .decrypt(&nonce_arr.into(), ciphertext.as_ref())
        .map_err(|_| "解密失败：密码错误或文件已损坏".to_string())
}

/// PBKDF2-HMAC-SHA256(passphrase, salt, iterations) → 32-byte AES key.
/// `pbkdf2_hmac` from the pbkdf2 crate writes directly into the output buf.
fn derive_key(passphrase: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2_hmac::<Sha256>(
        passphrase.as_bytes(),
        salt,
        PBKDF2_ITERATIONS,
        &mut key,
    );
    key
}

// =====================================================================
// Vault primitives — direct key-based AES-256-GCM for field-level
// encryption of SQLite columns. The dump-format functions above take a
// passphrase + run PBKDF2 inside this module; the vault functions take
// an already-derived 32-byte key so db.rs can derive once per session
// and reuse for thousands of column writes without re-KDF'ing.
// =====================================================================

/// Fixed plaintext encrypted by [`make_verifier`]. Decrypting the on-disk
/// verifier and matching this constant proves the master password is
/// correct without storing any password-equivalent material.
pub const VAULT_MAGIC: &[u8] = b"myshell-vault-v1";

/// Encrypt `plaintext` with `key` (AES-256-GCM, random 12B nonce). Output
/// is `base64(nonce || ciphertext || tag)` — a single self-contained blob
/// suitable for a SQLite TEXT column. Each call produces a different
/// ciphertext thanks to the fresh nonce, so two identical hosts won't
/// share a column value (defeats frequency analysis).
pub fn encrypt_with_key(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<String, String> {
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("AES key init: {}", e))?;
    let ciphertext = cipher
        .encrypt(&nonce.into(), plaintext)
        .map_err(|e| format!("AES encrypt: {}", e))?;

    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ciphertext);
    Ok(B64.encode(&blob))
}

/// Decrypt a blob produced by [`encrypt_with_key`]. Returns the raw
/// plaintext bytes. Wrong key → GCM tag mismatch → `Err`.
pub fn decrypt_with_key(key: &[u8; KEY_LEN], blob: &str) -> Result<Vec<u8>, String> {
    let raw = B64.decode(blob).map_err(|e| format!("Bad blob base64: {}", e))?;
    if raw.len() < NONCE_LEN {
        return Err("Blob too short".to_string());
    }
    let (nonce_bytes, ciphertext) = raw.split_at(NONCE_LEN);
    let nonce_arr: [u8; NONCE_LEN] = nonce_bytes
        .try_into()
        .map_err(|_| "Bad nonce slice".to_string())?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("AES key init: {}", e))?;
    cipher
        .decrypt(&nonce_arr.into(), ciphertext)
        .map_err(|_| "解密失败：主密码错误或数据已损坏".to_string())
}

/// PBKDF2 wrapper exposed for vault setup/unlock. Same KDF params as the
/// dump format, but caller owns the salt lifecycle.
pub fn derive_master_key(passphrase: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    derive_key(passphrase, salt)
}

/// Generate a verifier blob from a derived master key. Stored on disk so
/// future unlock attempts can prove the passphrase is correct without
/// keeping the key (or any password-equivalent) on disk.
pub fn make_verifier(key: &[u8; KEY_LEN]) -> Result<String, String> {
    encrypt_with_key(key, VAULT_MAGIC)
}

/// Return true iff `verifier` decrypts under `key` to [`VAULT_MAGIC`].
/// Constant-time comparison is unnecessary here because GCM's tag check
/// already authenticates the plaintext.
pub fn check_verifier(key: &[u8; KEY_LEN], verifier: &str) -> bool {
    match decrypt_with_key(key, verifier) {
        Ok(pt) => pt == VAULT_MAGIC,
        Err(_) => false,
    }
}
