//! Vault file helpers — login password verification + data encryption key (DEK).
//!
//! Architecture (方案3):
//! - Login password: only for unlocking the app, stored as verifier
//! - DEK (Data Encryption Key): random 32-byte key for encrypting all data
//! - DEK is encrypted with login password and stored as `dek.enc`
//!
//! Files in `<config_dir>/myshell/`:
//! - `vault.salt` — 16 random bytes for PBKDF2
//! - `vault.verifier` — AES-256-GCM(master_key, VAULT_MAGIC) for password verification
//! - `dek.enc` — AES-256-GCM(master_key, dek) for storing encrypted DEK
//! - `vault.kdf` — JSON recording the PBKDF2 algorithm + iteration count
//! - `lockout.json` — failed attempt tracking for rate limiting
//!
//! KDF migration: pre-0.x vaults were derived with 200k iterations; the
//! current default is 600k. The unlock path detects old vaults (missing
//! `vault.kdf`), derives with 200k to authenticate, then transparently
//! re-encrypts the verifier + DEK under a freshly-derived 600k key and
//! writes `vault.kdf`. From then on it always uses 600k.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

const SALT_FILE: &str = "vault.salt";
const VERIFIER_FILE: &str = "vault.verifier";
const DEK_FILE: &str = "dek.enc";
const KDF_FILE: &str = "vault.kdf";
const LOCKOUT_FILE: &str = "lockout.json";

// Lockout policy constants
pub const MAX_FAILED_ATTEMPTS: u32 = 3;        // Lock after 3 failed attempts
pub const LOCKOUT_DURATION_SECS: u64 = 300;    // 5 minutes
pub const MAX_DAILY_ATTEMPTS: u32 = 30;        // Max 30 failures per day

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

fn dek_path() -> PathBuf {
    vault_dir().join(DEK_FILE)
}

fn kdf_path() -> PathBuf {
    vault_dir().join(KDF_FILE)
}

fn lockout_path() -> PathBuf {
    vault_dir().join(LOCKOUT_FILE)
}

/// On-disk KDF parameters. Persisted so future unlock attempts use the same
/// iteration count the verifier was derived with — without this, bumping the
/// default iteration count would silently invalidate every existing vault.
#[derive(Serialize, Deserialize)]
pub struct KdfMeta {
    pub algorithm: String,
    pub iterations: u32,
}

/// Default KDF params for newly-created vaults. Mirrors `crypto::PBKDF2_ITERATIONS`
/// but exposed here so vault setup can write the metadata without crossing
/// module privacy.
pub fn default_kdf_meta() -> KdfMeta {
    KdfMeta {
        algorithm: "pbkdf2-hmac-sha256".to_string(),
        iterations: 600_000,
    }
}

/// Read the persisted KDF params. Returns None when the file is missing —
/// that includes both fresh installs (no vault yet) and pre-0.x vaults that
/// predate the KDF metadata. Callers must fall back to [`LEGACY_PBKDF2_ITERATIONS`]
/// in the latter case so old verifiers can still be decrypted.
pub fn read_kdf_meta() -> Option<KdfMeta> {
    let text = std::fs::read_to_string(kdf_path()).ok()?;
    serde_json::from_str(&text).ok()
}

/// Persist KDF params atomically.
pub fn write_kdf_meta(meta: &KdfMeta) -> Result<(), String> {
    let dir = vault_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
    let json = serde_json::to_string_pretty(meta)
        .map_err(|e| format!("serialize kdf meta: {}", e))?;
    let tmp = dir.join(format!("{}.tmp", KDF_FILE));
    std::fs::write(&tmp, json).map_err(|e| format!("write kdf tmp: {}", e))?;
    std::fs::rename(&tmp, kdf_path()).map_err(|e| format!("rename kdf: {}", e))
}

/// True iff vault files exist (salt + verifier). DEK may not exist yet
/// if setup is in progress.
pub fn is_initialized() -> bool {
    salt_path().exists() && verifier_path().exists()
}

/// Read the persisted salt. Returns None if file is missing.
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

/// Read the encrypted DEK blob (base64 string).
pub fn read_encrypted_dek() -> Option<String> {
    std::fs::read_to_string(dek_path()).ok()
}

/// Atomically persist salt + verifier together.
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

/// Write the encrypted DEK to disk.
pub fn write_encrypted_dek(encrypted_dek: &str) -> Result<(), String> {
    let dir = vault_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;

    let dek_tmp = dir.join(format!("{}.tmp", DEK_FILE));
    std::fs::write(&dek_tmp, encrypted_dek).map_err(|e| format!("write dek tmp: {}", e))?;
    std::fs::rename(&dek_tmp, dek_path()).map_err(|e| format!("rename dek: {}", e))?;

    Ok(())
}

// ============ Lockout Management ============

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct LockoutState {
    /// Total failed attempts in current lockout cycle (resets on success or lockout expiry)
    pub consecutive_failures: u32,
    /// Total failed attempts today (resets at midnight UTC)
    pub daily_failures: u32,
    /// Unix timestamp of last failed attempt
    pub last_failure_time: Option<u64>,
    /// Unix timestamp when lockout expires (if currently locked)
    pub locked_until: Option<u64>,
    /// Date string for daily reset tracking (YYYY-MM-DD format)
    pub last_failure_date: Option<String>,
}

impl LockoutState {
    pub fn load() -> Self {
        std::fs::read_to_string(lockout_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = vault_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serialize lockout: {}", e))?;
        std::fs::write(lockout_path(), json)
            .map_err(|e| format!("write lockout: {}", e))
    }

    /// Get current Unix timestamp in seconds
    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Get today's date string (YYYY-MM-DD) in UTC. Uses chrono (already a
    /// dependency) so we don't reimplement the Gregorian calendar by hand —
    /// the previous hand-rolled formula produced strings like `2026-day-165`
    /// that broke the daily-failure reset check.
    fn today() -> String {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    }

    /// Check if currently locked out. Returns Some(remaining_seconds) if locked.
    pub fn check_lockout(&mut self) -> Option<u64> {
        let now = Self::now();

        // Reset daily counter if it's a new day
        let today = Self::today();
        if self.last_failure_date.as_ref() != Some(&today) {
            self.daily_failures = 0;
            self.last_failure_date = Some(today);
            let _ = self.save();
        }

        // Check if lockout has expired
        if let Some(locked_until) = self.locked_until {
            if now >= locked_until {
                // Lockout expired, reset consecutive counter
                self.locked_until = None;
                self.consecutive_failures = 0;
                let _ = self.save();
            } else {
                // Still locked
                return Some(locked_until - now);
            }
        }

        None
    }

    /// Record a failed attempt. Returns error message if locked or daily limit exceeded.
    pub fn record_failure(&mut self) -> Result<(), String> {
        let now = Self::now();

        // Check if currently locked
        if let Some(remaining) = self.check_lockout() {
            return Err(format!("密码错误次数过多，请等待 {} 秒后重试", remaining));
        }

        // Check daily limit
        if self.daily_failures >= MAX_DAILY_ATTEMPTS {
            return Err("今日密码错误次数已达上限，请明天再试".to_string());
        }

        // Increment counters
        self.consecutive_failures += 1;
        self.daily_failures += 1;
        self.last_failure_time = Some(now);
        self.last_failure_date = Some(Self::today());

        // Check if should lock out
        if self.consecutive_failures >= MAX_FAILED_ATTEMPTS {
            let locked_until = now + LOCKOUT_DURATION_SECS;
            self.locked_until = Some(locked_until);
            self.save()?;
            return Err(format!(
                "密码连续错误 {} 次，已锁定 {} 分钟",
                MAX_FAILED_ATTEMPTS,
                LOCKOUT_DURATION_SECS / 60
            ));
        }

        self.save()?;
        Err(format!(
            "密码错误（已错 {} 次，{} 次后锁定）",
            self.consecutive_failures,
            MAX_FAILED_ATTEMPTS - self.consecutive_failures
        ))
    }

    /// Record a successful attempt, reset counters
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        // Don't reset daily_failures - keep tracking for the day
        let _ = self.save();
    }
}
