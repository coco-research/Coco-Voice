import React, { useEffect, useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  requestAccessibilityPermission,
  checkMicrophonePermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { useSettingsStore } from "@/stores/settingsStore";
import {
  runPermissionCheck,
  windowsMicrophoneAccess,
  type PermissionCheckResult,
} from "@/lib/utils/permissions";
import {
  accessibilityStatus,
  type AccessibilityCheck,
} from "@/lib/utils/accessibilityStatus";
import { invoke } from "@tauri-apps/api/core";
import PermissionAction, { type PermissionStatus } from "./PermissionAction";
import { Check, Loader2 } from "lucide-react";

const DOT_FILLED = "●";
export const STALE_AX_RECHECK_INTERVAL_MS = 1500;

export async function persistContinueWithoutTyping(
  changePasteMethod: (method: string) => Promise<unknown> = (m) =>
    commands.changePasteMethodSetting(m),
  refreshSettings?: () => Promise<unknown>,
  finish?: () => Promise<unknown> | void,
): Promise<void> {
  try {
    await changePasteMethod("none");
    await refreshSettings?.();
  } catch {
    // Clipboard fallback works outside Tauri runtime too.
  }
  await finish?.();
}

export async function probeAccessibilityStatus(
  checkTrusted: () => Promise<PermissionCheckResult> = () =>
    runPermissionCheck(checkAccessibilityPermission),
  runInputProbe: () => Promise<"ok" | "failed"> = () =>
    commands
      .initializeEnigo()
      .then((result) => (result.status === "ok" ? "ok" : "failed"))
      .catch(() => "failed" as const),
  checkBackendStale: () => Promise<boolean> = () =>
    invoke<boolean>("check_accessibility_stale").catch(() => false),
): Promise<AccessibilityCheck> {
  const trusted = await checkTrusted();
  if (trusted !== "granted") return trusted;
  const probe = await runInputProbe();
  if (probe === "failed") {
    console.warn("Input probe failed");
    return accessibilityStatus(trusted, probe);
  }
  const backendStale = await checkBackendStale();
  if (backendStale) return "stale";
  return accessibilityStatus(trusted, probe);
}

interface AccessibilityOnboardingProps {
  onComplete: () => void;
}

type PermissionPlatform = "macos" | "windows" | "other";

interface PermissionsState {
  accessibility: PermissionStatus;
  microphone: PermissionStatus;
}

export interface FirstRunWelcomeCardProps {
  microphoneStatus: PermissionStatus;
  accessibilityStatus: PermissionStatus;
  showMicrophone?: boolean;
  showAccessibility?: boolean;
  isWindows?: boolean;
  onGrantMicrophone: () => void;
  onGrantAccessibility: () => void;
  onRetry: () => void;
  onContinueWithoutTyping: () => void;
}

export const FirstRunWelcomeCard: React.FC<FirstRunWelcomeCardProps> = ({
  microphoneStatus,
  accessibilityStatus: axStatus,
  showMicrophone = true,
  showAccessibility = true,
  isWindows = false,
  onGrantMicrophone,
  onGrantAccessibility,
  onRetry,
  onContinueWithoutTyping,
}) => {
  const { t } = useTranslation();

  return (
    <div
      data-testid="first-run-welcome"
      className="w-full min-h-screen bg-background pt-6 px-7 pb-7 text-start space-y-4"
    >
      <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
        {t("onboarding.welcome.eyebrow", "Welcome")}
      </p>
      <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
        {t(
          "onboarding.welcome.heading",
          "Coco Voice turns your voice into text, on this Mac",
        )}
      </h2>
      <p className="text-[13.5px] leading-[20px] text-mid-gray">
        {t(
          "onboarding.welcome.subtitle",
          "Two permissions, then one model download. About a minute.",
        )}
      </p>

      <div className="divide-y divide-mid-gray/20 rounded-[12px] border border-mid-gray/25 bg-background">
        {showMicrophone ? (
          <div className="flex flex-wrap items-center justify-between gap-3 px-4 py-3.5">
            <div className="min-w-0">
              <div className="text-[14px] font-semibold text-text">
                {t("onboarding.welcome.microphoneTitle", "Microphone")}
              </div>
              <div className="text-[12.5px] text-mid-gray">
                {t("onboarding.welcome.microphoneDesc", "To hear you")}
              </div>
            </div>
            {microphoneStatus === "granted" ? (
              <span className="inline-flex items-center gap-1.5 text-[12.5px] font-semibold text-emerald-600 dark:text-emerald-400">
                <span aria-hidden="true">{DOT_FILLED}</span>
                <span>{t("onboarding.welcome.allowed", "Allowed")}</span>
              </span>
            ) : (
              <PermissionAction
                status={microphoneStatus}
                grantLabel={
                  isWindows
                    ? t("accessibility.openSettings", "Open Settings")
                    : t(
                        "onboarding.welcome.allowMicrophone",
                        "Allow microphone",
                      )
                }
                onGrant={onGrantMicrophone}
                onRetry={onRetry}
              />
            )}
          </div>
        ) : null}

        {showAccessibility ? (
          <div className="flex flex-wrap items-center justify-between gap-3 px-4 py-3.5">
            <div className="min-w-0">
              <div className="text-[14px] font-semibold text-text">
                {t("onboarding.welcome.accessibilityTitle", "Accessibility")}
              </div>
              <div className="text-[12.5px] text-mid-gray">
                {t(
                  "onboarding.welcome.accessibilityDesc",
                  "To type your words into other apps",
                )}
              </div>
            </div>
            {axStatus === "granted" ? (
              <span className="inline-flex items-center gap-1.5 text-[12.5px] font-semibold text-emerald-600 dark:text-emerald-400">
                <span aria-hidden="true">{DOT_FILLED}</span>
                <span>{t("onboarding.welcome.allowed", "Allowed")}</span>
              </span>
            ) : (
              <PermissionAction
                status={axStatus}
                grantLabel={t(
                  "accessibility.openSettings",
                  "Open System Settings",
                )}
                onGrant={onGrantAccessibility}
                onRetry={onRetry}
              />
            )}
          </div>
        ) : null}
      </div>

      {showAccessibility ? (
        <div className="flex flex-wrap items-center gap-3 pt-1">
          <button
            type="button"
            onClick={onContinueWithoutTyping}
            className="inline-flex min-h-[34px] items-center rounded-[10px] border border-mid-gray/45 bg-background px-3.5 text-[13px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
          >
            {t(
              "onboarding.permissions.accessibility.stale.continueWithoutTyping",
              "Continue without typing",
            )}
          </button>
          <span className="text-[12.5px] text-mid-gray">
            {t(
              "onboarding.welcome.continueWithoutTypingHint",
              "Your words go to the clipboard instead. You can turn typing on later.",
            )}
          </span>
        </div>
      ) : null}
    </div>
  );
};

export interface FirstRunStaleAxCardProps {
  onOpenAccessibility: () => void;
  onContinueWithoutTyping: () => void;
}

export const FirstRunStaleAxCard: React.FC<FirstRunStaleAxCardProps> = ({
  onOpenAccessibility,
  onContinueWithoutTyping,
}) => {
  const { t } = useTranslation();

  return (
    <div
      data-testid="first-run-stale-ax"
      className="w-full min-h-screen bg-background pt-6 px-7 pb-7 text-start space-y-4"
    >
      <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
        {t("onboarding.staleAx.eyebrow", "System Settings")}
      </p>
      <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
        {t(
          "onboarding.staleAx.heading",
          "Coco Voice is switched on, but macOS still blocks it",
        )}
      </h2>
      <p className="text-[13.5px] leading-[20px] text-mid-gray">
        {t(
          "onboarding.staleAx.subtitle",
          "This happens after an update: macOS keeps the old entry. Three steps fix it:",
        )}
      </p>

      <ol className="m-0 list-none divide-y divide-mid-gray/20 rounded-[12px] border border-mid-gray/25 bg-background p-0">
        <li className="px-4 py-3 text-[13.5px] text-text">
          {t(
            "onboarding.staleAx.step1",
            "1. In System Settings › Privacy & Security, open the list of apps allowed to control your Mac, select Coco Voice and click minus (−).",
          )}
        </li>
        <li className="px-4 py-3 text-[13.5px] text-text">
          {t(
            "onboarding.staleAx.step2",
            "2. Click plus (+), choose Applications → Coco Voice, switch it on.",
          )}
        </li>
        <li className="px-4 py-3 text-[13.5px] text-text">
          {t(
            "onboarding.staleAx.step3",
            "3. Come back here. We check again by ourselves.",
          )}
        </li>
      </ol>

      <div className="flex flex-wrap items-center gap-3 pt-1">
        <button
          type="button"
          onClick={onOpenAccessibility}
          className="inline-flex min-h-[34px] items-center rounded-[10px] bg-logo-primary px-4 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
        >
          {t("accessibility.openSettings", "Open System Settings")}
        </button>
        <button
          type="button"
          onClick={onContinueWithoutTyping}
          className="inline-flex min-h-[34px] items-center rounded-[10px] border border-mid-gray/45 bg-background px-3.5 text-[13px] font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
        >
          {t(
            "onboarding.permissions.accessibility.stale.continueWithoutTyping",
            "Continue without typing",
          )}
        </button>
      </div>
    </div>
  );
};

// A fresh check that still says "needed" keeps a card the user is acting on in "waiting".
const mergeCheck = (
  prev: PermissionStatus,
  next: PermissionCheckResult | AccessibilityCheck,
): PermissionStatus => (prev === "waiting" && next === "needed" ? prev : next);

/**
 * The trusted flag plus an input probe. The probe only asks whether Coco Voice
 * can create its input simulator (the backend re-checks trust and opens a
 * macOS event source); it never types or pastes anything.
 */
async function checkAccessibility(): Promise<AccessibilityCheck> {
  return probeAccessibilityStatus();
}

const initializeAfterAccessibilityGrant = () =>
  Promise.all([
    commands.initializeEnigo(),
    commands.initializeShortcuts(),
  ]).catch((e) => {
    console.warn("Failed to initialize after permission grant:", e);
  });

const AccessibilityOnboarding: React.FC<AccessibilityOnboardingProps> = ({
  onComplete,
}) => {
  const { t } = useTranslation();
  const refreshAudioDevices = useSettingsStore(
    (state) => state.refreshAudioDevices,
  );
  const refreshOutputDevices = useSettingsStore(
    (state) => state.refreshOutputDevices,
  );
  const [permissionPlatform, setPermissionPlatform] =
    useState<PermissionPlatform | null>(null);
  const [staleAxOverride, setStaleAxOverride] = useState(false);
  const [permissions, setPermissions] = useState<PermissionsState>({
    accessibility: "checking",
    microphone: "checking",
  });
  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const errorCountRef = useRef<number>(0);
  const accessibilityInitializedRef = useRef(false);
  const MAX_POLLING_ERRORS = 3;

  const isMacOS = permissionPlatform === "macos";
  const isWindows = permissionPlatform === "windows";
  const showMicrophonePermission = isMacOS || isWindows;
  const showAccessibilityPermission = isMacOS;

  const allGranted = isMacOS
    ? permissions.accessibility === "granted" &&
      permissions.microphone === "granted"
    : isWindows
      ? permissions.microphone === "granted"
      : true;

  const completeOnboarding = useCallback(async () => {
    await Promise.all([refreshAudioDevices(), refreshOutputDevices()]);
    timeoutRef.current = setTimeout(() => onComplete(), 300);
  }, [onComplete, refreshAudioDevices, refreshOutputDevices]);

  const checkWindowsMicrophone = useCallback(
    async () =>
      windowsMicrophoneAccess(
        await commands.getWindowsMicrophonePermissionStatus(),
      ),
    [],
  );

  // Check every permission onboarding gates on; a check that can't complete is not granted.
  const checkAll = useCallback(
    async (target: PermissionPlatform) => {
      const [accessibility, microphone] = await Promise.all([
        target === "macos"
          ? checkAccessibility()
          : Promise.resolve<AccessibilityCheck>("granted"),
        runPermissionCheck(
          target === "windows"
            ? checkWindowsMicrophone
            : checkMicrophonePermission,
        ),
      ]);
      return { accessibility, microphone };
    },
    [checkWindowsMicrophone],
  );

  // Initialize Enigo and shortcuts once, the first time Accessibility is granted.
  const initializeOnce = useCallback(async () => {
    if (accessibilityInitializedRef.current) return;
    accessibilityInitializedRef.current = true;
    await initializeAfterAccessibilityGrant();
  }, []);

  const finishOnboarding = useCallback(async () => {
    try {
      await completeOnboarding();
    } catch (error) {
      console.error("Failed to finish permission onboarding:", error);
    }
  }, [completeOnboarding]);

  const runChecks = useCallback(
    async (target: PermissionPlatform) => {
      const { accessibility, microphone } = await checkAll(target);

      if (target === "macos" && accessibility === "granted") {
        await initializeOnce();
      }

      setPermissions((prev) => ({
        accessibility: mergeCheck(prev.accessibility, accessibility),
        microphone: mergeCheck(prev.microphone, microphone),
      }));

      if (accessibility === "granted" && microphone === "granted") {
        await finishOnboarding();
      }
    },
    [checkAll, finishOnboarding, initializeOnce],
  );

  // Check platform and permission status on mount
  useEffect(() => {
    const currentPlatform = platform();
    const nextPlatform: PermissionPlatform =
      currentPlatform === "macos"
        ? "macos"
        : currentPlatform === "windows"
          ? "windows"
          : "other";

    setPermissionPlatform(nextPlatform);

    // Skip immediately on unsupported platforms
    if (nextPlatform === "other") {
      onComplete();
      return;
    }

    runChecks(nextPlatform);
  }, [onComplete, runChecks]);

  // Polling for permissions after user clicks a button
  const startPolling = useCallback(() => {
    if (pollingRef.current || permissionPlatform === null) return;

    pollingRef.current = setInterval(async () => {
      const { accessibility, microphone } = await checkAll(permissionPlatform);
      const failed = accessibility === "error" || microphone === "error";
      errorCountRef.current = failed ? errorCountRef.current + 1 : 0;
      // After too many failed checks, stop waiting and show what the last check found.
      const gaveUp = errorCountRef.current >= MAX_POLLING_ERRORS;

      if (permissionPlatform === "macos" && accessibility === "granted") {
        initializeOnce();
      }

      setPermissions((prev) => {
        const next = { ...prev };

        if (accessibility === "granted" || accessibility === "stale") {
          next.accessibility = accessibility;
        }
        if (microphone === "granted") {
          next.microphone = "granted";
        }
        if (gaveUp) {
          if (next.accessibility === "waiting")
            next.accessibility = accessibility;
          if (next.microphone === "waiting") next.microphone = microphone;
        }

        return next;
      });

      const bothGranted =
        accessibility === "granted" && microphone === "granted";
      // A stale entry won't fix itself: stop polling and let the user Check again.
      const stale = accessibility === "stale";
      if ((bothGranted || gaveUp || stale) && pollingRef.current) {
        clearInterval(pollingRef.current);
        pollingRef.current = null;
        errorCountRef.current = 0;
      }
      if (bothGranted) {
        await finishOnboarding();
      }
    }, 1000);
  }, [checkAll, finishOnboarding, initializeOnce, permissionPlatform]);

  // Cleanup polling and timeouts on unmount
  useEffect(() => {
    return () => {
      if (pollingRef.current) {
        clearInterval(pollingRef.current);
      }
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, []);

  const handleRetry = () => {
    if (permissionPlatform && permissionPlatform !== "other") {
      runChecks(permissionPlatform);
    }
  };

  const handleGrantAccessibility = async () => {
    try {
      await requestAccessibilityPermission();
      setPermissions((prev) => ({ ...prev, accessibility: "waiting" }));
      startPolling();
    } catch (error) {
      console.error("Failed to request accessibility permission:", error);
      toast.error(t("onboarding.permissions.errors.requestFailed"));
    }
  };

  const handleGrantMicrophone = async () => {
    try {
      if (isWindows) {
        await commands.openMicrophonePrivacySettings();
      } else {
        await requestMicrophonePermission();
      }

      setPermissions((prev) => ({ ...prev, microphone: "waiting" }));
      startPolling();
    } catch (error) {
      console.error("Failed to request microphone permission:", error);
      toast.error(t("onboarding.permissions.errors.requestFailed"));
    }
  };

  const handleContinueWithoutTyping = async () => {
    setStaleAxOverride(false);
    await persistContinueWithoutTyping(
      (method) => commands.changePasteMethodSetting(method),
      () => useSettingsStore.getState().refreshSettings(),
      finishOnboarding,
    );
  };

  useEffect(() => {
    if (
      permissionPlatform !== "macos" ||
      permissions.accessibility !== "stale"
    ) {
      return;
    }
    const id = setInterval(() => {
      void runChecks("macos");
    }, STALE_AX_RECHECK_INTERVAL_MS);
    return () => clearInterval(id);
  }, [permissionPlatform, permissions.accessibility, runChecks]);

  const isChecking =
    permissionPlatform === null ||
    (isMacOS &&
      permissions.accessibility === "checking" &&
      permissions.microphone === "checking") ||
    (isWindows && permissions.microphone === "checking");

  // Still checking platform/initial permissions
  if (isChecking) {
    return (
      <div className="h-screen w-screen flex items-center justify-center">
        <Loader2 className="w-8 h-8 animate-spin text-text/50" />
      </div>
    );
  }

  // All permissions granted - show success briefly
  if (allGranted) {
    return (
      <div className="h-screen w-screen flex flex-col items-center justify-center gap-4">
        <div className="p-4 rounded-full bg-emerald-500/20">
          <Check className="w-12 h-12 text-emerald-400" />
        </div>
        <p className="text-lg font-medium text-text">
          {t("onboarding.permissions.allGranted")}
        </p>
      </div>
    );
  }

  if (permissions.accessibility === "stale" || staleAxOverride) {
    return (
      <div className="min-h-screen w-full bg-background">
        <FirstRunStaleAxCard
          onOpenAccessibility={handleGrantAccessibility}
          onContinueWithoutTyping={() => {
            void handleContinueWithoutTyping();
          }}
        />
      </div>
    );
  }

  return (
    <div className="min-h-screen w-full bg-background">
      <FirstRunWelcomeCard
        microphoneStatus={permissions.microphone}
        accessibilityStatus={permissions.accessibility}
        showMicrophone={showMicrophonePermission}
        showAccessibility={showAccessibilityPermission}
        isWindows={isWindows}
        onGrantMicrophone={() => {
          void handleGrantMicrophone();
        }}
        onGrantAccessibility={() => {
          void handleGrantAccessibility();
        }}
        onRetry={handleRetry}
        onContinueWithoutTyping={() => {
          void handleContinueWithoutTyping();
        }}
      />
    </div>
  );
};

export default AccessibilityOnboarding;
