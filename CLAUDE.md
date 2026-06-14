# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

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

Current command surface: `get_connections`, `save_connection`, `delete_connection`, `ssh_connect`, `ssh_send`, `ssh_resize`, `ssh_disconnect`, `sftp_list_dir`, `sftp_mkdir`, `sftp_remove`, `sftp_rename`.

Shared types (`ConnectionConfig`, `FileEntry`) are defined twice and must stay in sync: once in `src-tauri/src/main.rs` (serde-derived structs) and once in `src/api.ts` (TS interfaces).

### Rust backend (`src-tauri/src/`)

- `main.rs` — types, `AppState`, command wrappers, Tauri builder
- `ssh.rs` — russh client: connect, authenticate (password/pubkey), open PTY, send input, resize, disconnect
- `sftp.rs` — russh-sftp: list/create/remove/rename. **Each SFTP call opens a fresh subsystem channel** (`get_sftp_session`) — there's no cached `SftpSession`.
- `db.rs` — rusqlite (bundled) CRUD. DB lives at `<config_dir>/myshell/connections.db` (via the `dirs` crate).

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

## Living planning docs (Chinese)

`findings.md`, `progress.md`, and `task_plan.md` at the repo root are the project's phase tracker and decision log. They are written and updated in Chinese. Per the docs' own instructions, update `progress.md` after each phase or error, and re-read `task_plan.md` before major decisions.
