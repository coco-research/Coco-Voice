import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { platform } from "@tauri-apps/plugin-os";
import { checkAccessibilityPermission } from "tauri-plugin-macos-permissions-api";
import { commands } from "@/bindings";
import type { ShortcutRegistrationFailure } from "../lib/types/events";

export const SHORTCUT_REGISTRATION_TOAST_ID = "shortcut-registration-failed";

export function shortcutFailureBodyKey(
  cause: ShortcutRegistrationFailure["cause"],
): string {
  switch (cause) {
    case "taken":
      return "errors.shortcutRegistrationFailedTaken";
    case "rejected":
      return "errors.shortcutRegistrationFailedRejected";
    case "permission":
      return "errors.shortcutRegistrationFailedPermission";
    case "other":
    default:
      return "errors.shortcutRegistrationFailedOther";
  }
}

export interface ShortcutRegistrationToastOptions {
  id: string;
  description: string;
  duration: number;
  action: {
    label: string;
    onClick: () => void;
  };
  cancel: {
    label: string;
    onClick: () => void;
  };
}

export interface ShortcutRegistrationDeps {
  t: (key: string, options?: Record<string, unknown>) => string;
  onOpenSettings: () => void;
  listen: <T>(
    event: string,
    handler: (event: { payload: T }) => void,
  ) => Promise<UnlistenFn>;
  onFocusChanged: (
    handler: (event: { payload: boolean }) => void,
  ) => Promise<UnlistenFn>;
  getPlatform: () => string;
  checkAccessibilityPermission: () => Promise<boolean>;
  initializeShortcuts: () => Promise<unknown>;
  showMainWindowCommand: () => Promise<unknown>;
  showToast: (title: string, options: ShortcutRegistrationToastOptions) => void;
  dismissToast: (id: string) => void;
}

/**
 * Owns the shortcut-registration failure toast, auto-dismiss on success, and
 * macOS focus-return retry after a permission failure (R8, R9).
 *
 * Uses event listeners only — never a timer or polling loop (Q6) — and reads
 * Accessibility trust without prompting macOS (V2).
 */
export function startShortcutRegistrationListener(
  deps: ShortcutRegistrationDeps,
): () => void {
  let disposed = false;
  let pendingPermissionRetry = false;
  let checkingFocus = false;
  const unlisteners: UnlistenFn[] = [];

  const registerUnlisten = (promise: Promise<UnlistenFn>) => {
    void promise
      .then((fn) => {
        if (disposed) {
          fn();
        } else {
          unlisteners.push(fn);
        }
      })
      .catch(() => {});
  };

  const triggerRetry = () => {
    void deps.initializeShortcuts();
  };

  registerUnlisten(
    deps.listen<ShortcutRegistrationFailure[]>(
      "shortcut-registration-failed",
      (event) => {
        const failures = event.payload;
        const first = failures[0];
        if (!first) return;

        if (failures.some((failure) => failure.cause === "permission")) {
          pendingPermissionRetry = true;
        }

        const more =
          failures.length > 1
            ? deps.t("errors.shortcutRegistrationFailedMore", {
                count: failures.length - 1,
              })
            : "";

        // A dead shortcut leaves the app unusable, and a tray app's window is
        // often hidden: bring it forward and keep the notice until dismissed.
        void deps.showMainWindowCommand();
        deps.showToast(deps.t("errors.shortcutRegistrationFailedTitle"), {
          id: SHORTCUT_REGISTRATION_TOAST_ID,
          description:
            deps.t(shortcutFailureBodyKey(first.cause), {
              binding: first.binding,
            }) + more,
          duration: Infinity,
          action: {
            label: deps.t("errors.shortcutRegistrationOpenSettings"),
            onClick: deps.onOpenSettings,
          },
          cancel: {
            label: deps.t("errors.shortcutRegistrationTryAgain"),
            onClick: triggerRetry,
          },
        });
      },
    ),
  );

  registerUnlisten(
    deps.listen("shortcuts-registered", () => {
      pendingPermissionRetry = false;
      deps.dismissToast(SHORTCUT_REGISTRATION_TOAST_ID);
    }),
  );

  if (deps.getPlatform() === "macos") {
    registerUnlisten(
      deps.onFocusChanged((event) => {
        if (!event.payload || !pendingPermissionRetry || checkingFocus) {
          return;
        }
        checkingFocus = true;
        void deps
          .checkAccessibilityPermission()
          .then((trusted) => {
            if (disposed) return;
            if (pendingPermissionRetry && trusted) {
              pendingPermissionRetry = false;
              void deps.initializeShortcuts();
            }
          })
          .catch(() => {})
          .finally(() => {
            checkingFocus = false;
          });
      }),
    );
  }

  return () => {
    disposed = true;
    while (unlisteners.length > 0) {
      const fn = unlisteners.pop();
      try {
        fn?.();
      } catch {
        // Ignore teardown errors from already-closed listeners.
      }
    }
  };
}

export function useShortcutRegistration(onOpenSettings: () => void): void {
  const { t } = useTranslation();
  const onOpenSettingsRef = useRef(onOpenSettings);
  onOpenSettingsRef.current = onOpenSettings;

  useEffect(() => {
    return startShortcutRegistrationListener({
      t: (key, options) => t(key, options),
      onOpenSettings: () => onOpenSettingsRef.current(),
      listen,
      onFocusChanged: (handler) => getCurrentWindow().onFocusChanged(handler),
      getPlatform: platform,
      checkAccessibilityPermission,
      initializeShortcuts: () => commands.initializeShortcuts(),
      showMainWindowCommand: () => commands.showMainWindowCommand(),
      showToast: (title, options) => {
        toast(title, options);
      },
      dismissToast: (id) => {
        toast.dismiss(id);
      },
    });
  }, [t]);
}
