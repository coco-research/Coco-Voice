import { describe, expect, mock, test } from "bun:test";
import fs from "node:fs";
import path from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";

const MARKER_TITLE = "E-F10-MARKER-TITLE";
const MARKER_ATTENDEE = "E-F10-MARKER-ATTENDEE";
const MARKER_CALENDAR = "E-F10-MARKER-CALENDAR";
const MARKERS = [MARKER_TITLE, MARKER_ATTENDEE, MARKER_CALENDAR] as const;

const FIXTURE_EVENTS = [
  {
    title: MARKER_TITLE,
    start: 1794960000,
    end: 1794961800,
    calendar_name: MARKER_CALENDAR,
    attendee_display_names: [MARKER_ATTENDEE],
  },
];

interface RecordedInvoke {
  cmd: string;
  args: unknown;
}

const invokeCalls: RecordedInvoke[] = [];
const networkCalls: string[] = [];

mock.module("@tauri-apps/api/core", () => ({
  transformCallback: () => 0,
  Channel: class {},
  convertFileSrc: (s: string) => s,
  invoke: async (cmd: string, args?: unknown) => {
    invokeCalls.push({ cmd, args });
    if (cmd === "list_upcoming_meetings") {
      return FIXTURE_EVENTS;
    }
    if (cmd === "keep_upcoming_for_notes") {
      return FIXTURE_EVENTS[0];
    }
    if (cmd === "close_upcoming_meetings") {
      return null;
    }
    return null;
  },
}));

mock.module("@tauri-apps/plugin-http", () => ({
  fetch: (...args: unknown[]) => {
    networkCalls.push(`tauri-plugin-http.fetch:${JSON.stringify(args)}`);
    throw new Error("Unexpected Tauri HTTP plugin call during Coming up read");
  },
}));

const { commands } = await import("../src/bindings.meetings");
const { UpcomingMeetingsList } =
  await import("../src/components/meetings/UpcomingMeetings");

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

describe("E-F10 / AC-X43: Coming up frontend network and invoke boundary", () => {
  test("Coming up makes zero network calls and marker names reach only Coming up list and Start notes invokes", async () => {
    invokeCalls.length = 0;
    networkCalls.length = 0;

    const g = globalThis as Record<string, unknown>;
    const origFetch = g.fetch;
    const origXHR = g.XMLHttpRequest;
    const origWS = g.WebSocket;
    const origES = g.EventSource;

    g.fetch = (...args: unknown[]) => {
      networkCalls.push(`fetch:${JSON.stringify(args)}`);
      throw new Error("Unexpected fetch during Coming up");
    };
    g.XMLHttpRequest = class SpyXHR {
      constructor() {
        networkCalls.push("XMLHttpRequest");
        throw new Error("Unexpected XMLHttpRequest during Coming up");
      }
    };
    g.WebSocket = class SpyWebSocket {
      constructor(url: unknown) {
        networkCalls.push(`WebSocket:${String(url)}`);
        throw new Error("Unexpected WebSocket during Coming up");
      }
    };
    g.EventSource = class SpyEventSource {
      constructor(url: unknown) {
        networkCalls.push(`EventSource:${String(url)}`);
        throw new Error("Unexpected EventSource during Coming up");
      }
    };

    try {
      // 1. Fetch fixture events through the real Coming up binding.
      const listed = await commands.listUpcomingMeetings();
      expect(listed.status).toBe("ok");
      if (listed.status !== "ok") return;
      expect(listed.data).toEqual(FIXTURE_EVENTS);

      // 2. Render Coming up with the fixture events carrying all 3 marker names.
      let startedWith: (typeof FIXTURE_EVENTS)[number] | null = null;
      const html = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <UpcomingMeetingsList
            meetings={listed.data}
            hasCalendars={true}
            onStartNotes={(m) => {
              startedWith = m;
            }}
          />
        </I18nextProvider>,
      );

      expect(html).toContain('data-testid="frame-10-coming-up"');
      expect(html).toContain(MARKER_TITLE);
      expect(html).toContain(MARKER_CALENDAR);
      expect(html).toContain(MARKER_ATTENDEE);

      // 3. Simulate Start notes handoff and close lifecycle (exact UpcomingMeetings calls).
      const target = listed.data[0]!;
      startedWith = target;
      const kept = await commands.keepUpcomingForNotes(
        startedWith.start,
        startedWith.title,
        startedWith.calendar_name,
      );
      expect(kept.status).toBe("ok");
      await commands.closeUpcomingMeetings();

      // 4. Assert ZERO network calls across fetch, XHR, WebSocket, EventSource, Tauri HTTP plugin.
      expect(networkCalls).toEqual([]);

      // 5. Assert marker names reach NO invoke other than Coming up list and Start notes.
      const allowedCalendarInvokes = new Set([
        "list_upcoming_meetings",
        "keep_upcoming_for_notes",
        "close_upcoming_meetings",
      ]);
      for (const call of invokeCalls) {
        expect(allowedCalendarInvokes.has(call.cmd)).toBe(true);
        const serializedArgs = JSON.stringify(call.args ?? "");
        const hasMarker = MARKERS.some((m) => serializedArgs.includes(m));
        if (hasMarker) {
          expect(call.cmd).toBe("keep_upcoming_for_notes");
        }
        // Attendee display names must never leave the UI via any invoke arg.
        expect(serializedArgs).not.toContain(MARKER_ATTENDEE);
      }

      // 6. Source guard on Coming up frontend files: zero direct network primitives.
      const upcomingFiles = [
        "src/components/meetings/UpcomingMeetings.tsx",
        "src/components/meetings/upcoming.ts",
      ];
      const repoRoot = path.resolve(import.meta.dir, "..");
      for (const rel of upcomingFiles) {
        const src = fs.readFileSync(path.join(repoRoot, rel), "utf8");
        for (const banned of [
          "fetch(",
          "XMLHttpRequest",
          "WebSocket",
          "EventSource",
          "plugin-http",
        ]) {
          expect(src).not.toContain(banned);
        }
      }
    } finally {
      g.fetch = origFetch;
      g.XMLHttpRequest = origXHR;
      g.WebSocket = origWS;
      g.EventSource = origES;
    }
  });
});
