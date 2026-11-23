import React, { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ProgressBar } from "../shared";
import { useSettings } from "../../hooks/useSettings";
import { commands } from "../../bindings";
import {
  closeUpdateHandleOnce,
  isInstallableUpdate,
  useUpdateStore,
  type PendingUpdateHandle,
} from "../../stores/updateStore";

export function shouldRunUpdateCheck(
  updateChecksEnabled: boolean,
  isChecking: boolean,
): boolean {
  return updateChecksEnabled && !isChecking;
}

/**
 * Conservative portable-mode probe: any failure or non-boolean/unknown value
 * is treated as portable (`true`), suppressing automatic downloads/installs.
 */
export async function resolvePortableConservatively(
  probe: () => Promise<unknown>,
): Promise<boolean> {
  try {
    const result = await probe();
    if (typeof result === "boolean") {
      return result;
    }
    return true;
  } catch {
    return true;
  }
}

export type GuardedCheckOutcome =
  | { status: "skipped" }
  | { status: "aborted" }
  | { status: "up_to_date" }
  | { status: "portable" }
  | { status: "ready"; version: string }
  | { status: "download_failed" }
  | { status: "uninstallable" };

export async function runGuardedUpdateCheck(deps: {
  check: () => Promise<PendingUpdateHandle | null>;
  isPortable: () => Promise<unknown>;
}): Promise<GuardedCheckOutcome> {
  const store = useUpdateStore.getState();
  const epoch = store.beginCheckEpoch();
  if (epoch === null) {
    return { status: "skipped" };
  }

  // Note: turning updates off prevents any new check, leaves readyVersion not
  // published, and performs no install. If a download is already running, its
  // result is dropped when it returns and its handle is closed once.
  const update = await deps.check();
  if (!useUpdateStore.getState().isEpochCurrent(epoch)) {
    await closeUpdateHandleOnce(update);
    return { status: "aborted" };
  }

  if (!update) {
    useUpdateStore.getState().clearReady();
    return { status: "up_to_date" };
  }

  const portable = await resolvePortableConservatively(deps.isPortable);
  if (!useUpdateStore.getState().isEpochCurrent(epoch)) {
    await closeUpdateHandleOnce(update);
    return { status: "aborted" };
  }

  if (portable) {
    useUpdateStore.getState().clearReady();
    await closeUpdateHandleOnce(update);
    return { status: "portable" };
  }

  if (!isInstallableUpdate(update)) {
    useUpdateStore.getState().clearReady();
    await closeUpdateHandleOnce(update);
    return { status: "uninstallable" };
  }

  if (typeof update.download === "function") {
    try {
      await update.download();
    } catch (downloadErr) {
      console.error("Failed to pre-download update:", downloadErr);
      await closeUpdateHandleOnce(update);
      return { status: "download_failed" };
    }
    if (!useUpdateStore.getState().isEpochCurrent(epoch)) {
      await closeUpdateHandleOnce(update);
      return { status: "aborted" };
    }
  } else if (typeof update.downloadAndInstall !== "function") {
    // Do not publish a ready handle that has never been downloaded and cannot
    // perform a download when installed.
    useUpdateStore.getState().clearReady();
    await closeUpdateHandleOnce(update);
    return { status: "uninstallable" };
  }

  const version = update.version?.trim() ?? "";
  if (version && useUpdateStore.getState().setReady(version, update, epoch)) {
    return { status: "ready", version };
  }
  await closeUpdateHandleOnce(update);
  return { status: "aborted" };
}

interface UpdateCheckerProps {
  className?: string;
}

const UpdateChecker: React.FC<UpdateCheckerProps> = ({ className = "" }) => {
  const { t } = useTranslation();
  // Update checking state
  const [isChecking, setIsChecking] = useState(false);
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [showUpToDate, setShowUpToDate] = useState(false);
  const [showPortableUpdateDialog, setShowPortableUpdateDialog] =
    useState(false);
  const isInstalling = useUpdateStore((state) => state.isInstalling);

  const { settings, isLoading } = useSettings();
  const settingsLoaded = !isLoading && settings !== null;
  const updateChecksEnabled = settings?.update_checks_enabled ?? false;

  const upToDateTimeoutRef = useRef<ReturnType<typeof setTimeout>>();
  const isManualCheckRef = useRef(false);
  const downloadedBytesRef = useRef(0);
  const contentLengthRef = useRef(0);

  useEffect(() => {
    // Wait for settings to load before doing anything
    if (!settingsLoaded) return;

    useUpdateStore.getState().setUpdateChecksEnabled(updateChecksEnabled);

    if (!updateChecksEnabled) {
      if (upToDateTimeoutRef.current) {
        clearTimeout(upToDateTimeoutRef.current);
      }
      setIsChecking(false);
      setUpdateAvailable(false);
      setShowUpToDate(false);
      return;
    }

    void checkForUpdates();

    // Listen for update check events
    const updateUnlisten = listen("check-for-updates", () => {
      handleManualUpdateCheck();
    });

    return () => {
      if (upToDateTimeoutRef.current) {
        clearTimeout(upToDateTimeoutRef.current);
      }
      updateUnlisten.then((fn) => fn());
    };
  }, [settingsLoaded, updateChecksEnabled]);

  // Update checking functions
  const checkForUpdates = async () => {
    if (!shouldRunUpdateCheck(updateChecksEnabled, isChecking)) return;

    try {
      setIsChecking(true);
      const outcome = await runGuardedUpdateCheck({
        check: async () =>
          (await check()) as unknown as PendingUpdateHandle | null,
        isPortable: () => commands.isPortable(),
      });

      if (outcome.status === "aborted" || outcome.status === "skipped") {
        setUpdateAvailable(false);
        return;
      }

      if (outcome.status === "up_to_date") {
        setUpdateAvailable(false);
        if (isManualCheckRef.current) {
          setShowUpToDate(true);
          if (upToDateTimeoutRef.current) {
            clearTimeout(upToDateTimeoutRef.current);
          }
          upToDateTimeoutRef.current = setTimeout(() => {
            setShowUpToDate(false);
          }, 3000);
        }
        return;
      }

      if (
        outcome.status === "ready" ||
        outcome.status === "portable" ||
        outcome.status === "download_failed"
      ) {
        setUpdateAvailable(true);
        setShowUpToDate(false);
      } else {
        setUpdateAvailable(false);
      }
    } catch (error) {
      console.error("Failed to check for updates:", error);
    } finally {
      setIsChecking(false);
      isManualCheckRef.current = false;
    }
  };

  const handleManualUpdateCheck = () => {
    if (!updateChecksEnabled) return;
    isManualCheckRef.current = true;
    void checkForUpdates();
  };

  const installUpdate = async () => {
    if (!updateChecksEnabled || useUpdateStore.getState().isInstalling) return;

    const portable = await resolvePortableConservatively(() =>
      commands.isPortable(),
    );
    if (portable) {
      setShowPortableUpdateDialog(true);
      return;
    }

    const epoch = useUpdateStore.getState().beginCheckEpoch();
    if (epoch === null) return;

    try {
      setDownloadProgress(0);
      downloadedBytesRef.current = 0;
      contentLengthRef.current = 0;
      const update = (await check()) as unknown as PendingUpdateHandle | null;

      if (!useUpdateStore.getState().isEpochCurrent(epoch)) {
        return;
      }

      if (!isInstallableUpdate(update)) {
        console.log("No installable update available during install attempt");
        return;
      }

      // Fresh handle from manual check() is NEVER marked `alreadyDownloaded`:
      // `installHandleAndRestart` must explicitly downloadAndInstall (or
      // download + install) rather than calling bare `install()`.
      await useUpdateStore.getState().installHandleAndRestart(
        update,
        () => relaunch(),
        (rawEvent) => {
          const event = rawEvent as {
            event?: string;
            data?: { contentLength?: number; chunkLength?: number };
          };
          switch (event?.event) {
            case "Started":
              downloadedBytesRef.current = 0;
              contentLengthRef.current = event.data?.contentLength ?? 0;
              break;
            case "Progress": {
              downloadedBytesRef.current += event.data?.chunkLength ?? 0;
              const progress =
                contentLengthRef.current > 0
                  ? Math.round(
                      (downloadedBytesRef.current / contentLengthRef.current) *
                        100,
                    )
                  : 0;
              setDownloadProgress(Math.min(progress, 100));
              break;
            }
          }
        },
      );
    } catch (error) {
      console.error("Failed to install update:", error);
    } finally {
      setDownloadProgress(0);
      downloadedBytesRef.current = 0;
      contentLengthRef.current = 0;
    }
  };

  // Update status functions
  const getUpdateStatusText = () => {
    if (!updateChecksEnabled) {
      return t("footer.updateCheckingDisabled");
    }
    if (isInstalling) {
      return downloadProgress > 0 && downloadProgress < 100
        ? t("footer.downloading", {
            progress: downloadProgress.toString().padStart(3),
          })
        : downloadProgress === 100
          ? t("footer.installing")
          : t("footer.preparing");
    }
    if (isChecking) return t("footer.checkingUpdates");
    if (showUpToDate) return t("footer.upToDate");
    if (updateAvailable) return t("footer.updateAvailableShort");
    return t("footer.checkForUpdates");
  };

  const getUpdateStatusAction = () => {
    if (!updateChecksEnabled) return undefined;
    if (updateAvailable && !isInstalling) return installUpdate;
    if (!isChecking && !isInstalling && !updateAvailable)
      return handleManualUpdateCheck;
    return undefined;
  };

  const isUpdateDisabled = !updateChecksEnabled || isChecking || isInstalling;
  const isUpdateClickable =
    !isUpdateDisabled && (updateAvailable || (!isChecking && !showUpToDate));

  return (
    <>
      {showPortableUpdateDialog && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className="bg-bg border border-border rounded-lg p-6 max-w-md w-full mx-4 space-y-4">
            <h2 className="text-base font-semibold">
              {t("footer.portableUpdateTitle")}
            </h2>
            <p className="text-sm text-text/70">
              {t("footer.portableUpdateMessage")}
            </p>
            <div className="flex gap-2 justify-end">
              <button
                className="px-3 py-1.5 text-sm rounded border border-border hover:bg-border/50 transition-colors"
                onClick={() => setShowPortableUpdateDialog(false)}
              >
                {t("common.close")}
              </button>
              <button
                className="px-3 py-1.5 text-sm rounded bg-logo-primary text-white hover:bg-logo-primary/80 transition-colors"
                onClick={() => {
                  openUrl(
                    "https://github.com/coco-research/Coco-Voice/releases/latest",
                  );
                  setShowPortableUpdateDialog(false);
                }}
              >
                {t("footer.portableUpdateButton")}
              </button>
            </div>
          </div>
        </div>
      )}
      <div className={`flex items-center gap-3 ${className}`}>
        {isUpdateClickable ? (
          <button
            onClick={getUpdateStatusAction()}
            disabled={isUpdateDisabled}
            className={`transition-colors disabled:opacity-50 tabular-nums ${
              updateAvailable
                ? "text-logo-primary hover:text-logo-primary/80 font-medium"
                : "text-text/60 hover:text-text/80"
            }`}
          >
            {getUpdateStatusText()}
          </button>
        ) : (
          <span className="text-text/60 tabular-nums">
            {getUpdateStatusText()}
          </span>
        )}

        {isInstalling && downloadProgress > 0 && downloadProgress < 100 && (
          <ProgressBar
            progress={[
              {
                id: "update",
                percentage: downloadProgress,
              },
            ]}
            size="large"
          />
        )}
      </div>
    </>
  );
};

export default UpdateChecker;
