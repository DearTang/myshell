//! 壁纸导入：把用户选的本地图片（或旧版存下的 data URL）收进应用自管的
//! `app_local_data/wallpapers/` 目录，返回绝对路径供前端 `convertFileSrc`
//! 生成 asset URL。
//!
//! ## 为什么不能直接用 `convertFileSrc(用户路径)`
//!
//! asset 协议的 scope 来自 `tauri.conf.json` 的 `app.security.assetProtocol.scope`。
//! 只授权壁纸自管目录时，用户把图库放在任意自建目录的话，`convertFileSrc`
//! 照样拼得出 URL，但请求到达 Rust 侧即被 scope 判为 403 —— 前端拿到一个
//! 看起来合法的 URL，壁纸却一次都不渲染。收进自管目录同时根治「原图被
//! 移动/删除后壁纸失效」。
//!
//! ## 为什么文件名不能固定
//!
//! **同一个 background-image URL 浏览器不会重新取图**（Chromium 实测：CSS
//! 里重复设置同一 URL，请求计数不增加），换同格式图片时前端拿到同一个
//! URL，watch 不触发、页面继续显示旧图。文件名带单调后缀，保证每次导入
//! 都产生新 URL。
//!
//! ## 安全边界
//!
//! - 只接受扩展名白名单（与设置页选图过滤器一致），不嗅探内容；
//! - 拒绝目录与空路径；data URL 只认 `data:image/<白名单>;base64,` 前缀；
//! - 有体积上限，防止误选几百 MB 的图把磁盘写满；
//! - 文件名 = 固定前缀 + Unix 毫秒 + 白名单扩展名，**不含任何来自用户
//!   路径的输入**，不存在路径穿越或文件名注入面。
//!
//! （设计对齐 omp-desktop 的 wallpaper.rs，按 MyShell 的目录约定适配。）

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

/// 单张壁纸体积上限。4K 壁纸通常 3–8MB，16MB 与截图读取同口径。
const MAX_WALLPAPER_BYTES: u64 = 16 * 1024 * 1024;

/// 扩展名白名单（小写）。与设置页选图过滤器一致；svg 不收（矢量文本，
/// 浏览器里当壁纸意义不大且体积不可控）。
const WALLPAPER_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"];

/// 壁纸目录名：`app_local_data_dir()/wallpapers`。壁纸是本机资源，
/// 放 local data（Windows %LOCALAPPDATA%，本机不同步）而非 config。
const WALLPAPER_DIR: &str = "wallpapers";

const WALLPAPER_STEM: &str = "wallpaper";

/// 生成本次导入的文件名：固定前缀 + 毫秒时间戳 + 白名单扩展名。
///
/// 后缀只有一个作用：让每次导入落在不同路径上（= 不同 asset URL =
/// 浏览器重新取图）。只含数字，无任何用户输入；时间戳撞车时顺延。
fn wallpaper_file_name(dir: &Path, ext: &str) -> PathBuf {
    let mut millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    loop {
        let candidate = dir.join(format!("{WALLPAPER_STEM}-{millis}.{ext}"));
        if !candidate.exists() {
            return candidate;
        }
        millis += 1;
    }
}

/// 扩展名 → 白名单内的小写形式；不在表内返回 None。
fn extension_of(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.trim().to_ascii_lowercase();
    WALLPAPER_EXTENSIONS.contains(&ext.as_str()).then_some(ext)
}

/// `app_local_data/wallpapers` 的绝对路径（不创建目录）。
fn wallpaper_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|err| format!("无法定位应用本地数据目录：{err}"))?;
    Ok(dir.join(WALLPAPER_DIR))
}

/// 源文件是否已经在壁纸目录内：是则原样返回，不复制。
///
/// 启动迁移的幂等路径与重复选同一张图都会走到这里，避免每次重启都把
/// 4K 图再拷一遍。
fn inside_wallpaper_dir(path: &Path, dir: &Path) -> bool {
    path.starts_with(dir)
}

/// 清掉目录里的其它壁纸文件：换图后磁盘只留一份。
fn clear_other_wallpapers(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path == keep {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let is_wallpaper = name
            .strip_prefix(WALLPAPER_STEM)
            .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('-'));
        if is_wallpaper {
            if let Err(error) = std::fs::remove_file(&path) {
                log::warn!("[wallpaper] 清理旧壁纸失败（可忽略）：{error}");
            }
        }
    }
}

/// 导入纯逻辑（不含 Tauri 依赖，便于单测）：校验源图后复制进 `dir`。
fn import_into_dir(dir: &Path, source_text: &str) -> Result<PathBuf, String> {
    if source_text.is_empty() {
        return Err("图片路径为空".to_string());
    }
    let source = Path::new(source_text);
    let ext = extension_of(source).ok_or_else(|| format!("不是支持的图片格式：{source_text}"))?;
    if !source.is_file() {
        return Err(format!("图片文件不存在：{source_text}"));
    }
    let size = std::fs::metadata(source)
        .map_err(|err| format!("读取图片属性失败：{err}"))?
        .len();
    if size == 0 {
        return Err(format!("图片文件为空：{source_text}"));
    }
    if size > MAX_WALLPAPER_BYTES {
        return Err(format!(
            "图片过大（{:.1} MB，上限 {:.0} MB）：{source_text}",
            size as f64 / 1024.0 / 1024.0,
            MAX_WALLPAPER_BYTES as f64 / 1024.0 / 1024.0
        ));
    }

    if inside_wallpaper_dir(source, dir) {
        return Ok(source.to_path_buf());
    }

    std::fs::create_dir_all(dir).map_err(|err| format!("创建壁纸目录失败：{err}"))?;
    let target = wallpaper_file_name(dir, &ext);
    std::fs::copy(source, &target).map_err(|err| format!("复制壁纸失败：{err}"))?;
    clear_other_wallpapers(dir, &target);
    Ok(target)
}

/// data URL → (扩展名, 字节)。只认 `data:image/<ext>;base64,` 前缀，
/// ext 必须在白名单内。旧版把壁纸 base64 塞 localStorage，升级时经此迁入
/// 自管目录，从此 localStorage 只存 asset 路径。
fn parse_data_url(data_url: &str) -> Result<(String, Vec<u8>), String> {
    let rest = data_url
        .strip_prefix("data:image/")
        .ok_or("不是图片 data URL")?;
    let (meta, payload) = rest
        .split_once(";base64,")
        .ok_or("data URL 缺少 base64 载荷")?;
    let ext = meta.trim().to_ascii_lowercase();
    if !WALLPAPER_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("不支持的图片格式：{ext}"));
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|err| format!("base64 解码失败：{err}"))?;
    if bytes.is_empty() {
        return Err("图片内容为空".to_string());
    }
    if bytes.len() as u64 > MAX_WALLPAPER_BYTES {
        return Err(format!(
            "图片过大（{:.1} MB，上限 {:.0} MB）",
            bytes.len() as f64 / 1024.0 / 1024.0,
            MAX_WALLPAPER_BYTES as f64 / 1024.0 / 1024.0
        ));
    }
    Ok((ext, bytes))
}

/// data URL 迁入自管目录（纯逻辑，便于单测）。
fn import_data_into_dir(dir: &Path, data_url: &str) -> Result<PathBuf, String> {
    let (ext, bytes) = parse_data_url(data_url)?;
    std::fs::create_dir_all(dir).map_err(|err| format!("创建壁纸目录失败：{err}"))?;
    let target = wallpaper_file_name(dir, &ext);
    std::fs::write(&target, &bytes).map_err(|err| format!("写入壁纸失败：{err}"))?;
    clear_other_wallpapers(dir, &target);
    Ok(target)
}

fn import_wallpaper_impl(app: &tauri::AppHandle, path: String) -> Result<String, String> {
    let dir = wallpaper_dir(app)?;
    let stored = import_into_dir(&dir, path.trim())?;
    Ok(stored.to_string_lossy().into_owned())
}

fn import_wallpaper_data_impl(app: &tauri::AppHandle, data_url: String) -> Result<String, String> {
    let dir = wallpaper_dir(app)?;
    let stored = import_data_into_dir(&dir, data_url.trim())?;
    Ok(stored.to_string_lossy().into_owned())
}

/// 导入本地图片为壁纸。前端拿返回的绝对路径调 `convertFileSrc` 生成
/// asset URL —— 落在 `$APPLOCALDATA/wallpapers/**` scope 内，重启后依然放行。
#[tauri::command]
pub async fn import_wallpaper(app: tauri::AppHandle, path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || import_wallpaper_impl(&app, path))
        .await
        .map_err(|err| format!("导入壁纸任务失败：{err}"))?
}

/// 旧版壁纸（base64 data URL）一次性迁入自管目录。
#[tauri::command]
pub async fn import_wallpaper_data(
    app: tauri::AppHandle,
    data_url: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || import_wallpaper_data_impl(&app, data_url))
        .await
        .map_err(|err| format!("迁移壁纸任务失败：{err}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("myshell-wallpaper-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp dir");
        root
    }

    #[test]
    fn extension_whitelist_is_case_insensitive() {
        assert_eq!(extension_of(Path::new("a/b.PNG")), Some("png".to_string()));
        assert_eq!(extension_of(Path::new("x.JPEG")), Some("jpeg".to_string()));
        assert_eq!(extension_of(Path::new("x.avif")), Some("avif".to_string()));
        assert_eq!(extension_of(Path::new("x.jpg ")), Some("jpg".to_string()));
    }

    #[test]
    fn non_image_extensions_are_rejected() {
        assert_eq!(extension_of(Path::new("a.svg")), None);
        assert_eq!(extension_of(Path::new("a.pdf")), None);
        assert_eq!(extension_of(Path::new("noext")), None);
    }

    /// 同一毫秒内两次导入也必须落不同路径：路径不变 = asset URL 不变 =
    /// 浏览器不重新取图，换图表现为「点了没反应」。
    #[test]
    fn wallpaper_file_name_is_unique_per_import() {
        let dir = temp_root("name");
        let first = wallpaper_file_name(&dir, "png");
        std::fs::write(&first, b"x").expect("touch first");
        let second = wallpaper_file_name(&dir, "png");
        assert_ne!(first, second, "two imports must produce different paths");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 文件名只由固定前缀 + 数字 + 白名单扩展名组成，无路径穿越面。
    #[test]
    fn wallpaper_file_name_has_no_user_input() {
        let name = wallpaper_file_name(Path::new("C:\\base"), "png");
        let name = name.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("wallpaper-"), "got {name}");
        assert!(name.ends_with(".png"), "got {name}");
        assert!(
            name.trim_start_matches("wallpaper-")
                .trim_end_matches(".png")
                .chars()
                .all(|c| c.is_ascii_digit()),
            "got {name}"
        );
    }

    /// 端到端：连续导入两张图必须返回不同路径，且旧图被清理。
    #[test]
    fn consecutive_imports_produce_distinct_paths() {
        let root = temp_root("import");
        let src_dir = root.join("src");
        let dst_dir = root.join("dst");
        std::fs::create_dir_all(&src_dir).expect("create src");
        std::fs::create_dir_all(&dst_dir).expect("create dst");

        let a = src_dir.join("a.jpg");
        let b = src_dir.join("b.jpg");
        std::fs::write(&a, b"first-image").expect("write a");
        std::fs::write(&b, b"second-image").expect("write b");

        let first = import_into_dir(&dst_dir, a.to_str().unwrap()).expect("import a");
        let second = import_into_dir(&dst_dir, b.to_str().unwrap()).expect("import b");

        assert_ne!(first, second, "两次导入必须落到不同路径");
        assert_eq!(std::fs::read(&second).unwrap(), b"second-image");
        assert!(!first.exists(), "旧壁纸应被清理，磁盘只留一份");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 已在自管目录内的路径必须原样返回（幂等），不能每次重启都换文件。
    #[test]
    fn reimport_inside_wallpaper_dir_keeps_path() {
        let root = temp_root("idempotent");
        let existing = root.join("wallpaper-1000.jpg");
        std::fs::write(&existing, b"x").expect("write");

        let stored = import_into_dir(&root, existing.to_str().unwrap()).expect("import");
        assert_eq!(stored, existing);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_and_new_names_are_both_cleaned() {
        let dir = temp_root("clean");
        let keep = dir.join("wallpaper-2000.jpg");
        let legacy = dir.join("wallpaper.jpg");
        let stale = dir.join("wallpaper-1000.png");
        let foreign = dir.join("notes.txt");
        for path in [&keep, &legacy, &stale, &foreign] {
            std::fs::write(path, b"x").expect("touch");
        }

        clear_other_wallpapers(&dir, &keep);

        assert!(keep.exists());
        assert!(!legacy.exists(), "旧命名壁纸应被清理");
        assert!(!stale.exists(), "换下的壁纸应被清理");
        assert!(foreign.exists(), "非壁纸文件不动");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// data URL 迁移：白名单格式入库、非图片前缀拒绝、写出的文件可读回。
    #[test]
    fn data_url_migration_roundtrip() {
        use base64::Engine;
        let dir = temp_root("dataurl");

        let payload = base64::engine::general_purpose::STANDARD.encode(b"png-bytes");
        let stored =
            import_data_into_dir(&dir, &format!("data:image/png;base64,{payload}")).expect("migrate");
        assert_eq!(stored.extension().unwrap(), "png");
        assert_eq!(std::fs::read(&stored).unwrap(), b"png-bytes");

        assert!(import_data_into_dir(&dir, "data:image/svg+xml;base64,AAAA").is_err());
        assert!(import_data_into_dir(&dir, "not-a-data-url").is_err());
        assert!(import_data_into_dir(&dir, "data:image/png;base64,!!!").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
