import type { PermissionCheckResult } from "./permissions";

/** What the input probe found: it worked, it failed, or it wasn't run. */
export type InputProbe = "ok" | "failed" | "not-run";

/** Accessibility as onboarding sees it: a permission check, or a stale entry. */
export type AccessibilityCheck = PermissionCheckResult | "stale";

/**
 * Turns the macOS trusted check and the input probe into one answer. A trusted
 * app whose probe failed has a stale Accessibility entry (common after an
 * update changes the app's signature): macOS lists it as allowed, but pasting
 * won't work until the entry is removed and added again. Only a trusted app
 * whose probe worked is granted; an unclear check stays as it was.
 */
export function accessibilityStatus(
  trusted: PermissionCheckResult,
  probe: InputProbe,
): AccessibilityCheck {
  if (trusted !== "granted") return trusted;
  if (probe === "ok") return "granted";
  if (probe === "failed") return "stale";
  return "needed";
}

export type DownloadFailureCause = "network" | "disk_space" | "checksum" | "other";
export function downloadFailureTranslationKey(cause: DownloadFailureCause): string {
  if (cause === "network") return "modelSelector.downloadFailed.network";
  if (cause === "disk_space") return "modelSelector.downloadFailed.diskSpace";
  if (cause === "checksum") return "modelSelector.downloadFailed.checksum";
  return "modelSelector.downloadFailed.other";
}
