/**
 * SFTP transfer settings (localStorage-backed, read at transfer start).
 *
 * Concurrency is the number of files downloaded in parallel. russh-sftp
 * multiplexes concurrent file handles over a single channel by request id,
 * so this mostly buys throughput on many-small-files transfers (round-trip
 * latency dominated) rather than one big file. The Rust side clamps to
 * 1..=16 regardless of what's stored here.
 */

const STORAGE_KEY_CONCURRENCY = "myshell-sftp-download-concurrency";

export const DEFAULT_SFTP_CONCURRENCY = 3;
export const MAX_SFTP_CONCURRENCY = 16;

function clamp(n: number): number {
  if (!Number.isFinite(n)) return DEFAULT_SFTP_CONCURRENCY;
  return Math.min(MAX_SFTP_CONCURRENCY, Math.max(1, Math.floor(n)));
}

export function getSftpDownloadConcurrency(): number {
  try {
    const v = Number(localStorage.getItem(STORAGE_KEY_CONCURRENCY));
    if (Number.isFinite(v) && v >= 1) return clamp(v);
  } catch {
    // localStorage unavailable — default below.
  }
  return DEFAULT_SFTP_CONCURRENCY;
}

export function setSftpDownloadConcurrency(n: number): void {
  try {
    localStorage.setItem(STORAGE_KEY_CONCURRENCY, String(clamp(n)));
  } catch {
    // localStorage full/unavailable — setting not persisted.
  }
}
