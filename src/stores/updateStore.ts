import { create } from "zustand";
import { relaunch } from "@tauri-apps/plugin-process";

export interface PendingUpdateHandle {
  version?: string;
  download?: (cb?: (event: unknown) => void) => Promise<void>;
  install?: () => Promise<void>;
  downloadAndInstall?: (cb?: (event: unknown) => void) => Promise<void>;
  close?: () => Promise<void>;
}

export interface InstallHandleOptions {
  /**
   * True ONLY when installing a handle previously pre-downloaded and published
   * via `setReady(...)`. Fresh handles (such as manual Check -> Install) must
   * never invoke bare `install()` without downloading first.
   */
  alreadyDownloaded?: boolean;
}

/**
 * Tracks exact update handle object references that have been downloaded and
 * published through `setReady(...)`. A fresh handle returned by `check()` is
 * never in this set and cannot bypass explicit download.
 */
const downloadedHandles = new WeakSet<PendingUpdateHandle>();

/**
 * Tracks handles on which `close()` has already been initiated so close is
 * strictly idempotent (called at most once per handle).
 */
const closedHandles = new WeakSet<PendingUpdateHandle>();

export async function closeUpdateHandleOnce(
  handle: PendingUpdateHandle | null | undefined,
): Promise<void> {
  if (!handle || closedHandles.has(handle)) {
    return;
  }
  closedHandles.add(handle);
  if (typeof handle.close === "function") {
    try {
      await handle.close();
    } catch {
      // Best-effort cleanup of a discarded update handle.
    }
  }
}

export function isDownloadedUpdateHandle(
  update: PendingUpdateHandle | null | undefined,
): boolean {
  return Boolean(update && downloadedHandles.has(update));
}

export function isInstallableUpdate(
  update: PendingUpdateHandle | null | undefined,
): update is PendingUpdateHandle {
  return Boolean(
    update &&
    (typeof update.install === "function" ||
      typeof update.downloadAndInstall === "function"),
  );
}

interface UpdateStoreState {
  updateChecksEnabled: boolean;
  checkEpoch: number;
  readyVersion: string | null;
  pendingUpdate: PendingUpdateHandle | null;
  isInstalling: boolean;
  setUpdateChecksEnabled: (enabled: boolean) => void;
  beginCheckEpoch: () => number | null;
  isEpochCurrent: (epoch: number) => boolean;
  setReady: (
    version: string,
    update: PendingUpdateHandle | null,
    epoch?: number,
  ) => boolean;
  clearReady: () => void;
  installHandleAndRestart: (
    update: PendingUpdateHandle | null,
    relauncher?: () => Promise<void>,
    onProgress?: (event: unknown) => void,
    options?: InstallHandleOptions,
  ) => Promise<boolean>;
  installAndRestart: (relauncher?: () => Promise<void>) => Promise<boolean>;
}

export const useUpdateStore = create<UpdateStoreState>((set, get) => ({
  updateChecksEnabled: true,
  checkEpoch: 0,
  readyVersion: null,
  pendingUpdate: null,
  isInstalling: false,

  setUpdateChecksEnabled: (enabled) => {
    if (!enabled) {
      // Disabling update checks means: no new check, no "Update ready" notice
      // published, and no install. Any already-ready handle is cleared and
      // closed; the handle of a download already running is not published and
      // is closed when that download returns (dropped when it returns).
      // NEVER reset `isInstalling` (which is owned exclusively by an active install).
      const prevPending = get().pendingUpdate;
      set((state) => ({
        updateChecksEnabled: false,
        checkEpoch: state.checkEpoch + 1,
        readyVersion: null,
        pendingUpdate: null,
      }));
      if (prevPending) {
        void closeUpdateHandleOnce(prevPending);
      }
      return;
    }
    set({ updateChecksEnabled: true });
  },

  beginCheckEpoch: () => {
    if (!get().updateChecksEnabled) {
      return null;
    }
    let nextEpoch = 0;
    set((state) => {
      nextEpoch = state.checkEpoch + 1;
      return { checkEpoch: nextEpoch };
    });
    return nextEpoch;
  },

  isEpochCurrent: (epoch) => {
    const state = get();
    return state.updateChecksEnabled && state.checkEpoch === epoch;
  },

  setReady: (version, update, epoch) => {
    const state = get();
    if (!state.updateChecksEnabled) return false;
    if (epoch !== undefined && state.checkEpoch !== epoch) return false;
    const clean = version.trim();
    if (!clean || !isInstallableUpdate(update)) return false;
    downloadedHandles.add(update);
    set({ readyVersion: clean, pendingUpdate: update });
    return true;
  },

  clearReady: () => {
    // Clearing pending state must NEVER unlock an in-flight install.
    set({ readyVersion: null, pendingUpdate: null });
  },

  installHandleAndRestart: async (
    update,
    relauncher = () => relaunch(),
    onProgress,
    options,
  ) => {
    if (get().isInstalling) return false;
    if (!isInstallableUpdate(update)) return false;
    const canInstallDirectly =
      options?.alreadyDownloaded === true &&
      downloadedHandles.has(update) &&
      typeof update.install === "function";
    const canDownloadAndInstall =
      typeof update.downloadAndInstall === "function";
    const canDownloadThenInstall =
      typeof update.download === "function" &&
      typeof update.install === "function";

    if (
      !canInstallDirectly &&
      !canDownloadAndInstall &&
      !canDownloadThenInstall
    ) {
      return false;
    }

    set({ isInstalling: true });
    try {
      if (canInstallDirectly) {
        await update.install!();
      } else if (canDownloadAndInstall) {
        await update.downloadAndInstall!(onProgress);
      } else if (canDownloadThenInstall) {
        await update.download!(onProgress);
        await update.install!();
      } else {
        return false;
      }
      await relauncher();
      return true;
    } catch (error) {
      console.error("Failed to install update and restart:", error);
      return false;
    } finally {
      set({ isInstalling: false });
    }
  },

  installAndRestart: async (relauncher = () => relaunch()) => {
    const updateToInstall = get().pendingUpdate;
    if (!updateToInstall || !downloadedHandles.has(updateToInstall)) {
      return false;
    }
    return get().installHandleAndRestart(
      updateToInstall,
      relauncher,
      undefined,
      { alreadyDownloaded: true },
    );
  },
}));
