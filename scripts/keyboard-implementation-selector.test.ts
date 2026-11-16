import { expect, mock, test } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import type { DropdownOption } from "../src/components/ui/Dropdown";

let currentImplementation: string | undefined;
let capturedOptions: DropdownOption[] = [];
let capturedSelected: string | null = null;
let capturedOnSelect: ((value: string) => void) | undefined;
const sentImplementations: string[] = [];

mock.module("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

mock.module("../src/hooks/useSettings", () => ({
  useSettings: () => ({
    getSetting: () => currentImplementation,
    isUpdating: () => false,
    refreshSettings: async () => {},
  }),
}));

mock.module("@/bindings", () => ({
  commands: {
    changeKeyboardImplementationSetting: async (value: string) => {
      sentImplementations.push(value);
      return { status: "ok", data: { success: true, reset_bindings: [] } };
    },
  },
}));

mock.module("../src/components/ui/Dropdown", () => ({
  Dropdown: (props: {
    options: DropdownOption[];
    selectedValue: string | null;
    onSelect: (value: string) => void;
  }) => {
    capturedOptions = props.options;
    capturedSelected = props.selectedValue;
    capturedOnSelect = props.onSelect;
    const active = props.options.find((o) => o.value === props.selectedValue);
    return active ? active.label : "unmatched";
  },
}));

const { KeyboardImplementationSelector } =
  await import("../src/components/settings/debug/KeyboardImplementationSelector");

test("KeyboardImplementationSelector offers and selects coco_voice_keys", async () => {
  currentImplementation = "coco_voice_keys";
  const html = renderToStaticMarkup(
    createElement(KeyboardImplementationSelector),
  );
  expect(capturedOptions).toEqual([
    { value: "tauri", label: "System global shortcut" },
    { value: "coco_voice_keys", label: "Coco Voice keys" },
  ]);
  expect(capturedSelected).toBe("coco_voice_keys");
  expect(html).toContain("Coco Voice keys");

  currentImplementation = "tauri";
  renderToStaticMarkup(createElement(KeyboardImplementationSelector));
  await capturedOnSelect?.("coco_voice_keys");
  expect(sentImplementations).toEqual(["coco_voice_keys"]);
});
