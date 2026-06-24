import { useCallback, useEffect, useRef, useState } from "react";
import { checkForUpdates, type UpdateInfo } from "../api";

/**
 * Background update check against Gitee's latest release.
 *
 * - `enabled`: gate (typically `vault === "ready"`) so we never fire a
 *   network request before the app is interactive — i.e. before the user has
 *   logged in and reached the main page.
 * - Runs **once per session** when `enabled` flips true (once per login),
 *   per the update-check spec. No cross-session throttle: the user asked for
 *   a fresh check each time they land on the main page.
 * - `checkNow` is the manual re-check wired to the About dialog's
 *   "检查更新" button.
 * - Never throws: on failure `info` keeps its previous value (or null) and
 *   the failure is encoded in `info.error`. The UI treats a null/errored
 *   info as "no update available" and stays silent.
 */
export function useUpdateCheck(enabled: boolean): {
  info: UpdateInfo | null;
  loading: boolean;
  /** Force a fresh check now. */
  checkNow: () => void;
} {
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [loading, setLoading] = useState(false);
  // Guards against double-fire under StrictMode + re-renders while a request
  // is in flight, and ensures the auto path runs at most once per enabled-session.
  const inFlightRef = useRef(false);
  const autoRanRef = useRef(false);

  const runCheck = useCallback(async () => {
    if (inFlightRef.current) return;
    inFlightRef.current = true;
    setLoading(true);
    try {
      const result = await checkForUpdates();
      setInfo(result);
    } catch {
      // Defensive: checkForUpdates resolves rather than rejecting on failure,
      // so this branch should not trigger. If it ever does, keep the previous
      // info rather than surfacing an error to the user.
      setInfo((prev) => prev);
    } finally {
      inFlightRef.current = false;
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!enabled) return;
    // Auto-check exactly once per enabled-session (once per login).
    if (autoRanRef.current) return;
    autoRanRef.current = true;
    void runCheck();
  }, [enabled, runCheck]);

  const checkNow = useCallback(() => {
    void runCheck();
  }, [runCheck]);

  return { info, loading, checkNow };
}
