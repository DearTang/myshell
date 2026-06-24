import { useCallback, useEffect, useRef, useState } from "react";
import { checkForUpdates, type UpdateInfo } from "../api";

/**
 * localStorage keys for update-check pacing.
 *
 * `lastCheck` — unix ms of the last automatic check. The auto path refuses
 *   to run again until at least CHECK_INTERVAL_MS has elapsed, so the Gitee
 *   unauthenticated API isn't hit on every launch (rate-limit + startup
 *   latency). `checkNow` ignores and refreshes this.
 *
 * (Per-version "notify once" state lives in App.tsx as
 * `myshell.lastNotifiedUpdateVersion`; kept out of this hook so the hook
 * stays a pure data-fetch.)
 */
const STORAGE_KEY_LAST_CHECK = "myshell.lastUpdateCheck";
const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000; // 24h

/**
 * Background update check against Gitee's latest release.
 *
 * - `enabled`: gate (typically `vault === "ready"`) so we never fire a
 *   network request before the app is interactive.
 * - Auto-runs once on enable, but is throttled to once per 24h via
 *   localStorage. `checkNow` bypasses the throttle and is wired to the
 *   About dialog's "检查更新" button.
 * - Never throws: on failure `info` keeps its previous value (or null) and
 *   `error` is surfaced through `info.error`. The UI treats a null/errored
 *   info as "no update available" and stays silent.
 */
export function useUpdateCheck(enabled: boolean): {
  info: UpdateInfo | null;
  loading: boolean;
  /** Force a check now, ignoring the 24h throttle. Refreshes the throttle. */
  checkNow: () => void;
} {
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [loading, setLoading] = useState(false);
  // Guards against double-fire under StrictMode + re-renders while a request
  // is in flight.
  const inFlightRef = useRef(false);

  const runCheck = useCallback(async (force: boolean) => {
    if (inFlightRef.current) return;

    // Throttle: skip the automatic path if we checked recently.
    if (!force) {
      try {
        const last = Number(localStorage.getItem(STORAGE_KEY_LAST_CHECK));
        if (Number.isFinite(last) && Date.now() - last < CHECK_INTERVAL_MS) {
          return;
        }
      } catch {
        // localStorage unavailable — proceed with the check.
      }
    }

    inFlightRef.current = true;
    setLoading(true);
    try {
      const result = await checkForUpdates();
      setInfo(result);
      try {
        localStorage.setItem(STORAGE_KEY_LAST_CHECK, String(Date.now()));
      } catch {
        // Persistence best-effort; an inability to write the throttle key
        // just means we might re-check sooner next launch — harmless.
      }
    } catch {
      // Defensive: checkForUpdates resolves rather than rejecting on failure,
      // so this branch should not trigger. If it ever does, drop the info
      // silently rather than surfacing an error to the user.
      setInfo((prev) => prev);
    } finally {
      inFlightRef.current = false;
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!enabled) return;
    void runCheck(false);
    // Intentionally no deps beyond `enabled`/`runCheck` (stable): we want
    // exactly one auto-check per ready-session.
  }, [enabled, runCheck]);

  const checkNow = useCallback(() => {
    void runCheck(true);
  }, [runCheck]);

  return { info, loading, checkNow };
}
