import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { HistoryEntry } from "../src/bindings";
import { SHELL_ROUTES, Sidebar } from "../src/components/layout/shellRoutes";
import {
  HistorySettings,
  selectDictationEntries,
} from "../src/components/settings/history/HistorySettings";

const i18n = i18next.createInstance();
void i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const REF_NOW = Math.floor(new Date(2026, 9, 27, 11, 0, 0).getTime() / 1000);
const TODAY_941 = Math.floor(new Date(2026, 9, 27, 9, 41, 0).getTime() / 1000);
const YESTERDAY_1730 = Math.floor(
  new Date(2026, 9, 26, 17, 30, 0).getTime() / 1000,
);

const take = (
  id: number,
  timestamp: number,
  raw: string,
  cleaned: string | null,
  appName: string | null,
): HistoryEntry & { app_name: string | null } => ({
  id,
  file_name: `take-${id}.wav`,
  timestamp,
  saved: false,
  title: `Recording ${id}`,
  transcription_text: raw,
  post_processed_text: cleaned,
  post_process_prompt: cleaned ? "default" : null,
  post_process_requested: Boolean(cleaned),
  app_name: appName,
});

const DICTATION_TAKES = [
  take(
    11,
    TODAY_941,
    "can we move the review",
    "Can we move the review to Thursday afternoon?",
    "Mail",
  ),
  take(10, YESTERDAY_1730, "Thanks, see you at three.", null, null),
];

const SAVED_MEETING = {
  id: 99,
  title: "Saved executive sync meeting",
  transcription_text: "Saved executive sync meeting transcript",
  started_at: TODAY_941,
  ended_at: TODAY_941 + 1800,
  notes_ready: true,
  saved: true,
};

function renderDictation(entries: readonly unknown[], query = ""): string {
  const g = globalThis as { window?: unknown };
  const prev = Object.getOwnPropertyDescriptor(globalThis, "window");
  g.window = { __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" } };
  try {
    return renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <HistorySettings
          initialEntries={entries}
          initialQuery={query}
          nowTimestamp={REF_NOW}
        />
      </I18nextProvider>,
    )
      .replaceAll("&#x27;", "'")
      .replaceAll("&quot;", '"');
  } finally {
    if (prev) Object.defineProperty(globalThis, "window", prev);
    else delete g.window;
  }
}

describe("Dictation history under Voice (V-L4 / v0.2.2 Page 6)", () => {
  test("renders compact Page 6 rows (quoted text, mono app when set, Today/Yesterday time, Cleaned up only when cleaned, Copy/Play only) and never shows meetings", () => {
    const spec = SHELL_ROUTES.find((r) => r.id === "voice-history");
    expect(spec?.group).toBe("voice");
    expect(spec?.domain).toBe("voice");
    const nav = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <Sidebar activeSection="voice-history" onSectionChange={() => {}} />
      </I18nextProvider>,
    );
    expect(nav).toContain('data-nav-id="voice-history" aria-current="page"');

    const mixed = [...DICTATION_TAKES, SAVED_MEETING];
    expect(selectDictationEntries(mixed, "").map((e) => e.id)).toEqual([
      11, 10,
    ]);

    const html = renderDictation(mixed);
    expect(html).toContain('data-testid="dictation-history-section"');
    expect(html).toContain(">Voice<");
    expect(html).toContain("Dictation history");
    expect(html).toContain("Kept 30 days");
    expect(html).not.toContain("Open Recordings Folder");
    expect(html).not.toContain("<audio");
    expect(html).not.toContain("italic");
    expect(html).toContain('aria-label="Search dictations"');

    // Row 1: app_name = "Mail", Today 9:41, Cleaned up
    expect(html).toContain('>"Can we move the review to Thursday afternoon?"<');
    expect(html).toContain(">Mail</span> · Today 9:41 · Cleaned up<");

    // Row 2: app_name = null -> omits app segment, no Cleaned up
    expect(html).toContain('>"Thanks, see you at three."<');
    expect(html).toContain(">Yesterday 17:30</p>");
    expect(html).not.toContain("· Yesterday 17:30");
    expect(html).not.toContain("Yesterday 17:30 · Cleaned up");

    // Right side buttons: Copy and Play only
    expect(html).toContain(">Copy</button>");
    expect(html).toContain(">Play</button>");

    // Mixed-list guard: saved meeting never appears
    expect(html).not.toContain("Saved executive sync meeting");

    const searched = renderDictation(mixed, "three");
    expect(searched).toContain("Thanks, see you at three.");
    expect(searched).not.toContain("Thursday afternoon");
  });
});
