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

    // Read existing or create new
    let mut json = if config_path.exists() {
        let content = fs::read_to_string(&config_path).map_err(|e| format!("读取配置失败: {}", e))?;
        serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Already configured? Skip
    if json.pointer("/mcpServers/myshell").is_some() {
        return Ok(false);
    }

    // Ensure mcpServers exists
    if json.get("mcpServers").is_none() {
        json["mcpServers"] = serde_json::json!({});
    }

    // Insert myshell config (no env — reads from keyring)
    json["mcpServers"]["myshell"] = serde_json::json!({
        "command": binary_path,
        "description": "MyShell SSH/SFTP client — remote command execution and file operations"
    });

    // Write back
    fs::write(&config_path, serde_json::to_string_pretty(&json).unwrap())
        .map_err(|e| format!("写入配置失败: {}", e))?;

    Ok(true)
}

fn mcp_write_opencode(binary_path: &str) -> Result<bool, String> {
    let config_path = user_home()
        .map(|h| h.join(".config").join("opencode").join("opencode.json"))
        .ok_or("无法确定用户目录")?;

    let mut json = if config_path.exists() {
        let content = fs::read_to_string(&config_path).map_err(|e| format!("读取配置失败: {}", e))?;
        serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if json.pointer("/mcp/myshell").is_some() {
        return Ok(false);
    }

    if json.get("mcp").is_none() {
        json["mcp"] = serde_json::json!({});
    }

    json["mcp"]["myshell"] = serde_json::json!({
        "command": [binary_path],
        "enabled": true,
        "type": "local"
    });

    fs::write(&config_path, serde_json::to_string_pretty(&json).unwrap())
        .map_err(|e| format!("写入配置失败: {}", e))?;

    Ok(true)
}

fn mcp_write_zcode(binary_path: &str) -> Result<bool, String> {
    let config_path = user_home()
        .map(|h| h.join(".zcode").join("cli").join("config.json"))
        .ok_or("无法确定用户目录")?;

    let mut json = if config_path.exists() {
        let content = fs::read_to_string(&config_path).map_err(|e| format!("读取配置失败: {}", e))?;
        serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if json.pointer("/mcp/servers/myshell").is_some() {
        return Ok(false);
    }

    // Ensure mcp.servers exists
    if json.get("mcp").is_none() {
        json["mcp"] = serde_json::json!({ "servers": serde_json::json!({}) });
    } else if json["mcp"].get("servers").is_none() {
        json["mcp"]["servers"] = serde_json::json!({});
    }

    json["mcp"]["servers"]["myshell"] = serde_json::json!({
        "enabled": true,
        "command": binary_path,
        "args": [],
        "type": "stdio"
    });

    fs::write(&config_path, serde_json::to_string_pretty(&json).unwrap())
        .map_err(|e| format!("写入配置失败: {}", e))?;

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
        fs::write(&config_path, serde_json::to_string_pretty(&json).unwrap())
            .map_err(|e| format!("写入配置失败: {}", e))?;
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
        fs::write(&config_path, serde_json::to_string_pretty(&json).unwrap())
            .map_err(|e| format!("写入配置失败: {}", e))?;
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
        fs::write(&config_path, serde_json::to_string_pretty(&json).unwrap())
            .map_err(|e| format!("写入配置失败: {}", e))?;
    }
    Ok(())
}
