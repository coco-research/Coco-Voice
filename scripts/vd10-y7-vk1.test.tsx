import { describe, expect, mock, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import i18next from "i18next";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { ReadFocusedTextSection } from "../src/components/settings/ReadFocusedTextCard";
import { useModelStore } from "../src/stores/modelStore";
import { useSettingsStore } from "../src/stores/settingsStore";
import { verifyY7PrivacyGuards } from "./privacy-scan";

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

const { SettingsV022Page } =
  await import("../src/components/settings/SettingsV022Page");
const { ModelsSettings } =
  await import("../src/components/settings/models/ModelsSettings");
const { AboutSettings } =
  await import("../src/components/settings/about/AboutSettings");
const { WordsAndCleanupV022 } =
  await import("../src/components/settings/post-processing/PostProcessingSettings");

const root = join(import.meta.dir, "..");

async function createTestI18n() {
  const instance = i18next.createInstance();
  await instance.use(initReactI18next).init({
    lng: "en",
    resources: { en: { translation: en } },
    interpolation: { escapeValue: false },
  });
  return instance;
}

describe("V-D10 focused text formatting, Y7 privacy guards, and V-K1 seeded winner", () => {
  test("renders ReadFocusedTextCard off by default with local-only disclosure", async () => {
    const i18n = await createTestI18n();
    const offHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <ReadFocusedTextSection
          enabled={false}
          windowContextEnabled={false}
          onToggleEnabled={() => {}}
          onToggleWindowContext={() => {}}
        />
      </I18nextProvider>,
    );
    expect(offHtml).toContain('data-testid="read-focused-text-card"');
    expect(offHtml).toContain("Off by default · Stays on this Mac");
    expect(offHtml).toContain(
      "Read active text box to format spacing, capitalization, and spoken edits",
    );
    expect(offHtml).toContain("Also match names visible in the active window");

    const onHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <ReadFocusedTextSection
          enabled={true}
          windowContextEnabled={true}
          onToggleEnabled={() => {}}
          onToggleWindowContext={() => {}}
        />
      </I18nextProvider>,
    );
    expect(onHtml).toContain('data-testid="read-focused-text-on"');
  });

  test("mounts ReadFocusedTextCard and preserved General/Advanced controls without removing any setting", () => {
    const profiles = readFileSync(
      join(root, "src/components/settings/AppProfilesView.tsx"),
      "utf8",
    );
    const general = readFileSync(
      join(root, "src/components/settings/general/GeneralSettings.tsx"),
      "utf8",
    );
    const v022 = readFileSync(
      join(root, "src/components/settings/SettingsV022Page.tsx"),
      "utf8",
    );
    const words = readFileSync(
      join(
        root,
        "src/components/settings/post-processing/PostProcessingSettings.tsx",
      ),
      "utf8",
    );
    // AC-P32: ReadFocusedTextCard lives once in Settings, not duplicated on AppProfilesView.
    expect(profiles).not.toContain("ReadFocusedTextCard");
    expect(general).toContain("ReadFocusedTextCard");
    expect(v022).toContain("ReadFocusedTextCard");
    expect(v022).toContain("AgentsCanAskCard");
    expect(v022).toContain("ClamshellMicrophoneSelector");
    expect(v022).toContain("MuteWhileRecording");
    expect(v022).toContain("OutputDeviceSelector");
    expect(v022).toContain("VolumeSlider");
    expect(v022).toContain("AdvancedSettings");
    expect(words).toContain('data-testid="words-preserved-post-processing"');
  });

  test("verifies Y7 do-not-copy privacy guards across the repository", () => {
    const result = verifyY7PrivacyGuards(root);
    expect(result.usageOrWordCountViolations).toEqual([]);
    expect(result.audioOrFeedbackUploadViolations).toEqual([]);
    expect(result.unapprovedOutboundViolations).toEqual([]);
  });

  test("seeds measured_runs.json with real M1 wer-dev-40 runs where Parakeet Ultra wins", () => {
    const measured = JSON.parse(
      readFileSync(
        join(root, "src-tauri/src/catalog/measured_runs.json"),
        "utf8",
      ),
    ) as {
      runs: Array<{
        model_id: string;
        clip_set: string;
        words_wrong_per_100: number;
        speed_x_realtime: number;
        peak_memory_mb: number;
      }>;
    };
    expect(measured.runs.length).toBeGreaterThanOrEqual(2);
    const ultra = measured.runs.find((r) =>
      r.model_id.includes("parakeet-ultra"),
    );
    const v3 = measured.runs.find((r) =>
      r.model_id.includes("parakeet-tdt-0.6b-v3"),
    );
    expect(ultra).toBeDefined();
    expect(v3).toBeDefined();
    expect(ultra!.words_wrong_per_100).toBeLessThan(v3!.words_wrong_per_100);
    expect(measured.runs.some((r) => r.model_id.includes("canary-180m"))).toBe(
      false,
    );
    expect(
      measured.runs.some((r) => r.model_id.includes("whisper-medium")),
    ).toBe(false);
    expect(
      measured.runs.some((r) => r.model_id.includes("cohere-transcribe")),
    ).toBe(false);
  });

  test("renders v0.2.2 Settings 4-card grid, Models with local Qwen3-4B section, About, and two-column Words and clean-up", async () => {
    const i18n = await createTestI18n();
    const g = globalThis as { window?: unknown };
    const prevWin = Object.getOwnPropertyDescriptor(globalThis, "window");
    g.window = { __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" } };

    const prevLoading = useSettingsStore.getState().isLoading;
    const prevSettings = useSettingsStore.getState().settings;
    const prevModels = useModelStore.getState().models;
    const prevModelLoading = useModelStore.getState().loading;
    const prevCurrentModel = useModelStore.getState().currentModel;

    const nextSettingsState = {
      isLoading: false,
      settings: {
        ...(prevSettings ?? {}),
        push_to_talk: true,
        paste_delay_ms: 60,
        post_process_enabled: true,
        filler_word_removal_enabled: true,
        update_checks_enabled: true,
        custom_words: ["Coco Voice", "Parakeet"],
      } as never,
    };
    const nextModelState = {
      loading: false,
      currentModel:
        "handy-computer/parakeet-ultra-gguf/parakeet-ultra-Q8_0.gguf",
      models: [
        {
          id: "handy-computer/parakeet-ultra-gguf/parakeet-ultra-Q8_0.gguf",
          name: "Parakeet Ultra",
          description: "Fast local speech model",
          filename: "parakeet-ultra-Q8_0.gguf",
          source: "Local",
          size_mb: 612,
          is_downloaded: true,
          is_downloading: false,
          partial_size: 0,
          is_directory: false,
          engine_type: "Parakeet",
          accuracy_score: 0.94,
          speed_score: 0.96,
          supports_translation: false,
          is_recommended: true,
          measured_best: {
            clip_set: "wer-dev-40",
            words_wrong_per_100: 9.2,
            speed_x_realtime: 24.8,
            peak_memory_mb: 1007,
            models_compared: 2,
          },
          supported_languages: ["en"],
          supports_language_selection: true,
          is_custom: false,
          supports_streaming: true,
          supports_language_detection: true,
        } as never,
      ],
    };
    Object.assign(useSettingsStore.getInitialState(), nextSettingsState);
    useSettingsStore.setState(nextSettingsState);
    Object.assign(useModelStore.getInitialState(), nextModelState);
    useModelStore.setState(nextModelState);

    try {
      const settingsHtml = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <SettingsV022Page />
        </I18nextProvider>,
      ).replaceAll("&#x27;", "'");
      expect(settingsHtml).toContain("Shortcut and microphone");
      expect(settingsHtml).toContain("Typing");
      expect(settingsHtml).toContain("Meetings");
      expect(settingsHtml).toContain("App");
      expect(settingsHtml).toContain("Keep meeting audio");
      expect(settingsHtml).toContain("Notes as files");

      const modelsHtml = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <ModelsSettings />
        </I18nextProvider>,
      );
      expect(modelsHtml).toContain("Speech: used for dictation and meetings");
      expect(modelsHtml).toContain("Parakeet Ultra");
      expect(modelsHtml).toContain('data-testid="recommended-badge"');
      expect(modelsHtml).toContain("Clean-up and notes: runs on this Mac");
      expect(modelsHtml).toContain("Qwen3-4B");
      expect(modelsHtml).toContain("In use");

      const aboutHtml = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <AboutSettings />
        </I18nextProvider>,
      ).replaceAll("&#x27;", "'");
      expect(aboutHtml).toContain("Coco Voice");
      expect(aboutHtml).not.toContain("0.20.0");
      expect(aboutHtml).toContain("What's new");
      expect(aboutHtml).toContain("Check for updates");
      expect(aboutHtml).toContain("Source code");
      expect(aboutHtml).toContain('data-testid="what-goes-online"');

      const wordsHtml = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <WordsAndCleanupV022 />
        </I18nextProvider>,
      ).replaceAll("&quot;", '"');
      expect(wordsHtml).toContain("Words and clean-up");
      expect(wordsHtml).toContain("Your words");
      expect(wordsHtml).toContain("Clean up");
      expect(wordsHtml).toContain("Clean every take");
      expect(wordsHtml).toContain("Remove filler words");
      expect(wordsHtml).toContain("Where clean-up runs");
      expect(wordsHtml).toContain("On this Mac (Qwen3-4B)");
      expect(wordsHtml).not.toContain("cocoa voice");
      expect(wordsHtml).toContain('data-testid="saved-blocks-card"');
    } finally {
      Object.assign(useSettingsStore.getInitialState(), {
        isLoading: prevLoading,
        settings: prevSettings,
      });
      useSettingsStore.setState({
        isLoading: prevLoading,
        settings: prevSettings,
      });
      Object.assign(useModelStore.getInitialState(), {
        models: prevModels,
        loading: prevModelLoading,
        currentModel: prevCurrentModel,
      });
      useModelStore.setState({
        models: prevModels,
        loading: prevModelLoading,
        currentModel: prevCurrentModel,
      });
      if (prevWin) Object.defineProperty(globalThis, "window", prevWin);
      else delete g.window;
    }
  });
});
