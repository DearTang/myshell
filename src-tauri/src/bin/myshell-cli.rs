// MyShell CLI — command-line access to saved SSH/SFTP connections for AI
// agents and power users. Shares the same database, vault, and keyring as
// the GUI application.
//
// Usage examples:
//   myshell-cli list --json
//   myshell-cli exec myserver "uname -a" --json
//   myshell-cli sftp ls myserver /var/log --json
//   myshell-cli sftp get myserver /etc/hosts ./hosts
//   myshell-cli test myserver
//   myshell-cli ssh myserver

use clap::{Parser, Subcommand};
use myshell_core::*;
use std::sync::{Arc, Mutex};

#[derive(Parser)]
#[command(name = "myshell-cli", version, about = "MyShell CLI — SSH/SFTP from the command line")]
struct Cli {
    /// Master password for vault unlock (prefer MYSHELL_PASSPHRASE env var)
    #[arg(long, global = true)]
    passphrase: Option<String>,

    /// Output as JSON (machine-readable, AI-friendly)
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all saved connections
    List,

    /// Test a connection's reachability
    Test {
        /// Connection name (or group/name path)
        connection: String,
    },

    /// Execute a command on a remote server (one-shot, no interactive PTY)
    Exec {
        /// Connection name
        connection: String,
        /// Command to execute
        command: String,
        /// Timeout in seconds (default: 30)
        #[arg(long, default_value = "30")]
        timeout: u64,
    },

    /// Interactive SSH terminal session
    Ssh {
        /// Connection name
        connection: String,
    },

    /// SFTP file operations
    Sftp {
        #[command(subcommand)]
        action: SftpAction,
    },

    /// Vault management
    Vault {
        #[command(subcommand)]
        action: VaultAction,
    },
}

#[derive(Subcommand)]
enum SftpAction {
    /// List remote directory
    Ls {
        /// Connection name
        connection: String,
        /// Remote path (default: home directory "~")
        #[arg(default_value = "~")]
        path: String,
    },
    /// Download file(s) from remote
    Get {
        /// Connection name
        connection: String,
        /// Remote file path
        remote: String,
        /// Local destination path
        local: String,
    },
    /// Upload file(s) to remote
    Put {
        /// Connection name
        connection: String,
        /// Local file path
        local: String,
        /// Remote destination directory
        remote: String,
    },
    /// Create remote directory
    Mkdir {
        /// Connection name
        connection: String,
        /// Remote directory path
        path: String,
    },
    /// Remove remote file or directory
    Rm {
        /// Connection name
        connection: String,
        /// Remote path to remove
        path: String,
    },
    /// Rename/move remote file
    Rename {
        /// Connection name
        connection: String,
        /// Old path
        old: String,
        /// New path
        new: String,
    },
}

#[derive(Subcommand)]
enum VaultAction {
    /// Show vault status (initialized / unlocked)
    Status,
}

// ============ Event sink for CLI ============

struct CliSink;

impl EventSink for CliSink {
    fn emit_raw(&self, event: &str, payload: serde_json::Value) {
        // For interactive SSH, ssh_output data goes to stdout raw.
        // Other events are logged to stderr so they don't pollute JSON output.
        match event {
            "ssh_output" => {
                if let Some(data) = payload.get("data").and_then(|d| d.as_array()) {
                    let bytes: Vec<u8> = data.iter().filter_map(|b| b.as_u64().map(|v| v as u8)).collect();
                    use std::io::Write;
                    let _ = std::io::stdout().write_all(&bytes);
                    let _ = std::io::stdout().flush();
                }
            }
            _ => {
                eprintln!("[{}]", event);
            }
        }
    }
}

// ============ Vault unlock ============

fn resolve_passphrase(cli_passphrase: Option<&str>) -> Result<String, String> {
    // Priority: --passphrase flag > MYSHELL_PASSPHRASE env > interactive prompt
    if let Some(p) = cli_passphrase {
        return Ok(p.to_string());
    }
    if let Ok(p) = std::env::var("MYSHELL_PASSPHRASE") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    // Interactive prompt (no echo)
    eprint!("MyShell 主密码: ");
    rpassword::read_password().map_err(|e| format!("读取密码失败: {}", e))
}

fn unlock(state: &AppState, passphrase: &str) -> Result<(), String> {
    // Mirrors the GUI's unlock_vault logic without Tauri State.
    let mut lockout = vault::LockoutState::load();
    if let Some(remaining) = lockout.check_lockout() {
        return Err(format!("密码错误次数过多，请等待 {} 秒后重试", remaining));
    }

    let salt = vault::read_salt().ok_or("Vault 未初始化")?;
    let verifier = vault::read_verifier().ok_or("Vault 未初始化")?;
    let encrypted_dek_opt = vault::read_encrypted_dek();

    let (iterations, _kdf_meta_present) = match vault::read_kdf_meta() {
        Some(meta) => (meta.iterations, true),
        None => (crypto::LEGACY_PBKDF2_ITERATIONS, false),
    };
    let master_key = crypto::derive_master_key_with_iterations(passphrase, &salt, iterations);
    if !crypto::check_verifier(&master_key, &verifier) {
        lockout.record_failure()?;
        return Err("密码错误".to_string());
    }

    let dek: [u8; 32] = match encrypted_dek_opt {
        Some(blob) => {
            let bytes = crypto::decrypt_with_key(&master_key, &blob)?;
            bytes.as_slice().try_into().map_err(|_| "DEK 长度错误")?
        }
        None => master_key,
    };

    lockout.record_success();

    let mut slot = state.dek.lock().map_err(|e| e.to_string())?;
    *slot = Some(dek);
    Ok(())
}

// ============ Connection lookup ============

fn find_connection(state: &AppState, name: &str) -> Result<ConnectionConfig, String> {
    let key = require_dek(state)?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let connections = db::get_all_connections(&db, &key).map_err(|e| e.to_string())?;

    // Match by name, or by group_path/name (e.g. "/prod/myserver")
    connections
        .into_iter()
        .find(|c| {
            c.name == name
                || format!("{}/{}", c.group_path.trim_end_matches('/'), c.name) == name
        })
        .ok_or_else(|| format!("未找到连接: {}（使用 myshell-cli list 查看可用连接）", name))
}

/// Resolve password from keyring and fill into config (mirrors ssh_connect in main.rs).
fn resolve_secrets(state: &AppState, config: &mut ConnectionConfig) -> Result<(), String> {
    if config.auth_method != "key" && config.password.is_none() {
        let key = require_dek(state)?;
        config.password = secrets::get_password(&config.id, &key)?;
    }
    if config.auth_method == "password"
        && config.password.as_deref().map(str::is_empty).unwrap_or(true)
    {
        return Err("未找到保存的密码，请在 GUI 中重新编辑该连接并输入密码".to_string());
    }
    if config.proxy_type != "none" && config.proxy_password.is_none() {
        let key = require_dek(state)?;
        config.proxy_password = secrets::get_proxy_password(&config.id, &key)?;
    }
    Ok(())
}

// ============ Main ============

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Initialize database (same path as GUI: <config_dir>/myshell/connections.db)
    let conn = match db::init_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("数据库初始化失败: {}", e);
            std::process::exit(1);
        }
    };
    let _ = db::migrate_legacy_schema(&conn);

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        ssh_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        ftp_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        local_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        zmodem_files: Mutex::new(std::collections::HashMap::new()),
        dek: Arc::new(Mutex::new(None)),
        transfer_cancels: Arc::new(Mutex::new(std::collections::HashMap::new())),
    };

    // Unlock vault for all other commands
    if vault::is_initialized() {
        let passphrase = match resolve_passphrase(cli.passphrase.as_deref()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        };
        if let Err(e) = unlock(&state, &passphrase) {
            eprintln!("Vault 解锁失败: {}", e);
            std::process::exit(1);
        }
    }

    let result = match cli.command {
        Commands::List => cmd_list(&state, cli.json).await,
        Commands::Test { connection } => cmd_test(&state, &connection, cli.json).await,
        Commands::Exec { connection, command, timeout } => {
            cmd_exec(&state, &connection, &command, timeout, cli.json).await
        }
        Commands::Ssh { connection } => cmd_ssh(&state, &connection).await,
        Commands::Sftp { action } => cmd_sftp(&state, action, cli.json).await,
        Commands::Vault { action } => match action {
            VaultAction::Status => {
                let initialized = vault::is_initialized();
                if cli.json {
                    println!("{}", serde_json::json!({ "initialized": initialized }));
                } else {
                    println!("Vault 已初始化: {}", if initialized { "是" } else { "否" });
                }
                Ok(())
            }
        },
    };

    if let Err(e) = result {
        eprintln!("错误: {}", e);
        std::process::exit(1);
    }
}

// ============ Command implementations ============

async fn cmd_list(state: &AppState, json: bool) -> Result<(), String> {
    let key = require_dek(state)?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let connections = db::get_all_connections(&db, &key).map_err(|e| e.to_string())?;

    if json {
        let items: Vec<serde_json::Value> = connections
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "name": c.name,
                    "host": c.host,
                    "port": c.port,
                    "username": c.username,
                    "auth_method": c.auth_method,
                    "conn_type": c.conn_type,
                    "group_path": c.group_path,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items).unwrap());
    } else {
        if connections.is_empty() {
            println!("（无已保存的连接）");
            return Ok(());
        }
        println!("{:<20} {:<25} {:<6} {:<8} {}", "名称", "主机", "端口", "类型", "分组");
        println!("{}", "─".repeat(75));
        for c in &connections {
            println!(
                "{:<20} {:<25} {:<6} {:<8} {}",
                c.name, c.host, c.port, c.conn_type, c.group_path
            );
        }
    }
    Ok(())
}

async fn cmd_test(state: &AppState, name: &str, json: bool) -> Result<(), String> {
    let mut config = find_connection(state, name)?;
    resolve_secrets(state, &mut config)?;

    let result = match config.conn_type.as_str() {
        "ssh" | "sftp" => ssh::test_connection(state, &config).await,
        "ftp" => ftp::test_connection(&config).await,
        "local" => local::test_connection(&config).await,
        _ => Err(format!("未知连接类型: {}", config.conn_type)),
    };

    if json {
        match &result {
            Ok(msg) => println!("{}", serde_json::json!({ "success": true, "message": msg })),
            Err(e) => println!("{}", serde_json::json!({ "success": false, "error": e })),
        }
    } else {
        match &result {
            Ok(msg) => println!("✓ {}", msg),
            Err(e) => println!("✗ {}", e),
        }
    }
    result.map(|_| ())
}

async fn cmd_exec(
    state: &AppState,
    name: &str,
    command: &str,
    timeout_secs: u64,
    json: bool,
) -> Result<(), String> {
    let mut config = find_connection(state, name)?;
    resolve_secrets(state, &mut config)?;

    if config.conn_type != "ssh" && config.conn_type != "sftp" && !config.conn_type.is_empty() {
        return Err(format!("exec 仅支持 SSH 连接（当前类型: {}）", config.conn_type));
    }

    // Connect (no PTY, no session registration — just dial + auth)
    let handle = ssh::dial_and_authenticate(state, &config, false).await?;

    // Open exec channel and run command
    let mut channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("打开 exec 通道失败: {}", e))?;
    channel
        .exec(true, command)
        .await
        .map_err(|e| format!("exec 失败: {}", e))?;

    // Collect output with timeout
    let collect = async {
        use russh::ChannelMsg;
        let mut stdout: Vec<u8> = Vec::new();
        let mut stderr: Vec<u8> = Vec::new();
        let mut exit_code: Option<u32> = None;
        const MAX_BYTES: usize = 4 * 1024 * 1024;

        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { ref data }) => {
                    if stdout.len() < MAX_BYTES {
                        let room = MAX_BYTES - stdout.len();
                        stdout.extend_from_slice(&data[..data.len().min(room)]);
                    }
                }
                Some(ChannelMsg::ExtendedData { ref data, ext: 1 }) => {
                    if stderr.len() < MAX_BYTES {
                        let room = MAX_BYTES - stderr.len();
                        stderr.extend_from_slice(&data[..data.len().min(room)]);
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    exit_code = Some(exit_status);
                }
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            }
        }
        (stdout, stderr, exit_code)
    };

    let (stdout, stderr, exit_code) = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        collect,
    )
    .await
    .map_err(|_| format!("命令超时（{}秒）", timeout_secs))?;

    // Graceful disconnect
    let _ = handle
        .disconnect(russh::Disconnect::ByApplication, "exec done", "en")
        .await;

    let stdout_str = String::from_utf8_lossy(&stdout);
    let stderr_str = String::from_utf8_lossy(&stderr);
    let code = exit_code.unwrap_or(0);

    if json {
        println!(
            "{}",
            serde_json::json!({
                "exit_code": code,
                "stdout": stdout_str,
                "stderr": stderr_str,
            })
        );
    } else {
        if !stdout_str.is_empty() {
            print!("{}", stdout_str);
        }
        if !stderr_str.is_empty() {
            eprint!("{}", stderr_str);
        }
    }

    if code != 0 {
        std::process::exit(code as i32);
    }
    Ok(())
}

async fn cmd_ssh(state: &AppState, name: &str) -> Result<(), String> {
    let mut config = find_connection(state, name)?;
    resolve_secrets(state, &mut config)?;

    let sink: Arc<dyn EventSink> = Arc::new(CliSink);
    let session_id = ssh::connect(state, sink, config).await?;

    eprintln!("[已连接 session={}，Ctrl+D 退出]", session_id);

    // Clone the session map Arc so the blocking stdin reader can send input
    // without holding a reference to the stack-local AppState.
    let sessions = Arc::clone(&state.ssh_sessions);
    let sid = session_id.clone();
    let stdin_task = tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut buf = [0u8; 4096];
        loop {
            match std::io::stdin().read(&mut buf) {
                Ok(0) => break, // EOF (Ctrl+D)
                Ok(n) => {
                    let data = buf[..n].to_vec();
                    let Ok(map) = sessions.lock() else { break };
                    let Some(session) = map.get(&sid) else { break };
                    if session
                        .command_tx
                        .send(ssh::SessionCommand::Input(data))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let _ = stdin_task.await;
    let _ = ssh::disconnect(state, &session_id).await;
    Ok(())
}

async fn cmd_sftp(state: &AppState, action: SftpAction, json: bool) -> Result<(), String> {
    match action {
        SftpAction::Ls { connection, path } => {
            let mut config = find_connection(state, &connection)?;
            resolve_secrets(state, &mut config)?;
            let handle = ssh::dial_and_authenticate(state, &config, false).await?;
            let sftp = open_sftp(&handle).await?;

            let entries = sftp_list(&sftp, &path).await?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            if json {
                println!("{}", serde_json::to_string_pretty(&entries).unwrap());
            } else {
                for e in &entries {
                    let kind = if e.is_dir { "📁" } else { "📄" };
                    println!("{} {:<40} {:>10}  {}", kind, e.name, e.size, e.permissions);
                }
            }
            Ok(())
        }
        SftpAction::Get { connection, remote, local } => {
            let mut config = find_connection(state, &connection)?;
            resolve_secrets(state, &mut config)?;
            let handle = ssh::dial_and_authenticate(state, &config, false).await?;
            let sftp = open_sftp(&handle).await?;

            sftp_download_file(&sftp, &remote, &local).await?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            if !json {
                println!("✓ 已下载: {} → {}", remote, local);
            }
            Ok(())
        }
        SftpAction::Put { connection, local, remote } => {
            let mut config = find_connection(state, &connection)?;
            resolve_secrets(state, &mut config)?;
            let handle = ssh::dial_and_authenticate(state, &config, false).await?;
            let sftp = open_sftp(&handle).await?;

            sftp_upload_file(&sftp, &local, &remote).await?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            if !json {
                println!("✓ 已上传: {} → {}", local, remote);
            }
            Ok(())
        }
        SftpAction::Mkdir { connection, path } => {
            let mut config = find_connection(state, &connection)?;
            resolve_secrets(state, &mut config)?;
            let handle = ssh::dial_and_authenticate(state, &config, false).await?;
            let sftp = open_sftp(&handle).await?;

            sftp.create_dir(&path).await.map_err(|e| format!("创建目录失败: {}", e))?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            if !json {
                println!("✓ 已创建目录: {}", path);
            }
            Ok(())
        }
        SftpAction::Rm { connection, path } => {
            let mut config = find_connection(state, &connection)?;
            resolve_secrets(state, &mut config)?;
            let handle = ssh::dial_and_authenticate(state, &config, false).await?;
            let sftp = open_sftp(&handle).await?;

            // Try removing as file first, then as directory
            if sftp.remove_file(&path).await.is_err() {
                sftp.remove_dir(&path).await.map_err(|e| format!("删除失败: {}", e))?;
            }
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            if !json {
                println!("✓ 已删除: {}", path);
            }
            Ok(())
        }
        SftpAction::Rename { connection, old, new } => {
            let mut config = find_connection(state, &connection)?;
            resolve_secrets(state, &mut config)?;
            let handle = ssh::dial_and_authenticate(state, &config, false).await?;
            let sftp = open_sftp(&handle).await?;

            sftp.rename(&old, &new).await.map_err(|e| format!("重命名失败: {}", e))?;
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            if !json {
                println!("✓ 已重命名: {} → {}", old, new);
            }
            Ok(())
        }
    }
}

// ============ SFTP helpers (direct russh-sftp, no session map needed) ============

async fn open_sftp(
    handle: &russh::client::Handle<ssh::SshClient>,
) -> Result<russh_sftp::client::SftpSession, String> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("SFTP 通道打开失败: {}", e))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("SFTP 子系统请求失败: {}", e))?;
    russh_sftp::client::SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("SFTP 会话初始化失败: {}", e))
}

async fn sftp_list(
    sftp: &russh_sftp::client::SftpSession,
    path: &str,
) -> Result<Vec<FileEntry>, String> {
    // Resolve ~ like the GUI does
    let resolved = if path == "~" || path.starts_with("~/") {
        let home = sftp
            .canonicalize(".")
            .await
            .map_err(|e| format!("解析主目录失败: {}", e))?;
        let trimmed = home.trim_end_matches('/');
        match path.strip_prefix('~').unwrap_or("").trim_start_matches('/') {
            "" => trimmed.to_string(),
            suffix => format!("{}/{}", trimmed, suffix),
        }
    } else {
        path.to_string()
    };

    let entries = sftp
        .read_dir(&resolved)
        .await
        .map_err(|e| format!("读取目录失败: {}", e))?;
    let mut files = Vec::new();
    for entry in entries {
        let name = entry.file_name().to_string();
        if name == "." || name == ".." {
            continue;
        }
        let file_type = entry.file_type();
        let full_path = if resolved.ends_with('/') {
            format!("{}{}", resolved, name)
        } else {
            format!("{}/{}", resolved, name)
        };
        files.push(FileEntry {
            name,
            path: full_path,
            is_dir: file_type.is_dir(),
            size: entry.metadata().size.unwrap_or(0),
            permissions: format!("{}", entry.metadata().permissions()),
            modified: entry
                .metadata()
                .mtime
                .map(|t| t.to_string())
                .unwrap_or_default(),
        });
    }
    files.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(files)
}

async fn sftp_download_file(
    sftp: &russh_sftp::client::SftpSession,
    remote: &str,
    local: &str,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    // open() = READ
    let mut remote_file = sftp
        .open(remote)
        .await
        .map_err(|e| format!("打开远程文件失败: {}", e))?;

    let mut local_file = tokio::fs::File::create(local)
        .await
        .map_err(|e| format!("创建本地文件失败: {}", e))?;

    // Chunked transfer (32 KiB, same as GUI)
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        use tokio::io::AsyncReadExt;
        let n = remote_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("读取失败: {}", e))?;
        if n == 0 {
            break;
        }
        local_file
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("写入失败: {}", e))?;
    }
    Ok(())
}

async fn sftp_upload_file(
    sftp: &russh_sftp::client::SftpSession,
    local: &str,
    remote_dir: &str,
) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let filename = std::path::Path::new(local)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("无法提取文件名")?;
    let remote_path = if remote_dir.ends_with('/') {
        format!("{}{}", remote_dir, filename)
    } else {
        format!("{}/{}", remote_dir, filename)
    };

    let mut local_file = tokio::fs::File::open(local)
        .await
        .map_err(|e| format!("读取本地文件失败: {}", e))?;
    // create() = CREATE|TRUNCATE|WRITE → overwrite semantics
    let mut remote_file = sftp
        .create(&remote_path)
        .await
        .map_err(|e| format!("创建远程文件失败: {}", e))?;

    let mut buf = vec![0u8; 32 * 1024];
    loop {
        let n = local_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("读取失败: {}", e))?;
        if n == 0 {
            break;
        }
        remote_file
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("写入失败: {}", e))?;
    }
    let _ = remote_file.flush().await;
    Ok(())
}
