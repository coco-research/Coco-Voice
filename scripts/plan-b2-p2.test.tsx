import { describe, expect, test } from "bun:test";
import {
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import {
  CatchUpCard,
  MeetingNotesSection,
} from "../src/components/meetings/MeetingNotesPanel";
import {
  UpcomingMeetingsList,
  upcomingEventKey,
} from "../src/components/meetings/UpcomingMeetings";
import {
  MeetingTodosPanel,
  formatTodoSourceLine,
} from "../src/components/meetings/AskMeetingsView";
import { RecordMeetingSection } from "../src/components/meetings/RecordMeetingPage";
import { SPEAKER_SPLIT_ENABLED } from "../src/components/meetings/gate";

const ROOT = resolve(import.meta.dir, "..");

const i18n = i18next.createInstance();
void i18n.use(initReactI18next).init({
  lng: "en",
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  resources: { en: { translation: en } },
});

function renderHtml(node: React.ReactElement): string {
  return renderToStaticMarkup(
    React.createElement(I18nextProvider, { i18n }, node),
  );
}

function walkFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) {
      out.push(...walkFiles(full));
    } else {
      out.push(full);
    }
  }
  return out;
}

describe("Plan B2 Part 2 — X-d..X-l & Amendment 3 (AC-X41, Q-X4, AC-X42)", () => {
  test("X-e (AC-X18, D-X3, D-X6): CatchUpCard renders 5/10/15m window chooser, not-enough-said notice, and cited lines", () => {
    const emptyHtml = renderHtml(
      React.createElement(CatchUpCard, {
        lines: [],
        open: true,
        minutes: 5,
        notEnoughSaid: true,
        onSelectMinutes: () => {},
        onRequestCatchUp: () => {},
        onJumpToSegment: () => {},
      }),
    );
    expect(emptyHtml).toContain("What did I miss?");
    expect(emptyHtml).toContain("5m");
    expect(emptyHtml).toContain("10m");
    expect(emptyHtml).toContain("15m");
    expect(emptyHtml).toContain("Not enough has been said yet.");

    const linesHtml = renderHtml(
      React.createElement(CatchUpCard, {
        lines: [
          {
            kind: "summary",
            text: "Agreed to keep Friday launch date.",
            owner: null,
            start_ms: 125_000,
            from_notepad: false,
          },
        ],
        open: true,
        minutes: 10,
        notEnoughSaid: false,
        onSelectMinutes: () => {},
        onRequestCatchUp: () => {},
        onJumpToSegment: () => {},
      }),
    );
    expect(linesHtml).toContain("Last 10 minutes");
    expect(linesHtml).toContain("Agreed to keep Friday launch date.");
    expect(linesHtml).toContain("[02:05]");
  });

  test("X-f (AC-X26, D-X6): MeetingNotesSection renders persisted not_covered spans with topic+reason and empty left-out notice", () => {
    const withLeftOutHtml = renderHtml(
      React.createElement(MeetingNotesSection, {
        recording: false,
        snapshot: {
          meeting_id: 11,
          state: "ready",
          progress_label: null,
          elapsed_secs: 4,
          notepad_text: "",
          lines: [
            {
              kind: "decision",
              text: "Ship desktop release Friday.",
              owner: null,
              start_ms: 10_000,
              from_notepad: false,
            },
          ],
          left_out: [
            {
              category: "small_talk",
              start_ms: 42_000,
              topic: "Morning coffee chat",
              reason: "Small talk before agenda started.",
            },
            {
              category: "not_covered",
              start_ms: 95_000,
              topic: "Vendor pricing range",
              reason: "Too long to make notes from.",
            },
          ],
          oversized_count: 0,
        },
        notepadDraft: "",
        initialView: "left_out",
        onNotepadChange: () => {},
        onJumpToSegment: () => {},
        onRetry: () => {},
      }),
    );
    expect(withLeftOutHtml).toContain("notes-view-segmented");
    expect(withLeftOutHtml).toContain("frame-15-left-out");
    expect(withLeftOutHtml).toContain("Morning coffee chat");
    expect(withLeftOutHtml).toContain("Vendor pricing range");
    expect(withLeftOutHtml).toContain("Too long to make notes from.");

    const emptyLeftOutDefaultHtml = renderHtml(
      React.createElement(MeetingNotesSection, {
        recording: false,
        snapshot: {
          meeting_id: 11,
          state: "ready",
          progress_label: null,
          elapsed_secs: 4,
          notepad_text: "",
          lines: [
            {
              kind: "decision",
              text: "Ship desktop release Friday.",
              owner: null,
              start_ms: 10_000,
              from_notepad: false,
            },
          ],
          left_out: [],
          oversized_count: 0,
        },
        notepadDraft: "",
        onNotepadChange: () => {},
        onJumpToSegment: () => {},
        onRetry: () => {},
      }),
    );
    expect(emptyLeftOutDefaultHtml).toContain("notes-view-segmented");
    expect(emptyLeftOutDefaultHtml).toContain("Ship desktop release Friday.");
    expect(emptyLeftOutDefaultHtml).not.toContain("Nothing was left out.");

    const emptyLeftOutPaneHtml = renderHtml(
      React.createElement(MeetingNotesSection, {
        recording: false,
        snapshot: {
          meeting_id: 11,
          state: "ready",
          progress_label: null,
          elapsed_secs: 4,
          notepad_text: "",
          lines: [
            {
              kind: "decision",
              text: "Ship desktop release Friday.",
              owner: null,
              start_ms: 10_000,
              from_notepad: false,
            },
          ],
          left_out: [],
          oversized_count: 0,
        },
        notepadDraft: "",
        initialView: "left_out",
        onNotepadChange: () => {},
        onJumpToSegment: () => {},
        onRetry: () => {},
      }),
    );
    expect(emptyLeftOutPaneHtml).toContain("frame-15-left-out");
    expect(emptyLeftOutPaneHtml).toContain("Nothing was left out.");
    expect(emptyLeftOutPaneHtml).toContain("Ship desktop release Friday.");
  });

  test("X-i (Q-X4 Amendment 3, CX-5, D-X4, D-X6): Frame 10 Coming up omits held footer wording and renders count, started badge, empty & denied states", () => {
    const upcomingSource = readFileSync(
      join(ROOT, "src", "components", "meetings", "UpcomingMeetings.tsx"),
      "utf8",
    );
    expect(upcomingSource).not.toContain("Read from your Mac's calendar");
    expect(upcomingSource).not.toContain("doesn't send your calendar anywhere");

    const sample = {
      title: "Weekly Design Review",
      start: 1_760_000_000,
      end: 1_760_001_800,
      calendar_name: "Personal",
      attendee_display_names: ["Dana", "Sam"],
    };
    const startedHtml = renderHtml(
      React.createElement(UpcomingMeetingsList, {
        meetings: [sample],
        hasCalendars: true,
        totalCount: 12,
        startedKey: upcomingEventKey(sample),
        onStartNotes: () => {},
      }),
    );
    expect(startedHtml).toContain("Nothing starts by itself.");
    expect(startedHtml).toContain("1 of 12");
    expect(startedHtml).toContain("Notes started");
    expect(startedHtml).not.toContain("Read from your Mac");

    const emptyHtml = renderHtml(
      React.createElement(UpcomingMeetingsList, {
        meetings: [],
        hasCalendars: true,
        onStartNotes: () => {},
      }),
    );
    expect(emptyHtml).toContain("No meetings today or tomorrow.");

    const deniedHtml = renderHtml(
      React.createElement(UpcomingMeetingsList, {
        meetings: [],
        hasCalendars: true,
        permissionDenied: true,
        onStartNotes: () => {},
      }),
    );
    expect(deniedHtml).toContain(
      "Calendar access is turned off in System Settings.",
    );
  });

  test("X-k (I15): MeetingTodosPanel formats 'You said it in' and omits repeated date on consecutive same-meeting rows", () => {
    const fakeT = (
      _key: string,
      opts?: { defaultValue?: string; title?: string; date?: string },
    ) =>
      (opts?.defaultValue ?? "")
        .replace("{{title}}", opts?.title ?? "")
        .replace("{{date}}", opts?.date ?? "");

    const first = formatTodoSourceLine(
      {
        meeting_id: 7,
        meeting_title: "Launch Prep",
        meeting_date: "Mon, Nov 18",
        said_by_me: true,
      },
      null,
      fakeT,
    );
    const secondSame = formatTodoSourceLine(
      {
        meeting_id: 7,
        meeting_title: "Launch Prep",
        meeting_date: "Mon, Nov 18",
        said_by_me: true,
      },
      7,
      fakeT,
    );
    expect(first).toBe("You said it in Launch Prep, Mon, Nov 18");
    expect(secondSame).toBe("You said it in Launch Prep");

    const html = renderHtml(
      React.createElement(MeetingTodosPanel, {
        items: [
          {
            id: "todo-1",
            text: "Send rollout notes",
            meeting_id: 7,
            meeting_title: "Launch Prep",
            meeting_date: "Mon, Nov 18",
            start_ms: 60_000,
            done: false,
            said_by_me: true,
          },
          {
            id: "todo-2",
            text: "Share checklist",
            meeting_id: 7,
            meeting_title: "Launch Prep",
            meeting_date: "Mon, Nov 18",
            start_ms: 90_000,
            done: false,
            said_by_me: false,
          },
        ],
      }),
    );
    expect(html).toContain("You said it in Launch Prep, Mon, Nov 18");
    expect(html).toContain("Share checklist");
  });

  test("X-d (AC-X14): RecordMeetingSection exposes Pause while recording and Resume + Stop + Add audio while paused", () => {
    const recordingHtml = renderHtml(
      React.createElement(RecordMeetingSection, {
        offerEnabled: true,
        offer: null,
        consentOpen: false,
        status: {
          state: "recording",
          elapsed_secs: 45,
          paused: false,
          meeting_id: 3,
          mic_name: "Built-in",
          warnings: [],
        },
        segments: [],
        meetingWords: [],
        onToggleOffer: () => {},
        onOpenConsent: () => {},
        onConfirmStart: () => {},
        onCancelConsent: () => {},
        onDismissOffer: () => {},
        onStopMeeting: () => {},
        onPauseMeeting: () => {},
        onResumeMeeting: () => {},
        onAddMeetingWord: () => {},
        onRemoveMeetingWord: () => {},
      }),
    );
    expect(recordingHtml).toContain("Pause");

    const pausedHtml = renderHtml(
      React.createElement(RecordMeetingSection, {
        offerEnabled: true,
        offer: null,
        consentOpen: false,
        status: {
          state: "paused",
          elapsed_secs: 45,
          paused: true,
          meeting_id: 3,
          mic_name: "Built-in",
          warnings: [],
        },
        segments: [],
        meetingWords: [],
        onToggleOffer: () => {},
        onOpenConsent: () => {},
        onConfirmStart: () => {},
        onCancelConsent: () => {},
        onDismissOffer: () => {},
        onStopMeeting: () => {},
        onPauseMeeting: () => {},
        onResumeMeeting: () => {},
        onAppendAudio: () => {},
        onAddMeetingWord: () => {},
        onRemoveMeetingWord: () => {},
      }),
    );
    expect(pausedHtml).toContain("Resume");
    expect(pausedHtml).toContain(
      "Paused: nothing is recorded. Still one meeting.",
    );
  });

  test("Amendment 3 (AC-X41): speaker-split is OFF by default in Cargo.toml, gate.ts, MeetingsList.tsx, and SpeakerSplitSection is tree-shaken out of VITE_MEETINGS=1 Vite dist/", () => {
    expect(SPEAKER_SPLIT_ENABLED).toBe(false);

    const cargoToml = readFileSync(
      join(ROOT, "src-tauri", "Cargo.toml"),
      "utf8",
    );
    expect(cargoToml).toContain('speaker-split = ["meetings"]');
    expect(cargoToml).not.toMatch(/default\s*=\s*\[[^\]]*speaker-split/);

    const meetingsListTs = readFileSync(
      join(ROOT, "src", "components", "meetings", "MeetingsList.tsx"),
      "utf8",
    );
    expect(meetingsListTs).not.toContain('from "./SpeakerSplitSection"');
    expect(meetingsListTs).toContain("useSpeakerSplitSection");

    const outDir = mkdtempSync(join(tmpdir(), "coco-voice-b2-p2-dist-"));
    try {
      const res = spawnSync(
        "bunx",
        ["vite", "build", "--outDir", outDir, "--emptyOutDir"],
        {
          cwd: ROOT,
          env: {
            ...process.env,
            VITE_MEETINGS: "1",
            VITE_SPEAKER_SPLIT: "",
          },
          encoding: "utf8",
        },
      );
      expect(res.status).toBe(0);
      const distText = walkFiles(outDir)
        .filter((f) => /\.(js|mjs|css|html)$/i.test(f))
        .map((f) => readFileSync(f, "utf8"))
        .join("\n");
      expect(distText).not.toContain("frame-8-speakers");
      expect(distText).not.toContain("Speaker split needs a 31 MB download");
      expect(distText).not.toContain("Working out speakers…");
      expect(distText).not.toContain("github.com/k2-fsa");
    } finally {
      rmSync(outDir, { recursive: true, force: true });
    }
  }, 20_000);

  test("AC-X40 & AC-X42: calendar, notes, todos, and pipeline Rust production modules contain zero network calls and calendar modules contain zero model calls", () => {
    const netForbidden = [
      "reqwest",
      "ureq",
      "hyper",
      "std::net",
      "NSURLSession",
      "tauri_plugin_http",
    ];
    const localOnlyFiles = [
      "calendar_commands.rs",
      "calendar_eventkit.rs",
      "calendar_upcoming.rs",
      "meeting_notes_v42.rs",
      "meeting_todos.rs",
      "meeting_pipeline.rs",
    ];
    for (const file of localOnlyFiles) {
      const full = readFileSync(join(ROOT, "src-tauri", "src", file), "utf8");
      const prodOnly = full.split("#[cfg(test)]")[0] ?? full;
      for (const tok of netForbidden) {
        expect(prodOnly).not.toContain(tok);
      }
    }

    const modelForbidden = [
      "local_llm",
      "apple_intelligence",
      "NotesGenerator",
    ];
    for (const file of [
      "calendar_commands.rs",
      "calendar_eventkit.rs",
      "calendar_upcoming.rs",
    ]) {
      const full = readFileSync(join(ROOT, "src-tauri", "src", file), "utf8");
      const prodOnly = full.split("#[cfg(test)]")[0] ?? full;
      for (const tok of modelForbidden) {
        expect(prodOnly).not.toContain(tok);
      }
    }
  });
});
