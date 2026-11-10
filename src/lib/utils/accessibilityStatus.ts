/** What the input probe found: it worked, it failed, or it wasn't run. */
export type InputProbe = "ok" | "failed" | "not-run";

/** Accessibility as onboarding sees it. */
export type AccessibilityStatus = "granted" | "needed" | "stale";

/**
 * Turns the macOS trusted flag and the input probe into one answer. A trusted
 * app whose probe failed has a stale Accessibility entry (common after an
 * update changes the app's signature): macOS lists it as allowed, but pasting
 * won't work until the entry is removed and added again.
 */
export function accessibilityStatus(
  trusted: boolean,
  probe: InputProbe,
): AccessibilityStatus {
  if (!trusted) return "needed";
  if (probe === "ok") return "granted";
  if (probe === "failed") return "stale";
  return "needed";
}
