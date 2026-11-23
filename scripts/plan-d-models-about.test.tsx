import { describe, expect, mock, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { useModelStore } from "../src/stores/modelStore";
import { useSettingsStore } from "../src/stores/settingsStore";
import {
  ONLINE_CLOUD_PROVIDERS,
  WhatGoesOnlineSummaryCard,
} from "../src/components/settings/about/WhatGoesOnline";

mock.module("../src/components/whats-new/releaseNotes", () => ({
  findReleaseNoteToShow: () => null,
  findLatestReleaseNote: () => ({
    version: "0.20.0-test",
    markdown: "- Meetings and a new layout",
  }),
}));
mock.module("../src/i18n", () => ({
  default: i18next,
  SUPPORTED_LANGUAGES: [{ code: "en", name: "English", nativeName: "English" }],
  syncLanguageFromSettings: async () => {},
  getLanguageDirection: () => "ltr",
  isRTLLanguage: () => false,
}));

const { ModelsSettings, formatCleanModelSize, formatSpeechModelSubtitle } =
  await import("../src/components/settings/models/ModelsSettings");
const { AboutSettings } =
  await import("../src/components/settings/about/AboutSettings");

const ROOT = join(import.meta.dir, "..");

async function makeI18n() {
  const inst = i18next.createInstance();
  await inst.use(initReactI18next).init({
    lng: "en",
    resources: { en: { translation: en } },
    interpolation: { escapeValue: false },
  });
  return inst;
}

function makeModel(overrides: Record<string, unknown>) {
  return {
    id: "speech-1",
    name: "Speech Model 1",
    description: "Fast local speech model",
    filename: "speech-1.gguf",
    source: "Local",
    size_mb: 500,
    is_downloaded: false,
    is_downloading: false,
    partial_size: 0,
    is_directory: false,
    engine_type: "Parakeet",
    accuracy_score: 0.9,
    speed_score: 0.9,
    supports_translation: false,
    is_recommended: false,
    measured_best: null,
    supported_languages: ["en"],
    supports_language_selection: true,
    is_custom: false,
    supports_streaming: true,
    supports_language_detection: true,
    ...overrides,
  };
}

function withStores<T>(
  settingsPatch: Record<string, unknown>,
  modelPatch: Record<string, unknown>,
  fn: () => T,
): T {
  const g = globalThis as { window?: unknown };
  const prevWin = Object.getOwnPropertyDescriptor(globalThis, "window");
  g.window = { __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" } };
  const prevSettingsLoading = useSettingsStore.getState().isLoading;
  const prevSettings = useSettingsStore.getState().settings;
  const prevModels = useModelStore.getState().models;
  const prevModelLoading = useModelStore.getState().loading;
  const prevCurrentModel = useModelStore.getState().currentModel;
  const prevDownloading = useModelStore.getState().downloadingModels;
  const prevProgress = useModelStore.getState().downloadProgress;
  const prevFailures = useModelStore.getState().downloadFailures;

  const nextSettings = {
    isLoading: false,
    settings: { ...(prevSettings ?? {}), ...settingsPatch } as never,
  };
  const nextModels = {
    loading: false,
    downloadingModels: {},
    downloadProgress: {},
    downloadFailures: {},
    ...modelPatch,
  };

  Object.assign(useSettingsStore.getInitialState(), nextSettings);
  useSettingsStore.setState(nextSettings);
  Object.assign(useModelStore.getInitialState(), nextModels);
  useModelStore.setState(nextModels as never);

  try {
    return fn();
  } finally {
    Object.assign(useSettingsStore.getInitialState(), {
      isLoading: prevSettingsLoading,
      settings: prevSettings,
    });
    useSettingsStore.setState({
      isLoading: prevSettingsLoading,
      settings: prevSettings,
    });
    Object.assign(useModelStore.getInitialState(), {
      models: prevModels,
      loading: prevModelLoading,
      currentModel: prevCurrentModel,
      downloadingModels: prevDownloading,
      downloadProgress: prevProgress,
      downloadFailures: prevFailures,
    });
    useModelStore.setState({
      models: prevModels,
      loading: prevModelLoading,
      currentModel: prevCurrentModel,
      downloadingModels: prevDownloading,
      downloadProgress: prevProgress,
      downloadFailures: prevFailures,
    });
    if (prevWin) Object.defineProperty(globalThis, "window", prevWin);
    else delete g.window;
  }
}

describe("Plan D Models (D6) and About (D7)", () => {
  test("AC-D12: renders exact two group headings, ALL speech models without hidden cap, and progress/failure states", async () => {
    const i18n = await makeI18n();
    const models = [
      makeModel({
        id: "parakeet-ultra",
        name: "Parakeet Ultra",
        is_downloaded: true,
        measured_best: {
          clip_set: "wer-dev-40",
          words_wrong_per_100: 9.2,
          speed_x_realtime: 24.8,
          peak_memory_mb: 1007,
          models_compared: 2,
        },
      }),
      makeModel({
        id: "whisper-small",
        name: "Whisper Small",
        is_downloaded: true,
      }),
      makeModel({ id: "whisper-medium", name: "Whisper Medium" }),
      makeModel({ id: "whisper-large", name: "Whisper Large" }),
      makeModel({ id: "sense-voice", name: "SenseVoice" }),
      makeModel({
        id: "qwen3-4b",
        name: "Qwen3-4B",
        engine_type: "LlamaCpp",
        size_mb: 2560,
        is_downloaded: true,
      }),
    ];

    const html = withStores(
      {
        post_process_enabled: true,
        post_process_provider_id: "local_llm",
        post_process_models: { local_llm: "qwen3-4b" },
      },
      {
        currentModel: "parakeet-ultra",
        models,
        downloadingModels: { "whisper-medium": true },
        downloadProgress: {
          "whisper-medium": {
            model_id: "whisper-medium",
            downloaded: 42,
            total: 100,
            percentage: 42,
          },
        },
        downloadFailures: { "whisper-large": "network" },
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <ModelsSettings />
          </I18nextProvider>,
        ),
    );

    expect(html).toContain("Speech: used for dictation and meetings");
    expect(html).toContain("Clean-up and notes: runs on this Mac");
    // All 5 speech models rendered without hidden cap.
    for (const name of [
      "Parakeet Ultra",
      "Whisper Small",
      "Whisper Medium",
      "Whisper Large",
      "SenseVoice",
    ]) {
      expect(html).toContain(name);
    }
    expect(html).not.toContain("Show all");
    expect(html).toContain("42%");
    expect(html).toContain("Cancel");
    expect(html).toContain("Download failed: check your connection.");
    expect(html).toContain("Try again");
    expect(html).toContain('data-testid="cleanup-model-in-use"');
  });

  test("AC-D13: Qwen3-4B In use is false for Apple Intelligence, cloud providers, and undownloaded local_llm", async () => {
    const i18n = await makeI18n();

    // Case 1: Apple Intelligence selected while Qwen3-4B is downloaded => NOT In use, shows Use + Delete.
    const appleHtml = withStores(
      {
        post_process_enabled: true,
        post_process_provider_id: "apple_intelligence",
      },
      {
        currentModel: "parakeet-ultra",
        models: [
          makeModel({
            id: "parakeet-ultra",
            name: "Parakeet Ultra",
            is_downloaded: true,
          }),
          makeModel({
            id: "qwen3-4b",
            name: "Qwen3-4B",
            engine_type: "LlamaCpp",
            is_downloaded: true,
          }),
        ],
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <ModelsSettings />
          </I18nextProvider>,
        ),
    );
    expect(appleHtml).not.toContain('data-testid="cleanup-model-in-use"');
    const cleanupSlice1 = appleHtml.slice(
      appleHtml.indexOf('data-testid="local-cleanup-models-section"'),
    );
    expect(cleanupSlice1).toContain(">Use<");
    expect(cleanupSlice1).toContain(">Delete<");

    // Case 2: local_llm selected, but Qwen3-4B is NOT downloaded => NOT In use, shows Download.
    const notDownloadedHtml = withStores(
      {
        post_process_enabled: true,
        post_process_provider_id: "local_llm",
      },
      {
        currentModel: "parakeet-ultra",
        models: [
          makeModel({
            id: "parakeet-ultra",
            name: "Parakeet Ultra",
            is_downloaded: true,
          }),
          makeModel({
            id: "qwen3-4b",
            name: "Qwen3-4B",
            engine_type: "LlamaCpp",
            is_downloaded: false,
          }),
        ],
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <ModelsSettings />
          </I18nextProvider>,
        ),
    );
    expect(notDownloadedHtml).not.toContain(
      'data-testid="cleanup-model-in-use"',
    );
    const cleanupSlice2 = notDownloadedHtml.slice(
      notDownloadedHtml.indexOf('data-testid="local-cleanup-models-section"'),
    );
    expect(cleanupSlice2).toContain(">Download<");
  });

  test("W37 & S2: clean model sizes without trailing .0, exact Whisper/Moonshine subtitles, and body font on headings + In use", async () => {
    const i18n = await makeI18n();
    const t = i18n.t.bind(i18n);

    expect(formatCleanModelSize(58)).toBe("58 MB");
    expect(formatCleanModelSize(478)).toBe("478 MB");
    expect(formatCleanModelSize(1638)).toBe("1.6 GB");
    expect(formatCleanModelSize(2560)).toBe("2.5 GB");

    const whisper = makeModel({
      id: "whisper-large-v3-turbo",
      name: "Whisper Large v3 Turbo",
      description:
        "99-language local speech recognition with translation support",
      engine_type: "Whisper",
      size_mb: 1638,
      is_downloaded: false,
    });
    const moonshine = makeModel({
      id: "moonshine-base",
      name: "Moonshine Base",
      description: "Fast English-only model",
      engine_type: "Moonshine",
      size_mb: 58,
      is_downloaded: false,
    });

    expect(formatSpeechModelSubtitle(whisper, t)).toBe("99 languages · 1.6 GB");
    expect(formatSpeechModelSubtitle(moonshine, t)).toBe(
      "English · smallest · 58 MB",
    );

    const html = withStores(
      {
        post_process_enabled: true,
        post_process_provider_id: "local_llm",
        post_process_models: { local_llm: "qwen3-4b" },
      },
      {
        currentModel: "parakeet-tdt-0.6b-v3",
        models: [
          makeModel({
            id: "parakeet-tdt-0.6b-v3",
            name: "Parakeet V3",
            size_mb: 478,
            is_downloaded: true,
          }),
          whisper,
          moonshine,
          makeModel({
            id: "qwen3-4b",
            name: "Qwen3-4B",
            engine_type: "LlamaCpp",
            size_mb: 2560,
            is_downloaded: true,
          }),
        ],
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <ModelsSettings />
          </I18nextProvider>,
        ),
    );

    // W37: exact rendered subtitles without catalog blurb or trailing ".0".
    expect(html).toContain("99 languages · 1.6 GB");
    expect(html).not.toContain("translation support");
    expect(html).toContain("English · smallest · 58 MB");
    expect(html).not.toContain("58.0 MB");

    // S2: section headings and "In use" badges use body font (no font-mono).
    expect(html).not.toMatch(
      /<p[^>]*font-mono[^>]*>Speech: used for dictation and meetings<\/p>/,
    );
    expect(html).not.toMatch(
      /<p[^>]*font-mono[^>]*>Clean-up and notes: runs on this Mac<\/p>/,
    );
    expect(html).not.toMatch(
      /data-testid="model-in-use-parakeet-tdt-0\.6b-v3"[^>]*font-mono/,
    );
    expect(html).not.toMatch(
      /data-testid="cleanup-model-in-use"[^>]*font-mono/,
    );
  });

  test("AC-D14 & D-D6: About What goes online summary adapts to update_checks_enabled, uses dynamic cloud count, and zero literal 0.20.0 in src/", async () => {
    const i18n = await makeI18n();

    const onHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <WhatGoesOnlineSummaryCard
          updateChecksEnabled={true}
          onOpenSettings={() => {}}
        />
      </I18nextProvider>,
    );
    expect(onHtml).toContain("Every launch");
    expect(onHtml).toContain("turn it off in Settings");
    expect(onHtml).toContain(
      `${ONLINE_CLOUD_PROVIDERS.length} providers, or your own server on localhost`,
    );

    const offHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <WhatGoesOnlineSummaryCard updateChecksEnabled={false} />
      </I18nextProvider>,
    );
    expect(offHtml).not.toContain("Every launch");
    expect(offHtml).toContain("Only when you click");
    expect(offHtml).toContain("github.com (update check is off in Settings)");

    // AboutSettings uses real WhatsNewModal and runManualUpdateCheck (not a hardcoded "up to date" string).
    const aboutSrc = readFileSync(
      join(ROOT, "src/components/settings/about/AboutSettings.tsx"),
      "utf8",
    );
    expect(aboutSrc).toContain("WhatsNewModal");
    expect(aboutSrc).toContain("runManualUpdateCheck");
    expect(aboutSrc).toContain("Checking…");
    expect(aboutSrc).toContain("Coco Voice is up to date");
    expect(aboutSrc).toContain("Update ready:");
    expect(aboutSrc).toContain("Couldn't check. Try again.");

    // Render AboutSettings cleanly without throwing and without literal 0.20.0.
    const aboutHtml = withStores(
      { update_checks_enabled: false },
      { models: [] },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <AboutSettings />
          </I18nextProvider>,
        ).replaceAll("&#x27;", "'"),
    );
    expect(aboutHtml).toContain("What's new");
    expect(aboutHtml).toContain("Check for updates");
    expect(aboutHtml).toContain("github.com (update check is off in Settings)");
    expect(aboutHtml).not.toContain("0.20.0");

    // Zero literal "0.20.0" anywhere in src/ (must come from getVersion()).
    const scanSrc = (dir: string): string[] => {
      const hits: string[] = [];
      for (const entry of readdirSync(dir)) {
        const full = join(dir, entry);
        const st = statSync(full);
        if (st.isDirectory()) {
          hits.push(...scanSrc(full));
        } else if (entry.endsWith(".ts") || entry.endsWith(".tsx")) {
          const text = readFileSync(full, "utf8");
          if (text.includes('"0.20.0"') || text.includes("'0.20.0'")) {
            hits.push(full.slice(ROOT.length + 1));
          }
        }
      }
      return hits;
    };
    expect(scanSrc(join(ROOT, "src"))).toEqual([]);
  });

  test("AC-D14a: About has no min-h-[ class and full WhatGoesOnline card follows summary card as the immediate next block", async () => {
    const i18n = await makeI18n();
    const aboutHtml = withStores(
      { update_checks_enabled: true },
      { models: [] },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <AboutSettings />
          </I18nextProvider>,
        ).replaceAll("&#x27;", "'"),
    );

    // 1. No element on About may carry a fixed min-h-[...] class.
    const minHMatches = aboutHtml.match(/\bmin-h-\[[^\s"']+/g) ?? [];
    expect(minHMatches).toEqual([]);

    // 2. Both cards and both headings remain rendered.
    expect(aboutHtml).toContain('data-testid="what-goes-online-summary"');
    expect(aboutHtml).toContain('data-testid="what-goes-online"');
    expect(aboutHtml).toContain("What goes online");
    expect(aboutHtml).toContain(
      "Your speech, meetings and notes stay on this Mac. These are the only places the app contacts:",
    );

    // 3. The full card's <section> root follows the summary card's </section> immediately
    // as the next block inside the same space-y-3 container, with nothing between.
    const summaryStart = aboutHtml.indexOf(
      'data-testid="what-goes-online-summary"',
    );
    const fullStart = aboutHtml.indexOf('data-testid="what-goes-online"');
    expect(summaryStart).toBeGreaterThan(-1);
    expect(fullStart).toBeGreaterThan(summaryStart);

    const summaryClose = aboutHtml.indexOf("</section>", summaryStart);
    expect(summaryClose).toBeGreaterThan(summaryStart);
    const fullOpen = aboutHtml.lastIndexOf("<section", fullStart);
    expect(fullOpen).toBeGreaterThanOrEqual(summaryClose);

    const between = aboutHtml
      .slice(summaryClose + "</section>".length, fullOpen)
      .trim();
    expect(between).toBe("");
  });
});
