import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { renderToStaticMarkup } from "react-dom/server";
import en from "../src/i18n/locales/en/translation.json";
import type { UpcomingMeeting } from "../src/components/meetings/upcoming";
import { UpcomingMeetingsList } from "../src/components/meetings/UpcomingMeetings";
import { CalendarPickerView } from "../src/components/settings/CalendarPicker";

const root = join(import.meta.dir, "..");

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});
const render = (node: React.ReactElement) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

// Synthetic meeting, never read from a real calendar.
const standup: UpcomingMeeting = {
  title: "Team standup",
  start: 1_800_000_000,
  end: 1_800_001_800,
  calendar_name: "Home",
  attendee_display_names: ["Ann", "Bo"],
};

test("the Coming up list shows title, calendar and who is invited", () => {
  const html = render(
    <UpcomingMeetingsList
      meetings={[standup]}
      hasCalendars={true}
      startNotesAvailable={true}
      onStartNotes={() => {}}
    />,
  );
  expect(html).toContain("Coming up");
  expect(html).toContain("Team standup");
  expect(html).toContain("Home");
  expect(html).toContain("With Ann, Bo");
  expect(html).toContain("Start notes");
});

test("the Coming up list says to pick calendars when none are picked", () => {
  const html = render(
    <UpcomingMeetingsList
      meetings={[]}
      hasCalendars={false}
      startNotesAvailable={true}
      onStartNotes={() => {}}
    />,
  );
  expect(html.replace(/&gt;/g, ">")).toContain(
    en.meetings.upcoming.noneSelected,
  );
  expect(html).not.toContain("Start notes");
});

test("the Start notes button is off while no consent reminder is wired in", () => {
  const html = render(
    <UpcomingMeetingsList
      meetings={[standup]}
      hasCalendars={true}
      startNotesAvailable={false}
      onStartNotes={() => {}}
    />,
  );
  expect(html).toMatch(/<button[^>]*disabled=""/);
});

const choices = [
  { id: "h", title: "Home", account: "iCloud", blocked: null },
  { id: "w", title: "Team", account: "Exchange", blocked: "work" as const },
];

test("no calendar is checked by default", () => {
  const html = render(
    <CalendarPickerView choices={choices} selected={[]} onToggle={() => {}} />,
  );
  expect(html).not.toContain("checked");
  expect(html.match(/type="checkbox"/g)?.length).toBe(2);
});

test("a work calendar row is disabled and says why, even if saved as picked", () => {
  const html = render(
    <CalendarPickerView
      choices={choices}
      selected={["w", "h"]}
      onToggle={() => {}}
    />,
  );
  const rows = html.split("<li").slice(1);
  expect(rows[0]).toContain('checked=""');
  expect(rows[1]).toContain('disabled=""');
  expect(rows[1]).not.toContain('checked=""');
  expect(rows[1]).toContain(en.meetings.calendar.blockedWork);
});

test("the picker explains the data stays on this Mac", () => {
  expect(en.meetings.calendar.description).toContain("stays on this Mac");
  expect(en.meetings.calendar.description).toContain("only reads");
});

test("calendar ui code never logs, never calls a model, never starts capture itself", () => {
  const files = [
    "src/components/meetings/upcoming.ts",
    "src/components/meetings/UpcomingMeetings.tsx",
    "src/components/settings/CalendarPicker.tsx",
  ];
  for (const f of files) {
    const text = readFileSync(join(root, f), "utf8");
    for (const banned of [
      "console.",
      "fetch(",
      "localStorage",
      "sessionStorage",
      "post_process",
      "llm",
      "startRecording",
    ]) {
      expect(text.includes(banned), `${f} uses ${banned}`).toBe(false);
    }
  }
  // The only way the list begins notes is through the consent flow.
  const list = readFileSync(
    join(root, "src/components/meetings/UpcomingMeetings.tsx"),
    "utf8",
  );
  expect(list).toContain("startNotesFlow(");
  expect(list).not.toContain("deps.beginNotes(");
});
