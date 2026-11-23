import { beforeEach, describe, expect, mock, test } from "bun:test";
import React from "react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { ShortcutRegistrationFailure } from "../src/lib/types/events";

type EventHandler = (event: { payload: unknown }) => void;

const eventListeners = new Map<string, Set<EventHandler>>();
let homeHealthCallCount = 0;
let currentHealthPayload: Record<string, unknown> = {
  staleAccessibility: false,
  shortcutUnset: false,
  shortcutFailureCause: null,
  typingOff: false,
  cleanupEnabledWithoutLocalModel: false,
  cleanupNoticeAlreadyShown: false,
  updateReadyVersion: null,
  rolledBackFromVersion: null,
};

function emitEvent(name: string, payload: unknown = null) {
  const handlers = eventListeners.get(name);
  if (!handlers) return;
  for (const handler of [...handlers]) {
    handler({ payload });
  }
}

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
      homeHealthCallCount += 1;
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
  requestAccessibilityPermission: async () => {},
}));

mock.module("@/bindings", () => ({
  commands: {
    getHistoryEntries: async () => ({
      status: "ok",
      data: { entries: [], has_more: false },
    }),
    initializeShortcuts: async () => ({ status: "ok", data: null }),
    showMainWindowCommand: async () => {},
  },
}));

const testI18n = i18next.createInstance();
await testI18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

mock.module("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      String(testI18n.t(key, opts)),
    i18n: testI18n,
  }),
  initReactI18next,
}));

const {
  SHORTCUT_REGISTRATION_TOAST_ID,
  shortcutFailureBodyKey,
  startShortcutRegistrationListener,
} = await import("../src/hooks/useShortcutRegistration");
const { resolveTopHomeNotice } =
  await import("../src/components/home/HomeHealthNotice");
const { HomeView } = await import("../src/components/home/HomeView");
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

async function flushMicrotasks(rounds = 6) {
  for (let i = 0; i < rounds; i += 1) {
    await Promise.resolve();
  }
}

function makeControllerHarness(options: {
  platformName?: string;
  checkTrust?: () => Promise<boolean>;
}) {
  const listeners = new Map<string, (event: { payload: unknown }) => void>();
  let focusHandler: ((event: { payload: boolean }) => void) | null = null;
  let initCalls = 0;
  let showWindowCalls = 0;
  let openSettingsCalls = 0;
  const shownToasts: Array<{
    title: string;
    id: string;
    description: string;
    onTryAgain: () => void;
    onOpenSettings: () => void;
  }> = [];
  const dismissedIds: string[] = [];

  const stop = startShortcutRegistrationListener({
    t: (key, opts) =>
      opts ? `${key}(${JSON.stringify(opts)})` : key,
    onOpenSettings: () => {
      openSettingsCalls += 1;
    },
    listen: async (event, handler) => {
      listeners.set(event, handler as (e: { payload: unknown }) => void);
      return () => {
        listeners.delete(event);
      };
    },
    onFocusChanged: async (handler) => {
      focusHandler = handler;
      return () => {
        focusHandler = null;
      };
    },
    getPlatform: () => options.platformName ?? "macos",
    checkAccessibilityPermission: options.checkTrust ?? (async () => true),
    initializeShortcuts: async () => {
      initCalls += 1;
    },
    showMainWindowCommand: async () => {
      showWindowCalls += 1;
    },
    showToast: (title, opts) => {
      shownToasts.push({
        title,
        id: opts.id,
        description: opts.description,
        onTryAgain: opts.cancel.onClick,
        onOpenSettings: opts.action.onClick,
      });
    },
    dismissToast: (id) => {
      dismissedIds.push(id);
    },
  });

  return {
    stop,
    emitFailed: (failures: ShortcutRegistrationFailure[]) => {
      listeners.get("shortcut-registration-failed")?.({ payload: failures });
    },
    emitRegistered: () => {
      listeners.get("shortcuts-registered")?.({ payload: null });
    },
    emitFocus: (focused: boolean) => {
      focusHandler?.({ payload: focused });
    },
    getFocusHandler: () => focusHandler,
    getInitCalls: () => initCalls,
    getShowWindowCalls: () => showWindowCalls,
    getOpenSettingsCalls: () => openSettingsCalls,
    shownToasts,
    dismissedIds,
  };
}

describe("Voice F-g / F-h Part A frontend recovery (T-g4, T-g8, T-h2, T-h3)", () => {
  beforeEach(() => {
    eventListeners.clear();
    homeHealthCallCount = 0;
    currentHealthPayload = {
      staleAccessibility: false,
      shortcutUnset: false,
      shortcutFailureCause: null,
      typingOff: false,
      cleanupEnabledWithoutLocalModel: false,
      cleanupNoticeAlreadyShown: false,
      updateReadyVersion: null,
      rolledBackFromVersion: null,
    };
  });

  test("T-g4: focus retry fires once when pending & trusted, 0 when untrusted/not-pending/registered/non-macOS, and coalesces rapid focus events", async () => {
    // 1. Focus with pending set and trust true -> 1 call.
    let trust = true;
    const h1 = makeControllerHarness({ checkTrust: async () => trust });
    await flushMicrotasks();
    h1.emitFailed([
      {
        id: "transcribe",
        binding: "fn",
        error: "Accessibility permission not granted",
        cause: "permission",
      },
    ]);
    h1.emitFocus(true);
    await flushMicrotasks();
    expect(h1.getInitCalls()).toBe(1);
    h1.stop();

    // 2. Trust false -> 0 calls even across repeated focus gains (AC-3).
    trust = false;
    const h2 = makeControllerHarness({ checkTrust: async () => trust });
    await flushMicrotasks();
    h2.emitFailed([
      {
        id: "transcribe",
        binding: "fn",
        error: "Accessibility permission not granted",
        cause: "permission",
      },
    ]);
    for (let i = 0; i < 5; i += 1) {
      h2.emitFocus(true);
      await flushMicrotasks();
    }
    expect(h2.getInitCalls()).toBe(0);
    h2.stop();

    // 3. Not pending (no permission failure) -> 0 calls.
    const h3 = makeControllerHarness({ checkTrust: async () => true });
    await flushMicrotasks();
    h3.emitFocus(true);
    await flushMicrotasks();
    expect(h3.getInitCalls()).toBe(0);
    h3.stop();

    // 4. After shortcuts-registered -> 0 calls.
    const h4 = makeControllerHarness({ checkTrust: async () => true });
    await flushMicrotasks();
    h4.emitFailed([
      {
        id: "transcribe",
        binding: "fn",
        error: "Accessibility permission not granted",
        cause: "permission",
      },
    ]);
    h4.emitRegistered();
    h4.emitFocus(true);
    await flushMicrotasks();
    expect(h4.getInitCalls()).toBe(0);
    h4.stop();

    // 5. Two quick focus events back-to-back while trust check is in flight -> 1 call.
    let resolveTrust!: (v: boolean) => void;
    let trustChecks = 0;
    const h5 = makeControllerHarness({
      checkTrust: () => {
        trustChecks += 1;
        return new Promise<boolean>((resolve) => {
          resolveTrust = resolve;
        });
      },
    });
    await flushMicrotasks();
    h5.emitFailed([
      {
        id: "transcribe",
        binding: "fn",
        error: "Accessibility permission not granted",
        cause: "permission",
      },
    ]);
    h5.emitFocus(true);
    h5.emitFocus(true);
    expect(trustChecks).toBe(1);
    resolveTrust(true);
    await flushMicrotasks();
    expect(h5.getInitCalls()).toBe(1);
    h5.stop();

    // 6. Non-macOS -> focus listener never attached, 0 calls.
    const h6 = makeControllerHarness({
      platformName: "linux",
      checkTrust: async () => true,
    });
    await flushMicrotasks();
    expect(h6.getFocusHandler()).toBeNull();
    h6.emitFailed([
      {
        id: "transcribe",
        binding: "fn",
        error: "Accessibility permission not granted",
        cause: "permission",
      },
    ]);
    h6.emitFocus(true);
    await flushMicrotasks();
    expect(h6.getInitCalls()).toBe(0);
    h6.stop();
  });

  test("T-g8: failure toasts share one fixed id, shortcuts-registered dismisses it, and taken/rejected/other keep today's keys", async () => {
    expect(shortcutFailureBodyKey("taken")).toBe(
      "errors.shortcutRegistrationFailedTaken",
    );
    expect(shortcutFailureBodyKey("rejected")).toBe(
      "errors.shortcutRegistrationFailedRejected",
    );
    expect(shortcutFailureBodyKey("other")).toBe(
      "errors.shortcutRegistrationFailedOther",
    );
    // Part B: permission uses the signed permission body key.
    expect(shortcutFailureBodyKey("permission")).toBe(
      "errors.shortcutRegistrationFailedPermission",
    );

    const h = makeControllerHarness({});
    await flushMicrotasks();

    h.emitFailed([
      {
        id: "transcribe",
        binding: "fn",
        error: "Hotkey already registered: fn",
        cause: "taken",
      },
    ]);
    h.emitFailed([
      {
        id: "transcribe",
        binding: "fn",
        error: "Unknown key",
        cause: "rejected",
      },
    ]);

    expect(h.shownToasts).toHaveLength(2);
    expect(h.shownToasts[0]!.id).toBe(SHORTCUT_REGISTRATION_TOAST_ID);
    expect(h.shownToasts[1]!.id).toBe(SHORTCUT_REGISTRATION_TOAST_ID);
    expect(h.shownToasts[0]!.description).toContain(
      "errors.shortcutRegistrationFailedTaken",
    );
    expect(h.shownToasts[1]!.description).toContain(
      "errors.shortcutRegistrationFailedRejected",
    );

    h.shownToasts[1]!.onTryAgain();
    expect(h.getInitCalls()).toBe(1);

    h.emitRegistered();
    expect(h.dismissedIds).toEqual([SHORTCUT_REGISTRATION_TOAST_ID]);
    h.stop();
  });

  test("T-h2: resolveTopHomeNotice keeps shortcut_unset above typing_off", () => {
    const notice = resolveTopHomeNotice({
      shortcutUnset: true,
      typingOff: true,
    });
    expect(notice?.kind).toBe("shortcut_unset");
    expect(notice?.actionKey).toBe("homeNotices.chooseShortcut");
  });

  test("T-h3: HomeView re-reads home_health_status on shortcut-registration-failed and shortcuts-registered", async () => {
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

    const mounted = mountComponent(() =>
      React.createElement(HomeView, {
        onNavigate: () => {},
      }),
    );

    try {
      await mounted.flushAndRerender(4);
      expect(homeHealthCallCount).toBe(1);
      expect(textContent(mounted.getTree())).not.toContain(
        "Your shortcut is not set up",
      );

      // Failure event triggers a fresh home_health_status read -> notice b appears.
      currentHealthPayload = {
        ...currentHealthPayload,
        shortcutUnset: true,
        shortcutFailureCause: "taken",
      };
      emitEvent("shortcut-registration-failed", [
        {
          id: "transcribe",
          binding: "fn",
          error: "Hotkey already registered: fn",
          cause: "taken",
        },
      ]);
      await mounted.flushAndRerender(4);
      expect(homeHealthCallCount).toBe(2);
      const afterFailText = textContent(mounted.getTree());
      expect(afterFailText).toContain("Your shortcut is not set up");
      expect(afterFailText).toContain("Another app uses fn.");

      // shortcuts-registered triggers a fresh home_health_status read -> notice b clears.
      currentHealthPayload = {
        ...currentHealthPayload,
        shortcutUnset: false,
        shortcutFailureCause: null,
      };
      emitEvent("shortcuts-registered", null);
      await mounted.flushAndRerender(4);
      expect(homeHealthCallCount).toBe(3);
      expect(textContent(mounted.getTree())).not.toContain(
        "Your shortcut is not set up",
      );
    } finally {
      mounted.unmount();
      useSettingsStore.setState({ settings: prevSettings });
    }
  });
});
