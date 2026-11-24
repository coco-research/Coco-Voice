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
  listen: async () => () => {},
  emit: async () => {},
}));

mock.module("@tauri-apps/plugin-dialog", () => ({
  save: async () => null,
  open: async () => null,
}));

mock.module("../src/components/meetings/gate", () => ({
  SPEAKER_SPLIT_ENABLED: false,
  useMeetingsUi: () => null,
  useSpeakerSplitSection: () => null,
  buildSpeakerNameMapFromSlots: () => ({}),
}));

const MEETING_A: MeetingListItem = {
  id: 1,
  title: "Meeting Alpha",
  started_at: 1_700_000_000,
  ended_at: 1_700_001_800,
  notes_ready: true,
};

const MEETING_B: MeetingListItem = {
  id: 2,
  title: "Meeting Beta",
  started_at: 1_700_003_600,
  ended_at: 1_700_005_400,
  notes_ready: true,
};

const SEGMENTS_BY_MEETING: Record<number, MeetingSegmentView[]> = {
  1: [
    {
      id: 101,
      meeting_id: 1,
      channel: "me",
      start_ms: 1000,
      end_ms: 3000,
      text: "Alpha line one.",
    },
    {
      id: 102,
      meeting_id: 1,
      channel: "others",
      start_ms: 4000,
      end_ms: 6000,
      text: "Alpha line two.",
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

function makeSnapshot(meetingId: number): MeetingNotesSnapshot {
  return {
    meeting_id: meetingId,
    state: "ready",
    lines: [],
    left_out: [],
    notepad_text: "",
    progress_step: 0,
    progress_total: 0,
    error: null,
  };
}

const IDLE_STATUS: MeetingStatus = {
  state: "idle",
  elapsed_secs: 0,
  attendees: [],
  meeting_id: null,
  mic_name: null,
  warnings: [],
};

type DeleteSpanResult =
  { status: "ok"; data: null } | { status: "error"; error: string };

let resolveDeleteSpan: ((res: DeleteSpanResult) => void) | null = null;
const deleteSpanCalls: Array<{
  meetingId: number;
  startMs: number;
  endMs: number;
}> = [];

mock.module("../src/bindings.meetings", () => ({
  commands: {
    meetingStatus: async () => IDLE_STATUS,
    meetingNotesSnapshot: async (meetingId: number) => ({
      status: "ok" as const,
      data: makeSnapshot(meetingId),
    }),
    meetingSegments: async (meetingId: number) => ({
      status: "ok" as const,
      data: SEGMENTS_BY_MEETING[meetingId] ?? [],
    }),
    listMeetingSpeakers: async () => ({ status: "ok" as const, data: [] }),
    listMeetingFolders: async () => ({ status: "ok" as const, data: [] }),
    listMeetingTodos: async () => ({ status: "ok" as const, data: [] }),
    saveMeetingNotepad: async () => ({ status: "ok" as const, data: null }),
    deleteTranscriptSpanSecure: (
      meetingId: number,
      startMs: number,
      endMs: number,
    ) => {
      deleteSpanCalls.push({ meetingId, startMs, endMs });
      return new Promise<DeleteSpanResult>((resolve) => {
        resolveDeleteSpan = resolve;
      });
    },
  },
}));

const { MeetingsList } =
  await import("../src/components/meetings/MeetingsList");

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
        const walked = walk(
          el.props.children as React.ReactNode,
          `${path}.${String(el.type)}`,
        );
        return Array.isArray(walked)
          ? React.cloneElement(el, undefined, ...walked)
          : React.cloneElement(el, undefined, walked);
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

function collectElements(node: React.ReactNode, out: Elem[] = []): Elem[] {
  if (
    node == null ||
    typeof node === "boolean" ||
    typeof node === "string" ||
    typeof node === "number"
  ) {
    return out;
  }
  if (Array.isArray(node)) {
    for (const child of node) collectElements(child, out);
    return out;
  }
  if (typeof node === "object" && "type" in node) {
    const el = node as Elem;
    out.push(el);
    if (el.props && "children" in el.props) {
      collectElements(el.props.children as React.ReactNode, out);
    }
  }
  return out;
}

function textOf(node: React.ReactNode): string {
  if (node == null || typeof node === "boolean") return "";
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textOf).join("");
  if (typeof node === "object" && "props" in node) {
    const props = (node as Elem).props;
    return textOf(props?.children as React.ReactNode);
  }
  return "";
}

describe("MeetingsList part-delete meeting switch guard", () => {
  test("a failed part delete after switching meetings leaves the open meeting's transcript alone", async () => {
    deleteSpanCalls.length = 0;
    resolveDeleteSpan = null;

    const hadDocument = "document" in globalThis;
    const prevDocument = (globalThis as Record<string, unknown>).document;
    if (!hadDocument) {
      (globalThis as Record<string, unknown>).document = {
        addEventListener: () => {},
        removeEventListener: () => {},
      };
    }

    const app = mountAppTree(() => (
      <MeetingsList initialItems={[MEETING_A, MEETING_B]} />
    ));

    try {
      await app.flushAndRerender();

      // 1. Open meeting A and verify A's lines render.
      const rowA = collectElements(app.getTree()).find(
        (el) =>
          el.type === "div" &&
          typeof el.props.onClick === "function" &&
          textOf(el).includes("Meeting Alpha"),
      );
      expect(rowA).toBeDefined();
      (rowA!.props.onClick as () => void)();
      await app.flushAndRerender();

      const htmlA = renderToStaticMarkup(app.getTree() as React.ReactElement);
      expect(htmlA).toContain("Alpha line one.");
      expect(htmlA).toContain("Alpha line two.");

      // 2. Select a part of A and confirm the part delete while holding the command in flight.
      const segA1 = collectElements(app.getTree()).find(
        (el) =>
          el.type === "li" &&
          el.props["data-segment-key"] === "101" &&
          typeof el.props.onClick === "function",
      );
      expect(segA1).toBeDefined();
      (segA1!.props.onClick as (e: { shiftKey: boolean }) => void)({
        shiftKey: false,
      });
      app.rerender();

      const deletePartBtn = collectElements(app.getTree()).find(
        (el) =>
          el.type === "button" &&
          el.props["data-testid"] === "delete-this-part-btn",
      );
      expect(deletePartBtn).toBeDefined();
      (deletePartBtn!.props.onClick as () => void)();
      app.rerender();

      const confirmDialog = collectElements(app.getTree()).find(
        (el) => el.props["data-testid"] === "confirm-delete-part",
      );
      expect(confirmDialog).toBeDefined();
      const confirmDeleteBtn = collectElements(confirmDialog!).find(
        (el) =>
          el.type === "button" && textOf(el).trim() === en.meetings.list.delete,
      );
      expect(confirmDeleteBtn).toBeDefined();
      (confirmDeleteBtn!.props.onClick as () => void)();
      await app.flushAndRerender();

      expect(deleteSpanCalls).toHaveLength(1);
      expect(deleteSpanCalls[0].meetingId).toBe(MEETING_A.id);
      expect(resolveDeleteSpan).not.toBeNull();

      // 3. Switch to meeting B and verify B's lines render.
      const backBtn = collectElements(app.getTree()).find(
        (el) =>
          el.type === "button" &&
          typeof el.props.onClick === "function" &&
          textOf(el).includes("All meetings"),
      );
      expect(backBtn).toBeDefined();
      (backBtn!.props.onClick as () => void)();
      await app.flushAndRerender();

      const rowB = collectElements(app.getTree()).find(
        (el) =>
          el.type === "div" &&
          typeof el.props.onClick === "function" &&
          textOf(el).includes("Meeting Beta"),
      );
      expect(rowB).toBeDefined();
      (rowB!.props.onClick as () => void)();
      await app.flushAndRerender();

      const htmlBBefore = renderToStaticMarkup(
        app.getTree() as React.ReactElement,
      );
      expect(htmlBBefore).toContain("Beta line one.");
      expect(htmlBBefore).toContain("Beta line two.");

      // 4. Resolve the held delete on A as an error.
      resolveDeleteSpan!({ status: "error", error: "delete failed" });
      await app.flushAndRerender();

      // 5. Assert B's rendered lines are exactly B's (none of A's) and the delete error is absent.
      const renderedSegmentRows = collectElements(app.getTree()).filter(
        (el) => el.type === "li" && el.props["data-segment-key"] !== undefined,
      );
      expect(
        renderedSegmentRows.map((el) => el.props["data-segment-key"]),
      ).toEqual(["201", "202"]);

      const htmlBAfter = renderToStaticMarkup(
        app.getTree() as React.ReactElement,
      );
      expect(htmlBAfter).toContain("Beta line one.");
      expect(htmlBAfter).toContain("Beta line two.");
      expect(htmlBAfter).not.toContain("Alpha line one.");
      expect(htmlBAfter).not.toContain("Alpha line two.");
      expect(htmlBAfter).not.toContain("Could not delete this part.");
    } finally {
      app.unmount();
      if (!hadDocument) {
        delete (globalThis as Record<string, unknown>).document;
      } else {
        (globalThis as Record<string, unknown>).document = prevDocument;
      }
    }
  });
});
