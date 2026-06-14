use crate::{AppState, FileEntry};
use russh_sftp::client::SftpSession;
use std::sync::Arc;
use tauri::State;

async fn get_sftp_session(
    state: &State<'_, AppState>,
    session_id: &str,
) -> Result<SftpSession, String> {
    // Scope the MutexGuard so it's dropped before any await — std::sync::MutexGuard
    // is not Send, and tauri commands require the returned future to be Send.
    let handle = {
        let sessions = state.ssh_sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| "SSH session not found".to_string())?;
        Arc::clone(&session.handle)
    };

    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("SFTP channel open failed: {}", e))?;

    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("SFTP subsystem request failed: {}", e))?;

    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("SFTP session init failed: {}", e))?;

    Ok(sftp)
}

pub async fn list_dir(
    state: &State<'_, AppState>,
    session_id: &str,
    path: &str,
) -> Result<Vec<FileEntry>, String> {
    let sftp = get_sftp_session(state, session_id).await?;

    let entries = sftp
        .read_dir(path)
        .await
        .map_err(|e| format!("Read dir failed: {}", e))?;

    let mut files = Vec::new();
    for entry in entries {
        let file_type = entry.file_type();
        let name = entry.file_name().to_string();
        let full_path = if path.ends_with('/') {
            format!("{}{}", path, name)
        } else {
            format!("{}/{}", path, name)
        };

        files.push(FileEntry {
            name,
            path: full_path,
            is_dir: file_type.is_dir(),
            size: entry.metadata().size.unwrap_or(0),
            permissions: format!("{}", entry.metadata().permissions()),
            modified: String::new(),
        });
    }

    // Sort: dirs first, then files
    files.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(files)
}

pub async fn create_dir(
    state: &State<'_, AppState>,
    session_id: &str,
    path: &str,
) -> Result<(), String> {
    let sftp = get_sftp_session(state, session_id).await?;
    sftp.create_dir(path)
        .await
        .map_err(|e| format!("Create dir failed: {}", e))?;
    Ok(())
}

pub async fn remove(
    state: &State<'_, AppState>,
    session_id: &str,
    path: &str,
) -> Result<(), String> {
    let sftp = get_sftp_session(state, session_id).await?;

    // Try removing as file first, then as directory
    if sftp.remove_file(path).await.is_err() {
        sftp.remove_dir(path)
            .await
            .map_err(|e| format!("Remove failed: {}", e))?;
    }
    Ok(())
}

pub async fn rename(
    state: &State<'_, AppState>,
    session_id: &str,
    old_path: &str,
    new_path: &str,
) -> Result<(), String> {
    let sftp = get_sftp_session(state, session_id).await?;
    sftp.rename(old_path, new_path)
        .await
        .map_err(|e| format!("Rename failed: {}", e))?;
    Ok(())
}
