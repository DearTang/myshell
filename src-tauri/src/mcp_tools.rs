//! MCP server lifecycle management for AI tools (Claude Desktop / Opencode / Zcode).
//!
//! Provides: binary path resolution, installed-tool detection, config read/write
//! (with duplicate detection), and keyring-backed passphrase storage.
//!
//! Config formats handled:
//! - Claude Desktop: `<USERPROFILE>/.claude/mcp.json` → `mcpServers.myshell.{command, args, env}`
//! - Opencode:       `<USERPROFILE>/.config/opencode/opencode.json` → `mcp.myshell.{command, enabled, type}`
//! - Zcode:          `<USERPROFILE>/.zcode/cli/config.json` → `mcp.servers.myshell.{command, args, type}`

use std::fs;
use std::path::PathBuf;

/// Detected AI tool with its config path and whether myshell is already configured.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiToolInfo {
    /// "claude" | "opencode" | "zcode"
    pub id: String,
    pub name: String,
    /// Absolute path to the config file
    pub config_path: String,
    /// true if the tool is installed (config file exists)
    pub installed: bool,
    /// true if myshell MCP server is already configured
    pub configured: bool,
}

/// Get the absolute path to `myshell-mcp.exe` (same directory as the running exe).
pub fn mcp_binary_path() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("获取程序路径失败: {}", e))?;
    let parent = exe.parent().ok_or("无法确定程序目录")?;
    let mcp_path = parent.join("myshell-mcp.exe");
    Ok(mcp_path.to_string_lossy().into_owned())
}

/// Detect which AI tools are installed and whether they already have myshell configured.
pub fn mcp_detect_tools() -> Vec<AiToolInfo> {
    let mut tools = Vec::new();

    tools.push(mcp_check_tool_claude());
    tools.push(mcp_check_tool_opencode());
    tools.push(mcp_check_tool_zcode());

    tools
}

fn user_home() -> Option<PathBuf> {
    dirs::home_dir()
}

fn mcp_check_tool_claude() -> AiToolInfo {
    let id = "claude".to_string();
    let name = "Claude Desktop".to_string();
    let config_path = user_home()
        .map(|h| h.join(".claude").join("mcp.json"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let installed = !config_path.is_empty() && PathBuf::from(&config_path).exists();
    let configured = installed && mcp_config_has_myshell_claude(&config_path);
    AiToolInfo { id, name, config_path, installed, configured }
}

fn mcp_check_tool_opencode() -> AiToolInfo {
    let id = "opencode".to_string();
    let name = "Opencode".to_string();
    let config_path = user_home()
        .map(|h| h.join(".config").join("opencode").join("opencode.json"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let installed = !config_path.is_empty() && PathBuf::from(&config_path).exists();
    let configured = installed && mcp_config_has_myshell_opencode(&config_path);
    AiToolInfo { id, name, config_path, installed, configured }
}

fn mcp_check_tool_zcode() -> AiToolInfo {
    let id = "zcode".to_string();
    let name = "ZCode".to_string();
    let config_path = user_home()
        .map(|h| h.join(".zcode").join("cli").join("config.json"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let installed = !config_path.is_empty() && PathBuf::from(&config_path).exists();
    let configured = installed && mcp_config_has_myshell_zcode(&config_path);
    AiToolInfo { id, name, config_path, installed, configured }
}

// ── Duplicate detection ───────────────────────────────────────────────

fn mcp_config_has_myshell_claude(path: &str) -> bool {
    let Ok(content) = fs::read_to_string(path) else { return false };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else { return false };
    json.pointer("/mcpServers/myshell").is_some()
}

fn mcp_config_has_myshell_opencode(path: &str) -> bool {
    let Ok(content) = fs::read_to_string(path) else { return false };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else { return false };
    json.pointer("/mcp/myshell").is_some()
}

fn mcp_config_has_myshell_zcode(path: &str) -> bool {
    let Ok(content) = fs::read_to_string(path) else { return false };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else { return false };
    json.pointer("/mcp/servers/myshell").is_some()
}

// ── Config write ──────────────────────────────────────────────────────

/// Read an existing tool config STRICTLY. A file that exists but isn't valid
/// JSON is an ERROR — the old code silently fell back to `{}` and then wrote
/// that back, wiping the user's whole config (comments-free JSON files can
/// still be mid-edit, corrupted, or written concurrently by the tool itself).
fn read_tool_config(path: &std::path::Path) -> Result<serde_json::Value, String> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(path).map_err(|e| format!("读取配置失败: {}", e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("现有配置不是有效 JSON，已中止写入（原文件未修改）: {}", e))
}

/// Ensure every node on `pointers` exists and is a JSON object. serde_json's
/// IndexMut would silently REPLACE a non-object node with an object — for a
/// config like `"mcp": []` that would discard unrelated data.
fn ensure_object_nodes(json: &mut serde_json::Value, pointers: &[&str]) -> Result<(), String> {
    for pointer in pointers {
        let mut cur = &mut *json;
        for seg in pointer.trim_start_matches('/').split('/') {
            if cur.get(seg).map(|v| !v.is_object()).unwrap_or(false) {
                return Err(format!(
                    "配置节点 {} 不是对象，已中止写入（原文件未修改）",
                    pointer
                ));
            }
            if cur.get(seg).is_none() {
                cur[seg] = serde_json::json!({});
            }
            cur = &mut cur[seg];
        }
    }
    Ok(())
}

/// Atomically write pretty JSON to `path`: temp file in the same directory →
/// fsync → rename over the target (plus a one-shot `.bak` of the original so
/// a bad merge is user-recoverable).
fn write_tool_config(path: &std::path::Path, json: &serde_json::Value) -> Result<(), String> {
    use std::io::Write;
    let pretty = serde_json::to_string_pretty(json).map_err(|e| format!("序列化配置失败: {}", e))?;
    let dir = path.parent().ok_or("配置文件无父目录")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {}", e))?;
    if path.exists() {
        let bak = path.with_extension("json.bak");
        let _ = std::fs::copy(path, &bak);
    }
    let tmp = dir.join(format!(
        ".{}.myshell-tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("config")
    ));
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("创建临时文件失败: {}", e))?;
        f.write_all(pretty.as_bytes())
            .map_err(|e| format!("写入临时文件失败: {}", e))?;
        f.sync_all().map_err(|e| format!("落盘临时文件失败: {}", e))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("替换配置文件失败: {}", e)
    })
}

/// Write myshell MCP config to the specified tool. Returns true if written,
/// false if already configured (skipped to avoid overwrite).
pub fn mcp_write_config(tool_id: &str, binary_path: &str) -> Result<bool, String> {
    match tool_id {
        "claude" => mcp_write_claude(binary_path),
        "opencode" => mcp_write_opencode(binary_path),
        "zcode" => mcp_write_zcode(binary_path),
        _ => Err(format!("未知工具: {}", tool_id)),
    }
}

fn mcp_write_claude(binary_path: &str) -> Result<bool, String> {
    let config_path = user_home()
        .map(|h| h.join(".claude").join("mcp.json"))
        .ok_or("无法确定用户目录")?;

    let mut json = read_tool_config(&config_path)?;

    // Already configured? Skip
    if json.pointer("/mcpServers/myshell").is_some() {
        return Ok(false);
    }

    ensure_object_nodes(&mut json, &["/mcpServers"])?;

    // Insert myshell config (no env — reads from keyring)
    json["mcpServers"]["myshell"] = serde_json::json!({
        "command": binary_path,
        "description": "MyShell SSH/SFTP client — remote command execution and file operations"
    });

    write_tool_config(&config_path, &json)?;
    Ok(true)
}

fn mcp_write_opencode(binary_path: &str) -> Result<bool, String> {
    let config_path = user_home()
        .map(|h| h.join(".config").join("opencode").join("opencode.json"))
        .ok_or("无法确定用户目录")?;

    let mut json = read_tool_config(&config_path)?;

    if json.pointer("/mcp/myshell").is_some() {
        return Ok(false);
    }

    ensure_object_nodes(&mut json, &["/mcp"])?;

    json["mcp"]["myshell"] = serde_json::json!({
        "command": [binary_path],
        "enabled": true,
        "type": "local"
    });

    write_tool_config(&config_path, &json)?;
    Ok(true)
}

fn mcp_write_zcode(binary_path: &str) -> Result<bool, String> {
    let config_path = user_home()
        .map(|h| h.join(".zcode").join("cli").join("config.json"))
        .ok_or("无法确定用户目录")?;

    let mut json = read_tool_config(&config_path)?;

    if json.pointer("/mcp/servers/myshell").is_some() {
        return Ok(false);
    }

    ensure_object_nodes(&mut json, &["/mcp", "/mcp/servers"])?;

    json["mcp"]["servers"]["myshell"] = serde_json::json!({
        "enabled": true,
        "command": binary_path,
        "args": [],
        "type": "stdio"
    });

    write_tool_config(&config_path, &json)?;
    Ok(true)
}

/// Remove myshell from a tool's MCP config.
pub fn mcp_remove_config(tool_id: &str) -> Result<(), String> {
    match tool_id {
        "claude" => mcp_remove_claude(),
        "opencode" => mcp_remove_opencode(),
        "zcode" => mcp_remove_zcode(),
        _ => Err(format!("未知工具: {}", tool_id)),
    }
}

fn remove_json_pointer(json: &mut serde_json::Value, path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 {
        return false;
    }
    let parent_path = &parts[..parts.len() - 1];
    let last = parts[parts.len() - 1];

    let mut cur = json;
    for part in parent_path {
        cur = match cur.get_mut(part) {
            Some(v) => v,
            None => return false,
        };
    }

    match cur {
        serde_json::Value::Object(map) => map.remove(last).is_some(),
        _ => false,
    }
}

fn mcp_remove_claude() -> Result<(), String> {
    let config_path = user_home()
        .map(|h| h.join(".claude").join("mcp.json"))
        .ok_or("无法确定用户目录")?;
    if !config_path.exists() { return Ok(()); }

    let content = fs::read_to_string(&config_path).map_err(|e| format!("读取配置失败: {}", e))?;
    let mut json: serde_json::Value = serde_json::from_str(&content).map_err(|e| format!("解析配置失败: {}", e))?;

    if remove_json_pointer(&mut json, "mcpServers/myshell") {
        write_tool_config(&config_path, &json)?;
    }
    Ok(())
}

fn mcp_remove_opencode() -> Result<(), String> {
    let config_path = user_home()
        .map(|h| h.join(".config").join("opencode").join("opencode.json"))
        .ok_or("无法确定用户目录")?;
    if !config_path.exists() { return Ok(()); }

    let content = fs::read_to_string(&config_path).map_err(|e| format!("读取配置失败: {}", e))?;
    let mut json: serde_json::Value = serde_json::from_str(&content).map_err(|e| format!("解析配置失败: {}", e))?;

    if remove_json_pointer(&mut json, "mcp/myshell") {
        write_tool_config(&config_path, &json)?;
    }
    Ok(())
}

fn mcp_remove_zcode() -> Result<(), String> {
    let config_path = user_home()
        .map(|h| h.join(".zcode").join("cli").join("config.json"))
        .ok_or("无法确定用户目录")?;
    if !config_path.exists() { return Ok(()); }

    let content = fs::read_to_string(&config_path).map_err(|e| format!("读取配置失败: {}", e))?;
    let mut json: serde_json::Value = serde_json::from_str(&content).map_err(|e| format!("解析配置失败: {}", e))?;

    if remove_json_pointer(&mut json, "mcp/servers/myshell") {
        write_tool_config(&config_path, &json)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str, content: Option<&str>) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("myshell-mcp-tools-test"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        match content {
            Some(c) => std::fs::write(&path, c).unwrap(),
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        path
    }

    #[test]
    fn invalid_json_is_an_error_not_a_wipe() {
        let path = temp_file("invalid.json", Some("{ not valid json "));
        let err = read_tool_config(&path).unwrap_err();
        assert!(err.contains("已中止写入"), "unexpected: {err}");
        // The original file is untouched.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not valid json ");
    }

    #[test]
    fn missing_file_yields_empty_object() {
        let path = temp_file("absent-config.json", None);
        assert_eq!(read_tool_config(&path).unwrap(), serde_json::json!({}));
    }

    #[test]
    fn object_nodes_are_created_and_type_checked() {
        let mut json = serde_json::json!({ "other": "kept" });
        ensure_object_nodes(&mut json, &["/mcp", "/mcp/servers"]).unwrap();
        assert!(json["mcp"]["servers"].is_object());
        assert_eq!(json["other"], "kept");

        // An existing non-object node must be REJECTED, not replaced.
        let mut json = serde_json::json!({ "mcp": [] });
        assert!(ensure_object_nodes(&mut json, &["/mcp/servers"]).is_err());
        assert!(json["mcp"].is_array(), "array must survive a failed write");
    }

    #[test]
    fn atomic_write_preserves_and_backs_up() {
        let path = temp_file(
            "atomic.json",
            Some(r#"{ "keep": true, "mcpServers": { "other-tool": { "command": "x" } } }"#),
        );
        let mut json = read_tool_config(&path).unwrap();
        json["mcpServers"]["myshell"] = serde_json::json!({ "command": "myshell-mcp.exe" });
        write_tool_config(&path, &json).unwrap();

        let reread: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(reread["keep"], true, "unrelated keys survive");
        assert_eq!(
            reread["mcpServers"]["other-tool"]["command"], "x",
            "other MCP servers survive"
        );
        assert_eq!(reread["mcpServers"]["myshell"]["command"], "myshell-mcp.exe");
        // No temp file left behind; backup exists.
        let dir = path.parent().unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("myshell-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp file should be renamed away");
        let bak = path.with_extension("json.bak");
        assert!(bak.exists(), "backup of the original should exist");
    }
}
