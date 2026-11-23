import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { LiveTranscriptSection } from "../src/components/meetings/LiveTranscriptSection";

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  interpolation: { escapeValue: false },
  resources: { en: { translation: en } },
});

const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

const SAMPLE_SEGMENTS = [
  {
    id: 1,
    meeting_id: 101,
    channel: "me" as const,
    start_ms: 10_000,
    end_ms: 14_000,
    text: "Ship desktop release on Friday.",
  },
];

describe("W34: finished meetings hide live-only lines; live recording view renders all three", () => {
  test("finished meeting renders none of Auto-scrolling, headphones hint, or dictation line", () => {
    const finishedHtml = wrap(
      <LiveTranscriptSection
        status={{
          state: "idle",
          elapsed_secs: 1800,
          meeting_id: 101,
          mic_name: null,
          warnings: [],
        }}
        segments={SAMPLE_SEGMENTS}
        dictationShortcut="Option+Space"
        onStop={() => {}}
      />,
    );

    expect(finishedHtml).not.toContain("Auto-scrolling");
    expect(finishedHtml).not.toContain("Use headphones.");
    expect(finishedHtml).not.toContain("Dictation still works");
    expect(finishedHtml).not.toContain("No hidden or background mode.");
  });

  test("live recording view renders Auto-scrolling, headphones hint, and dictation shortcut line", () => {
    const liveHtml = wrap(
      <LiveTranscriptSection
        status={{
          state: "recording",
          elapsed_secs: 768,
          meeting_id: 101,
          mic_name: "MacBook Pro Microphone",
          warnings: [],
        }}
        segments={SAMPLE_SEGMENTS}
        dictationShortcut="Option+Space"
        onStop={() => {}}
      />,
    );

    expect(liveHtml).toContain("Auto-scrolling");
    expect(liveHtml).toContain(
      "Use headphones. On speakers, the call can leak into your microphone.",
    );
    expect(liveHtml).toContain("Dictation still works (hold Option+Space).");
    expect(liveHtml).toContain(
      "No hidden or background mode. The pill stays while recording.",
    );
  });
});
