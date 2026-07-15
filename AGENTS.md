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
- `cargo build` — compile Rust backend only
- `cargo check` — fast type/error check without codegen

`TAURI_DEV_HOST` env var forwards Vite HMR over the LAN (used for mobile/dev on another machine).

## Architecture

### Process boundary & IPC contract

Rust and TS are bridged exclusively through Tauri commands. The wire contract:

```
React component → src/api.ts (typed wrapper) → invoke("snake_case_name", { camelCaseArgs })
                                                       ↓
src-tauri/src/main.rs #[tauri::command] fn snake_case_name(...) → ssh.rs / sftp.rs / db.rs
```

When adding a command, update **three** places in lockstep:
1. `main.rs` — `#[tauri::command]` fn + registration in `generate_handler!`
2. The relevant module (`ssh.rs` / `sftp.rs` / `db.rs`) for the actual logic
3. `src/api.ts` — typed wrapper so the frontend gets types

Current command surface (see `generate_handler!` in `main.rs` for the full, ever-growing list): `get_connections`, `save_connection`, `delete_connection`, `ssh_connect`, `ssh_send`, `ssh_resize`, `ssh_disconnect`, `sftp_list_dir`, `sftp_mkdir`, `sftp_remove`, `sftp_rename`, plus vault / folder / quick-command / FTP commands and the local-terminal set `local_connect` / `local_send` / `local_resize` / `local_disconnect`.

Shared types (`ConnectionConfig`, `FileEntry`) are defined twice and must stay in sync: once in `src-tauri/src/main.rs` (serde-derived structs) and once in `src/api.ts` (TS interfaces).

### Rust backend (`src-tauri/src/`)

- `main.rs` — types, `AppState`, command wrappers, Tauri builder
- `ssh.rs` — russh client: connect, authenticate (password/pubkey), open PTY, send input, resize, disconnect
- `sftp.rs` — russh-sftp: list/create/remove/rename. **Each SFTP call opens a fresh subsystem channel** (`get_sftp_session`) — there's no cached `SftpSession`.
- `db.rs` — rusqlite (bundled) CRUD. DB lives at `<config_dir>/myshell/connections.db` (via the `dirs` crate).
- `local.rs` — local terminal (`conn_type='local'`): spawns a shell under a PTY (`portable-pty`: ConPTY on Windows / openpty on Unix) and emits `ssh_output`/`ssh_closed` so `TerminalPanel` reuses the SSH render path. Reader runs on a `spawn_blocking` thread (portable-pty's reader is a blocking `Read`); a writer task owns the master for input/resize and kills the child on disconnect.

`AppState` is registered via `.manage()` and accessed through `State<AppState>`. It holds:
- `db: Mutex<rusqlite::Connection>` — single shared SQLite handle
- `ssh_sessions: Mutex<HashMap<String, SshSession>>` — keyed by UUID session ID

The frontend uses the session UUID as the tab ID, so `tab.id === sessionId` is an invariant.

### Frontend (`src/`)

- `App.tsx` — top-level state: connections list, open tabs, active tab, SFTP panel visibility. Single source of truth for tab lifecycle.
- `components/Sidebar.tsx` — connection manager with grouping + right-click context menu
- `components/TabBar.tsx` — terminal tabs + SFP toggle
- `components/TerminalPanel.tsx` — xterm.js + FitAddon + WebLinksAddon, Catppuccin Mocha colors hardcoded
- `components/SftpPanel.tsx` — file browser with in-memory history stack
- `components/ConnectionDialog.tsx` — create/edit connection form
- `styles/global.css` — Catppuccin Mocha palette exposed as CSS custom properties (`--bg-dark`, `--accent`, etc.). Components use **inline styles + these vars** — no CSS-in-JS, no Tailwind.

### Known incomplete spots

- **SSH output wiring was rewritten but not yet compiled.** The original `ssh.rs` had a placeholder reader task that never ran — keystrokes flowed TO the server but output never came BACK. This has been refactored: the russh `Channel` is now owned by a `channel_reader` task that uses `tokio::select!` to multiplex commands (`SessionCommand::Input/Resize/Disconnect` via mpsc) with incoming `channel.wait()` messages, flushing bytes to the frontend via `window.emit("ssh_output", ...)` on a 16ms coalescing timer (mitigates [tauri#13234](https://github.com/tauri-apps/tauri/issues/13234)). Emits are scoped to the originating `WebviewWindow` (not broadcast via `AppHandle`) so a future second window can't read another tab's terminal. Output buffer is capped at 256KB with a `[output truncated]` marker to prevent frontend OOM. Frontend subscribes in `TerminalPanel.tsx` via `onSshOutput`/`onSshClosed`. Port input is validated to `[1, 65535]` in `ConnectionDialog.tsx`. `npx tsc --noEmit` passes; **the Rust side has not yet been compiled because rustup isn't installed**. Run `cargo build` from `src-tauri/` after installing Rust to verify.
- `check_server_key` in `ssh.rs` returns `Ok(true)` unconditionally — accepts all host keys. Intentional placeholder; revisit before any release.
- Passwords are stored plaintext in SQLite.
- App/window close does not disconnect active sessions — SSH TCP connections leak until OS reaps them. Add a `RunEvent::Exit` handler in `main.rs` to drain `ssh_sessions`.
- `load_secret_key` accepts arbitrary path (no canonicalization/allow-list) — file existence oracle.
- Known risk: `channel.wait()` in `select!` may not be cancel-safe — if bytes are observed being dropped under high traffic, switch to `channel.make_reader()` + `tokio::io::split()`.
- **Local terminal (`conn_type='local'`) code is written but not yet compiled** — rustup isn't installed in the editing environment, so `local.rs` + the 4 `local_*` commands are unverified. Run `cargo build` from `src-tauri/` after installing Rust; the most likely fix point is `portable-pty` 0.8 trait bounds on `take_writer`/`spawn_command`. Also: on Windows the PTY output follows the shell's console codepage — pwsh is UTF-8, but Windows PowerShell 5.1 on a zh-CN system may emit GBK and render mojibake in xterm. v1 emits raw bytes (no re-encode, matching SSH); mitigation TBD (`encoding_rs` decode in the reader, or inject `chcp 65001`). See `progress.md` 阶段10.
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
A Gitee personal access token with the `projects`/release scope must be available, either as:
- env var `$GITEE_TOKEN`, or
- a `.gitee-token` file at repo root (gitignored — **never commit it, never paste the value into chat/commit messages**).

If neither is present when you reach the publish step, STOP at that step and tell the user how to create the token (Gitee → 设置 → 私人令牌 → 生成新令牌) and where to put it — do NOT proceed to a broken publish and do NOT invent a token.

### Pipeline (run in order)
1. **Decide the version bump** (auto). Primary signal = the entry **types** in `RELEASE_NOTES_STAGING.md` (any ✨新增 → **minor**; only 🐛/🛠️/🔒 → **patch**). Read the current `src-tauri/Cargo.toml` version. State the chosen version + one-line rationale before applying.
2. **Bump the version** — edit `src-tauri/Cargo.toml` `version = "..."` ONLY (single source of truth), then run `npm run version:sync`.
3. **Generate the release notes** — **`RELEASE_NOTES_STAGING.md` is the PRIMARY source**: turn its "待发布条目" into the new `## vX.Y.Z（YYYY-MM-DD）` CHANGELOG section (group by ✨新增 / 🛠️优化 / 🐛修复 / 🔒安全). Then run a **completeness check**: `git diff --stat <baseline>..HEAD` (baseline = the `baseline:` line in the staging file) — if changed files aren't covered by any staging entry, surface them and add entries (catches un-logged / out-of-session work). Keep CHANGELOG's existing header comment intact. Mirror into `README.md` 更新日志. Write the new section to the temp notes file used by publish.
3.5. **⚠️ CONFIRMATION GATE — STOP here.** Present: (a) chosen version + rationale, (b) full text of the new CHANGELOG section. **Do NOT build/commit/push/publish until the user confirms.** If they edit, revise and re-present. On confirm, steps 4–9 run autonomously.
4. **Pre-check** — `npx tsc --noEmit` and `cargo check` (in `src-tauri/`). Both must pass.
5. **Build the installer** — `npm run tauri:build` from repo root. Long (~10+ min first time); background it. Output: `src-tauri/target/release/bundle/nsis/MyShell_X.Y.Z_x64-setup.exe`.
6. **Commit + push** — stage `Cargo.toml`, `package.json`, `package-lock.json`, `CHANGELOG.md`, `README.md`, `progress.md`, `RELEASE_NOTES_STAGING.md` (and feature code), commit `release: vX.Y.Z`, push to upstream. (The Gitee release tags `main` HEAD, so the bump must be on the remote first.) Don't commit `.gitee-token`. The host classifier may gate push-to-default-branch → ask the user to authorize or run `! git push origin main`.
7. **Publish to Gitee** — `node scripts/publish-gitee-release.mjs <version> <temp-notes-file>` (creates release + uploads `.exe`; built-in retry). Report the URL. Host classifier may gate the public release → hand the user the command to run via `!`.
8. **Clear the staging buffer** — in `RELEASE_NOTES_STAGING.md`: empty the "待发布条目" section (leave the header/baseline structure) and update `baseline: v<X.Y.Z>` to the just-released version. Commit + push this cleanup (`chore: clear release staging after vX.Y.Z`).
9. **Doc-after-feature** — ensure `progress.md` has the stage entry and `README.md` 更新日志 is in sync.

### Notes / safety
- If a Gitee release for the tag already exists, the publish step will error on create — that's fine, report it; don't delete-and-recreate silently.
- This rule authorizes git commit + push + public release **only** as part of this explicit `打包` flow. It does not extend to other tasks.
- Never read, log, or print the token value; the script reads it directly.

