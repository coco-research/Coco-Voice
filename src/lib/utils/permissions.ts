import type { WindowsMicrophonePermissionStatus } from "@/bindings";

/** What a permission check found. "error" means the check couldn't say. */
export type PermissionCheckResult = "granted" | "needed" | "error";

/**
 * The one place a permission check becomes granted or not. Only a definite
 * `true` is granted: a thrown error, a rejected promise or any other value
 * (an unknown state) is not, and comes back as "error" so onboarding can
 * offer a Retry instead of letting the user through.
 */
export async function runPermissionCheck(
  check: () => Promise<boolean | null>,
): Promise<PermissionCheckResult> {
  try {
    const result = await check();
    if (result === true) return "granted";
    if (result === false) return "needed";
    return "error";
  } catch (error) {
    console.warn("Permission check failed:", error);
    return "error";
  }
}

/** Windows microphone access: allowed, denied, or null when unknown. */
export function windowsMicrophoneAccess(
  status: WindowsMicrophonePermissionStatus,
): boolean | null {
  if (!status.supported) return true;
  if (status.overall_access === "allowed") return true;
  if (status.overall_access === "denied") return false;
  return null;
}
