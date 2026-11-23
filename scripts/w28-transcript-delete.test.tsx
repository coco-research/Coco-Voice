import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { MeetingSegmentView } from "../src/bindings.meetings";
import {
  LiveTranscriptSection,
  resolveSelectedSpanBounds,
  segmentKey,
} from "../src/components/meetings/LiveTranscriptSection";

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  interpolation: { escapeValue: false },
  resources: { en: { translation: en } },
});

const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

// Meeting with 3 spoken lines + 1 empty gap segment BEFORE line 3.
const SEGMENTS: MeetingSegmentView[] = [
  {
    id: 11,
    meeting_id: 77,
    channel: "me",
    start_ms: 10_000,
    end_ms: 15_000,
    text: "Welcome everyone to the weekly sync.",
  },
  {
    id: 12,
    meeting_id: 77,
    channel: "others",
    start_ms: 20_000,
    end_ms: 25_000,
    text: "Let us review onboarding metrics first.",
  },
  {
    id: 13,
    meeting_id: 77,
    channel: "others",
    start_ms: 30_000,
    end_ms: 32_000,
    text: "   ",
  },
  {
    id: 14,
    meeting_id: 77,
    channel: "me",
    start_ms: 45_000,
    end_ms: 52_000,
    text: "Budget approval for Q4 stays frozen until Friday.",
  },
];

describe("W28 (CX-10, AC-X3, I12): transcript search highlights without filtering & deletes by segment identity", () => {
  test("Case 1: searching a word only line 3 has keeps lines 1 & 2 rendered, highlights line 3, and deletes line 3's exact start_ms/end_ms", () => {
    const line3 = SEGMENTS[3]!;
    const selectedKeys = [segmentKey(line3)];

    const html = wrap(
      <LiveTranscriptSection
        status={{
          state: "idle",
          elapsed_secs: 60,
          meeting_id: 77,
          mic_name: null,
          warnings: [],
        }}
        segments={SEGMENTS}
        searchQuery="frozen"
        selectedKeys={selectedKeys}
        onSelectSegment={() => {}}
        onStop={() => {}}
      />,
    );

    // Lines 1 & 2 MUST stay rendered (search never filters rows out).
    expect(html).toContain("Welcome everyone to the weekly sync.");
    expect(html).toContain("Let us review onboarding metrics first.");
    // Line 3 carries the highlight (<mark>) and inline (selected).
    expect(html).toContain('data-search-match="true"');
    expect(html).toContain("<mark");
    expect(html).toContain("frozen");
    expect(html).toContain("(selected)");

    const deletedCalls: Array<{
      meetingId: number;
      startMs: number;
      endMs: number;
    }> = [];
    const bounds = resolveSelectedSpanBounds(SEGMENTS, selectedKeys);
    expect(bounds).not.toBeNull();
    if (bounds) {
      deletedCalls.push({
        meetingId: 77,
        startMs: bounds.startMs,
        endMs: bounds.endMs,
      });
    }

    expect(deletedCalls).toEqual([
      { meetingId: 77, startMs: 45_000, endMs: 52_000 },
    ]);
  });

  test("Case 2: no search typed with empty gap segment before line 3 still resolves line 3's exact start_ms/end_ms", () => {
    const line3 = SEGMENTS[3]!;
    const selectedKeys = [segmentKey(line3)];

    const html = wrap(
      <LiveTranscriptSection
        status={{
          state: "idle",
          elapsed_secs: 60,
          meeting_id: 77,
          mic_name: null,
          warnings: [],
        }}
        segments={SEGMENTS}
        searchQuery=""
        selectedKeys={selectedKeys}
        onSelectSegment={() => {}}
        onStop={() => {}}
      />,
    );

    expect(html).toContain("Welcome everyone to the weekly sync.");
    expect(html).toContain("Let us review onboarding metrics first.");
    expect(html).toContain("Budget approval for Q4 stays frozen until Friday.");
    expect(html).toContain('data-segment-key="14"');
    expect(html).toContain("(selected)");

    const bounds = resolveSelectedSpanBounds(SEGMENTS, selectedKeys);
    expect(bounds?.startMs).toBe(45_000);
    expect(bounds?.endMs).toBe(52_000);
    expect(bounds?.picked.length).toBe(1);
    expect(bounds?.picked[0]?.id).toBe(14);
  });
});
