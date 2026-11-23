import { beforeEach, describe, expect, mock, test } from "bun:test";
import type { ModelInfo } from "../src/bindings";
import { FIRST_RUN_DEFAULT_MODEL_ID } from "../src/components/onboarding/FirstRunModelDownloadView";

const listeners = new Map<string, (event: { payload: unknown }) => void>();
const deleteModelCalls: string[] = [];
const pendingEffects: Array<() => void | (() => void)> = [];

// Capture real React module before overriding `useEffect` so Onboarding's real
// `useEffect` registrations can be flushed deterministically under renderToStaticMarkup.
// eslint-disable-next-line @typescript-eslint/no-require-imports
const realReact = require("react") as typeof import("react");

mock.module("react", () => ({
  ...realReact,
  default: {
    ...realReact,
    useEffect: (effect: () => void | (() => void)) => {
      pendingEffects.push(effect);
    },
  },
  useEffect: (effect: () => void | (() => void)) => {
    pendingEffects.push(effect);
  },
}));

mock.module("@tauri-apps/api/event", () => ({
  listen: async (
    eventName: string,
    handler: (event: { payload: unknown }) => void,
  ) => {
    listeners.set(eventName, handler);
    return () => {
      listeners.delete(eventName);
    };
  },
}));

mock.module("@/bindings", () => ({
  commands: {
    deleteModel: async (modelId: string) => {
      deleteModelCalls.push(modelId);
      return { status: "ok", data: null };
    },
    getAvailableModels: async () => ({ status: "ok", data: [] }),
    getCurrentModel: async () => ({ status: "ok", data: "" }),
    hasAnyModelsAvailable: async () => ({ status: "ok", data: false }),
    downloadModel: async () => ({ status: "ok", data: null }),
    cancelDownload: async () => ({ status: "ok", data: null }),
    setActiveModel: async () => ({ status: "ok", data: null }),
    isRecording: async () => false,
  },
}));

mock.module("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

mock.module("sonner", () => ({
  toast: {
    error: () => {},
    success: () => {},
    info: () => {},
  },
}));

const { renderToStaticMarkup } = await import("react-dom/server");
const { useModelStore } = await import("../src/stores/modelStore");
const { default: Onboarding, cleanupPartialOnDownloadFailure } =
  await import("../src/components/onboarding/Onboarding");

const defaultModel: ModelInfo = {
  id: FIRST_RUN_DEFAULT_MODEL_ID,
  name: "Parakeet V3",
  description: "Fast multilingual speech model",
  filename: "parakeet-tdt-0.6b-v3-int8",
  source: {
    Url: {
      url: "https://blob.handy.computer/parakeet-v3-int8.tar.gz",
      sha256: null,
    },
  },
  size_mb: 478,
  is_downloaded: false,
  is_downloading: false,
  partial_size: 4096,
  is_directory: true,
  engine_type: "Parakeet",
  accuracy_score: 0.9,
  speed_score: 0.9,
  supports_translation: false,
  is_recommended: true,
  measured_best: null,
  supported_languages: ["en"],
  supports_language_selection: false,
  is_custom: false,
  supports_streaming: false,
  supports_language_detection: false,
};

describe("AC-A7 frontend partial cleanup on model-download-failed", () => {
  beforeEach(() => {
    listeners.clear();
    deleteModelCalls.length = 0;
    pendingEffects.length = 0;
    useModelStore.setState({
      models: [defaultModel],
      currentModel: "",
      downloadingModels: { [FIRST_RUN_DEFAULT_MODEL_ID]: true },
      verifyingModels: {},
      extractingModels: {},
      downloadProgress: {},
      downloadStats: {},
      downloadFailures: {},
      suppressFailureToast: true,
      loading: false,
      error: null,
      hasAnyModels: false,
      isFirstRun: true,
      initialized: false,
    });
  });

  test("Onboarding cleanupPartialOnDownloadFailure calls commands.deleteModel(id) when model-download-failed arrives", async () => {
    await useModelStore.getState().initialize();
    useModelStore.setState({ models: [defaultModel] });

    const failedListener = listeners.get("model-download-failed");
    expect(failedListener).toBeDefined();

    // Simulate backend emitting `model-download-failed` after a mid-download network drop
    failedListener!({
      payload: {
        model_id: FIRST_RUN_DEFAULT_MODEL_ID,
        error: "network stream disconnected mid-file",
        cause: "network",
      },
    });

    expect(
      useModelStore.getState().downloadFailures[FIRST_RUN_DEFAULT_MODEL_ID],
    ).toBe("network");

    // Sync Zustand's SSR server snapshot (`getInitialState()`) with the post-event
    // live store state so `renderToStaticMarkup` renders Onboarding with the failed state.
    Object.assign(useModelStore.getInitialState(), useModelStore.getState());

    // Render real Onboarding and flush its registered effects so its AC-A7 cleanup effect runs
    renderToStaticMarkup(
      realReact.createElement(Onboarding, { onModelSelected: () => {} }),
    );
    expect(pendingEffects.length).toBeGreaterThan(0);
    for (const effect of pendingEffects) {
      effect();
    }
    await Promise.resolve();
    await Promise.resolve();

    expect(deleteModelCalls).toEqual([FIRST_RUN_DEFAULT_MODEL_ID]);

    // Also verify default cleanup helper swallows missing-file rejections cleanly
    await cleanupPartialOnDownloadFailure("missing-partial-id", async () => {
      throw new Error("No model files found to delete");
    });

    console.log(
      `AC_A7_FRONTEND_PROOF: event=model-download-failed deleted_ids=${deleteModelCalls.join(",")}`,
    );
  });
});
