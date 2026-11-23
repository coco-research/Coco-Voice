import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { resolveTopHomeNotice } from "../src/components/home/HomeHealthNotice";
import { HomeViewSection } from "../src/components/home/HomeView";
import AccessibilityStaleNotice, {
  ModelDownloadFailedBanner,
  TryItStepCard,
} from "../src/components/onboarding/AccessibilityStaleNotice";
import {
  FirstRunStaleAxCard,
  FirstRunWelcomeCard,
} from "../src/components/onboarding/AccessibilityOnboarding";
import {
  buildTrayMenuEntries,
  shouldShowWhatsNewOnce,
  TrayMenuSurface,
  UpdateRollbackNotice,
} from "../src/components/whats-new/UpdateAndTraySurfaces";
import { WhatsNewModal } from "../src/components/whats-new/WhatsNewModal";
import { accessibilityStatus } from "../src/lib/utils/accessibilityStatus";
import {
  RecordingPillShowcase,
  RecordingPillStateView,
} from "../src/overlay/RecordingPillStates";

const i18n = i18next.createInstance();
void i18n
  .use(initReactI18next)
  .init({ lng: "en", resources: { en: { translation: en } } });
const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

describe("Group 5 (V-R1..V-R5 + G8) UI components", () => {
  test("V-R1 renders all 6 recording pill states (Listening, streaming words, Working, Sent to app, On the clipboard, Words kept press paste)", () => {
    expect(wrap(<RecordingPillStateView kind="listening" />)).toContain(
      "Listening",
    );
    expect(
      wrap(
        <RecordingPillStateView
          kind="streaming"
          streamingWords="can we move the review to…"
        />,
      ),
    ).toContain("can we move the review to…");
    expect(wrap(<RecordingPillStateView kind="working" />)).toContain(
      "Working",
    );
    expect(
      wrap(<RecordingPillStateView kind="sent" targetApp="Slack" />),
    ).toContain("Sent to Slack");
    expect(wrap(<RecordingPillStateView kind="clipboard" />)).toContain(
      "On the clipboard",
    );
    expect(wrap(<RecordingPillStateView kind="kept_press_paste" />)).toContain(
      "Words kept · press ⌘V",
    );
  });

  test("V-R2 resolves stale Accessibility, renders 3-step fix + Continue without typing, download failure cause + Try again/Skip, and Try it step", () => {
    expect(accessibilityStatus("needed", "ok")).toBe("needed");
    expect(accessibilityStatus("granted", "ok")).toBe("granted");
    expect(accessibilityStatus("granted", "failed")).toBe("stale");
    expect(accessibilityStatus("granted", "not-run")).toBe("needed");

    const staleHtml = wrap(
      <AccessibilityStaleNotice
        onCheckAgain={() => {}}
        onContinueWithoutTyping={() => {}}
      />,
    );
    expect(staleHtml).toContain("Check again");
    expect(staleHtml).toContain("Continue without typing");
    expect(staleHtml).toContain("Privacy &amp; Security");

    const dlFailHtml = wrap(
      <ModelDownloadFailedBanner
        cause="disk_space"
        onRetry={() => {}}
        onSkip={() => {}}
      />,
    );
    expect(dlFailHtml).toContain("not enough disk space");
    expect(dlFailHtml).toContain("Try again");
    expect(dlFailHtml).toContain("Skip for now");

    const tryItHtml = wrap(<TryItStepCard shortcut="fn" onDone={() => {}} />);
    expect(tryItHtml).toContain("Monday, scratch that, Tuesday");
    expect(tryItHtml).toContain("Tuesday.");
    expect(tryItHtml).toContain("Done");
  });

  test("V-R3 + G8 shows at most ONE highest-priority Home notice above the two cards and suppresses G8 once shown", () => {
    expect(
      resolveTopHomeNotice({
        staleAccessibility: true,
        shortcutUnset: true,
        typingOff: true,
        cleanupEnabledWithoutLocalModel: true,
        updateReadyVersion: "0.10.2",
      })?.kind,
    ).toBe("stale_accessibility");

    expect(
      resolveTopHomeNotice({
        cleanupEnabledWithoutLocalModel: true,
        cleanupNoticeAlreadyShown: false,
      })?.kind,
    ).toBe("cleanup_model_missing");

    expect(
      resolveTopHomeNotice({
        cleanupEnabledWithoutLocalModel: true,
        cleanupNoticeAlreadyShown: true,
      }),
    ).toBeNull();

    const homeHtml = wrap(
      <HomeViewSection
        shortcut="fn"
        meetings={[]}
        dictations={[]}
        health={{
          cleanupEnabledWithoutLocalModel: true,
          cleanupNoticeAlreadyShown: false,
        }}
        onStartRecording={() => {}}
        onNavigate={() => {}}
      />,
    );
    expect(homeHtml).toContain("Clean-up skipped: local model not downloaded.");
    expect(homeHtml).toContain("Open Models");
  });

  test("V-R4 tray menu surfaces Paste last take + Copy last take and V-R5 shows What's new once + update rollback notice", () => {
    const entries = buildTrayMenuEntries((k) => k, true);
    expect(entries.map((e) => e.id)).toEqual([
      "start_meeting",
      "paste_last_take",
      "copy_last_take",
      "open_app",
      "check_updates",
      "quit",
    ]);
    const trayHtml = wrap(<TrayMenuSurface />);
    expect(trayHtml).toContain("Paste last take");
    expect(trayHtml).toContain("Copy last take");
    expect(trayHtml).toContain("Start recording a meeting");

    expect(shouldShowWhatsNewOnce("0.10.2", "0.10.1")).toBe(true);
    expect(shouldShowWhatsNewOnce("0.10.2", "0.10.2")).toBe(false);

    const rollbackHtml = wrap(
      <UpdateRollbackNotice failedVersion="0.10.2" currentVersion="0.10.1" />,
    );
    expect(rollbackHtml).toContain("If an update fails");
    expect(rollbackHtml).toContain(
      "0.10.2 did not start, so Coco Voice went back to 0.10.1. Your settings and history are unchanged.",
    );
    expect(rollbackHtml).toContain("Report it");

    const whatsNewHtml = wrap(
      <WhatsNewModal
        note={{ version: "0.20.0", markdown: "" }}
        open={true}
        onDismiss={() => {}}
        rolledBackFromVersion="0.20.1"
      />,
    ).replaceAll("&#x27;", "'");
    expect(whatsNewHtml).toContain("What's new in 0.20.0");
    expect(whatsNewHtml).toContain("Meetings, and a new layout");
    expect(whatsNewHtml).toContain(
      "Record a meeting from Home. Meetings and dictation now have their own sections.",
    );
    expect(whatsNewHtml).toContain("Close");
    expect(whatsNewHtml).toContain(
      "0.20.1 did not start, so Coco Voice went back to 0.20.0. Your settings and history are unchanged.",
    );
    expect(whatsNewHtml).toContain("Report it");
    expect(whatsNewHtml).not.toContain("If an update fails");
  });

  test("v0.2.2 First Run 1, First Run 2, and 6-state Pill Showcase render approved wording", () => {
    const fr1Html = wrap(
      <FirstRunWelcomeCard
        microphoneStatus="granted"
        accessibilityStatus="needed"
        onGrantMicrophone={() => {}}
        onGrantAccessibility={() => {}}
        onRetry={() => {}}
        onContinueWithoutTyping={() => {}}
      />,
    );
    expect(fr1Html).toContain("Welcome");
    expect(fr1Html).toContain(
      "Coco Voice turns your voice into text, on this Mac",
    );
    expect(fr1Html).toContain(
      "Two permissions, then one model download. About a minute.",
    );
    expect(fr1Html).toContain("Allowed");
    expect(fr1Html).toContain("Open System Settings");
    expect(fr1Html).toContain("Continue without typing");
    expect(fr1Html).toContain(
      "Your words go to the clipboard instead. You can turn typing on later.",
    );

    const fr2Html = wrap(
      <FirstRunStaleAxCard
        onOpenAccessibility={() => {}}
        onContinueWithoutTyping={() => {}}
      />,
    );
    expect(fr2Html).toContain(
      "Coco Voice is switched on, but macOS still blocks it",
    );
    expect(fr2Html).toContain(
      "1. In System Settings › Privacy &amp; Security, open the list of apps allowed to control your Mac, select Coco Voice and click minus (−).",
    );
    expect(fr2Html).toContain(
      "2. Click plus (+), choose Applications → Coco Voice, switch it on.",
    );
    expect(fr2Html).toContain(
      "3. Come back here. We check again by ourselves.",
    );
    expect(fr2Html).toContain("Open System Settings");
    expect(fr2Html).toContain("Continue without typing");

    const showcaseHtml = wrap(<RecordingPillShowcase />);
    expect(showcaseHtml).toContain("Listening");
    expect(showcaseHtml).toContain("can we move the review to…");
    expect(showcaseHtml).toContain("Working");
    expect(showcaseHtml).toContain("Sent to Mail");
    expect(showcaseHtml).toContain("On the clipboard");
    expect(showcaseHtml).toContain("Words kept · press ⌘V");
  });
});
