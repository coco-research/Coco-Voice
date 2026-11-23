import { beforeEach, describe, expect, mock, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { MeetingStatus } from "../src/bindings.meetings";
import {
  MEETING_STATE_EVENT,
  MEETINGS_OPEN_CONSENT_EVENT,
} from "../src/components/meetings/meetingEvents";

const invokeCalls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
let nextInvokeHandler:
  | ((cmd: string, args?: Record<string, unknown>) => Promise<unknown>)
  | null = null;
const eventListeners = new Map<
  string,
  Array<(ev: { event: string; payload: unknown }) => void>
>();

function emitMockEvent(event: string, payload: unknown) {
  const list = [...(eventListeners.get(event) ?? [])];
  for (const cb of list) {
    cb({ event, payload });
  }
}

const i18nInst = i18next.createInstance();
await i18nInst.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

mock.module("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: unknown) =>
      i18nInst.t(key, opts as Record<string, unknown>) as string,
    i18n: i18nInst,
  }),
  initReactI18next,
  Trans: ({ children }: { children?: React.ReactNode }) => children ?? null,
}));

mock.module("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    invokeCalls.push({ cmd, args });
    if (nextInvokeHandler) return nextInvokeHandler(cmd, args);
    return null;
  },
  transformCallback: () => 0,
  Channel: class {},
  Resource: class {},
  SERIALIZE_TO_IPC_FN: "__TAURI_TO_IPC_KEY__",
  convertFileSrc: (s: string) => s,
}));

mock.module("@tauri-apps/api/event", () => ({
  listen: async (
    event: string,
    handler: (ev: { event: string; payload: unknown }) => void,
  ) => {
    const list = eventListeners.get(event) ?? [];
    list.push(handler);
    eventListeners.set(event, list);
    return () => {
      const cur = eventListeners.get(event);
      if (!cur) return;
      const idx = cur.indexOf(handler);
      if (idx >= 0) cur.splice(idx, 1);
    };
  },
  emit: async () => {},
}));

mock.module("@tauri-apps/api/app", () => ({
  getVersion: async () => "0.20.0",
  getName: async () => "Coco Voice Lab",
}));

mock.module("@tauri-apps/plugin-os", () => ({
  platform: () => "macos",
  version: () => "15.1.0",
  type: () => "macos",
}));

mock.module("tauri-plugin-macos-permissions-api", () => ({
  checkAccessibilityPermission: async () => true,
  checkMicrophonePermission: async () => true,
  requestAccessibilityPermission: async () => true,
  requestMicrophonePermission: async () => true,
}));

mock.module("sonner", () => ({
  Toaster: () => null,
  toast: Object.assign(() => {}, {
    error: () => {},
    success: () => {},
    info: () => {},
    warning: () => {},
  }),
}));

mock.module("../src/components/whats-new", () => ({
  WhatsNewGate: () => null,
}));

mock.module("../src/components/whats-new/releaseNotes", () => ({
  findReleaseNoteToShow: () => null,
  findLatestReleaseNote: () => null,
}));

mock.module("../src/components/AccessibilityPermissions", () => ({
  default: () => null,
}));

mock.module("../src/components/footer", () => ({
  default: () => null,
}));

mock.module("../src/i18n", () => ({
  default: i18nInst,
  SUPPORTED_LANGUAGES: [{ code: "en", name: "English", nativeName: "English" }],
  syncLanguageFromSettings: async () => {},
  getLanguageDirection: () => "ltr",
  isRTLLanguage: () => false,
}));

// Load the REAL meetingsUi and wire useMeetingsUi to return it synchronously so App runs with meetings ON.
const { meetingsUi } = await import("../src/components/meetings/meetings-ui");
mock.module("../src/components/meetings/gate", () => ({
  SPEAKER_SPLIT_ENABLED: false,
  useMeetingsUi: () => meetingsUi,
  useSpeakerSplitSection: () => null,
  buildSpeakerNameMapFromSlots: () => ({}),
}));

const { useSettingsStore } = await import("../src/stores/settingsStore");
const { default: App } = await import("../src/App");

type Elem = React.ReactElement<Record<string, unknown>>;

/**
 * Recursive React 18 tree mounter with per-instance hook state, effect lifecycle,
 * and automatic unmount cleanup when components leave the tree (e.g. section switches).
 */
function mountAppTree(rootFactory: () => React.ReactNode) {
  interface Instance {
    type: unknown;
    states: unknown[];
    refs: Array<{ current: unknown }>;
    callbacks: Array<{ fn: unknown; deps?: readonly unknown[] }>;
    memos: Array<{ value: unknown; deps?: readonly unknown[] }>;
    effects: Array<{ deps?: readonly unknown[]; cleanup?: void | (() => void) }>;
  }
  const instances = new Map<string, Instance>();
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const reactAny = React as any;
  const r18 =
    reactAny.__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED
      ?.ReactCurrentDispatcher;
  const r19 =
    reactAny.__CLIENT_INTERNALS_DO_NOT_USE_OR_WARN_USERS_THEY_CANNOT_UPGRADE;
  const getDisp = () => (r18 ? r18.current : r19.H);
  const setDisp = (d: unknown) => {
    if (r18) r18.current = d;
    else r19.H = d;
  };
  const origDispatcher = getDisp();

  const depsChanged = (
    prev?: readonly unknown[],
    next?: readonly unknown[],
  ) => {
    if (!prev || !next) return true;
    if (prev.length !== next.length) return true;
    for (let i = 0; i < next.length; i += 1) {
      if (!Object.is(prev[i], next[i])) return true;
    }
    return false;
  };

  let renderedTree: React.ReactNode = null;

  const rerender = (): React.ReactNode => {
    const alive = new Set<string>();
    const pendingEffects: Array<{
      inst: Instance;
      idx: number;
      fn: () => void | (() => void);
      deps?: readonly unknown[];
    }> = [];

    const walk = (node: React.ReactNode, path: string): React.ReactNode => {
      if (
        node == null ||
        typeof node === "boolean" ||
        typeof node === "string" ||
        typeof node === "number"
      ) {
        return node;
      }
      if (Array.isArray(node)) {
        return node.map((child, idx) => walk(child, `${path}[${idx}]`));
      }
      if (typeof node !== "object" || !("type" in node)) return node;
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
          useState<T>(init: T | (() => T)): [T, (next: T | ((p: T) => T)) => void] {
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
          walk(el.props.children as React.ReactNode, `${path}.${String(el.type)}`),
        );
      }
      return el;
    };

    renderedTree = walk(rootFactory(), "root");

    // Clean up unmounted instances
    for (const [k, inst] of [...instances.entries()]) {
      if (!alive.has(k)) {
        for (const eff of inst.effects) {
          if (eff?.cleanup) eff.cleanup();
        }
        instances.delete(k);
      }
    }

    // Run queued effects
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
      await new Promise((r) => setTimeout(r, 0));
      rerender();
    }
    return renderedTree;
  };

  rerender();
  return {
    getTree: () => renderedTree,
    rerender,
    flushAndRerender,
    unmount,
  };
}

function collectElements(
  node: React.ReactNode,
  pred: (el: Elem) => boolean,
  acc: Elem[] = [],
): Elem[] {
  if (node == null || typeof node !== "object") return acc;
  if (Array.isArray(node)) {
    for (const c of node) collectElements(c, pred, acc);
    return acc;
  }
  const el = node as Elem;
  if (pred(el)) acc.push(el);
  if (el.props && "children" in el.props) {
    collectElements(el.props.children as React.ReactNode, pred, acc);
  }
  return acc;
}

function textContent(node: React.ReactNode): string {
  if (node == null || typeof node === "boolean") return "";
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textContent).join("");
  if (typeof node === "object" && "props" in node) {
    const el = node as Elem;
    return textContent(el.props?.children as React.ReactNode);
  }
  return "";
}

function withAppGlobals<T>(fn: () => Promise<T>): Promise<T> {
  const g = globalThis as {
    window?: unknown;
    document?: unknown;
    localStorage?: unknown;
  };
  const prevWin = Object.getOwnPropertyDescriptor(globalThis, "window");
  const prevDoc = Object.getOwnPropertyDescriptor(globalThis, "document");
  const prevLs = Object.getOwnPropertyDescriptor(globalThis, "localStorage");

  const storage = new Map<string, string>();
  g.localStorage = {
    getItem: (k: string) => storage.get(k) ?? null,
    setItem: (k: string, v: string) => storage.set(k, v),
    removeItem: (k: string) => storage.delete(k),
  };
  g.document = {
    documentElement: {
      dir: "ltr",
      dataset: {},
      style: {},
      setAttribute() {},
      getAttribute: () => null,
      classList: { toggle() {}, add() {}, remove() {} },
    },
    addEventListener() {},
    removeEventListener() {},
  };
  g.window = {
    __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos", version: "15.1.0" },
    setTimeout: globalThis.setTimeout.bind(globalThis),
    clearTimeout: globalThis.clearTimeout.bind(globalThis),
    addEventListener() {},
    removeEventListener() {},
    matchMedia: () => ({
      matches: false,
      addEventListener() {},
      removeEventListener() {},
    }),
  };

  const prevState = useSettingsStore.getState();
  const prevInitial = { ...useSettingsStore.getInitialState() };
  const nextSettings = {
    ...(prevState.settings ?? {}),
    onboarding_completed: true,
    meetings_enabled: true,
    selected_model: "parakeet-tdt-0.6b-v3-int8",
    app_language: "en",
    theme: "light",
    bindings: {
      transcribe: {
        id: "transcribe",
        name: "Transcribe",
        description: "",
        default_binding: "fn",
        current_binding: "fn",
      },
    },
  };
  const nextState = {
    isLoading: false,
    settings: nextSettings as never,
    refreshAudioDevices: async () => {},
    refreshOutputDevices: async () => {},
    refreshSettings: async () => {},
  };
  Object.assign(useSettingsStore.getInitialState(), nextState);
  useSettingsStore.setState(nextState);

  return fn().finally(() => {
    Object.assign(useSettingsStore.getInitialState(), prevInitial);
    useSettingsStore.setState({
      isLoading: prevState.isLoading,
      settings: prevState.settings,
      refreshAudioDevices: prevState.refreshAudioDevices,
      refreshOutputDevices: prevState.refreshOutputDevices,
      refreshSettings: prevState.refreshSettings,
    });
    if (prevWin) Object.defineProperty(globalThis, "window", prevWin);
    else delete g.window;
    if (prevDoc) Object.defineProperty(globalThis, "document", prevDoc);
    else delete g.document;
    if (prevLs) Object.defineProperty(globalThis, "localStorage", prevLs);
    else delete g.localStorage;
  });
}

describe("X3h App-wide refusal hand-off (AC-B19j)", () => {
  beforeEach(() => {
    invokeCalls.length = 0;
    eventListeners.clear();
    nextInvokeHandler = null;
  });

  test("AC-B19j: app-wide ConsentDialog refusal closes dialog, switches to Record a meeting showing the approved panel, stays one-shot, and leaves successful starts unchanged", async () => {
    await withAppGlobals(async () => {
      const idleStatus: MeetingStatus = {
        state: "idle",
        elapsed_secs: 0,
        meeting_id: null,
        mic_name: null,
        mic_only: false,
        warnings: [],
      };
      const consentOpenStatus: MeetingStatus = {
        state: "consent_open",
        elapsed_secs: 0,
        meeting_id: null,
        mic_name: null,
        mic_only: false,
        warnings: [],
      };
      const recordingStatus: MeetingStatus = {
        state: "recording",
        elapsed_secs: 0,
        meeting_id: 202,
        title: "Product sync",
        mic_name: "Built-in Mic",
        mic_only: false,
        warnings: [],
      };
      const micOnlyStatus: MeetingStatus = {
        ...recordingStatus,
        mic_only: true,
      };

      const toHtml = (tree: React.ReactNode) =>
        renderToStaticMarkup(<>{tree}</>).replaceAll("&#x27;", "'");

      async function bootOnHomeAndOpenConsent(opts: {
        confirmResult: { ok: true } | { ok: false; error: unknown };
      }) {
        invokeCalls.length = 0;
        eventListeners.clear();
        let currentStatus: MeetingStatus = idleStatus;
        let micOnlyCalls = 0;
        let cancelCalls = 0;

        nextInvokeHandler = async (cmd) => {
          if (cmd === "get_app_settings") {
            return {
              onboarding_completed: true,
              meetings_enabled: true,
              selected_model: "parakeet-tdt-0.6b-v3-int8",
              app_language: "en",
            };
          }
          if (cmd === "has_any_models_available" || cmd === "has_any_models") {
            return true;
          }
          if (cmd === "meeting_status") return currentStatus;
          if (cmd === "current_meeting_offer") return null;
          if (cmd === "get_meeting_custom_words") return [];
          if (cmd === "list_meetings") return [];
          if (cmd === "list_upcoming_meetings") return [];
          if (cmd === "get_history_entries") {
            return { entries: [], has_more: false, next_cursor: null };
          }
          if (cmd === "get_available_models") return [];
          if (cmd === "confirm_meeting_start") {
            if (opts.confirmResult.ok) {
              currentStatus = recordingStatus;
              return recordingStatus;
            }
            throw opts.confirmResult.error;
          }
          if (cmd === "confirm_meeting_start_mic_only") {
            micOnlyCalls += 1;
            currentStatus = micOnlyStatus;
            return micOnlyStatus;
          }
          if (cmd === "cancel_meeting_consent") {
            cancelCalls += 1;
            currentStatus = idleStatus;
            return idleStatus;
          }
          return null;
        };

        const mounted = mountAppTree(() => <App />);
        await mounted.flushAndRerender(4);

        // Step 1: App shell is on Home with meetings on; deliver shortcut events.
        const homeHtml = toHtml(mounted.getTree());
        expect(homeHtml).not.toContain('data-testid="page-2-record-meeting"');
        expect(homeHtml).not.toContain('data-testid="frame-2-consent"');

        currentStatus = consentOpenStatus;
        emitMockEvent(MEETINGS_OPEN_CONSENT_EVENT, null);
        emitMockEvent(MEETING_STATE_EVENT, consentOpenStatus);
        await mounted.flushAndRerender(2);

        const openDialogHtml = toHtml(mounted.getTree());
        expect(openDialogHtml).toContain('data-testid="frame-2-consent"');

        // Click Start recording inside the app-wide ConsentDialog.
        const dialogNodes = collectElements(
          mounted.getTree(),
          (el) => el.props["data-testid"] === "frame-2-consent",
        );
        expect(dialogNodes.length).toBe(1);
        const startBtns = collectElements(
          dialogNodes[0],
          (el) =>
            el.type === "button" &&
            textContent(el).trim() === "Start recording",
        );
        expect(startBtns.length).toBe(1);
        (startBtns[0].props.onClick as () => void)();
        await mounted.flushAndRerender(4);

        return {
          mounted,
          getHtml: () => toHtml(mounted.getTree()),
          getStatus: () => currentStatus,
          getMicOnlyCalls: () => micOnlyCalls,
          getCancelCalls: () => cancelCalls,
        };
      }

      // Steps 1-3 & 6: permission tap failure -> dialog gone, Record a meeting shows r3a panel (3 buttons),
      // no page consent dialog, controller stays consent_open, mic-only works once; AND navigating Home -> back shows no panel (one-shot).
      const permRun = await bootOnHomeAndOpenConsent({
        confirmResult: {
          ok: false,
          error: {
            system_audio_tap_failed: {
              cause: "permission",
              call: "create_process_tap",
              code: "!obj (560947818)",
            },
          },
        },
      });
      try {
        const htmlAfter = permRun.getHtml();
        expect(htmlAfter).not.toContain('data-testid="frame-2-consent"');
        expect(htmlAfter).toContain('data-testid="page-2-record-meeting"');
        expect(htmlAfter).toContain('data-testid="meeting-start-problem-panel"');
        expect(permRun.getStatus().state).toBe("consent_open");
        expect(permRun.getCancelCalls()).toBe(0);

        const panelIdx = htmlAfter.indexOf('data-testid="meeting-start-problem-panel"');
        const panelSlice = htmlAfter.slice(
          panelIdx,
          htmlAfter.indexOf("</section>", panelIdx),
        );
        const permButtons = [
          ...panelSlice.matchAll(/<button[^>]*>(.*?)<\/button>/g),
        ].map((m) => m[1].replace(/<[^>]+>/g, "").trim());
        expect(permButtons).toEqual([
          "Record my mic only",
          "Open System Settings",
          "Cancel",
        ]);

        // Step 6: Go to Home and back to Record a meeting with no new refusal -> no panel shows (one-shot).
        const homeNavBtns = collectElements(
          permRun.mounted.getTree(),
          (el) =>
            el.type === "button" &&
            textContent(el).trim() === "Home",
        );
        expect(homeNavBtns.length).toBeGreaterThan(0);
        (homeNavBtns[0].props.onClick as () => void)();
        await permRun.mounted.flushAndRerender(3);
        expect(permRun.getHtml()).not.toContain('data-testid="page-2-record-meeting"');

        const recordNavBtns = collectElements(
          permRun.mounted.getTree(),
          (el) =>
            el.type === "button" &&
            textContent(el).trim() === "Record a meeting",
        );
        expect(recordNavBtns.length).toBeGreaterThan(0);
        (recordNavBtns[0].props.onClick as () => void)();
        await permRun.mounted.flushAndRerender(3);
        const revisitHtml = permRun.getHtml();
        expect(revisitHtml).toContain('data-testid="page-2-record-meeting"');
        expect(revisitHtml).not.toContain('data-testid="meeting-start-problem-panel"');
      } finally {
        permRun.mounted.unmount();
      }

      // Step 3: Fresh run -> click "Record my mic only" -> confirm_meeting_start_mic_only called exactly once.
      const micOnlyRun = await bootOnHomeAndOpenConsent({
        confirmResult: {
          ok: false,
          error: {
            system_audio_tap_failed: {
              cause: "permission",
              call: "create_process_tap",
              code: "!obj (560947818)",
            },
          },
        },
      });
      try {
        const micBtns = collectElements(
          micOnlyRun.mounted.getTree(),
          (el) =>
            el.type === "button" &&
            el.props["data-testid"] === "meeting-start-mic-only-btn",
        );
        expect(micBtns.length).toBe(1);
        (micBtns[0].props.onClick as () => void)();
        await micOnlyRun.mounted.flushAndRerender(3);
        expect(micOnlyRun.getMicOnlyCalls()).toBe(1);
        expect(micOnlyRun.getHtml()).toContain('data-testid="mic-only-chip"');
      } finally {
        micOnlyRun.mounted.unmount();
      }

      // Step 4: Fresh run with device refusal `!dev (560227702)` -> panel shows "(error !dev)" with 2 buttons.
      const devRun = await bootOnHomeAndOpenConsent({
        confirmResult: {
          ok: false,
          error: {
            system_audio_tap_failed: {
              cause: "device",
              call: "create_process_tap",
              code: "!dev (560227702)",
            },
          },
        },
      });
      try {
        const devHtml = devRun.getHtml();
        expect(devHtml).not.toContain('data-testid="frame-2-consent"');
        expect(devHtml).toContain('data-testid="page-2-record-meeting"');
        expect(devHtml).toContain('data-testid="meeting-start-problem-panel"');
        expect(devHtml).toContain("(error !dev)");
        const idx = devHtml.indexOf('data-testid="meeting-start-problem-panel"');
        const slice = devHtml.slice(idx, devHtml.indexOf("</section>", idx));
        const devButtons = [
          ...slice.matchAll(/<button[^>]*>(.*?)<\/button>/g),
        ].map((m) => m[1].replace(/<[^>]+>/g, "").trim());
        expect(devButtons).toEqual(["Record my mic only", "Cancel"]);
      } finally {
        devRun.mounted.unmount();
      }

      // Step 5: Fresh run with `system_audio_unavailable` -> Record a meeting shows never-silent message.
      const unavailRun = await bootOnHomeAndOpenConsent({
        confirmResult: {
          ok: false,
          error: "system_audio_unavailable",
        },
      });
      try {
        const unavailHtml = unavailRun.getHtml();
        expect(unavailHtml).not.toContain('data-testid="frame-2-consent"');
        expect(unavailHtml).toContain('data-testid="page-2-record-meeting"');
        expect(unavailHtml).toContain("Could not start the meeting.");
        expect(unavailHtml).toContain('role="alert"');
      } finally {
        unavailRun.mounted.unmount();
      }

      // Step 7: Fresh run where confirm_meeting_start SUCCEEDS -> dialog closes and section does NOT change (stays on Home).
      const okRun = await bootOnHomeAndOpenConsent({
        confirmResult: { ok: true },
      });
      try {
        const okHtml = okRun.getHtml();
        expect(okHtml).not.toContain('data-testid="frame-2-consent"');
        expect(okHtml).not.toContain('data-testid="page-2-record-meeting"');
        expect(okHtml).not.toContain('data-testid="meeting-start-problem-panel"');
      } finally {
        okRun.mounted.unmount();
      }
    });
  });
});
