import { describe, expect, mock, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { ShellSectionId } from "../src/components/layout/shellRoutes";

mock.module("../src/components/whats-new/releaseNotes", () => ({
  findReleaseNoteToShow: () => null,
  findLatestReleaseNote: () => null,
}));
mock.module("../src/i18n", () => ({
  default: i18next,
  SUPPORTED_LANGUAGES: [{ code: "en", name: "English", nativeName: "English" }],
  syncLanguageFromSettings: async () => {},
  getLanguageDirection: () => "ltr",
  isRTLLanguage: () => false,
}));

const { renderShellSection, SHELL_ROUTES, Sidebar } =
  await import("../src/components/layout/shellRoutes");

const i18n = i18next.createInstance();
void i18n
  .use(initReactI18next)
  .init({ lng: "en", resources: { en: { translation: en } } });
const wrap = (n: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{n}</I18nextProvider>);

describe("Sidebar & shell navigation (V-L1)", () => {
  test("renders Home, Meetings, Voice, and shared bottom in exact v0.2.2 order", () => {
    const html = wrap(
      <Sidebar activeSection="home" onSectionChange={() => {}} />,
    );
    const labels = [
      ...html.matchAll(/data-nav-id="[^"]+"[^>]*><span[^>]*>([^<]+)<\/span>/g),
    ].map((m) => m[1]);
    expect(labels.join("|")).toBe(
      "Home|Record a meeting|Meeting history|Ask your meetings|Folders and templates|Dictation history|Per-app profiles|Words and clean-up|Models|Settings|About",
    );
    const pos = [
      'id="home"',
      'heading="meetings"',
      'id="meetings-record"',
      'heading="voice"',
      'id="voice-history"',
      'group="shared"',
      'id="models"',
    ].map((k) => html.indexOf(`data-nav-${k}`));
    for (let i = 0; i < pos.length - 1; i++)
      expect(pos[i]! > -1 && pos[i]! < pos[i + 1]!).toBe(true);
    for (const id of ["home", "meetings-history", "voice-history"] as const) {
      const s = wrap(
        <Sidebar
          activeSection={id as ShellSectionId}
          onSectionChange={() => {}}
        />,
      );
      expect(
        [...s.matchAll(/data-nav-id="([^"]+)"[^>]*aria-current="page"/g)].map(
          (m) => m[1],
        ),
      ).toEqual([id]);
    }
  });

  test("keeps Meetings and Voice routes disjoint so no route shows both kinds", () => {
    const g = globalThis as { window?: unknown };
    const prev = Object.getOwnPropertyDescriptor(globalThis, "window");
    g.window = { __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" } };
    try {
      for (const rt of SHELL_ROUTES) {
        if (rt.group === "meetings") expect(rt.domain).toBe("meetings");
        if (rt.group === "voice") expect(rt.domain).toBe("voice");
        if (rt.id === "home") continue;
        const h = wrap(renderShellSection(rt.id, null));
        const m =
          h.includes('data-route-domain="meetings"') ||
          h.includes("All meetings");
        const v =
          h.includes('data-route-domain="voice"') ||
          h.includes("Loading history...");
        expect(m && v, rt.id).toBe(false);
        expect(
          rt.domain === "meetings" ? m : rt.domain === "voice" ? v : true,
          rt.id,
        ).toBe(true);
      }
    } finally {
      if (prev) Object.defineProperty(globalThis, "window", prev);
      else delete g.window;
    }
  });
});
