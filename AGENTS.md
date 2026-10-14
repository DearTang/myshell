# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Project

MyShell is a Tauri v2 desktop SSH/SFTP client inspired by FinalShell. Rust backend handles SSH/SFTP/SQLite; Vue 3 + myui frontend renders the UI and terminal via xterm.js.

## Commands

Frontend (run from repo root):
- `npm run dev` — Vite dev server on fixed port 1420 (Tauri convention; `strictPort: true`, do not change)
- `npm run build` — `tsc && vite build` (type-check + bundle to `dist/`)
- `npm run test:ts` — type-check only (`vue-tsc --noEmit`)
- `npm run preview` — preview the production frontend bundle

Full app (run from `src-tauri/`):
- `cargo tauri dev` — launch the desktop app with HMR (calls `npm run dev` via `beforeDevCommand`)
- `cargo tauri build` — produce a distributable bundle (calls `npm run build` first)
- `cargo build` — compile all Rust targets (GUI + CLI + MCP)
- `cargo build --bin myshell` — compile only the GUI binary
- `cargo build --bin myshell-cli` — compile only the CLI binary
- `cargo build --bin myshell-mcp` — compile only the MCP server binary
- `cargo check` — fast type/error check without codegen

Building a release installer:
- `scripts/build-release.bat` (Windows) or `scripts/build-release.sh` (Linux) — builds MCP + CLI binaries first, then runs `cargo tauri build`. The NSIS installer bundles `myshell-mcp.exe` alongside `myshell.exe` (configured in `nsis/installer.nsi`).

`TAURI_DEV_HOST` env var forwards Vite HMR over the LAN (used for mobile/dev on another machine).

## Architecture

### Process boundary & IPC contract

Rust and TS are bridged exclusively through Tauri commands. The wire contract:

```
Vue component → src/api.ts (typed wrapper) → invoke("snake_case_name", { camelCaseArgs })
                                                       ↓
src-tauri/src/main.rs #[tauri::command] fn snake_case_name(...) → myshell_core (ssh.rs / sftp.rs / db.rs)
```

When adding a command, update **three** places in lockstep:
1. `main.rs` — `#[tauri::command]` fn + registration in `generate_handler!`
2. The relevant module in `myshell_core` (`ssh.rs` / `sftp.rs` / `db.rs`) for the actual logic
3. `src/api.ts` — typed wrapper so the frontend gets types

Current command surface (see `generate_handler!` in `main.rs` for the full ~120-command list): `get_connections`, `save_connection`, `delete_connection`, `ssh_connect`, `ssh_send`, `ssh_resize`, `ssh_disconnect`, `sftp_list_dir`, `sftp_mkdir`, `sftp_remove`, `sftp_rename`, `sftp_upload`, `sftp_download`, `sftp_cancel_transfer`, plus vault / folder / quick-command / command-history / FTP / zmodem (`rz_*`/`sz_*`) / AI-assistant / MCP-config commands and the local-terminal set `local_connect` / `local_send` / `local_resize` / `local_disconnect`.

Shared types (`ConnectionConfig`, `FileEntry`) are defined in `src-tauri/src/lib.rs` (serde-derived structs, `myshell_core` crate) and mirrored in `src/api.ts` (TS interfaces). Update both in lockstep.

### Three-binary architecture

The project has **three binary targets** sharing the same `myshell_core` library:

| Binary | Cargo target | Path | Purpose |
|--------|-------------|------|---------|
| `myshell` | `[[bin]]` | `src/main.rs` | Tauri GUI app (the desktop SSH/SFTP client) |
| `myshell-cli` | `[[bin]]` | `src/bin/myshell-cli.rs` | CLI tool for AI agents and power users |
| `myshell-mcp` | `[[bin]]` | `src/bin/myshell-mcp.rs` | MCP server for AI agent integration (stdio transport) |

All three share the same database (`<config_dir>/myshell/connections.db`), vault, and keyring.

### CLI binary (`myshell-cli`)

Provides command-line access to saved SSH/SFTP connections:
- `myshell-cli list [--json]` — list connections
- `myshell-cli exec <连接名> "命令" [--json] [--timeout N]` — one-shot remote command execution
- `myshell-cli sftp ls/get/put/mkdir/rm/rename` — SFTP file operations
- `myshell-cli ssh <连接名>` — interactive terminal session
- `myshell-cli test <连接名>` — test connection reachability
- `myshell-cli vault status` — vault status check

Vault unlock priority: `--passphrase` flag > `MYSHELL_PASSPHRASE` env var > interactive prompt. The MCP server does NOT use any of these — it never holds the passphrase (see below).

### MCP server (`myshell-mcp`)

Exposes SSH/SFTP operations as MCP tools for AI agents (Claude Desktop, Cursor, ZCode, etc.):
- Tools (21): `vault_status`, `list_connections`, `ssh_exec`, `ssh_run`, `ssh_status`, `ssh_cancel`, `ssh_read_text`, `sftp_list`, `sftp_download`, `sftp_upload`, `sftp_mkdir`, `sftp_remove`, `sftp_rename`, `upload_project`, `download_project`, `test_connection`, `screenshot_terminal`, `open_in_gui`, `zmodem_download`, `zmodem_upload`, `zmodem_status`
- Transport: stdio, newline-delimited JSON-RPC 2.0 (MCP 2025-06-18 spec). Read side also accepts legacy LSP-style `Content-Length` framing for old clients (Claude Desktop et al).
- Auth: NONE — the MCP server stores no passphrase and holds no DEK. All credential access is delegated to the GUI over a localhost IPC bridge (port + per-session auth token discovered via `<config_dir>/myshell/gui-ipc-port`; every command must carry the token or the GUI rejects it before dispatch — see Known incomplete spots).

**Argument-encoding norm (阶段 186).** The `initialize` `instructions` carry a NORMATIVE rule (not a hint): commands containing any of `"` `'` `\` `$` backtick `{` `}` `|` `;` or a newline MUST go through `command_b64`; plain `command` stays supported for trivial cases and is explicitly NOT deprecated. `ssh_exec` / `ssh_run` schemas express the choice as `required: ["connection"]` + `anyOf: [command, command_b64]` — a mismatch here (required `command` while the server also accepts the b64 form) silently rejects valid calls in clients that validate schemas strictly; `tests::command_tools_accept_either_command_form` locks the two together. Human-facing spec, with the client-integration checklist, lives in `docs/mcp-integration-spec.md`. Malformed-JSON errors are raised by the CLIENT before the request arrives — the server cannot diagnose them, and the instructions say so explicitly.

**Caller-facing contract (阶段 185).** `tools/call` runs on its own tokio task — stdin is read inline, so a slow command no longer blocks `ssh_status` / `ssh_cancel` or a second connection's call (responses are serialized through one `Mutex<BufWriter<Stdout>>`; on stdin EOF the process waits ≤30s for in-flight calls to flush). Two arguments reduce caller-side failures: `command_b64` (base64 shell command, accepted by `ssh_exec` / `ssh_run`, removes every character that needs JSON escaping) and `ssh_read_text` (escape-free file read: SFTP path, shell `cat` fallback with `shell_quote`). `ssh_exec` results carry `exit_code`, `stdout` (ANSI/OSC stripped), `duration_ms`, `timed_out`, `truncated`, `command`, `exit_code_source`; every tool failure carries `error_kind` (`permission_denied` / `user_denied` / `timeout` / `busy` / `connection_lost` / `connect_failed` / `vault_locked` / `bad_request` / `not_found` / `internal`) plus a `hint` — classify at the boundary via `classify_error`, don't re-derive it. Deliberately NOT added: a `confirm:false`-style caller-declared bypass and a `readonly_hint` parameter (both would let the AI self-authorize past the confirmation gate; `free` / `ss` / `journalctl` are already in the default whitelist).

**Vault gate (fail-fast).** Every tool except `ssh_status` / `ssh_cancel` / `zmodem_status` / `vault_status` requires the GUI's vault to be unlocked — including `list_connections`. The gate (`ensure_vault_ready` in `myshell-mcp.rs`):
1. Auto-launches `myshell.exe` when the GUI isn't running. A stale `gui-ipc-port` file (left by a crashed GUI) is detected by probing the port: connection refused → delete the file and relaunch; connected-but-unresponsive → error only (never relaunch — would trigger the single-instance "覆盖启动" flow and could kill a hung-but-alive instance).
2. Queries the vault state ONCE. Locked → sends a `focus_unlock` IPC action (brings the GUI's password gate to the front), then returns an immediate actionable error. No silent waiting — the old 30s poll was removed.

Auto-configuration: The GUI settings panel (MCP → "一键配置全部") auto-detects installed AI tools and writes configs. Manually configure other tools:

- Claude Desktop: `<USERPROFILE>\.claude\mcp.json` → `mcpServers.myshell.command`
- Opencode: `<USERPROFILE>\.config\opencode\opencode.json` → `mcp.myshell.command`
- Zcode: `<USERPROFILE>\.zcode\cli\config.json` → `mcp.servers.myshell.command`
- Cursor: `<USERPROFILE>\.cursor\mcp.json` → `mcpServers.myshell.command`

**Safety: dangerous operation confirmation.** `ssh_exec` / `ssh_run` use a configurable whitelist/blacklist (regex rules in `<config_dir>/myshell/mcp-command-rules.json`, editable in the GUI: 设置 → MCP 支持 → 命令确认规则): read-only commands (ps, ls, cat, ...) run without a dialog; dangerous ones (rm, kill, sudo, shutdown, write-redirects, ...) must be confirmed by a human. When `show_in_gui` is enabled (default) the confirmation is a React/Vue dialog in the GUI tab; headless fallback pops a native Windows `MessageBoxW`. The file tools `sftp_upload`, `sftp_remove`, `sftp_rename`, `upload_project`, `download_project`, and `zmodem_upload` ALWAYS confirm regardless of rules. "Cancel" returns a HARD STOP error (`denied_by_user_text` in `myshell-mcp.rs`; the GUI-path copy lives in `App.vue` — keep the two in sync): the AI must immediately stop ALL further operations (no retry, no alternative tool/path/connection workaround, no next task step), output a summary of the current task for the user, and wait for the user to explicitly decide whether to continue. The AI agent cannot bypass the dialog; async denials (ssh_run / zmodem_upload) surface the same text via `ssh_status` / `zmodem_status`.

**登录门禁契约（阶段 187）**：门禁失败**绝不隐式等待**——客户端普遍 30s 砍请求，MCP 端死等必然双重失败。改为：
- 失败返回结构化结果：`error_kind: "vault_locked"` + `vault_state`（`locked` / `not_initialized` / `gui_unavailable` / `ipc_unresponsive`）+ hint（重试**同一个**工具，不要改用其他工具）。所有变体的 `error_kind` 恒为 `vault_locked`（调用方修法一致），`vault_state` 只做细分。
- AI 用 `vault_status`（豁免门禁，只返三个布尔，不泄漏任何连接元数据）每 5 秒轮询一次，约 2 分钟上限。门禁拦住 `vault_status` 会自相矛盾（锁定时永远查不到状态），所以它必须在豁免名单里。
- 门禁状态由 `enum VaultGateError::state()` 导出，**不能**从中文错误文本匹配。

**确认门的安全约束（阶段 187，勿回退）**：
- GUI 可达性由 `GuiLaunchError::gui_reachable()` 判定，**绝不**用 `e.message.contains("未找到 myshell.exe")` 之类的字符串匹配 —— 那会让错误措辞成为安全策略的一部分，改文案即可能让高危命令静默执行。回归测试 `gui_reachability_comes_from_type_not_wording` 锁住这点。
- `IpcUnresponsive` → `gui_reachable = true`：对话框没显示，但 GUI 进程在且端口在绑，**不得**再启动第二个（会触发单实例「覆盖启动」并可能杀掉挂起但仍开着的实例）。
- 不可逆文件操作的确认（六个文件工具）在 GUI 内渲染，走 `mcp_confirm` IPC action；**180s 超时按 deny 处理** —— 不可逆操作里「用户没说好」必须读作「不」，读作「是」则等待超时反而变成放行。GUI 完全不可达时才回退原生 `MessageBoxW`。
- 不要加 `confirm:false` / `readonly_hint` 这类让 AI 自我授权的旁路（与确认门设计前提冲突）。

### Rust backend (`src-tauri/src/`)

- `lib.rs` — `myshell_core` library crate root. Holds shared types (`ConnectionConfig`, `FileEntry`, `AppState`), `EventSink` trait (abstracts Tauri's `WebviewWindow::emit` for CLI/MCP), and `pub mod` declarations for all 18 modules. This is the single source of truth for types used by GUI, CLI, and MCP binaries.
- `main.rs` — Tauri GUI binary: thin `#[tauri::command]` wrappers calling into `myshell_core`, `WindowSink` adapter for `EventSink`, Tauri builder.
- `bin/myshell-cli.rs` — CLI binary (clap-based)
- `bin/myshell-mcp.rs` — MCP server binary (stdio JSON-RPC 2.0)
- `ssh.rs` — russh client: connect, authenticate (password/pubkey), open PTY, send input, resize, disconnect
- `sftp.rs` — russh-sftp: list/create/remove/rename. **Each SFTP call opens a fresh subsystem channel** (`get_sftp_session`) — there's no cached `SftpSession`.
- `db.rs` — rusqlite (bundled) CRUD. DB lives at `<config_dir>/myshell/connections.db` (via the `dirs` crate).
- `local.rs` — local terminal (`conn_type='local'`): spawns a shell under a PTY (`portable-pty`: ConPTY on Windows / openpty on Unix) and emits `ssh_output`/`ssh_closed` so `TerminalPanel` reuses the SSH render path. Reader runs on a `spawn_blocking` thread (portable-pty's reader is a blocking `Read`); a writer task owns the master for input/resize and kills the child on disconnect.

`AppState` is registered via `.manage()` and accessed through `State<AppState>`. It holds:
- `db: Arc<Mutex<rusqlite::Connection>>` — single shared SQLite handle
- `ssh_sessions: Arc<Mutex<HashMap<String, SshSession>>>` — keyed by UUID session ID
- `ftp_sessions: Arc<Mutex<HashMap<String, FtpSession>>>` — FTP sessions
- `local_sessions: Arc<Mutex<HashMap<String, LocalSession>>>` — local PTY sessions
- `zmodem_files: Mutex<HashMap<String, ZmodemFileHandle>>` — ZMODEM file handles
- `dek: Arc<Mutex<Option<[u8; 32]>>>` — Data Encryption Key (None until vault unlocked)

### Frontend (`src/`)

Vue 3 + myui 统一界面框架（v2.15.0 从 React 迁移；React 旧实现留在 `src-legacy/` 备查）。
组件是 `.vue` 单文件组件，状态用 Pinia store，**不是** class 组件 / hooks。

- `App.vue` — 应用外壳：顶栏 / 连接树 / 工作区 / AI 右栏 / 状态栏五区布局
- `store/connections.ts` · `store/sessions.ts` · `store/ui.ts` — Pinia store，分别持有连接列表、会话与标签页生命周期、界面偏好
- `components/AppSidebar.vue` — 连接树，分组 + 右键菜单 + 拖拽排序
- `components/TerminalPanel.vue` — xterm.js + FitAddon + WebLinksAddon，配色由 appearance store 驱动
- `components/SftpPanel.vue` — 文件浏览器
- `components/ConnectionDialog.vue` — 新建/编辑连接表单
- `components/settings/*.vue` — 设置抽屉各分区（外观 / 数据 / 安全 / AI / MCP / 命令规则）
- `composables/*.ts` — 跨组件复用的组合式函数（`useVault` / `useMcpBridge` / `useConnectionDrag` …）
- `styles/app.css` — 样式分层：Element Plus 基础 → EP 暗色变量 → myui 令牌 → 壳层覆盖

> **类型检查用 `npm run test:ts`（即 `vue-tsc --noEmit`），不要用 `npx tsc`。**
> 裸 `tsc` 不认识 `.vue` 单文件组件，会在 `src/main.ts` 报
> `TS2307: Cannot find module './App.vue'` —— 那是工具选错，不是代码坏了。

### Known incomplete spots

- ~~GUI IPC bridge has no authentication~~ **Fixed in 阶段 109 (token handshake)**: the `gui-ipc-port` file now carries `"<port>\n<token>"` (per-session random 32-byte token, base64). Every IPC command must include the token; the GUI validates it constant-time before dispatching any action and never logs it. This blocks **other local users' processes** (localhost TCP is machine-reachable, the port file is user-profile-ACL'd). Residual exposure: a process running as the **same user** can still read the port file and authenticate — inherent to the MCP architecture (the MCP server is itself such a process); a full fix needs named pipes with ACLs. See `progress.md` 阶段 106/109. Backward compat: an old-format port file (port only) is accepted for read; requests then omit the token, which only an old GUI honors — a version-skewed old MCP gets a clear "unauthorized" error telling the user to update both binaries together.
- Known risk: `rsa` crate Marvin-attack advisory (RUSTSEC-2023-0071, timing sidechannel on RSA private-key ops) — upstream has no fixed release; accepted (client-side exposure, mostly host-key verification uses public ops). Re-check when rsa > 0.10 ships stable.
- Known risk: `channel.wait()` in `select!` may not be cancel-safe — if bytes are observed being dropped under high traffic, switch to `channel.make_reader()` + `tokio::io::split()`.
- **Chinese IME input shifts the terminal viewport left — ConPTY upstream bug, not fixable at the app layer.** Typing Chinese via an IME in a line-redrawing program (PowerShell PSReadLine, Codex/ink) makes ConPTY miscalculate the composition string's width and emit a wrong cursor-position sequence, so xterm.js shifts the viewport left during composition; it snaps back when the IME is confirmed (space/enter). ASCII input is unaffected. Matches [VSCode #255285](https://github.com/microsoft/vscode/issues/255285) verbatim. VSCode's workaround is the winpty backend, but **`portable-pty` 0.8.1 has dropped winpty** (`src/win/` ships `conpty.rs` only), so we can't switch backends without replacing the PTY crate — and winpty is UTF-8-hostile (would trade this left-shift for Chinese mojibake). Accepted as a known limitation; waiting on Microsoft to fix ConPTY. See `progress.md` 阶段23. User workaround: confirm the IME often (the shift recovers instantly) and avoid very long single compositions.

## Living planning docs (Chinese)

`findings.md`, `progress.md`, and `task_plan.md` at the repo root are the project's phase tracker and decision log. They are written and updated in Chinese. Per the docs' own instructions, update `progress.md` after each phase or error, and re-read `task_plan.md` before major decisions.

**Doc-after-feature (standing rule):** every time a feature/fix/optimization is finished, update the docs **directly, without asking** — this is a built-in final step of "change done", same as running `npm run test:ts`:
1. append a `### 阶段 N` entry (+ 五问重启检查) to `progress.md`;
2. keep `README.md` in sync (功能特性 sections);
3. **append one line to `RELEASE_NOTES_STAGING.md`** under "待发布条目" — format `- <emoji> <one-sentence desc>` (✨新增 / 🛠️优化 / 🐛修复 / 🔒安全). This staging file is the primary source for the next release's changelog (see the `打包` rule) and is cleared after each release. Pure discussion / Q&A with no code change → don't append.

## Slash rule: `打包` / `帮我打包` (durable release authorization)

When the user sends **`打包`** or **`帮我打包`** (or any message whose core intent is "cut a release"), execute the release pipeline autonomously **except for one mandatory confirmation gate after the changelog is written** (see step 3.5). The user has durably pre-authorized version bumping, building, git commit/push, and Gitee publishing by issuing the command — but they want to review and approve the **version number + update content** before anything is built or pushed. Announce each step as you start it (so it's transparent), then proceed — and STOP at the gate.

### Prerequisite (one-time, done by the user)
Tokens for **both** platforms must be available:

**Gitee** — personal access token with `projects`/release scope:
- env var `$GITEE_TOKEN`, or
- a `.gitee-token` file at repo root (gitignored — **never commit it, never paste the value into chat/commit messages**).

**GitHub** — fine-grained PAT with Contents (read/write) on `DearTang/myshell`:
- env var `$GITHUB_TOKEN`, or
- a `.github-token` file at repo root (gitignored).

If either is missing when you reach the publish step, STOP at that step and tell the user how to create the token and where to put it — do NOT proceed to a broken publish and do NOT invent a token.

Git remotes: `origin` = Gitee (`gitee.com/argustang/myshell`), `github` = GitHub (`github.com/DearTang/myshell`).

### Pipeline (run in order)
1. **Decide the version bump** (auto). Primary signal = the entry **types** in `RELEASE_NOTES_STAGING.md` (any ✨新增 → **minor**; only 🐛/🛠️/🔒 → **patch**). Read the current `src-tauri/Cargo.toml` version. State the chosen version + one-line rationale before applying.
2. **Bump the version** — edit `src-tauri/Cargo.toml` `version = "..."` ONLY (single source of truth), then run `npm run version:sync`.
3. **Generate the release notes** — **`RELEASE_NOTES_STAGING.md` is the PRIMARY source**: turn its "待发布条目" into the new `## vX.Y.Z（YYYY-MM-DD）` CHANGELOG section (group by ✨新增 / 🛠️优化 / 🐛修复 / 🔒安全). Then run a **completeness check**: `git diff --stat <baseline>..HEAD` (baseline = the `baseline:` line in the staging file) — if changed files aren't covered by any staging entry, surface them and add entries (catches un-logged / out-of-session work). Keep CHANGELOG's existing header comment intact. Mirror into `README.md` 更新日志. Write the new section to the temp notes file used by publish.
3.5. **⚠️ CONFIRMATION GATE — STOP here.** Present: (a) chosen version + rationale, (b) full text of the new CHANGELOG section. **Do NOT build/commit/push/publish until the user confirms.** If they edit, revise and re-present. On confirm, steps 4–9 run autonomously.
4. **Pre-check** — `npm run test:ts` (vue-tsc) and `cargo check` (in `src-tauri/`). Both must pass.
5. **Build the installer** — `npm run tauri:build` from repo root. Long (~10+ min first time); background it. Output: `src-tauri/target/release/bundle/nsis/MyShell_X.Y.Z_x64-setup.exe`.
6. **Commit + push (both remotes)** — stage `Cargo.toml`, `package.json`, `package-lock.json`, `CHANGELOG.md`, `README.md`, `progress.md`, `RELEASE_NOTES_STAGING.md` (and feature code), commit `release: vX.Y.Z`, then push to **both** `origin` (Gitee) and `github`. Don't commit `.gitee-token` / `.github-token`. The host classifier may gate push-to-default-branch → ask the user to authorize or run `! git push origin main && git push github main`.
7. **Publish releases (both platforms)** — run both scripts (sequentially, either order):
   - `node scripts/publish-gitee-release.mjs <version> <temp-notes-file>`
   - `node scripts/publish-github-release.mjs <version> <temp-notes-file>`
   Report both URLs. If one platform fails, still attempt the other; report the failure.
8. **Clear the staging buffer** — in `RELEASE_NOTES_STAGING.md`: empty the "待发布条目" section (leave the header/baseline structure) and update `baseline: v<X.Y.Z>` to the just-released version. Commit + push this cleanup to **both** remotes (`chore: clear release staging after vX.Y.Z`).
9. **Doc-after-feature** — ensure `progress.md` has the stage entry and `README.md` 更新日志 is in sync.

### Notes / safety
- If a release for the tag already exists on either platform, the publish step will error on create — that's fine, report it; don't delete-and-recreate silently.
- This rule authorizes git commit + push + public release **only** as part of this explicit `打包` flow. It does not extend to other tasks.
- Never read, log, or print the token value; the scripts read them directly.

