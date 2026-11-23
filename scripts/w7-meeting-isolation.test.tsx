import { describe, expect, mock, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";

const TUE_27_OCT = Math.floor(new Date(2026, 9, 27, 9, 0, 0).getTime() / 1000);
const MON_26_OCT = Math.floor(new Date(2026, 9, 26, 14, 0, 0).getTime() / 1000);

const invokeCalls: Array<{
  cmd: string;
  args: Record<string, unknown> | undefined;
}> = [];

const PRODUCT_SYNC_SNAPSHOT = {
  meeting_id: 101,
  state: "ready" as const,
  progress_label: null,
  elapsed_secs: 38,
  notepad_text: "desktop update this week\nexport = single markdown file",
  lines: [
    {
      kind: "decision",
      text: "Ship the desktop update this week.",
      owner: null,
      start_ms: 435_000,
      from_notepad: true,
    },
  ],
  left_out: [],
  oversized_count: 0,
};

const DESIGN_CRITIQUE_SNAPSHOT = {
  meeting_id: 102,
  state: "ready" as const,
  progress_label: null,
  elapsed_secs: 18,
  notepad_text: "check typography contrast\nsimplify sidebar spacing",
  lines: [
    {
      kind: "summary",
      text: "Tightened sidebar vertical rhythm and bumped light-mode secondary contrast.",
      owner: null,
      start_ms: 145_000,
      from_notepad: true,
    },
    {
      kind: "decision",
      text: "Standardize meeting header dates to Ddd DD Mon format.",
      owner: null,
      start_ms: 310_000,
      from_notepad: false,
    },
  ],
  left_out: [],
  oversized_count: 0,
};

const DESIGN_CRITIQUE_SEGMENTS = [
  {
    meeting_id: 102,
    channel: "me" as const,
    start_ms: 145_000,
    end_ms: 153_000,
    text: "Let us tighten the sidebar rhythm and bump light-mode secondary text contrast.",
  },
];

mock.module("@tauri-apps/api/core", () => ({
  transformCallback: () => 0,
  Channel: class {},
  convertFileSrc: (s: string) => s,
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    invokeCalls.push({ cmd, args });
    if (cmd === "meeting_status") {
      return {
        state: "recording",
        elapsed_secs: 768,
        title: "Product sync",
        meeting_id: 101,
        mic_name: "MacBook Pro Microphone",
        warnings: [],
      };
    }
    if (cmd === "meeting_notes_snapshot") {
      const id = Number(args?.meetingId ?? args?.meeting_id);
      if (id === 102) return DESIGN_CRITIQUE_SNAPSHOT;
      return PRODUCT_SYNC_SNAPSHOT;
    }
    if (cmd === "meeting_segments") {
      const id = Number(args?.meetingId ?? args?.meeting_id);
      if (id === 102) return DESIGN_CRITIQUE_SEGMENTS;
      return [
        {
          meeting_id: 101,
          channel: "me",
          start_ms: 435_000,
          end_ms: 441_000,
          text: "Agreed, we ship the desktop update this week.",
        },
      ];
    }
    if (cmd === "list_meetings") {
      return [
        {
          id: 101,
          title: "Product sync",
          started_at: TUE_27_OCT,
          ended_at: TUE_27_OCT + 1800,
          notes_ready: true,
        },
        {
          id: 102,
          title: "Design critique",
          started_at: MON_26_OCT,
          ended_at: MON_26_OCT + 2700,
          notes_ready: true,
        },
      ];
    }
    return null;
  },
}));

mock.module("@tauri-apps/api/event", () => ({
  listen: async () => () => {},
}));

mock.module("@tauri-apps/plugin-dialog", () => ({
  save: async () => null,
  open: async () => null,
}));

const { commands } = await import("../src/bindings.meetings");
const { formatShortDateDddDDMon } =
  await import("../src/components/meetings/UpcomingMeetings");
const { formatDurationMinutes } =
  await import("../src/components/meetings/MeetingsList");
const { MeetingNotesSection } =
  await import("../src/components/meetings/MeetingNotesPanel");
const { OlderNoteWhileRecordingBanner } =
  await import("../src/components/meetings/MeetingHistoryExtras");
const { EditableMeetingTitle } =
  await import("../src/components/meetings/MeetingEntryPanels");

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  interpolation: { escapeValue: false },
  resources: { en: { translation: en } },
});

describe("W7 (CX-11): opening older meeting while recording isolates snapshot & segments by meetingId", () => {
  test("requests Design critique (102) while Product sync (101) is recording and renders zero Product sync lines", async () => {
    invokeCalls.length = 0;

    const liveStatus = await commands.meetingStatus();
    expect(liveStatus.state).toBe("recording");
    expect(liveStatus.meeting_id).toBe(101);
    expect(liveStatus.title).toBe("Product sync");

    const snapRes = await commands.meetingNotesSnapshot(102);
    const segRes = await commands.meetingSegments(102);
    expect(snapRes.status).toBe("ok");
    expect(segRes.status).toBe("ok");
    if (snapRes.status !== "ok" || segRes.status !== "ok") return;

    const snapCall = invokeCalls.find(
      (c) => c.cmd === "meeting_notes_snapshot",
    );
    const segCall = invokeCalls.find((c) => c.cmd === "meeting_segments");
    expect(snapCall?.args).toEqual({ meetingId: 102 });
    expect(segCall?.args).toEqual({ meetingId: 102 });

    const dateLabel = formatShortDateDddDDMon(MON_26_OCT, "en");
    const mins = formatDurationMinutes(MON_26_OCT, MON_26_OCT + 2700);
    expect(`${dateLabel} · ${mins} min`).toBe("Mon 26 Oct · 45 min");

    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <div>
          <OlderNoteWhileRecordingBanner
            liveTitle={liveStatus.title ?? "Active call"}
            elapsedSecs={liveStatus.elapsed_secs}
            onBackToLive={() => {}}
          />
          <header>
            <EditableMeetingTitle title="Design critique" />
            <span>{`${dateLabel} · ${mins} min`}</span>
          </header>
          <MeetingNotesSection
            recording={false}
            snapshot={snapRes.data}
            notepadDraft={snapRes.data.notepad_text}
            segments={segRes.data}
            hasSpokenSegments={true}
            onNotepadChange={() => {}}
            onJumpToSegment={() => {}}
            onRetry={() => {}}
          />
        </div>
      </I18nextProvider>,
    );

    expect(html).toContain("Recording Product sync 12:48");
    expect(html).toContain("Design critique");
    expect(html).toContain("Mon 26 Oct · 45 min");
    expect(html).toContain("check typography contrast");
    expect(html).toContain("Tightened sidebar vertical rhythm");
    expect(html).toContain(
      "Standardize meeting header dates to Ddd DD Mon format.",
    );

    // Strict isolation: ZERO Product sync notes or notepad lines may appear.
    expect(html).not.toContain("Ship the desktop update this week.");
    expect(html).not.toContain("desktop update this week");
    expect(html).not.toContain("export = single markdown file");
  });

  test("MeetingsList source resets detail state and guards async responses by selectedIdRef", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const src = fs.readFileSync(
      path.resolve(
        import.meta.dir,
        "../src/components/meetings/MeetingsList.tsx",
      ),
      "utf8",
    );
    expect(src).toContain("selectedIdRef.current = item.id;");
    expect(src).toContain("setDetailNotes(null);");
    expect(src).toContain("if (selectedIdRef.current !== item.id) return;");
    expect(src).toContain("meetingCommands.meetingNotesSnapshot(item.id)");
    expect(src).toContain("meetingCommands.meetingSegments(item.id)");
  });

  test("W36 fixture-schema gate: rejects unknown AppSettings, CorrectionPair, or SavedBlock keys", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const repoRoot = path.resolve(import.meta.dir, "..");
    const bindingsSrc = fs.readFileSync(
      path.join(repoRoot, "src/bindings.ts"),
      "utf8",
    );
    const settingsRs = fs.readFileSync(
      path.join(repoRoot, "src-tauri/src/settings.rs"),
      "utf8",
    );

    const validAppSettingsKeys = new Set<string>();
    const tsBlock = bindingsSrc.match(
      /export type AppSettings = \{([\s\S]*?)\n\}/,
    );
    if (tsBlock?.[1]) {
      for (const m of tsBlock[1].matchAll(/([a-z][a-z0-9_]*)\??:/g)) {
        validAppSettingsKeys.add(m[1]!);
      }
    }
    const rsStart = settingsRs.indexOf("pub struct AppSettings {");
    if (rsStart >= 0) {
      const rsEnd = settingsRs.indexOf("\n}", rsStart);
      const rsBlock = settingsRs.slice(rsStart, rsEnd);
      for (const m of rsBlock.matchAll(/pub\s+([a-z][a-z0-9_]*)\s*:/g)) {
        validAppSettingsKeys.add(m[1]!);
      }
    }
    const validCorrectionKeys = new Set(["from", "to"]);
    const validSavedBlockKeys = new Set([
      "name",
      "trigger",
      "content",
      "expansion",
    ]);

    const validateFixtureSettings = (
      obj: Record<string, unknown>,
    ): string[] => {
      const unknown: string[] = [];
      for (const key of Object.keys(obj)) {
        if (!validAppSettingsKeys.has(key)) {
          unknown.push(`AppSettings.${key}`);
        }
      }
      if (Array.isArray(obj.corrections)) {
        for (const item of obj.corrections as Array<Record<string, unknown>>) {
          for (const k of Object.keys(item)) {
            if (!validCorrectionKeys.has(k)) {
              unknown.push(`CorrectionPair.${k}`);
            }
          }
        }
      }
      if (Array.isArray(obj.saved_blocks)) {
        for (const item of obj.saved_blocks as Array<Record<string, unknown>>) {
          for (const k of Object.keys(item)) {
            if (!validSavedBlockKeys.has(k)) {
              unknown.push(`SavedBlock.${k}`);
            }
          }
        }
      }
      return unknown;
    };

    // Negative check: legacy bad fixture keys MUST be caught.
    expect(
      validateFixtureSettings({
        offer_remember_fix: true,
        corrections: [{ wrong: "coco boys", right: "Coco Voice" }],
        saved_blocks: [{ cue: "a", text: "b" }],
      }),
    ).toEqual([
      "AppSettings.offer_remember_fix",
      "CorrectionPair.wrong",
      "CorrectionPair.right",
      "SavedBlock.cue",
      "SavedBlock.text",
    ]);

    // Positive check: real schema keys pass with 0 errors.
    expect(
      validateFixtureSettings({
        paste_delay_ms: 60,
        offer_remember_fix_enabled: true,
        corrections: [{ from: "coco boys", to: "Coco Voice" }],
        saved_blocks: [
          {
            name: "my address",
            trigger: "my address",
            content: "221 Example Street, Apt 4...",
            expansion: "221 Example Street, Apt 4...",
          },
        ],
      }),
    ).toEqual([]);

    // Optional external fixture verification when FIXTURE_SETTINGS_JSON is provided.
    if (process.env.FIXTURE_SETTINGS_JSON) {
      const parsed = JSON.parse(process.env.FIXTURE_SETTINGS_JSON) as Record<
        string,
        unknown
      >;
      expect(validateFixtureSettings(parsed)).toEqual([]);
    }
  });
});
