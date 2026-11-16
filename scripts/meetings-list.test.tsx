import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import {
  defaultMarkdownFilename,
  formatDurationMinutes,
  MeetingsListSection,
  NOTES_READY_TONE,
  SEARCH_DEBOUNCE_MS,
  type MeetingListItem,
} from "../src/components/meetings/MeetingsList";

const testI18n = i18next.createInstance();
void testI18n.use(initReactI18next).init({
  lng: "en",
  fallbackLng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const SAMPLE_ITEMS: MeetingListItem[] = [
  {
    id: 2,
    title: "Product sync",
    started_at: 1_700_003_600,
    ended_at: 1_700_005_400,
    notes_ready: true,
  },
  {
    id: 1,
    title: "Monday standup",
    started_at: 1_700_000_000,
    ended_at: 1_700_000_900,
    notes_ready: false,
  },
];

function renderList(
  overrides: Partial<React.ComponentProps<typeof MeetingsListSection>> = {},
): string {
  return renderToStaticMarkup(
    <I18nextProvider i18n={testI18n}>
      <MeetingsListSection
        items={SAMPLE_ITEMS}
        query=""
        loading={false}
        confirmingId={null}
        busyDeleteId={null}
        audioLeftWarning={false}
        errorKey={null}
        onQueryChange={() => {}}
        onExport={() => {}}
        onRequestDelete={() => {}}
        onCancelDelete={() => {}}
        onConfirmDelete={() => {}}
        {...overrides}
      />
    </I18nextProvider>,
  );
}

describe("MeetingsList (Frame 7)", () => {
  test("renders right-aligned count, green Notes ready vs hollow grey Transcript only, outlined Export Markdown & Delete, and no Start recording button", () => {
    const html = renderList();
    expect(html).toContain("All meetings");
    expect(html).toContain("2 meetings");
    expect(html).toContain("justify-between");
    expect(html).not.toContain("Start recording");
    expect(html).toContain('type="search"');
    expect(html).toContain("Product sync");
    expect(html).toContain("30 min");
    expect(html).toContain("Notes ready");
    expect(html).toContain(NOTES_READY_TONE);
    expect(html).toContain('fill="currentColor"');
    expect(html).toContain("Monday standup");
    expect(html).toContain("15 min");
    expect(html).toContain("Transcript only");
    expect(html).toContain('fill="none"');
    expect(html).toContain("Export Markdown");
    expect(html).toContain("Delete");
    expect(html).toContain("border-text/75 bg-transparent");
  });

  test("renders empty search message, delete confirmation, and helper formatters", () => {
    expect(renderList({ items: [], query: "pricing" })).toContain(
      "No meetings match your search.",
    );
    expect(renderList({ items: [], query: "" })).toContain(
      "No meetings yet. Start notes from the menu bar.",
    );
    expect(SEARCH_DEBOUNCE_MS).toBeGreaterThan(0);

    const confirmHtml = renderList({ confirmingId: 2 });
    expect(confirmHtml).toContain("Delete this meeting?");
    expect(confirmHtml).toContain("Confirm delete");
    expect(confirmHtml).toContain("Cancel");

    const audioLeftHtml = renderList({
      items: [SAMPLE_ITEMS[1]!],
      audioLeftWarning: true,
    });
    expect(audioLeftHtml).toContain("1 meeting");
    expect(audioLeftHtml).toContain(
      "Meeting deleted, but its audio folder is still on disk.",
    );

    expect(defaultMarkdownFilename("Product sync")).toBe("product-sync.md");
    expect(defaultMarkdownFilename("   ")).toBe("meeting.md");
    expect(formatDurationMinutes(1_000, 2_800)).toBe(30);
    expect(formatDurationMinutes(1_000, null)).toBeNull();
  });
});
