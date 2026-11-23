import { describe, expect, mock, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { HistoryEntry } from "../src/bindings";

mock.module("../src/components/whats-new/releaseNotes", () => ({
  findReleaseNoteToShow: () => null,
  findLatestReleaseNote: () => null,
}));
mock.module("../src/i18n", () => ({
  default: i18next,
  SUPPORTED_LANGUAGES: [{ code: "en", name: "English", nativeName: "English" }],
  syncLanguageFromSettings: async () => {},
  getLanguageDirection: () => "ltr",
  isRTLLanguage: () => false,
}));

const { SHELL_ROUTES, Sidebar } = await import(
  "../src/components/layout/shellRoutes"
);
const {
  HistorySettings,
  resolveKeepDictationsChoice,
  selectDictationEntries,
} = await import("../src/components/settings/history/HistorySettings");

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

  test("renders Keep dictations 4 buttons, Keep the audio switch, dynamic header badge, legacy choices, and hides Play on text-only rows", () => {
    expect(resolveKeepDictationsChoice(true, "days_30")).toBe("never");
    expect(resolveKeepDictationsChoice(false, "hours_24")).toBe("hours_24");
    expect(resolveKeepDictationsChoice(false, "days_30")).toBe("days_30");
    // Legacy Rust "never" ("never auto-delete") maps to Forever when never_store_history is false.
    expect(resolveKeepDictationsChoice(false, "never")).toBe("forever");
    // Legacy choices stay distinct so no drawn button lights up.
    expect(resolveKeepDictationsChoice(false, "preserve_limit")).toBe(
      "preserve_limit",
    );
    expect(resolveKeepDictationsChoice(false, "days_3")).toBe("days_3");
    expect(resolveKeepDictationsChoice(false, "weeks_2")).toBe("weeks_2");
    expect(resolveKeepDictationsChoice(false, "months_3")).toBe("months_3");

    const g = globalThis as { window?: unknown };
    const prev = Object.getOwnPropertyDescriptor(globalThis, "window");
    g.window = { __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" } };
    try {
      const neverHtml = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <HistorySettings
            initialEntries={[]}
            initialNeverStore={true}
            initialRetentionPeriod="days_30"
            initialKeepAudio={false}
            nowTimestamp={REF_NOW}
          />
        </I18nextProvider>,
      );
      expect(neverHtml).toContain("Keep dictations");
      expect(neverHtml).toContain('role="radiogroup"');
      expect(neverHtml).toContain(">Never</button>");
      expect(neverHtml).toContain(">24 hours</button>");
      expect(neverHtml).toContain(">30 days</button>");
      expect(neverHtml).toContain(">Forever</button>");
      expect(neverHtml).toContain("Keep the audio");
      expect(neverHtml).toContain("Off: only the text is kept");
      expect(neverHtml).toContain(
        "Never: no text and no audio is written to disk. History stays empty.",
      );
      // Header badge is hidden under Never (D-C3) and must never show "Kept 30 days".
      expect(neverHtml).not.toContain("Kept 30 days");
      // Keep the audio switch stays usable (not disabled) in Never mode per C1 state d.
      expect(neverHtml).toContain('role="switch" aria-checked="false"');
      expect(neverHtml).not.toContain('role="switch" aria-checked="false" aria-label="Keep the audio" disabled');
      // When a drawn button is lit, Auto-Delete Recordings select is empty-but-neutral/muted and still clickable.
      expect(neverHtml).toContain('aria-label="Auto-Delete Recordings"');
      expect(neverHtml).toContain("opacity-60");
      expect(neverHtml).toContain('<option value="" selected=""></option>');
      expect(neverHtml).not.toContain('aria-label="Auto-Delete Recordings" disabled');

      // Legacy preserve_limit lights NO drawn button and shows "Keep latest 10".
      const legacyHtml = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <HistorySettings
            initialEntries={[]}
            initialNeverStore={false}
            initialRetentionPeriod="preserve_limit"
            initialHistoryLimit={10}
            initialKeepAudio={false}
            nowTimestamp={REF_NOW}
          />
        </I18nextProvider>,
      );
      expect(legacyHtml).toContain("Keep latest 10");
      expect(legacyHtml).not.toContain('role="radio" aria-checked="true"');
      expect(legacyHtml).not.toContain("opacity-60");

      const textOnlyEntry: HistoryEntry = {
        ...DICTATION_TAKES[0]!,
        file_name: "",
      };
      const textOnlyHtml = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <HistorySettings
            initialEntries={[textOnlyEntry]}
            initialNeverStore={false}
            initialRetentionPeriod="hours_24"
            initialKeepAudio={false}
            nowTimestamp={REF_NOW}
          />
        </I18nextProvider>,
      );
      expect(textOnlyHtml).toContain("Kept 24 hours");
      expect(textOnlyHtml).toContain(">Copy</button>");
      expect(textOnlyHtml).not.toContain(">Play</button>");
    } finally {
      if (prev) Object.defineProperty(globalThis, "window", prev);
      else delete g.window;
    }
  });
});
