/**
 * Keystroke buffer for command-history recording.
 *
 * xterm's `onData` fires with raw byte strings (including control chars,
 * ANSI escape sequences, and line-editor noise). This module accumulates
 * printable characters and flushes on Enter, stripping ANSI and handling
 * backspace/del/Ctrl-C/Ctrl-D so the recorded command approximates what
 * the user intended to run.
 *
 * IMPORTANT LIMITATIONS:
 * - Tab completion: the accumulated literal may not match the expanded
 *   command the shell actually executed (e.g., `ls --ta[Tab]` records as
 *   `ls --ta` instead of `ls --table`).
 * - Arrow-key history navigation: shell replaces the line, but we only
 *   see the literal characters typed before the arrow key.
 * - Multi-line paste: we record each line as a separate command.
 * - Shell aliases: we record what user types, not the expanded command.
 */

/** Commands that should NOT be recorded to history. */
const SKIP_PATTERNS = [
  /^!+$/,           // !! or !!! (bash history substitution)
  /^!:\d/,          // !:1, !:2 (history word designator)
  /^history/,       // history command itself
  /^exit$/,         // exit (don't pollute history with exit)
  /^clear$/,        // clear
  /^cd\s*$/,        // cd without args
  // Password-bearing commands — record only the command name, never the
  // argument that contains the secret. We refuse to record the whole line
  // because the password prompt form is varied: `-p Secret`, `--password=x`,
  // `url:user:pass`, etc.
  /\bpasswd\b/,                       // passwd (interactive, next inputs are password)
  /^su\b/,                            // su prompts for password
  /\bmysql\s+.*-p[\s=]/,              // mysql -pSECRET or mysql -p SECRET
  /\bpsql\s+.*\bPGPASSWORD=/,         // PGPASSWORD=... psql
  /\bredis-cli\s+.*-a\b/,             // redis-cli -a PASSWORD
  /\bmongo\b.*-p\b/,                  // mongo -p
  /\bcurl\s+.*-u\s+\S+:\S+/,          // curl -u user:pass
  /\bwget\s+.*--(user|password)=/,    // wget creds
  /\bgit\s+(push|pull|clone)\s+https?:\/\/\S+:\S+@/, // git URL with embedded creds
  /\bssh\s+-i\b/,                     // ssh -i (private key path)
  /\bscp\s+-i\b/,                     // scp -i
  /\brsync\s+.*-e\s+["']?ssh/,        // rsync over ssh with custom opts
  // Generic secret-bearing tokens often found inline.
  /\bpassword\s*=\s*\S+/i,
  /\btoken\s*=\s*\S+/i,
  /\bsecret\s*=\s*\S+/i,
  /\bapi[_-]?key\s*=\s*\S+/i,
  /\bauthorization:\s*\S+/i,
];

/** Check if a command should be skipped from history recording. */
function shouldSkip(cmd: string): boolean {
  const trimmed = cmd.trim();
  if (trimmed.length === 0) return true;
  if (trimmed.length > 2000) return true; // Very long commands (pasted scripts)
  for (const pattern of SKIP_PATTERNS) {
    if (pattern.test(trimmed)) return true;
  }
  return false;
}

/**
 * Process a chunk of xterm input data and update the command buffer.
 *
 * @param data Raw string from `term.onData(data => ...)`.
 * @param bufRef Mutable ref holding the accumulated command so far.
 * @param ansiEscRef Mutable ref tracking whether we're inside an ANSI
 *                   escape sequence (stateful across calls).
 * @param flush Called when a complete command is ready (Enter pressed).
 *              Receives the trimmed command text; empty commands are
 *              NOT flushed.
 */
export function recordKeystroke(
  data: string,
  bufRef: { current: string },
  ansiEscRef: { current: boolean },
  flush: (cmd: string) => void
): void {
  // Batch paste detection: if this chunk has 2+ newlines and is long,
  // treat it as a multi-line paste and only record the first non-empty line.
  // This prevents a 200-line script paste from creating 200 history entries.
  const newlineCount = (data.match(/\n/g) || []).length;
  if (newlineCount >= 2 && data.length > 64) {
    const lines = data.split(/\n/);
    const firstLine = lines[0]?.trim();
    if (firstLine && firstLine.length > 0 && !shouldSkip(firstLine)) {
      flush(firstLine);
    }
    bufRef.current = "";
    ansiEscRef.current = false;
    return;
  }

  // Process each character
  for (let i = 0; i < data.length; i++) {
    const ch = data[i];
    const code = ch.charCodeAt(0);

    // Handle ANSI escape sequences
    if (ansiEscRef.current) {
      // CSI sequences (ESC [ ... final_byte)
      // Final byte is 0x40-0x7E (@A-Z[\]^_`a-z{|}~)
      // We specifically only terminate on letters to be safe
      if (code >= 0x40 && code <= 0x7E) {
        ansiEscRef.current = false;
      }
      // Swallow all bytes in the sequence
      continue;
    }

    // ESC starts an ANSI escape sequence
    if (code === 0x1b) {
      ansiEscRef.current = true;
      continue;
    }

    // Backspace (0x08) or Del (0x7f) — delete last char from buffer
    if (code === 0x08 || code === 0x7f) {
      if (bufRef.current.length > 0) {
        bufRef.current = bufRef.current.slice(0, -1);
      }
      continue;
    }

    // Ctrl+C (0x03) or Ctrl+D (0x04) — abort current buffer
    if (code === 0x03 || code === 0x04) {
      bufRef.current = "";
      continue;
    }

    // CR (0x0d) or LF (0x0a) — flush the buffer
    if (code === 0x0d || code === 0x0a) {
      const cmd = bufRef.current.trim();
      bufRef.current = "";
      if (cmd.length > 0 && !shouldSkip(cmd)) {
        flush(cmd);
      }
      continue;
    }

    // Skip other control characters (0x00-0x1F except those handled above)
    if (code < 0x20) {
      continue;
    }

    // Printable ASCII (0x20-0x7E) or high-byte UTF-8 (>= 0x80)
    if ((code >= 0x20 && code <= 0x7E) || code >= 0x80) {
      bufRef.current += ch;
    }
  }
}
