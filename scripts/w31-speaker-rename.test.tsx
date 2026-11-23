import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type {
  MeetingSegmentView,
  MeetingSpeakerSlot,
} from "../src/bindings.meetings";
import {
  buildSpeakerNameMap,
  SpeakerSplitSection,
} from "../src/components/meetings/SpeakerSplitSection";
import { LiveTranscriptSection } from "../src/components/meetings/LiveTranscriptSection";

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  interpolation: { escapeValue: false },
  resources: { en: { translation: en } },
});

const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

describe("W31 (CX-3, I5): speaker rename displays typed name or Speaker N on others lines, Me on mic lines, and ○ Unnamed", () => {
  test("Speaker 2 named Jordan -> Speaker 2 lines show Jordan, Speaker 1 lines show Speaker 1, mic lines show Me, Speaker 1 row shows ○ Unnamed", () => {
    const slots: MeetingSpeakerSlot[] = [
      { slot: "Speaker 1", custom_name: "", display_name: null },
      { slot: "Speaker 2", custom_name: "Jordan", display_name: "Jordan" },
    ];
    const segments: MeetingSegmentView[] = [
      {
        id: 1,
        meeting_id: 101,
        channel: "others",
        speaker_label: "Speaker 1",
        start_ms: 12_000,
        end_ms: 18_000,
        text: "Export should be one clean Markdown file.",
      },
      {
        id: 2,
        meeting_id: 101,
        channel: "me",
        speaker_label: "Speaker 2",
        start_ms: 24_000,
        end_ms: 30_000,
        text: "Agreed, shipping this week.",
      },
      {
        id: 3,
        meeting_id: 101,
        channel: "others",
        speaker_label: "Speaker 2",
        start_ms: 36_000,
        end_ms: 42_000,
        text: "I will draft the empty-state strings before Thursday.",
      },
    ];

    const speakerHtml = wrap(
      <SpeakerSplitSection
        meetingId={101}
        segments={segments}
        initialSlots={slots}
      />,
    );
    expect(speakerHtml).toContain("Speaker 1");
    expect(speakerHtml).toContain("No name typed for this meeting.");
    expect(speakerHtml).toContain("○");
    expect(speakerHtml).toContain("Unnamed");
    expect(speakerHtml).toContain("Speaker 2");
    expect(speakerHtml).toContain("Renamed in this meeting.");
    expect(speakerHtml).toContain('value="Jordan"');

    const nameMap = buildSpeakerNameMap(slots);
    expect(nameMap).toEqual({ "Speaker 2": "Jordan" });

    const transcriptHtml = wrap(
      <LiveTranscriptSection
        status={{
          state: "idle",
          elapsed_secs: 60,
          meeting_id: 101,
          mic_name: null,
          warnings: [],
        }}
        segments={segments}
        showSpeakerLabels={true}
        speakerNames={nameMap}
        onStop={() => {}}
      />,
    );

    expect(transcriptHtml).toContain(">Speaker 1<");
    expect(transcriptHtml).toContain(">Jordan<");
    expect(transcriptHtml).toContain(">Me<");
    expect(transcriptHtml).not.toContain(">Speaker 2<");
  });
});
