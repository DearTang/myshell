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
//! ## 文件名固定，缓存由前端查询串兜住
//!
//! **同一个 background-image URL 浏览器不会重新取图**（Chromium 实测：CSS
//! 里重复设置同一 URL，请求计数不增加）。早期版本用「时间戳文件名」保证
//! 每次导入都是新 URL，代价是 localStorage 里的旧 URL 会指向被清理的文件：
//! 启动预检 404 → 壁纸静默丢失（实测每次启动一条
//! `File does not exist at path`，用户报「设完当时生效、重启就不见了」）。
//!
//! 现在文件名固定为 `wallpaper.<ext>`：URL 与磁盘路径永久一一对应，换图是
//! 覆盖写，旧 URL 永远有效。缓存问题改由前端拼 `?v=<时间戳>` 查询串解决
//! （见 `appearance.ts` 的 `wallpaperUrl`）—— 查询串不参与 asset scope 的
//! 路径判定。
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

/// 本次导入的文件名：固定 `wallpaper.<ext>`。
///
/// 路径稳定 = asset URL 稳定（换图靠覆盖写，前端用 `?v=` 查询串绕过缓存）。
/// 只含固定前缀与白名单扩展名，无任何用户输入。
fn wallpaper_file_name(ext: &str) -> PathBuf {
    PathBuf::from(format!("{WALLPAPER_STEM}.{ext}"))
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
    let target = dir.join(wallpaper_file_name(&ext));
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
    let target = dir.join(wallpaper_file_name(&ext));
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

/// 当前壁纸的绝对路径：优先固定文件名（新逻辑导入的那张），没有则退回
/// 目录里修改时间最新的白名单图片（兼容升级前的时间戳命名）。
///
/// 前端启动时若 `localStorage` 里的壁纸 URL 不可达，用它自愈 —— 壁纸配置
/// 只存在 WebView 的 localStorage 里，而进程退出不保证 Chromium 把最后一批
/// 写入刷进 LevelDB（实测：设置后重启读回旧值）。磁盘文件才是可靠来源。
fn pick_current_wallpaper(dir: &Path) -> Option<PathBuf> {
    for ext in WALLPAPER_EXTENSIONS {
        let candidate = dir.join(wallpaper_file_name(ext));
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_file() || extension_of(&path).is_none() {
            continue;
        }
        let Some(modified) = entry.metadata().ok().and_then(|m| m.modified().ok()) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(best, _)| modified > *best) {
            newest = Some((modified, path));
        }
    }
    newest.map(|(_, path)| path)
}

fn current_wallpaper_impl(app: &tauri::AppHandle) -> Result<Option<String>, String> {
    let dir = wallpaper_dir(app)?;
    Ok(pick_current_wallpaper(&dir).map(|path| path.to_string_lossy().into_owned()))
}

/// 读取当前壁纸路径（无壁纸时返回 null）。供前端在 localStorage 里的
/// 壁纸 URL 失效时自愈。
#[tauri::command]
pub async fn current_wallpaper(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || current_wallpaper_impl(&app))
        .await
        .map_err(|err| format!("读取壁纸路径失败：{err}"))?
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

    /// 文件名必须固定：换图走覆盖写，URL 才不会指向被清理的旧文件。
    #[test]
    fn wallpaper_file_name_is_stable_per_extension() {
        assert_eq!(wallpaper_file_name("png"), wallpaper_file_name("png"));
        assert_ne!(wallpaper_file_name("png"), wallpaper_file_name("jpg"));
        assert_eq!(
            wallpaper_file_name("png"),
            PathBuf::from("wallpaper.png"),
            "文件名只由固定前缀与扩展名组成"
        );
    }

    /// 文件名只由固定前缀 + 白名单扩展名组成，无路径穿越面。
    #[test]
    fn wallpaper_file_name_has_no_user_input() {
        let name = wallpaper_file_name("png");
        let name = name.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, "wallpaper.png");
    }

    /// 端到端：连续导入两张图落到同一路径（覆盖写），内容为第二张，
    /// 目录里只留一份壁纸。
    #[test]
    fn consecutive_imports_overwrite_same_path() {
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

        assert_eq!(first, second, "换图必须是同一路径的覆盖写");
        assert_eq!(std::fs::read(&second).unwrap(), b"second-image");
        let left: Vec<_> = std::fs::read_dir(&dst_dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from("wallpaper.jpg")]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 已在自管目录内的路径必须原样返回（幂等），不能每次重启都换文件。
    #[test]
    fn reimport_inside_wallpaper_dir_keeps_path() {
        let root = temp_root("idempotent");
        let existing = root.join("wallpaper.jpg");
        std::fs::write(&existing, b"x").expect("write");

        let stored = import_into_dir(&root, existing.to_str().unwrap()).expect("import");
        assert_eq!(stored, existing);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_and_fixed_names_are_cleaned() {
        let dir = temp_root("clean");
        let keep = dir.join("wallpaper.jpg");
        let legacy = dir.join("wallpaper-2000.png");
        let other_ext = dir.join("wallpaper.jpeg");
        let foreign = dir.join("notes.txt");
        for path in [&keep, &legacy, &other_ext, &foreign] {
            std::fs::write(path, b"x").expect("touch");
        }

        clear_other_wallpapers(&dir, &keep);

        assert!(keep.exists());
        assert!(!legacy.exists(), "旧时间戳命名的壁纸应被清理");
        assert!(!other_ext.exists(), "换扩展名后的旧壁纸应被清理");
        assert!(foreign.exists(), "非壁纸文件不动");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 固定名优先于更新的时间戳文件：新逻辑导入的那张就是当前壁纸。
    #[test]
    fn pick_current_wallpaper_prefers_fixed_name() {
        let dir = temp_root("pick-fixed");
        let fixed = dir.join("wallpaper.png");
        let legacy = dir.join("wallpaper-9999.jpg");
        std::fs::write(&fixed, b"x").expect("touch fixed");
        std::fs::write(&legacy, b"x").expect("touch legacy");
        assert_eq!(pick_current_wallpaper(&dir), Some(fixed));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 没有固定名时退回扫描：时间戳命名的旧壁纸仍能被找回（升级兼容）。
    #[test]
    fn pick_current_wallpaper_falls_back_to_legacy_name() {
        let dir = temp_root("pick-legacy");
        let legacy = dir.join("wallpaper-1000.jpg");
        std::fs::write(&legacy, b"x").expect("touch");
        assert_eq!(pick_current_wallpaper(&dir), Some(legacy));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 目录不存在 / 只有非图片文件 → None（前端据此保持无壁纸态）。
    #[test]
    fn pick_current_wallpaper_none_without_images() {
        let dir = temp_root("pick-none");
        std::fs::write(dir.join("notes.txt"), b"x").expect("touch");
        assert_eq!(pick_current_wallpaper(&dir), None);
        assert_eq!(pick_current_wallpaper(&dir.join("missing")), None);
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
