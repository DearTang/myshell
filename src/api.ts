import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ============ Types ============

export type ConnType = "ssh" | "sftp" | "ftp";

export type FtpTls = "none" | "implicit" | "explicit";

export interface ConnectionConfig {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_method: string;
  password?: string;
  /** Private key PEM content (transient — encrypted at rest in the vault).
   * ssh.rs loads from this string via decode_secret_key; no file IO. */
  private_key_pem?: string;
  conn_type?: ConnType;
  group_path?: string;
  ftp_tls?: FtpTls;
  ftp_passive?: boolean;
  created_at: string;
}

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  permissions: string;
  modified: string;
}

export interface Tab {
  id: string;
  name: string;
  sessionId?: string;
  type: "terminal" | "sftp" | "ftp";
  connType?: ConnType;
  ftpSessionId?: string;
  /** When true, this tab is part of the broadcast group: any keystroke in
   * any broadcast-group tab is mirrored to all other broadcast-group tabs
   * (must be terminal-type SSH tabs). SFTP/FTP tabs are excluded. */
  broadcast?: boolean;
}

// ============ Connection API ============

export async function getConnections(): Promise<ConnectionConfig[]> {
  return await invoke("get_connections");
}

// ============ Vault API ============
//
// Master-password gate. App.tsx calls vaultStatus() on startup to decide
// between rendering the setup screen (no on-disk vault yet) or the unlock
// screen (vault exists, master key not yet loaded into AppState). Until
// one of these resolves, all connection commands return "Vault 未解锁".

export interface VaultStatus {
  initialized: boolean;
  unlocked: boolean;
}

export async function vaultStatus(): Promise<VaultStatus> {
  return await invoke("vault_status");
}

export async function setupVault(passphrase: string): Promise<void> {
  await invoke("setup_vault", { passphrase });
}

export async function unlockVault(passphrase: string): Promise<void> {
  await invoke("unlock_vault", { passphrase });
}

export async function lockVault(): Promise<void> {
  await invoke("lock_vault");
}

export async function saveConnection(config: ConnectionConfig): Promise<void> {
  await invoke("save_connection", { config });
}

export async function deleteConnection(id: string): Promise<void> {
  await invoke("delete_connection", { id });
}

export async function copyConnection(id: string): Promise<ConnectionConfig> {
  return await invoke("copy_connection", { srcId: id });
}

// ============ Import/Export API ============
//
// Encrypted dump format: AES-256-GCM(pbkdf2-hmac-sha256(passphrase, salt, 200k) || plaintext).
// Plaintext is JSON with all connection fields INCLUDING passwords pulled from
// the OS keychain. The dump file contains NO plaintext credentials — opening
// it in a text editor shows only base64 ciphertext + KDF params.

export async function exportConnections(passphrase: string, path: string): Promise<number> {
  return await invoke("export_connections", { passphrase, path });
}

export async function importConnections(passphrase: string, path: string): Promise<number> {
  return await invoke("import_connections", { passphrase, path });
}

// ============ Folder API ============

export async function listFolders(): Promise<string[]> {
  return await invoke("list_folders");
}

export async function saveFolder(path: string): Promise<void> {
  await invoke("save_folder", { path });
}

export async function deleteFolder(path: string): Promise<void> {
  await invoke("delete_folder", { path });
}

export async function renameFolder(oldPath: string, newPath: string): Promise<void> {
  await invoke("rename_folder", { oldPath, newPath });
}

// ============ Server Info API ============

export interface ServerInfo {
  osPretty: string;
  kernel: string;
  cpuCores: number;
  memTotalBytes: number;
  memUsedBytes: number;
  memUsagePct: number;
  cpuUsagePct: number;
  diskTotalBytes: number;
  diskUsedBytes: number;
  diskTotalPct: number;
  diskMaxDev: string;
  diskMaxMount: string;
  diskMaxSize: string;
  diskMaxUsed: string;
  diskMaxPct: number;
  stale: boolean;
}

export async function sshGetServerInfo(sessionId: string): Promise<ServerInfo> {
  return await invoke("ssh_get_server_info", { sessionId });
}

// ============ SSH API ============

export async function sshConnect(config: ConnectionConfig): Promise<string> {
  return await invoke("ssh_connect", { config });
}

export async function sshSend(sessionId: string, data: string): Promise<void> {
  await invoke("ssh_send", { sessionId, data });
}

export async function sshResize(
  sessionId: string,
  cols: number,
  rows: number
): Promise<void> {
  await invoke("ssh_resize", { sessionId, cols, rows });
}

export async function sshDisconnect(sessionId: string): Promise<void> {
  await invoke("ssh_disconnect", { sessionId });
}

// ============ SFTP API ============

export async function sftpListDir(
  sessionId: string,
  path: string
): Promise<FileEntry[]> {
  return await invoke("sftp_list_dir", { sessionId, path });
}

export async function sftpMkdir(
  sessionId: string,
  path: string
): Promise<void> {
  await invoke("sftp_mkdir", { sessionId, path });
}

export async function sftpRemove(
  sessionId: string,
  path: string
): Promise<void> {
  await invoke("sftp_remove", { sessionId, path });
}

export async function sftpRename(
  sessionId: string,
  oldPath: string,
  newPath: string
): Promise<void> {
  await invoke("sftp_rename", { sessionId, oldPath, newPath });
}

// ============ FTP API ============

export async function ftpConnect(config: ConnectionConfig): Promise<string> {
  return await invoke("ftp_connect", { config });
}

export async function ftpListDir(
  sessionId: string,
  path: string
): Promise<FileEntry[]> {
  return await invoke("ftp_list_dir", { sessionId, path });
}

export async function ftpMkdir(sessionId: string, path: string): Promise<void> {
  await invoke("ftp_mkdir", { sessionId, path });
}

export async function ftpRemove(
  sessionId: string,
  path: string,
  isDir: boolean
): Promise<void> {
  await invoke("ftp_remove", { sessionId, path, isDir });
}

export async function ftpRename(
  sessionId: string,
  oldPath: string,
  newPath: string
): Promise<void> {
  await invoke("ftp_rename", { sessionId, oldPath, newPath });
}

export async function ftpDisconnect(sessionId: string): Promise<void> {
  await invoke("ftp_disconnect", { sessionId });
}

// ============ SSH Event Subscriptions ============

export interface SshOutputPayload {
  sessionId: string;
  data: number[];
}

export interface SshExitPayload {
  sessionId: string;
  code: number;
}

/**
 * Subscribe to terminal output for a specific session.
 * The handler receives raw bytes; pass them directly to xterm's term.write().
 * Returns an unlisten function — call it on cleanup.
 */
export function onSshOutput(
  sessionId: string,
  handler: (data: Uint8Array) => void
): Promise<UnlistenFn> {
  return listen<SshOutputPayload>("ssh_output", (event) => {
    if (event.payload.sessionId === sessionId) {
      handler(new Uint8Array(event.payload.data));
    }
  });
}

/**
 * Subscribe to connection-closed events for a specific session.
 * Fires when the server sends EOF/Close or the channel dies unexpectedly.
 */
export function onSshClosed(
  sessionId: string,
  handler: () => void
): Promise<UnlistenFn> {
  return listen<string>("ssh_closed", (event) => {
    if (event.payload === sessionId) handler();
  });
}

/**
 * Subscribe to remote exit-status events for a specific session.
 * informational — emitted when the remote shell reports an exit code.
 */
export function onSshExit(
  sessionId: string,
  handler: (code: number) => void
): Promise<UnlistenFn> {
  return listen<SshExitPayload>("ssh_exit", (event) => {
    if (event.payload.sessionId === sessionId) handler(event.payload.code);
  });
}

// ============ ZMODEM API ============

export async function sshSendZmodem(sessionId: string, data: Uint8Array): Promise<void> {
  await invoke("ssh_send_zmodem", { sessionId, data: Array.from(data) });
}

export async function sshSendZmodemAbort(sessionId: string): Promise<void> {
  await invoke("ssh_send_zmodem_abort", { sessionId });
}

export interface ZmodemReadOpenResult {
  id: string;
  size: number;
  mtime: number;
}

export interface ZmodemWriteOpenResult {
  id: string;
  existingSize: number;
}

export async function rzOpenRead(path: string): Promise<ZmodemReadOpenResult> {
  return await invoke("rz_open_read", { path });
}

export async function rzReadChunk(
  id: string,
  offset: number,
  len: number
): Promise<Uint8Array> {
  const bytes: number[] = await invoke("rz_read_chunk", { id, offset, len });
  return new Uint8Array(bytes);
}

export async function rzClose(id: string): Promise<void> {
  await invoke("rz_close", { id });
}

export async function szOpenWrite(path: string): Promise<ZmodemWriteOpenResult> {
  return await invoke("sz_open_write", { path });
}

export async function szWriteChunk(
  id: string,
  offset: number,
  bytes: Uint8Array
): Promise<void> {
  await invoke("sz_write_chunk", { id, offset, bytes: Array.from(bytes) });
}

export async function szClose(id: string): Promise<void> {
  await invoke("sz_close", { id });
}

// ============ ZMODEM Event Subscriptions ============

export interface ZmodemStartPayload {
  sessionId: string;
  direction: "upload" | "download" | "auto";
}

/**
 * Fires when the Rust backend detects a ZMODEM auto-start sequence
 * (`**\x18A/B/C`) anywhere in the terminal stream. Subscribe *before*
 * connecting — the start signal can arrive inside the first data burst.
 */
export function onZmodemStart(
  sessionId: string,
  handler: (direction: string) => void
): Promise<UnlistenFn> {
  return listen<ZmodemStartPayload>("zmodem_start", (event) => {
    if (event.payload.sessionId === sessionId) handler(event.payload.direction);
  });
}

/**
 * Raw ZMODEM protocol bytes — pass straight to zmodem.js's Session.parse()
 * on the first chunk and Session.consume() afterwards. Already filtered
 * (no terminal output mixed in).
 */
export function onZmodemRaw(
  sessionId: string,
  handler: (data: Uint8Array) => void
): Promise<UnlistenFn> {
  return listen<SshOutputPayload>("zmodem_raw", (event) => {
    if (event.payload.sessionId === sessionId) {
      handler(new Uint8Array(event.payload.data));
    }
  });
}

/**
 * Fires when the Rust backend detects end-of-ZMODEM (ZFIN or 5× CAN).
 * After this, subsequent bytes belong to the terminal again.
 */
export function onZmodemEnd(
  sessionId: string,
  handler: () => void
): Promise<UnlistenFn> {
  return listen<string>("zmodem_end", (event) => {
    if (event.payload === sessionId) handler();
  });
}
