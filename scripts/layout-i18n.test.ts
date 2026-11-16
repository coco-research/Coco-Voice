import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const LOCALES_DIR = join(import.meta.dir, "../src/i18n/locales");

export const REQUIRED_LAYOUT_KEYS = [
  "sidebar.home",
  "meetings.sidebar",
  "sidebar.recordMeeting",
  "sidebar.meetingHistory",
  "sidebar.askMeetings",
  "sidebar.foldersAndTemplates",
  "sidebar.agentAccess",
  "sidebar.voiceSection",
  "sidebar.dictationHistory",
  "sidebar.perAppProfiles",
  "sidebar.wordsAndCleanup",
  "sidebar.models",
  "sidebar.settings",
  "sidebar.about",
  "sidebar.sectionPlaceholder",
] as const;

function getPath(obj: Record<string, unknown>, dotted: string): unknown {
  return dotted.split(".").reduce<unknown>((cur, seg) => {
    if (cur && typeof cur === "object" && seg in cur) {
      return (cur as Record<string, unknown>)[seg];
    }
    return undefined;
  }, obj);
}

describe("App layout v0.2.2 translations", () => {
  test("every locale defines non-empty layout navigation strings", () => {
    const locales = readdirSync(LOCALES_DIR).sort();
    expect(locales.length).toBe(22);
    for (const loc of locales) {
      const raw = JSON.parse(
        readFileSync(join(LOCALES_DIR, loc, "translation.json"), "utf8"),
      ) as Record<string, unknown>;
      for (const key of REQUIRED_LAYOUT_KEYS) {
        const val = getPath(raw, key);
        expect(typeof val, `${loc}:${key}`).toBe("string");
        expect((val as string).trim().length, `${loc}:${key}`).toBeGreaterThan(
          0,
        );
      }
      expect(getPath(raw, "sidebar.settings") as string).not.toMatch(/[.…]/);
    }
  });
});
