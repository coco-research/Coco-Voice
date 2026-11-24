import { describe, expect, mock, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";

const i18n = i18next.createInstance();
await i18n
  .use(initReactI18next)
  .init({ lng: "en", resources: { en: { translation: en } } });

mock.module("../src/components/whats-new/releaseNotes", () => ({
  findReleaseNoteToShow: () => null,
  findLatestReleaseNote: () => null,
}));
mock.module("../src/i18n", () => ({
  default: i18n,
  SUPPORTED_LANGUAGES: [{ code: "en", name: "English", nativeName: "English" }],
  syncLanguageFromSettings: async () => {},
  getLanguageDirection: () => "ltr",
  isRTLLanguage: () => false,
}));

const { Sidebar: MainSidebar } = await import("../src/components/Sidebar");
const { ShellContent } = await import("../src/components/layout/ShellContent");
const { SHELL_ROUTES: NAV_SHELL_ROUTES } =
  await import("../src/components/layout/shellNavigation");
const {
  renderShellSection,
  SHELL_ROUTES: ROUTE_SHELL_ROUTES,
  Sidebar: RoutesSidebar,
} = await import("../src/components/layout/shellRoutes");

type NavSectionId = (typeof NAV_SHELL_ROUTES)[number]["id"] | "debug";
type RouteSectionId = (typeof ROUTE_SHELL_ROUTES)[number]["id"] | "debug";

const wrap = (node: React.ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

describe("T-mx0: Agent access page is hidden and unreachable in 0.20.2", () => {
  test("neither route table includes meetings-agent", () => {
    const navIds = NAV_SHELL_ROUTES.map((r) => r.id as string);
    const routeIds = ROUTE_SHELL_ROUTES.map((r) => r.id as string);
    expect(navIds).not.toContain("meetings-agent");
    expect(routeIds).not.toContain("meetings-agent");
  });

  test("rendered sidebars show no Agent access button or meetings-agent nav id", () => {
    const mainSidebarHtml = wrap(
      <MainSidebar
        activeSection="home"
        onSectionChange={() => {}}
        showMeetings={true}
      />,
    );
    expect(mainSidebarHtml).not.toContain('data-nav-id="meetings-agent"');
    expect(mainSidebarHtml).not.toContain(en.sidebar.agentAccess);

    const routesSidebarHtml = wrap(
      <RoutesSidebar activeSection="home" onSectionChange={() => {}} />,
    );
    expect(routesSidebarHtml).not.toContain('data-nav-id="meetings-agent"');
    expect(routesSidebarHtml).not.toContain(en.sidebar.agentAccess);
  });

  test("no route id renders page-5-agent-access in the shell", () => {
    const g = globalThis as { window?: unknown };
    const prev = Object.getOwnPropertyDescriptor(globalThis, "window");
    g.window = { __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" } };
    try {
      const routeIds: RouteSectionId[] = [
        ...ROUTE_SHELL_ROUTES.map((r) => r.id),
        "debug",
      ];
      for (const id of routeIds) {
        const html = wrap(renderShellSection(id, null));
        expect(html, `renderShellSection(${id})`).not.toContain(
          'data-testid="page-5-agent-access"',
        );
      }

      const navIds: NavSectionId[] = [
        ...NAV_SHELL_ROUTES.map((r) => r.id),
        "debug",
      ];
      for (const id of navIds) {
        const html = wrap(
          <ShellContent
            section={id}
            meetingsUi={null}
            showMeetings={true}
            onNavigate={() => {}}
            about={null}
            debug={null}
          />,
        );
        expect(html, `ShellContent(${id})`).not.toContain(
          'data-testid="page-5-agent-access"',
        );
      }
    } finally {
      if (prev) Object.defineProperty(globalThis, "window", prev);
      else delete g.window;
    }
  });
});
