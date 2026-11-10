import { expect, mock, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

// ShortcutInput only chooses a recorder, so stand in for the setting and both
// recorders (the real ones need the Tauri runtime).
let keyboardImplementation: string | undefined;
mock.module("../src/hooks/useSettings", () => ({
  useSettings: () => ({ getSetting: () => keyboardImplementation }),
}));
mock.module("../src/components/settings/CocoVoiceKeysShortcutInput", () => ({
  CocoVoiceKeysShortcutInput: () => "backend-recorder",
}));
mock.module("../src/components/settings/GlobalShortcutInput", () => ({
  GlobalShortcutInput: () => "js-recorder",
}));

const { ShortcutInput } =
  await import("../src/components/settings/ShortcutInput");

function render(value: string | undefined): string {
  keyboardImplementation = value;
  return renderToStaticMarkup(
    createElement(ShortcutInput, { shortcutId: "transcribe" }),
  );
}

test("coco_voice_keys uses the backend key recorder", () => {
  expect(render("coco_voice_keys")).toBe("backend-recorder");
});

test("tauri uses the JS key recorder", () => {
  expect(render("tauri")).toBe("js-recorder");
});

test("no setting yet falls back to the JS key recorder", () => {
  expect(render(undefined)).toBe("js-recorder");
});
