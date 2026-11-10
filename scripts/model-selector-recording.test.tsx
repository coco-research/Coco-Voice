import { beforeEach, expect, test } from "bun:test";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { renderToStaticMarkup } from "react-dom/server";
import en from "../src/i18n/locales/en/translation.json";
import ModelSelector from "../src/components/model-selector/ModelSelector";
import { useModelStore } from "../src/stores/modelStore";

const i18n = i18next.createInstance();
await i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const render = () =>
  renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>
      <ModelSelector />
    </I18nextProvider>,
  );

// Server rendering reads zustand's initial snapshot, not setState, so seed that.
const seed = (isRecording: boolean) =>
  Object.assign(useModelStore.getInitialState(), { isRecording });

// The model picker's trigger is the first (and, while closed, only) button.
const pickerButton = (html: string) => html.match(/<button[^>]*>/)?.[0] ?? "";

beforeEach(() => seed(false));

test("the model picker is disabled while a recording is in progress", () => {
  seed(true);
  expect(pickerButton(render())).toMatch(/\sdisabled=""/);
});

test("the model picker is enabled again once the recording ends", () => {
  seed(true);
  render();
  seed(false);
  const button = pickerButton(render());
  expect(button).not.toBe("");
  expect(button).not.toMatch(/\sdisabled=""/);
});
