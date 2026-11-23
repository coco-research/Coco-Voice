import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import {
  CatchUpCard,
  filterCitedNoteLines,
  MeetingNotesSection,
  scrollTranscriptToSegment,
} from "../src/components/meetings/MeetingNotesPanel";
import {
  FoldersAndTemplatesSection,
  sanitizeFlatFolderName,
} from "../src/components/meetings/FoldersAndTemplatesView";

const i18n = i18next.createInstance();
void i18n
  .use(initReactI18next)
  .init({ lng: "en", resources: { en: { translation: en } } });
const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

describe("A2 Meetings screens (Frames 5, 11, 14, 15, 16 + Catch up)", () => {
  test("drops uncited note lines, renders Summary/Decisions/Action items/Open questions, Left out, and scrollTranscriptToSegment", () => {
    const cited = filterCitedNoteLines([
      {
        kind: "summary",
        text: "Launch scope is locked.",
        owner: null,
        start_ms: 41_000,
        from_notepad: true,
      },
      {
        kind: "decisions",
        text: "Uncited hallucination must never render.",
        owner: null,
        start_ms: null,
        from_notepad: false,
      },
    ]);
    expect(cited.length).toBe(1);

    let scrolledSelector = "";
    const ok = scrollTranscriptToSegment(
      {
        querySelector: (sel) => {
          scrolledSelector = sel;
          return { scrollIntoView: () => {} };
        },
      },
      41_000,
    );
    expect(ok).toBe(true);
    expect(scrolledSelector).toBe('[data-start-ms="41000"]');

    const html = wrap(
      <MeetingNotesSection
        recording={false}
        notepadDraft="Ship empty state copy"
        snapshot={{
          meeting_id: 12,
          state: "ready",
          progress_label: null,
          elapsed_secs: 9,
          time_to_notes_secs: 8,
          notepad_text: "Ship empty state copy",
          oversized_count: 0,
          lines: [
            {
              kind: "summary",
              text: "Agreed on empty state copy.",
              owner: null,
              start_ms: 41_000,
              from_notepad: true,
            },
            {
              kind: "actions",
              text: "Send updated draft by Thursday.",
              owner: "Dana",
              start_ms: 588_000,
              from_notepad: false,
            },
            {
              kind: "questions",
              text: "Should export include timestamps?",
              owner: null,
              start_ms: 760_000,
              from_notepad: false,
            },
            {
              kind: "summary",
              text: "No-source line",
              owner: null,
              start_ms: null,
              from_notepad: false,
            },
          ],
          left_out: [
            {
              category: "small_talk",
              start_ms: 5_000,
              speaker: "Me",
              text: "Weekend weather check-in.",
            },
            {
              category: "uncovered",
              start_ms: 310_000,
              speaker: "Dana",
              text: "Maybe revisit keyboard themes later.",
            },
          ],
        }}
        onNotepadChange={() => {}}
        onJumpToSegment={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(html).toContain("Your notes");
    expect(html).toContain("Summary");
    expect(html).toContain("Action items");
    expect(html).toContain("Open questions");
    expect(html).toContain("your note");
    expect(html).toContain("[00:41]");
    expect(html).toContain("[09:48]");
    expect(html).not.toContain("Ready in 8 s");
    expect(html).not.toContain("No-source line");
    expect(html).toContain("Left out");
    const leftOutHtml = wrap(
      <MeetingNotesSection
        recording={false}
        notepadDraft="Ship empty state copy"
        initialView="left_out"
        snapshot={{
          meeting_id: 14,
          state: "ready",
          progress_label: null,
          elapsed_secs: 8,
          time_to_notes_secs: 8,
          notepad_text: "Ship empty state copy",
          oversized_count: 0,
          lines: [],
          left_out: [
            {
              category: "small_talk",
              start_ms: 5_000,
              speaker: "Me",
              text: "Weekend weather check-in.",
            },
            {
              category: "uncovered",
              start_ms: 310_000,
              speaker: "Dana",
              text: "Maybe revisit keyboard themes later.",
            },
          ],
        }}
        onNotepadChange={() => {}}
        onJumpToSegment={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(leftOutHtml).toContain("Treated as small talk");
    expect(leftOutHtml).toContain("Not covered by any note");
    expect(leftOutHtml).toContain("Show in transcript");
  });

  test("renders Frame 16 Notes so far, Finishing notes progress, failure Retry, Catch up, and flat Folders & templates", () => {
    const runningHtml = wrap(
      <MeetingNotesSection
        recording={false}
        notepadDraft=""
        snapshot={{
          meeting_id: 14,
          state: "running",
          progress_label: null,
          progress_step: 2,
          progress_total: 4,
          elapsed_secs: 6,
          notepad_text: "",
          lines: [],
          oversized_count: 0,
        }}
        onNotepadChange={() => {}}
        onJumpToSegment={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(runningHtml).toContain("Finishing notes: 6 s");

    const failedHtml = wrap(
      <MeetingNotesSection
        recording={false}
        notepadDraft="Safe note"
        snapshot={{
          meeting_id: 14,
          state: "failed",
          progress_label: null,
          elapsed_secs: 6,
          notepad_text: "Safe note",
          lines: [],
          oversized_count: 0,
        }}
        onNotepadChange={() => {}}
        onJumpToSegment={() => {}}
        onRetry={() => {}}
      />,
    );
    expect(failedHtml).toContain(
      "Notes couldn&#x27;t be made. Your transcript and notes are safe.",
    );
    expect(failedHtml).toContain("Transcript");
    expect(failedHtml).toContain("Your notes");
    expect(failedHtml).toContain("grid-cols-2");
    expect(failedHtml).toContain("Retry");

    const catchUpHtml = wrap(
      <CatchUpCard
        open={true}
        lines={[
          {
            kind: "summary",
            text: "Moved rollout date to Friday.",
            owner: null,
            start_ms: 120_000,
            from_notepad: false,
          },
        ]}
        onRequestCatchUp={() => {}}
        onJumpToSegment={() => {}}
      />,
    );
    expect(catchUpHtml).toContain("What did I miss?");
    expect(catchUpHtml).toContain("Last 5 minutes");
    expect(catchUpHtml).toContain("[02:00]");

    expect(sanitizeFlatFolderName("Customers/Enterprise/Q4")).toBe(
      "Customers Enterprise Q4",
    );
    const foldersHtml = wrap(
      <FoldersAndTemplatesSection
        folders={[
          {
            id: "customers",
            name: "Customers",
            open: true,
            meetings: [],
          },
        ]}
        templates={[
          {
            id: "one-to-one",
            name: "One-to-one",
            sections: ["Updates", "Decisions", "Action items", "Open questions"],
          },
          {
            id: "customer-call",
            name: "Customer call",
            sections: ["Their needs", "Decisions", "Action items", "Follow-ups"],
          },
        ]}
        selectedTemplateId="one-to-one"
        onToggleFolder={() => {}}
        onAddFolder={() => {}}
        onSelectTemplate={() => {}}
      />,
    );
    expect(foldersHtml).toContain("Customers");
    expect(foldersHtml).toContain("One-to-one");
    expect(foldersHtml).toContain("Customer call");
    expect(foldersHtml).toContain("Edit templates");
  });
});
