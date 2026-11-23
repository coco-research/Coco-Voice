import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { UpcomingMeetingsList } from "../src/components/meetings/UpcomingMeetings";
import { RecordMeetingSection } from "../src/components/meetings/RecordMeetingPage";

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  interpolation: { escapeValue: false },
  resources: { en: { translation: en } },
});

const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

const NOW = Math.floor(new Date(2026, 9, 27, 13, 35, 0).getTime() / 1000);
const EVENTS = [
  {
    title: "Design review",
    start: NOW + 25 * 60,
    end: NOW + 55 * 60,
    calendar_name: "Personal",
    attendee_display_names: ["Dana"],
  },
  {
    title: "Q3 planning sync",
    start: NOW + 90 * 60,
    end: NOW + 120 * 60,
    calendar_name: "Personal",
    attendee_display_names: ["Alex"],
  },
  {
    title: "Release readiness",
    start: NOW + 180 * 60,
    end: NOW + 210 * 60,
    calendar_name: "Personal",
    attendee_display_names: ["Sam"],
  },
];

describe("W32 & W33 / AC-X47 (B2 Amendment 5): Coming up card on Record a meeting & standalone Coming up view", () => {
  test("card renders B1 line without '(frame 10)', lists event row, and triggers onOpenView", () => {
    let opened = false;
    const cardHtml = wrap(
      <UpcomingMeetingsList
        mode="card"
        meetings={[EVENTS[0]!]}
        hasCalendars={true}
        nowSecs={NOW}
        onStartNotes={() => {}}
        onOpenView={() => {
          opened = true;
        }}
      />,
    );

    expect(cardHtml).toContain("From the calendar on this Mac, once you turn it on.");
    expect(cardHtml).not.toContain("(frame 10)");
    expect(cardHtml).not.toContain("Nothing starts by itself.");
    expect(cardHtml).toContain("Design review");
    expect(cardHtml).toContain("in 25 min");
    expect(cardHtml).toContain("Start notes");
    expect(cardHtml).toContain('data-testid="coming-up-open-view"');

    // Verify RecordMeetingSection switches into standalone Coming up view when comingUpOpen=true
    const pageViewHtml = wrap(
      <RecordMeetingSection
        offerEnabled={true}
        offer={null}
        consentOpen={false}
        comingUpOpen={true}
        status={{
          state: "idle",
          elapsed_secs: 0,
          meeting_id: null,
          mic_name: null,
          warnings: [],
        }}
        segments={[]}
        meetingWords={[]}
        onToggleOffer={() => {}}
        onOpenConsent={() => {}}
        onConfirmStart={() => {}}
        onCancelConsent={() => {}}
        onDismissOffer={() => {}}
        onStopMeeting={() => {}}
        onAddMeetingWord={() => {}}
        onRemoveMeetingWord={() => {}}
      />,
    );
    expect(pageViewHtml).toContain('data-testid="page-2-coming-up-view"');
    expect(opened).toBe(false);
  });

  test("standalone Coming up view renders eyebrow, title, '3 of 3', 'Nothing starts by itself.', and flat rows with NO per-row bordered card", () => {
    const viewHtml = wrap(
      <UpcomingMeetingsList
        mode="page"
        meetings={EVENTS}
        hasCalendars={true}
        onStartNotes={() => {}}
      />,
    );

    expect(viewHtml).toContain("Meetings");
    expect(viewHtml).toContain("Coming up");
    expect(viewHtml).toContain("3 of 3");
    expect(viewHtml).toContain("Nothing starts by itself.");
    expect(viewHtml).toContain('data-testid="coming-up-flat-list"');
    expect(viewHtml).toContain("divide-y");

    // Extract <li> opening tags and verify zero per-row rounded/bordered card classes.
    const liTags = viewHtml.match(/<li\b[^>]*>/g) ?? [];
    expect(liTags.length).toBe(3);
    for (const tag of liTags) {
      expect(tag).not.toContain("rounded-[10px]");
      expect(tag).not.toContain("border-mid-gray");
    }
  });

  test("with no calendars picked, BOTH card and standalone view render D-X6 words and Open button navigates to Settings › Meetings", () => {
    let settingsOpens = 0;
    const onOpenSettings = () => {
      settingsOpens += 1;
    };

    const cardEmpty = wrap(
      <UpcomingMeetingsList
        mode="card"
        meetings={[]}
        hasCalendars={false}
        onStartNotes={() => {}}
        onOpenSettings={onOpenSettings}
      />,
    );
    const viewEmpty = wrap(
      <UpcomingMeetingsList
        mode="page"
        meetings={[]}
        hasCalendars={false}
        onStartNotes={() => {}}
        onOpenSettings={onOpenSettings}
      />,
    );

    const expected = "Pick calendars in Settings › Meetings to see what&#x27;s coming up.";
    expect(cardEmpty).toContain(expected);
    expect(cardEmpty).toContain('data-testid="coming-up-open-settings"');
    expect(cardEmpty).toContain(">Open<");
    expect(viewEmpty).toContain(expected);
    expect(viewEmpty).toContain('data-testid="coming-up-open-settings"');
    expect(viewEmpty).toContain(">Open<");

    onOpenSettings();
    expect(settingsOpens).toBe(1);
  });
});
