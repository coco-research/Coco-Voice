import React from "react";
import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { SpeakerSplitSection } from "../src/components/meetings/SpeakerSplitSection";
import { UpcomingMeetingsList } from "../src/components/meetings/UpcomingMeetings";
import { startNotesFlow } from "../src/components/meetings/upcoming";
import {
  CALENDAR_PERMISSION_USAGE_COPY,
  CalendarPickerView,
} from "../src/components/settings/CalendarPicker";

const EXPECTED_C4_USAGE =
  "Coco reads your calendar on this Mac to show upcoming meetings. It never changes your calendar or sends it anywhere.";

describe("G8 speaker split/rename and test-calendar upcoming meetings", () => {
  test("Frame 8 SpeakerSplitSection renders per-meeting slots, Unnamed badge, and zero-voiceprint legal notice", () => {
    const html = renderToStaticMarkup(
      <SpeakerSplitSection
        meetingId={7}
        segments={[]}
        initialSlots={[
          { slot: "Speaker 1", custom_name: "" },
          { slot: "Speaker 2", custom_name: "Dana" },
        ]}
      />,
    );
    expect(html).toContain('data-testid="frame-8-speakers"');
    expect(html).toContain("Speaker 1");
    expect(html).toContain("Unnamed");
    expect(html).toContain('value="Dana"');
    expect(html).toContain('aria-label="Name for Speaker 2"');
    expect(html).toContain(
      "Speaker labels are worked out per meeting and nothing about anyone&#x27;s voice is saved.",
    );
  });

  test("About acknowledgments translations carry exact CNRS MIT and WeSpeaker CC BY 4.0 VoxCeleb sherpa-onnx provenance", () => {
    const repoRoot = join(import.meta.dir, "..");
    const enRaw = JSON.parse(
      readFileSync(
        join(repoRoot, "src/i18n/locales/en/translation.json"),
        "utf8",
      ),
    );
    const ack = enRaw.settings.about.acknowledgments;
    expect(ack.pyannoteDetails).toContain("MIT, Copyright (c) 2022 CNRS");
    expect(ack.pyannoteDetails).not.toContain("Apache-2.0");
    expect(ack.wespeakerDetails).toContain("CC BY 4.0");
    expect(ack.wespeakerDetails).toContain(
      "https://creativecommons.org/licenses/by/4.0/",
    );
    expect(ack.wespeakerDetails).toContain("VoxCeleb");
    expect(ack.wespeakerDetails).toContain("k2-fsa/sherpa-onnx");

    const aboutSrc = readFileSync(
      join(repoRoot, "src/components/settings/about/AboutSettings.tsx"),
      "utf8",
    );
    expect(aboutSrc).toContain("MIT, Copyright (c) 2022 CNRS");
    expect(aboutSrc).toContain("CC BY 4.0");
    expect(aboutSrc).not.toContain("Apache-2.0");
  });

  test("Info.meetings.plist carries the exact calendar usage disclosure string", () => {
    const repoRoot = join(import.meta.dir, "..");
    const plist = readFileSync(
      join(repoRoot, "src-tauri/Info.meetings.plist"),
      "utf8",
    );
    expect(plist).toContain("<key>NSCalendarsFullAccessUsageDescription</key>");
    expect(plist).toContain(`<string>${EXPECTED_C4_USAGE}</string>`);
    expect(CALENDAR_PERMISSION_USAGE_COPY).toBe(EXPECTED_C4_USAGE);
  });

  test("CalendarPickerView defaults to none pre-selected and disables work calendars", () => {
    const html = renderToStaticMarkup(
      <CalendarPickerView
        choices={[
          {
            id: "test-1",
            title: "[Test] Product syncs",
            account: "Local",
            blocked: null,
          },
          {
            id: "work-1",
            title: "Coco Voice Test Work",
            account: "Exchange",
            blocked: "work",
          },
        ]}
        selected={[]}
        onToggle={() => {}}
      />,
    );
    expect(html).toContain('data-testid="frame-calendars-picker"');
    expect(html).toContain(
      "None chosen yet. Pick the calendars Coming up may read.",
    );
    expect(html).not.toContain("checked=");
    expect(html).toContain("Work calendar: never read");
    expect(html).toContain("disabled");
  });

  test("UpcomingMeetingsList and startNotesFlow require Frame 2 consent reminder before starting", async () => {
    const emptyHtml = renderToStaticMarkup(
      <UpcomingMeetingsList
        meetings={[]}
        hasCalendars={false}
        onStartNotes={() => {}}
      />,
    );
    expect(emptyHtml).toContain('data-testid="frame-10-coming-up"');
    expect(emptyHtml).toContain("No calendars chosen yet");

    const sample = {
      title: "Design review",
      start: 1700000000,
      end: 1700001800,
      calendar_name: "[Test] Product syncs",
      attendee_display_names: ["Alex"],
    };
    let began = false;
    let kept = false;
    const declined = await startNotesFlow(sample, {
      showConsentReminder: async () => false,
      keepForNotes: async () => {
        kept = true;
      },
      beginNotes: async () => {
        began = true;
      },
    });
    expect(declined).toBe("declined");
    expect(kept).toBe(false);
    expect(began).toBe(false);

    const started = await startNotesFlow(sample, {
      showConsentReminder: async () => true,
      keepForNotes: async () => {
        kept = true;
      },
      beginNotes: async () => {
        began = true;
      },
    });
    expect(started).toBe("started");
    expect(kept).toBe(true);
    expect(began).toBe(true);
  });
});
