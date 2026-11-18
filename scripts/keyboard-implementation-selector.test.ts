import { expect, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import {
  applyKeyboardImplementationSelection,
  KEYBOARD_IMPLEMENTATION_OPTIONS,
  KeyboardImplementationSelector,
} from "../src/components/settings/debug/KeyboardImplementationSelector";
import { useSettingsStore } from "../src/stores/settingsStore";

const i18n = i18next.createInstance();
void i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

test("KeyboardImplementationSelector offers and selects coco_voice_keys without global module mocks", async () => {
  const sentImplementations: string[] = [];
  const prevSettings = useSettingsStore.getState().settings;
  const g = globalThis as { window?: unknown };
  const prevWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  g.window = {
    __TAURI_INTERNALS__: {
      invoke: (cmd: string, args?: { implementation?: string }) => {
        if (cmd === "change_keyboard_implementation_setting") {
          sentImplementations.push(String(args?.implementation ?? ""));
          return Promise.resolve({ success: true, reset_bindings: [] });
        }
        return Promise.resolve(null);
      },
    },
  };

  try {
    expect(KEYBOARD_IMPLEMENTATION_OPTIONS).toEqual([
      { value: "tauri", label: "System global shortcut" },
      { value: "coco_voice_keys", label: "Coco Voice keys" },
    ]);

    useSettingsStore.setState({
      settings: { keyboard_implementation: "coco_voice_keys" },
    });
    const html = renderToStaticMarkup(
      createElement(
        I18nextProvider,
        { i18n },
        createElement(KeyboardImplementationSelector),
      ),
    );
    expect(html).toContain("Keyboard handler");
    expect(html).toContain("Coco Voice keys");

    await applyKeyboardImplementationSelection(
      "coco_voice_keys",
      "tauri",
      async () => {},
      (k) => i18n.t(k),
    );
    expect(sentImplementations).toEqual(["coco_voice_keys"]);
  } finally {
    useSettingsStore.setState({ settings: prevSettings });
    if (prevWindow) Object.defineProperty(globalThis, "window", prevWindow);
    else delete g.window;
  }
});
