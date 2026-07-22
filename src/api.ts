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

/**
 * Move a connection into another folder by rewriting only its `group_path`.
 * Single-column UPDATE by id equality — does NOT re-encrypt host/user/key or
 * touch the OS keyring (unlike saveConnection). `newGroupPath` of "/" unfiles.
 */
export async function moveConnection(connId: string, newGroupPath: string): Promise<void> {
  await invoke("move_connection", { connId, newGroupPath });
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

/** Delete a folder. Cascade-deletes descendants and moves any child
 * connections to root; the backend returns a one-line summary of what it did.
 * The frontend confirms with the user first when the folder is non-empty. */
export async function deleteFolder(path: string): Promise<string> {
  return invoke("delete_folder", { path });
}

export async function renameFolder(oldPath: string, newPath: string): Promise<void> {
  await invoke("rename_folder", { oldPath, newPath });
}

/** Rename a connection without re-saving the full encrypted row. `name` is a
 * plaintext column, so this is a lightweight single-column UPDATE. */
export async function renameConnection(id: string, newName: string): Promise<void> {
  await invoke("rename_connection", { id, newName });
}

/** A connection in the recycle bin. The backend flattens its ConnectionConfig
 * fields alongside `deleted_at`, so this mirrors that shape. */
export interface DeletedConnection extends ConnectionConfig {
  deletedAt: string;
}

/** List soft-deleted connections (newest-deleted first) for the recycle bin. */
export async function getDeletedConnections(): Promise<DeletedConnection[]> {
  return invoke("get_deleted_connections");
}

/** Restore a soft-deleted connection by clearing its deleted_at. */
export async function restoreConnection(id: string): Promise<void> {
  await invoke("restore_connection", { id });
}

/** Permanently delete a single recycled connection (also purges its keyring). */
export async function purgeConnection(id: string): Promise<void> {
  await invoke("purge_connection", { id });
}

/** Empty the recycle bin: hard-delete every soft-deleted connection + keyring. */
export async function purgeAllDeletedConnections(): Promise<void> {
  await invoke("purge_all_deleted_connections");
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

// ============ Connection Test ============

/**
 * Probe a connection config WITHOUT registering a persistent session or
 * opening a tab. Mirrors the save flow's credential handling: typed
 * password/proxy_password are passed inline; when absent and `config.id`
 * exists, the backend resolves them from the keyring (edit mode). Wrapped in
 * a server-side 15s timeout. Ok = success message (e.g. "连接成功（123 ms，认证方式=密码）");
 * Err = failure reason (auth / network / timeout).
 */
export async function testConnection(config: ConnectionConfig): Promise<string> {
  return await invoke("test_connection", { config });
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

export type AiProvider =
  | "claude"
  | "openai"
  | "ollama"
  | "openai_compatible"
  | "anthropic_compatible";

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

/** Optional per-field overrides for `aiTestSettings`. Any field that is
 * undefined or empty-string falls back to the vault-stored value. `apiKey`
 * is the key one: pass the unsaved typed key to validate before saving; pass
 * "" / undefined to test the vault-stored key. */
export interface AiTestOverrides {
  supplierId?: number;
  provider?: string;
  model?: string;
  baseUrl?: string;
  proxyUrl?: string;
  apiKey?: string;
  temperature?: number;
}

/**
 * Probe the AI provider config with a minimal non-streaming request. Tests
 * the CURRENT FORM VALUES when overrides are provided — does NOT require
 * saving first. Empty/undefined override fields fall back to the vault-stored
 * value. Returns a success message or throws the provider's error (status
 * code + truncated body). Never emits ai_token/ai_done events.
 */
export async function aiTestSettings(overrides?: AiTestOverrides): Promise<string> {
  return await invoke("ai_test_settings", { overrides: overrides ?? null });
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

// ── Multi-model management ──────────────────────────────────────────────

/** A single model belonging to a supplier (no secrets — just id + label). */
export interface SupplierModel {
  id: number;
  supplierId: number;
  modelId: string;
  label?: string;
  sortOrder: number;
}

/** Model info returned to the frontend (no api_key — only `hasKey`). */
export interface AiModel {
  id: number;
  name: string;
  provider: AiProvider;
  modelId: string;
  baseUrl?: string;
  hasKey: boolean;
  proxyUrl?: string;
  temperature: number;
  isPreset: boolean;
  isEnabled: boolean;
  sortOrder: number;
  /** Full model list for this supplier (includes the primary modelId). */
  models: SupplierModel[];
}

/** List all configured AI models (presets + user-created). */
export function listAiModels(): Promise<AiModel[]> {
  return invoke("list_ai_models");
}

/** Get the currently active model id. Returns null if using legacy config. */
export function getActiveAiModelId(): Promise<number | null> {
  return invoke("get_active_ai_model_id");
}

/** Save (create or update) an AI model. Pass `id` to update, omit to create.
 *  `models` syncs the full model list for this supplier (replaces all). */
export function saveAiModel(params: {
  id?: number;
  name: string;
  provider: AiProvider;
  modelId: string;
  baseUrl?: string;
  apiKey?: string;
  proxyUrl?: string;
  temperature: number;
  models?: { modelId: string; label?: string }[];
}): Promise<number> {
  return invoke("save_ai_model", {
    id: params.id ?? null,
    name: params.name,
    provider: params.provider,
    modelId: params.modelId,
    baseUrl: params.baseUrl || null,
    apiKey: params.apiKey || null,
    proxyUrl: params.proxyUrl || null,
    temperature: params.temperature,
    models: params.models ?? null,
  });
}

/** Delete a user-created AI model. Presets cannot be deleted. */
export function deleteAiModel(id: number): Promise<void> {
  return invoke("delete_ai_model", { id });
}

/** Switch the active AI model. `modelString` selects the specific model_id
 *  within the supplier (undefined = fall back to the supplier's primary). */
export function setActiveAiModel(id: number, modelString?: string): Promise<void> {
  return invoke("set_active_ai_model", { id, modelString });
}

// ── Supplier model management ─────────────────────────────────────────

/** List all models for a given supplier. */
export function listSupplierModels(supplierId: number): Promise<SupplierModel[]> {
  return invoke("list_supplier_models", { supplierId });
}

/** Add a model to a supplier. Returns the new row id. */
export function addSupplierModel(
  supplierId: number,
  modelId: string,
  label?: string,
): Promise<number> {
  return invoke("add_supplier_model", { supplierId, modelId, label });
}

/** Remove a single supplier model row by id. */
export function removeSupplierModel(id: number): Promise<void> {
  return invoke("remove_supplier_model", { id });
}

/** Toggle whether a supplier is selectable in the AI chat model picker. */
export function toggleAiModelEnabled(id: number, enabled: boolean): Promise<void> {
  return invoke("toggle_ai_model_enabled", { id, enabled });
}

/**
 * Fetch available models from a provider's /models endpoint (OpenAI format).
 * Returns sorted model IDs. Only works for OpenAI-compatible / Ollama
 * providers — Anthropic and Claude official APIs have no such endpoint.
 */
export function fetchProviderModels(
  provider: AiProvider,
  baseUrl: string,
  apiKey: string,
): Promise<string[]> {
  return invoke("fetch_provider_models", { provider, baseUrl, apiKey });
}

/**
 * Fetch available models for a specific supplier by id. Decrypts the stored
 * key server-side — use this after save so plaintext keys never reach the
 * frontend. Pass overrideKey only during create/edit-before-save.
 */
export function fetchModelsForSupplier(
  supplierId: number,
  overrideKey?: string,
): Promise<string[]> {
  return invoke("fetch_models_for_supplier", { supplierId, overrideKey: overrideKey ?? null });
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

// ============ Feedback / Diagnostics ============

/** Log info returned by `get_feedback_log` for the feedback dialog. */
export interface FeedbackLogInfo {
  /** Absolute path to the logs dir, for the "open folder" button. */
  logDir: string;
  /** Today's (+ yesterday's) log content, already scrubbed of hosts/IPs. */
  content: string;
  /** True if the content was tail-truncated (log was larger than 200 KiB). */
  truncated: boolean;
}

/**
 * Read the current + previous day's log for the feedback dialog. The backend
 * scrubs IPs / `user@host` targets a second time, so the content is safe to
 * ship in a feedback email without leaking server addresses.
 */
export async function getFeedbackLog(): Promise<FeedbackLogInfo> {
  return await invoke("get_feedback_log");
}

/**
 * Reveal a path in the OS file explorer. Backend whitelists the MyShell log
 * dir only — arbitrary paths are rejected. Used by the feedback dialog's
 * "open feedback / log folder" button.
 */
export async function revealPath(path: string): Promise<void> {
  await invoke("reveal_path", { path });
}

/**
 * Save a feedback zip (built in the frontend with fflate) into the MyShell
 * feedback dir. Returns the full saved path so the UI can offer "open folder".
 * The filename is sanitized server-side; only alphanumerics / - _ . survive.
 */
export async function saveFeedbackZip(
  filename: string,
  data: Uint8Array,
): Promise<string> {
  // Tauri serializes Vec<u8> from a JS array of numbers, not a Uint8Array
  // directly — convert to a plain array.
  return await invoke("save_feedback_zip", {
    filename,
    data: Array.from(data),
  });
}

/**
 * Remove all feedback zip files from the feedback directory. Called when the
 * feedback dialog closes to prevent disk accumulation of old packages.
 */
export async function clearFeedbackDir(): Promise<void> {
  await invoke("clear_feedback_dir");
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

// ============ Update check ============

/**
 * Result of a single update check against Gitee's latest release.
 * Mirrors the Rust `UpdateInfo` struct (snake_case kept intentionally — these
 * field names carry no meaning conflict and match the wire payload exactly).
 *
 * On any failure (network/parse/no release) `error` is set and the rest are
 * empty/false — the check never throws. Treat a non-empty `error` or
 * `has_update === false` as "no actionable update info".
 */
export interface UpdateInfo {
  current_version: string;
  latest_version: string;
  has_update: boolean;
  /** html_url of the release; fallback destination for the download button. */
  release_url: string;
  /** First asset's browser_download_url, falling back to release_url. */
  download_url: string;
  /** Truncated release notes body (Markdown). */
  notes: string;
  /** created_at of the release, raw API string. */
  published_at: string;
  /** Unix seconds when the check ran. */
  checked_at: number;
  /** Present (non-empty) when the check failed. */
  error: string | null;
  /** "auto" = built-in download+install (Windows); "browser" = open release page in default browser (Linux/macOS). */
  update_strategy: string;
}

/** Ask the Rust backend to fetch the latest Gitee release and compare. */
export async function checkForUpdates(): Promise<UpdateInfo> {
  return await invoke("check_for_updates");
}

/** Open an external http(s) URL in the system default browser. */
export async function openExternalUrl(url: string): Promise<void> {
  await invoke("open_external_url", { url });
}

/** Payload of the `update_download_progress` event from `downloadUpdate`. */
export interface DownloadProgress {
  downloaded: number;
  total: number;
}

/**
 * Download the installer `.exe` to the OS temp dir, streaming progress via the
 * `update_download_progress` event. Resolves with the temp file path; pass it
 * to `installUpdate`. Rejects on network/HTTP/write failure (caller falls
 * back to a browser download).
 */
export async function downloadUpdate(url: string): Promise<string> {
  return await invoke("download_update", { url });
}

/**
 * Subscribe to download-progress events. `total` is 0 when the server didn't
 * report Content-Length (indeterminate). Returns an unlisten fn.
 */
export function onUpdateDownloadProgress(
  handler: (p: DownloadProgress) => void
): Promise<UnlistenFn> {
  return listen<DownloadProgress>("update_download_progress", (event) => {
    handler(event.payload);
  });
}

/**
 * Launch the downloaded installer (NSIS) and exit the app so it can replace
 * the binaries. The installer triggers a UAC prompt. This call does not
 * return normally — the app exits during it.
 */
export async function installUpdate(path: string): Promise<void> {
  await invoke("install_update", { path });
}

// ============ Renderer / GPU acceleration ============
//
// WebView2 (the system webview on Windows) renders via the GPU. On some
// GPU/driver combinations the WebGL/canvas compositing misbehaves — which is
// the root cause behind the "terminal cursor invisible / selection highlight
// invisible" reports. This toggle persists a flag to a Rust-managed file so
// the Rust side can read it on the NEXT launch (before WebView2 is created)
// and seed WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--disable-gpu. The flag
// therefore only takes effect after an app restart, which the UI surfaces.

/** Read the persisted GPU-off flag. True = --disable-gpu applies on launch. */
export async function getGpuAccelerationDisabled(): Promise<boolean> {
  return invoke<boolean>("get_gpu_acceleration_disabled");
}

/**
 * Persist the GPU-off flag. Takes effect on the NEXT app launch (the env var
 * must be set before WebView2 initializes). Returns nothing on success.
 */
export async function setGpuAccelerationDisabled(disabled: boolean): Promise<void> {
  await invoke("set_gpu_acceleration_disabled", { disabled });
}

// ============ Attachment directory + screenshot ============
//
// The attachment directory is where terminal screenshots (and future MCP
// attachments) are auto-saved. Configured by the user in Settings → MCP 支持.
// `null` means "not configured yet" — the GUI prompts the user to set one on
// first screenshot attempt.

/** Read the configured attachment directory. `null` if not yet set. */
export async function getAttachmentDir(): Promise<string | null> {
  return invoke<string | null>("get_attachment_dir");
}

/**
 * Persist the attachment directory. Validates the path (creates it if needed)
 * and returns the canonical absolute path that was actually stored.
 */
export async function setAttachmentDir(dir: string): Promise<string> {
  return invoke<string>("set_attachment_dir", { dir });
}

/**
 * Save a PNG screenshot to the attachment directory.
 *
 * @param dataUrl  `data:image/png;base64,...` URL from canvas.toDataURL("image/png")
 * @param connectionName  Used to build a human-friendly filename
 * @returns Absolute path of the saved file
 */
export async function saveScreenshot(dataUrl: string, connectionName: string): Promise<string> {
  return invoke<string>("save_screenshot", { dataUrl, connectionName });
}

/**
 * Open a file/folder in the OS file manager. On Windows, selects the file in
 * Explorer (like "Show in Folder"). On macOS, reveals in Finder. On Linux,
 * opens the containing directory.
 */
export async function showInFolder(path: string): Promise<void> {
  await invoke("show_in_folder", { path });
}

// ============ MCP command confirmation rules ============
//
// Configurable whitelist + blacklist (regex) controlling which ssh_exec
// commands skip the human-confirmation dialog. Editable from Settings → MCP.

export interface CommandRules {
  /** Regex patterns (case-insensitive). A match → confirm, unless whitelist exempts. */
  blacklist: string[];
  /** Regex patterns (case-insensitive). A match → exempt from confirmation. */
  whitelist: string[];
  /** If true, commands matching NEITHER list also confirm. Default false. */
  confirm_unknown: boolean;
  /** If true, ssh_exec runs in a visible GUI terminal tab. Default true. */
  show_in_gui: boolean;
}

/** Read the configured command rules. Returns defaults if not yet configured. */
export async function getCommandRules(): Promise<CommandRules> {
  return invoke<CommandRules>("get_command_rules");
}

/** Persist command rules to the JSON config file. */
export async function setCommandRules(rules: CommandRules): Promise<void> {
  await invoke("set_command_rules", { rules });
}

/**
 * Deliver the result of an MCP `exec_in_tab` request back to the waiting IPC
 * listener. Called by the frontend after it captures the command output from
 * the terminal tab.
 *
 * @param requestId  The UUID generated by the IPC listener for this exec.
 * @param result     `{ ok: boolean, stdout?: string, exit_code?: number, error?: string }`
 */
export async function mcpExecResult(requestId: string, result: Record<string, unknown>): Promise<void> {
  await invoke("mcp_exec_result", { requestId, result });
}

// ============ Frontend log forwarding ============
//
// The webview's console isn't persisted on the user's machine, so to diagnose
// frontend-side anomalies (render crashes, unhandled promise rejections) we
// forward them to the Rust backend, which writes them into the SAME daily log
// file as the Rust output — tagged `[frontend]`. main.tsx wires the global
// error/rejection handlers to this.

/** Severity levels accepted by the backend. Anything else is coerced to info. */
export type FrontendLogLevel = "error" | "warn" | "info" | "debug";

/**
 * Forward a single frontend log line into the shared backend log file.
 * `message` should already be a single line (multi-line stacks pre-joined);
 * the backend re-scrubs newlines defensively. Fire-and-forget — a logging
 * failure must never throw (it would itself become an uncaught error).
 */
export function writeFrontendLog(level: FrontendLogLevel, message: string): void {
  invoke("write_frontend_log", { level, message }).catch(() => {
    /* Swallow — logging must never throw. */
  });
}

// ============ MCP Server Management ============
//
// Configure and manage the myshell-mcp server for AI tools
// (Claude Desktop / Opencode / Zcode).

/** Detected AI tool with its config path and whether myshell is already configured. */
export interface AiToolInfo {
  /** "claude" | "opencode" | "zcode" */
  id: string;
  name: string;
  /** Absolute path to the config file */
  configPath: string;
  /** true if the tool is installed (config file exists) */
  installed: boolean;
  /** true if myshell MCP server is already configured */
  configured: boolean;
}

/** Get the absolute path to myshell-mcp.exe. */
export async function getMcpBinaryPath(): Promise<string> {
  return await invoke("mcp_get_binary_path");
}

/** Detect installed AI tools and whether they have myshell configured. */
export async function mcpDetectTools(): Promise<AiToolInfo[]> {
  return await invoke("mcp_detect_tools");
}

/** Write myshell MCP config to a specific tool. Returns true if written, false if already configured. */
export async function mcpWriteConfig(toolId: string): Promise<boolean> {
  return await invoke("mcp_write_config", { toolId });
}

/** Remove myshell from a tool's MCP config. */
export async function mcpRemoveConfig(toolId: string): Promise<void> {
  await invoke("mcp_remove_config", { toolId });
}
