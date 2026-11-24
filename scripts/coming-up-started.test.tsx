import { beforeEach, describe, expect, mock, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { MeetingStatus, UpcomingEvent } from "../src/bindings.meetings";

const sample: UpcomingEvent = {
  title: "Design review",
  start: 1700000000,
  end: 1700001800,
  calendar_name: "[Test] Product syncs",
  attendee_display_names: ["Alex"],
};

const idleStatus: MeetingStatus = {
  state: "idle",
  elapsed_secs: 0,
  meeting_id: null,
  mic_name: null,
  mic_only: false,
  warnings: [],
};

const consentOpenStatus: MeetingStatus = {
  ...idleStatus,
  state: "consent_open",
};

const recordingStatus: MeetingStatus = {
  state: "recording",
  elapsed_secs: 0,
  meeting_id: 202,
  title: "Design review",
  mic_name: "Built-in Mic",
  mic_only: false,
  warnings: [],
};

const micOnlyStatus: MeetingStatus = {
  ...recordingStatus,
  mic_only: true,
};

let startOutcome:
  { status: "ok"; data: MeetingStatus } | { status: "error"; error: unknown } =
  {
    status: "ok",
    data: recordingStatus,
  };
let micOnlyOutcome:
  { status: "ok"; data: MeetingStatus } | { status: "error"; error: unknown } =
  {
    status: "ok",
    data: micOnlyStatus,
  };
const keptEvents: Array<{
  start: number;
  title: string;
  calendar_name: string;
}> = [];
const eventHandlers = new Map<
  string,
  Set<(ev: { payload?: unknown }) => void>
>();

function fireEvent(name: string, payload?: unknown) {
  const handlers = eventHandlers.get(name);
  if (!handlers) return;
  for (const handler of [...handlers]) {
    handler({ payload });
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
  invoke: async () => [],
  transformCallback: () => 0,
  Channel: class {},
  Resource: class {},
  SERIALIZE_TO_IPC_FN: "__TAURI_TO_IPC_KEY__",
  convertFileSrc: (s: string) => s,
}));

mock.module("@tauri-apps/api/event", () => ({
  listen: async (
    event: string,
    handler: (ev: { payload?: unknown }) => void,
  ) => {
    let handlers = eventHandlers.get(event);
    if (!handlers) {
      handlers = new Set();
      eventHandlers.set(event, handlers);
    }
    handlers.add(handler);
    return () => {
      handlers?.delete(handler);
    };
  },
  once: async () => () => {},
  emit: async () => {},
  emitTo: async () => {},
  TauriEvent: {},
}));

mock.module("../src/bindings.meetings", () => ({
  commands: {
    meetingStatus: async () => idleStatus,
    meetingSegments: async () => ({ status: "ok", data: [] }),
    currentMeetingOffer: async () => ({ status: "ok", data: null }),
    getMeetingCustomWords: async () => ({ status: "ok", data: [] }),
    setMeetingCustomWords: async () => ({ status: "ok", data: null }),
    setMeetingOfferVisible: async () => ({ status: "ok", data: null }),
    changeMeetingsEnabledSetting: async () => ({ status: "ok", data: null }),
    openMeetingConsent: async () => {
      fireEvent("meetings-open-consent");
      return { status: "ok", data: consentOpenStatus };
    },
    confirmMeetingStart: async () => startOutcome,
    confirmMeetingStartMicOnly: async () => micOnlyOutcome,
    cancelMeetingConsent: async () => ({ status: "ok", data: idleStatus }),
    dismissMeetingOffer: async () => ({ status: "ok", data: null }),
    stopMeeting: async () => ({ status: "ok", data: idleStatus }),
    pauseMeeting: async () => ({ status: "ok", data: idleStatus }),
    resumeMeeting: async () => ({ status: "ok", data: recordingStatus }),
    appendMeetingAudio: async () => ({ status: "ok", data: recordingStatus }),
    meetingNotesSnapshot: async () => ({
      status: "ok",
      data: {
        meeting_id: 202,
        state: "idle",
        progress_label: null,
        elapsed_secs: 0,
        estimate_secs: null,
        time_to_notes_secs: null,
        failure_reason: null,
        notepad_text: "",
        lines: [],
        oversized_count: 0,
      },
    }),
    saveMeetingNotepad: async () => ({ status: "ok", data: null }),
    retryMeetingNotes: async () => ({ status: "ok", data: null }),
    renameMeeting: async (_id: number, title: string) => ({
      status: "ok",
      data: title,
    }),
    openSystemAudioPrivacySettings: async () => ({ status: "ok", data: null }),
    listUpcomingMeetings: async () => ({
      status: "ok",
      data: [sample],
    }),
    keepUpcomingForNotes: async (
      start: number,
      title: string,
      calendar_name: string,
    ) => {
      keptEvents.push({ start, title, calendar_name });
      return { status: "ok", data: sample };
    },
    closeUpcomingMeetings: async () => ({ status: "ok", data: null }),
  },
}));

mock.module("../src/hooks/useSettings", () => ({
  useSettings: () => ({
    settings: {
      selected_calendars: ["test-1"],
      push_to_talk: true,
      bindings: { transcribe: { current_binding: "fn" } },
    },
    refreshSettings: async () => {},
  }),
}));

const { RecordMeetingPage } =
  await import("../src/components/meetings/RecordMeetingPage");

type Elem = React.ReactElement<Record<string, unknown>>;

function mountAppTree(rootFactory: () => React.ReactNode) {
  interface Instance {
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

  const depsChanged = (a?: readonly unknown[], b?: readonly unknown[]) =>
    !a || !b || a.length !== b.length || b.some((v, i) => !Object.is(a[i], v));

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
        return node.map((child, idx) => {
          const out = walk(child, `${path}[${idx}]`);
          if (
            out &&
            typeof out === "object" &&
            "type" in out &&
            out.key == null
          ) {
            return React.cloneElement(out as Elem, { key: `k-${idx}` });
          }
          return out;
        });
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
          inst?.effects.forEach((e) => e?.cleanup?.());
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
          useState<T>(init: T | (() => T)) {
            const idx = stateCursor++;
            if (idx >= inst!.states.length) {
              inst!.states[idx] =
                typeof init === "function" ? (init as () => T)() : init;
            }
            const target = inst!;
            return [
              target.states[idx] as T,
              (next: T | ((p: T) => T)) => {
                target.states[idx] =
                  typeof next === "function"
                    ? (next as (p: T) => T)(target.states[idx] as T)
                    : next;
              },
            ];
          },
          useEffect(fn: () => void | (() => void), deps?: readonly unknown[]) {
            const idx = effCursor++;
            if (
              !inst!.effects[idx] ||
              depsChanged(inst!.effects[idx]!.deps, deps)
            ) {
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
            if (
              !inst!.callbacks[idx] ||
              depsChanged(inst!.callbacks[idx]!.deps, deps)
            ) {
              inst!.callbacks[idx] = { fn, deps };
            }
            return inst!.callbacks[idx]!.fn as T;
          },
          useMemo<T>(fn: () => T, deps?: readonly unknown[]) {
            const idx = memoCursor++;
            if (
              !inst!.memos[idx] ||
              depsChanged(inst!.memos[idx]!.deps, deps)
            ) {
              inst!.memos[idx] = { value: fn(), deps };
            }
            return inst!.memos[idx]!.value as T;
          },
          useContext: (ctx: { _currentValue?: unknown }) => ctx._currentValue,
          useSyncExternalStore: <T,>(_s: unknown, g: () => T) => g(),
          useId: () => `id-${instKey}`,
          useTransition: () => [false, (fn: () => void) => fn()],
          useDebugValue() {},
          useDeferredValue: <T,>(v: T) => v,
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
        inst.effects.forEach((e) => e?.cleanup?.());
        instances.delete(k);
      }
    }
    for (const eff of pendingEffects) {
      eff.inst.effects[eff.idx]?.cleanup?.();
      eff.inst.effects[eff.idx] = { deps: eff.deps, cleanup: eff.fn() };
    }
    return renderedTree;
  };

  const unmount = () => {
    for (const inst of instances.values()) {
      inst.effects.forEach((e) => e?.cleanup?.());
    }
    instances.clear();
  };

  const flushAndRerender = async (rounds = 4) => {
    for (let i = 0; i < rounds; i += 1) {
      await Promise.resolve();
      await Promise.resolve();
      rerender();
    }
    return renderedTree;
  };

  rerender();
  return { getTree: () => renderedTree, flushAndRerender, unmount };
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
    return textContent((node as Elem).props?.children as React.ReactNode);
  }
  return "";
}

const toHtml = (tree: React.ReactNode) =>
  renderToStaticMarkup(<>{tree}</>).replaceAll("&#x27;", "'");

async function openConsentFromComingUp() {
  const mounted = mountAppTree(() => <RecordMeetingPage />);
  await mounted.flushAndRerender();

  const openViewBtns = collectElements(
    mounted.getTree(),
    (el) =>
      el.type === "button" && el.props["data-testid"] === "coming-up-open-view",
  );
  expect(openViewBtns.length).toBe(1);
  (openViewBtns[0]!.props.onClick as () => void)();
  await mounted.flushAndRerender();

  const beforeHtml = toHtml(mounted.getTree());
  expect(beforeHtml).toContain('data-testid="frame-10-coming-up"');
  expect(beforeHtml).toContain("Design review");
  expect(beforeHtml).toContain("Not started");
  expect(beforeHtml).not.toContain("Notes started");

  const startNotesBtns = collectElements(
    mounted.getTree(),
    (el) => el.type === "button" && textContent(el).trim() === "Start notes",
  );
  expect(startNotesBtns.length).toBe(1);
  (startNotesBtns[0]!.props.onClick as () => void)();
  await mounted.flushAndRerender();

  expect(toHtml(mounted.getTree())).toContain('data-testid="frame-2-consent"');
  return mounted;
}

describe("Coming up row started status and event hold", () => {
  beforeEach(() => {
    keptEvents.length = 0;
    eventHandlers.clear();
    startOutcome = { status: "ok", data: recordingStatus };
    micOnlyOutcome = { status: "ok", data: micOnlyStatus };
  });

  test("cancel leaves row as Not started and never calls keepUpcomingForNotes", async () => {
    const mounted = await openConsentFromComingUp();
    try {
      const dialogNodes = collectElements(
        mounted.getTree(),
        (el) => el.props["data-testid"] === "frame-2-consent",
      );
      const cancelBtns = collectElements(
        dialogNodes[0]!,
        (el) => el.type === "button" && textContent(el).trim() === "Cancel",
      );
      (cancelBtns[0]!.props.onClick as () => void)();
      await mounted.flushAndRerender();

      const afterHtml = toHtml(mounted.getTree());
      expect(afterHtml).toContain('data-testid="frame-10-coming-up"');
      expect(afterHtml).toContain("Not started");
      expect(afterHtml).not.toContain("Notes started");
      expect(keptEvents).toEqual([]);
    } finally {
      mounted.unmount();
    }
  });

  test("refusal leaves row as Not started and never calls keepUpcomingForNotes", async () => {
    startOutcome = {
      status: "error",
      error: {
        system_audio_tap_failed: {
          cause: "permission",
          call: "create_process_tap",
          code: "!obj (560947818)",
        },
      },
    };
    const mounted = await openConsentFromComingUp();
    try {
      const dialogNodes = collectElements(
        mounted.getTree(),
        (el) => el.props["data-testid"] === "frame-2-consent",
      );
      const startBtns = collectElements(
        dialogNodes[0]!,
        (el) =>
          el.type === "button" && textContent(el).trim() === "Start recording",
      );
      (startBtns[0]!.props.onClick as () => void)();
      await mounted.flushAndRerender();

      expect(keptEvents).toEqual([]);
      const panelNodes = collectElements(
        mounted.getTree(),
        (el) => el.props["data-testid"] === "meeting-start-problem-panel",
      );
      const cancelBtns = collectElements(
        panelNodes[0]!,
        (el) => el.type === "button" && textContent(el).trim() === "Cancel",
      );
      (cancelBtns[0]!.props.onClick as () => void)();
      await mounted.flushAndRerender();

      const afterHtml = toHtml(mounted.getTree());
      expect(afterHtml).toContain('data-testid="frame-10-coming-up"');
      expect(afterHtml).toContain("Not started");
      expect(afterHtml).not.toContain("Notes started");
      expect(keptEvents).toEqual([]);
    } finally {
      mounted.unmount();
    }
  });

  test("ok start marks row as Notes started and calls keepUpcomingForNotes once", async () => {
    const mounted = await openConsentFromComingUp();
    try {
      const dialogNodes = collectElements(
        mounted.getTree(),
        (el) => el.props["data-testid"] === "frame-2-consent",
      );
      const startBtns = collectElements(
        dialogNodes[0]!,
        (el) =>
          el.type === "button" && textContent(el).trim() === "Start recording",
      );
      (startBtns[0]!.props.onClick as () => void)();
      await mounted.flushAndRerender();

      expect(keptEvents).toEqual([
        {
          start: sample.start,
          title: sample.title,
          calendar_name: sample.calendar_name,
        },
      ]);

      const stopBtns = collectElements(
        mounted.getTree(),
        (el) => el.type === "button" && textContent(el).trim() === "Stop",
      );
      (stopBtns[0]!.props.onClick as () => void)();
      await mounted.flushAndRerender();

      const afterHtml = toHtml(mounted.getTree());
      expect(afterHtml).toContain('data-testid="frame-10-coming-up"');
      expect(afterHtml).toContain("Notes started");
      expect(afterHtml).not.toContain("Not started");
    } finally {
      mounted.unmount();
    }
  });

  test("mic-only fallback after refusal marks row as Notes started and calls keepUpcomingForNotes once", async () => {
    startOutcome = {
      status: "error",
      error: {
        system_audio_tap_failed: {
          cause: "permission",
          call: "create_process_tap",
          code: "!obj (560947818)",
        },
      },
    };
    const mounted = await openConsentFromComingUp();
    try {
      const dialogNodes = collectElements(
        mounted.getTree(),
        (el) => el.props["data-testid"] === "frame-2-consent",
      );
      const startBtns = collectElements(
        dialogNodes[0]!,
        (el) =>
          el.type === "button" && textContent(el).trim() === "Start recording",
      );
      (startBtns[0]!.props.onClick as () => void)();
      await mounted.flushAndRerender();
      expect(keptEvents).toEqual([]);

      const micBtns = collectElements(
        mounted.getTree(),
        (el) =>
          el.type === "button" &&
          el.props["data-testid"] === "meeting-start-mic-only-btn",
      );
      (micBtns[0]!.props.onClick as () => void)();
      await mounted.flushAndRerender();

      expect(keptEvents).toEqual([
        {
          start: sample.start,
          title: sample.title,
          calendar_name: sample.calendar_name,
        },
      ]);

      const stopBtns = collectElements(
        mounted.getTree(),
        (el) => el.type === "button" && textContent(el).trim() === "Stop",
      );
      (stopBtns[0]!.props.onClick as () => void)();
      await mounted.flushAndRerender();

      const afterHtml = toHtml(mounted.getTree());
      expect(afterHtml).toContain('data-testid="frame-10-coming-up"');
      expect(afterHtml).toContain("Notes started");
      expect(afterHtml).not.toContain("Not started");
    } finally {
      mounted.unmount();
    }
  });

  test("an open-consent event from outside the page drops a pending coming-up meeting", async () => {
    const mounted = await openConsentFromComingUp();
    try {
      const dialogNodes = collectElements(
        mounted.getTree(),
        (el) => el.props["data-testid"] === "frame-2-consent",
      );
      const cancelBtns = collectElements(
        dialogNodes[0]!,
        (el) => el.type === "button" && textContent(el).trim() === "Cancel",
      );
      (cancelBtns[0]!.props.onClick as () => void)();
      await mounted.flushAndRerender();

      fireEvent("meetings-open-consent");
      await mounted.flushAndRerender();

      const reopenedDialog = collectElements(
        mounted.getTree(),
        (el) => el.props["data-testid"] === "frame-2-consent",
      );
      expect(reopenedDialog.length).toBe(1);
      const startBtns = collectElements(
        reopenedDialog[0]!,
        (el) =>
          el.type === "button" && textContent(el).trim() === "Start recording",
      );
      (startBtns[0]!.props.onClick as () => void)();
      await mounted.flushAndRerender();

      expect(keptEvents).toEqual([]);
    } finally {
      mounted.unmount();
    }
  });
});
