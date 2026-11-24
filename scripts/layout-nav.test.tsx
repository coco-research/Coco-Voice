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
const { MCP_STDIO_COMMAND, resolveAndCopyMcpCommand } =
  await import("../src/components/meetings/AgentAccessView");

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
      "Home|Record a meeting|Meeting history|Ask your meetings|Folders and templates|Agent access|Dictation history|Per-app profiles|Words and clean-up|Models|Settings|About",
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

  test("restores Meetings > Agent access right after Folders and templates with toggle off and exact MCP command", async () => {
    const ids = SHELL_ROUTES.map((r) => r.id as string);
    const foldersIdx = ids.indexOf("meetings-folders");
    const agentIdx = ids.indexOf("meetings-agent");
    expect(foldersIdx).toBeGreaterThan(-1);
    expect(agentIdx).toBe(foldersIdx + 1);

    const sidebarHtml = wrap(
      <Sidebar
        activeSection={"meetings-agent" as ShellSectionId}
        onSectionChange={() => {}}
      />,
    );
    expect(sidebarHtml).toContain(
      'data-nav-id="meetings-agent" aria-current="page"',
    );
    expect(sidebarHtml).toContain(en.sidebar.agentAccess);

    const pageHtml = wrap(
      renderShellSection("meetings-agent" as ShellSectionId, null),
    );
    expect(pageHtml).toContain('data-testid="page-5-agent-access"');
    expect(pageHtml).not.toContain(en.sidebar.sectionPlaceholder);
    expect(pageHtml).toContain(en.meetings.agent.offByDefault);
    expect(pageHtml).toContain(MCP_STDIO_COMMAND);
    expect(MCP_STDIO_COMMAND).toBe("coco-voice mcp");
    // Access toggle starts OFF: master switch unchecked, child switches disabled.
    const switches = pageHtml.match(/<input[^>]*type="checkbox"[^>]*>/g) ?? [];
    expect(switches.length).toBeGreaterThanOrEqual(3);
    expect(switches[0]).not.toContain("checked");
    expect(pageHtml).toContain(
      'data-testid="agent-scope-card" aria-disabled="true"',
    );

    const copied: string[] = [];
    const fallbackCopy = await resolveAndCopyMcpCommand(
      {
        mcpCommandLine: async () => ({
          status: "error",
          error: "unavailable",
        }),
      },
      (text) => {
        copied.push(text);
      },
    );
    expect(fallbackCopy).toEqual({
      command: "coco-voice mcp",
      usedFallback: true,
    });
    expect(copied[0]).toBe("coco-voice mcp");

    const bundledCmd =
      "'/Applications/Coco Voice.app/Contents/MacOS/coco-voice' mcp";
    const bundledCopy = await resolveAndCopyMcpCommand(
      {
        mcpCommandLine: async () => ({
          status: "ok",
          data: bundledCmd,
        }),
      },
      (text) => {
        copied.push(text);
      },
    );
    expect(bundledCopy).toEqual({
      command: bundledCmd,
      usedFallback: false,
    });
    expect(copied[1]).toBe(bundledCmd);
  });
});
