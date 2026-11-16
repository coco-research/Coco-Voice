import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import {
  HOME_RECENT_LIMIT,
  HomeViewSection,
  type HomeDictationItem,
} from "../src/components/home/HomeView";
import type { MeetingListItem } from "../src/components/meetings/MeetingsList";

const i18n = i18next.createInstance();
void i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const FAKE_MEETINGS: MeetingListItem[] = [
  {
    id: 3,
    title: "Product sync",
    started_at: 1_700_003_600,
    ended_at: 1_700_005_400,
    notes_ready: true,
  },
  {
    id: 2,
    title: "Design critique",
    started_at: 1_700_000_000,
    ended_at: 1_700_002_700,
    notes_ready: true,
  },
  {
    id: 1,
    title: "Third older meeting",
    started_at: 1_699_990_000,
    ended_at: 1_699_990_900,
    notes_ready: false,
  },
];

const FAKE_DICTATIONS: HomeDictationItem[] = [
  {
    id: 10,
    text: "Can we move the review to Thursday afternoon?",
    appName: "Mail",
    time: "9:41",
  },
  {
    id: 9,
    text: "Add a test for the empty list case.",
    appName: "Cursor",
    time: "9:12",
  },
  {
    id: 8,
    text: "Third older dictation hidden on Home",
    appName: "Notes",
    time: "8:00",
  },
];

describe("HomeView (V-L2 / v0.2.2 Page 1)", () => {
  test("renders compact Record a meeting and Dictate anywhere cards and omits removed headings/descriptions", () => {
    expect(HOME_RECENT_LIMIT).toBe(2);
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <HomeViewSection
          shortcut="fn"
          meetings={FAKE_MEETINGS}
          dictations={FAKE_DICTATIONS}
          onStartRecording={() => {}}
          onNavigate={() => {}}
        />
      </I18nextProvider>,
    ).replaceAll("&#x27;", "'");

    // 4 removed items per v0.2.2 Page 1 design call:
    expect(html).not.toContain("Good morning");
    expect(html).not.toContain("Notes and a transcript");
    expect(html).not.toContain("Hold your shortcut");
    expect(html).not.toContain("Recent meetings");
    expect(html).not.toContain("Recent dictations");

    const mIdx = html.indexOf('data-home-card="meetings"');
    const vIdx = html.indexOf('data-home-card="voice"');
    expect(mIdx).toBeGreaterThan(-1);
    expect(vIdx).toBeGreaterThan(mIdx);

    const mCard = html.slice(mIdx, vIdx);
    const vCard = html.slice(vIdx);
    expect(mCard).not.toContain(">Meetings<");
    expect(vCard).not.toContain(">Voice<");

    expect(mCard).toContain("Record a meeting");
    expect(mCard).toContain("Start recording");
    expect(mCard).toContain("Product sync");
    expect(mCard).toContain("Design critique");
    expect(mCard).not.toContain("Third older meeting");
    expect(mCard).toContain("All meetings →");
    expect(mCard).not.toContain("Add a test for the empty list case");

    expect(vCard).toContain("Dictate anywhere");
    expect(vCard).toContain(">fn</kbd>");
    expect(vCard).toContain("hold to talk");
    expect(vCard).toContain("Change shortcut");
    expect(vCard).toContain("Can we move the review to Thursday");
    expect(vCard).toContain(">Mail<");
    expect(vCard).toContain("9:41");
    expect(vCard).toContain("Add a test for the empty list case.");
    expect(vCard).not.toContain("Third older dictation");
    expect(vCard).toContain("All dictations →");
    expect(vCard).not.toContain("Product sync");
  });
});
