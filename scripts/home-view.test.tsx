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
    appName: null,
    time: "9:12",
  },
  {
    id: 8,
    text: "Third older dictation hidden on Home",
    appName: "Notes",
    time: "8:00",
  },
];

describe("HomeView (V-L2 Page 1 + G8 Home notice)", () => {
  test("renders PAGE 1 Good morning, Meetings/Voice labels, descriptions, Recent headings, and at most 2 items each", () => {
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
    )
      .replaceAll("&#x27;", "'")
      .replaceAll("&quot;", '"');

    expect(html).toContain(">Home</p>");
    expect(html).toContain(">Good morning</h2>");
    expect(html).not.toContain("home-cleanup-notice");

    const mIdx = html.indexOf('data-home-card="meetings"');
    const vIdx = html.indexOf('data-home-card="voice"');
    expect(mIdx).toBeGreaterThan(-1);
    expect(vIdx).toBeGreaterThan(mIdx);

    const mCard = html.slice(mIdx, vIdx);
    const vCard = html.slice(vIdx);
    expect(mCard).toContain(">Meetings</p>");
    expect(mCard).toContain("Record a meeting");
    expect(mCard).toContain(
      "Notes and a transcript, made on this Mac. Recording starts only when you press Start.",
    );
    expect(mCard).toContain("Start recording");
    expect(mCard).toContain("Recent meetings");
    expect(mCard).toContain("Product sync");
    expect(mCard).toContain("Design critique");
    expect(mCard).not.toContain("Third older meeting");
    expect(mCard).toContain("All meetings →");

    expect(vCard).toContain(">Voice</p>");
    expect(vCard).toContain("Dictate anywhere");
    expect(vCard).toContain(
      "Hold your shortcut, speak, let go. The words are typed where your cursor is.",
    );
    expect(vCard).toContain(">fn</kbd>");
    expect(vCard).toContain("hold to talk");
    expect(vCard).toContain("Change shortcut");
    expect(vCard).toContain("Recent dictations");
    expect(vCard).toContain("Can we move the review to Thursday");
    expect(vCard).toContain(">Mail</span> · 9:41");
    expect(vCard).toContain(">9:12</p>");
    expect(vCard).not.toContain("Third older dictation");
    expect(vCard).toContain("All dictations →");
  });

  test("renders single V-R3 Home notice when on-device cleanup model is missing", () => {
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <HomeViewSection
          shortcut="fn"
          meetings={[]}
          dictations={[]}
          missingLocalModelNotice
          onDismissCleanupNotice={() => {}}
          onStartRecording={() => {}}
          onNavigate={() => {}}
        />
      </I18nextProvider>,
    ).replaceAll("&#x27;", "'");

    expect(html).toContain('data-testid="home-cleanup-notice"');
    expect(html).toContain("Not cleaned: no model");
    expect(html).toContain(
      "Cleanup is off: the on-device model isn't downloaded.",
    );
    expect(html).toContain("Download model");
  });
});
