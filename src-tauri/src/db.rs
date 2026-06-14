use crate::ConnectionConfig;
use crate::crypto;
use rusqlite::{Connection, Result, params};
use std::path::PathBuf;

fn db_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("myshell");
    std::fs::create_dir_all(&path).ok();
    path.push("connections.db");
    path
}

/// Open + initialize the on-disk SQLite DB. Fresh installs get the
/// encrypted-column schema directly; legacy installs are upgraded by
/// [`migrate_legacy_schema`] (v0.1 → v0.2) and [`migrate_to_vault`]
/// (v0.2 → vault) on subsequent launches.
pub fn init_db() -> Result<Connection> {
    let conn = Connection::open(db_path())?;
    // IF NOT EXISTS: existing tables are left alone. The vault migration
    // (run after unlock) is responsible for transforming an existing
    // connections table from plaintext to encrypted columns.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS connections (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            host_enc TEXT,
            port INTEGER NOT NULL DEFAULT 22,
            username_enc TEXT,
            auth_method TEXT NOT NULL DEFAULT 'password',
            private_key_pem_enc TEXT,
            private_key_path_enc TEXT,
            conn_type TEXT NOT NULL DEFAULT 'ssh',
            group_path TEXT NOT NULL DEFAULT '/',
            ftp_tls TEXT NOT NULL DEFAULT 'none',
            ftp_passive INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS known_hosts (
            host TEXT PRIMARY KEY,
            fingerprint TEXT NOT NULL,
            key_type TEXT NOT NULL,
            first_seen TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS folders (
            path TEXT PRIMARY KEY,
            created_at TEXT NOT NULL
        );"
    )?;
    Ok(conn)
}

/// Returns true if `table.column` exists. SQLite has no
/// `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`, so we probe PRAGMA.
fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info({})", table);
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let names = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .ok()
        .map(|rows| rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
        .unwrap_or_default();
    names.iter().any(|n| n == column)
}

/// v0.1 → v0.2 schema upgrade. Drops plaintext `password` column (already
/// migrated to keyring), renames `group_name` → `group_path` with leading
/// slash, adds conn_type/ftp_tls/ftp_passive. Idempotent.
pub fn migrate_legacy_schema(conn: &Connection) -> Result<()> {
    if column_exists(conn, "connections", "password") {
        conn.execute("ALTER TABLE connections DROP COLUMN password", [])?;
        eprintln!("[db] dropped legacy password column");
    }

    if column_exists(conn, "connections", "group_name") {
        conn.execute(
            "ALTER TABLE connections ADD COLUMN group_path_new TEXT NOT NULL DEFAULT '/'",
            [],
        )?;
        conn.execute(
            "UPDATE connections SET group_path_new = \
             CASE WHEN group_name IS NULL OR group_name = '' THEN '/' \
             ELSE '/' || group_name END",
            [],
        )?;
        conn.execute("ALTER TABLE connections DROP COLUMN group_name", [])?;
        conn.execute("ALTER TABLE connections RENAME COLUMN group_path_new TO group_path", [])?;
        eprintln!("[db] migrated group_name -> group_path");
    }

    if !column_exists(conn, "connections", "conn_type") {
        conn.execute(
            "ALTER TABLE connections ADD COLUMN conn_type TEXT NOT NULL DEFAULT 'ssh'",
            [],
        )?;
    }
    if !column_exists(conn, "connections", "ftp_tls") {
        conn.execute(
            "ALTER TABLE connections ADD COLUMN ftp_tls TEXT NOT NULL DEFAULT 'none'",
            [],
        )?;
    }
    if !column_exists(conn, "connections", "ftp_passive") {
        conn.execute(
            "ALTER TABLE connections ADD COLUMN ftp_passive INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }

    conn.execute(
        "CREATE TABLE IF NOT EXISTS folders (path TEXT PRIMARY KEY, created_at TEXT NOT NULL)",
        [],
    )?;
    Ok(())
}

/// v0.2 → vault schema upgrade: encrypt host/username/private_key_path in
/// place under the user's master key, then drop the plaintext columns.
/// Idempotent — if `host_enc` already exists, returns Ok(0). Wrapped in a
/// transaction so a mid-migration crash leaves the plaintext intact.
///
/// `key` is the already-derived master key from `setup_vault`. Caller
/// guarantees it's correct (verifier check happens before this runs).
pub fn migrate_to_vault(conn: &mut Connection, key: &[u8; 32]) -> Result<usize> {
    // Fresh install (init_db created host_enc directly) or already migrated.
    if column_exists(conn, "connections", "host_enc") {
        // But there's an edge case: an install that already had the legacy
        // `host` column from a v0.2 install pre-vault, then ran init_db on
        // a vault build for the first time — IF NOT EXISTS skips column
        // creation, so we still have plaintext `host` alongside a missing
        // `host_enc`. Detect that: if both `host` and `host_enc` are missing,
        // we're a fresh install. If `host` exists, we need to migrate.
        if !column_exists(conn, "connections", "host") {
            return Ok(0);
        }
    }

    // Ensure encrypted columns exist (idempotent — they may already be
    // present from init_db on a fresh install that nonetheless also has
    // legacy `host` from a previous binary version).
    if !column_exists(conn, "connections", "host_enc") {
        conn.execute("ALTER TABLE connections ADD COLUMN host_enc TEXT", [])?;
    }
    if !column_exists(conn, "connections", "username_enc") {
        conn.execute("ALTER TABLE connections ADD COLUMN username_enc TEXT", [])?;
    }
    if !column_exists(conn, "connections", "private_key_pem_enc") {
        conn.execute("ALTER TABLE connections ADD COLUMN private_key_pem_enc TEXT", [])?;
    }
    if !column_exists(conn, "connections", "private_key_path_enc") {
        conn.execute("ALTER TABLE connections ADD COLUMN private_key_path_enc TEXT", [])?;
    }

    // Read all plaintext rows in one pass, then write encrypted versions
    // in a single transaction. Doing it row-by-row with autocommit would
    // be O(n) fsyncs — bad for users with hundreds of connections.
    let tx = conn.transaction()?;
    let mut select = tx.prepare(
        "SELECT id, host, username, private_key_path FROM connections",
    )?;
    let rows: Vec<(String, Option<String>, Option<String>, Option<String>)> = select
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    drop(select);

    let mut migrated = 0;
    for (id, host, username, key_path) in rows {
        let host_enc = host
            .as_ref()
            .map(|h| crypto::encrypt_with_key(key, h.as_bytes()))
            .transpose()
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
        let user_enc = username
            .as_ref()
            .map(|u| crypto::encrypt_with_key(key, u.as_bytes()))
            .transpose()
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;

        // Try to read the key file as PEM so we can store content (not just
        // path). If the file is gone or unreadable, fall back to encrypting
        // the path itself — connection will fail at SSH time with a clear
        // "missing key" error, but the row is preserved.
        let (pem_enc, path_enc) = match key_path.as_ref() {
            Some(p) if !p.is_empty() => {
                match std::fs::read_to_string(p) {
                    Ok(pem) => (
                        crypto::encrypt_with_key(key, pem.as_bytes())
                            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?
                            .into(),
                        None,
                    ),
                    Err(_) => (
                        None,
                        crypto::encrypt_with_key(key, p.as_bytes())
                            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?
                            .into(),
                    ),
                }
            }
            _ => (None, None),
        };

        tx.execute(
            "UPDATE connections SET host_enc = ?2, username_enc = ?3,
                private_key_pem_enc = ?4, private_key_path_enc = ?5
             WHERE id = ?1",
            params![id, host_enc, user_enc, pem_enc, path_enc],
        )?;
        migrated += 1;
    }

    // Drop plaintext columns now that every row has encrypted equivalents.
    // SQLite 3.35+ supports DROP COLUMN; rusqlite 0.32 bundled has 3.45+.
    if column_exists(&tx, "connections", "host") {
        tx.execute("ALTER TABLE connections DROP COLUMN host", [])?;
    }
    if column_exists(&tx, "connections", "username") {
        tx.execute("ALTER TABLE connections DROP COLUMN username", [])?;
    }
    if column_exists(&tx, "connections", "private_key_path") {
        tx.execute("ALTER TABLE connections DROP COLUMN private_key_path", [])?;
    }
    tx.commit()?;
    eprintln!("[db] vault migration: {} rows encrypted", migrated);
    Ok(migrated)
}

// ============ known_hosts ============

pub fn get_known_host(conn: &Connection, host: &str) -> Result<Option<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT fingerprint, key_type FROM known_hosts WHERE host = ?1",
    )?;
    let mut rows = stmt.query_map(params![host], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn set_known_host(
    conn: &Connection,
    host: &str,
    fingerprint: &str,
    key_type: &str,
    first_seen: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO known_hosts (host, fingerprint, key_type, first_seen)
         VALUES (?1, ?2, ?3, ?4)",
        params![host, fingerprint, key_type, first_seen],
    )?;
    Ok(())
}

// ============ Connections CRUD ============
//
// All read/write paths take `key: &[u8; 32]` and encrypt/decrypt at the
// column boundary. Callers in main.rs are responsible for surfacing
// "vault 未解锁" if master_key is None — these functions panic on a missing
// key (caller bug, not a user-facing condition).

pub fn get_all_connections(conn: &Connection, key: &[u8; 32]) -> Result<Vec<ConnectionConfig>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, host_enc, port, username_enc, auth_method, private_key_pem_enc,
                private_key_path_enc, conn_type, group_path, ftp_tls, ftp_passive, created_at
         FROM connections ORDER BY group_path, name"
    )?;

    // Pull every column out of the Row inside the closure — we can't return
    // a `&Row` reference because rusqlite ties Row to the Statement lifetime.
    // Own all the values up front; decrypt outside the closure.
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,                       // id
            row.get::<_, String>(1)?,                       // name
            row.get::<_, Option<String>>(2)?,               // host_enc
            row.get::<_, i64>(3)?,                          // port (read as i64 for u16 conversion)
            row.get::<_, Option<String>>(4)?,               // username_enc
            row.get::<_, String>(5)?,                       // auth_method
            row.get::<_, Option<String>>(6)?,               // pem_enc
            row.get::<_, String>(8)?,                       // conn_type
            row.get::<_, String>(9)?,                       // group_path
            row.get::<_, String>(10)?,                      // ftp_tls
            row.get::<_, i64>(11)?,                         // ftp_passive
            row.get::<_, String>(12)?,                      // created_at
        ))
    })?;

    let mut configs = Vec::new();
    for row in rows {
        let (id, name, host_enc, port_i, user_enc, auth_method, pem_enc,
             conn_type, group_path, ftp_tls, ftp_passive, created_at) = row?;
        let host = decrypt_field(key, host_enc)?.unwrap_or_default();
        let username = decrypt_field(key, user_enc)?.unwrap_or_default();
        let private_key_pem = decrypt_field(key, pem_enc)?;
        let port: u16 = port_i.try_into().unwrap_or(0);
        configs.push(ConnectionConfig {
            id,
            name,
            host,
            port,
            username,
            auth_method,
            password: None,
            private_key_pem,
            conn_type,
            group_path,
            ftp_tls,
            ftp_passive: ftp_passive != 0,
            created_at,
        });
    }
    Ok(configs)
}

pub fn save_connection(conn: &Connection, key: &[u8; 32], config: &ConnectionConfig) -> Result<()> {
    let host_enc = crypto::encrypt_with_key(key, config.host.as_bytes())
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
    let user_enc = crypto::encrypt_with_key(key, config.username.as_bytes())
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
    let pem_enc = match config.private_key_pem.as_ref() {
        Some(p) if !p.is_empty() => Some(
            crypto::encrypt_with_key(key, p.as_bytes())
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?,
        ),
        _ => None,
    };

    conn.execute(
        "INSERT OR REPLACE INTO connections
            (id, name, host_enc, port, username_enc, auth_method, private_key_pem_enc,
             private_key_path_enc, conn_type, group_path, ftp_tls, ftp_passive, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, ?10, ?11, ?12)",
        params![
            config.id,
            config.name,
            host_enc,
            config.port,
            user_enc,
            config.auth_method,
            pem_enc,
            config.conn_type,
            config.group_path,
            config.ftp_tls,
            config.ftp_passive as i64,
            config.created_at,
        ],
    )?;
    Ok(())
}

pub fn get_connection(conn: &Connection, key: &[u8; 32], id: &str) -> Result<Option<ConnectionConfig>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, host_enc, port, username_enc, auth_method, private_key_pem_enc,
                private_key_path_enc, conn_type, group_path, ftp_tls, ftp_passive, created_at
         FROM connections WHERE id = ?1"
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, String>(12)?,
        ))
    })?;
    match rows.next() {
        Some(Ok((id, name, host_enc, port_i, user_enc, auth_method, pem_enc,
                 conn_type, group_path, ftp_tls, ftp_passive, created_at))) => {
            let host = decrypt_field(key, host_enc)?.unwrap_or_default();
            let username = decrypt_field(key, user_enc)?.unwrap_or_default();
            let private_key_pem = decrypt_field(key, pem_enc)?;
            let port: u16 = port_i.try_into().unwrap_or(0);
            Ok(Some(ConnectionConfig {
                id,
                name,
                host,
                port,
                username,
                auth_method,
                password: None,
                private_key_pem,
                conn_type,
                group_path,
                ftp_tls,
                ftp_passive: ftp_passive != 0,
                created_at,
            }))
        }
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

pub fn connection_name_exists(conn: &Connection, name: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM connections WHERE name = ?1",
        params![name],
        |row| row.get(0),
    )?;
    Ok(n > 0)
}

pub fn delete_connection(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM connections WHERE id = ?1", params![id])?;
    Ok(())
}

// ============ Folders ============

pub fn list_folders(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT path FROM folders ORDER BY path")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn save_folder(conn: &Connection, path: &str, created_at: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO folders (path, created_at) VALUES (?1, ?2)",
        params![path, created_at],
    )?;
    Ok(())
}

pub fn delete_folder(conn: &Connection, path: &str) -> Result<()> {
    conn.execute("DELETE FROM folders WHERE path = ?1", params![path])?;
    Ok(())
}

pub fn rename_folder(conn: &Connection, old_path: &str, new_path: &str) -> Result<()> {
    let pattern = format!("{}/%", old_path);
    conn.execute(
        "UPDATE connections SET group_path = ?1 || substr(group_path, length(?2) + 1)
         WHERE group_path = ?2 OR group_path LIKE ?3",
        params![new_path, old_path, pattern],
    )?;
    conn.execute(
        "UPDATE folders SET path = ?1 || substr(path, length(?2) + 1)
         WHERE path = ?2 OR path LIKE ?3",
        params![new_path, old_path, pattern],
    )?;
    Ok(())
}

pub fn folder_has_children(conn: &Connection, path: &str) -> Result<bool> {
    let pattern = format!("{}/%", path);
    let conn_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM connections WHERE group_path = ?1 OR group_path LIKE ?2",
        params![path, pattern],
        |row| row.get(0),
    )?;
    if conn_count > 0 {
        return Ok(true);
    }
    let folder_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM folders WHERE path = ?1 OR path LIKE ?2",
        params![path, pattern],
        |row| row.get(0),
    )?;
    Ok(folder_count > 0)
}

// ============ Helpers ============

/// Decrypt an encrypted column value. None → None (NULL column or fresh row
/// not yet populated). Error surfaces as a rusqlite failure so the caller's
/// `?` propagates it cleanly.
fn decrypt_field(key: &[u8; 32], blob: Option<String>) -> Result<Option<String>> {
    match blob {
        None => Ok(None),
        Some(b) => {
            let pt = crypto::decrypt_with_key(key, &b)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                ))?;
            String::from_utf8(pt)
                .map(Some)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                ))
        }
    }
}
