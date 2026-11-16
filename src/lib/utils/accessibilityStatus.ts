export type InputProbe = "ok" | "failed" | "not-run";
export type AccessibilityStatus = "granted" | "needed" | "stale";

export type DownloadFailureCause =
  "network" | "disk_space" | "checksum" | "other";

export function accessibilityStatus(
  trusted: boolean,
  probe: InputProbe,
): AccessibilityStatus {
  if (!trusted) return "needed";
  if (probe === "ok") return "granted";
  if (probe === "failed") return "stale";
  return "needed";
}

export function downloadFailureTranslationKey(
  cause: DownloadFailureCause,
): string {
  if (cause === "network") return "modelSelector.downloadFailed.network";
  if (cause === "disk_space") return "modelSelector.downloadFailed.diskSpace";
  if (cause === "checksum") return "modelSelector.downloadFailed.checksum";
  return "modelSelector.downloadFailed.other";
}
