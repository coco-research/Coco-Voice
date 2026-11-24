import { describe, expect, mock, test } from "bun:test";
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { getStoredTheme, THEME_STORAGE_KEY } from "../src/lib/utils/theme";
import { useSettingsStore } from "../src/stores/settingsStore";
import { findMissingStaticI18nKeys } from "./i18n-key-scan";

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

const { AboutSettings } =
  await import("../src/components/settings/about/AboutSettings");
const { AdvancedSettings } =
  await import("../src/components/settings/advanced/AdvancedSettings");
const { Corrections } = await import("../src/components/settings/Corrections");
const { DebugSettings } =
  await import("../src/components/settings/debug/DebugSettings");

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
  const prevLoading = useSettingsStore.getState().isLoading;
  const prevSettings = useSettingsStore.getState().settings;
  const nextSettings = {
    ...(prevSettings ?? {}),
    experimental_enabled: false,
    post_process_enabled: false,
    debug_mode: false,
    update_checks_enabled: true,
    paste_delay_ms: 60,
    paste_delay_after_ms: 60,
    ...overrides,
  } as never;
  Object.assign(useSettingsStore.getInitialState(), {
    isLoading: false,
    settings: nextSettings,
  });
  useSettingsStore.setState({ isLoading: false, settings: nextSettings });
  try {
    return renderToStaticMarkup(
      <I18nextProvider i18n={makeTestI18n()}>{node}</I18nextProvider>,
    ).replaceAll("&#x27;", "'");
  } finally {
    Object.assign(useSettingsStore.getInitialState(), {
      isLoading: prevLoading,
      settings: prevSettings,
    });
    useSettingsStore.setState({
      isLoading: prevLoading,
      settings: prevSettings,
    });
    if (prev) {
      Object.defineProperty(globalThis, "window", prev);
    } else {
      delete g.window;
    }
  }
}

describe("V-C2 (E13): Clean up page out of Experimental", () => {
  test.todo(
    "sidebar always shows Clean up and PostProcessingToggle lives on the Clean up page, not Experimental (label copy pass pending)",
  );
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

describe("V-C3 (E14): Plain sentence-case labels across Onboarding, General, Advanced, Clean up, and Overlay/Tray", () => {
  test.todo(
    "Onboarding and Overlay/Tray use plain sentence-case labels across all 22 locales (label copy pass pending)",
  );

  test.todo(
    "General and About use plain sentence-case labels across all 22 locales (label copy pass pending)",
  );

  test.todo(
    "Advanced uses plain sentence-case labels across App, Output, Transcription, History, and Experimental (label copy pass pending)",
  );

  test.todo(
    "Clean up matches Spoken fixes and uses plain style labels across all 22 locales (label copy pass pending)",
  );
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

    const allowedHandyFiles = new Set([
      "src/bindings.ts",
      "src/components/settings/about/WhatGoesOnline.tsx",
      "src/content/release-notes/0.9.0.md",
      "src/lib/utils/theme.ts",
    ]);

    const themeSource = readFileSync(
      join(rootDir, "src/lib/utils/theme.ts"),
      "utf8",
    );
    expect((themeSource.match(/handy/gi) ?? []).length).toBe(1);

    const handyHits: string[] = [];

    for (const file of walk(srcDir)) {
      const rel = relative(rootDir, file).replaceAll("\\", "/");
      const text = readFileSync(file, "utf8");

      if (!allowedHandyFiles.has(rel) && /handy/i.test(text)) {
        handyHits.push(rel);
      }
    }

    const missingKeys = findMissingStaticI18nKeys(rootDir, {
      onlyWithoutDefault: false,
    });

    expect(missingKeys).toEqual([]);
    expect(handyHits).toEqual([]);
  });

  test("migrates legacy handy.theme to coco_voice.theme once on boot and removes the old key", () => {
    const store = new Map<string, string>();
    const prev = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: {
        getItem: (k: string) => store.get(k) ?? null,
        setItem: (k: string, v: string) => {
          store.set(k, v);
        },
        removeItem: (k: string) => {
          store.delete(k);
        },
      },
    });
    try {
      store.set("handy.theme", "dark");
      expect(getStoredTheme()).toBe("dark");
      expect(store.get(THEME_STORAGE_KEY)).toBe("dark");
      expect(store.has("handy.theme")).toBe(false);

      store.set(THEME_STORAGE_KEY, "light");
      store.set("handy.theme", "dark");
      expect(getStoredTheme()).toBe("light");
      expect(store.get(THEME_STORAGE_KEY)).toBe("light");
    } finally {
      if (prev) {
        Object.defineProperty(globalThis, "localStorage", prev);
      } else {
        delete (globalThis as { localStorage?: unknown }).localStorage;
      }
    }
  });
});

/**
 * Repo-wide upstream name gate (post-rebrand allow-list).
 *
 * Every entry below is either:
 * - Tier C upstream URL/crate/repo/historical reference (blob.handy.computer,
 *   handy-computer/*, handy-keys, 0.9.0 release note, test env hook),
 * - A runtime upgrade fallback (HANDY_NO_GTK_LAYER_SHELL in overlay.rs,
 *   legacy portable marker in portable.rs / installer.nsi, handy.theme in theme.ts),
 * - Upstream attribution (NOTICE and the single Credits line in README.md), or
 * - This gate file itself.
 *
 * Exact-match ratchet: any new file, higher count, or lower count fails so the
 * allow-list stays tight as remaining upstream references are retired.
 */
const REPO_HANDY_ALLOWLIST: Record<string, number> = {
  ".github/workflows/build.yml": 2,
  "BUILD.md": 1,
  NOTICE: 1,
  "README.md": 1,
  "sbom/Coco-Voice.cdx.json": 1,
  "sbom/model-licences.md": 47,
  "scripts/ac-a7-partial-cleanup.test.tsx": 1,
  "scripts/mirror-models.test.ts": 5,
  "scripts/mirror-models.ts": 2,
  "scripts/mirror-render.test.ts": 8,
  "scripts/mirror-render.ts": 5,
  "scripts/mirror-templates.test.ts": 2,
  "scripts/mirror-upload.test.ts": 1,
  "scripts/mirror/ATTRIBUTION.template.md": 8,
  "scripts/mirror/README-card.template.md": 8,
  "scripts/privacy-allowlist.json": 1,
  "scripts/vc-makes-sense.test.tsx": 20,
  "scripts/vd10-y7-vk1.test.tsx": 2,
  "scripts/voice-journeys.json": 1,
  "src-tauri/Cargo.toml": 4,
  "src-tauri/nsis/installer.nsi": 1,
  "src-tauri/src/catalog/catalog.json": 46,
  "src-tauri/src/catalog/measured.rs": 5,
  "src-tauri/src/catalog/measured_runs.json": 2,
  "src-tauri/src/catalog/mod.rs": 59,
  "src-tauri/src/catalog/model_licences.json": 46,
  "src-tauri/src/commands/history.rs": 1,
  "src-tauri/src/commands/models.rs": 3,
  "src-tauri/src/managers/meeting_chunker.rs": 3,
  "src-tauri/src/managers/model.rs": 40,
  "src-tauri/src/managers/transcription.rs": 13,
  "src-tauri/src/meeting_session.rs": 5,
  "src-tauri/src/overlay.rs": 1,
  "src-tauri/src/portable.rs": 1,
  "src-tauri/src/shortcut/coco_keys.rs": 25,
  "src-tauri/src/shortcut/handler.rs": 1,
  "src-tauri/src/shortcut/mod.rs": 13,
  "src/bindings.ts": 1,
  "src/components/settings/about/WhatGoesOnline.tsx": 1,
  "src/content/release-notes/0.9.0.md": 1,
  "src/lib/utils/theme.ts": 1,
};

describe("Repo-wide Handy gate", () => {
  test("git grep -c -i handy matches the exact allow-list ratchet", () => {
    const raw = execFileSync(
      "git",
      ["grep", "-c", "-i", "handy", "--", ".", ":!*.lock"],
      { cwd: rootDir, encoding: "utf8" },
    );

    const actual: Record<string, number> = {};
    for (const line of raw.trim().split("\n")) {
      if (!line) continue;
      const idx = line.lastIndexOf(":");
      if (idx === -1) {
        throw new Error(`Unexpected git grep output line: ${line}`);
      }
      const file = line.slice(0, idx);
      const count = Number(line.slice(idx + 1));
      actual[file] = count;
    }

    expect(actual).toEqual(REPO_HANDY_ALLOWLIST);
  });
});
