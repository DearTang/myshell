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
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(name = "myshell-cli", version, about = "MyShell CLI — SSH/SFTP from the command line")]
struct Cli {
    /// Master password for vault unlock (prefer MYSHELL_PASSPHRASE env var)
    #[arg(long, global = true)]
    passphrase: Option<String>,

    /// Output as JSON (machine-readable, AI-friendly)
    #[arg(long, global = true)]
    json: bool,

    /// Acknowledge a dangerous operation and run it anyway.
    ///
    /// `exec`, `sftp rm`, `sftp put` and `sftp rename` are checked against the
    /// same command-confirmation rules the GUI and MCP server use. A hit is
    /// REFUSED without this flag — there is no dialog on a terminal, so the
    /// opt-in has to be explicit and greppable.
    #[arg(long, global = true, short = 'y')]
    yes: bool,

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
        /// Timeout in seconds (default: 60, max 3600=1h). Raise for long jobs
        /// (apt upgrade, git clone, big rsync). For tasks >5min, prefer
        /// running with `nohup ... &` on the remote and tailing a log file
        /// — this tool is synchronous and blocks until the command exits.
        #[arg(long, default_value = "60", value_parser = clap::value_parser!(u64).range(1..=3600))]
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

/// Resolve the master password. Returns `Zeroizing` so the copy the CLI
/// holds between here and `unlock` is scrubbed when the command ends —
/// including the one read straight out of `MYSHELL_PASSPHRASE`, which an
/// agent's environment may keep around long after the process exits.
fn resolve_passphrase(cli_passphrase: Option<&str>) -> Result<Zeroizing<String>, String> {
    // Priority: --passphrase flag > MYSHELL_PASSPHRASE env > interactive prompt
    if let Some(p) = cli_passphrase {
        return Ok(Zeroizing::new(p.to_string()));
    }
    if let Ok(p) = std::env::var("MYSHELL_PASSPHRASE") {
        if !p.is_empty() {
            return Ok(Zeroizing::new(p));
        }
    }
    // Interactive prompt (no echo)
    eprint!("MyShell 主密码: ");
    rpassword::read_password()
        .map(Zeroizing::new)
        .map_err(|e| format!("读取密码失败: {}", e))
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

    let dek: Zeroizing<[u8; 32]> = match encrypted_dek_opt {
        Some(blob) => {
            let bytes = crypto::decrypt_with_key(&master_key, &blob)?;
            Zeroizing::new(bytes.as_slice().try_into().map_err(|_| "DEK 长度错误")?)
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
        config.password = secrets::get_password(&config.id, &key)?.map(|p| p.to_string());
    }
    if config.auth_method == "password"
        && config.password.as_deref().map(str::is_empty).unwrap_or(true)
    {
        return Err("未找到保存的密码，请在 GUI 中重新编辑该连接并输入密码".to_string());
    }
    if config.proxy_type != "none" && config.proxy_password.is_none() {
        let key = require_dek(state)?;
        config.proxy_password =
        secrets::get_proxy_password(&config.id, &key)?.map(|p| p.to_string());
    }
    // Key auth: `get_all_connections` deliberately never returns the PEM (see
    // `ConnectionConfig::private_key_pem` — it must not reach a UI), so the CLI
    // resolves it from the vault here, exactly like the GUI's `ssh_connect`.
    // Without this, key-auth connections would fail with "未导入私钥".
    if config.auth_method == "key" && config.private_key_pem.is_none() {
        let key = require_dek(state)?;
        let db = state.db.lock().map_err(|e| e.to_string())?;
        config.private_key_pem =
            db::get_private_key_pem(&db, &key, &config.id).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Load the user's command-confirmation rules. Mirrors the GUI/MCP loader: a
/// missing file means built-in defaults, a corrupt file is an ERROR — never a
/// silent downgrade to the weaker defaults (`confirm_unknown: false`).
fn load_command_rules() -> Result<command_rules::CommandRules, String> {
    let mut path = dirs::config_dir().ok_or_else(|| "无法定位配置目录".to_string())?;
    path.push("myshell");
    path.push("mcp-command-rules.json");
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|e| format!("解析命令规则失败: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(command_rules::CommandRules::default())
        }
        Err(e) => Err(format!("读取命令规则失败: {e}")),
    }
}

/// Gate a remote command (or a destructive file op) behind the same rule engine
/// the GUI and MCP server use.
///
/// The CLI used to run `exec`, `sftp rm` and `sftp put` with NO policy check at
/// all, so the entire confirmation layer was bypassable by shelling out to
/// `myshell-cli` instead of calling the sanctioned MCP tool — a real hole the
/// moment `MYSHELL_PASSPHRASE` is exported into an agent's environment (the
/// documented unlock mechanism).
///
/// There is no dialog on a terminal, so a hit is REFUSED unless the caller
/// passes `--yes`: an explicit, greppable opt-in rather than a silent allow.
fn ensure_allowed(op: &str, detail: &str, yes: bool) -> Result<(), String> {
    if yes {
        return Ok(());
    }
    // A rules file we cannot read is an error, not a pass — fail closed.
    let rules = load_command_rules()?;
    if !command_rules::command_needs_confirmation(detail, &rules) {
        return Ok(());
    }
    let reasons = command_rules::command_danger_reasons(detail, &rules);
    let why = if reasons.is_empty() {
        String::new()
    } else {
        format!(
            "\n命中规则：\n{}",
            reasons
                .iter()
                .map(|s| format!("  - {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    Err(format!(
        "⛔ {op} 命中危险命令规则，已拒绝执行：{detail}{why}\n\
         确认要执行请显式加 --yes 重试（该参数会被记入 shell history，请自行评估）。"
    ))
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
        zmodem_files: Arc::new(Mutex::new(std::collections::HashMap::new())),
        dek: Arc::new(Mutex::new(None)),
        transfer_cancels: Arc::new(Mutex::new(std::collections::HashMap::new())),
    };

    // Unlock the vault for every command that actually needs credentials.
    //
    // `vault status` only reports `{"initialized": bool}` and never touches the
    // DEK, but it used to go through the full unlock anyway. Headless (stdin at
    // EOF) `read_password()` returns an empty string, `unlock()` fails against
    // the verifier, and the failure was persisted via `lockout.record_failure()`
    // — so three innocuous status probes locked the user out of their own
    // vault for the backoff window (MAX_FAILED_ATTEMPTS = 3).
    let needs_unlock = !matches!(
        cli.command,
        Commands::Vault {
            action: VaultAction::Status
        }
    );
    if needs_unlock && vault::is_initialized() {
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
            cmd_exec(&state, &connection, &command, timeout, cli.json, cli.yes).await
        }
        Commands::Ssh { connection } => cmd_ssh(&state, &connection).await,
        Commands::Sftp { action } => cmd_sftp(&state, action, cli.json, cli.yes).await,
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
    yes: bool,
) -> Result<(), String> {
    // Check the policy BEFORE resolving credentials or dialing.
    ensure_allowed("exec", command, yes)?;
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
    .map_err(|_| {
        format!(
            "命令超时（{}秒）。如需更长时间，--timeout 上限 3600s = 1h；超过 1h 的任务建议在远端用 `nohup ... > /tmp/log 2>&1 &` 后台跑，再 tail 日志。",
            timeout_secs
        )
    })?;

    // Graceful disconnect
    let _ = handle
        .disconnect(russh::Disconnect::ByApplication, "exec done", "en")
        .await;

    let stdout_str = String::from_utf8_lossy(&stdout);
    let stderr_str = String::from_utf8_lossy(&stderr);
    // A missing ExitStatus is NOT exit code 0 — it means the connection dropped
    // or the remote was signal-terminated. `--json` exists so an AI agent can
    // act on the result; reporting a dropped connection as `"exit_code": 0`
    // with truncated stdout is worse than useless, it is confidently wrong.
    let Some(code) = exit_code else {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "error": "远端未返回退出码：连接中断或进程被信号终止，结果未知（不能视为成功）",
                    "exit_code": serde_json::Value::Null,
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
            eprintln!(
                "远端未返回退出码：连接中断或进程被信号终止，结果未知（不能视为成功）。"
            );
        }
        std::process::exit(255);
    };

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
    // hold_startup=false: headless consumer — CliSink exists before the
    // reader spawns, so there's no listener race to hold the banner for.
    let session_id = ssh::connect(state, sink, config, false).await?;

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

async fn cmd_sftp(
    state: &AppState,
    action: SftpAction,
    json: bool,
    yes: bool,
) -> Result<(), String> {
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
            // Matches the MCP policy: file tools ALWAYS require human
            // acknowledgement regardless of the command rules. On a terminal
            // that acknowledgement is the explicit `--yes` flag.
            if !yes {
                return Err(format!(
                    "⛔ sftp put 会覆盖远端文件，必须显式确认。\n  本地: {local}\n  远端: {remote}\n确认无误请加 --yes 重试。"
                ));
            }
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
            if !yes {
                return Err(format!(
                    "⛔ sftp rm 会删除远端文件，必须显式确认。\n  远端: {path}\n确认无误请加 --yes 重试。"
                ));
            }
            let mut config = find_connection(state, &connection)?;
            resolve_secrets(state, &mut config)?;
            let handle = ssh::dial_and_authenticate(state, &config, false).await?;
            let sftp = open_sftp(&handle).await?;

            // Try removing as file first, then as directory. Keep both errors:
            // discarding the file error made a permission-denied FILE surface
            // as an RMDIR failure — a message about an operation that was never
            // intended, hiding the real cause.
            if let Err(file_err) = sftp.remove_file(&path).await {
                sftp.remove_dir(&path).await.map_err(|dir_err| {
                    format!("删除失败: 删文件失败({file_err})，删目录失败({dir_err})")
                })?;
            }
            let _ = handle.disconnect(russh::Disconnect::ByApplication, "done", "en").await;

            if !json {
                println!("✓ 已删除: {}", path);
            }
            Ok(())
        }
        SftpAction::Rename { connection, old, new } => {
            if !yes {
                return Err(format!(
                    "⛔ sftp rename 会覆盖远端路径，必须显式确认。\n  原路径: {old}\n  新路径: {new}\n确认无误请加 --yes 重试。"
                ));
            }
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
