import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
const SRC_DIR = join(ROOT, "src");
const TAURI_SRC_DIR = join(ROOT, "src-tauri", "src");

function walkFiles(dir: string, exts: string[]): string[] {
  const results: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      results.push(...walkFiles(full, exts));
    } else if (exts.some((ext) => full.endsWith(ext))) {
      results.push(full);
    }
  }
  return results;
}

function extractConstStrings(source: string, pattern: RegExp): Set<string> {
  const values = new Set<string>();
  for (const match of source.matchAll(pattern)) {
    if (match[1]) values.add(match[1]);
  }
  return values;
}

describe("Plan B1 static contracts (AC-B11, AC-B3, AC-B16)", () => {
  test("AC-B11: Rust and frontend meeting event names match 1:1 with zero legacy meetings:// listeners", () => {
    const pipelineRs = readFileSync(
      join(TAURI_SRC_DIR, "meeting_pipeline.rs"),
      "utf8",
    );
    const controllerRs = readFileSync(
      join(TAURI_SRC_DIR, "meeting_controller.rs"),
      "utf8",
    );
    const meetingEventsTs = readFileSync(
      join(SRC_DIR, "components", "meetings", "meetingEvents.ts"),
      "utf8",
    );
    const recordPageTs = readFileSync(
      join(SRC_DIR, "components", "meetings", "RecordMeetingPage.tsx"),
      "utf8",
    );

    const rustEvents = new Set<string>([
      ...extractConstStrings(
        pipelineRs,
        /pub const [A-Z0-9_]+_EVENT:\s*&str\s*=\s*"([^"]+)"/g,
      ),
      ...extractConstStrings(
        controllerRs,
        /pub const [A-Z0-9_]+_EVENT:\s*&str\s*=\s*"([^"]+)"/g,
      ),
    ]);

    const tsEvents = extractConstStrings(
      meetingEventsTs,
      /export const [A-Z0-9_]+_EVENT\s*=\s*"([^"]+)"/g,
    );

    expect(tsEvents.size).toBe(10);
    expect([...tsEvents].sort()).toEqual([...rustEvents].sort());

    // RecordMeetingPage must use canonical event constants for live segments and notes lifecycle.
    expect(recordPageTs).toContain("MEETING_SEGMENT_EVENT");
    expect(recordPageTs).toContain("MEETING_NOTES_PROGRESS_EVENT");
    expect(recordPageTs).toContain("MEETING_NOTES_SO_FAR_EVENT");
    expect(recordPageTs).toContain("MEETING_NOTES_READY_EVENT");
    expect(recordPageTs).toContain("MEETING_NOTES_FAILED_EVENT");

    // Zero legacy `meetings://` event listeners anywhere in src/.
    const tsFiles = walkFiles(SRC_DIR, [".ts", ".tsx"]);
    const legacyHits: string[] = [];
    for (const file of tsFiles) {
      const content = readFileSync(file, "utf8");
      if (content.includes("meetings://")) {
        legacyHits.push(file);
      }
    }
    expect(legacyHits).toEqual([]);
  });

  test("AC-B3: meeting_pipeline.rs stays 100% local with zero HTTP or cloud provider references", () => {
    const pipelineRs = readFileSync(
      join(TAURI_SRC_DIR, "meeting_pipeline.rs"),
      "utf8",
    );
    const forbidden = [
      "reqwest",
      "http://",
      "https://",
      "openai",
      "anthropic",
      "openrouter",
      "groq",
      "cerebras",
      "gemini",
      "mistral",
    ];
    for (const token of forbidden) {
      expect(pipelineRs.toLowerCase()).not.toContain(token);
    }
  });

  test("AC-B16: hardcoded 'Product sync' sample title appears nowhere in src/ source files", () => {
    const tsFiles = walkFiles(SRC_DIR, [".ts", ".tsx"]);
    const hits: string[] = [];
    for (const file of tsFiles) {
      const content = readFileSync(file, "utf8");
      if (content.includes("Product sync")) {
        hits.push(file);
      }
    }
    expect(hits).toEqual([]);
  });
});
