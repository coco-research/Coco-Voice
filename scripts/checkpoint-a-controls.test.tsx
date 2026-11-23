import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { ModelInfo } from "../src/bindings";
import { resolveInitialOnboardingStep } from "../src/components/onboarding/onboardingStep";
import {
  FirstRunStaleAxCard,
  FirstRunWelcomeCard,
  persistContinueWithoutTyping,
  probeAccessibilityStatus,
  STALE_AX_RECHECK_INTERVAL_MS,
} from "../src/components/onboarding/AccessibilityOnboarding";
import {
  applyTryItSelfCorrection,
  evaluateTryItTakeResult,
  TryItStepCard,
} from "../src/components/onboarding/AccessibilityStaleNotice";
import {
  BYTES_PER_MB,
  bytesToWholeMb,
  computeRollingEtaSeconds,
  FIRST_RUN_DEFAULT_MODEL_ID,
  FirstRunModelDownloadView,
  pickDefaultFirstRunModel,
} from "../src/components/onboarding/FirstRunModelDownloadView";
import {
  cleanupPartialOnDownloadFailure,
  resolveOnboardingShortcut,
} from "../src/components/onboarding/Onboarding";
import {
  HomeHealthNoticesSection,
  resolveTopHomeNotice,
} from "../src/components/home/HomeHealthNotice";
import {
  DEFAULT_TYPING_PASTE_METHOD,
  getHomeGreeting,
  getHomeGreetingBand,
  HOME_MOUNT_BUDGET_MS,
  HomeViewSection,
  restartAppForUpdate,
  turnTypingOnAction,
} from "../src/components/home/HomeView";
import { shouldRunUpdateCheck } from "../src/components/update-checker/UpdateChecker";
import { useUpdateStore } from "../src/stores/updateStore";
import {
  buildRollbackReportUrl,
  compareSemver,
  evaluateClientUpdateNotice,
  openRollbackReport,
} from "../src/components/whats-new/UpdateAndTraySurfaces";
import { WhatsNewModal } from "../src/components/whats-new/WhatsNewModal";

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>{node}</I18nextProvider>,
  ).replaceAll("&#x27;", "'");

function makeModel(overrides: Partial<ModelInfo>): ModelInfo {
  return {
    id: "model-a",
    name: "Parakeet V3",
    description: "Fast speech model",
    filename: "parakeet-v3.gguf",
    source: {
      HuggingFace: {
        repo_id: "org/parakeet",
        revision: "main",
        sha256: "abc",
      },
    },
    size_mb: 478,
    is_downloaded: false,
    is_downloading: false,
    partial_size: 0,
    is_directory: false,
    engine_type: "Parakeet",
    accuracy_score: 0.9,
    speed_score: 0.9,
    supports_translation: false,
    is_recommended: false,
    measured_best: null,
    supported_languages: ["en"],
    supports_language_selection: true,
    is_custom: false,
    supports_streaming: true,
    supports_language_detection: true,
    ...overrides,
  } as ModelInfo;
}

describe("Checkpoint A / Plan A (Rows 1-7 & AC-A2..AC-A17)", () => {
  test("AC-A1/A3/A17: Welcome & permissions states, Continue without typing persists paste_method=none, and returning users skip first run", async () => {
    const htmlAllowed = wrap(
      <FirstRunWelcomeCard
        microphoneStatus="granted"
        accessibilityStatus="needed"
        onGrantMicrophone={() => {}}
        onGrantAccessibility={() => {}}
        onRetry={() => {}}
        onContinueWithoutTyping={() => {}}
      />,
    );
    expect(htmlAllowed).toContain("Welcome");
    expect(htmlAllowed).toContain("pt-6 px-7");
    expect(htmlAllowed).not.toContain("max-w-[560px]");
    expect(htmlAllowed).not.toContain("shadow-lg");
    expect(htmlAllowed).toContain(
      "Coco Voice turns your voice into text, on this Mac",
    );
    expect(htmlAllowed).toContain(
      "Two permissions, then one model download. About a minute.",
    );
    expect(htmlAllowed).toContain("Microphone");
    expect(htmlAllowed).toContain("To hear you");
    expect(htmlAllowed).toContain("Allowed");
    expect(htmlAllowed).toContain("Accessibility");
    expect(htmlAllowed).toContain("To type your words into other apps");
    expect(htmlAllowed).toContain("Open System Settings");
    expect(htmlAllowed).toContain("Continue without typing");
    expect(htmlAllowed).toContain(
      "Your words go to the clipboard instead. You can turn typing on later.",
    );

    const calls: string[] = [];
    await persistContinueWithoutTyping(
      async (method) => {
        calls.push(`paste:${method}`);
      },
      async () => {
        calls.push("refresh");
      },
      async () => {
        calls.push("finish");
      },
    );
    expect(calls).toEqual(["paste:none", "refresh", "finish"]);

    // AC-A17: returning user with permissions goes straight to Home; missing permission sees only A1/A2
    expect(
      resolveInitialOnboardingStep({
        hasModels: true,
        onboardingCompleted: true,
        missingPermission: false,
      }),
    ).toEqual({ step: "done", isReturningUser: true });
    expect(
      resolveInitialOnboardingStep({
        hasModels: true,
        onboardingCompleted: true,
        missingPermission: true,
      }),
    ).toEqual({ step: "accessibility", isReturningUser: true });
    expect(
      resolveInitialOnboardingStep({
        hasModels: false,
        onboardingCompleted: false,
        missingPermission: true,
      }),
    ).toEqual({ step: "accessibility", isReturningUser: false });
  });

  test("AC-A2: First run 2 detects stale Accessibility, renders 3-step fix, and polls at 1500ms", async () => {
    expect(STALE_AX_RECHECK_INTERVAL_MS).toBe(1500);

    expect(
      await probeAccessibilityStatus(
        async () => "granted",
        async () => "failed",
        async () => false,
      ),
    ).toBe("stale");
    expect(
      await probeAccessibilityStatus(
        async () => "granted",
        async () => "ok",
        async () => true,
      ),
    ).toBe("stale");
    expect(
      await probeAccessibilityStatus(
        async () => "granted",
        async () => "ok",
        async () => false,
      ),
    ).toBe("granted");

    const html = wrap(
      <FirstRunStaleAxCard
        onOpenAccessibility={() => {}}
        onContinueWithoutTyping={() => {}}
      />,
    );
    expect(html).toContain("System Settings");
    expect(html).toContain("pt-6 px-7");
    expect(html).not.toContain("max-w-[560px]");
    expect(html).not.toContain("shadow-lg");
    expect(html).toContain(
      "Coco Voice is switched on, but macOS still blocks it",
    );
    expect(html).toContain(
      "1. In System Settings › Privacy &amp; Security, open the list of apps allowed to control your Mac, select Coco Voice and click minus (−).",
    );
    expect(html).toContain(
      "2. Click plus (+), choose Applications → Coco Voice, switch it on.",
    );
    expect(html).toContain("3. Come back here. We check again by ourselves.");
    expect(html).toContain("Open System Settings");
    expect(html).toContain("Continue without typing");
  });

  test("AC-A4/A5/A6/A7: Single default parakeet-tdt-0.6b-v3 download uses 1MB=1,000,000B, 5s rolling ETA hidden <2s, partial cleanup on failure, and clean cancel layout", async () => {
    expect(BYTES_PER_MB).toBe(1_000_000);
    expect(bytesToWholeMb(312_000_000)).toBe(312);
    expect(bytesToWholeMb(478_000_000)).toBe(478);

    const parakeetV3 = makeModel({
      id: FIRST_RUN_DEFAULT_MODEL_ID,
      name: "Parakeet V3",
      size_mb: 478,
    });
    const otherBest = makeModel({
      id: "other-best",
      name: "Other Best",
      is_recommended: true,
      measured_best: {
        clip_set: "wer-dev-40",
        words_wrong_per_100: 9.2,
        speed_x_realtime: 24.8,
        peak_memory_mb: 1007,
        models_compared: 2,
      },
    });
    expect(pickDefaultFirstRunModel([otherBest, parakeetV3])?.id).toBe(
      FIRST_RUN_DEFAULT_MODEL_ID,
    );

    // Hidden before first 2 seconds
    expect(
      computeRollingEtaSeconds(
        [
          { atMs: 0, bytes: 0 },
          { atMs: 1500, bytes: 12_000_000 },
        ],
        478_000_000,
        1500,
        0,
      ),
    ).toBeNull();

    // AC-A4: 312,000,000 of 478,000,000 arriving every 0.5 s at 8 MB/s -> about 20 seconds left
    expect(
      computeRollingEtaSeconds(
        [
          { atMs: 2500, bytes: 296_000_000 },
          { atMs: 3000, bytes: 300_000_000 },
          { atMs: 3500, bytes: 304_000_000 },
          { atMs: 4000, bytes: 308_000_000 },
          { atMs: 4500, bytes: 312_000_000 },
        ],
        478_000_000,
        4500,
        0,
      ),
    ).toBe(20);

    // Under 5s rounds up to 5s ("about 5 seconds left")
    expect(
      computeRollingEtaSeconds(
        [
          { atMs: 2500, bytes: 454_000_000 },
          { atMs: 4500, bytes: 470_000_000 },
        ],
        478_000_000,
        4500,
        0,
      ),
    ).toBe(5);

    const progressHtml = wrap(
      <FirstRunModelDownloadView
        model={parakeetV3}
        downloadedMb={312}
        totalMb={478}
        percentage={65}
        secondsLeft={20}
        phase="downloading"
        onCancel={() => {}}
        onTryAgain={() => {}}
        onSkipForNow={() => {}}
      />,
    );
    expect(progressHtml).toContain("Speech model");
    expect(progressHtml).toContain("Downloading Parakeet V3");
    expect(progressHtml).toContain("Parakeet V3 · 478 MB");
    expect(progressHtml).toContain("312 MB of 478 MB · about 20 seconds left");
    expect(progressHtml).toContain('role="progressbar"');
    expect(progressHtml).toContain("Cancel");

    const failHtml = wrap(
      <FirstRunModelDownloadView
        model={parakeetV3}
        downloadedMb={312}
        totalMb={478}
        percentage={65}
        secondsLeft={null}
        phase="failed"
        failureCause="network"
        failedAtMb={312}
        onCancel={() => {}}
        onTryAgain={() => {}}
        onSkipForNow={() => {}}
      />,
    );
    expect(failHtml).toContain("pt-6 px-7");
    expect(failHtml).not.toContain("max-w-[560px]");
    expect(failHtml).not.toContain("shadow-lg");
    expect(failHtml).toContain("Parakeet V3 · 478 MB");
    expect(failHtml).not.toContain("312 MB of 478 MB");
    expect(failHtml).not.toContain('role="progressbar"');
    expect(failHtml).not.toContain(">Cancel<");
    expect(failHtml).toContain(
      "The download stopped: the connection dropped at 312 MB. Nothing was saved.",
    );
    expect(failHtml).toContain("Try again");
    expect(failHtml).toContain("Skip for now");
    expect(failHtml.indexOf("Try again")).toBeLessThan(
      failHtml.indexOf("Skip for now"),
    );
    expect(failHtml).not.toContain("If it fails");

    // AC-A6: cancelled state shows only Try again & Skip for now, no invented copy
    const cancelHtml = wrap(
      <FirstRunModelDownloadView
        model={parakeetV3}
        downloadedMb={100}
        totalMb={478}
        percentage={21}
        secondsLeft={null}
        phase="cancelled"
        onCancel={() => {}}
        onTryAgain={() => {}}
        onSkipForNow={() => {}}
      />,
    );
    expect(cancelHtml).toContain("Try again");
    expect(cancelHtml).toContain("Skip for now");
    expect(cancelHtml).not.toContain("Download cancelled");

    // AC-A7: failure cleanup removes partial via deleteModel
    const deleted: string[] = [];
    await cleanupPartialOnDownloadFailure(
      FIRST_RUN_DEFAULT_MODEL_ID,
      async (id) => {
        deleted.push(id);
      },
    );
    expect(deleted).toEqual([FIRST_RUN_DEFAULT_MODEL_ID]);
  });

  test("AC-A8/A9: Try it binds real shortcut, calls evaluate_try_it_take, shows dropped sentence only when non-null", async () => {
    expect(resolveOnboardingShortcut("Option+Space", "Shortcut not set")).toBe(
      "Option+Space",
    );
    expect(applyTryItSelfCorrection("Monday, scratch that, Tuesday")).toEqual({
      cleaned: "Tuesday",
      droppedPrefix: "Monday, scratch that",
    });

    expect(
      await evaluateTryItTakeResult(
        "Monday, scratch that, Tuesday",
        "Tuesday",
        async () => ({ dropped: "Monday, scratch that" }),
      ),
    ).toBe("Monday, scratch that");
    expect(
      await evaluateTryItTakeResult(
        "Monday, Tuesday",
        "Monday, Tuesday",
        async () => ({ dropped: null }),
      ),
    ).toBeNull();

    const withDropHtml = wrap(
      <TryItStepCard
        shortcut="Option+Space"
        spokenCleanedText="Tuesday"
        droppedText="Monday, scratch that"
        onDone={() => {}}
      />,
    );
    expect(withDropHtml).toContain(
      "Hold Option+Space and say: &quot;Monday, scratch that, Tuesday&quot;",
    );
    expect(withDropHtml).toContain(
      "Clean-up dropped &quot;Monday, scratch that&quot;. That is how every take is cleaned.",
    );

    const noDropHtml = wrap(
      <TryItStepCard
        shortcut="Option+Space"
        spokenCleanedText="Monday, Tuesday"
        droppedText={null}
        onDone={() => {}}
      />,
    );
    expect(noDropHtml).not.toContain("Clean-up dropped");
    expect(noDropHtml).toContain("Done");
  });

  test("AC-A10 & Part 5: Home renders local time-of-day greeting, hides Meetings when showMeetings=false, and mounts within 300ms budget", () => {
    const morning = new Date(2026, 9, 27, 9, 30);
    const afternoon = new Date(2026, 9, 27, 14, 15);
    const evening = new Date(2026, 9, 27, 20, 45);

    expect(getHomeGreetingBand(morning)).toBe("morning");
    expect(getHomeGreetingBand(afternoon)).toBe("afternoon");
    expect(getHomeGreetingBand(evening)).toBe("evening");
    expect(getHomeGreeting(morning).defaultValue).toBe("Good morning");
    expect(getHomeGreeting(afternoon).defaultValue).toBe("Good afternoon");
    expect(getHomeGreeting(evening).defaultValue).toBe("Good evening");

    const t0 = performance.now();
    const html = wrap(
      <HomeViewSection
        shortcut="fn"
        meetings={[
          {
            id: 1,
            title: "Product sync",
            started_at: 1_700_003_600,
            ended_at: 1_700_005_400,
            notes_ready: true,
          },
        ]}
        dictations={[
          {
            id: 10,
            text: "Can we move the review to Thursday?",
            appName: "Mail",
            time: "9:41",
          },
        ]}
        greetingDate={morning}
        onStartRecording={() => {}}
        onNavigate={() => {}}
      />,
    );
    const elapsedMs = performance.now() - t0;
    expect(elapsedMs).toBeLessThanOrEqual(HOME_MOUNT_BUDGET_MS);
    expect(html).toContain("Good morning");
    expect(html).toContain('data-home-card="meetings"');
    expect(html).toContain('data-home-card="voice"');

    const noMeetingsHtml = wrap(
      <HomeViewSection
        shortcut="fn"
        meetings={[]}
        dictations={[]}
        showMeetings={false}
        greetingDate={evening}
        onStartRecording={() => {}}
        onNavigate={() => {}}
      />,
    );
    expect(noMeetingsHtml).toContain("Good evening");
    expect(noMeetingsHtml).not.toContain('data-home-card="meetings"');
    expect(noMeetingsHtml).toContain('data-home-card="voice"');
  });

  test("AC-A11/A12/A13/A14: Strict a>b>c>d Home notice order, Turn typing on restores default paste method, updateStore installs then relaunches, and update_checks_enabled=false blocks check()", async () => {
    expect(
      resolveTopHomeNotice({
        staleAccessibility: true,
        shortcutUnset: true,
        typingOff: true,
        updateReadyVersion: "0.20.1",
      })?.kind,
    ).toBe("stale_accessibility");
    expect(
      resolveTopHomeNotice({
        shortcutUnset: true,
        typingOff: true,
        updateReadyVersion: "0.20.1",
      })?.kind,
    ).toBe("shortcut_unset");
    expect(
      resolveTopHomeNotice({
        typingOff: true,
        updateReadyVersion: "0.20.1",
      })?.kind,
    ).toBe("typing_off");
    expect(
      resolveTopHomeNotice({
        updateReadyVersion: "0.20.1",
      })?.kind,
    ).toBe("update_ready");

    // AC-A12: Turn typing on restores platform default and requests AX if untrusted
    const typingCalls: string[] = [];
    const turnedOn = await turnTypingOnAction({
      changePasteMethod: async (m) => {
        typingCalls.push(`paste:${m}`);
      },
      refreshSettings: async () => {
        typingCalls.push("refresh");
      },
      isMac: () => true,
      checkAx: async () => false,
      requestAx: async () => {
        typingCalls.push("request_ax");
      },
    });
    expect(turnedOn).toBe(true);
    expect(typingCalls).toEqual([
      `paste:${DEFAULT_TYPING_PASTE_METHOD}`,
      "refresh",
      "request_ax",
    ]);

    // AC-A13: updateStore.readyVersion drives notice d and Restart now calls install then relaunch
    useUpdateStore.getState().setUpdateChecksEnabled(true);
    const sequence: string[] = [];
    useUpdateStore.getState().setReady("0.20.1", {
      version: "0.20.1",
      install: async () => {
        sequence.push("install");
      },
    });
    expect(useUpdateStore.getState().readyVersion).toBe("0.20.1");

    const noticeDHtml = wrap(
      <HomeHealthNoticesSection
        health={{ updateReadyVersion: useUpdateStore.getState().readyVersion }}
        onNavigate={() => {}}
        onRestartNow={() => {}}
      />,
    );
    expect(noticeDHtml).toContain("Update ready: 0.20.1");
    expect(noticeDHtml).toContain("Installs when you restart.");
    expect(noticeDHtml).toContain("Restart now");

    const restarted = await restartAppForUpdate(async () => {
      sequence.push("relaunch");
    });
    expect(restarted).toBe(true);
    expect(sequence).toEqual(["install", "relaunch"]);
    useUpdateStore.getState().clearReady();

    // AC-A14: update_checks_enabled=false prevents check() from ever running
    expect(shouldRunUpdateCheck(false, false)).toBe(false);
    expect(shouldRunUpdateCheck(true, true)).toBe(false);
    expect(shouldRunUpdateCheck(true, false)).toBe(true);
  });

  test("AC-A15 & Part 13: What's new show-once rules and privacy-safe Report it URL", async () => {
    expect(compareSemver("0.20.1", "0.20.0")).toBe(1);
    expect(evaluateClientUpdateNotice("0.20.0", "0.20.0", true)).toEqual({
      kind: "none",
    });
    expect(evaluateClientUpdateNotice("0.20.0", "0.11.0", true)).toEqual({
      kind: "whats_new",
      version: "0.20.0",
    });
    // Downgrade never emits went_back in 0.20 (fixture-only for shot 36b until R2).
    expect(evaluateClientUpdateNotice("0.20.0", "0.20.1", true)).toEqual({
      kind: "none",
    });

    const modalHtml = wrap(
      <WhatsNewModal
        note={{ version: "0.20.0", markdown: "" }}
        open={true}
        onDismiss={() => {}}
        rolledBackFromVersion="0.20.1"
        onReportRollback={() => {}}
      />,
    );
    expect(modalHtml).toContain("What's new in 0.20.0");
    expect(modalHtml).toContain("Meetings, and a new layout");
    expect(modalHtml).toContain(
      "Record a meeting from Home. Meetings and dictation now have their own sections.",
    );
    expect(modalHtml).toContain("Close");
    expect(modalHtml).toContain("Report it");
    expect(modalHtml).not.toContain("If an update fails");

    const openedUrls: string[] = [];
    const url = await openRollbackReport(async (u) => {
      openedUrls.push(u);
    });
    expect(url).toBe(buildRollbackReportUrl());
    expect(url).toBe(
      "https://github.com/coco-research/Coco-Voice/issues/new?template=bug_report.md",
    );
    expect(openedUrls).toEqual([url]);
  });
});
