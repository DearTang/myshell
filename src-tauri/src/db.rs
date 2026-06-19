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
            proxy_type TEXT NOT NULL DEFAULT 'none',
            proxy_host_enc TEXT,
            proxy_port INTEGER,
            proxy_username TEXT,
            shell_path TEXT,
            shell_args TEXT,
            init_command TEXT,
            created_at TEXT NOT NULL,
            terminal_font TEXT
        );
        CREATE TABLE IF NOT EXISTS known_hosts (
            host TEXT NOT NULL,
            port INTEGER NOT NULL DEFAULT 22,
            fingerprint TEXT NOT NULL,
            key_type TEXT NOT NULL,
            first_seen TEXT NOT NULL,
            PRIMARY KEY (host, port)
        );
        CREATE TABLE IF NOT EXISTS folders (
            path TEXT PRIMARY KEY,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS command_history (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            connection_id TEXT    NOT NULL,
            command       TEXT    NOT NULL,
            pinned        INTEGER NOT NULL DEFAULT 0,
            created_at    TEXT    NOT NULL,
            pinned_at     TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_cmd_history_conn ON command_history(connection_id);
        CREATE INDEX IF NOT EXISTS idx_cmd_history_pinned ON command_history(connection_id, pinned, pinned_at DESC);
        CREATE TABLE IF NOT EXISTS quick_commands (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            connection_id TEXT,
            label         TEXT    NOT NULL,
            command       TEXT    NOT NULL,
            sort_order    INTEGER NOT NULL DEFAULT 0,
            created_at    TEXT    NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_qc_conn ON quick_commands(connection_id);
        CREATE INDEX IF NOT EXISTS idx_qc_sort ON quick_commands(connection_id, sort_order, id);
        -- AI assistant: single-row config (CHECK id=1 forces it). api_key_enc
        -- holds a crypto::encrypt_with_key blob (AES-256-GCM, base64); never
        -- plaintext. Provider/model/baseUrl/temperature are non-secret.
        CREATE TABLE IF NOT EXISTS ai_settings (
            id          INTEGER PRIMARY KEY CHECK (id = 1),
            provider    TEXT NOT NULL DEFAULT 'claude',
            model       TEXT,
            base_url    TEXT,
            api_key_enc TEXT,
            proxy_url   TEXT,
            temperature REAL NOT NULL DEFAULT 0.7
        );
        CREATE TABLE IF NOT EXISTS ai_conversations (
            id            TEXT PRIMARY KEY,
            connection_id TEXT,
            role          TEXT NOT NULL,
            content       TEXT NOT NULL,
            created_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ai_conv_conn ON ai_conversations(connection_id, created_at);"
    )?;
    // AI proxy_url column — idempotent ALTER for installs that already
    // created ai_settings without it (CREATE IF NOT EXISTS won't add it).
    if !column_exists(&conn, "ai_settings", "proxy_url") {
        conn.execute("ALTER TABLE ai_settings ADD COLUMN proxy_url TEXT", [])?;
    }
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
/// slash, adds conn_type/ftp_tls/ftp_passive. Idempotent. Wrapped in a
/// transaction so a mid-migration crash leaves the schema consistent.
pub fn migrate_legacy_schema(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    if column_exists(&tx, "connections", "password") {
        tx.execute("ALTER TABLE connections DROP COLUMN password", [])?;
        eprintln!("[db] dropped legacy password column");
    }

    if column_exists(&tx, "connections", "group_name") {
        tx.execute(
            "ALTER TABLE connections ADD COLUMN group_path_new TEXT NOT NULL DEFAULT '/'",
            [],
        )?;
        tx.execute(
            "UPDATE connections SET group_path_new = \
             CASE WHEN group_name IS NULL OR group_name = '' THEN '/' \
             ELSE '/' || group_name END",
            [],
        )?;
        tx.execute("ALTER TABLE connections DROP COLUMN group_name", [])?;
        tx.execute("ALTER TABLE connections RENAME COLUMN group_path_new TO group_path", [])?;
        eprintln!("[db] migrated group_name -> group_path");
    }

    if !column_exists(&tx, "connections", "conn_type") {
        tx.execute(
            "ALTER TABLE connections ADD COLUMN conn_type TEXT NOT NULL DEFAULT 'ssh'",
            [],
        )?;
    }
    if !column_exists(&tx, "connections", "ftp_tls") {
        tx.execute(
            "ALTER TABLE connections ADD COLUMN ftp_tls TEXT NOT NULL DEFAULT 'none'",
            [],
        )?;
    }
    if !column_exists(&tx, "connections", "ftp_passive") {
        tx.execute(
            "ALTER TABLE connections ADD COLUMN ftp_passive INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }

    // Proxy support columns (v0.2 → v0.3). Idempotent — existing installs
    // get the columns added with safe defaults; new installs have them via
    // init_db's CREATE TABLE.
    if !column_exists(&tx, "connections", "proxy_type") {
        tx.execute(
            "ALTER TABLE connections ADD COLUMN proxy_type TEXT NOT NULL DEFAULT 'none'",
            [],
        )?;
    }
    if !column_exists(&tx, "connections", "proxy_host_enc") {
        tx.execute(
            "ALTER TABLE connections ADD COLUMN proxy_host_enc TEXT",
            [],
        )?;
    }
    if !column_exists(&tx, "connections", "proxy_port") {
        tx.execute("ALTER TABLE connections ADD COLUMN proxy_port INTEGER", [])?;
    }
    if !column_exists(&tx, "connections", "proxy_username") {
        tx.execute(
            "ALTER TABLE connections ADD COLUMN proxy_username TEXT",
            [],
        )?;
    }

    // Local terminal shell config (added for conn_type='local'). Plain
    // columns — a shell executable path isn't a secret, same treatment as
    // conn_type/ftp_tls/proxy_type. Idempotent.
    if !column_exists(&tx, "connections", "shell_path") {
        tx.execute("ALTER TABLE connections ADD COLUMN shell_path TEXT", [])?;
    }
    if !column_exists(&tx, "connections", "shell_args") {
        tx.execute("ALTER TABLE connections ADD COLUMN shell_args TEXT", [])?;
    }
    if !column_exists(&tx, "connections", "init_command") {
        tx.execute("ALTER TABLE connections ADD COLUMN init_command TEXT", [])?;
    }
    // Per-connection terminal font override (nullable; NULL = use global).
    if !column_exists(&tx, "connections", "terminal_font") {
        tx.execute("ALTER TABLE connections ADD COLUMN terminal_font TEXT", [])?;
    }

    tx.execute(
        "CREATE TABLE IF NOT EXISTS folders (path TEXT PRIMARY KEY, created_at TEXT NOT NULL)",
        [],
    )?;

    // known_hosts: the original schema used `host` as the sole primary key,
    // so the same hostname reachable on two different ports (e.g. 22 internal
    // + 2222 jump host) shared one fingerprint slot — swapping ports
    // silently invalidated the trusted entry, opening a MITM window. Rebuild
    // the table with a composite (host, port) PK when upgrading from the old
    // shape. Existing rows default to port 22.
    if !column_exists(&tx, "known_hosts", "port") {
        tx.execute_batch(
            "ALTER TABLE known_hosts RENAME TO known_hosts_old_v1;
             CREATE TABLE known_hosts (
                 host TEXT NOT NULL,
                 port INTEGER NOT NULL DEFAULT 22,
                 fingerprint TEXT NOT NULL,
                 key_type TEXT NOT NULL,
                 first_seen TEXT NOT NULL,
                 PRIMARY KEY (host, port)
             );
             INSERT INTO known_hosts (host, port, fingerprint, key_type, first_seen)
                 SELECT host, 22, fingerprint, key_type, first_seen FROM known_hosts_old_v1;
             DROP TABLE known_hosts_old_v1;",
        )?;
        eprintln!("[db] known_hosts: rebuilt with (host, port) primary key");
    }

    tx.commit()?;
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

pub fn get_known_host(conn: &Connection, host: &str, port: u16) -> Result<Option<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT fingerprint, key_type FROM known_hosts WHERE host = ?1 AND port = ?2",
    )?;
    let mut rows = stmt.query_map(params![host, port], |row| {
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
    port: u16,
    fingerprint: &str,
    key_type: &str,
    first_seen: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO known_hosts (host, port, fingerprint, key_type, first_seen)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![host, port, fingerprint, key_type, first_seen],
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
                private_key_path_enc, conn_type, group_path, ftp_tls, ftp_passive,
                proxy_type, proxy_host_enc, proxy_port, proxy_username, shell_path, shell_args, init_command, created_at, terminal_font
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
            row.get::<_, String>(12)?,                      // proxy_type
            row.get::<_, Option<String>>(13)?,              // proxy_host_enc
            row.get::<_, Option<i64>>(14)?,                 // proxy_port
            row.get::<_, Option<String>>(15)?,              // proxy_username
            row.get::<_, Option<String>>(16)?,              // shell_path
            row.get::<_, Option<String>>(17)?,              // shell_args
            row.get::<_, Option<String>>(18)?,              // init_command
            row.get::<_, String>(19)?,                      // created_at
            row.get::<_, Option<String>>(20)?,              // terminal_font
        ))
    })?;

    let mut configs = Vec::new();
    for row in rows {
        let (id, name, host_enc, port_i, user_enc, auth_method, pem_enc,
             conn_type, group_path, ftp_tls, ftp_passive,
             proxy_type, proxy_host_enc, proxy_port_i, proxy_username,
             shell_path, shell_args, init_command, created_at, terminal_font) = row?;
        let host = decrypt_field(key, host_enc)?.unwrap_or_default();
        let username = decrypt_field(key, user_enc)?.unwrap_or_default();
        let private_key_pem = decrypt_field(key, pem_enc)?;
        let proxy_host = decrypt_field(key, proxy_host_enc)?;
        let port: u16 = port_i.try_into().unwrap_or(0);
        let proxy_port = proxy_port_i.and_then(|p| p.try_into().ok());
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
            proxy_type,
            proxy_host,
            proxy_port,
            proxy_username,
            shell_path,
            shell_args,
            init_command,
            proxy_password: None, // resolved from keyring by caller
            created_at,
            terminal_font,
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
    // Proxy host encryption — same scheme as host. Empty proxy_host is stored
    // as NULL (proxy_type='none' case usually).
    let proxy_host_enc = match config.proxy_host.as_ref() {
        Some(h) if !h.is_empty() => Some(
            crypto::encrypt_with_key(key, h.as_bytes())
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?,
        ),
        _ => None,
    };

    conn.execute(
        "INSERT OR REPLACE INTO connections
            (id, name, host_enc, port, username_enc, auth_method, private_key_pem_enc,
             private_key_path_enc, conn_type, group_path, ftp_tls, ftp_passive,
             proxy_type, proxy_host_enc, proxy_port, proxy_username, shell_path, shell_args, init_command, created_at, terminal_font)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
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
            config.proxy_type,
            proxy_host_enc,
            config.proxy_port.map(|p| p as i64),
            config.proxy_username,
            config.shell_path,
            config.shell_args,
            config.init_command,
            config.created_at,
            config.terminal_font,
        ],
    )?;
    Ok(())
}

pub fn get_connection(conn: &Connection, key: &[u8; 32], id: &str) -> Result<Option<ConnectionConfig>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, host_enc, port, username_enc, auth_method, private_key_pem_enc,
                private_key_path_enc, conn_type, group_path, ftp_tls, ftp_passive,
                proxy_type, proxy_host_enc, proxy_port, proxy_username, shell_path, shell_args, init_command, created_at, terminal_font
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
            row.get::<_, String>(12)?,                       // proxy_type
            row.get::<_, Option<String>>(13)?,               // proxy_host_enc
            row.get::<_, Option<i64>>(14)?,                  // proxy_port
            row.get::<_, Option<String>>(15)?,               // proxy_username
            row.get::<_, Option<String>>(16)?,               // shell_path
            row.get::<_, Option<String>>(17)?,               // shell_args
            row.get::<_, Option<String>>(18)?,               // init_command
            row.get::<_, String>(19)?,                       // created_at
            row.get::<_, Option<String>>(20)?,               // terminal_font
        ))
    })?;
    match rows.next() {
        Some(Ok((id, name, host_enc, port_i, user_enc, auth_method, pem_enc,
                 conn_type, group_path, ftp_tls, ftp_passive,
                 proxy_type, proxy_host_enc, proxy_port_i, proxy_username,
                 shell_path, shell_args, init_command, created_at, terminal_font))) => {
            let host = decrypt_field(key, host_enc)?.unwrap_or_default();
            let username = decrypt_field(key, user_enc)?.unwrap_or_default();
            let private_key_pem = decrypt_field(key, pem_enc)?;
            let proxy_host = decrypt_field(key, proxy_host_enc)?;
            let port: u16 = port_i.try_into().unwrap_or(0);
            let proxy_port = proxy_port_i.and_then(|p| p.try_into().ok());
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
                proxy_type,
                proxy_host,
                proxy_port,
                proxy_username,
                shell_path,
                shell_args,
                init_command,
                proxy_password: None,
                created_at,
                terminal_font,
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
    // Cascade: remove the connection's per-server quick_commands in the same
    // transaction. command_history is intentionally NOT cascaded (preserves
    // existing behavior — it self-trims to 50 rows anyway).
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM quick_commands WHERE connection_id = ?1", params![id])?;
    tx.execute("DELETE FROM connections WHERE id = ?1", params![id])?;
    tx.commit()?;
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

// ============ Command History ============
//
// Per-connection shell command history with pin support. Pinned entries
// survive the 50-row trim and surface at the top of list_command_history.
// Created_at is an epoch-seconds string (same scheme as `folders`); we
// additionally order by `id DESC` as a stable tiebreaker for sub-second
// bursts (rapid-fire pastes, scripted `ssh_send` from broadcast).

pub fn add_command_history(conn: &Connection, connection_id: &str, command: &str, created_at: &str) -> Result<i64> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }

    let tx = conn.unchecked_transaction()?;

    // Delete any existing entry with the same command (dedup across all history).
    // The new insert will place it at the top (most recent).
    tx.execute(
        "DELETE FROM command_history WHERE connection_id = ?1 AND command = ?2",
        params![connection_id, trimmed],
    )?;

    tx.execute(
        "INSERT INTO command_history (connection_id, command, pinned, created_at, pinned_at)
         VALUES (?1, ?2, 0, ?3, NULL)",
        params![connection_id, trimmed, created_at],
    )?;
    let new_id = tx.last_insert_rowid();

    // Trim unpinned entries beyond the most recent 50.
    tx.execute(
        "DELETE FROM command_history
         WHERE connection_id = ?1
           AND pinned = 0
           AND id NOT IN (
             SELECT id FROM command_history
             WHERE connection_id = ?1 AND pinned = 0
             ORDER BY created_at DESC, id DESC
             LIMIT 50
           )",
        params![connection_id],
    )?;
    tx.commit()?;
    Ok(new_id)
}

pub fn list_command_history(conn: &Connection, connection_id: &str) -> Result<Vec<(i64, String, bool, String)>> {
    // Pinned first (by pinned_at DESC — most recently pinned wins top spot,
    // falling back to id DESC when pinned_at ties or is null), then up to 50
    // most-recent unpinned. We select all columns needed for sorting and let
    // the outer ORDER BY reference them.
    let mut stmt = conn.prepare(
        "SELECT id, command, pinned, created_at, pinned_at FROM (
            SELECT id, command, pinned, created_at, pinned_at FROM command_history
             WHERE connection_id = ?1 AND pinned = 1
             ORDER BY pinned_at DESC, id DESC
         )
         UNION ALL
         SELECT id, command, pinned, created_at, pinned_at FROM (
            SELECT id, command, pinned, created_at, pinned_at FROM command_history
             WHERE connection_id = ?1 AND pinned = 0
             ORDER BY created_at DESC, id DESC
             LIMIT 50
         )
         ORDER BY pinned DESC, pinned_at DESC NULLS LAST, created_at DESC, id DESC"
    )?;
    let rows = stmt.query_map(params![connection_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)? != 0,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn set_command_history_pinned(conn: &Connection, id: i64, pinned: bool, pinned_at: Option<&str>) -> Result<()> {
    match pinned {
        true => conn.execute(
            "UPDATE command_history SET pinned = 1, pinned_at = ?2 WHERE id = ?1",
            params![id, pinned_at],
        )?,
        false => conn.execute(
            "UPDATE command_history SET pinned = 0, pinned_at = NULL WHERE id = ?1",
            params![id],
        )?,
    };
    Ok(())
}

pub fn delete_command_history(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM command_history WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn clear_command_history(conn: &Connection, connection_id: &str, include_pinned: bool) -> Result<()> {
    match include_pinned {
        true => conn.execute(
            "DELETE FROM command_history WHERE connection_id = ?1",
            params![connection_id],
        )?,
        false => conn.execute(
            "DELETE FROM command_history WHERE connection_id = ?1 AND pinned = 0",
            params![connection_id],
        )?,
    };
    Ok(())
}

// ============ Quick Commands ============
//
// User-defined reusable command snippets. `connection_id` is NULL for global
// scope (available on every server) or a `ConnectionConfig.id` for per-server
// scope. Multi-line commands are stored verbatim (with `\n`); line splitting
// for ordered execution happens in the frontend before `sshSend`.

/// `(id, connection_id, label, command, sort_order)` — raw column tuple for
/// the management listing, unwrapped into a struct in main.rs.
type QuickCommandTuple = (i64, Option<String>, String, String, i64);

/// Read a quick_commands row into [`QuickCommandTuple`].
fn read_quick_command_row(row: &rusqlite::Row) -> rusqlite::Result<QuickCommandTuple> {
    Ok((
        row.get::<_, i64>(0)?,
        row.get::<_, Option<String>>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, String>(3)?,
        row.get::<_, i64>(4)?,
    ))
}

pub fn add_quick_command(
    conn: &Connection,
    connection_id: Option<&str>,
    label: &str,
    command: &str,
    created_at: &str,
) -> Result<i64> {
    let tx = conn.unchecked_transaction()?;
    // Append at the end of the current scope's ordering. Branch on scope to
    // match NULL (global) rows correctly — `MAX(sort_order) ... WHERE id = ?`
    // would never match a global row.
    let next_order: i64 = if connection_id.is_some() {
        tx.query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM quick_commands WHERE connection_id = ?1",
            params![connection_id],
            |row| row.get(0),
        )?
    } else {
        tx.query_row(
            "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM quick_commands WHERE connection_id IS NULL",
            [],
            |row| row.get(0),
        )?
    };
    tx.execute(
        "INSERT INTO quick_commands (connection_id, label, command, sort_order, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![connection_id, label, command, next_order, created_at],
    )?;
    let new_id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(new_id)
}

pub fn list_quick_commands(
    conn: &Connection,
    connection_id: Option<&str>,
) -> Result<Vec<QuickCommandTuple>> {
    // Branch on scope to avoid relying on `connection_id IS ?1` NULL semantics
    // (whose behavior with a bound NULL can vary across SQLite versions).
    let (sql, scoped): (&str, bool) = if connection_id.is_some() {
        (
            "SELECT id, connection_id, label, command, sort_order FROM quick_commands
             WHERE connection_id = ?1 ORDER BY sort_order ASC, id ASC",
            true,
        )
    } else {
        (
            "SELECT id, connection_id, label, command, sort_order FROM quick_commands
             WHERE connection_id IS NULL ORDER BY sort_order ASC, id ASC",
            false,
        )
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = if scoped {
        stmt.query_map(params![connection_id], read_quick_command_row)?
    } else {
        stmt.query_map([], read_quick_command_row)?
    };
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn list_quick_commands_for_connection(
    conn: &Connection,
    connection_id: &str,
) -> Result<Vec<(i64, bool, String, String)>> {
    // Union of global + this connection's per-server commands. Global first
    // (is_global DESC) so shared commands surface above server-specific ones.
    let mut stmt = conn.prepare(
        "SELECT id, (connection_id IS NULL) AS is_global, label, command
         FROM quick_commands
         WHERE connection_id IS NULL OR connection_id = ?1
         ORDER BY is_global DESC, sort_order ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![connection_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)? != 0,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn update_quick_command(conn: &Connection, id: i64, label: &str, command: &str) -> Result<()> {
    // Scope is immutable — changing scope equals delete + re-add.
    conn.execute(
        "UPDATE quick_commands SET label = ?2, command = ?3 WHERE id = ?1",
        params![id, label, command],
    )?;
    Ok(())
}

pub fn update_quick_command_order(conn: &Connection, id: i64, sort_order: i64) -> Result<()> {
    conn.execute(
        "UPDATE quick_commands SET sort_order = ?2 WHERE id = ?1",
        params![id, sort_order],
    )?;
    Ok(())
}

pub fn delete_quick_command(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM quick_commands WHERE id = ?1", params![id])?;
    Ok(())
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
