import { beforeEach, describe, expect, mock, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

const changeAgentsCanAskCalls: boolean[] = [];
const warnMessages: string[] = [];
let lastToggleOnChange: ((next: boolean) => void) | null = null;
let lastToggleChecked: boolean | null = null;

let backendSettings = {
  agents_can_ask_enabled: false,
  agent_ask_enabled: true,
};

mock.module("@/bindings", () => ({
  commands: {
    changeAgentsCanAskSetting: async (enabled: boolean) => {
      changeAgentsCanAskCalls.push(enabled);
      backendSettings = {
        ...backendSettings,
        agents_can_ask_enabled: enabled,
        agent_ask_enabled: enabled ? backendSettings.agent_ask_enabled : false,
      };
      return { status: "ok", data: null };
    },
    getAppSettings: async () => ({
      status: "ok",
      data: { ...backendSettings },
    }),
    getDefaultSettings: async () => ({
      status: "ok",
      data: { agents_can_ask_enabled: false, agent_ask_enabled: false },
    }),
    getAvailableMicrophones: async () => ({ status: "ok", data: [] }),
    getAvailableOutputDevices: async () => ({ status: "ok", data: [] }),
    checkCustomSounds: async () => ({ start: false, stop: false }),
  },
}));

mock.module("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

mock.module("../src/components/ui/ToggleSwitch", () => ({
  ToggleSwitch: (props: {
    checked: boolean;
    onChange: (next: boolean) => void;
    label: string;
  }) => {
    lastToggleChecked = props.checked;
    lastToggleOnChange = props.onChange;
    return React.createElement("button", {
      type: "button",
      role: "switch",
      "aria-checked": props.checked,
      "aria-label": props.label,
    });
  },
}));

const { useSettingsStore } = await import("../src/stores/settingsStore");
const { AgentsCanAskCard } =
  await import("../src/components/settings/AgentsCanAskCard");

describe("D5 · Agents can ask switch saves via backend command and survives reload", () => {
  beforeEach(() => {
    changeAgentsCanAskCalls.length = 0;
    warnMessages.length = 0;
    lastToggleOnChange = null;
    lastToggleChecked = null;
    backendSettings = {
      agents_can_ask_enabled: false,
      agent_ask_enabled: true,
    };
    const initial = {
      agents_can_ask_enabled: false,
      agent_ask_enabled: false,
    } as any;
    useSettingsStore.setState({
      settings: initial,
      defaultSettings: initial,
      isLoading: false,
      isUpdating: {},
    });
    Object.assign(useSettingsStore.getInitialState(), {
      settings: initial,
      defaultSettings: initial,
      isLoading: false,
      isUpdating: {},
    });
  });

  test("toggling AgentsCanAskCard calls changeAgentsCanAskSetting with no 'No handler' warning and reloads from backend", async () => {
    const origWarn = console.warn;
    console.warn = (...args: unknown[]) => {
      warnMessages.push(args.map(String).join(" "));
    };

    try {
      // Initial render: card reads off state from store and binds real ToggleSwitch onChange.
      const htmlInitial = renderToStaticMarkup(
        React.createElement(AgentsCanAskCard),
      );
      expect(htmlInitial).toContain('data-testid="agents-can-ask-state-off"');
      expect(lastToggleChecked).toBe(false);
      expect(lastToggleOnChange).not.toBeNull();

      // Toggle ON via the real AgentsCanAskCard callback.
      lastToggleOnChange!(true);
      await Promise.resolve();
      await Promise.resolve();

      expect(changeAgentsCanAskCalls).toEqual([true]);
      expect(
        warnMessages.some((m) =>
          m.includes("No handler for setting: agents_can_ask_enabled"),
        ),
      ).toBe(false);

      // Clear local store and reload from backend (`getAppSettings`).
      useSettingsStore.setState({ settings: null });
      await useSettingsStore.getState().refreshSettings();
      expect(useSettingsStore.getState().settings?.agents_can_ask_enabled).toBe(
        true,
      );

      Object.assign(
        useSettingsStore.getInitialState(),
        useSettingsStore.getState(),
      );
      const htmlOn = renderToStaticMarkup(
        React.createElement(AgentsCanAskCard),
      );
      expect(htmlOn).toContain('data-testid="agents-can-ask-state-on"');
      expect(lastToggleChecked).toBe(true);

      // Toggle OFF — must persist false and clear legacy agent_ask_enabled.
      lastToggleOnChange!(false);
      await Promise.resolve();
      await Promise.resolve();

      expect(changeAgentsCanAskCalls).toEqual([true, false]);
      expect(
        warnMessages.some((m) =>
          m.includes("No handler for setting: agents_can_ask_enabled"),
        ),
      ).toBe(false);

      useSettingsStore.setState({ settings: null });
      await useSettingsStore.getState().refreshSettings();
      expect(useSettingsStore.getState().settings?.agents_can_ask_enabled).toBe(
        false,
      );
      expect(useSettingsStore.getState().settings?.agent_ask_enabled).toBe(
        false,
      );

      Object.assign(
        useSettingsStore.getInitialState(),
        useSettingsStore.getState(),
      );
      const htmlOff = renderToStaticMarkup(
        React.createElement(AgentsCanAskCard),
      );
      expect(htmlOff).toContain('data-testid="agents-can-ask-state-off"');
      expect(lastToggleChecked).toBe(false);
    } finally {
      console.warn = origWarn;
    }
  });
});
