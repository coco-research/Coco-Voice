import { describe, expect, mock, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import i18next from "i18next";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { I18nContext, I18nextProvider, initReactI18next } from "react-i18next";
import type { ModelInfo } from "../src/bindings";
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
const PARAKEET_ULTRA_ID =
  "handy-computer/parakeet-ultra-gguf/parakeet-ultra-Q8_0.gguf";

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
      currentModel: PARAKEET_ULTRA_ID,
      models: [
        {
          id: PARAKEET_ULTRA_ID,
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

  test("clean-up card binds only to a LlamaCpp clean-up model, never Qwen3-ASR or qwen- speech models", async () => {
    const i18n = await createTestI18n();
    const DEFAULT_CLEANUP_ID = "Qwen/Qwen3-4B-GGUF/Qwen3-4B-Q4_K_M.gguf";
    const QWEN_ASR_ID =
      "handy-computer/Qwen3-ASR-0.6B-gguf/Qwen3-ASR-0.6B-Q8_0.gguf";

    const makeModel = (overrides: Partial<ModelInfo>): ModelInfo => ({
      id: PARAKEET_ULTRA_ID,
      name: "Parakeet Ultra",
      description: "Fast local speech model",
      filename: "parakeet-ultra-Q8_0.gguf",
      url: null,
      size_mb: 612,
      is_downloaded: false,
      is_downloading: false,
      partial_size: 0,
      is_directory: false,
      engine_type: "Parakeet",
      accuracy_score: 0.94,
      speed_score: 0.96,
      supports_translation: false,
      is_recommended: false,
      supported_languages: ["en"],
      supports_language_selection: true,
      is_custom: false,
      supports_streaming: true,
      supports_language_detection: true,
      measured_best: null,
      ...overrides,
    });

    const qwenAsrSpeech = makeModel({
      id: QWEN_ASR_ID,
      name: "Qwen3-ASR 0.6B",
      filename: "Qwen3-ASR-0.6B-Q8_0.gguf",
      engine_type: "TranscribeCpp",
      size_mb: 811,
    });
    const qwenPrefixSpeech = makeModel({
      id: "qwen-speech-first",
      name: "qwen-speech-first",
      filename: "qwen-speech-first.gguf",
      engine_type: "TranscribeCpp",
      size_mb: 500,
    });
    const gemmaLlamaSpeech = makeModel({
      id: "gemma-speech-test",
      name: "llama-speech-test",
      filename: "gemma-speech-test.gguf",
      engine_type: "Whisper",
      size_mb: 400,
    });
    const qwenCleanup = makeModel({
      id: DEFAULT_CLEANUP_ID,
      name: "Qwen3 4B (Local LLM)",
      filename: "Qwen3-4B-Q4_K_M.gguf",
      engine_type: "LlamaCpp",
      size_mb: 2382,
    });
    const otherCleanup = makeModel({
      id: "custom/other-cleanup-model.gguf",
      name: "Other Cleanup LLM",
      filename: "other-cleanup-model.gguf",
      engine_type: "LlamaCpp",
      size_mb: 1500,
    });

    type ReactElem = React.ReactElement<{
      children?: React.ReactNode;
      disabled?: boolean;
      onClick?: () => void;
      "data-testid"?: string;
    }>;

    const renderModelsTree = (): ReactElem => {
      const internals = (
        React as unknown as {
          __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: {
            ReactCurrentDispatcher: { current: unknown };
          };
        }
      ).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED
        .ReactCurrentDispatcher;
      const prevDispatcher = internals.current;
      internals.current = {
        useState: <T,>(init: T | (() => T)) => [
          typeof init === "function" ? (init as () => T)() : init,
          () => {},
        ],
        useRef: <T,>(init: T) => ({ current: init }),
        useEffect: () => {},
        useLayoutEffect: () => {},
        useMemo: <T,>(fn: () => T) => fn(),
        useCallback: <T,>(fn: T) => fn,
        useDebugValue: () => {},
        useContext: (ctx: unknown) =>
          ctx === I18nContext
            ? { i18n }
            : (ctx as { _currentValue?: unknown })._currentValue,
        useSyncExternalStore: (_sub: unknown, getSnapshot: () => unknown) =>
          getSnapshot(),
      };
      try {
        return (ModelsSettings as (props: Record<string, never>) => ReactElem)(
          {},
        );
      } finally {
        internals.current = prevDispatcher;
      }
    };

    const findElement = (
      node: React.ReactNode,
      pred: (el: ReactElem) => boolean,
    ): ReactElem | null => {
      if (!node) return null;
      if (Array.isArray(node)) {
        for (const child of node) {
          const hit = findElement(child, pred);
          if (hit) return hit;
        }
        return null;
      }
      if (React.isValidElement(node)) {
        const el = node as ReactElem;
        if (pred(el)) return el;
        return findElement(el.props.children, pred);
      }
      return null;
    };

    const invokedDownloads: string[] = [];
    const g = globalThis as { window?: unknown };
    const prevWin = Object.getOwnPropertyDescriptor(globalThis, "window");
    const prevSettingsState = {
      isLoading: useSettingsStore.getState().isLoading,
      settings: useSettingsStore.getState().settings,
    };
    const prevModelState = {
      models: useModelStore.getState().models,
      loading: useModelStore.getState().loading,
      currentModel: useModelStore.getState().currentModel,
      downloadingModels: useModelStore.getState().downloadingModels,
      downloadProgress: useModelStore.getState().downloadProgress,
      downloadFailures: useModelStore.getState().downloadFailures,
    };

    g.window = {
      __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" },
      __TAURI_INTERNALS__: {
        invoke: async (cmd: string, args?: { modelId?: string }) => {
          if (cmd === "download_model" && args?.modelId) {
            invokedDownloads.push(args.modelId);
          }
          return null;
        },
        transformCallback: () => 0,
      },
    };

    const setModelsFixture = (models: ModelInfo[]) => {
      const nextSettings = {
        isLoading: false,
        settings: {
          ...(prevSettingsState.settings ?? {}),
          post_process_enabled: false,
          post_process_provider_id: "local_llm",
          post_process_models: { local_llm: DEFAULT_CLEANUP_ID },
        } as never,
      };
      const nextModels = {
        loading: false,
        currentModel: "",
        models,
        downloadingModels: {},
        downloadProgress: {},
        downloadFailures: {},
      };
      Object.assign(useSettingsStore.getInitialState(), nextSettings);
      useSettingsStore.setState(nextSettings);
      Object.assign(useModelStore.getInitialState(), nextModels);
      useModelStore.setState(nextModels);
    };

    const splitSections = (html: string) => {
      const cleanupIdx = html.indexOf(
        'data-testid="local-cleanup-models-section"',
      );
      return {
        speechHtml: html.slice(0, cleanupIdx),
        cleanupHtml: html.slice(cleanupIdx),
      };
    };

    try {
      // Case 1: Real catalog order — Qwen3-ASR 0.6B speech model before Qwen3-4B LlamaCpp.
      setModelsFixture([qwenAsrSpeech, qwenCleanup]);
      const html1 = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <ModelsSettings />
        </I18nextProvider>,
      );
      const s1 = splitSections(html1);
      expect(s1.speechHtml).toContain(`data-model-row-id="${QWEN_ASR_ID}"`);
      expect(s1.speechHtml).toContain("Qwen3-ASR 0.6B");
      expect(s1.speechHtml).not.toContain(DEFAULT_CLEANUP_ID);
      expect(s1.cleanupHtml).toContain("Qwen3 4B (Local LLM)");
      expect(s1.cleanupHtml).not.toContain("Qwen3-ASR 0.6B");

      const tree1 = renderModelsTree();
      const cleanupSec1 = findElement(
        tree1,
        (el) => el.props["data-testid"] === "local-cleanup-models-section",
      );
      const downloadBtn1 = findElement(
        cleanupSec1,
        (el) => el.type === "button" && el.props.children === "Download",
      );
      expect(downloadBtn1).not.toBeNull();
      expect(downloadBtn1?.props.disabled).toBe(false);
      invokedDownloads.length = 0;
      downloadBtn1?.props.onClick?.();
      await Promise.resolve();
      expect(invokedDownloads).toEqual([DEFAULT_CLEANUP_ID]);

      // Case 2: Qwen3-4B absent — clean-up card has no model, never binds to Qwen3-ASR.
      setModelsFixture([qwenAsrSpeech]);
      const html2 = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <ModelsSettings />
        </I18nextProvider>,
      );
      const s2 = splitSections(html2);
      expect(s2.speechHtml).toContain(`data-model-row-id="${QWEN_ASR_ID}"`);
      expect(s2.cleanupHtml).not.toContain("Qwen3-ASR 0.6B");
      const tree2 = renderModelsTree();
      const cleanupSec2 = findElement(
        tree2,
        (el) => el.props["data-testid"] === "local-cleanup-models-section",
      );
      const downloadBtn2 = findElement(
        cleanupSec2,
        (el) => el.type === "button" && el.props.children === "Download",
      );
      expect(downloadBtn2?.props.disabled).toBe(true);
      invokedDownloads.length = 0;
      downloadBtn2?.props.onClick?.();
      await Promise.resolve();
      expect(invokedDownloads).toEqual([]);

      // Case 3a: Speech model whose id and name start with "qwen-" placed first (with 4B present).
      setModelsFixture([qwenPrefixSpeech, qwenCleanup]);
      const html3a = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <ModelsSettings />
        </I18nextProvider>,
      );
      const s3a = splitSections(html3a);
      expect(s3a.speechHtml).toContain('data-model-row-id="qwen-speech-first"');
      expect(s3a.cleanupHtml).toContain("Qwen3 4B (Local LLM)");
      expect(s3a.cleanupHtml).not.toContain("qwen-speech-first");
      const tree3a = renderModelsTree();
      const cleanupSec3a = findElement(
        tree3a,
        (el) => el.props["data-testid"] === "local-cleanup-models-section",
      );
      const downloadBtn3a = findElement(
        cleanupSec3a,
        (el) => el.type === "button" && el.props.children === "Download",
      );
      expect(downloadBtn3a?.props.disabled).toBe(false);
      invokedDownloads.length = 0;
      downloadBtn3a?.props.onClick?.();
      await Promise.resolve();
      expect(invokedDownloads).toEqual([DEFAULT_CLEANUP_ID]);

      // Case 3b: Speech models named "qwen-..." / "gemma-..." / "llama-..." with no LlamaCpp model.
      setModelsFixture([qwenPrefixSpeech, gemmaLlamaSpeech]);
      const html3b = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <ModelsSettings />
        </I18nextProvider>,
      );
      const s3b = splitSections(html3b);
      expect(s3b.speechHtml).toContain('data-model-row-id="qwen-speech-first"');
      expect(s3b.speechHtml).toContain('data-model-row-id="gemma-speech-test"');
      expect(s3b.cleanupHtml).not.toContain("qwen-speech-first");
      expect(s3b.cleanupHtml).not.toContain("llama-speech-test");
      const tree3b = renderModelsTree();
      const cleanupSec3b = findElement(
        tree3b,
        (el) => el.props["data-testid"] === "local-cleanup-models-section",
      );
      const downloadBtn3b = findElement(
        cleanupSec3b,
        (el) => el.type === "button" && el.props.children === "Download",
      );
      expect(downloadBtn3b?.props.disabled).toBe(true);
      invokedDownloads.length = 0;
      downloadBtn3b?.props.onClick?.();
      await Promise.resolve();
      expect(invokedDownloads).toEqual([]);

      // Case 4: Multiple LlamaCpp models — prefers exact LOCAL_LLM_DEFAULT_MODEL_ID and hides all LlamaCpp from speech.
      setModelsFixture([otherCleanup, qwenCleanup]);
      const html4 = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <ModelsSettings />
        </I18nextProvider>,
      );
      const s4 = splitSections(html4);
      expect(s4.speechHtml).not.toContain("other-cleanup-model");
      expect(s4.speechHtml).not.toContain(DEFAULT_CLEANUP_ID);
      expect(s4.cleanupHtml).toContain("Qwen3 4B (Local LLM)");
      const tree4 = renderModelsTree();
      const cleanupSec4 = findElement(
        tree4,
        (el) => el.props["data-testid"] === "local-cleanup-models-section",
      );
      const downloadBtn4 = findElement(
        cleanupSec4,
        (el) => el.type === "button" && el.props.children === "Download",
      );
      invokedDownloads.length = 0;
      downloadBtn4?.props.onClick?.();
      await Promise.resolve();
      expect(invokedDownloads).toEqual([DEFAULT_CLEANUP_ID]);
    } finally {
      Object.assign(useSettingsStore.getInitialState(), prevSettingsState);
      useSettingsStore.setState(prevSettingsState);
      Object.assign(useModelStore.getInitialState(), prevModelState);
      useModelStore.setState(prevModelState);
      if (prevWin) Object.defineProperty(globalThis, "window", prevWin);
      else delete g.window;
    }
  });
});
