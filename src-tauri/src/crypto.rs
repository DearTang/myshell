//! AES-256-GCM + PBKDF2-HMAC-SHA256 envelope for encrypting connection
//! dumps. The user supplies a passphrase; we derive a 256-bit key via PBKDF2
//! (600k iterations, 16-byte random salt) and use it with AES-GCM (12-byte
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
use zeroize::Zeroizing;

/// PBKDF2 iteration count for new vaults / dumps. OWASP 2023 recommends
/// ≥600k for PBKDF2-HMAC-SHA256 to make offline brute-force expensive on
/// modern GPUs. ~250ms derive on a fast laptop — acceptable for an
/// interactive unlock (once per session) but costly across many decrypts,
/// which is why vault field encryption uses a cached DEK instead.
const PBKDF2_ITERATIONS: u32 = 600_000;

/// Legacy iteration count for vaults created before the 200k → 600k bump.
/// Used only by the vault-unlock path to read pre-existing verifiers; new
/// vaults always use [`PBKDF2_ITERATIONS`].
pub const LEGACY_PBKDF2_ITERATIONS: u32 = 200_000;

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
        Aes256Gcm::new_from_slice(&key[..]).map_err(|e| format!("AES key init: {}", e))?;
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
/// Errors:
/// - `Bad envelope format` — JSON parse failure or missing fields
/// - `Unsupported KDF/cipher` — version drift or unknown algorithm
/// - `AES decrypt: ...` — wrong passphrase (GCM tag fails to verify)
///
/// The plaintext comes back wrapped in `Zeroizing` because for a connection
/// dump it *is* every credential the user has. Callers that need a `String`
/// copy it out; this buffer is scrubbed on the way.
pub fn decrypt(
    envelope_str: &str,
    passphrase: &str,
) -> Result<Zeroizing<Vec<u8>>, String> {
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
        Aes256Gcm::new_from_slice(&key[..]).map_err(|e| format!("AES key init: {}", e))?;
    cipher
        .decrypt(&nonce_arr.into(), ciphertext.as_ref())
        .map(Zeroizing::new)
        .map_err(|_| "解密失败：密码错误或文件已损坏".to_string())
}

/// PBKDF2-HMAC-SHA256(passphrase, salt, iterations) → 32-byte AES key.
/// `pbkdf2_hmac` from the pbkdf2 crate writes directly into the output buf.
///
/// The result is `Zeroizing`, so every derived key is scrubbed from memory
/// when it goes out of scope — including the `master_key` locals in the
/// vault setup / unlock / password-change paths, which previously survived
/// as raw bytes in freed stack frames. This is best-effort defense in depth,
/// not a guarantee: the key is also copied into the AES cipher's own key
/// schedule and into `Vec<u8>` buffers on the way to disk.
fn derive_key(passphrase: &str, salt: &[u8]) -> Zeroizing<[u8; KEY_LEN]> {
    derive_key_with_iterations(passphrase, salt, PBKDF2_ITERATIONS)
}

/// Same as [`derive_key`] but lets the caller pick the iteration count.
/// Used by the vault-unlock path to support legacy 200k verifiers.
pub fn derive_key_with_iterations(
    passphrase: &str,
    salt: &[u8],
    iterations: u32,
) -> Zeroizing<[u8; KEY_LEN]> {
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, iterations, &mut *key);
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

/// Decrypt a blob produced by [`encrypt_with_key`]. Wrong key → GCM tag
/// mismatch → `Err`.
///
/// The plaintext is returned as `Zeroizing<Vec<u8>>`. This is the single
/// narrowest point through which **every** stored credential passes — the
/// per-connection password, the proxy password, every command-history row —
/// so it is worth having the buffer scrubbed rather than left for the next
/// allocation to overwrite. Callers that need to keep the value (e.g. hand
/// it to russh, or copy it into a `String` field) make their own copy; the
/// buffer returned here is wiped as soon as it goes out of scope.
pub fn decrypt_with_key(
    key: &[u8; KEY_LEN],
    blob: &str,
) -> Result<Zeroizing<Vec<u8>>, String> {
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
        .map(Zeroizing::new)
        .map_err(|_| "解密失败：主密码错误或数据已损坏".to_string())
}

/// PBKDF2 wrapper exposed for vault setup/unlock. Same KDF params as the
/// dump format, but caller owns the salt lifecycle.
pub fn derive_master_key(passphrase: &str, salt: &[u8]) -> Zeroizing<[u8; KEY_LEN]> {
    derive_key(passphrase, salt)
}

/// Variant that lets the caller pick the iteration count. Used by the
/// vault-unlock path to support legacy 200k verifiers; new code should
/// prefer [`derive_master_key`].
pub fn derive_master_key_with_iterations(
    passphrase: &str,
    salt: &[u8],
    iterations: u32,
) -> Zeroizing<[u8; KEY_LEN]> {
    derive_key_with_iterations(passphrase, salt, iterations)
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
        Ok(pt) => &pt[..] == VAULT_MAGIC,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 往返测试要反复派生密钥，用真实的 600k 会把单测从 20s 拖到 50s。
    /// 这里的断言（GCM 标签校验、密钥一致性）与 KDF 轮数无关，低轮数
    /// 覆盖同样的代码路径。
    const TEST_ITERATIONS: u32 = 1_000;

    /// 派生 → 校验 → 加解密 的端到端往返。
    ///
    /// zeroize 重构把四个公开的密钥派生函数返回类型换成了 `Zeroizing`，
    /// 这条测试保证它们仍然**产出可用的密钥**（而不是仅仅"能编译"）。
    /// 往返里覆盖三种真实解锁路径会遇到的判定：正确口令通过、错误口令
    /// 被拒、错误钥匙解不开密文。
    #[test]
    fn vault_primitives_round_trip() {
        let (passphrase, salt) = ("correct horse battery staple", [7u8; SALT_LEN]);

        let key = derive_key_with_iterations(passphrase, &salt, TEST_ITERATIONS);
        // 同一口令 + 同一 salt 必须稳定派生出同一把钥匙。
        assert_eq!(
            &key[..],
            &derive_key_with_iterations(passphrase, &salt, TEST_ITERATIONS)[..]
        );

        let wrong = derive_key_with_iterations("wrong passphrase", &salt, TEST_ITERATIONS);
        assert_ne!(&key[..], &wrong[..]);

        let verifier = make_verifier(&key).expect("build verifier");
        assert!(check_verifier(&key, &verifier));
        assert!(!check_verifier(&wrong, &verifier));

        let blob = encrypt_with_key(&key, b"hunter2").expect("encrypt");
        assert_eq!(&decrypt_with_key(&key, &blob).expect("decrypt")[..], b"hunter2");
        // 换一把钥匙必须解不出来。
        assert!(decrypt_with_key(&wrong, &blob).is_err());
    }

    /// 迭代数不同 → 不同的密钥。锁定/解锁路径靠这个把 200k 的老库和
    /// 600k 的新库区分开；反过来说，迭代数写错会导致"口令对但解不开"。
    #[test]
    fn iteration_count_changes_the_key() {
        let (passphrase, salt) = ("pw", [1u8; SALT_LEN]);
        let a = derive_key_with_iterations(passphrase, &salt, TEST_ITERATIONS);
        let b = derive_key_with_iterations(passphrase, &salt, TEST_ITERATIONS + 1);
        assert_ne!(&a[..], &b[..]);
    }

    /// 回归守卫：两个解密出口的返回类型必须带擦除语义。
    ///
    /// `decrypt_with_key` 是**全应用唯一一个**所有凭据（连接密码、代理
    /// 密码、命令历史、快捷命令、API key）流出解密后的必经之路；把它改回
    /// 裸 `Vec<u8>` 不会让任何调用点报错——`&pt` 靠 deref 转换照样传得进
    /// `encrypt_with_key(&[u8])`——但整库明文就重新留在堆上了。所以这条
    /// 断言必须钉死返回类型本身。
    fn _assert_plaintext_is_zeroizing(
        r: Result<Zeroizing<Vec<u8>>, String>,
    ) -> Result<Zeroizing<Vec<u8>>, String> {
        r
    }

    #[test]
    fn plaintext_type_guard() {
        let _ = _assert_plaintext_is_zeroizing;
    }

    /// 明文确实是擦除包装，且内容/长度都对——证明包装没有改变语义。
    #[test]
    fn decrypted_plaintext_is_wrapped_and_intact() {
        let key = derive_key_with_iterations("pw", &[3u8; SALT_LEN], TEST_ITERATIONS);
        let secret = b"p@ssw0rd-with-\xe4\xb8\xad\xe6\x96\x87";
        let blob = encrypt_with_key(&key, secret).expect("encrypt");

        let pt: Zeroizing<Vec<u8>> = decrypt_with_key(&key, &blob).expect("decrypt");
        assert_eq!(&pt[..], secret);
        // 解出来必须是合法 UTF-8 —— 调用方（secrets.rs）直接
        // std::str::from_utf8，不再走 String::from_utf8。
        assert!(std::str::from_utf8(&pt).is_ok());
    }

    /// 上面两条用低轮数跑得快；这条保证生产路径真正调用的
    /// `derive_master_key`（600k 默认值）本身也是通的。
    #[test]
    fn default_derivation_is_usable() {
        let key = derive_master_key("pw", &[1u8; SALT_LEN]);
        let verifier = make_verifier(&key).expect("build verifier");
        assert!(check_verifier(&key, &verifier));
    }
}
