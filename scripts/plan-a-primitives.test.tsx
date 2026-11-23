import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import {
  Card,
  Notice,
  PageHeader,
  Row,
  Segmented,
  StatusDot,
  TimeChip,
  formatTimeChipLabel,
  nextSegmentedIndex,
} from "../src/components/ui";

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>{node}</I18nextProvider>,
  ).replaceAll("&#x27;", "'");

describe("Plan A Step A1 shared UI primitives", () => {
  test("StatusDot always pairs symbol with readable text for on, off, and working", () => {
    const onHtml = wrap(<StatusDot state="on" label="Notes ready" />);
    const offHtml = wrap(<StatusDot state="off" label="Not recording" />);
    const workHtml = wrap(<StatusDot state="working" label="Working" />);

    expect(onHtml).toContain("●");
    expect(onHtml).toContain("Notes ready");
    expect(offHtml).toContain("○");
    expect(offHtml).toContain("Not recording");
    expect(workHtml).toContain("◐");
    expect(workHtml).toContain("Working");
  });

  test("PageHeader renders eyebrow, title, status, and optional action", () => {
    const html = wrap(
      <PageHeader
        eyebrow="Meetings"
        title="Product sync"
        status={{ state: "on", label: "Notes ready" }}
        action={<button type="button">Stop</button>}
      />,
    );
    expect(html).toContain('data-ui="page-header"');
    expect(html).toContain("Meetings");
    expect(html).toContain("Product sync");
    expect(html).toContain("Notes ready");
    expect(html).toContain("Stop");
  });

  test("Card and Row expose accessible labels combining label, sub-label, and value", () => {
    const html = wrap(
      <Card ariaLabel="Shortcut and microphone" testId="settings-card">
        <Row
          label="Dictation shortcut"
          subLabel="Hold fn"
          value="fn"
          action={<button type="button">Change</button>}
        />
      </Card>,
    );
    expect(html).toContain('data-testid="settings-card"');
    expect(html).toContain('aria-label="Dictation shortcut, Hold fn, fn"');
    expect(html).toContain("Change");
  });

  test("Segmented renders radiogroup semantics, VoiceOver position labels, and wraps arrow navigation", () => {
    const options = [
      { value: "never", label: "Never" },
      { value: "24h", label: "24 hours" },
      { value: "30d", label: "30 days" },
      { value: "forever", label: "Forever" },
    ] as const;

    const html = wrap(
      <Segmented
        ariaLabel="Keep dictations"
        options={options}
        value="30d"
        onChange={() => {}}
      />,
    );
    expect(html).toContain('role="radiogroup"');
    expect(html).toContain('aria-label="Keep dictations"');
    expect(html).toContain('role="radio"');
    expect(html).toContain('aria-label="30 days, 3 of 4, selected"');
    expect(html).toContain('aria-label="Never, 1 of 4"');

    expect(nextSegmentedIndex(options, 2, "ArrowRight")).toBe(3);
    expect(nextSegmentedIndex(options, 3, "ArrowRight")).toBe(0);
    expect(nextSegmentedIndex(options, 0, "ArrowLeft")).toBe(3);
    expect(nextSegmentedIndex(options, 2, "Home")).toBe(0);
    expect(nextSegmentedIndex(options, 0, "End")).toBe(3);
  });

  test("TimeChip formats mm:ss and provides VoiceOver jump label", () => {
    expect(formatTimeChipLabel(435)).toEqual({
      display: "07:15",
      minutes: 7,
      seconds: 15,
    });
    const html = wrap(<TimeChip seconds={435} onJump={() => {}} />);
    expect(html).toContain(">07:15<");
    expect(html).toContain('aria-label="Jump to 7 minutes 15 seconds"');
  });

  test("Notice renders status role, title, body, and single action button", () => {
    const html = wrap(
      <Notice
        title="Typing is off"
        body="Your words go to the clipboard."
        actionLabel="Turn typing on"
        onAction={() => {}}
        testId="notice-typing-off"
      />,
    );
    expect(html).toContain('role="status"');
    expect(html).toContain('data-testid="notice-typing-off"');
    expect(html).toContain("Typing is off");
    expect(html).toContain("Your words go to the clipboard.");
    expect(html).toContain("Turn typing on");
  });
});
