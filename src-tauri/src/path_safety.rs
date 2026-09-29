//! Local path-safety helpers for transferring remote files to disk.
//!
//! Remote filenames are attacker-controlled data (a malicious or compromised
//! SSH/SFTP/FTP server chooses them). Downloading must never let a remote
//! name escape the user-chosen destination directory or follow an existing
//! local symlink out of it. Two layers enforce that:
//!
//! 1. [`validate_component`] — a remote name may form exactly ONE local
//!    filename component. Separators, traversal segments, drive/UNC syntax,
//!    Windows-reserved device names, and Windows-illegal characters are
//!    rejected (NOT silently rewritten — a renamed file could collide with a
//!    real file the user has).
//! 2. [`ensure_no_symlink_components`] — before creating anything, every
//!    existing ancestor (and the leaf) of the target path must be a real
//!    directory/file, never a symlink or reparse point. Without this,
//!    `File::create` would happily follow a pre-planted link.
//!
//! The MCP ZMODEM path has its own sanitizer (rewrites instead of rejecting);
//! the GUI SFTP/FTP transfer paths use this stricter module.

use std::path::Path;

/// Reject a remote entry name that cannot safely become ONE local filename
/// component. Cross-platform: both `/` and `\` are rejected everywhere, so a
/// POSIX-legal name like `..\evil.txt` can't become a Windows path separator
/// on a Windows client.
pub fn validate_component(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("空文件名".to_string());
    }
    if name.len() > 255 {
        return Err("文件名过长（>255 字节）".to_string());
    }
    if name == "." || name == ".." {
        return Err(format!("非法文件名: {name:?}"));
    }
    for c in name.chars() {
        match c {
            '/' | '\\' => return Err(format!("文件名包含路径分隔符: {name:?}")),
            '\0' => return Err("文件名包含 NUL 字节".to_string()),
            c if (c as u32) < 0x20 => {
                return Err(format!("文件名包含控制字符: {name:?}"))
            }
            _ => {}
        }
    }
    #[cfg(windows)]
    windows_extra_checks(name)?;
    #[cfg(not(windows))]
    let _ = name;
    Ok(())
}

/// Windows-only rejections: illegal chars, drive/UNC syntax, trailing
/// dots/spaces (the filesystem silently strips them), and reserved device
/// names (CON, NUL, COM1… — including with an extension like `CON.txt`).
#[cfg(windows)]
fn windows_extra_checks(name: &str) -> Result<(), String> {
    for c in name.chars() {
        if matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            return Err(format!("文件名包含 Windows 非法字符: {name:?}"));
        }
    }
    // Drive-letter / alternate-data-stream syntax (`C:evil`, `evil:ads`).
    if name.contains(':') {
        return Err(format!("文件名包含盘符/流语法: {name:?}"));
    }
    let last = name.chars().last().unwrap();
    if last == '.' || last == ' ' {
        return Err(format!("文件名以点或空格结尾（Windows 会静默剥离）: {name:?}"));
    }
    let stem = name.split('.').next().unwrap_or(name);
    if is_windows_reserved(stem) {
        return Err(format!("文件名使用 Windows 保留设备名: {name:?}"));
    }
    Ok(())
}

/// CON / PRN / AUX / NUL / COM1-9 / LPT1-9, case-insensitive. Superscript
/// digit variants (COM¹) are deliberately not enumerated — the digit check
/// below only accepts ASCII digits, and the reserved list is what MS-DOS
/// actually reserves.
#[cfg(windows)]
fn is_windows_reserved(stem: &str) -> bool {
    const NAMES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
    let upper = stem.to_ascii_uppercase();
    if NAMES.contains(&upper.as_str()) {
        return true;
    }
    for prefix in ["COM", "LPT"] {
        if let Some(num) = upper.strip_prefix(prefix) {
            if num.len() == 1 && num.as_bytes()[0].is_ascii_digit() && num != "0" {
                return true;
            }
        }
    }
    false
}

/// Join a destination directory with already-validated relative components.
/// Pushing one component at a time guarantees by construction that the result
/// stays under `dest` — no string parsing of user data, no separator traps.
pub fn build_path(dest: &Path, components: &[&str]) -> std::path::PathBuf {
    let mut p = dest.to_path_buf();
    for c in components {
        p.push(c);
    }
    p
}

/// Ensure no component of `path` (from the filesystem root down to the leaf)
/// is a symlink or Windows reparse point. Call before `create_dir_all` /
/// `File::create` so an attacker-planted link can't redirect the write
/// outside the destination tree.
pub async fn ensure_no_symlink_components(path: &Path) -> Result<(), String> {
    ensure_no_symlink_components_sync(path)
}

/// Synchronous form of [`ensure_no_symlink_components`].
///
/// The walk is a handful of `symlink_metadata` calls on an already-chosen path,
/// so there is nothing to await. Exists separately because the ZMODEM
/// receiver's `accept_offer` is a synchronous function (it is called from the
/// SSH reader task, and has to hand a `std::fs::File` straight to the disk
/// queue) and still needs the same guarantee the SFTP/FTP paths have.
pub fn ensure_no_symlink_components_sync(path: &Path) -> Result<(), String> {
    let mut cur = std::path::PathBuf::new();
    let mut iter = path.components();
    // Skip the RootDir/Prefix component — that's the drive/mount itself.
    if let Some(first) = iter.next() {
        cur.push(first.as_os_str());
    }
    for comp in iter {
        cur.push(comp.as_os_str());
        let md = std::fs::symlink_metadata(&cur);
        match md {
            Ok(md) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    // Reparse points include symlinks and junctions; is_symlink
                    // only covers tags std recognizes, so check the attribute.
                    if md.file_type().is_symlink()
                        || (md.file_attributes() & 0x400) != 0
                    {
                        return Err(format!(
                            "本地路径包含符号链接/junction，已拒绝写入: {}",
                            cur.display()
                        ));
                    }
                }
                #[cfg(not(windows))]
                if md.file_type().is_symlink() {
                    return Err(format!(
                        "本地路径包含符号链接，已拒绝写入: {}",
                        cur.display()
                    ));
                }
            }
            // Not-yet-existing components are fine — we're about to create them.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("无法检查本地路径 {}: {}", cur.display(), e)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_separators() {
        assert!(validate_component("..").is_err());
        assert!(validate_component(".").is_err());
        assert!(validate_component("").is_err());
        assert!(validate_component("a/b").is_err());
        assert!(validate_component("a\\b").is_err());
        assert!(validate_component("..\\evil.txt").is_err());
        assert!(validate_component("dir/../../evil").is_err());
        assert!(validate_component("a\0b").is_err());
        assert!(validate_component("a\nb").is_err());
    }

    #[test]
    fn accepts_normal_names() {
        assert!(validate_component("file.txt").is_ok());
        assert!(validate_component("数据-资料_2026.tar.gz").is_ok());
        assert!(validate_component("a b c.txt").is_ok());
        // A dash-leading name is odd but harmless.
        assert!(validate_component("-rf").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn rejects_windows_specific() {
        assert!(validate_component("a:b").is_err());
        assert!(validate_component("C:evil").is_err());
        assert!(validate_component("con").is_err());
        assert!(validate_component("CON.txt").is_err());
        assert!(validate_component("com1").is_err());
        assert!(validate_component("lpt4.log").is_err());
        assert!(validate_component("nul ").is_err());
        assert!(validate_component("evil.").is_err());
        assert!(validate_component("a*b").is_err());
        assert!(validate_component("a?b").is_err());
        assert!(validate_component("a<b").is_err());
        assert!(validate_component("a|b").is_err());
        assert!(validate_component("a\"b").is_err());
        // com0 / com10 are NOT reserved
        assert!(validate_component("com0.txt").is_ok());
        assert!(validate_component("com10").is_ok());
    }

    #[cfg(not(windows))]
    #[test]
    fn accepts_posix_legal_windows_illegal() {
        // On POSIX these are legal filenames; the GUI download path rejects
        // them anyway for cross-platform safety, but the unit contract here
        // documents that the Windows-only rules don't fire on Unix.
        assert!(validate_component("a:b").is_ok());
        assert!(validate_component("con").is_ok());
        assert!(validate_component("evil.").is_ok());
    }

    #[test]
    fn build_path_stays_under_dest() {
        let dest = if cfg!(windows) {
            Path::new("C:\\tmp\\dl")
        } else {
            Path::new("/tmp/dl")
        };
        let p = build_path(dest, &["sub", "file.txt"]);
        assert!(p.starts_with(dest));
        assert_eq!(
            p,
            if cfg!(windows) {
                Path::new("C:\\tmp\\dl\\sub\\file.txt")
            } else {
                Path::new("/tmp/dl/sub/file.txt")
            }
        );
    }
}
