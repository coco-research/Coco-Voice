import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { MeetingsRetentionSection } from "../src/components/settings/MeetingsRetention";

function makeI18n() {
  const inst = i18next.createInstance();
  void inst.use(initReactI18next).init({
    lng: "en",
    fallbackLng: "en",
    resources: { en: { translation: en } },
    interpolation: { escapeValue: false },
  });
  return inst;
}

function renderRetention(
  overrides: Partial<
    React.ComponentProps<typeof MeetingsRetentionSection>
  > = {},
): string {
  return renderToStaticMarkup(
    <I18nextProvider i18n={makeI18n()}>
      <MeetingsRetentionSection
        deleteAudioAfterNotes={true}
        updatingToggle={false}
        confirmingDeleteAll={false}
        deletingAll={false}
        audioLeftCount={0}
        errorKey={null}
        onToggleDeleteAudio={() => {}}
        onRequestDeleteAll={() => {}}
        onCancelDeleteAll={() => {}}
        onConfirmDeleteAll={() => {}}
        {...overrides}
      />
    </I18nextProvider>,
  ).replaceAll("&#x27;", "'");
}

describe("MeetingsRetentionSection", () => {
  test("renders default-on audio cleanup toggle, Keep transcripts on this Mac badge without off switch, Delete all, and omits unverified upload claim", () => {
    const html = renderRetention();
    expect(html).toContain("Retention");
    expect(html).toContain("Delete audio after notes are made");
    expect(html).toContain(
      "On by default. Audio files are removed once the notes are ready.",
    );
    expect(html).toContain("Keep transcripts on this Mac");
    expect(html).toContain("On this Mac");
    expect(html).toContain("Delete all");
    expect((html.match(/type="checkbox"/g) ?? []).length).toBe(1);
    expect(html.toLowerCase()).not.toContain("doesn't upload");
    expect(html.toLowerCase()).not.toContain("does not upload");
  });

  test("renders in-page Delete all confirmation and leftover audio notice", () => {
    const confirmHtml = renderRetention({ confirmingDeleteAll: true });
    expect(confirmHtml).toContain(
      "Delete every meeting, transcript, note, and recording?",
    );
    expect(confirmHtml).toContain("Confirm delete all");
    expect(confirmHtml).toContain("Cancel");

    const warnHtml = renderRetention({ audioLeftCount: 1 });
    expect(warnHtml).toContain(
      "Meetings were deleted, but one or more audio folders could not be removed.",
    );
  });
});
