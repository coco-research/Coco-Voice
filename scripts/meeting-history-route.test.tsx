import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import {
  renderShellSection,
  SHELL_ROUTES,
  Sidebar,
} from "../src/components/layout/shellRoutes";
import { MeetingsListSection } from "../src/components/meetings/MeetingsList";

const i18n = i18next.createInstance();
void i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

describe("Meeting history route (V-L3 / Frame 7)", () => {
  test("mounts Meeting history under Meetings with right-aligned count and NO Start recording button", () => {
    const spec = SHELL_ROUTES.find((r) => r.id === "meetings-history");
    expect(spec?.group).toBe("meetings");
    expect(spec?.domain).toBe("meetings");

    const navHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <Sidebar activeSection="meetings-history" onSectionChange={() => {}} />
      </I18nextProvider>,
    );
    expect(navHtml).toContain(
      'data-nav-id="meetings-history" aria-current="page"',
    );

    const routeHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        {renderShellSection("meetings-history", null)}
      </I18nextProvider>,
    );
    expect(routeHtml).toContain('data-testid="meeting-history-section"');
    expect(routeHtml).toContain(">Meetings<");
    expect(routeHtml).toContain("All meetings");
    expect(routeHtml).not.toContain("Start recording");
    expect(routeHtml).toContain('aria-label="Search meetings"');
    expect(routeHtml).not.toContain("Dictation history");

    const populated = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <MeetingsListSection
          items={[
            {
              id: 7,
              title: "Product sync",
              started_at: 1_700_003_600,
              ended_at: 1_700_005_400,
              notes_ready: true,
            },
          ]}
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
        />
      </I18nextProvider>,
    );
    expect(populated).toContain("Product sync");
    expect(populated).toContain("1 meeting");
    expect(populated).not.toContain("Start recording");
    expect(populated).toContain("Export Markdown");
    expect(populated).toContain("Delete");
  });
});
