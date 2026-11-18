import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { HistoryEntry } from "../src/bindings";
import {
  HomeSection,
  HomeView,
  homeDisplayText,
} from "../src/components/home/HomeView";
import { ShellContent } from "../src/components/layout/ShellContent";
import { normalizeShellSection } from "../src/components/layout/shellNavigation";

const i18n = i18next.createInstance();
await i18n
  .use(initReactI18next)
  .init({ lng: "en", resources: { en: { translation: en } } });
const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);
const item = (
  id: number,
  raw: string,
  cleaned: string | null = null,
): HistoryEntry => ({
  id,
  file_name: "fixture.wav",
  timestamp: 1_700_000_000,
  saved: false,
  title: "fixture",
  transcription_text: raw,
  post_processed_text: cleaned,
  post_process_prompt: null,
  post_process_requested: cleaned !== null,
});
const props = {
  shortcut: "Ctrl+Space",
  entries: [],
  loading: false,
  error: false,
  showMeetings: true,
  onNavigate: () => {},
};

test("Home separates two cards without presenting an enabled unsupported recording action", () => {
  const html = wrap(<HomeSection {...props} />);
  expect(html).toContain('data-home-card="meetings"');
  expect(html).toContain('data-home-card="voice"');
  expect(html).toMatch(
    /<button[^>]*disabled=""[^>]*>Start recording<\/button>/,
  );
  expect(html).toContain("Ctrl+Space");
  expect(html).toContain("Meeting recording is not available yet");
  expect(html).not.toContain("No meetings");
});

test("unavailable meetings capability leaves only the dictation card", () => {
  const html = wrap(<HomeSection {...props} showMeetings={false} />);
  expect(html).not.toContain('data-home-card="meetings"');
  expect(html).toContain('data-home-card="voice"');
});

test("recent dictations are bounded and display the text the user received", () => {
  const entries = [
    item(1, "raw first", "cleaned first"),
    item(2, "raw second", "  "),
    item(3, "third hidden"),
  ];
  const html = wrap(<HomeSection {...props} entries={entries} />);
  expect(html).toContain("cleaned first");
  expect(html).toContain("raw second");
  expect(html).not.toContain("raw first");
  expect(html).not.toContain("third hidden");
  expect(homeDisplayText(entries[1]!)).toBe("raw second");
});

test("loading and error views never display invalidated dictation text", () => {
  const entries = [item(1, "deleted fixture")];
  for (const state of [
    { loading: true, error: false },
    { loading: false, error: true },
  ]) {
    const html = wrap(<HomeSection {...props} {...state} entries={entries} />);
    expect(html).not.toContain("deleted fixture");
  }
});

test("an unset shortcut is shown truthfully rather than guessed as fn", () => {
  const html = wrap(<HomeView showMeetings={false} onNavigate={() => {}} />);
  expect(html).toContain("Shortcut not set");
  expect(html).not.toMatch(/<kbd[^>]*>fn<\/kbd>/);
});

test("meeting history slot is separate and does not claim an empty recorded history", () => {
  const html = wrap(
    <ShellContent
      section="meetings-history"
      meetingsUi={null}
      showMeetings={false}
      onNavigate={() => {}}
      about={null}
      debug={null}
    />,
  );
  expect(html).toContain('data-route-domain="meetings"');
  expect(html).not.toContain('data-route-domain="voice"');
  expect(html).toContain("Meetings is not available on this configuration");
  expect(html).not.toContain("No meetings");
});

test("dictation history uses the existing history page in its own route domain", () => {
  const before = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    writable: true,
    value: { __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" } },
  });
  try {
    const html = wrap(
      <ShellContent
        section="voice-history"
        meetingsUi={null}
        showMeetings={false}
        onNavigate={() => {}}
        about={null}
        debug={null}
      />,
    );
    expect(html).toContain('data-route-domain="voice"');
    expect(html).not.toContain('data-route-domain="meetings"');
  } finally {
    if (before) Object.defineProperty(globalThis, "window", before);
    else Reflect.deleteProperty(globalThis, "window");
  }
});

test("capability changes normalize selected routes to visible current pages", () => {
  expect(normalizeShellSection("meetings-history", false, true)).toBe("home");
  expect(normalizeShellSection("debug", true, false)).toBe("settings");
  expect(normalizeShellSection("voice-history", false, false)).toBe(
    "voice-history",
  );
  expect(normalizeShellSection("debug", true, true)).toBe("debug");
});
