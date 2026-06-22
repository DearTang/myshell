//! FTP/FTPS client built on suppaftp v8 (tokio runtime). Mirrors the
//! sftp.rs command surface so the frontend can reuse SftpPanel with
//! `source: "ftp"`. Sessions live in AppState::ftp_sessions keyed by UUID.
//!
//! NOTE: This module currently supports plain FTP only. FTPS (implicit +
//! explicit TLS) is deferred to a later phase because suppaftp's rustls
//! connector requires wiring in the rustls + webpki-roots crates as direct
//! deps. The TLS path returns a clear "not yet supported" error so the user
//! can pick `ftp_tls=none` and proceed.

use crate::{ConnectionConfig, FileEntry};
use crate::proxy;
use std::time::SystemTime;
use suppaftp::list::File as FtpFile;
use suppaftp::tokio::AsyncFtpStream;
use suppaftp::types::FileType;

pub struct FtpSession {
    pub stream: AsyncFtpStream,
}

pub async fn connect(cfg: &ConnectionConfig) -> Result<FtpSession, String> {
    if cfg.ftp_tls != "none" {
        return Err("FTPS (TLS) 当前版本暂不支持，请在连接配置中选择「不加密 (FTP)」".to_string());
    }

    let port = if cfg.port == 0 { 21 } else { cfg.port };

    // Branch on proxy config. FTP TLS + proxy is intentionally unsupported
    // (FTP TLS itself is a stub — see module doc). Plain FTP through a
    // proxy works by handing the upgraded stream to suppaftp's
    // connect_with_stream variant.
    let mut stream = match proxy::ProxyConfig::from_config(cfg)? {
        Some(proxy_cfg) => {
            eprintln!(
                "[ftp] connecting via {} proxy {}:{} → {}:{}",
                cfg.proxy_type,
                proxy_cfg.host(),
                proxy_cfg.port(),
                cfg.host,
                port
            );
            let stream = proxy::connect_via_proxy(&proxy_cfg, &cfg.host, port).await?;
            AsyncFtpStream::connect_with_stream(stream)
                .await
                .map_err(|e| format!("FTP connect via proxy failed: {}", e))?
        }
        None => AsyncFtpStream::connect(format!("{}:{}", cfg.host, port))
            .await
            .map_err(|e| format!("FTP connect failed: {}", e))?,
    };

    // Caller (ftp_connect command) is expected to pre-resolve the password
    // from keyring into cfg.password. If absent (anonymous FTP) we fall
    // back to empty.
    let password = cfg.password.clone().unwrap_or_default();

    stream
        .login(&cfg.username, &password)
        .await
        .map_err(|e| format!("FTP login failed: {}", e))?;

    if !cfg.ftp_passive {
        stream = stream.active_mode(std::time::Duration::from_secs(10));
    }
    stream
        .transfer_type(FileType::Binary)
        .await
        .map_err(|e| format!("FTP binary type failed: {}", e))?;

    Ok(FtpSession { stream })
}

pub async fn list_dir(s: &mut FtpSession, path: &str) -> Result<Vec<FileEntry>, String> {
    // MLSD is the modern machine-readable listing. Fall back to LIST (POSIX)
    // for servers that don't support it. Both commands return raw lines —
    // ListParser turns them into typed File structs.
    let lines: Vec<String> = match s.stream.mlsd(Some(path)).await {
        Ok(v) if !v.is_empty() => v,
        _ => s
            .stream
            .list(Some(path))
            .await
            .map_err(|e| format!("FTP list failed: {}", e))?,
    };

    let parent = path.trim_end_matches('/');
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        // Try MLSD grammar first, then POSIX. Each line that fails both
        // parsers is silently dropped — better to render a partial listing
        // than error the whole dir.
        let parsed = suppaftp::list::ListParser::parse_mlsd(&line)
            .or_else(|_| suppaftp::list::ListParser::parse_posix(&line));
        let f = match parsed {
            Ok(f) => f,
            Err(_) => continue,
        };
        let name = f.name().to_string();
        if name == "." || name == ".." {
            continue;
        }
        let full = if parent.is_empty() || parent == "." {
            format!("/{}", name)
        } else {
            format!("{}/{}", parent, name)
        };
        out.push(FileEntry {
            name,
            path: full,
            is_dir: f.is_directory(),
            size: f.size() as u64,
            permissions: format_pex(&f),
            modified: format_time(f.modified()),
        });
    }
    out.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(out)
}

pub async fn mkdir(s: &mut FtpSession, path: &str) -> Result<(), String> {
    s.stream
        .mkdir(path)
        .await
        .map_err(|e| format!("FTP mkdir failed: {}", e))
}

pub async fn remove(s: &mut FtpSession, path: &str, is_dir: bool) -> Result<(), String> {
    let r = if is_dir {
        s.stream.rmdir(path).await
    } else {
        s.stream.rm(path).await
    };
    r.map_err(|e| format!("FTP remove failed: {}", e))
}

pub async fn rename(s: &mut FtpSession, from: &str, to: &str) -> Result<(), String> {
    s.stream
        .rename(from, to)
        .await
        .map_err(|e| format!("FTP rename failed: {}", e))
}

pub async fn disconnect(s: &mut FtpSession) -> Result<(), String> {
    // quit sends QUIT command and closes — best-effort, ignore errors.
    let _ = s.stream.quit().await;
    Ok(())
}

fn format_pex(f: &FtpFile) -> String {
    use suppaftp::list::PosixPexQuery;
    let r = |who: PosixPexQuery| f.can_read(who);
    let w = |who: PosixPexQuery| f.can_write(who);
    let x = |who: PosixPexQuery| f.can_execute(who);
    let o = PosixPexQuery::Owner;
    let g = PosixPexQuery::Group;
    let ot = PosixPexQuery::Others;
    let bit = |cond: bool, chr: char| if cond { chr } else { '-' };
    format!(
        "{}{}{}{}{}{}{}{}{}",
        bit(r(o), 'r'),
        bit(w(o), 'w'),
        bit(x(o), 'x'),
        bit(r(g), 'r'),
        bit(w(g), 'w'),
        bit(x(g), 'x'),
        bit(r(ot), 'r'),
        bit(w(ot), 'w'),
        bit(x(ot), 'x'),
    )
}

fn format_time(t: SystemTime) -> String {
    // chrono is already a dependency (used by vault.rs). The previous
    // hand-rolled days_to_ymd was the same anti-pattern vault.rs warns
    // about — it produced wrong month/day values for some timestamps.
    chrono::DateTime::<chrono::Utc>::from(t)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}
