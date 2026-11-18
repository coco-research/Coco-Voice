import { expect, mock, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";

// Sidebar holds page references but never mounts their content.
mock.module("../src/components/settings", () => ({
  GeneralSettings: () => null, AdvancedSettings: () => null,
  HistorySettings: () => null, DebugSettings: () => null,
  AboutSettings: () => null, PostProcessingSettings: () => null,
  ModelsSettings: () => null,
}));
const { Sidebar } = await import("../src/components/Sidebar");
const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en", resources: { en: { translation: en } },
});

const render = (page = "home", showMeetings = true) => renderToStaticMarkup(
  <I18nextProvider i18n={i18n}>
    <Sidebar activeSection={page} onSectionChange={() => {}} showMeetings={showMeetings} />
  </I18nextProvider>,
);
const ids = (html: string) => [...html.matchAll(/data-nav-id="([^"]+)"/g)].map(m => m[1]);

test("enabled navigation separates meetings, dictation and shared settings in the approved order", () => {
  const html = render();
  expect(ids(html)).toEqual([
    "home", "meetings-record", "meetings-history", "meetings-ask",
    "meetings-folders", "meetings-agent", "voice-history", "voice-profiles",
    "voice-words", "models", "settings", "about",
  ]);
  const positions = ['data-nav-id="home"', 'data-nav-heading="meetings"',
    'data-nav-id="meetings-history"', 'data-nav-heading="voice"',
    'data-nav-id="voice-history"', 'data-nav-group="shared"'].map(s => html.indexOf(s));
  expect(positions.every((p, i) => p >= 0 && (i === 0 || p > positions[i - 1]!))).toBe(true);
  expect(html).toContain("Dictation history");
  expect(html).toContain("Meeting history");
});

test("an unavailable meetings capability leaves dictation and shared pages reachable", () => {
  const html = render("voice-history", false);
  expect(ids(html).some(id => id.startsWith("meetings-"))).toBe(false);
  expect(html).not.toContain('data-nav-heading="meetings"');
  expect(ids(html)).toEqual(["home", "voice-history", "voice-profiles", "voice-words", "models", "settings", "about"]);
});

test("every route remains a native button and only the selected page is current", () => {
  for (const page of ["home", "meetings-history", "voice-history", "settings", "about"]) {
    const buttons = render(page).match(/<button\b[^>]*>/g) ?? [];
    expect(buttons).toHaveLength(12);
    expect(buttons.every(b => b.includes('type="button"'))).toBe(true);
    const selected = buttons.filter(b => b.includes('aria-current="page"'));
    expect(selected).toHaveLength(1);
    expect(selected[0]).toContain(`data-nav-id="${page}"`);
  }
});
