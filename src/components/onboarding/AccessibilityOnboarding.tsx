import { useEffect, useState, useCallback, useRef } from "react";
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
import CocoVoiceTextLogo from "../icons/CocoVoiceTextLogo";
import PermissionAction, { type PermissionStatus } from "./PermissionAction";
import { Keyboard, Mic, Check, Loader2 } from "lucide-react";

interface AccessibilityOnboardingProps {
  onComplete: () => void;
}

type PermissionPlatform = "macos" | "windows" | "other";

interface PermissionsState {
  accessibility: PermissionStatus;
  microphone: PermissionStatus;
}

// A fresh check that still says "needed" keeps a card the user is acting on in "waiting".
const mergeCheck = (
  prev: PermissionStatus,
  next: PermissionCheckResult,
): PermissionStatus => (prev === "waiting" && next === "needed" ? prev : next);

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
          ? runPermissionCheck(checkAccessibilityPermission)
          : Promise.resolve<PermissionCheckResult>("granted"),
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

        if (accessibility === "granted") {
          next.accessibility = "granted";
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
      if ((bothGranted || gaveUp) && pollingRef.current) {
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

  // Show permissions request screen
  return (
    <div className="h-screen w-screen flex flex-col p-6 gap-6 items-center justify-center">
      <div className="flex flex-col items-center gap-2">
        <CocoVoiceTextLogo width={200} />
      </div>

      <div className="max-w-md w-full flex flex-col items-center gap-4">
        <div className="text-center mb-2">
          <h2 className="text-xl font-semibold text-text mb-2">
            {t("onboarding.permissions.title")}
          </h2>
          <p className="text-text/70">
            {t("onboarding.permissions.description")}
          </p>
        </div>

        {/* Microphone Permission Card */}
        {showMicrophonePermission && (
          <div className="w-full p-4 rounded-lg bg-white/5 border border-mid-gray/20">
            <div className="flex items-center gap-4">
              <div className="p-3 rounded-full bg-logo-primary/20 shrink-0">
                <Mic className="w-6 h-6 text-logo-primary" />
              </div>
              <div className="flex-1 min-w-0">
                <h3 className="font-medium text-text">
                  {t("onboarding.permissions.microphone.title")}
                </h3>
                <p className="text-sm text-text/60 mb-3">
                  {t("onboarding.permissions.microphone.description")}
                </p>
                <PermissionAction
                  status={permissions.microphone}
                  grantLabel={
                    isWindows
                      ? t("accessibility.openSettings")
                      : t("onboarding.permissions.grant")
                  }
                  onGrant={handleGrantMicrophone}
                  onRetry={handleRetry}
                />
              </div>
            </div>
          </div>
        )}

        {/* Accessibility Permission Card */}
        {showAccessibilityPermission && (
          <div className="w-full p-4 rounded-lg bg-white/5 border border-mid-gray/20">
            <div className="flex items-center gap-4">
              <div className="p-3 rounded-full bg-logo-primary/20 shrink-0">
                <Keyboard className="w-6 h-6 text-logo-primary" />
              </div>
              <div className="flex-1 min-w-0">
                <h3 className="font-medium text-text">
                  {t("onboarding.permissions.accessibility.title")}
                </h3>
                <p className="text-sm text-text/60 mb-3">
                  {t("onboarding.permissions.accessibility.description")}
                </p>
                <PermissionAction
                  status={permissions.accessibility}
                  grantLabel={t("onboarding.permissions.grant")}
                  onGrant={handleGrantAccessibility}
                  onRetry={handleRetry}
                />
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};

export default AccessibilityOnboarding;
