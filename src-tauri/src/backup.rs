//! Version-based backup system for configuration files.
//!
//! On startup, detects if the app version has changed and automatically
//! backs up critical configuration files (database, vault files, lockout state).
//! Backups are stored in `<config_dir>/myshell/backups/<version>/`.
//!
//! Provides rollback functionality in case an upgrade breaks something.

use std::path::PathBuf;
use std::fs;
use serde::{Deserialize, Serialize};

/// Current app version (from Cargo.toml)
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Maximum number of backup versions to keep
const MAX_BACKUPS: usize = 5;

/// Backup manifest file name
const MANIFEST_FILE: &str = "backup_manifest.json";

/// Get the base backup directory
fn backup_base_dir() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("myshell");
    path.push("backups");
    path
}

/// Get the backup directory for a specific version
fn backup_dir(version: &str) -> PathBuf {
    let mut path = backup_base_dir();
    path.push(version);
    path
}

/// Get the current version marker file path
fn version_marker_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("myshell");
    path.push(".version");
    path
}

/// Get the main config directory
fn config_dir() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("myshell");
    path
}

/// List of files to backup
fn backup_files() -> Vec<&'static str> {
    vec![
        "connections.db",
        "vault.salt",
        "vault.verifier",
        "dek.enc",
        "lockout.json",
    ]
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BackupInfo {
    pub version: String,
    pub timestamp: u64,
    pub files: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct BackupManifest {
    pub backups: Vec<BackupInfo>,
    pub last_version: Option<String>,
}

impl BackupManifest {
    fn load() -> Self {
        let path = backup_base_dir().join(MANIFEST_FILE);
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> Result<(), String> {
        let dir = backup_base_dir();
        fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败: {}", e))?;
        let path = dir.join(MANIFEST_FILE);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化备份清单失败: {}", e))?;
        fs::write(&path, json).map_err(|e| format!("写入备份清单失败: {}", e))
    }
}

/// Check if this is a new version and backup if needed
pub fn check_and_backup() -> Result<Option<String>, String> {
    let current_version = APP_VERSION.to_string();
    let marker_path = version_marker_path();

    // Read the last version marker
    let last_version = fs::read_to_string(&marker_path).ok();

    // Check if version changed
    let is_new_version = last_version.as_ref() != Some(&current_version);

    if is_new_version {
        // Backup current config before upgrade
        let backup_result = backup_config(&current_version)?;

        // Update version marker
        fs::write(&marker_path, &current_version)
            .map_err(|e| format!("写入版本标记失败: {}", e))?;

        // Clean up old backups
        cleanup_old_backups()?;

        Ok(Some(backup_result))
    } else {
        Ok(None)
    }
}

/// Backup current configuration files
fn backup_config(new_version: &str) -> Result<String, String> {
    let config = config_dir();
    let backup = backup_dir(new_version);

    // Create backup directory
    fs::create_dir_all(&backup).map_err(|e| format!("创建备份目录失败: {}", e))?;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut backed_up_files = Vec::new();

    // Copy each config file
    for file in backup_files() {
        let src = config.join(file);
        let dst = backup.join(file);

        if src.exists() {
            if let Err(e) = fs::copy(&src, &dst) {
                log::warn!("[backup] 警告: 无法备份 {}: {}", file, e);
            } else {
                backed_up_files.push(file.to_string());
            }
        }
    }

    // Update manifest
    let mut manifest = BackupManifest::load();

    // Check if this version already has a backup
    if let Some(existing) = manifest.backups.iter_mut().find(|b| b.version == new_version) {
        // Update existing backup info
        existing.timestamp = timestamp;
        existing.files = backed_up_files.clone();
    } else {
        // Add new backup entry
        manifest.backups.push(BackupInfo {
            version: new_version.to_string(),
            timestamp,
            files: backed_up_files.clone(),
        });
    }

    manifest.last_version = Some(new_version.to_string());
    manifest.save()?;

    let msg = format!(
        "已备份配置文件到 {} ({} 个文件)",
        new_version,
        backed_up_files.len()
    );
    log::info!("[backup] {}", msg);

    Ok(msg)
}

/// Clean up old backups, keeping only the most recent ones
fn cleanup_old_backups() -> Result<(), String> {
    let mut manifest = BackupManifest::load();

    // Sort by timestamp descending (newest first)
    manifest.backups.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    // Keep only MAX_BACKUPS
    if manifest.backups.len() > MAX_BACKUPS {
        let to_remove: Vec<_> = manifest.backups.drain(MAX_BACKUPS..).collect();

        for backup in to_remove {
            let dir = backup_dir(&backup.version);
            if let Err(e) = fs::remove_dir_all(&dir) {
                log::warn!("[backup] 警告: 无法删除旧备份 {}: {}", backup.version, e);
            } else {
                log::info!("[backup] 已清理旧备份: {}", backup.version);
            }
        }

        manifest.save()?;
    }

    Ok(())
}

/// List available backups for rollback
pub fn list_backups() -> Result<Vec<BackupInfo>, String> {
    let manifest = BackupManifest::load();
    Ok(manifest.backups)
}

/// True for a syntactically safe version string (ASCII digits and dots only,
/// no leading/trailing dot, no empty `.` segments). Used to gate path
/// construction so a user-supplied version can never traverse out of the
/// backups directory via `..`, separators, or an absolute path.
fn is_valid_version(version: &str) -> bool {
    if version.is_empty() || version.starts_with('.') || version.ends_with('.') {
        return false;
    }
    version.chars().all(|c| c.is_ascii_digit() || c == '.')
        && version.split('.').all(|seg| !seg.is_empty())
}

/// Rollback to a specific version
pub fn rollback(version: &str) -> Result<String, String> {
    // Defense-in-depth: `version` flows into a path. The manifest lookup
    // below already restricts it to known backup versions (which are always
    // clean CARGO_PKG_VERSION strings), but reject anything that isn't a
    // dotted version number up front so the path can never escape the
    // backups dir via "..", separators, or an absolute path.
    if !is_valid_version(version) {
        return Err(format!("无效的备份版本: {}", version));
    }

    let manifest = BackupManifest::load();
    let backup_info = manifest.backups.iter()
        .find(|b| b.version == version)
        .ok_or_else(|| format!("找不到版本 {} 的备份信息", version))?;

    let backup = backup_dir(version);
    let config = config_dir();

    if !backup.exists() {
        return Err(format!("备份版本 {} 不存在", version));
    }

    let mut restored_files = Vec::new();

    // Restore each file
    for file in &backup_info.files {
        let src = backup.join(file);
        let dst = config.join(file);

        if src.exists() {
            fs::copy(&src, &dst)
                .map_err(|e| format!("恢复 {} 失败: {}", file, e))?;
            restored_files.push(file.clone());
        }
    }

    // Write the *current* app version (not `"{version} (rolled back)"`) so
    // the next launch's version check is stable — the old suffix never
    // matched APP_VERSION and triggered a fresh backup on every startup.
    // The rolled-back state is already captured by the restored files.
    fs::write(version_marker_path(), APP_VERSION)
        .map_err(|e| format!("更新版本标记失败: {}", e))?;
    log::info!("[backup] rolled back config to version {}", version);

    let msg = format!(
        "已回退到版本 {}，恢复了 {} 个文件",
        version,
        restored_files.len()
    );
    log::info!("[backup] {}", msg);

    Ok(msg)
}

/// Get the previous version (for quick rollback)
pub fn get_previous_version() -> Option<String> {
    let manifest = BackupManifest::load();

    // Skip current version and get the previous one
    let current = APP_VERSION;
    manifest.backups.iter()
        .filter(|b| b.version != current)
        .max_by_key(|b| b.timestamp)
        .map(|b| b.version.clone())
}
