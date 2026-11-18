import { readFileSync } from "node:fs";
import { expect, test } from "bun:test";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { renderToStaticMarkup } from "react-dom/server";
import en from "../src/i18n/locales/en/translation.json";
import ModelCard from "../src/components/onboarding/ModelCard";
import type { MeasuredReason, ModelInfo } from "../src/bindings";

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const reason: MeasuredReason = {
  clip_set: "wer-dev-40",
  words_wrong_per_100: 4.5,
  speed_x_realtime: 8,
  peak_memory_mb: 1500,
  models_compared: 3,
};

const model = (over: Partial<ModelInfo>): ModelInfo => ({
  id: "org/repo/file.gguf",
  name: "Test model",
  description: "A test model",
  filename: "file.gguf",
  source: "Local",
  size_mb: 100,
  is_downloaded: false,
  is_downloading: false,
  partial_size: 0,
  is_directory: false,
  engine_type: "Whisper",
  accuracy_score: 0.5,
  speed_score: 0.5,
  supports_translation: false,
  is_recommended: false,
  measured_best: null,
  supported_languages: ["en"],
  supports_language_selection: false,
  is_custom: false,
  supports_streaming: false,
  supports_language_detection: false,
  ...over,
});

const render = (m: ModelInfo, showRecommended = true) =>
  renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>
      <ModelCard
        model={m}
        onSelect={() => {}}
        showRecommended={showRecommended}
      />
    </I18nextProvider>,
  );

test("a model with a measured reason gets the tag", () => {
  const html = render(model({ measured_best: reason }));
  expect(html).toContain("Recommended");
  expect(html).toContain('data-testid="recommended-badge"');
});

test("an untested model never gets the tag, even if the catalog flag is set", () => {
  const html = render(model({ is_recommended: true, measured_best: null }));
  expect(html).not.toContain("Recommended");
  expect(html).not.toContain("recommended-badge");
  expect(html).not.toContain('role="tooltip"');
});

test("the tag stays hidden where the card asks for no tag", () => {
  const html = render(model({ measured_best: reason }), false);
  expect(html).not.toContain("recommended-badge");
});

test("the reason is reachable by keyboard and described to screen readers", () => {
  const html = render(model({ measured_best: reason }));
  const badge =
    html.match(/<span[^>]*data-testid="recommended-badge"[^>]*>/)?.[0] ?? "";
  expect(badge).toMatch(/tabindex="0"/);
  const describedBy = badge.match(/aria-describedby="([^"]+)"/)?.[1];
  expect(describedBy).toBeTruthy();
  const tooltip = html.match(/<span[^>]*role="tooltip"[^>]*>/)?.[0] ?? "";
  expect(tooltip).toContain(`id="${describedBy}"`);
  // Shown on hover and on keyboard focus.
  expect(tooltip).toContain("group-hover/recommended:opacity-100");
  expect(tooltip).toContain("group-focus/recommended:opacity-100");
});

test("the reason states the measured numbers", () => {
  const html = render(model({ measured_best: reason }));
  expect(html).toContain("4.5 per 100 words");
  expect(html).toContain("8× faster than real time");
  expect(html).toContain("1500 MB of memory");
  expect(html).toContain("of 3 models");
});

test("onboarding offers the tag on every model it lets you download", () => {
  const src = readFileSync(
    new URL("../src/components/onboarding/Onboarding.tsx", import.meta.url),
    "utf8",
  );
  const downloadSection = src.slice(
    src.indexOf("onboarding.downloadModelsTitle"),
  );
  expect(downloadSection).not.toContain("showRecommended={false}");
  // Models already on disk are a choice, not a suggestion.
  expect(src.slice(0, src.indexOf("onboarding.downloadModelsTitle"))).toContain(
    "showRecommended={false}",
  );
});
