import { describe, expect, mock, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
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
const { AboutSettings } =
  await import("../src/components/settings/about/AboutSettings");
const { AdvancedSettings } =
  await import("../src/components/settings/advanced/AdvancedSettings");
const { Corrections } = await import("../src/components/settings/Corrections");
const { DebugSettings } =
  await import("../src/components/settings/debug/DebugSettings");
const { GeneralSettings } =
  await import("../src/components/settings/general/GeneralSettings");
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

describe("V-C6 (E17): Update checks and paste timing in normal settings", () => {
  test("update checks live in About, paste timing lives in Advanced Output, Debug has neither, and What goes online names About instead of Debug", () => {
    const aboutHtml = renderWithSettings(<AboutSettings />);
    expect(aboutHtml).toContain(`>${en.settings.debug.updateChecks.label}<`);
    expect(aboutHtml).toContain(en.settings.online.githubWhen);

    const advancedHtml = renderWithSettings(<AdvancedSettings />, {
      debug_mode: false,
      experimental_enabled: false,
    });
    expect(advancedHtml).toContain(`>${en.settings.debug.pasteDelay.title}<`);
    expect(advancedHtml).toContain(
      `>${en.settings.debug.pasteDelayAfter.title}<`,
    );

    const debugHtml = renderWithSettings(<DebugSettings />, {
      debug_mode: true,
    });
    expect(debugHtml).not.toContain(
      `>${en.settings.debug.updateChecks.label}<`,
    );
    expect(debugHtml).not.toContain(`>${en.settings.debug.pasteDelay.title}<`);
    expect(debugHtml).not.toContain(
      `>${en.settings.debug.pasteDelayAfter.title}<`,
    );

    for (const loc of readdirSync(localesDir).sort()) {
      const data = JSON.parse(
        readFileSync(join(localesDir, loc, "translation.json"), "utf8"),
      ) as { settings?: { online?: { githubWhen?: string } } };
      const gw = data.settings?.online?.githubWhen ?? "";
      expect(gw.length).toBeGreaterThan(0);
      expect(gw).not.toContain("Cmd+Shift+D");
      expect(gw.toLowerCase()).not.toContain("debug");
    }
  });
});

describe("V-C3 (E14): Plain labels on General and Clean up screens", () => {
  test("replaces technical jargon on General and Clean up across all 22 locales", () => {
    expect(en.settings.general.pushToTalk.label).toBe("Hold to talk");
    expect(
      en.settings.general.shortcut.bindings.transcribe_with_post_process.name,
    ).toBe("Clean-up shortcut");
    expect(en.settings.postProcessing.hotkey.title).toBe("Shortcut");
    expect(en.settings.postProcessing.correction.title).toBe("Spoken fixes");
    expect(en.settings.postProcessing.api.title).toBe("Clean-up model");
    expect(en.settings.postProcessing.prompts.title).toBe("Instructions");

    const generalHtml = renderWithSettings(<GeneralSettings />);
    expect(generalHtml).toContain(">Hold to talk<");
    expect(generalHtml).not.toContain("Push To Talk");

    const cleanupHtml = renderWithSettings(<PostProcessingSettings />);
    expect(cleanupHtml).toContain(">Shortcut<");
    expect(cleanupHtml).toContain(">Spoken fixes<");
    expect(cleanupHtml).toContain(">Clean-up model<");
    expect(cleanupHtml).toContain(">Instructions<");
    expect(cleanupHtml).not.toContain("Iterative Correction");
    expect(cleanupHtml).not.toContain("OpenAI Compatible");

    for (const loc of readdirSync(localesDir).sort()) {
      const text = readFileSync(
        join(localesDir, loc, "translation.json"),
        "utf8",
      );
      expect(text).not.toContain("API (OpenAI Compatible)");
      expect(text).not.toContain("Iterative Correction");
      expect(text).not.toContain("Push To Talk");
    }
  });

  test("replaces jargon on Advanced and Output screens across all 22 locales", () => {
    expect(en.settings.advanced.voiceActivityDetection.title).toBe(
      "Skip silence",
    );
    expect(en.settings.debug.appendTrailingSpace.label).toBe(
      "Add a space at the end",
    );
    expect(en.settings.debug.pasteDelay.title).toBe("Delay before paste");
    expect(en.settings.debug.pasteDelayAfter.title).toBe("Delay after paste");
    expect(en.settings.advanced.autostart.label).toBe("Open at login");

    const advancedHtml = renderWithSettings(<AdvancedSettings />);
    expect(advancedHtml).toContain(">Skip silence<");
    expect(advancedHtml).toContain(">Add a space at the end<");
    expect(advancedHtml).toContain(">Delay before paste<");
    expect(advancedHtml).toContain(">Delay after paste<");
    expect(advancedHtml).toContain(">Open at login<");
    expect(advancedHtml).not.toContain("Voice Activity Detection");
    expect(advancedHtml).not.toContain("Append Trailing Space");

    for (const loc of readdirSync(localesDir).sort()) {
      const text = readFileSync(
        join(localesDir, loc, "translation.json"),
        "utf8",
      );
      expect(text).not.toContain("Voice Activity Detection");
      expect(text).not.toContain("VAD tail");
    }
  });
});

describe("V-C7 (E18): Localized Corrections and History strings with zero missing i18n keys", () => {
  test("every static t() and i18nKey reference in src/ resolves in en/translation.json and Corrections renders non-English placeholders", () => {
    const fr = JSON.parse(
      readFileSync(join(localesDir, "fr/translation.json"), "utf8"),
    );
    const frInst = i18next.createInstance();
    void frInst.use(initReactI18next).init({
      lng: "fr",
      fallbackLng: "fr",
      resources: { fr: { translation: fr } },
      interpolation: { escapeValue: false },
    });
    const prevSettings = useSettingsStore.getState().settings;
    useSettingsStore.setState({
      settings: {
        ...(prevSettings ?? {}),
        corrections: [{ from: "teh", to: "the" }],
      } as never,
    });
    let frHtml = "";
    try {
      frHtml = renderToStaticMarkup(
        <I18nextProvider i18n={frInst}>
          <Corrections />
        </I18nextProvider>,
      );
    } finally {
      useSettingsStore.setState({ settings: prevSettings });
    }
    expect(frHtml).toContain(
      `placeholder="${fr.settings.advanced.corrections.fromPlaceholder}"`,
    );
    expect(frHtml).not.toContain('placeholder="From"');
    expect(frHtml).not.toContain('placeholder="To"');
  });

  test("no missing static i18n keys and no leftover Handy code in src/", () => {
    const srcDir = join(rootDir, "src");
    const walk = (dir: string): string[] =>
      readdirSync(dir).flatMap((name) => {
        const full = join(dir, name);
        return statSync(full).isDirectory() ? walk(full) : [full];
      });

    const hasKey = (obj: Record<string, unknown>, keyPath: string): boolean => {
      let cur: unknown = obj;
      for (const part of keyPath.split(".")) {
        if (typeof cur !== "object" || cur === null || !(part in cur)) {
          return false;
        }
        cur = (cur as Record<string, unknown>)[part];
      }
      return true;
    };

    const allowedHandyFiles = new Set([
      "src/bindings.ts",
      "src/components/settings/CocoVoiceKeysShortcutInput.tsx",
      "src/components/settings/about/WhatGoesOnline.tsx",
      "src/content/release-notes/0.9.0.md",
    ]);

    const missingKeys: string[] = [];
    const handyHits: string[] = [];

    for (const file of walk(srcDir)) {
      const rel = relative(rootDir, file).replaceAll("\\", "/");
      const text = readFileSync(file, "utf8");

      if (!allowedHandyFiles.has(rel) && /handy/i.test(text)) {
        handyHits.push(rel);
      }
      if (!rel.startsWith("src/bindings") && /\.(ts|tsx)$/.test(rel)) {
        for (const m of text.matchAll(
          /(?<![A-Za-z0-9_])t\(\s*["']([A-Za-z0-9_.-]+)["']/g,
        )) {
          if (!hasKey(en as Record<string, unknown>, m[1])) {
            missingKeys.push(`${rel}: ${m[1]}`);
          }
        }
        for (const m of text.matchAll(
          /i18nKey=\s*["']([A-Za-z0-9_.-]+)["']/g,
        )) {
          if (!hasKey(en as Record<string, unknown>, m[1])) {
            missingKeys.push(`${rel}: ${m[1]}`);
          }
        }
      }
    }

    expect(missingKeys).toEqual([]);
    expect(handyHits).toEqual([]);
  });
});
