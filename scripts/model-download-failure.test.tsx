import { beforeEach, expect, test } from "bun:test";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { renderToStaticMarkup } from "react-dom/server";
import type { ReactNode } from "react";
import type { DownloadFailureCause, ModelInfo } from "../src/bindings";
import en from "../src/i18n/locales/en/translation.json";
import ModelCard from "../src/components/onboarding/ModelCard";
import Onboarding from "../src/components/onboarding/Onboarding";
import { useModelStore } from "../src/stores/modelStore";

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const model: ModelInfo = {
  id: "test/model.gguf",
  name: "Test Model",
  description: "A model for tests",
  filename: "model.gguf",
  source: {
    HuggingFace: { repo_id: "test/model", revision: "main", sha256: null },
  },
  size_mb: 100,
  is_downloaded: false,
  is_downloading: false,
  partial_size: 0,
  is_directory: false,
  engine_type: "TranscribeCpp",
  accuracy_score: 0.5,
  speed_score: 0.5,
  supports_translation: false,
  is_recommended: true,
  supported_languages: ["en"],
  supports_language_selection: false,
  is_custom: false,
  supports_streaming: false,
  supports_language_detection: false,
};

const failed = en.modelSelector.downloadFailed;
const retry = failed.retry;
const causeText: Record<DownloadFailureCause, string> = {
  network: failed.network,
  disk_space: failed.diskSpace,
  checksum: failed.checksum,
  other: failed.other,
};

const render = (node: ReactNode) =>
  renderToStaticMarkup(<I18nextProvider i18n={i18n}>{node}</I18nextProvider>);

const noop = () => {};

// Server rendering reads zustand's initial snapshot, not setState, so seed that.
const seed = (state: Partial<ReturnType<typeof useModelStore.getState>>) =>
  Object.assign(useModelStore.getInitialState(), state);

const failWith = (cause: DownloadFailureCause) =>
  seed({ models: [model], downloadFailures: { [model.id]: cause } });

beforeEach(() => seed({ models: [model], downloadFailures: {} }));

test("each failure cause shows its own text with Try again", () => {
  for (const [cause, text] of Object.entries(causeText)) {
    failWith(cause as DownloadFailureCause);
    const html = render(
      <ModelCard model={model} onSelect={noop} onDownload={noop} />,
    );
    expect(html).toContain(text);
    expect(html).toContain(retry);
  }
});

test("the settings card offers Try again but not Skip", () => {
  failWith("network");
  const html = render(
    <ModelCard model={model} onSelect={noop} onDownload={noop} />,
  );
  expect(html).toContain(retry);
  expect(html).not.toContain(failed.skip);
});

test("onboarding offers Skip next to Try again", () => {
  failWith("disk_space");
  const html = render(<Onboarding onModelSelected={noop} />);
  expect(html).toContain(failed.diskSpace);
  expect(html).toContain(retry);
  expect(html).toContain(failed.skip);
});

test("no failure, no failure message", () => {
  const html = render(<Onboarding onModelSelected={noop} />);
  expect(html).not.toContain(retry);
  expect(html).not.toContain(failed.skip);
});
