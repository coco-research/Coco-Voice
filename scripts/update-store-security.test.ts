import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import {
  isInstallableUpdate,
  useUpdateStore,
  type PendingUpdateHandle,
} from "../src/stores/updateStore";
import {
  resolvePortableConservatively,
  runGuardedUpdateCheck,
} from "../src/components/update-checker/UpdateChecker";
import { restartAppForUpdate } from "../src/components/home/HomeView";
import { resolveWhatsNewGateDecision } from "../src/components/whats-new/UpdateAndTraySurfaces";

function resetUpdateStore() {
  useUpdateStore.setState({
    updateChecksEnabled: true,
    checkEpoch: 0,
    readyVersion: null,
    pendingUpdate: null,
    isInstalling: false,
  });
}

describe("Plan A Updater Security Hardening", () => {
  beforeEach(() => {
    resetUpdateStore();
  });

  afterEach(() => {
    resetUpdateStore();
  });

  test("B.1a: disabling update checks while check() is in flight prevents download and readyVersion publication", async () => {
    let resolveCheck!: (val: PendingUpdateHandle | null) => void;
    const checkPromise = new Promise<PendingUpdateHandle | null>((resolve) => {
      resolveCheck = resolve;
    });
    let downloadCalled = false;
    let portableCalled = false;

    const run = runGuardedUpdateCheck({
      check: () => checkPromise,
      isPortable: async () => {
        portableCalled = true;
        return false;
      },
    });

    // User disables update checks while check() is still in flight.
    useUpdateStore.getState().setUpdateChecksEnabled(false);

    resolveCheck({
      version: "0.21.0",
      download: async () => {
        downloadCalled = true;
      },
      install: async () => {},
    });

    const outcome = await run;
    expect(outcome).toEqual({ status: "aborted" });
    expect(portableCalled).toBe(false);
    expect(downloadCalled).toBe(false);
    expect(useUpdateStore.getState().readyVersion).toBeNull();
    expect(useUpdateStore.getState().pendingUpdate).toBeNull();
  });

  test("B.1b: disabling update checks while download() is in flight prevents readyVersion publication", async () => {
    let resolveDownload!: () => void;
    const downloadPromise = new Promise<void>((resolve) => {
      resolveDownload = resolve;
    });
    let markDownloadStarted!: () => void;
    const downloadStartedPromise = new Promise<void>((resolve) => {
      markDownloadStarted = resolve;
    });
    let downloadStarted = false;

    const run = runGuardedUpdateCheck({
      check: async () => ({
        version: "0.21.0",
        download: async () => {
          downloadStarted = true;
          markDownloadStarted();
          await downloadPromise;
        },
        install: async () => {},
      }),
      isPortable: async () => false,
    });

    await downloadStartedPromise;
    expect(downloadStarted).toBe(true);

    // User disables update checks while download() is in flight.
    useUpdateStore.getState().setUpdateChecksEnabled(false);
    resolveDownload();

    const outcome = await run;
    expect(outcome).toEqual({ status: "aborted" });
    expect(useUpdateStore.getState().readyVersion).toBeNull();
    expect(useUpdateStore.getState().pendingUpdate).toBeNull();
  });

  test("B.2: clearReady() and setUpdateChecksEnabled(false) never unlock an in-flight install", async () => {
    let releaseFirstInstall!: () => void;
    const firstInstallGate = new Promise<void>((resolve) => {
      releaseFirstInstall = resolve;
    });
    let installCount = 0;
    let relaunchCount = 0;

    useUpdateStore.getState().setReady("0.21.0", {
      version: "0.21.0",
      install: async () => {
        installCount += 1;
        await firstInstallGate;
      },
    });

    const firstRun = useUpdateStore.getState().installAndRestart(async () => {
      relaunchCount += 1;
    });

    expect(useUpdateStore.getState().isInstalling).toBe(true);

    // Clearing pending state or disabling checks MUST NOT clear isInstalling!
    useUpdateStore.getState().clearReady();
    expect(useUpdateStore.getState().isInstalling).toBe(true);
    useUpdateStore.getState().setUpdateChecksEnabled(false);
    expect(useUpdateStore.getState().isInstalling).toBe(true);

    // Attempting a second concurrent install while first is running is rejected.
    const secondRun = await useUpdateStore.getState().installHandleAndRestart(
      {
        version: "0.21.1",
        install: async () => {
          installCount += 1;
        },
      },
      async () => {
        relaunchCount += 1;
      },
    );
    expect(secondRun).toBe(false);
    expect(installCount).toBe(1);

    // Finish first install -> succeeds and finally releases isInstalling.
    releaseFirstInstall();
    expect(await firstRun).toBe(true);
    expect(relaunchCount).toBe(1);
    expect(useUpdateStore.getState().isInstalling).toBe(false);
  });

  test("B.3: missing, non-installable, or failing install NEVER calls relaunch()", async () => {
    let relaunchCalls = 0;
    const fakeRelaunch = async () => {
      relaunchCalls += 1;
    };

    // 1) No pending update at all (null)
    expect(
      await useUpdateStore.getState().installAndRestart(fakeRelaunch),
    ).toBe(false);
    expect(await restartAppForUpdate(fakeRelaunch)).toBe(false);
    expect(relaunchCalls).toBe(0);

    // 2) Non-installable handle (lacks both install and downloadAndInstall)
    const bogus: PendingUpdateHandle = { version: "0.21.0" };
    expect(isInstallableUpdate(bogus)).toBe(false);
    expect(useUpdateStore.getState().setReady("0.21.0", bogus)).toBe(false);
    expect(useUpdateStore.getState().readyVersion).toBeNull();
    expect(
      await useUpdateStore
        .getState()
        .installHandleAndRestart(bogus, fakeRelaunch),
    ).toBe(false);
    expect(relaunchCalls).toBe(0);

    // 3) Install throws/rejects -> relaunch MUST NOT run, lock MUST release
    useUpdateStore.getState().setReady("0.21.0", {
      version: "0.21.0",
      install: async () => {
        throw new Error("signature verification failed");
      },
    });
    expect(await restartAppForUpdate(fakeRelaunch)).toBe(false);
    expect(relaunchCalls).toBe(0);
    expect(useUpdateStore.getState().isInstalling).toBe(false);
  });

  test("B.4: unknown or failing portable-mode probe conservatively disables auto-update", async () => {
    expect(
      await resolvePortableConservatively(async () => {
        throw new Error("IPC error");
      }),
    ).toBe(true);
    expect(await resolvePortableConservatively(async () => undefined)).toBe(
      true,
    );
    expect(await resolvePortableConservatively(async () => null)).toBe(true);
    expect(await resolvePortableConservatively(async () => "false")).toBe(true);
    expect(await resolvePortableConservatively(async () => false)).toBe(false);
    expect(await resolvePortableConservatively(async () => true)).toBe(true);

    let downloadCalled = false;
    const outcome = await runGuardedUpdateCheck({
      check: async () => ({
        version: "0.21.0",
        download: async () => {
          downloadCalled = true;
        },
        install: async () => {},
      }),
      isPortable: async () => {
        throw new Error("probe unavailable");
      },
    });
    expect(outcome).toEqual({ status: "portable" });
    expect(downloadCalled).toBe(false);
    expect(useUpdateStore.getState().readyVersion).toBeNull();
  });

  test("B.5: went-back notice is unreachable in 0.20 on both health rollback and client version downgrade", () => {
    // Even if a stale health rollback string were provided, 0.20 never surfaces went_back.
    expect(
      resolveWhatsNewGateDecision({
        currentVersion: "0.20.0",
        lastSeenVersion: "0.20.0",
        showWhatsNewOnUpdate: false,
        healthRolledBackFrom: "0.20.1",
      }),
    ).toEqual({
      shouldShow: false,
      rolledBackFrom: null,
      noticeKind: "none",
    });

    // Client downgrade (lastSeen > current) stays silent ("none") in 0.20.
    expect(
      resolveWhatsNewGateDecision({
        currentVersion: "0.20.0",
        lastSeenVersion: "0.20.1",
        showWhatsNewOnUpdate: false,
        healthRolledBackFrom: null,
      }),
    ).toEqual({
      shouldShow: false,
      rolledBackFrom: null,
      noticeKind: "none",
    });

    // Downgrade with showWhatsNewOnUpdate=true also returns "none".
    expect(
      resolveWhatsNewGateDecision({
        currentVersion: "0.20.0",
        lastSeenVersion: "0.20.1",
        showWhatsNewOnUpdate: true,
        healthRolledBackFrom: null,
      }),
    ).toEqual({
      shouldShow: false,
      rolledBackFrom: null,
      noticeKind: "none",
    });

    // Normal upgrade shows What's new without rollback
    expect(
      resolveWhatsNewGateDecision({
        currentVersion: "0.20.0",
        lastSeenVersion: "0.19.0",
        showWhatsNewOnUpdate: true,
        healthRolledBackFrom: null,
      }),
    ).toEqual({
      shouldShow: true,
      rolledBackFrom: null,
      noticeKind: "whats_new",
    });

    // Same version shows nothing
    expect(
      resolveWhatsNewGateDecision({
        currentVersion: "0.20.0",
        lastSeenVersion: "0.20.0",
        showWhatsNewOnUpdate: true,
        healthRolledBackFrom: null,
      }),
    ).toEqual({
      shouldShow: false,
      rolledBackFrom: null,
      noticeKind: "none",
    });
  });

  test("B.6: manual flow never calls bare install() on a fresh handle; background-downloaded handle installs only for its exact registered handle", async () => {
    let relaunchCount = 0;
    const fakeRelaunch = async () => {
      relaunchCount += 1;
    };

    // 1) Fresh handle exposing downloadAndInstall + install (where bare install throws if not downloaded):
    //    manual installHandleAndRestart MUST call downloadAndInstall, NEVER bare install().
    const events1: string[] = [];
    let freshDownloaded1 = false;
    const freshWithCombined: PendingUpdateHandle = {
      version: "0.20.1",
      downloadAndInstall: async () => {
        events1.push("downloadAndInstall");
        freshDownloaded1 = true;
      },
      install: async () => {
        events1.push("install");
        if (!freshDownloaded1) {
          throw new Error("bare install called on undownloaded handle");
        }
      },
    };
    expect(
      await useUpdateStore
        .getState()
        .installHandleAndRestart(freshWithCombined, fakeRelaunch),
    ).toBe(true);
    expect(events1).toEqual(["downloadAndInstall"]);
    expect(relaunchCount).toBe(1);

    // 2) Fresh handle exposing download + install (no downloadAndInstall):
    //    manual installHandleAndRestart MUST call download() first, then install().
    const events2: string[] = [];
    let freshDownloaded2 = false;
    const freshWithSplit: PendingUpdateHandle = {
      version: "0.20.1",
      download: async () => {
        events2.push("download");
        freshDownloaded2 = true;
      },
      install: async () => {
        events2.push("install");
        if (!freshDownloaded2) {
          throw new Error("bare install called before download()");
        }
      },
    };
    expect(
      await useUpdateStore
        .getState()
        .installHandleAndRestart(freshWithSplit, fakeRelaunch),
    ).toBe(true);
    expect(events2).toEqual(["download", "install"]);
    expect(relaunchCount).toBe(2);

    // 3) Fresh handle exposing ONLY bare install() (never registered via setReady):
    //    refused even if caller attempts to pass { alreadyDownloaded: true }.
    let bareCalls = 0;
    const freshBareOnly: PendingUpdateHandle = {
      version: "0.20.1",
      install: async () => {
        bareCalls += 1;
      },
    };
    expect(
      await useUpdateStore
        .getState()
        .installHandleAndRestart(freshBareOnly, fakeRelaunch, undefined, {
          alreadyDownloaded: true,
        }),
    ).toBe(false);
    expect(bareCalls).toBe(0);
    expect(relaunchCount).toBe(2);

    // 4) Background-downloaded handle passed through runGuardedUpdateCheck -> setReady -> installAndRestart:
    //    calls install() on THAT exact downloaded handle, and refuses if swapped with an unrelated fresh handle.
    const bgEvents: string[] = [];
    let bgDownloaded = false;
    const downloadedHandle: PendingUpdateHandle = {
      version: "0.20.1",
      download: async () => {
        bgEvents.push("download");
        bgDownloaded = true;
      },
      install: async () => {
        bgEvents.push("install");
        if (!bgDownloaded) {
          throw new Error("install called on undownloaded background handle");
        }
      },
    };
    const outcome = await runGuardedUpdateCheck({
      check: async () => downloadedHandle,
      isPortable: async () => false,
    });
    expect(outcome).toEqual({ status: "ready", version: "0.20.1" });
    expect(bgEvents).toEqual(["download"]);

    // Swapping pendingUpdate in state with an unrelated fresh handle must refuse installAndRestart.
    const swappedFresh: PendingUpdateHandle = {
      version: "0.20.1",
      install: async () => {
        throw new Error("swapped fresh handle must never run bare install()");
      },
    };
    useUpdateStore.setState({ pendingUpdate: swappedFresh });
    expect(
      await useUpdateStore.getState().installAndRestart(fakeRelaunch),
    ).toBe(false);
    expect(relaunchCount).toBe(2);

    // Restoring the exact downloadedHandle allows installAndRestart() to run install() and relaunch.
    useUpdateStore.setState({ pendingUpdate: downloadedHandle });
    expect(
      await useUpdateStore.getState().installAndRestart(fakeRelaunch),
    ).toBe(true);
    expect(bgEvents).toEqual(["download", "install"]);
    expect(relaunchCount).toBe(3);
  });

  test("AC-A13b: fresh manual handle uses downloadAndInstall (install 0x), downloaded handle uses install (download 0x again), and rejected install keeps ready notice with relaunch 0x", async () => {
    let relaunchCalls = 0;
    const fakeRelaunch = async () => {
      relaunchCalls += 1;
    };

    // (1) Fresh manual handle whose install() rejects unless download() ran on that same object:
    let manualDownloadAndInstall = 0;
    let manualInstall = 0;
    let manualDownloaded = false;
    const manualHandle: PendingUpdateHandle = {
      version: "0.20.2",
      download: async () => {
        manualDownloaded = true;
      },
      downloadAndInstall: async () => {
        manualDownloadAndInstall += 1;
        manualDownloaded = true;
      },
      install: async () => {
        manualInstall += 1;
        if (!manualDownloaded) {
          throw new Error("install called before download on fresh handle");
        }
      },
    };
    expect(
      await useUpdateStore
        .getState()
        .installHandleAndRestart(manualHandle, fakeRelaunch),
    ).toBe(true);
    expect(manualDownloadAndInstall).toBe(1);
    expect(manualInstall).toBe(0);
    expect(relaunchCalls).toBe(1);

    // (2) Store's downloaded handle: install() 1x, download() NOT called a second time, relaunch 1x.
    let bgDownloads = 0;
    let bgInstalls = 0;
    let bgDownloaded = false;
    const bgHandle: PendingUpdateHandle = {
      version: "0.20.2",
      download: async () => {
        bgDownloads += 1;
        bgDownloaded = true;
      },
      install: async () => {
        bgInstalls += 1;
        if (!bgDownloaded) {
          throw new Error("install called before download");
        }
      },
    };
    const readyOutcome = await runGuardedUpdateCheck({
      check: async () => bgHandle,
      isPortable: async () => false,
    });
    expect(readyOutcome).toEqual({ status: "ready", version: "0.20.2" });
    expect(bgDownloads).toBe(1);
    expect(
      await useUpdateStore.getState().installAndRestart(fakeRelaunch),
    ).toBe(true);
    expect(bgDownloads).toBe(1);
    expect(bgInstalls).toBe(1);
    expect(relaunchCalls).toBe(2);

    // (3) Failing install rejects -> relaunch 0x, notice d (readyVersion) stays set.
    let failDownloaded = false;
    const failingHandle: PendingUpdateHandle = {
      version: "0.20.3",
      download: async () => {
        failDownloaded = true;
      },
      install: async () => {
        if (!failDownloaded) throw new Error("not downloaded");
        throw new Error("install failed");
      },
    };
    await runGuardedUpdateCheck({
      check: async () => failingHandle,
      isPortable: async () => false,
    });
    expect(useUpdateStore.getState().readyVersion).toBe("0.20.3");
    expect(
      await useUpdateStore.getState().installAndRestart(fakeRelaunch),
    ).toBe(false);
    expect(relaunchCalls).toBe(2);
    expect(useUpdateStore.getState().readyVersion).toBe("0.20.3");
  });

  test("AC-A13c: disabling updates while a download is running drops the result when it returns (not published, close once, install 0x, no second check)", async () => {
    let resolveDownload!: () => void;
    const downloadPromise = new Promise<void>((resolve) => {
      resolveDownload = resolve;
    });
    let markStarted!: () => void;
    const startedPromise = new Promise<void>((resolve) => {
      markStarted = resolve;
    });

    let closeCalls = 0;
    let installCalls = 0;
    let checkCalls = 0;

    const run = runGuardedUpdateCheck({
      check: async () => {
        checkCalls += 1;
        return {
          version: "0.20.2",
          download: async () => {
            markStarted();
            await downloadPromise;
          },
          install: async () => {
            installCalls += 1;
          },
          close: async () => {
            closeCalls += 1;
          },
        };
      },
      isPortable: async () => false,
    });

    await startedPromise;

    // Disable updates while the running download is still pending:
    // nothing is published and the handle is closed once when the download returns.
    useUpdateStore.getState().setUpdateChecksEnabled(false);
    expect(useUpdateStore.getState().readyVersion).toBeNull();
    expect(useUpdateStore.getState().pendingUpdate).toBeNull();

    // Let the running download return -> result dropped (not published), handle closed once.
    resolveDownload();

    const outcome = await run;
    expect(outcome).toEqual({ status: "aborted" });
    expect(useUpdateStore.getState().readyVersion).toBeNull();
    expect(useUpdateStore.getState().pendingUpdate).toBeNull();
    expect(closeCalls).toBe(1);
    expect(installCalls).toBe(0);

    // No second check() starts while update checks remain disabled.
    const secondOutcome = await runGuardedUpdateCheck({
      check: async () => {
        checkCalls += 1;
        return null;
      },
      isPortable: async () => false,
    });
    expect(secondOutcome).toEqual({ status: "skipped" });
    expect(checkCalls).toBe(1);
  });
});
