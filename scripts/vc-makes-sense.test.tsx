import { describe, expect, mock, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { useSettingsStore } from "../src/stores/settingsStore";

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

const { SECTIONS_CONFIG } = await import("../src/components/Sidebar");
const { AdvancedSettings } =
  await import("../src/components/settings/advanced/AdvancedSettings");
const { PostProcessingSettings } =
  await import("../src/components/settings/post-processing/PostProcessingSettings");

const rootDir = join(import.meta.dir, "..");
const localesDir = join(rootDir, "src/i18n/locales");

function makeTestI18n() {
  const inst = i18next.createInstance();
  void inst.use(initReactI18next).init({
    lng: "en",
    fallbackLng: "en",
    resources: { en: { translation: en } },
    interpolation: { escapeValue: false },
  });
  return inst;
}

function renderWithSettings(
  node: React.ReactElement,
  overrides: Record<string, unknown> = {},
): string {
  const g = globalThis as { window?: unknown };
  const prev = Object.getOwnPropertyDescriptor(globalThis, "window");
  g.window = { __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" } };
  const prevSettings = useSettingsStore.getState().settings;
  useSettingsStore.setState({
    settings: {
      ...(prevSettings ?? {}),
      experimental_enabled: false,
      post_process_enabled: false,
      debug_mode: false,
      update_checks_enabled: true,
      paste_delay_ms: 60,
      paste_delay_after_ms: 60,
      ...overrides,
    } as never,
  });
  try {
    return renderToStaticMarkup(
      <I18nextProvider i18n={makeTestI18n()}>{node}</I18nextProvider>,
    ).replaceAll("&#x27;", "'");
  } finally {
    useSettingsStore.setState({ settings: prevSettings });
    if (prev) {
      Object.defineProperty(globalThis, "window", prev);
    } else {
      delete g.window;
    }
  }
}

describe("V-C2 (E13): Clean up page out of Experimental", () => {
  test("sidebar always shows Clean up and PostProcessingToggle lives on the Clean up page, not Experimental", () => {
    expect(en.sidebar.postProcessing).toBe("Clean up");
    expect(en.settings.debug.postProcessingToggle.label).toBe("Clean up");
    expect(
      SECTIONS_CONFIG.postprocessing.enabled({
        experimental_enabled: false,
        post_process_enabled: false,
      }),
    ).toBe(true);

    for (const loc of readdirSync(localesDir).sort()) {
      const data = JSON.parse(
        readFileSync(join(localesDir, loc, "translation.json"), "utf8"),
      ) as { sidebar?: { postProcessing?: string } };
      expect(data.sidebar?.postProcessing).not.toBe("Post Process");
    }

    const advancedWithExperimental = renderWithSettings(<AdvancedSettings />, {
      experimental_enabled: true,
    });
    expect(advancedWithExperimental).not.toContain(">Clean up<");

    const cleanupHtml = renderWithSettings(<PostProcessingSettings />, {
      experimental_enabled: false,
      post_process_enabled: false,
    });
    expect(cleanupHtml).toContain(">Clean up</h2>");
    expect(cleanupHtml).toContain(">Clean up</h3>");
  });
});
