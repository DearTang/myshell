import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ============ Types ============

export type ConnType = "ssh" | "sftp" | "ftp" | "local";

export type FtpTls = "none" | "implicit" | "explicit";

export type ProxyType = "none" | "socks5" | "http";

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
  /** Proxy type. "none" = direct. Backend reads proxy_* fields only when
   * this is "socks5" or "http". Default "none" for migrated connections. */
  proxy_type?: ProxyType;
  /** Proxy server host (encrypted at rest via proxy_host_enc). */
  proxy_host?: string;
  proxy_port?: number;
  proxy_username?: string;
  /** Transient plaintext. Stored in OS keyring under `{conn_id}.myshell.proxy`
   * when non-empty — never persisted to SQLite. Empty on edit means "keep
   * existing keyring value"; backend reads from keyring at connect time. */
  proxy_password?: string;
  /** Local terminal only (conn_type === "local"): shell executable to spawn
   * — e.g. "pwsh.exe", "cmd.exe", "wsl.exe", or an absolute path. Ignored for
   * ssh/sftp/ftp. Plain column (a program path isn't a secret). Matches the
   * Rust field name verbatim (serde default = snake_case, like the other
   * fields here). */
  shell_path?: string;
  /** Local terminal only: optional shell args (e.g. "-d Ubuntu"). */
  shell_args?: string;
  /** Optional command auto-executed right after the shell starts (e.g.
   * "claude" to launch on open). Currently honored for local terminals. */
  init_command?: string;
  /** Optional per-connection terminal font override (family name). When set,
   * takes precedence over the global terminal font for this connection's
   * tabs. Empty/undefined = use the global setting. Plain column. */
  terminal_font?: string;
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
  /** ConnectionConfig.id this tab was opened from. Stable across reconnects
   * (unlike sessionId which is a fresh UUID each connect). Used as the key
   * for command_history persistence so history survives tab close/reopen. */
  connectionId?: string;
  /** When true, this tab is part of the broadcast group: any keystroke in
   * any broadcast-group tab is mirrored to all other broadcast-group tabs
   * (must be terminal-type SSH tabs). SFTP/FTP tabs are excluded. */
  broadcast?: boolean;
  /** Connection status: "connecting" | "connected" | "disconnected" | "error" */
  status?: "connecting" | "connected" | "disconnected" | "error";
  /** Error message when status is "error" */
  errorMessage?: string;
  /** Connection config for reconnection */
  config?: ConnectionConfig;
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

export async function changeMasterPassword(
  oldPassphrase: string,
  newPassphrase: string
): Promise<void> {
  await invoke("change_master_password", { oldPassphrase, newPassphrase });
}

export async function verifyPassword(passphrase: string): Promise<boolean> {
  return await invoke("verify_password", { passphrase });
}

export interface LockoutInfo {
  consecutiveFailures: number;
  dailyFailures: number;
  lastFailureTime: number | null;
  lockoutRemaining: number | null;
  isLocked: boolean;
}

export async function getLockoutInfo(): Promise<LockoutInfo> {
  return await invoke("get_lockout_info");
}

export async function getConnectionPassword(id: string): Promise<string | null> {
  return await invoke("get_connection_password", { id });
}

export async function getConnectionProxyPassword(id: string): Promise<string | null> {
  return await invoke("get_connection_proxy_password", { id });
}

// ============ Backup API ============

export interface BackupInfo {
  version: string;
  timestamp: number;
  files: string[];
  timestampStr: string;
}

export async function listBackups(): Promise<BackupInfo[]> {
  return await invoke("list_backups");
}

export async function rollbackBackup(version: string): Promise<string> {
  return await invoke("rollback_backup", { version });
}

export async function getAppVersion(): Promise<string> {
  return await invoke("get_app_version");
}

export async function getPreviousVersion(): Promise<string | null> {
  return await invoke("get_previous_version");
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

// ============ Command History API ============
//
// Per-connection shell command history with optional pinning. Pinned
// entries bypass the 50-row rolling window and surface at the top of the
// list returned by listCommandHistory. History is keyed by ConnectionConfig
// id (not sessionId) so it survives reconnects.

export interface CommandHistoryItem {
  id: number;
  command: string;
  pinned: boolean;
  createdAt: string;
}

export async function addCommandHistory(connectionId: string, command: string): Promise<number> {
  return await invoke("add_command_history", { connectionId, command });
}

export async function listCommandHistory(connectionId: string): Promise<CommandHistoryItem[]> {
  return await invoke("list_command_history", { connectionId });
}

export async function setCommandHistoryPinned(id: number, pinned: boolean): Promise<void> {
  await invoke("set_command_history_pinned", { id, pinned });
}

export async function deleteCommandHistory(id: number): Promise<void> {
  await invoke("delete_command_history", { id });
}

export async function clearCommandHistory(connectionId: string, includePinned: boolean): Promise<void> {
  await invoke("clear_command_history", { connectionId, includePinned });
}

// ============ Quick Commands API ============
//
// User-defined reusable command snippets. `connectionId` is null for global
// scope (every server) or a ConnectionConfig.id for per-server scope. Stored
// as plaintext (commands aren't secrets). Keyed by connectionId (stable across
// reconnects), same convention as Command History.

export interface QuickCommandItem {
  id: number;
  connectionId: string | null;
  label: string;
  command: string;
  sortOrder: number;
}

/** Flattened item for the terminal execution panel: union of global + current
 * connection commands, with isGlobal for grouping. */
export interface QuickCommandExecItem {
  id: number;
  isGlobal: boolean;
  label: string;
  command: string;
}

export async function addQuickCommand(
  connectionId: string | null,
  label: string,
  command: string
): Promise<number> {
  return await invoke("add_quick_command", { connectionId, label, command });
}

export async function listQuickCommands(connectionId: string | null): Promise<QuickCommandItem[]> {
  return await invoke("list_quick_commands", { connectionId });
}

export async function listQuickCommandsForConnection(
  connectionId: string
): Promise<QuickCommandExecItem[]> {
  return await invoke("list_quick_commands_for_connection", { connectionId });
}

export async function updateQuickCommand(
  id: number,
  label: string,
  command: string
): Promise<void> {
  await invoke("update_quick_command", { id, label, command });
}

export async function updateQuickCommandOrder(id: number, sortOrder: number): Promise<void> {
  await invoke("update_quick_command_order", { id, sortOrder });
}

export async function deleteQuickCommand(id: number): Promise<void> {
  await invoke("delete_quick_command", { id });
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

// ============ Local Terminal API ============
//
// Local PTY terminals (conn_type === "local"). They reuse the SSH event
// channel (onSshOutput / onSshClosed), so only these four commands differ —
// TerminalPanel branches on connType to pick ssh_* vs local_*.

export async function localConnect(config: ConnectionConfig): Promise<string> {
  return await invoke("local_connect", { config });
}

export async function localSend(sessionId: string, data: string): Promise<void> {
  await invoke("local_send", { sessionId, data });
}

export async function localResize(
  sessionId: string,
  cols: number,
  rows: number
): Promise<void> {
  await invoke("local_resize", { sessionId, cols, rows });
}

/** Installed font family names on the host (sorted, deduped) — for the
 * terminal font picker in Settings and per-connection config. Empty list on
 * failure (picker then degrades to free-text entry). */
export async function listSystemFonts(): Promise<string[]> {
  return invoke<string[]>("list_system_fonts");
}

export async function localDisconnect(sessionId: string): Promise<void> {
  await invoke("local_disconnect", { sessionId });
}

// ============ Elevation (run as admin) ============

/** Whether MyShell is running elevated (admin on Windows, root on Unix). */
export async function isElevated(): Promise<boolean> {
  return invoke<boolean>("is_elevated");
}

/**
 * Re-launch MyShell elevated via the UAC consent dialog; the current process
 * exits after launching. Rejects if the user cancels UAC (we then stay as-is).
 */
export async function restartAsAdmin(): Promise<void> {
  await invoke("restart_as_admin");
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

// ============ SFTP Transfer (upload / download) ============
//
// Batch, files-only, overwrite. Progress streams via onSftpTransferProgress
// (once per file + throttled within large files); completion — with any
// per-file errors — via onSftpTransferDone. Fatal failures (no session,
// channel can't open) reject the returned promise; there is no separate error
// event. Subscribe BEFORE calling so early progress events aren't missed.

export interface SftpTransferProgressPayload {
  requestId: string;
  /** "upload" | "download" — drives the overlay label. */
  phase: string;
  currentFile: string;
  fileIndex: number;
  fileCount: number;
  bytesDone: number;
  bytesTotal: number;
}

export interface SftpTransferDonePayload {
  requestId: string;
  errors: string[];
}

export async function sftpUpload(
  sessionId: string,
  localPaths: string[],
  remoteDestDir: string,
  requestId: string
): Promise<void> {
  await invoke("sftp_upload", { sessionId, localPaths, remoteDestDir, requestId });
}

export async function sftpDownload(
  sessionId: string,
  remotePaths: string[],
  localDestDir: string,
  requestId: string
): Promise<void> {
  await invoke("sftp_download", { sessionId, remotePaths, localDestDir, requestId });
}

/** Live progress for a transfer (filtered by requestId). */
export function onSftpTransferProgress(
  requestId: string,
  handler: (p: SftpTransferProgressPayload) => void
): Promise<UnlistenFn> {
  return listen<SftpTransferProgressPayload>("sftp_transfer_progress", (event) => {
    if (event.payload.requestId === requestId) handler(event.payload);
  });
}

/** Transfer finished — handler receives the list of per-file errors (empty if all OK). */
export function onSftpTransferDone(
  requestId: string,
  handler: (errors: string[]) => void
): Promise<UnlistenFn> {
  return listen<SftpTransferDonePayload>("sftp_transfer_done", (event) => {
    if (event.payload.requestId === requestId) handler(event.payload.errors);
  });
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

// ============ AI API ============

export type AiProvider = "claude" | "openai" | "ollama";

export interface ChatMessage {
  role: "user" | "assistant" | "system";
  content: string;
}

/** Live terminal context attached to a chat request. Serialized into the
 * system prompt so the model sees the user's current shell state alongside
 * their question. */
export interface AiContext {
  terminalOutput?: string;
  selection?: string;
  inspectData?: string;
  /** Shell hint ("bash" / "powershell" / "pwsh" / "zsh"…) so generated
   * commands use the right syntax. */
  shellHint?: string;
  /** Active connection type — lets the system prompt describe the runtime
   * (remote SSH server vs. the user's own machine). */
  connType?: ConnType;
}

/** Non-secret AI config returned by getAiSettings. The API key is never sent
 * to the frontend — `hasKey` only indicates one is stored in the vault. */
export interface AiSettings {
  provider: AiProvider;
  model?: string;
  baseUrl?: string;
  proxyUrl?: string;
  hasKey: boolean;
  temperature: number;
}

/** Streaming payloads, keyed by requestId so concurrent streams don't cross. */
export interface AiTokenPayload {
  requestId: string;
  token: string;
}

export interface AiDonePayload {
  requestId: string;
}

export interface AiErrorPayload {
  requestId: string;
  error: string;
}

/**
 * Stream a chat completion. Tokens arrive via onAiToken(requestId); the stream
 * ends with onAiDone or onAiError. Subscribe BEFORE calling so no early
 * tokens are missed.
 */
export function aiChat(
  requestId: string,
  messages: ChatMessage[],
  system: string | null,
  context: AiContext | null
): Promise<void> {
  return invoke("ai_chat", { requestId, messages, system, context });
}

/** Run the read-only health-inspection script on an SSH/Linux host and stream
 * an AI health report. */
export function aiInspectHealthSsh(
  sessionId: string,
  requestId: string
): Promise<void> {
  return invoke("ai_inspect_health_ssh", { sessionId, requestId });
}

/** Run the read-only health-inspection script on the local machine and stream
 * an AI health report. */
export function aiInspectHealthLocal(requestId: string): Promise<void> {
  return invoke("ai_inspect_health_local", { requestId });
}

/** Read the AI provider config (no key — only `hasKey`). Works with the vault
 * locked, so the settings form can render before unlock. */
export function getAiSettings(): Promise<AiSettings> {
  return invoke("get_ai_settings");
}

/** Save the AI provider config. `apiKey`: a non-empty string re-encrypts &
 * overwrites; "" / undefined leaves the existing key. Requires the vault
 * unlocked. */
export function saveAiSettings(
  provider: AiProvider,
  model: string | null,
  baseUrl: string | null,
  proxyUrl: string | null,
  apiKey: string | null,
  temperature: number
): Promise<void> {
  return invoke("save_ai_settings", {
    provider,
    model,
    baseUrl,
    proxyUrl,
    apiKey,
    temperature,
  });
}

/** Subscribe to streaming tokens for a specific request. */
export function onAiToken(
  requestId: string,
  handler: (token: string) => void
): Promise<UnlistenFn> {
  return listen<AiTokenPayload>("ai_token", (event) => {
    if (event.payload.requestId === requestId) handler(event.payload.token);
  });
}

/** Subscribe to stream-completion for a specific request. */
export function onAiDone(
  requestId: string,
  handler: () => void
): Promise<UnlistenFn> {
  return listen<AiDonePayload>("ai_done", (event) => {
    if (event.payload.requestId === requestId) handler();
  });
}

/** Subscribe to stream-error for a specific request. */
export function onAiError(
  requestId: string,
  handler: (error: string) => void
): Promise<UnlistenFn> {
  return listen<AiErrorPayload>("ai_error", (event) => {
    if (event.payload.requestId === requestId) handler(event.payload.error);
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

// ============ File Utilities ============

/**
 * Read a local image file and return a base64 data URL suitable for CSS
 * background-image.  Max file size: 8 MiB.
 */
export async function readFileBase64(path: string): Promise<string> {
  return await invoke("read_file_base64", { path });
}

/**
 * Read a local text file (PEM private key, etc.) for the key picker.
 * The backend rejects anything over 1 MiB, non-UTF-8, or not starting with
 * a PEM `-----BEGIN` marker.
 */
export async function readTextFile(path: string): Promise<string> {
  return await invoke("read_text_file", { path });
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
