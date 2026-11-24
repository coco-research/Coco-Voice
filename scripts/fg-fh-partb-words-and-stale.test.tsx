import { beforeEach, describe, expect, mock, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { ShortcutRegistrationFailure } from "../src/lib/types/events";
import type { ShellSectionId } from "../src/components/layout/shellRoutes";

type EventHandler = (event: { payload: unknown }) => void;

const eventListeners = new Map<string, Set<EventHandler>>();
let requestAxCallCount = 0;
let currentHealthPayload: Record<string, unknown> = {
  staleAccessibility: false,
  shortcutUnset: true,
  shortcutFailureCause: "permission",
  typingOff: false,
  cleanupEnabledWithoutLocalModel: false,
  cleanupNoticeAlreadyShown: false,
  updateReadyVersion: null,
  rolledBackFromVersion: null,
};

let translationMode: "en" | "fallback" = "en";
const requestedKeys: string[] = [];

// eslint-disable-next-line @typescript-eslint/no-require-imports
const realTauriEvent = require("@tauri-apps/api/event") as Record<
  string,
  unknown
>;

mock.module("@tauri-apps/api/event", () => ({
  ...realTauriEvent,
  listen: async (name: string, handler: EventHandler) => {
    let set = eventListeners.get(name);
    if (!set) {
      set = new Set();
      eventListeners.set(name, set);
    }
    set.add(handler);
    return () => {
      eventListeners.get(name)?.delete(handler);
    };
  },
  emit: async () => {},
}));

// eslint-disable-next-line @typescript-eslint/no-require-imports
const realTauriCore = require("@tauri-apps/api/core") as Record<
  string,
  unknown
>;

mock.module("@tauri-apps/api/core", () => ({
  ...realTauriCore,
  invoke: async (cmd: string) => {
    if (cmd === "home_health_status" || cmd === "get_home_health") {
      return currentHealthPayload;
    }
    if (cmd === "list_meetings") {
      return [];
    }
    return null;
  },
}));

mock.module("@tauri-apps/api/app", () => ({
  getVersion: async () => "0.20.2",
}));

mock.module("@tauri-apps/plugin-os", () => ({
  platform: () => "macos",
}));

mock.module("@tauri-apps/plugin-process", () => ({
  relaunch: async () => {},
}));

mock.module("tauri-plugin-macos-permissions-api", () => ({
  checkAccessibilityPermission: async () => false,
  requestAccessibilityPermission: async () => {
    requestAxCallCount += 1;
  },
  checkMicrophonePermission: async () => true,
  requestMicrophonePermission: async () => {},
}));

mock.module("@/bindings", () => ({
  commands: {
    getHistoryEntries: async () => ({
      status: "ok",
      data: { entries: [], has_more: false },
    }),
    initializeShortcuts: async () => ({ status: "ok", data: null }),
    initializeEnigo: async () => ({ status: "ok", data: null }),
    showMainWindowCommand: async () => {},
    changePasteMethodSetting: async () => ({ status: "ok", data: null }),
    openMicrophonePrivacySettings: async () => {},
    getWindowsMicrophonePermissionStatus: async () => "granted",
  },
}));

const testI18n = i18next.createInstance();
await testI18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

function translate(
  key: string,
  defaultOrOpts?: string | Record<string, unknown>,
): string {
  requestedKeys.push(key);
  if (translationMode === "fallback") {
    if (typeof defaultOrOpts === "string") return defaultOrOpts;
    if (
      defaultOrOpts &&
      typeof defaultOrOpts === "object" &&
      typeof defaultOrOpts.defaultValue === "string"
    ) {
      let out = defaultOrOpts.defaultValue;
      for (const [k, v] of Object.entries(defaultOrOpts)) {
        if (k !== "defaultValue") {
          out = out.replaceAll(`{{${k}}}`, String(v));
        }
      }
      return out;
    }
    return key;
  }
  if (typeof defaultOrOpts === "string") {
    return String(testI18n.t(key, { defaultValue: defaultOrOpts }));
  }
  return String(testI18n.t(key, defaultOrOpts));
}

mock.module("react-i18next", () => ({
  useTranslation: () => ({
    t: translate,
    i18n: testI18n,
  }),
  initReactI18next,
}));

const { shortcutFailureBodyKey, startShortcutRegistrationListener } =
  await import("../src/hooks/useShortcutRegistration");
const { HomeHealthNoticesSection, resolveTopHomeNotice } =
  await import("../src/components/home/HomeHealthNotice");
const { HomeView, openSystemSettingsAction } =
  await import("../src/components/home/HomeView");
const { FirstRunStaleAxCard, FirstRunWelcomeCard } =
  await import("../src/components/onboarding/AccessibilityOnboarding");
const { useSettingsStore } = await import("../src/stores/settingsStore");

type Elem = React.ReactElement<Record<string, unknown>>;

function depsChanged(
  prev: readonly unknown[] | undefined,
  next: readonly unknown[] | undefined,
): boolean {
  if (!prev || !next) return true;
  if (prev.length !== next.length) return true;
  for (let i = 0; i < prev.length; i += 1) {
    if (!Object.is(prev[i], next[i])) return true;
  }
  return false;
}

function mountComponent(rootFactory: () => React.ReactNode) {
  const internals = (
    React as unknown as {
      __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: {
        ReactCurrentDispatcher: { current: unknown };
      };
    }
  ).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;

  interface Inst {
    type: unknown;
    states: unknown[];
    refs: Array<{ current: unknown }>;
    callbacks: Array<{ fn: unknown; deps?: readonly unknown[] }>;
    memos: Array<{ value: unknown; deps?: readonly unknown[] }>;
    effects: Array<{
      deps?: readonly unknown[];
      cleanup?: void | (() => void);
    }>;
  }

  const instances = new Map<string, Inst>();
  let renderedTree: React.ReactNode = null;

  const rerender = () => {
    const origDispatcher = internals.ReactCurrentDispatcher.current;
    const setDisp = (d: unknown) => {
      internals.ReactCurrentDispatcher.current = d;
    };
    const alive = new Set<string>();
    const pendingEffects: Array<{
      inst: Inst;
      idx: number;
      fn: () => void | (() => void);
      deps?: readonly unknown[];
    }> = [];

    const walk = (node: React.ReactNode, path: string): React.ReactNode => {
      if (node == null || typeof node !== "object") return node;
      if (Array.isArray(node)) {
        return node.map((child, i) => walk(child, `${path}[${i}]`));
      }
      const el = node as Elem;
      if (el.type === React.Fragment) {
        return walk(el.props.children as React.ReactNode, `${path}.F`);
      }
      if (typeof el.type === "function") {
        const keyPart = el.key != null ? `#${String(el.key)}` : "";
        const fnName = (el.type as { name?: string }).name || "Fn";
        const instKey = `${path}/${fnName}${keyPart}`;
        alive.add(instKey);
        let inst = instances.get(instKey);
        if (!inst || inst.type !== el.type) {
          if (inst) {
            for (const eff of inst.effects) {
              if (eff?.cleanup) eff.cleanup();
            }
          }
          inst = {
            type: el.type,
            states: [],
            refs: [],
            callbacks: [],
            memos: [],
            effects: [],
          };
          instances.set(instKey, inst);
        }

        let stateCursor = 0;
        let refCursor = 0;
        let cbCursor = 0;
        let memoCursor = 0;
        let effCursor = 0;

        setDisp({
          useState<T>(
            init: T | (() => T),
          ): [T, (next: T | ((p: T) => T)) => void] {
            const idx = stateCursor++;
            if (idx >= inst!.states.length) {
              inst!.states[idx] =
                typeof init === "function" ? (init as () => T)() : init;
            }
            const target = inst!;
            const set = (next: T | ((p: T) => T)) => {
              target.states[idx] =
                typeof next === "function"
                  ? (next as (p: T) => T)(target.states[idx] as T)
                  : next;
            };
            return [inst!.states[idx] as T, set];
          },
          useEffect(fn: () => void | (() => void), deps?: readonly unknown[]) {
            const idx = effCursor++;
            const prev = inst!.effects[idx];
            if (!prev || depsChanged(prev.deps, deps)) {
              pendingEffects.push({ inst: inst!, idx, fn, deps });
            }
          },
          useLayoutEffect() {},
          useRef<T>(init: T) {
            const idx = refCursor++;
            if (idx >= inst!.refs.length) inst!.refs[idx] = { current: init };
            return inst!.refs[idx];
          },
          useCallback<T>(fn: T, deps?: readonly unknown[]) {
            const idx = cbCursor++;
            const prev = inst!.callbacks[idx];
            if (!prev || depsChanged(prev.deps, deps)) {
              inst!.callbacks[idx] = { fn, deps };
            }
            return inst!.callbacks[idx].fn as T;
          },
          useMemo<T>(fn: () => T, deps?: readonly unknown[]) {
            const idx = memoCursor++;
            const prev = inst!.memos[idx];
            if (!prev || depsChanged(prev.deps, deps)) {
              inst!.memos[idx] = { value: fn(), deps };
            }
            return inst!.memos[idx].value as T;
          },
          useContext(ctx: { _currentValue?: unknown }) {
            return ctx._currentValue;
          },
          useSyncExternalStore<T>(_sub: unknown, getSnap: () => T): T {
            return getSnap();
          },
          useId: () => `id-${instKey}`,
          useTransition: () => [false, (fn: () => void) => fn()],
          useDebugValue() {},
          useDeferredValue<T>(v: T) {
            return v;
          },
          useInsertionEffect() {},
          useImperativeHandle() {},
        });

        let out: React.ReactNode;
        try {
          out = (el.type as (p: Record<string, unknown>) => React.ReactNode)(
            el.props,
          );
        } finally {
          setDisp(origDispatcher);
        }
        return walk(out, instKey);
      }

      if (el.props && "children" in el.props) {
        return React.cloneElement(
          el,
          undefined,
          walk(
            el.props.children as React.ReactNode,
            `${path}.${String(el.type)}`,
          ),
        );
      }
      return el;
    };

    renderedTree = walk(rootFactory(), "root");
    for (const [k, inst] of [...instances.entries()]) {
      if (!alive.has(k)) {
        for (const eff of inst.effects) {
          if (eff?.cleanup) eff.cleanup();
        }
        instances.delete(k);
      }
    }
    for (const eff of pendingEffects) {
      const prev = eff.inst.effects[eff.idx];
      if (prev?.cleanup) prev.cleanup();
      const cleanup = eff.fn();
      eff.inst.effects[eff.idx] = { deps: eff.deps, cleanup };
    }
    return renderedTree;
  };

  const unmount = () => {
    for (const inst of instances.values()) {
      for (const eff of inst.effects) {
        if (eff?.cleanup) eff.cleanup();
      }
    }
    instances.clear();
  };

  const flushAndRerender = async (rounds = 3) => {
    for (let i = 0; i < rounds; i += 1) {
      await Promise.resolve();
      await Promise.resolve();
      rerender();
    }
    return renderedTree;
  };

  rerender();
  return { getTree: () => renderedTree, rerender, flushAndRerender, unmount };
}

function textContent(node: React.ReactNode): string {
  if (node == null || typeof node === "boolean") return "";
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textContent).join("");
  if (typeof node === "object" && "props" in node) {
    const props = (node as Elem).props;
    return props && "children" in props
      ? textContent(props.children as React.ReactNode)
      : "";
  }
  return "";
}

function findButtonByText(node: React.ReactNode, label: string): Elem | null {
  if (node == null || typeof node !== "object") return null;
  if (Array.isArray(node)) {
    for (const child of node) {
      const hit = findButtonByText(child, label);
      if (hit) return hit;
    }
    return null;
  }
  const el = node as Elem;
  if (el.type === "button" && textContent(el).trim() === label) {
    return el;
  }
  if (el.props && "children" in el.props) {
    return findButtonByText(el.props.children as React.ReactNode, label);
  }
  return null;
}

describe("Voice F-g / F-h Part B words, Notice b causes, and stale-fix (T-g5, T-h4, T-b1, T-b2)", () => {
  beforeEach(() => {
    eventListeners.clear();
    requestAxCallCount = 0;
    translationMode = "en";
    requestedKeys.length = 0;
    currentHealthPayload = {
      staleAccessibility: false,
      shortcutUnset: true,
      shortcutFailureCause: "permission",
      typingOff: false,
      cleanupEnabledWithoutLocalModel: false,
      cleanupNoticeAlreadyShown: false,
      updateReadyVersion: null,
      rolledBackFromVersion: null,
    };
  });

  test("T-g5: permission cause maps to and displays errors.shortcutRegistrationFailedPermission", async () => {
    expect(shortcutFailureBodyKey("permission")).toBe(
      "errors.shortcutRegistrationFailedPermission",
    );

    const listeners = new Map<string, (event: { payload: unknown }) => void>();
    const shownDescriptions: string[] = [];

    const stop = startShortcutRegistrationListener({
      t: (key, opts) => translate(key, opts),
      onOpenSettings: () => {},
      listen: async (event, handler) => {
        listeners.set(event, handler as (e: { payload: unknown }) => void);
        return () => {
          listeners.delete(event);
        };
      },
      onFocusChanged: async () => () => {},
      getPlatform: () => "macos",
      checkAccessibilityPermission: async () => false,
      initializeShortcuts: async () => {},
      showMainWindowCommand: async () => {},
      showToast: (_title, opts) => {
        shownDescriptions.push(opts.description);
      },
      dismissToast: () => {},
    });

    await Promise.resolve();
    const payload: ShortcutRegistrationFailure[] = [
      {
        id: "transcribe",
        binding: "fn",
        error: "Accessibility permission not granted",
        cause: "permission",
      },
    ];
    listeners.get("shortcut-registration-failed")?.({ payload });
    expect(shownDescriptions).toEqual([
      "Turn on Coco Voice in System Settings, then come back. Your shortcut starts by itself.",
    ]);
    stop();
  });

  test("T-h4: Notice b renders the signed body and action for every shortcut failure cause", () => {
    // 1. taken -> today's words + Choose a shortcut -> navigates to settings
    const navCalls: ShellSectionId[] = [];
    let openSettingsCalls = 0;
    const takenMount = mountComponent(() =>
      React.createElement(HomeHealthNoticesSection, {
        health: { shortcutUnset: true, shortcutFailureCause: "taken" },
        shortcut: "fn",
        onNavigate: (s) => navCalls.push(s),
        onOpenSystemSettings: () => {
          openSettingsCalls += 1;
        },
      }),
    );
    const takenText = textContent(takenMount.getTree());
    expect(takenText).toContain("Your shortcut is not set up");
    expect(takenText).toContain("Another app uses fn.");
    const takenBtn = findButtonByText(
      takenMount.getTree(),
      "Choose a shortcut",
    );
    expect(takenBtn).not.toBeNull();
    (takenBtn!.props.onClick as () => void)();
    expect(navCalls).toEqual(["settings"]);
    expect(openSettingsCalls).toBe(0);
    takenMount.unmount();

    // 2. permission -> S3 body + Open System Settings -> calls S4 handler, not navigate
    navCalls.length = 0;
    const permMount = mountComponent(() =>
      React.createElement(HomeHealthNoticesSection, {
        health: { shortcutUnset: true, shortcutFailureCause: "permission" },
        shortcut: "fn",
        onNavigate: (s) => navCalls.push(s),
        onOpenSystemSettings: () => {
          openSettingsCalls += 1;
        },
      }),
    );
    const permText = textContent(permMount.getTree());
    expect(permText).toContain("Your shortcut is not set up");
    expect(permText).toContain(
      "Turn on Coco Voice in System Settings, then come back. Your shortcut starts by itself.",
    );
    const permBtn = findButtonByText(
      permMount.getTree(),
      "Open System Settings",
    );
    expect(permBtn).not.toBeNull();
    (permBtn!.props.onClick as () => void)();
    expect(openSettingsCalls).toBe(1);
    expect(navCalls).toEqual([]);
    permMount.unmount();

    // 3. rejected -> S5 body + Choose a shortcut
    navCalls.length = 0;
    const rejectedMount = mountComponent(() =>
      React.createElement(HomeHealthNoticesSection, {
        health: { shortcutUnset: true, shortcutFailureCause: "rejected" },
        shortcut: "fn",
        onNavigate: (s) => navCalls.push(s),
        onOpenSystemSettings: () => {
          openSettingsCalls += 1;
        },
      }),
    );
    const rejectedText = textContent(rejectedMount.getTree());
    expect(rejectedText).toContain("fn couldn't be set up.");
    const rejectedBtn = findButtonByText(
      rejectedMount.getTree(),
      "Choose a shortcut",
    );
    expect(rejectedBtn).not.toBeNull();
    (rejectedBtn!.props.onClick as () => void)();
    expect(navCalls).toEqual(["settings"]);
    rejectedMount.unmount();

    // 4. other -> S5 body + Choose a shortcut
    navCalls.length = 0;
    const otherMount = mountComponent(() =>
      React.createElement(HomeHealthNoticesSection, {
        health: { shortcutUnset: true, shortcutFailureCause: "other" },
        shortcut: "Option+Space",
        onNavigate: (s) => navCalls.push(s),
        onOpenSystemSettings: () => {
          openSettingsCalls += 1;
        },
      }),
    );
    const otherText = textContent(otherMount.getTree());
    expect(otherText).toContain("Option+Space couldn't be set up.");
    const otherBtn = findButtonByText(
      otherMount.getTree(),
      "Choose a shortcut",
    );
    expect(otherBtn).not.toBeNull();
    (otherBtn!.props.onClick as () => void)();
    expect(navCalls).toEqual(["settings"]);
    otherMount.unmount();

    // 5. null cause (empty binding) -> today's words + Choose a shortcut
    const emptyHtml = renderToStaticMarkup(
      React.createElement(HomeHealthNoticesSection, {
        health: { shortcutUnset: true, shortcutFailureCause: null },
        shortcut: "fn",
        onNavigate: () => {},
      }),
    ).replaceAll("&#x27;", "'");
    expect(emptyHtml).toContain("Another app uses fn.");
    expect(emptyHtml).toContain("Choose a shortcut");

    // resolveTopHomeNotice aligns with cause
    expect(
      resolveTopHomeNotice({
        shortcutUnset: true,
        shortcutFailureCause: "permission",
      }),
    ).toEqual({
      kind: "shortcut_unset",
      messageKey: "homeNotices.v022ShortcutDescPermission",
      actionKey: "accessibility.openSettings",
    });
    expect(
      resolveTopHomeNotice({
        shortcutUnset: true,
        shortcutFailureCause: "other",
      })?.actionKey,
    ).toBe("homeNotices.chooseShortcut");
  });

  test("T-b1: S4 Open System Settings action calls requestAx once per click and 0 times on load or focus", async () => {
    const prevSettings = useSettingsStore.getState().settings;
    useSettingsStore.setState({
      isLoading: false,
      settings: {
        ...(prevSettings ?? {}),
        paste_method: "ctrl_v",
        bindings: {
          transcribe: {
            id: "transcribe",
            name: "Transcribe",
            description: "",
            default_binding: "fn",
            current_binding: "fn",
          },
        },
      } as never,
    });

    let focusHandler: ((event: { payload: boolean }) => void) | null = null;
    const stopListener = startShortcutRegistrationListener({
      t: (key, opts) => translate(key, opts),
      onOpenSettings: () => {},
      listen: async () => () => {},
      onFocusChanged: async (handler) => {
        focusHandler = handler;
        return () => {
          focusHandler = null;
        };
      },
      getPlatform: () => "macos",
      checkAccessibilityPermission: async () => false,
      initializeShortcuts: async () => {},
      showMainWindowCommand: async () => {},
      showToast: () => {},
      dismissToast: () => {},
    });

    const mounted = mountComponent(() =>
      React.createElement(HomeView, {
        onNavigate: () => {},
      }),
    );

    try {
      // Page load: 0 requestAx calls (V2 no-prompt check).
      await mounted.flushAndRerender(4);
      expect(requestAxCallCount).toBe(0);

      // Mocked focus events: still 0 requestAx calls.
      focusHandler?.({ payload: true });
      focusHandler?.({ payload: false });
      focusHandler?.({ payload: true });
      await mounted.flushAndRerender(3);
      expect(requestAxCallCount).toBe(0);

      // Clicking the S4 Open System Settings button calls requestAx once per click.
      const btn = findButtonByText(mounted.getTree(), "Open System Settings");
      expect(btn).not.toBeNull();
      (btn!.props.onClick as () => void)();
      await Promise.resolve();
      expect(requestAxCallCount).toBe(1);

      (btn!.props.onClick as () => void)();
      await Promise.resolve();
      expect(requestAxCallCount).toBe(2);

      // Direct helper check: swallows rejections safely.
      let directCalls = 0;
      await openSystemSettingsAction(async () => {
        directCalls += 1;
      });
      expect(directCalls).toBe(1);
    } finally {
      stopListener();
      mounted.unmount();
      useSettingsStore.setState({ settings: prevSettings });
    }
  });

  test("T-b2: both onboarding button call sites render accessibility.openSettings ('Open System Settings') and all PB-F2 inline defaults match signed copy", () => {
    // 1. FirstRunWelcomeCard (AccessibilityOnboarding.tsx:180)
    requestedKeys.length = 0;
    const welcomeHtml = renderToStaticMarkup(
      React.createElement(FirstRunWelcomeCard, {
        microphoneStatus: "granted",
        accessibilityStatus: "needed",
        onGrantMicrophone: () => {},
        onGrantAccessibility: () => {},
        onRetry: () => {},
        onContinueWithoutTyping: () => {},
      }),
    );
    expect(welcomeHtml).toContain("Open System Settings");
    expect(welcomeHtml).toContain("Accessibility");
    expect(requestedKeys).toContain("accessibility.openSettings");
    expect(requestedKeys).not.toContain("onboarding.welcome.openAccessibility");

    // 2. FirstRunStaleAxCard (AccessibilityOnboarding.tsx:275)
    requestedKeys.length = 0;
    const staleHtml = renderToStaticMarkup(
      React.createElement(FirstRunStaleAxCard, {
        onOpenAccessibility: () => {},
        onContinueWithoutTyping: () => {},
      }),
    );
    expect(staleHtml).toContain("Open System Settings");
    expect(staleHtml).toContain("System Settings");
    expect(staleHtml).toContain(
      "1. In System Settings › Privacy &amp; Security, open the list of apps allowed to control your Mac, select Coco Voice and click minus (−).",
    );
    expect(requestedKeys).toContain("accessibility.openSettings");
    expect(requestedKeys).not.toContain("onboarding.welcome.openAccessibility");

    // 3. PB-F2 inline fallback defaults (with missing-key fallback mode)
    translationMode = "fallback";
    const fallbackNoticeHtml = renderToStaticMarkup(
      React.createElement(HomeHealthNoticesSection, {
        health: { staleAccessibility: true },
        onNavigate: () => {},
      }),
    );
    expect(fallbackNoticeHtml).toContain(
      "After the update, turn on Coco Voice again in System Settings",
    );

    const fallbackWelcomeHtml = renderToStaticMarkup(
      React.createElement(FirstRunWelcomeCard, {
        microphoneStatus: "granted",
        accessibilityStatus: "needed",
        onGrantMicrophone: () => {},
        onGrantAccessibility: () => {},
        onRetry: () => {},
        onContinueWithoutTyping: () => {},
      }),
    );
    expect(fallbackWelcomeHtml).toContain("Open System Settings");
    expect(fallbackWelcomeHtml).toContain("Accessibility");

    const fallbackStaleHtml = renderToStaticMarkup(
      React.createElement(FirstRunStaleAxCard, {
        onOpenAccessibility: () => {},
        onContinueWithoutTyping: () => {},
      }),
    );
    expect(fallbackStaleHtml).toContain(">System Settings<");
    expect(fallbackStaleHtml).toContain(
      "1. In System Settings › Privacy &amp; Security, open the list of apps allowed to control your Mac, select Coco Voice and click minus (−).",
    );
    expect(fallbackStaleHtml).toContain("Open System Settings");
  });
});
