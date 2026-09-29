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
const BUNDLE_FILE: &str = "vault.bundle";
const LOCKOUT_FILE: &str = "lockout.json";

// Lockout policy constants
pub const MAX_FAILED_ATTEMPTS: u32 = 3;        // Lock after 3 failed attempts
pub const LOCKOUT_DURATION_SECS: u64 = 300;    // 5 minutes
pub const MAX_DAILY_ATTEMPTS: u32 = 30;        // Max 30 failures per day

/// Bounds for the on-disk PBKDF2 iteration count (see [`clamp_kdf`]).
/// The floor is the legacy count so old vaults still open; the ceiling keeps a
/// tampered/mistyped value from wedging the UI thread.
const MIN_PBKDF2_ITERATIONS: u32 = 100_000;
const MAX_PBKDF2_ITERATIONS: u32 = 10_000_000;

/// Resolve `<config_dir>/myshell/`.
fn vault_dir() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("myshell");
    // `.ok()` here used to swallow the failure entirely. A vault directory that
    // cannot be created (read-only profile, permissions) produced no
    // diagnostic at all: the subsequent read_salt / read_verifier calls simply
    // returned None and the user was told "Vault 未初始化", with no hint that
    // the real cause was an unwritable directory.
    if let Err(e) = std::fs::create_dir_all(&path) {
        log::warn!(
            "[vault] 无法创建保险库目录 {}: {}（后续读写可能失败）",
            path.display(),
            e
        );
    }
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

fn bundle_path() -> PathBuf {
    vault_dir().join(BUNDLE_FILE)
}

/// Read the bundled vault record, or None when the vault predates it (or the
/// file is unreadable — callers then fall back to the legacy per-field files).
pub fn read_vault_bundle() -> Option<VaultBundle> {
    let text = std::fs::read_to_string(bundle_path()).ok()?;
    let b: VaultBundle = serde_json::from_str(&text).ok()?;
    if !b.is_supported_version() {
        return None;
    }
    Some(b)
}

/// Persist salt + verifier + encrypted DEK + KDF metadata as a single atomic
/// record.
///
/// This is the ONLY writer of the master-password state. Callers must build
/// every value first and hand them over together; there is deliberately no way
/// to update one field in isolation, because that is exactly the operation
/// that could leave the vault unopenable.
pub fn write_vault_bundle(
    salt: &[u8; 16],
    verifier: &str,
    dek_enc: Option<&str>,
    kdf: &KdfMeta,
) -> Result<(), String> {
    let dir = vault_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
    let bundle = VaultBundle {
        version: VAULT_BUNDLE_VERSION,
        salt: *salt,
        verifier: verifier.to_string(),
        dek_enc: dek_enc.map(|s| s.to_string()),
        kdf: kdf.clone(),
    };
    let json =
        serde_json::to_string_pretty(&bundle).map_err(|e| format!("serialize vault bundle: {}", e))?;
    // Unique tmp name so two concurrent writers can't rename each other's
    // staging file out from under itself (same hazard as LockoutState::save).
    let tmp = dir.join(format!(".{}.{:016x}.tmp", BUNDLE_FILE, rand::random::<u64>()));
    std::fs::write(&tmp, json).map_err(|e| format!("write vault bundle tmp: {}", e))?;
    std::fs::rename(&tmp, bundle_path()).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename vault bundle: {}", e)
    })?;
    // The bundle is now authoritative and complete. Dropping the legacy
    // per-field files is best-effort cleanup: if we die here the bundle is
    // already correct and readers prefer it, so stale copies are inert.
    for legacy in [SALT_FILE, VERIFIER_FILE, DEK_FILE, KDF_FILE] {
        let _ = std::fs::remove_file(dir.join(legacy));
    }
    Ok(())
}

/// On-disk KDF parameters. Persisted so future unlock attempts use the same
/// iteration count the verifier was derived with — without this, bumping the
/// default iteration count would silently invalidate every existing vault.
#[derive(Serialize, Deserialize, Clone)]
pub struct KdfMeta {
    pub algorithm: String,
    pub iterations: u32,
}

/// Bumped only if the bundle's own layout changes in an incompatible way.
pub const VAULT_BUNDLE_VERSION: u32 = 1;

/// Everything the master password protects, as ONE atomic on-disk record.
///
/// These four values must be mutually consistent: `verifier` is an AEAD tag
/// produced by the same `master_key` that decrypts `dek_enc`, and both are
/// derived from `salt` at the iteration count in `kdf`.
///
/// They used to live in four independent files written in sequence. A crash
/// (or a full disk, or an AV scanner holding `vault.verifier` open so the
/// rename fails) between two of those writes left a mixture no passphrase can
/// open: the OLD password passed `check_verifier` and then failed to decrypt
/// `dek.enc`, while the NEW password failed verification outright. There was
/// no recovery short of deleting the vault and re-entering every credential.
/// One file + tmp/rename makes the whole record commit or not at all.
#[derive(Serialize, Deserialize, Clone)]
pub struct VaultBundle {
    /// Layout version, so a future format change is detected rather than
    /// silently mis-parsed.
    pub version: u32,
    pub salt: [u8; 16],
    pub verifier: String,
    /// `None` for pre-DEK vaults, where the master_key itself was the DEK.
    pub dek_enc: Option<String>,
    pub kdf: KdfMeta,
}

impl VaultBundle {
    /// A bundle from an unknown layout version must be rejected rather than
    /// mis-parsed — a newer build's fields could otherwise be reinterpreted
    /// with the wrong meaning.
    pub fn is_supported_version(&self) -> bool {
        self.version == VAULT_BUNDLE_VERSION
    }
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
///
/// The iteration count is CLAMPED, never trusted verbatim. `vault.kdf` is a
/// plain file in the config dir, and its value feeds the PBKDF2 loop directly:
///   * `iterations: 0` would run zero rounds and return
///     `HMAC-SHA256(passphrase, salt)` as the master key, collapsing the
///     600k-round KDF to a single HMAC for anyone who can write that file.
///   * `iterations: 4294967295` would wedge the UI thread for hours — unlock is
///     a synchronous Tauri command — with no cancel path.
pub fn read_kdf_meta() -> Option<KdfMeta> {
    if let Some(b) = read_vault_bundle() {
        return Some(clamp_kdf(b.kdf));
    }
    let text = std::fs::read_to_string(kdf_path()).ok()?;
    serde_json::from_str(&text).ok().map(clamp_kdf)
}

/// Keep an untrusted on-disk KDF config inside a sane band.
fn clamp_kdf(meta: KdfMeta) -> KdfMeta {
    if meta.iterations < MIN_PBKDF2_ITERATIONS {
        log::warn!(
            "[vault] vault.kdf 迭代数 {} 过低，按 {} 处理",
            meta.iterations,
            MIN_PBKDF2_ITERATIONS
        );
        return KdfMeta {
            algorithm: meta.algorithm,
            iterations: MIN_PBKDF2_ITERATIONS,
        };
    }
    if meta.iterations > MAX_PBKDF2_ITERATIONS {
        log::warn!(
            "[vault] vault.kdf 迭代数 {} 过高，按 {} 处理",
            meta.iterations,
            MAX_PBKDF2_ITERATIONS
        );
        return KdfMeta {
            algorithm: meta.algorithm,
            iterations: MAX_PBKDF2_ITERATIONS,
        };
    }
    meta
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

/// True iff a vault exists. A bundle counts on its own; the legacy
/// salt+verifier pair still counts so pre-bundle vaults keep unlocking.
pub fn is_initialized() -> bool {
    if bundle_path().exists() {
        return true;
    }
    salt_path().exists() && verifier_path().exists()
}

/// Read the persisted salt. Returns None if file is missing.
pub fn read_salt() -> Option<[u8; 16]> {
    if let Some(b) = read_vault_bundle() {
        return Some(b.salt);
    }
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
    if let Some(b) = read_vault_bundle() {
        return Some(b.verifier);
    }
    std::fs::read_to_string(verifier_path()).ok()
}

/// Read the encrypted DEK blob (base64 string). None means a pre-DEK vault
/// where the master_key itself was the DEK.
pub fn read_encrypted_dek() -> Option<String> {
    if let Some(b) = read_vault_bundle() {
        return b.dek_enc;
    }
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
    /// Load the brute-force counters, failing CLOSED.
    ///
    /// This used to be `read_to_string(..).ok().and_then(from_str.ok())
    /// .unwrap_or_default()`, which turned an empty, truncated or unreadable
    /// `lockout.json` into "zero failed attempts" — the exact hazard this very
    /// file's own write path was hardened against (see `save`). An attacker who
    /// can make the file unreadable between attempts, or who can simply rewrite
    /// or delete it (it is unauthenticated plain JSON in the config dir), gets
    /// the counter reset for free.
    ///
    /// `NotFound` is the one benign case — a first run has no file yet — and
    /// returns a fresh default. Anything else is logged and treated as a
    /// conservative state rather than a clean slate.
    pub fn load() -> Self {
        match std::fs::read_to_string(lockout_path()) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                log::warn!(
                    "[vault] 读取锁定状态失败（{}），按保守状态处理（不重置失败计数）",
                    e
                );
                Self::conservative()
            }
            Ok(text) => match serde_json::from_str(&text) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!(
                        "[vault] 锁定状态文件损坏（{}），按保守状态处理（不重置失败计数）",
                        e
                    );
                    Self::conservative()
                }
            },
        }
    }

    /// Stand-in used when the real state cannot be read: counters that block
    /// further guessing until the file becomes readable again. Better a
    /// confusing extra password prompt than an unlimited one.
    fn conservative() -> Self {
        LockoutState {
            consecutive_failures: MAX_FAILED_ATTEMPTS,
            daily_failures: MAX_DAILY_ATTEMPTS,
            last_failure_time: Some(Self::now()),
            // No expiry in the past: `check_lockout` will keep reporting a
            // lockout, which is the fail-closed outcome.
            locked_until: Some(Self::now() + LOCKOUT_DURATION_SECS),
            last_failure_date: Some(Self::today()),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = vault_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serialize lockout: {}", e))?;
        // Atomic write (tmp + rename): a plain fs::write that crashes
        // mid-write leaves a truncated lockout.json that parses as default
        // and silently resets the brute-force counter — an attacker who can
        // crash the process between attempts gets unlimited retries. The
        // unique tmp name avoids a lost-rename race when two concurrent
        // verify_password calls both save (same tmp name would let one
        // rename the other's tmp out from under it).
        let final_path = lockout_path();
        let tmp = dir.join(format!(".lockout.{:016x}.tmp", rand::random::<u64>()));
        std::fs::write(&tmp, &json).map_err(|e| format!("write lockout: {}", e))?;
        std::fs::rename(&tmp, &final_path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("rename lockout: {}", e)
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    // These are deliberately DISK-FREE. `vault_dir()` resolves to the real
    // `<config_dir>/myshell`, so a test that wrote there would overwrite the
    // user's actual vault. Only the on-disk shape is exercised here.

    /// The bundle is now the single source of truth for everything the master
    /// password protects, so its serialized shape must round-trip exactly — a
    /// field that silently fails to serialize would produce a vault that
    /// nobody can open.
    #[test]
    fn vault_bundle_round_trips() {
        let b = VaultBundle {
            version: VAULT_BUNDLE_VERSION,
            salt: [0xABu8; 16],
            verifier: "verifier-blob".into(),
            dek_enc: Some("dek-blob".into()),
            kdf: default_kdf_meta(),
        };
        let json = serde_json::to_string(&b).expect("serialize");
        let back: VaultBundle = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.version, VAULT_BUNDLE_VERSION);
        assert_eq!(back.salt, [0xABu8; 16]);
        assert_eq!(back.verifier, "verifier-blob");
        assert_eq!(back.dek_enc.as_deref(), Some("dek-blob"));
        assert_eq!(back.kdf.iterations, 600_000);
        assert_eq!(back.kdf.algorithm, "pbkdf2-hmac-sha256");
    }

    /// Pre-DEK vaults encrypted columns with the master_key directly and have
    /// no encrypted DEK at all. That must survive as `None` rather than
    /// deserializing into an empty blob that a later decrypt would "fail" on
    /// and break a legitimate legacy unlock.
    #[test]
    fn vault_bundle_supports_missing_dek() {
        let b = VaultBundle {
            version: VAULT_BUNDLE_VERSION,
            salt: [1u8; 16],
            verifier: "v".into(),
            dek_enc: None,
            kdf: default_kdf_meta(),
        };
        let json = serde_json::to_string(&b).expect("serialize");
        let back: VaultBundle = serde_json::from_str(&json).expect("deserialize");
        assert!(back.dek_enc.is_none());
    }

    /// A bundle from an unknown future layout must be rejected, not
    /// mis-parsed — a newer build's fields could otherwise be reinterpreted
    /// with the wrong meaning.
    #[test]
    fn vault_bundle_rejects_unknown_version() {
        let known = VaultBundle {
            version: VAULT_BUNDLE_VERSION,
            salt: [1u8; 16],
            verifier: "v".into(),
            dek_enc: None,
            kdf: default_kdf_meta(),
        };
        assert!(known.is_supported_version());

        let future = VaultBundle {
            version: VAULT_BUNDLE_VERSION + 1,
            ..known.clone()
        };
        assert!(
            !future.is_supported_version(),
            "an unknown layout version must be rejected, not guessed at"
        );

        let ancient = VaultBundle {
            version: 0,
            ..known
        };
        assert!(!ancient.is_supported_version());
    }
}

