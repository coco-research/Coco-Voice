import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import i18next from "i18next";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { I18nextProvider, initReactI18next } from "react-i18next";
import type {
  MeetingNotesSnapshot,
  MeetingSegmentView,
} from "../src/bindings.meetings";
import en from "../src/i18n/locales/en/translation.json";
import {
  loadFailedTranscriptSegments,
  MeetingNotesSection,
} from "../src/components/meetings/MeetingNotesPanel";
import { runOptimisticCommand } from "../src/components/meetings/meetings-ui";
import {
  changeTranscriptKeepDaysAction,
  deleteTrashedMeetingNowAction,
  restoreTrashedMeetingAction,
  type TrashedMeetingItem,
} from "../src/components/meetings/MeetingHistoryExtras";

const root = join(import.meta.dir, "..");

async function createTestI18n() {
  const instance = i18next.createInstance();
  await instance.use(initReactI18next).init({
    lng: "en",
    resources: { en: { translation: en } },
    interpolation: { escapeValue: false },
  });
  return instance;
}

function listFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      out.push(...listFiles(full));
    } else if (st.isFile()) {
      out.push(full);
    }
  }
  return out;
}

const FAILED_SNAPSHOT: MeetingNotesSnapshot = {
  meeting_id: 42,
  state: "failed",
  lines: [],
  left_out: [],
  notepad_text: "",
  progress_step: 2,
  progress_total: 5,
  error: "model unavailable",
};

describe("failed meeting transcript truth", () => {
  test("static render with empty segments renders zero transcript items", async () => {
    const i18n = await createTestI18n();
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <MeetingNotesSection
          recording={false}
          snapshot={FAILED_SNAPSHOT}
          notepadDraft=""
          segments={[]}
          onNotepadChange={() => {}}
          onJumpToSegment={() => {}}
          onRetry={() => {}}
        />
      </I18nextProvider>,
    );

    const sectionMatch = html.match(
      /<section[^>]*data-testid="notes-failed-transcript"[^>]*>([\s\S]*?)<\/section>/,
    );
    expect(sectionMatch).not.toBeNull();
    const listItems = sectionMatch![1].match(/<li[\s>]/g) ?? [];
    expect(listItems.length).toBe(0);
  });

  test("failed transcript loader handles error, throw, ok empty, and ok with lines", async () => {
    const errRes = await loadFailedTranscriptSegments(42, async () => ({
      status: "error",
      error: "db read failed",
    }));
    expect(errRes).toEqual({ segments: [], loadFailed: true });

    const throwRes = await loadFailedTranscriptSegments(42, async () => {
      throw new Error("transport disconnected");
    });
    expect(throwRes).toEqual({ segments: [], loadFailed: true });

    const emptyRes = await loadFailedTranscriptSegments(42, async () => ({
      status: "ok",
      data: [],
    }));
    expect(emptyRes).toEqual({ segments: [], loadFailed: false });

    const sampleRow: MeetingSegmentView = {
      meeting_id: 42,
      channel: "me",
      start_ms: 1000,
      end_ms: 4000,
      text: "Saved speaker line.",
    };
    const okRes = await loadFailedTranscriptSegments(42, async () => ({
      status: "ok",
      data: [sampleRow],
    }));
    expect(okRes).toEqual({ segments: [sampleRow], loadFailed: false });
  });

  test("source tree contains zero occurrences of FALLBACK_FAILED_SEGMENTS", () => {
    let hits = 0;
    for (const file of listFiles(join(root, "src"))) {
      const content = readFileSync(file, "utf-8");
      const matches = content.match(/FALLBACK_FAILED_SEGMENTS/g);
      if (matches) hits += matches.length;
    }
    expect(hits).toBe(0);
  });
});

describe("optimistic part delete truth", () => {
  test("optimistic command helper covers ok, error, and throw", async () => {
    let applied = 0;
    let undone = 0;
    const okRes = await runOptimisticCommand(
      () => {
        applied += 1;
      },
      async () => ({ status: "ok", data: true }),
      () => {
        undone += 1;
      },
    );
    expect(applied).toBe(1);
    expect(undone).toBe(0);
    expect(okRes).toEqual({ ok: true, data: true });

    applied = 0;
    undone = 0;
    const errRes = await runOptimisticCommand(
      () => {
        applied += 1;
      },
      async () => ({ status: "error", error: "failed" }),
      () => {
        undone += 1;
      },
    );
    expect(applied).toBe(1);
    expect(undone).toBe(1);
    expect(errRes).toEqual({ ok: false });

    applied = 0;
    undone = 0;
    const throwRes = await runOptimisticCommand(
      () => {
        applied += 1;
      },
      async () => {
        throw new Error("ipc down");
      },
      () => {
        undone += 1;
      },
    );
    expect(applied).toBe(1);
    expect(undone).toBe(1);
    expect(throwRes).toEqual({ ok: false });
  });

  test("MeetingsList has no unchecked deleteTranscriptSpanSecure statement", () => {
    const src = readFileSync(
      join(root, "src/components/meetings/MeetingsList.tsx"),
      "utf-8",
    );
    expect(
      /^\s*await\s+meetingCommands\.deleteTranscriptSpanSecure\(/m.test(src),
    ).toBe(false);
  });
});

describe("trash restore, delete-now, and keep-time truth", () => {
  const sampleTrash: TrashedMeetingItem[] = [
    { id: 10, title: "Design sync", deletedAt: 1_700_000_000 },
    { id: 11, title: "Sprint review", deletedAt: 1_700_000_100 },
  ];

  test("restore error puts the item back and sets restoreFailed", async () => {
    let items = [...sampleTrash];
    const setTrashed: React.Dispatch<
      React.SetStateAction<TrashedMeetingItem[]>
    > = (updater) => {
      items = typeof updater === "function" ? updater(items) : updater;
    };
    let errorKey: string | null = null;
    const ok = await restoreTrashedMeetingAction(
      10,
      items,
      setTrashed,
      (k) => {
        errorKey = k;
      },
      async () => ({ status: "error", error: "restore failed" }),
    );
    expect(ok).toBe(false);
    expect(items).toEqual(sampleTrash);
    expect(errorKey).toBe("meetings.storage.restoreFailed");
  });

  test("delete-now handles error rollback, audio-left warning true, and false", async () => {
    let items = [...sampleTrash];
    const setTrashed: React.Dispatch<
      React.SetStateAction<TrashedMeetingItem[]>
    > = (updater) => {
      items = typeof updater === "function" ? updater(items) : updater;
    };
    let errorKey: string | null = null;
    let audioWarn = false;
    const errOk = await deleteTrashedMeetingNowAction(
      10,
      items,
      setTrashed,
      (k) => {
        errorKey = k;
      },
      (w) => {
        audioWarn = w;
      },
      async () => ({ status: "error", error: "delete failed" }),
    );
    expect(errOk).toBe(false);
    expect(items).toEqual(sampleTrash);
    expect(errorKey).toBe("meetings.list.deleteFailed");
    expect(audioWarn).toBe(false);

    items = [...sampleTrash];
    errorKey = null;
    audioWarn = false;
    const warnOk = await deleteTrashedMeetingNowAction(
      10,
      items,
      setTrashed,
      (k) => {
        errorKey = k;
      },
      (w) => {
        audioWarn = w;
      },
      async () => ({ status: "ok", data: true }),
    );
    expect(warnOk).toBe(true);
    expect(items.map((x) => x.id)).toEqual([11]);
    expect(errorKey).toBeNull();
    expect(audioWarn).toBe(true);

    items = [...sampleTrash];
    errorKey = null;
    audioWarn = true;
    const cleanOk = await deleteTrashedMeetingNowAction(
      10,
      items,
      setTrashed,
      (k) => {
        errorKey = k;
      },
      (w) => {
        audioWarn = w;
      },
      async () => ({ status: "ok", data: false }),
    );
    expect(cleanOk).toBe(true);
    expect(items.map((x) => x.id)).toEqual([11]);
    expect(errorKey).toBeNull();
    expect(audioWarn).toBe(false);
  });

  test("keep-time error restores previous value and sets actionFailed", async () => {
    let currentDays: number | null = 30;
    let errorKey: string | null = null;
    const ok = await changeTranscriptKeepDaysAction(
      7,
      currentDays,
      (d) => {
        currentDays = d;
      },
      (k) => {
        errorKey = k;
      },
      async () => ({ status: "error", error: "save failed" }),
    );
    expect(ok).toBe(false);
    expect(currentDays).toBe(30);
    expect(errorKey).toBe("meetings.retention.actionFailed");
  });

  test("MeetingHistoryExtras has zero fire-and-forget void calls for trash and keep-time commands", () => {
    const src = readFileSync(
      join(root, "src/components/meetings/MeetingHistoryExtras.tsx"),
      "utf-8",
    );
    expect(src).not.toContain("void meetingCommands.restoreTrashedMeetingV2");
    expect(src).not.toContain("void meetingCommands.deleteMeetingForever");
    expect(src).not.toContain(
      "void meetingCommands.changeTranscriptDeleteDaysSetting",
    );
  });
});
