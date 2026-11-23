import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import {
  failureReasonMessage,
  MeetingNotesSection,
  normalizeNoteKind,
  scrollTranscriptToSegment,
} from "../src/components/meetings/MeetingNotesPanel";
import {
  isNearTranscriptBottom,
  LiveTranscriptSection,
} from "../src/components/meetings/LiveTranscriptSection";
import {
  NOTEPAD_SAVE_DEBOUNCE_MS,
  RecordMeetingSection,
} from "../src/components/meetings/RecordMeetingPage";
import { EditableMeetingTitle } from "../src/components/meetings/MeetingEntryPanels";
import { MeetingsListSection } from "../src/components/meetings/MeetingsList";

const i18n = i18next.createInstance();
void i18n
  .use(initReactI18next)
  .init({ lng: "en", resources: { en: { translation: en } } });
const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

describe("Plan B1 frontend flows (AC-B9, AC-B10, AC-B12..AC-B18)", () => {
  test("AC-B9 & D-B1: renders singular Rust kinds, accessible time chips, and failure reasons", () => {
    expect(normalizeNoteKind("decision")).toBe("decision");
    expect(normalizeNoteKind("decisions")).toBe("decision");
    expect(normalizeNoteKind("action")).toBe("action");
    expect(normalizeNoteKind("question")).toBe("question");

    const html = wrap(
      <MeetingNotesSection
        recording={false}
        notepadDraft="Ship Friday"
        snapshot={{
          meeting_id: 42,
          state: "ready",
          progress_label: null,
          elapsed_secs: 5,
          time_to_notes_secs: 5,
          notepad_text: "Ship Friday",
          oversized_count: 1,
          lines: [
            {
              kind: "summary",
              text: "Reviewed desktop release scope.",
              owner: null,
              start_ms: 12_000,
              from_notepad: false,
            },
            {
              kind: "decision",
              text: "Ship desktop release on Friday.",
              owner: null,
              start_ms: 41_000,
              from_notepad: true,
            },
            {
              kind: "action",
              text: "Send checklist to team.",
              owner: "Me",
              start_ms: 95_000,
              from_notepad: false,
            },
            {
              kind: "question",
              text: "Who updates release notes?",
              owner: null,
              start_ms: 120_000,
              from_notepad: false,
            },
            {
              kind: "decision",
              text: "Uncited line must stay hidden.",
              owner: null,
              start_ms: null,
              from_notepad: false,
            },
          ],
        }}
        onNotepadChange={() => {}}
        onJumpToSegment={() => {}}
        onRetry={() => {}}
      />,
    );

    expect(html).toContain("Summary");
    expect(html).toContain("Decisions");
    expect(html).toContain("Action items");
    expect(html).toContain("Open questions");
    expect(html.indexOf("Summary")).toBeLessThan(html.indexOf("Decisions"));
    expect(html.indexOf("Decisions")).toBeLessThan(
      html.indexOf("Action items"),
    );
    expect(html.indexOf("Action items")).toBeLessThan(
      html.indexOf("Open questions"),
    );
    expect(html).toContain('aria-label="Go to 00:41"');
    expect(html).toContain("your note");
    expect(html).not.toContain("Uncited line must stay hidden.");

    const tFn = (k: string, d: string) => i18n.t(k, d);
    expect(failureReasonMessage("no_model", tFn)).toBe(
      "No clean-up model is downloaded. Get one in Models.",
    );
    expect(failureReasonMessage("unavailable", tFn)).toBe(
      "Notes aren't available on this system yet.",
    );
    expect(failureReasonMessage("interrupted", tFn)).toBe(
      "Coco Voice closed while making notes.",
    );

    const failedHtml = wrap(
      <MeetingNotesSection
        recording={false}
        notepadDraft="My safe note"
        snapshot={{
          meeting_id: 42,
          state: "failed",
          progress_label: null,
          elapsed_secs: 0,
          failure_reason: "no_model",
          notepad_text: "My safe note",
          lines: [],
          oversized_count: 0,
        }}
        onNotepadChange={() => {}}
        onJumpToSegment={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(failedHtml).toContain(
      "No clean-up model is downloaded. Get one in Models.",
    );
    expect(failedHtml).toContain("Retry");
  });

  test("AC-B10, AC-B12 & D-B2: live transcript log role, gap filtering, Jump to latest, and chip scroll", () => {
    expect(
      isNearTranscriptBottom({
        scrollTop: 280,
        clientHeight: 200,
        scrollHeight: 490,
      }),
    ).toBe(true);
    expect(
      isNearTranscriptBottom({
        scrollTop: 50,
        clientHeight: 200,
        scrollHeight: 490,
      }),
    ).toBe(false);

    const pausedScrollHtml = wrap(
      <LiveTranscriptSection
        status={{
          state: "recording",
          elapsed_secs: 65,
          meeting_id: 9,
          mic_name: "Built-in Mic",
          warnings: [],
        }}
        following={false}
        dictationShortcut="Fn"
        segments={[
          {
            meeting_id: 9,
            channel: "me",
            start_ms: 10_000,
            end_ms: 12_000,
            text: "First spoken line.",
          },
          {
            meeting_id: 9,
            channel: "others",
            start_ms: 12_000,
            end_ms: 14_000,
            text: "   ",
          },
        ]}
        onStop={() => {}}
      />,
    );
    expect(pausedScrollHtml).toContain('role="log"');
    expect(pausedScrollHtml).toContain('aria-live="polite"');
    expect(pausedScrollHtml).toContain("Jump to latest");
    expect(pausedScrollHtml).toContain("First spoken line.");
    expect(pausedScrollHtml).not.toContain('data-start-ms="12000"');
    expect(pausedScrollHtml).toContain("Dictation still works (hold Fn).");

    let jumpedSelector = "";
    const jumped = scrollTranscriptToSegment(
      {
        querySelector: (sel) => {
          jumpedSelector = sel;
          return { scrollIntoView: () => {} };
        },
      },
      10_000,
    );
    expect(jumped).toBe(true);
    expect(jumpedSelector).toBe('[data-start-ms="10000"]');
  });

  test("AC-B14, AC-B16, AC-B17, AC-B18 & D-B4/D-B5/D-B6: single consent dialog, debounce, empty & finishing states", () => {
    expect(NOTEPAD_SAVE_DEBOUNCE_MS).toBe(500);

    // AC-B14: RecordMeetingSection does NOT mount a second duplicate consent dialog by default.
    const pageWithConsent = wrap(
      <RecordMeetingSection
        offerEnabled={true}
        offer={null}
        consentOpen={true}
        status={{
          state: "consent_open",
          elapsed_secs: 0,
          meeting_id: null,
          mic_name: null,
          warnings: [],
        }}
        segments={[]}
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
    expect(pageWithConsent).not.toContain('role="dialog"');
    expect(pageWithConsent).not.toContain("Product sync");

    // D-B4: Nothing was said state
    const emptyHtml = wrap(
      <MeetingNotesSection
        recording={false}
        hasSpokenSegments={false}
        notepadDraft=""
        snapshot={{
          meeting_id: 20,
          state: "idle",
          progress_label: null,
          elapsed_secs: 0,
          notepad_text: "",
          lines: [],
          oversized_count: 0,
        }}
        onNotepadChange={() => {}}
        onJumpToSegment={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(emptyHtml).toContain("Nothing was said, so there are no notes.");

    // D-B6: Notes so far before first piece + missing model during call
    const soFarPendingHtml = wrap(
      <MeetingNotesSection
        recording={true}
        showNotesSoFarCard={true}
        notepadDraft=""
        snapshot={null}
        onNotepadChange={() => {}}
        onJumpToSegment={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(soFarPendingHtml).toContain("Notes appear after a few minutes.");

    const soFarNoModelHtml = wrap(
      <MeetingNotesSection
        recording={true}
        showNotesSoFarCard={true}
        notepadDraft=""
        snapshot={{
          meeting_id: 21,
          state: "idle",
          progress_label: null,
          elapsed_secs: 0,
          failure_reason: "no_model",
          notepad_text: "",
          lines: [],
          oversized_count: 0,
        }}
        onNotepadChange={() => {}}
        onJumpToSegment={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(soFarNoModelHtml).toContain(
      "Notes need a clean-up model. Get one in Models.",
    );

    // D-B3: Inline editable title button
    const titleHtml = wrap(
      <EditableMeetingTitle title="Release review" onRename={() => {}} />,
    );
    expect(titleHtml).toContain('data-testid="meeting-title-button"');
    expect(titleHtml).toContain("Release review");

    // AC-B17 & D-B5: List row shows "Finishing notes" while running
    const listHtml = wrap(
      <MeetingsListSection
        items={[
          {
            id: 5,
            title: "Architecture review",
            started_at: 1_700_000_000,
            ended_at: 1_700_001_800,
            notes_ready: false,
          },
          {
            id: 6,
            title: "Design walkthrough",
            started_at: 1_700_003_600,
            ended_at: 1_700_005_400,
            notes_ready: true,
          },
        ]}
        finishingIds={[5]}
        query=""
        loading={false}
        confirmingId={null}
        busyDeleteId={null}
        audioLeftWarning={false}
        errorKey={null}
        onQueryChange={() => {}}
        onExport={() => {}}
        onRequestDelete={() => {}}
        onCancelDelete={() => {}}
        onConfirmDelete={() => {}}
      />,
    );
    expect(listHtml).toContain('data-status-kind="finishing-notes"');
    expect(listHtml).toContain("Finishing notes");
    expect(listHtml).toContain('data-status-kind="notes-ready"');
  });
});
