import { describe, expect, mock, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type {
  MeetingNotesSnapshot,
  MeetingSegmentView,
  MeetingStatus,
} from "../src/bindings.meetings";
import type { MeetingListItem } from "../src/components/meetings/MeetingsList";

const i18nInst = i18next.createInstance();
await i18nInst.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const MEETING_ALPHA: MeetingListItem = {
  id: 1,
  title: "Monday standup",
  started_at: 1_700_000_000,
  ended_at: 1_700_000_900,
  notes_ready: true,
};
const MEETING_BETA: MeetingListItem = {
  id: 2,
  title: "Product sync",
  started_at: 1_700_003_600,
  ended_at: 1_700_005_400,
  notes_ready: true,
};
const SEGMENTS: Record<number, MeetingSegmentView[]> = {
  1: [
    {
      id: 101,
      meeting_id: 1,
      channel: "me",
      start_ms: 1000,
      end_ms: 3000,
      text: "Alpha line one.",
    },
  ],
  2: [
    {
      id: 201,
      meeting_id: 2,
      channel: "me",
      start_ms: 1000,
      end_ms: 3000,
      text: "Beta line one.",
    },
    {
      id: 202,
      meeting_id: 2,
      channel: "others",
      start_ms: 4000,
      end_ms: 6000,
      text: "Beta line two.",
    },
  ],
};
const IDLE_STATUS: MeetingStatus = {
  state: "idle",
  elapsed_secs: 0,
  attendees: [],
  meeting_id: null,
  mic_name: null,
  warnings: [],
};
const makeSnap = (meeting_id: number): MeetingNotesSnapshot => ({
  meeting_id,
  state: "ready",
  lines: [],
  left_out: [],
  notepad_text: "",
  progress_step: 0,
  progress_total: 0,
  error: null,
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
mock.module("../src/i18n", () => ({
  default: i18nInst,
  SUPPORTED_LANGUAGES: [{ code: "en", name: "English", nativeName: "English" }],
  syncLanguageFromSettings: async () => {},
  getLanguageDirection: () => "ltr",
  isRTLLanguage: () => false,
}));
mock.module("../src/components/whats-new/releaseNotes", () => ({
  findReleaseNoteToShow: () => null,
  findLatestReleaseNote: () => null,
}));
mock.module("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string) =>
    cmd === "list_meetings" ? [MEETING_ALPHA, MEETING_BETA] : [],
  addPluginListener: async () => ({ unregister: async () => {} }),
  transformCallback: () => 0,
  Channel: class {},
  Resource: class {},
  SERIALIZE_TO_IPC_FN: "__TAURI_TO_IPC_KEY__",
  convertFileSrc: (s: string) => s,
}));
mock.module("@tauri-apps/api/event", () => ({
  listen: async () => () => {},
  emit: async () => {},
}));
mock.module("@tauri-apps/plugin-dialog", () => ({
  save: async () => null,
  open: async () => null,
  ask: async () => false,
  confirm: async () => false,
  message: async () => {},
}));
mock.module("../src/components/meetings/gate", () => ({
  SPEAKER_SPLIT_ENABLED: false,
  useMeetingsUi: () => null,
  useSpeakerSplitSection: () => null,
  buildSpeakerNameMapFromSlots: () => ({}),
}));
mock.module("../src/bindings.meetings", () => ({
  commands: {
    meetingStatus: async () => IDLE_STATUS,
    meetingNotesSnapshot: async (id: number) => ({
      status: "ok" as const,
      data: makeSnap(id),
    }),
    meetingSegments: async (id: number) => ({
      status: "ok" as const,
      data: SEGMENTS[id] ?? [],
    }),
    listMeetingSpeakers: async () => ({ status: "ok" as const, data: [] }),
    listMeetingFolders: async () => ({ status: "ok" as const, data: [] }),
    listMeetingTodos: async () => ({ status: "ok" as const, data: [] }),
    listTrashedMeetingsV2: async () => ({ status: "ok" as const, data: [] }),
    meetingRetentionState: async () => ({ transcript_delete_after_days: null }),
    saveMeetingNotepad: async () => ({ status: "ok" as const, data: null }),
    askMeetingsV2: async () => ({
      status: "ok" as const,
      data: {
        answer: "We agreed to ship the release candidate.",
        citations: [
          {
            meeting_id: MEETING_BETA.id,
            meeting_title: MEETING_BETA.title,
            date: "Tue 27 Oct",
            at_ms: 4000,
            segment_id: 202,
            label: "Product sync · 00:04",
          },
        ],
      },
    }),
    startAskByVoice: async () => ({ status: "ok" as const, data: null }),
    finishAskByVoice: async () => ({ status: "ok" as const, data: "" }),
    cancelAskByVoice: async () => ({ status: "ok" as const, data: null }),
  },
}));

const { renderShellSection } =
  await import("../src/components/layout/shellRoutes");
const { MeetingsList } =
  await import("../src/components/meetings/MeetingsList");

type Elem = React.ReactElement<Record<string, unknown>>;
type Inst = {
  type: unknown;
  states: unknown[];
  refs: Array<{ current: unknown }>;
  cbs: Array<{ fn: unknown; deps?: readonly unknown[] }>;
  memos: Array<{ val: unknown; deps?: readonly unknown[] }>;
  effects: Array<{ deps?: readonly unknown[]; cleanup?: void | (() => void) }>;
};

const depsDiff = (a?: readonly unknown[], b?: readonly unknown[]) =>
  !a || !b || a.length !== b.length || b.some((v, i) => !Object.is(a[i], v));

function mountTree(rootFactory: () => React.ReactNode) {
  const map = new Map<string, Inst>();
  const r18 = (
    React as unknown as {
      __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED?: {
        ReactCurrentDispatcher: { current: unknown };
      };
    }
  ).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const orig = r18?.ReactCurrentDispatcher.current;
  let tree: React.ReactNode = null;

  const rerender = (): React.ReactNode => {
    const alive = new Set<string>();
    const pending: Array<{
      inst: Inst;
      idx: number;
      fn: () => void | (() => void);
      deps?: readonly unknown[];
    }> = [];
    const walk = (node: React.ReactNode, path: string): React.ReactNode => {
      if (node == null || typeof node !== "object") return node;
      if (Array.isArray(node))
        return node.map((c, i) => walk(c, `${path}[${i}]`));
      if (!("type" in node)) return node;
      const el = node as Elem;
      if (el.type === React.Fragment)
        return walk(el.props.children as React.ReactNode, `${path}.F`);
      if (typeof el.type === "function") {
        const key = `${path}/${(el.type as { name?: string }).name || "Fn"}${el.key != null ? `#${String(el.key)}` : ""}`;
        alive.add(key);
        let inst = map.get(key);
        if (!inst || inst.type !== el.type) {
          inst?.effects.forEach((e) => e?.cleanup?.());
          inst = {
            type: el.type,
            states: [],
            refs: [],
            cbs: [],
            memos: [],
            effects: [],
          };
          map.set(key, inst);
        }
        let sc = 0,
          rc = 0,
          cc = 0,
          mc = 0,
          ec = 0;
        if (r18) {
          r18.ReactCurrentDispatcher.current = {
            useState: <T,>(init: T | (() => T)) => {
              const i = sc++;
              if (i >= inst!.states.length)
                inst!.states[i] =
                  typeof init === "function" ? (init as () => T)() : init;
              return [
                inst!.states[i] as T,
                (n: T | ((p: T) => T)) => {
                  inst!.states[i] =
                    typeof n === "function"
                      ? (n as (p: T) => T)(inst!.states[i] as T)
                      : n;
                },
              ];
            },
            useEffect: (
              fn: () => void | (() => void),
              deps?: readonly unknown[],
            ) => {
              const i = ec++;
              if (!inst!.effects[i] || depsDiff(inst!.effects[i].deps, deps))
                pending.push({ inst: inst!, idx: i, fn, deps });
            },
            useLayoutEffect() {},
            useRef: <T,>(init: T) => (
              (inst!.refs[rc] ??= { current: init }),
              inst!.refs[rc++]
            ),
            useCallback: <T,>(fn: T, deps?: readonly unknown[]) => {
              const i = cc++;
              if (!inst!.cbs[i] || depsDiff(inst!.cbs[i].deps, deps))
                inst!.cbs[i] = { fn, deps };
              return inst!.cbs[i].fn as T;
            },
            useMemo: <T,>(fn: () => T, deps?: readonly unknown[]) => {
              const i = mc++;
              if (!inst!.memos[i] || depsDiff(inst!.memos[i].deps, deps))
                inst!.memos[i] = { val: fn(), deps };
              return inst!.memos[i].val as T;
            },
            useContext: (ctx: { _currentValue?: unknown }) => ctx._currentValue,
            useSyncExternalStore: <T,>(_s: unknown, snap: () => T) => snap(),
            useId: () => `id-${key}`,
            useTransition: () => [false, (fn: () => void) => fn()],
            useDebugValue() {},
            useDeferredValue: <T,>(v: T) => v,
            useInsertionEffect() {},
            useImperativeHandle() {},
          };
        }
        let out: React.ReactNode;
        try {
          out = (el.type as (p: Record<string, unknown>) => React.ReactNode)(
            el.props,
          );
        } finally {
          if (r18) r18.ReactCurrentDispatcher.current = orig;
        }
        return walk(out, key);
      }
      return el.props && "children" in el.props
        ? React.cloneElement(
            el,
            undefined,
            walk(
              el.props.children as React.ReactNode,
              `${path}.${String(el.type)}`,
            ),
          )
        : el;
    };
    tree = walk(rootFactory(), "root");
    for (const [k, inst] of [...map.entries()]) {
      if (!alive.has(k)) {
        inst.effects.forEach((e) => e?.cleanup?.());
        map.delete(k);
      }
    }
    for (const eff of pending) {
      eff.inst.effects[eff.idx]?.cleanup?.();
      eff.inst.effects[eff.idx] = { deps: eff.deps, cleanup: eff.fn() };
    }
    return tree;
  };

  return {
    rerender,
    getTree: () => tree,
    flush: async () => {
      for (let i = 0; i < 6; i += 1) {
        rerender();
        await new Promise((r) => setTimeout(r, 15));
      }
      return rerender();
    },
    unmount: () => {
      for (const inst of map.values())
        inst.effects.forEach((e) => e?.cleanup?.());
      map.clear();
    },
  };
}

const collect = (node: React.ReactNode, out: Elem[] = []): Elem[] => {
  if (node == null || typeof node !== "object") return out;
  if (Array.isArray(node)) {
    node.forEach((c) => collect(c, out));
    return out;
  }
  if ("type" in node) {
    const el = node as Elem;
    out.push(el);
    if (el.props && "children" in el.props)
      collect(el.props.children as React.ReactNode, out);
  }
  return out;
};
const textOf = (node: React.ReactNode): string =>
  node == null || typeof node === "boolean"
    ? ""
    : typeof node === "string" || typeof node === "number"
      ? String(node)
      : Array.isArray(node)
        ? node.map(textOf).join("")
        : typeof node === "object" && "props" in node
          ? textOf((node as Elem).props?.children as React.ReactNode)
          : "";

function stubDom() {
  const g = globalThis as Record<string, unknown>;
  const hadDoc = "document" in g,
    prevDoc = g.document;
  const hadWin = "window" in g,
    prevWin = g.window;
  if (!hadDoc)
    g.document = { addEventListener: () => {}, removeEventListener: () => {} };
  if (!hadWin)
    g.window = {
      setTimeout: globalThis.setTimeout,
      clearTimeout: globalThis.clearTimeout,
      addEventListener: () => {},
      removeEventListener: () => {},
    };
  return () => {
    if (!hadDoc) delete g.document;
    else g.document = prevDoc;
    if (!hadWin) delete g.window;
    else g.window = prevWin;
  };
}

async function submitAsk(app: ReturnType<typeof mountTree>) {
  const ta = collect(app.getTree()).find(
    (el) => el.type === "textarea" && typeof el.props.onKeyDown === "function",
  );
  expect(ta).toBeDefined();
  (ta!.props.onChange as (e: { target: { value: string } }) => void)({
    target: { value: "What was decided?" },
  });
  app.rerender();
  const ta2 = collect(app.getTree()).find(
    (el) => el.type === "textarea" && typeof el.props.onKeyDown === "function",
  );
  (
    ta2!.props.onKeyDown as (e: {
      key: string;
      shiftKey: boolean;
      preventDefault: () => void;
    }) => void
  )({
    key: "Enter",
    shiftKey: false,
    preventDefault: () => {},
  });
  await app.flush();
}

describe("Standalone Ask citation click opens the cited meeting", () => {
  test("clicking a citation chip on meetings-ask opens that meeting in Meeting history at the cited timestamp", async () => {
    const restore = stubDom();
    const navigated: string[] = [];
    const Shell: React.FC = () => {
      const [section, setSection] = React.useState<
        "meetings-ask" | "meetings-history"
      >("meetings-ask");
      return (
        <>
          {renderShellSection(section, null, {
            onNavigate: (next) => {
              navigated.push(next);
              if (next === "meetings-history" || next === "meetings-ask")
                setSection(next);
            },
          })}
        </>
      );
    };
    const app = mountTree(() => <Shell />);
    try {
      await app.flush();
      await submitAsk(app);
      const chip = collect(app.getTree()).find(
        (el) =>
          el.type === "button" &&
          typeof el.props.onClick === "function" &&
          textOf(el).includes("Product sync") &&
          textOf(el).includes("00:04"),
      );
      expect(chip).toBeDefined();
      (chip!.props.onClick as () => void)();
      await app.flush();

      expect(navigated).toEqual(["meetings-history"]);
      const html = renderToStaticMarkup(app.getTree() as React.ReactElement);
      expect(html).toContain("Product sync");
      expect(html).toContain("Beta line two.");
      expect(html).not.toContain("Alpha line one.");
      const seg = collect(app.getTree()).find(
        (el) =>
          el.type === "li" &&
          el.props["data-segment-key"] === "202" &&
          el.props["data-start-ms"] === 4000,
      );
      expect(seg?.props["data-highlighted"]).toBe("true");
    } finally {
      app.unmount();
      restore();
    }
  });

  test("meeting-scoped Ask citation click inside MeetingsList keeps highlighting the cited timestamp", async () => {
    const restore = stubDom();
    const app = mountTree(() => (
      <MeetingsList initialItems={[MEETING_ALPHA, MEETING_BETA]} />
    ));
    try {
      await app.flush();
      const row = collect(app.getTree()).find(
        (el) =>
          el.type === "div" &&
          typeof el.props.onClick === "function" &&
          textOf(el).includes("Product sync"),
      );
      expect(row).toBeDefined();
      (row!.props.onClick as () => void)();
      await app.flush();
      await submitAsk(app);
      const chip = collect(app.getTree()).find(
        (el) =>
          el.type === "button" &&
          typeof el.props.onClick === "function" &&
          textOf(el).includes("00:04"),
      );
      expect(chip).toBeDefined();
      (chip!.props.onClick as () => void)();
      await app.flush();
      const seg = collect(app.getTree()).find(
        (el) =>
          el.type === "li" &&
          el.props["data-segment-key"] === "202" &&
          el.props["data-start-ms"] === 4000,
      );
      expect(seg?.props["data-highlighted"]).toBe("true");
    } finally {
      app.unmount();
      restore();
    }
  });
});
