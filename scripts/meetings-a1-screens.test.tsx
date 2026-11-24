import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import {
  ConsentDialog,
  MeetingFloatingPill,
  OfferPanel,
  shouldHideMeetingPill,
} from "../src/components/meetings/MeetingEntryPanels";
import {
  MEETING_WORDS_STORAGE_KEY,
  normalizeMeetingWordList,
  RecordMeetingSection,
} from "../src/components/meetings/RecordMeetingPage";
import { resolveTranscriptSpeakerLabel } from "../src/components/meetings/LiveTranscriptSection";

const i18n = i18next.createInstance();
void i18n
  .use(initReactI18next)
  .init({ lng: "en", resources: { en: { translation: en } } });
const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

describe("A1 Meetings screens (Page 2 + Frames 1-4 + V-M10 + 6b-3/6b-4)", () => {
  test("renders Page 2 Record a meeting without held Coming up calendar and keeps meeting words separate", () => {
    expect(MEETING_WORDS_STORAGE_KEY).toBe("coco_voice.meeting_custom_words");
    expect(normalizeMeetingWordList(["Parakeet"], "parakeet")).toEqual([
      "Parakeet",
    ]);
    expect(normalizeMeetingWordList(["Parakeet"], "Tauri")).toEqual([
      "Parakeet",
      "Tauri",
    ]);

    const html = wrap(
      <RecordMeetingSection
        offerEnabled={true}
        offer={null}
        consentOpen={false}
        status={{
          state: "idle",
          elapsed_secs: 0,
          meeting_id: null,
          mic_name: null,
          warnings: [],
        }}
        segments={[]}
        meetingWords={["Parakeet", "Dana"]}
        onToggleOffer={() => {}}
        onOpenConsent={() => {}}
        onConfirmStart={() => {}}
        onCancelConsent={() => {}}
        onDismissOffer={() => {}}
        onStopMeeting={() => {}}
        onAddMeetingWord={() => {}}
        onRemoveMeetingWord={() => {}}
      />,
    );
    expect(html).toContain("Start now");
    expect(html).toContain("Offer to start notes");
    expect(html).toContain("Never records on its own.");
    expect(html).not.toContain("Meeting words and names");
    expect(html).toContain(
      "Recording a meeting never stops dictation. Your shortcut keeps working.",
    );
    expect(html).toContain("Coming up");
    expect(en.tray.startNotes).toBe("Start notes...");
  });

  test("renders Frames 1-4 (Offer, Consent, Pill gate, Live Me/Others transcript, and shortcut failure)", () => {
    const offerHtml = wrap(
      <OfferPanel
        offer={{
          bundleId: "us.zoom.xos",
          app: "zoom",
          inCall: true,
          startsCapture: false,
        }}
        onStartNotes={() => {}}
        onDismiss={() => {}}
      />,
    );
    expect(offerHtml).toContain("Start notes?");
    expect(offerHtml).toContain("Zoom is in a call.");
    expect(offerHtml).toContain("Not recording");

    const consentHtml = wrap(
      <ConsentDialog
        open={true}
        onConfirmStart={() => {}}
        onCancel={() => {}}
      />,
    );
    expect(consentHtml).toContain("Tell participants first");
    expect(consentHtml).toContain("fixed inset-0");
    expect(consentHtml).toContain("Copy announcement");
    expect(consentHtml).toContain("I&#x27;ve told everyone");
    expect(consentHtml).toContain("optional");
    expect(consentHtml).toContain("Start recording");
    expect(consentHtml.indexOf("Copy announcement")).toBeLessThan(
      consentHtml.indexOf("taking notes with an app"),
    );
    expect(consentHtml.indexOf("Start recording")).toBeLessThan(
      consentHtml.indexOf(">Cancel<"),
    );
    expect(consentHtml.indexOf(">Cancel<")).toBeLessThan(
      consentHtml.indexOf("organisation&#x27;s rules"),
    );

    expect(
      resolveTranscriptSpeakerLabel(
        {
          meeting_id: 7,
          channel: "others",
          start_ms: 1000,
          end_ms: 2000,
          text: "Hi",
          speaker_label: "Speaker 1",
        },
        { me: "Me", others: "Others" },
        false,
      ),
    ).toBe("Others");
    expect(
      resolveTranscriptSpeakerLabel(
        {
          meeting_id: 7,
          channel: "others",
          start_ms: 1000,
          end_ms: 2000,
          text: "Hi",
          speaker_label: "Dana",
        },
        { me: "Me", others: "Others" },
        false,
      ),
    ).toBe("Dana");

    expect(shouldHideMeetingPill("dictation_hide", "recording")).toBe(false);
    expect(shouldHideMeetingPill("dictation_hide", "paused")).toBe(false);
    expect(shouldHideMeetingPill("stop", "recording")).toBe(true);

    const pillHtml = wrap(
      <MeetingFloatingPill elapsedSecs={768} onStop={() => {}} />,
    );
    expect(pillHtml).toContain("12:48");
    expect(pillHtml).toContain("Stop");

    const liveHtml = wrap(
      <RecordMeetingSection
        offerEnabled={true}
        offer={null}
        consentOpen={false}
        status={{
          state: "recording",
          elapsed_secs: 185,
          meeting_id: 7,
          mic_name: "MacBook Pro Microphone",
          warnings: ["system_audio_silent"],
          shortcut_failed: true,
        }}
        segments={[
          {
            meeting_id: 7,
            channel: "me",
            start_ms: 12_000,
            end_ms: 15_000,
            text: "Let us review the release checklist.",
          },
          {
            meeting_id: 7,
            channel: "others",
            start_ms: 18_000,
            end_ms: 21_000,
            text: "Sounds good, starting with onboarding.",
          },
        ]}
        meetingWords={[]}
        onToggleOffer={() => {}}
        onOpenConsent={() => {}}
        onConfirmStart={() => {}}
        onCancelConsent={() => {}}
        onDismissOffer={() => {}}
        onStopMeeting={() => {}}
        onAddMeetingWord={() => {}}
        onRemoveMeetingWord={() => {}}
      />,
    );
    expect(liveHtml).toContain("Me and Others");
    expect(liveHtml).toContain("Auto-scrolling");
    expect(liveHtml).toContain("Meeting shortcut could not be registered.");
    expect(liveHtml).toContain("Dictation still works during a meeting.");
  });
});
