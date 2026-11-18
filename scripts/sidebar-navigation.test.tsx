import { expect, mock, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import type { SidebarSection } from "../src/components/Sidebar";
import en from "../src/i18n/locales/en/translation.json";

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

// Page content is not mounted by Sidebar; isolate its Vite-only release-note imports.
mock.module("../src/components/settings", () => ({
  GeneralSettings: () => null,
  AdvancedSettings: () => null,
  HistorySettings: () => null,
  DebugSettings: () => null,
  AboutSettings: () => null,
  PostProcessingSettings: () => null,
  ModelsSettings: () => null,
}));
const { Sidebar } = await import("../src/components/Sidebar");

const render = (activeSection: SidebarSection) =>
  renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>
      <Sidebar activeSection={activeSection} onSectionChange={() => {}} />
    </I18nextProvider>,
  );

test("navigation uses native buttons so keyboard users can activate every page", () => {
  const html = render("home");
  const buttons = html.match(/<button\b[^>]*>/g) ?? [];
  expect(buttons.length).toBeGreaterThanOrEqual(5);
  expect(buttons.every((button) => button.includes('type="button"'))).toBe(
    true,
  );
  expect(html).not.toMatch(/<div[^>]*cursor-pointer/);
});

test("only the selected navigation page is exposed as current", () => {
  for (const page of ["home", "models", "voice-history", "about"] as const) {
    const buttons = render(page).match(/<button\b[^>]*>/g) ?? [];
    const selected = buttons.filter((button) =>
      button.includes('aria-current="page"'),
    );
    expect(selected).toHaveLength(1);
    expect(selected[0]).toContain(`data-nav-id="${page}"`);
  }
});
