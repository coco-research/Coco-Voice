import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { AppProfile } from "../src/bindings";
import {
  renderShellSection,
  SHELL_ROUTES,
  Sidebar,
} from "../src/components/layout/shellRoutes";
import {
  AppProfilesSection,
  summarizeAppProfile,
} from "../src/components/settings/AppProfilesView";

const i18n = i18next.createInstance();
void i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const SAMPLE_PROFILES: AppProfile[] = [
  {
    id: "mail",
    name: "Mail",
    app_identifier: "com.apple.mail",
    tone: "formal",
    capitalization: "sentence",
    punctuation: "full",
    post_process_enabled: true,
  },
  {
    id: "messages",
    name: "Messages",
    app_identifier: "com.apple.MobileSMS",
    tone: "casual",
    capitalization: "lowercase",
    punctuation: "drop_trailing_period",
    post_process_enabled: true,
  },
  {
    id: "cursor",
    name: "Cursor",
    app_identifier: "com.todesktop.230313mzl4w4u92",
    tone: "code",
    capitalization: "default",
    punctuation: "drop_trailing_period",
    post_process_enabled: true,
  },
  {
    id: "terminal",
    name: "Terminal",
    app_identifier: "com.apple.Terminal",
    tone: "default",
    capitalization: "default",
    punctuation: "default",
    post_process_enabled: false,
  },
];

describe("Per-app profiles under Voice (V-D16 + V-D9 / v0.2.2 Page 7)", () => {
  test("renders Page 7 Per-app profiles, Default fallback row, and Never dictate in blocked apps", () => {
    const spec = SHELL_ROUTES.find((r) => r.id === "voice-profiles");
    expect(spec?.group).toBe("voice");
    expect(spec?.domain).toBe("voice");

    const nav = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <Sidebar activeSection="voice-profiles" onSectionChange={() => {}} />
      </I18nextProvider>,
    );
    expect(nav).toContain('data-nav-id="voice-profiles" aria-current="page"');

    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <AppProfilesSection
          profiles={SAMPLE_PROFILES}
          blockedApps={["1Password", "Banking"]}
          onSaveProfiles={async () => {}}
          onSaveBlockedApps={async () => {}}
        />
      </I18nextProvider>,
    ).replaceAll("&#x27;", "'");

    expect(html).toContain('data-testid="per-app-profiles-section"');
    expect(html).toContain(">Voice</p>");
    expect(html).toContain(">Per-app profiles</h2>");
    expect(html).toContain(">Add an app</button>");
    expect(html).toContain(
      "How your words are written depends on the app in front. Apps without a profile use the default.",
    );

    expect(summarizeAppProfile(SAMPLE_PROFILES[0]!, (k) => i18n.t(k))).toBe(
      "Full sentences · capitals and punctuation · clean-up on",
    );
    expect(summarizeAppProfile(SAMPLE_PROFILES[1]!, (k) => i18n.t(k))).toBe(
      "Casual · no full stop at the end · clean-up on",
    );
    expect(summarizeAppProfile(SAMPLE_PROFILES[3]!, (k) => i18n.t(k))).toBe(
      "Exactly what you said · clean-up off",
    );

    expect(html).toContain(">Mail</p>");
    expect(html).toContain(
      "Full sentences · capitals and punctuation · clean-up on",
    );
    expect(html).toContain(">Messages</p>");
    expect(html).toContain("Casual · no full stop at the end · clean-up on");
    expect(html).toContain(">Terminal</p>");
    expect(html).toContain("Exactly what you said · clean-up off");
    expect(html).toContain(">Default</p>");
    expect(html).toContain(">Every other app</p>");
    expect(html).toContain(">Edit</button>");

    expect(html).toContain(">Never dictate in</h3>");
    expect(html).toContain(">1Password</p>");
    expect(html).toContain(">Banking</p>");
    expect(html).toContain(">The shortcut does nothing here</p>");
    expect(html).toContain(">Remove</button>");

    const routeHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        {renderShellSection("voice-profiles", null)}
      </I18nextProvider>,
    );
    expect(routeHtml).toContain('data-route-domain="voice"');
    expect(routeHtml).toContain("Per-app profiles");
    expect(routeHtml).not.toContain("Section content coming in its own slice");
  });
});
