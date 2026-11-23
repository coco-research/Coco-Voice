import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { MeetingNotesSection } from "../src/components/meetings/MeetingNotesPanel";
import { OlderNoteWhileRecordingBanner } from "../src/components/meetings/MeetingHistoryExtras";
import { RecordMeetingSection } from "../src/components/meetings/RecordMeetingPage";
import { LiveTranscriptSection } from "../src/components/meetings/LiveTranscriptSection";
import {
  formatMeetingParticipantsLine,
  typedSpeakerNamesFromMap,
} from "../src/components/meetings/channelLabels";
import { UpcomingMeetingsList } from "../src/components/meetings/UpcomingMeetings";
import type { MeetingNotesSnapshot } from "../src/bindings.meetings";

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  interpolation: { escapeValue: false },
  resources: { en: { translation: en } },
});

const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

const READY_SNAPSHOT: MeetingNotesSnapshot = {
  meeting_id: 101,
  state: "ready",
  progress_label: null,
  elapsed_secs: 20,
  notepad_text: "ship this week",
  lines: [
    {
      kind: "decision",
      text: "Ship the desktop update this week.",
      owner: null,
      start_ms: 435_000,
      from_notepad: true,
    },
  ],
  left_out: [
    {
      category: "small_talk",
      start_ms: 12_000,
      end_ms: 24_000,
      topic: "Audio check",
      reason: "Small talk",
    },
  ],
  oversized_count: 0,
};

describe("W39 segmented switch + W41 sticky revert + s1/s4/s8 polish", () => {
  test("W39 segmented control renders 'Your notes' and 'Left out', defaults to 'Your notes', and uses zero accent classes", () => {
    const html = wrap(
      <MeetingNotesSection
        recording={false}
        snapshot={READY_SNAPSHOT}
        notepadDraft="ship this week"
        onNotepadChange={() => {}}
        onJumpToSegment={() => {}}
        onRetry={() => {}}
      />,
    );

    expect(html).toContain('data-testid="notes-view-segmented"');
    const segStart = html.indexOf('data-testid="notes-view-segmented"');
    const segEnd = html.indexOf("</div>", segStart);
    expect(segStart).toBeGreaterThan(-1);
    expect(segEnd).toBeGreaterThan(segStart);
    const segHtml = html.slice(segStart, segEnd);

    // Both labels rendered; "Your notes" selected on open, "Left out" unselected.
    expect(segHtml).toContain("Your notes");
    expect(segHtml).toContain("Left out");
    expect(segHtml).toContain(
      'data-testid="notes-tab-notes" aria-pressed="true"',
    );
    expect(segHtml).toContain(
      'data-testid="notes-tab-left-out" aria-pressed="false"',
    );

    // No element inside the segmented control uses accent blue.
    expect(segHtml).not.toContain("bg-logo-primary");
    expect(segHtml).not.toContain("text-logo-primary");
    expect(segHtml).not.toContain("border-logo-primary");
    expect(segHtml).not.toContain("bg-accent");

    // s8: time chip renders inside a light pill keeping brackets.
    expect(html).toContain("[07:15]");
    expect(html).toContain("bg-logo-primary/10");
  });

  test("W39 clicking Left out switches to Frame 15 Notes | Left out with 16 px margins", async () => {
    const src = await Bun.file(
      new URL(
        "../src/components/meetings/MeetingNotesPanel.tsx",
        import.meta.url,
      ),
    ).text();
    expect(src).toContain('setActiveView("left_out")');
    expect(src).toContain('activeView === "left_out"');
    expect(src).toContain('data-testid="frame-15-left-out"');
    expect(src).toContain("my-4 flex items-center justify-start");
  });

  test("W43 empty left_out still renders segmented control, keeps no loose line under panes, and renders empty line inside Left out pane", () => {
    const emptySnapshot: MeetingNotesSnapshot = {
      ...READY_SNAPSHOT,
      left_out: [],
    };
    const defaultHtml = wrap(
      <MeetingNotesSection
        recording={false}
        snapshot={emptySnapshot}
        notepadDraft="ship this week"
        onNotepadChange={() => {}}
        onJumpToSegment={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(defaultHtml).toContain('data-testid="notes-view-segmented"');
    expect(defaultHtml).toContain("Your notes");
    expect(defaultHtml).toContain("Left out");
    expect(defaultHtml).not.toContain("Nothing was left out.");
    expect(defaultHtml).not.toContain('data-testid="frame-15-left-out"');

    const leftOutHtml = wrap(
      <MeetingNotesSection
        recording={false}
        snapshot={emptySnapshot}
        notepadDraft="ship this week"
        initialView="left_out"
        onNotepadChange={() => {}}
        onJumpToSegment={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(leftOutHtml).toContain('data-testid="notes-view-segmented"');
    expect(leftOutHtml).toContain('data-testid="frame-15-left-out"');
    expect(leftOutHtml).toContain('data-testid="left-out-empty"');
    expect(leftOutHtml).toContain("Nothing was left out.");
  });

  test("W44 + t2 older-note banner uses blue outline, sits above eyebrow, and hint sits under Notes card", async () => {
    const html = wrap(
      <OlderNoteWhileRecordingBanner
        liveTitle="Product sync"
        elapsedSecs={768}
        onBackToLive={() => {}}
      />,
    );
    expect(html).toContain("border-logo-primary");
    expect(html).toContain("bg-background dark:bg-surface");
    expect(html).toContain("text-logo-primary");
    expect(html).toContain("Recording Product sync 12:48");
    expect(html).toContain("Back to live notes");
    expect(html).not.toContain("keeps recording");
    expect(html).not.toContain("bg-red-500");
    expect(html).not.toContain("text-red-600");
    expect(html).not.toContain("bg-logo-primary px-3");

    const ml = await Bun.file(
      new URL("../src/components/meetings/MeetingsList.tsx", import.meta.url),
    ).text();
    const detailStart = ml.indexOf('data-testid="meeting-detail-view"');
    const bannerPos = ml.indexOf("<OlderNoteWhileRecordingBanner", detailStart);
    const eyebrowPos = ml.indexOf('t("meetings.sidebar")', detailStart);
    const notesPos = ml.indexOf("<MeetingNotesSection", detailStart);
    const hintPos = ml.indexOf(
      'data-testid="older-note-live-hint"',
      detailStart,
    );
    const addAudioPos = ml.indexOf("<AddAudioControl", detailStart);
    expect(bannerPos).toBeGreaterThan(detailStart);
    expect(bannerPos).toBeLessThan(eyebrowPos);
    expect(hintPos).toBeGreaterThan(notesPos);
    expect(hintPos).toBeLessThan(addAudioPos);
  });

  test("W46 I7 participants line: calendar attendees, typed speaker rename with per-meeting isolation, and fallback", () => {
    const t = i18n.t.bind(i18n);

    // 1. Attendees [Sam (current user), Dana, Raj, Mei] -> "Sam with Dana, Raj, Mei"
    const attendeesHtml = wrap(
      <RecordMeetingSection
        offerEnabled={true}
        offer={null}
        consentOpen={false}
        status={{
          state: "recording",
          elapsed_secs: 768,
          attendees: ["Sam", "Dana", "Raj", "Mei"],
          meeting_id: 101,
          mic_name: "MacBook Pro Microphone",
          warnings: [],
        }}
        segments={[]}
        meetingWords={[]}
        meetingTitle="Product sync"
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
    expect(attendeesHtml).toContain('data-testid="live-meeting-participants"');
    expect(attendeesHtml).toContain("Sam with Dana, Raj, Mei");
    expect(attendeesHtml).not.toContain("Me and Others");

    // 2. Rename Speaker 2 to Jordan -> "Me with Jordan", and no other meeting's line changes
    const meeting101Speakers: Record<string, string> = {
      "Speaker 2": "Jordan",
    };
    const meeting102Speakers: Record<string, string> = {};
    expect(
      formatMeetingParticipantsLine(
        [],
        typedSpeakerNamesFromMap(meeting101Speakers),
        t,
      ),
    ).toBe("Me with Jordan");
    expect(
      formatMeetingParticipantsLine(
        [],
        typedSpeakerNamesFromMap(meeting102Speakers),
        t,
      ),
    ).toBe("Me and Others");

    const renamedLiveHtml = wrap(
      <LiveTranscriptSection
        status={{
          state: "recording",
          elapsed_secs: 300,
          attendees: [],
          meeting_id: 101,
          mic_name: null,
          warnings: [],
        }}
        segments={[]}
        speakerNames={meeting101Speakers}
        onStop={() => {}}
      />,
    );
    expect(renamedLiveHtml).toContain("Me with Jordan");

    // 3. No names -> "Me and Others"
    const fallbackHtml = wrap(
      <RecordMeetingSection
        offerEnabled={true}
        offer={null}
        consentOpen={false}
        status={{
          state: "recording",
          elapsed_secs: 768,
          attendees: [],
          meeting_id: 102,
          mic_name: "MacBook Pro Microphone",
          warnings: [],
        }}
        segments={[]}
        meetingWords={[]}
        meetingTitle="Design critique"
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
    expect(fallbackHtml).toContain('data-testid="live-meeting-participants"');
    expect(fallbackHtml).toContain("Me and Others");
  });

  test("s1 UpcomingMeetingsList card omits '1 of 1' badge, uses 2-line row, and outlined Start notes button", () => {
    const now = Math.floor(Date.now() / 1000);
    const html = wrap(
      <UpcomingMeetingsList
        mode="card"
        hasCalendars={true}
        meetings={[
          {
            title: "Design review",
            start: now + 25 * 60,
            end: now + 55 * 60,
            calendar_name: "Personal",
            attendee_display_names: [],
          },
        ]}
        totalCount={1}
        onOpenView={() => {}}
        onStartNotes={() => {}}
      />,
    );

    expect(html).toContain("Design review");
    expect(html).toContain("Start notes");
    expect(html).not.toContain("1 of 1");
    expect(html).toContain("border-mid-gray/35 bg-transparent");
    expect(html).not.toContain("bg-logo-primary px-3");
  });

  test("s4 Hands-free pill placement radios render on the right of each row", async () => {
    const src = await Bun.file(
      new URL(
        "../src/components/settings/SettingsV022Page.tsx",
        import.meta.url,
      ),
    ).text();
    const blockStart = src.indexOf('data-testid="pill-placement-rows"');
    expect(blockStart).toBeGreaterThan(-1);
    const block = src.slice(blockStart, blockStart + 1800);
    expect(block).toContain("flex items-center justify-between gap-3 py-2.5");
    expect(block.indexOf("pillTopTitle")).toBeLessThan(
      block.indexOf('value="top"'),
    );
    expect(block.indexOf("pillBottomTitle")).toBeLessThan(
      block.indexOf('value="bottom"'),
    );
  });

  test("W41 no sticky headers remain in MeetingsList or AppProfilesView", async () => {
    const ml = await Bun.file(
      new URL("../src/components/meetings/MeetingsList.tsx", import.meta.url),
    ).text();
    const ap = await Bun.file(
      new URL(
        "../src/components/settings/AppProfilesView.tsx",
        import.meta.url,
      ),
    ).text();
    expect(ml).not.toContain("meeting-detail-sticky-header");
    expect(ml).not.toContain("sticky top-0");
    expect(ap).not.toContain("sticky top-0");
  });
});
