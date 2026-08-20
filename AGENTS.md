# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Project

MyShell is a Tauri v2 desktop SSH/SFTP client inspired by FinalShell. Rust backend handles SSH/SFTP/SQLite; React + TypeScript frontend renders the UI and terminal via xterm.js.

## Commands

Frontend (run from repo root):
- `npm run dev` — Vite dev server on fixed port 1420 (Tauri convention; `strictPort: true`, do not change)
- `npm run build` — `tsc && vite build` (type-check + bundle to `dist/`)
- `npx tsc --noEmit` — type-check only
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
React component → src/api.ts (typed wrapper) → invoke("snake_case_name", { camelCaseArgs })
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
- Tools (18): `list_connections`, `ssh_exec`, `ssh_run`, `ssh_status`, `ssh_cancel`, `sftp_list`, `sftp_download`, `sftp_upload`, `sftp_mkdir`, `sftp_remove`, `sftp_rename`, `upload_project`, `download_project`, `test_connection`, `screenshot_terminal`, `open_in_gui`, `zmodem_download`, `zmodem_upload`
- Transport: stdio, newline-delimited JSON-RPC 2.0 (MCP 2025-06-18 spec). Read side also accepts legacy LSP-style `Content-Length` framing for old clients (Claude Desktop et al).
- Auth: NONE — the MCP server stores no passphrase and holds no DEK. All credential access is delegated to the GUI over a localhost IPC bridge (port discovered via `<config_dir>/myshell/gui-ipc-port`).

**Vault gate (fail-fast).** Every tool except `ssh_status` / `ssh_cancel` / `zmodem_status` requires the GUI's vault to be unlocked — including `list_connections`. The gate (`ensure_vault_ready` in `myshell-mcp.rs`):
1. Auto-launches `myshell.exe` when the GUI isn't running. A stale `gui-ipc-port` file (left by a crashed GUI) is detected by probing the port: connection refused → delete the file and relaunch; connected-but-unresponsive → error only (never relaunch — would trigger the single-instance "覆盖启动" flow and could kill a hung-but-alive instance).
2. Queries the vault state ONCE. Locked → sends a `focus_unlock` IPC action (brings the GUI's password gate to the front), then returns an immediate actionable error. No silent waiting — the old 30s poll was removed.

Auto-configuration: The GUI settings panel (MCP → "一键配置全部") auto-detects installed AI tools and writes configs. Manually configure other tools:

- Claude Desktop: `<USERPROFILE>\.claude\mcp.json` → `mcpServers.myshell.command`
- Opencode: `<USERPROFILE>\.config\opencode\opencode.json` → `mcp.myshell.command`
- Zcode: `<USERPROFILE>\.zcode\cli\config.json` → `mcp.servers.myshell.command`
- Cursor: `<USERPROFILE>\.cursor\mcp.json` → `mcpServers.myshell.command`

**Safety: dangerous operation confirmation.** `ssh_exec` / `ssh_run` use a configurable whitelist/blacklist (regex rules in `<config_dir>/myshell/mcp-command-rules.json`, editable in the GUI: 设置 → MCP 支持 → 命令确认规则): read-only commands (ps, ls, cat, ...) run without a dialog; dangerous ones (rm, kill, sudo, shutdown, write-redirects, ...) must be confirmed by a human. When `show_in_gui` is enabled (default) the confirmation is a React dialog in the GUI tab; headless fallback pops a native Windows `MessageBoxW`. The file tools `sftp_upload`, `sftp_remove`, `sftp_rename`, `upload_project`, `download_project`, and `zmodem_upload` ALWAYS confirm regardless of rules. "Cancel" returns an error; the AI agent cannot bypass this.

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

- `App.tsx` — top-level state: connections list, open tabs, active tab, SFTP panel visibility. Single source of truth for tab lifecycle.
- `components/Sidebar.tsx` — connection manager with grouping + right-click context menu
- `components/TabBar.tsx` — terminal tabs + SFP toggle
- `components/TerminalPanel.tsx` — xterm.js + FitAddon + WebLinksAddon, Catppuccin Mocha colors hardcoded
- `components/SftpPanel.tsx` — file browser with in-memory history stack
- `components/ConnectionDialog.tsx` — create/edit connection form
- `styles/global.css` — Catppuccin Mocha palette exposed as CSS custom properties (`--bg-dark`, `--accent`, etc.). Components use **inline styles + these vars** — no CSS-in-JS, no Tailwind.

### Known incomplete spots

- GUI IPC bridge (localhost TCP, port discovered via the `gui-ipc-port` file) has **no authentication** — any process running as the same user can call `get_connection_secrets` while the vault is unlocked and receive full decrypted credentials. Inherent to the current MCP architecture (the MCP server is itself such a process); a full fix needs named pipes with ACLs or a token handshake. Recorded in `progress.md` 阶段 106.
- Known risk: `channel.wait()` in `select!` may not be cancel-safe — if bytes are observed being dropped under high traffic, switch to `channel.make_reader()` + `tokio::io::split()`.
- **Chinese IME input shifts the terminal viewport left — ConPTY upstream bug, not fixable at the app layer.** Typing Chinese via an IME in a line-redrawing program (PowerShell PSReadLine, Codex/ink) makes ConPTY miscalculate the composition string's width and emit a wrong cursor-position sequence, so xterm.js shifts the viewport left during composition; it snaps back when the IME is confirmed (space/enter). ASCII input is unaffected. Matches [VSCode #255285](https://github.com/microsoft/vscode/issues/255285) verbatim. VSCode's workaround is the winpty backend, but **`portable-pty` 0.8.1 has dropped winpty** (`src/win/` ships `conpty.rs` only), so we can't switch backends without replacing the PTY crate — and winpty is UTF-8-hostile (would trade this left-shift for Chinese mojibake). Accepted as a known limitation; waiting on Microsoft to fix ConPTY. See `progress.md` 阶段23. User workaround: confirm the IME often (the shift recovers instantly) and avoid very long single compositions.

## Living planning docs (Chinese)

`findings.md`, `progress.md`, and `task_plan.md` at the repo root are the project's phase tracker and decision log. They are written and updated in Chinese. Per the docs' own instructions, update `progress.md` after each phase or error, and re-read `task_plan.md` before major decisions.

**Doc-after-feature (standing rule):** every time a feature/fix/optimization is finished, update the docs **directly, without asking** — this is a built-in final step of "change done", same as running `npx tsc --noEmit`:
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
4. **Pre-check** — `npx tsc --noEmit` and `cargo check` (in `src-tauri/`). Both must pass.
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

