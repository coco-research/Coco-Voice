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
  test("renders v0.2.2 Good morning heading, Record a meeting and Dictate anywhere cards, descriptions, sub-labels, and arrow links", () => {
    expect(HOME_RECENT_LIMIT).toBe(2);
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <HomeViewSection
          shortcut="fn"
          meetings={FAKE_MEETINGS}
          dictations={FAKE_DICTATIONS}
          greetingDate={new Date(2026, 9, 27, 9, 30)}
          onStartRecording={() => {}}
          onNavigate={() => {}}
        />
      </I18nextProvider>,
    ).replaceAll("&#x27;", "'");

    expect(html).toContain("Good morning");
    expect(html).toContain(
      "Notes and a transcript, made on this Mac. Recording starts only when you press Start.",
    );
    expect(html).toContain(
      "Hold your shortcut, speak, let go. The words are typed where your cursor is.",
    );
    expect(html).toContain("Recent meetings");
    expect(html).toContain("Recent dictations");

    const mIdx = html.indexOf('data-home-card="meetings"');
    const vIdx = html.indexOf('data-home-card="voice"');
    expect(mIdx).toBeGreaterThan(-1);
    expect(vIdx).toBeGreaterThan(mIdx);

    const mCard = html.slice(mIdx, vIdx);
    const vCard = html.slice(vIdx);
    expect(mCard).toContain(">Meetings<");
    expect(vCard).toContain(">Voice<");

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

  test("renders strictly one highest-priority v0.2.2 Home notice when stale Accessibility and secondary notices are active", () => {
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <HomeViewSection
          shortcut="fn"
          meetings={FAKE_MEETINGS}
          dictations={FAKE_DICTATIONS}
          greetingDate={new Date(2026, 9, 27, 9, 30)}
          health={{
            staleAccessibility: true,
            shortcutUnset: true,
            typingOff: true,
            updateReadyVersion: "0.20.1",
          }}
          onStartRecording={() => {}}
          onNavigate={() => {}}
        />
      </I18nextProvider>,
    ).replaceAll("&#x27;", "'");

    expect(html).toContain(
      "After the update, turn on Coco Voice again in System Settings",
    );
    expect(html).toContain(
      "macOS kept the old entry. Until it is fixed, your words go to the clipboard.",
    );
    expect(html).toContain("Fix it (3 steps)");
    expect(html).not.toContain("Other Home notices");
    expect(html).not.toContain("Typing is off");
  });
});
