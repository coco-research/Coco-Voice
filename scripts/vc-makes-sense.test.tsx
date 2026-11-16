import { describe, expect, mock, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { getStoredTheme, THEME_STORAGE_KEY } from "../src/lib/utils/theme";
import { useModelStore } from "../src/stores/modelStore";
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
const { default: Onboarding } =
  await import("../src/components/onboarding/Onboarding");
const { default: RecordingOverlay } =
  await import("../src/overlay/RecordingOverlay");

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

describe("V-C3 (E14): Plain sentence-case labels across Onboarding, General, Advanced, Clean up, and Overlay/Tray", () => {
  test("Onboarding and Overlay/Tray use plain sentence-case labels across all 22 locales", () => {
    expect(en.onboarding.permissions.title).toBe("Permissions needed");
    expect(en.onboarding.permissions.microphone.title).toBe(
      "Microphone access",
    );
    expect(en.onboarding.permissions.accessibility.title).toBe(
      "Accessibility access",
    );
    expect(en.onboarding.permissions.grant).toBe("Allow access");
    expect(en.accessibility.openSettings).toBe("Open system settings");
    expect(en.tray.checkUpdates).toBe("Check for updates...");
    expect(en.tray.copyLastTranscript).toBe("Copy last transcript");
    expect(en.tray.unloadModel).toBe("Unload model");
    expect(en.overlay.processing).toBe("Cleaning up...");

    const mkModel = (id: string, name: string, is_downloaded: boolean) =>
      ({
        id,
        name,
        description: "Fast",
        filename: `${id}.bin`,
        size_mb: 100,
        is_downloaded,
        is_downloading: false,
        partial_size: 0,
        is_directory: false,
        accuracy_score: 0.9,
        speed_score: 0.9,
        supports_translation: false,
        is_recommended: true,
        supported_languages: ["en"],
        supports_language_selection: false,
        supports_language_detection: false,
        is_custom: false,
        is_streaming: false,
      }) as never;
    const prevModels = useModelStore.getState().models;
    const fakeModels = [
      mkModel("downloaded-m", "Parakeet V3", true),
      mkModel("downloadable-m", "Whisper Turbo", false),
    ];
    Object.assign(useModelStore.getInitialState(), { models: fakeModels });
    useModelStore.setState({ models: fakeModels });
    try {
      const onboardingHtml = renderWithSettings(
        <Onboarding onModelSelected={() => {}} />,
      );
      expect(onboardingHtml).toContain(">Compatible models<");
      expect(onboardingHtml).toContain(">Available to download<");
      expect(onboardingHtml).not.toContain("Compatible Models");
      expect(onboardingHtml).not.toContain("Available to Download");
    } finally {
      Object.assign(useModelStore.getInitialState(), { models: prevModels });
      useModelStore.setState({ models: prevModels });
    }

    const overlayHtml = renderWithSettings(<RecordingOverlay />);
    expect(overlayHtml).toContain('aria-label="Cancel"');

    for (const loc of readdirSync(localesDir).sort()) {
      const text = readFileSync(
        join(localesDir, loc, "translation.json"),
        "utf8",
      );
      const parsed = JSON.parse(text) as {
        onboarding: {
          existingModelsTitle: string;
          downloadModelsTitle: string;
        };
      };
      expect(parsed.onboarding.existingModelsTitle).not.toBe(
        "Compatible Models",
      );
      expect(parsed.onboarding.downloadModelsTitle).not.toBe(
        "Available to Download",
      );
      expect(text).not.toContain("Permissions Required");
      expect(text).not.toContain("Copy Last Transcript");
      expect(text).not.toContain("Unload Model");
      expect(text).not.toContain('"Processing..."');
    }
  });

  test("General and About use plain sentence-case labels across all 22 locales", () => {
    expect(en.settings.general.pushToTalk.label).toBe("Hold to talk");
    expect(en.settings.general.shortcut.bindings.transcribe.name).toBe(
      "Dictation shortcut",
    );
    expect(en.settings.general.shortcut.bindings.cancel.name).toBe(
      "Cancel shortcut",
    );
    expect(
      en.settings.general.shortcut.bindings.transcribe_with_post_process.name,
    ).toBe("Clean-up shortcut");
    expect(en.settings.debug.muteWhileRecording.label).toBe(
      "Mute while recording",
    );
    expect(en.settings.sound.audioFeedback.label).toBe("Recording sounds");
    expect(en.settings.sound.outputDevice.title).toBe("Output device");
    expect(en.appLanguage.title).toBe("App language");
    expect(en.theme.title).toBe("App theme");
    expect(en.settings.debug.updateChecks.label).toBe("Check for updates");
    expect(en.settings.about.whatsNewUpdates.label).toBe("Show what's new");
    expect(en.settings.about.sourceCode.title).toBe("Source code");
    expect(en.settings.about.appDataDirectory.title).toBe("App data folder");
    expect(en.settings.debug.logDirectory.title).toBe("Log folder");

    const generalHtml = renderWithSettings(<GeneralSettings />, {
      push_to_talk: false,
      bindings: {
        transcribe: { id: "transcribe", current_binding: "fn" },
        cancel: { id: "cancel", current_binding: "escape" },
      },
    });
    for (const label of [
      "Dictation shortcut",
      "Hold to talk",
      "Cancel shortcut",
      "Mute while recording",
      "Recording sounds",
      "Output device",
    ]) {
      expect(generalHtml).toContain(`>${label}<`);
    }
    expect(generalHtml).not.toContain("Push To Talk");
    expect(generalHtml).not.toContain("Mute While Recording");
    expect(generalHtml).not.toContain("Audio Feedback");

    const aboutHtml = renderWithSettings(<AboutSettings />);
    for (const label of [
      "App language",
      "App theme",
      "Check for updates",
      "Show what's new",
      "Source code",
    ]) {
      expect(aboutHtml).toContain(`>${label}<`);
    }
    expect(aboutHtml).not.toContain("Application Language");
    expect(aboutHtml).not.toContain("Application Theme");

    for (const loc of readdirSync(localesDir).sort()) {
      const text = readFileSync(
        join(localesDir, loc, "translation.json"),
        "utf8",
      );
      expect(text).not.toContain("Push To Talk");
      expect(text).not.toContain("Mute While Recording");
      expect(text).not.toContain("Application Language");
      expect(text).not.toContain("Application Theme");
    }
  });

  test("Advanced uses plain sentence-case labels across App, Output, Transcription, History, and Experimental", () => {
    expect(en.settings.advanced.clipboardHandling.options.copyToClipboard).toBe(
      "Copy to clipboard",
    );
    expect(en.settings.advanced.acceleration.ort.title).toBe(
      "ONNX acceleration",
    );

    const advancedHtml = renderWithSettings(<AdvancedSettings />, {
      experimental_enabled: true,
      overlay_style: "minimal",
    });
    for (const label of [
      "Start hidden",
      "Open at login",
      "Show in the menu bar",
      "Recording pill",
      "Where the pill shows",
      "Unload model",
      "Experimental features",
      "How words are typed",
      "Clipboard after pasting",
      "Keep previous clipboard",
      "Delay before paste",
      "Delay after paste",
      "Press key after paste",
      "Skip silence",
      "Custom words",
      "Add a space at the end",
      "History limit",
      "Auto-delete recordings",
      "Keyboard handler",
      "transcribe.cpp acceleration",
      "Keep mic open between transcriptions",
    ]) {
      expect(advancedHtml).toContain(label);
    }
    for (const banned of [
      "Voice Activity Detection",
      "Append Trailing Space",
      "Overlay Position",
      "Paste Method",
      "Clipboard Handling",
      "Don't Modify Clipboard",
      "Auto Submit",
      "Start Hidden",
      "Show Tray Icon",
      "Experimental Features",
    ]) {
      expect(advancedHtml).not.toContain(banned);
    }

    for (const loc of readdirSync(localesDir).sort()) {
      const raw = readFileSync(
        join(localesDir, loc, "translation.json"),
        "utf8",
      );
      const parsed = JSON.parse(raw) as {
        settings: { advanced: { overlay: { style: { title: string } } } };
      };
      expect(parsed.settings.advanced.overlay.style.title).not.toBe("Overlay");
      expect(raw).not.toContain("Voice Activity Detection");
      expect(raw).not.toContain("VAD tail");
      expect(raw).not.toContain("Don't Modify Clipboard");
    }
  });

  test("Clean up matches Spoken fixes and uses plain style labels across all 22 locales", () => {
    expect(en.settings.postProcessing.hotkey.title).toBe("Shortcut");
    expect(en.settings.postProcessing.correction.title).toBe("Spoken fixes");
    expect(en.settings.postProcessing.correction.toggleLabel).toBe(
      "Spoken fixes",
    );
    expect(en.settings.general.shortcut.bindings.correction.name).toBe(
      "Spoken fixes shortcut",
    );
    expect(en.settings.postProcessing.api.title).toBe("Clean-up model");
    expect(en.settings.postProcessing.api.apiKey.title).toBe("API key");
    expect(en.settings.postProcessing.prompts.title).toBe("Instructions");
    expect(en.settings.postProcessing.prompts.selectedPrompt.title).toBe(
      "Clean-up style",
    );
    expect(en.settings.postProcessing.prompts.createNew).toBe("New style");
    expect(en.settings.postProcessing.prompts.promptLabel).toBe("Name");
    expect(en.settings.postProcessing.prompts.promptInstructions).toBe(
      "Instructions",
    );
    expect(en.settings.postProcessing.prompts.updatePrompt).toBe(
      "Save changes",
    );
    expect(en.settings.postProcessing.prompts.deletePrompt).toBe("Delete");
    expect(en.settings.postProcessing.prompts.createPrompt).toBe("Create");

    const cleanupHtml = renderWithSettings(<PostProcessingSettings />, {
      iterative_correction_enabled: true,
      post_process_selected_prompt_id: "p1",
      post_process_prompts: [
        { id: "p1", name: "Default", prompt: "Fix ${output}" },
      ],
      bindings: {
        transcribe_with_post_process: {
          id: "transcribe_with_post_process",
          current_binding: "",
        },
        correction: { id: "correction", current_binding: "" },
      },
    });
    for (const label of [
      "Shortcut",
      "Spoken fixes",
      "Spoken fixes shortcut",
      "Clean-up model",
      "Instructions",
      "Clean-up style",
      "New style",
      "Name",
      "Save changes",
      "Delete",
    ]) {
      expect(cleanupHtml).toContain(`>${label}<`);
    }
    const bannedCleanup = [
      "API (OpenAI Compatible)",
      "Iterative Correction",
      "OpenAI Compatible",
      "Speak Corrections",
      "Selected Prompt",
      "Create New Prompt",
      "Prompt Label",
      "Prompt Instructions",
      "Update Prompt",
      "Delete Prompt",
    ];
    for (const banned of bannedCleanup) {
      expect(cleanupHtml).not.toContain(banned);
    }

    for (const loc of readdirSync(localesDir).sort()) {
      const text = readFileSync(
        join(localesDir, loc, "translation.json"),
        "utf8",
      );
      const parsed = JSON.parse(text) as {
        settings: {
          postProcessing: {
            correction: { title: string; toggleLabel: string };
          };
        };
      };
      expect(parsed.settings.postProcessing.correction.toggleLabel).toBe(
        parsed.settings.postProcessing.correction.title,
      );
      for (const banned of bannedCleanup) {
        expect(text).not.toContain(banned);
      }
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
      "src/lib/utils/theme.ts",
    ]);

    const themeSource = readFileSync(
      join(rootDir, "src/lib/utils/theme.ts"),
      "utf8",
    );
    expect((themeSource.match(/handy/gi) ?? []).length).toBe(1);

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

  test("G4 carry-over uses sentence case for openFolder and microphone error titles across 22 locales", () => {
    expect(en.settings.history.openFolder).toBe("Open recordings folder");
    expect(en.errors.micPermissionDeniedTitle).toBe("Microphone access denied");
    expect(en.errors.noInputDeviceTitle).toBe("No microphone found");
    for (const loc of readdirSync(localesDir).sort()) {
      const parsed = JSON.parse(
        readFileSync(join(localesDir, loc, "translation.json"), "utf8"),
      ) as typeof en;
      expect(parsed.settings.history.openFolder).not.toBe(
        "Open Recordings Folder",
      );
      expect(parsed.errors.micPermissionDeniedTitle).not.toBe(
        "Microphone Access Denied",
      );
      expect(parsed.errors.noInputDeviceTitle).not.toBe("No Microphone Found");
    }
  });
});
