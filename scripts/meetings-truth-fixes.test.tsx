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
