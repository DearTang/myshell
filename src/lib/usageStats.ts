/**
 * Anonymous per-version usage statistics via Umami Cloud.
 *
 * Design (privacy-first):
 *   - A random device ID is generated once and stored in localStorage. It is
 *     NOT tied to any personal info — it exists only so Umami can deduplicate
 *     the same device across launches (counting unique installs, not launches).
 *   - On each new version's first launch, one anonymous event is sent:
 *       { device_id, app_version, os }
 *     No hosts, usernames, IPs, or connection data are ever sent.
 *   - Consent model: the user is asked once. If they agree, the preference is
 *     remembered and future versions report silently (no re-prompting on
 *     upgrades). If they decline, the preference is NOT remembered — so each
 *     new version asks again. If a send fails after the user agreed, the
 *     next launch retries silently (no re-prompt) until it succeeds.
 *
 * Umami Cloud endpoint: POST https://cloud.umami.is/api/send
 *   body: { type: "event", payload: { website, name, data: {...} } }
 *
 * The website ID is public by design (Umami's model — it only allows sending
 * events to your dashboard; it can't read data back or compromise anything).
 */

// --- Config (from your Umami Cloud dashboard) ---
const UMAMI_ENDPOINT = "https://cloud.umami.is/api/send";
const UMAMI_WEBSITE_ID = "e3302fe3-b5fc-411f-8bc7-5948d3c923bb";

// --- localStorage keys ---
const KEY_DEVICE_ID = "myshell.deviceId";
const KEY_CONSENT = "myshell.statsConsent";
const KEY_VERSION = "myshell.statsVersion";

/** Generate or retrieve the persistent anonymous device ID. */
export function getDeviceId(): string {
  try {
    let id = localStorage.getItem(KEY_DEVICE_ID);
    if (!id) {
      // Crypto.randomUUID is available in WebView2 (Chromium 92+).
      id = crypto.randomUUID();
      localStorage.setItem(KEY_DEVICE_ID, id);
    }
    return id;
  } catch {
    // localStorage unavailable — return a throwaway ID for this session.
    return "unknown-device";
  }
}

/** Has the user previously agreed to anonymous usage stats? */
export function hasStatsConsent(): boolean {
  try {
    return localStorage.getItem(KEY_CONSENT) === "agreed";
  } catch {
    return false;
  }
}

/**
 * Has the current version already been handled (prompted AND decided)?
 *
 * `KEY_VERSION` records that the user has already been prompted for this
 * version and made a decision — agree OR decline — so we should NOT
 * re-prompt on subsequent launches of the same version. Only a version
 * bump re-triggers the prompt. See {@link markVersionHandled}.
 */
export function isVersionReported(version: string): boolean {
  try {
    return localStorage.getItem(KEY_VERSION) === version;
  } catch {
    return false;
  }
}

/**
 * Mark a version as "handled" — the user has been prompted and made a
 * decision, so the same version must NOT re-prompt on later launches. Called
 * from BOTH the agree and decline callbacks, which guarantees a version is
 * stamped regardless of the user's choice (or of a network failure on the
 * agree path). Only a version bump clears this and re-prompts.
 *
 * `reportVersion` also writes this key on a successful send (redundant when
 * the agree path already called this, but harmless and keeps report-only
 * callers consistent).
 */
export function markVersionHandled(version: string): void {
  try {
    localStorage.setItem(KEY_VERSION, version);
  } catch {
    // best-effort
  }
}

/**
 * Send one anonymous event for the current version. Fire-and-forget —
 * network failures are silently ignored (this is best-effort telemetry, not
 * critical functionality). Never throws.
 */
export async function reportVersion(version: string, os: string): Promise<void> {
  const deviceId = getDeviceId();

  const body = {
    type: "event" as const,
    payload: {
      website: UMAMI_WEBSITE_ID,
      name: `myshell_app_launch_v${version}`,
      // Custom properties — viewable & groupable in the Umami dashboard.
      data: {
        device_id: deviceId,
        app_version: version,
        os,
      },
    },
  };

  try {
    await fetch(UMAMI_ENDPOINT, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    // Mark this version as reported so we don't re-send on next launch.
    try {
      localStorage.setItem(KEY_VERSION, version);
    } catch {
      // best-effort
    }
  } catch {
    // Network failure — don't mark as reported, retry next launch.
    // Silent: telemetry must never disrupt the user.
  }
}

/**
 * Record the user's consent decision. "agreed" is persisted so future
 * versions report silently. A decline is NOT persisted — each new version
 * will re-prompt.
 */
export function setStatsConsent(agreed: boolean): void {
  try {
    if (agreed) {
      localStorage.setItem(KEY_CONSENT, "agreed");
    } else {
      // Don't store "declined" — absence means "ask again next version".
      localStorage.removeItem(KEY_CONSENT);
    }
  } catch {
    // best-effort
  }
}

/**
 * Determine whether we need to report the current version, and whether we
 * need to ask for consent first. Call this on app startup (after vault unlock).
 *
 * Consent model (ask-once):
 *   - First ever launch (no consent recorded): prompt the user.
 *   - User previously agreed: report SILENTLY for all future versions, no
 *     re-prompting on upgrades. Also covers retry-after-failure: if a prior
 *     send failed (network/CSP), the version stamp wasn't written, so the
 *     next launch returns {shouldReport:true, hasConsent:true} and retries
 *     silently.
 *   - User declined: consent is NOT remembered, so each new version
 *     re-prompts (a version bump is a meaningful new event worth re-asking).
 *
 * "Already handled" (`isVersionReported`) means THIS version was already
 * successfully reported (or explicitly declined), so we do nothing.
 *
 * Returns:
 *   - { shouldReport: true,  hasConsent: true  } → agreed before, report silently
 *   - { shouldReport: true,  hasConsent: false } → no prior consent, prompt user
 *   - { shouldReport: false, hasConsent: false } → already handled, do nothing
 */
export function checkReportNeeded(version: string): {
  shouldReport: boolean;
  hasConsent: boolean;
} {
  if (isVersionReported(version)) {
    return { shouldReport: false, hasConsent: false };
  }
  // Version not yet reported. If the user previously agreed (hasConsent),
  // retry SILENTLY — this covers the case where a prior send failed (network
  // error, CSP block) after the user clicked "agree": we don't re-prompt,
  // just try again. Only prompt when there's no prior consent.
  const hasConsent = hasStatsConsent();
  return { shouldReport: true, hasConsent };
}
