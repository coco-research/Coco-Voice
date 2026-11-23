import { afterEach, describe, expect, mock, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";

const SILENT_WARNING_TEXT =
  "No call audio heard yet. Check that System Audio Recording is allowed for Coco Voice in System Settings.";

type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
let invokeHandler: InvokeFn = async () => null;

const listenHandlers = new Map<string, Array<(ev: { payload: unknown }) => void>>();

mock.module("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) =>
    invokeHandler(cmd, args),
}));

mock.module("@tauri-apps/api/event", () => ({
  listen: async (
    event: string,
    handler: (ev: { payload: unknown }) => void,
  ) => {
    const list = listenHandlers.get(event) ?? [];
    list.push(handler);
    listenHandlers.set(event, list);
    return () => {
      const cur = listenHandlers.get(event);
      if (!cur) return;
      const idx = cur.indexOf(handler);
      if (idx >= 0) cur.splice(idx, 1);
    };
  },
}));

mock.module("@tauri-apps/plugin-dialog", () => ({
  save: async () => null,
  open: async () => null,
}));

const { MeetingsList } = await import("../src/components/meetings/MeetingsList");

function makeTestI18n() {
  const inst = i18next.createInstance();
  void inst.use(initReactI18next).init({
    lng: "en",
    fallbackLng: "en",
    resources: { en: { translation: en } },
    interpolation: { escapeValue: false },
  });
  return inst;
}

type HookSlot = {
  kind: "state" | "ref" | "effect" | "memo" | "callback";
  value?: unknown;
  deps?: unknown[];
  cleanup?: (() => void) | void;
  pendingEffect?: () => (() => void) | void;
};

type ComponentNode = {
  type: (...args: unknown[]) => React.ReactNode;
  hooks: HookSlot[];
  rendered: React.ReactNode;
};

function depsChanged(prev?: unknown[], next?: unknown[]): boolean {
  if (!prev || !next) return true;
  if (prev.length !== next.length) return true;
  for (let i = 0; i < prev.length; i++) {
    if (!Object.is(prev[i], next[i])) return true;
  }
  return false;
}

const g = globalThis as unknown as {
  window?: Record<string, unknown>;
  document?: Record<string, unknown>;
};

function mountInteractiveTree(rootElement: React.ReactElement) {
  const nodes = new Map<string, ComponentNode>();
  const pendingEffects: Array<{ node: ComponentNode; slot: HookSlot }> = [];
  let mounted = true;
  let dirty = false;
  let lastTree: React.ReactNode = null;

  const reactInternals = (
    React as unknown as {
      __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED?: {
        ReactCurrentDispatcher: { current: unknown };
      };
    }
  ).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;

  if (!reactInternals) {
    throw new Error("React internals dispatcher unavailable");
  }

  function renderNode(
    node: React.ReactNode,
    pathKey: string,
    seen: Set<string>,
  ): React.ReactNode {
    if (
      node === null ||
      node === undefined ||
      typeof node === "boolean" ||
      typeof node === "string" ||
      typeof node === "number"
    ) {
      return node;
    }
    if (Array.isArray(node)) {
      return node.map((child, i) => {
        const k =
          React.isValidElement(child) && child.key !== null
            ? String(child.key)
            : String(i);
        return renderNode(child, `${pathKey}[${k}]`, seen);
      });
    }
    if (!React.isValidElement(node)) {
      return node;
    }
    const { type, props } = node;
    if (type === React.Fragment) {
      return renderNode(
        (props as { children?: React.ReactNode }).children,
        `${pathKey}/frag`,
        seen,
      );
    }
    if (typeof type === "string") {
      const p = props as Record<string, unknown>;
      if (p.children === undefined) return node;
      const nextChildren = renderNode(
        p.children as React.ReactNode,
        `${pathKey}/${type}`,
        seen,
      );
      return React.cloneElement(node, undefined, nextChildren);
    }
    if (typeof type === "function") {
      const fn = type as (...args: unknown[]) => React.ReactNode;
      const keySeg = node.key !== null ? `#${String(node.key)}` : "";
      const fnName = (fn as { displayName?: string; name?: string }).displayName || fn.name || "anon";
      const nodeKey = `${pathKey}/${fnName}${keySeg}`;
      seen.add(nodeKey);
      let stateNode = nodes.get(nodeKey);
      if (!stateNode || stateNode.type !== fn) {
        if (stateNode) {
          for (const slot of stateNode.hooks) {
            if (slot.kind === "effect" && typeof slot.cleanup === "function") {
              slot.cleanup();
            }
          }
        }
        stateNode = { type: fn, hooks: [], rendered: null };
        nodes.set(nodeKey, stateNode);
      }

      const activeNode = stateNode;
      let hookIdx = 0;

      const dispatcher = {
        useState<T>(init: T | (() => T)): [T, (next: T | ((p: T) => T)) => void] {
          const i = hookIdx++;
          let slot = activeNode.hooks[i];
          if (!slot) {
            const initial =
              typeof init === "function" ? (init as () => T)() : init;
            slot = { kind: "state", value: initial };
            activeNode.hooks[i] = slot;
          }
          const setter = (next: T | ((p: T) => T)) => {
            const prev = slot!.value as T;
            const resolved =
              typeof next === "function"
                ? (next as (p: T) => T)(prev)
                : next;
            if (!Object.is(prev, resolved)) {
              slot!.value = resolved;
              dirty = true;
            }
          };
          return [slot.value as T, setter];
        },
        useReducer<S, A>(
          reducer: (s: S, a: A) => S,
          init: S,
        ): [S, (a: A) => void] {
          const i = hookIdx++;
          let slot = activeNode.hooks[i];
          if (!slot) {
            slot = { kind: "state", value: init };
            activeNode.hooks[i] = slot;
          }
          return [
            slot.value as S,
            (action: A) => {
              const next = reducer(slot!.value as S, action);
              if (!Object.is(slot!.value, next)) {
                slot!.value = next;
                dirty = true;
              }
            },
          ];
        },
        useRef<T>(init: T) {
          const i = hookIdx++;
          let slot = activeNode.hooks[i];
          if (!slot) {
            slot = { kind: "ref", value: { current: init } };
            activeNode.hooks[i] = slot;
          }
          return slot.value as { current: T };
        },
        useEffect(cb: () => (() => void) | void, deps?: unknown[]) {
          const i = hookIdx++;
          let slot = activeNode.hooks[i];
          if (!slot) {
            slot = { kind: "effect", deps: undefined, pendingEffect: cb };
            activeNode.hooks[i] = slot;
            pendingEffects.push({ node: activeNode, slot });
          } else if (depsChanged(slot.deps, deps)) {
            slot.pendingEffect = cb;
            slot.deps = deps;
            pendingEffects.push({ node: activeNode, slot });
          }
          if (slot.deps === undefined) {
            slot.deps = deps;
          }
        },
        useLayoutEffect(cb: () => (() => void) | void, deps?: unknown[]) {
          this.useEffect(cb, deps);
        },
        useMemo<T>(factory: () => T, deps?: unknown[]): T {
          const i = hookIdx++;
          let slot = activeNode.hooks[i];
          if (!slot || depsChanged(slot.deps, deps)) {
            slot = { kind: "memo", value: factory(), deps };
            activeNode.hooks[i] = slot;
          }
          return slot.value as T;
        },
        useCallback<T>(fnArg: T, deps?: unknown[]): T {
          const i = hookIdx++;
          let slot = activeNode.hooks[i];
          if (!slot || depsChanged(slot.deps, deps)) {
            slot = { kind: "callback", value: fnArg, deps };
            activeNode.hooks[i] = slot;
          }
          return slot.value as T;
        },
        useContext<T>(ctx: React.Context<T>): T {
          const current = (ctx as unknown as { _currentValue: T })._currentValue;
          return current;
        },
        useId() {
          const i = hookIdx++;
          return `:r${i}:`;
        },
        useSyncExternalStore<T>(
          _sub: (cb: () => void) => () => void,
          getSnap: () => T,
        ): T {
          return getSnap();
        },
      };

      const prevDisp = reactInternals.ReactCurrentDispatcher.current;
      reactInternals.ReactCurrentDispatcher.current = dispatcher;
      let out: React.ReactNode;
      try {
        out = fn(props);
      } finally {
        reactInternals.ReactCurrentDispatcher.current = prevDisp;
      }
      stateNode.rendered = out;
      return renderNode(out, nodeKey, seen);
    }
    // Context provider / forwardRef / memo
    const objType = type as unknown as {
      $$typeof?: symbol;
      _context?: { _currentValue: unknown };
      render?: (...args: unknown[]) => React.ReactNode;
      type?: unknown;
    };
    if (objType._context) {
      const ctx = objType._context;
      const prevVal = ctx._currentValue;
      ctx._currentValue = (props as { value: unknown }).value;
      try {
        return renderNode(
          (props as { children?: React.ReactNode }).children,
          `${pathKey}/ctx`,
          seen,
        );
      } finally {
        ctx._currentValue = prevVal;
      }
    }
    if (typeof objType.render === "function") {
      return renderNode(objType.render(props, null), `${pathKey}/fwd`, seen);
    }
    if (objType.type) {
      return renderNode(
        React.createElement(
          objType.type as React.ElementType,
          props as Record<string, unknown>,
        ),
        `${pathKey}/memo`,
        seen,
      );
    }
    return node;
  }

  function flushRender(): void {
    if (!mounted) return;
    for (let pass = 0; pass < 12; pass++) {
      dirty = false;
      const seen = new Set<string>();
      lastTree = renderNode(rootElement, "root", seen);
      for (const [k, n] of [...nodes.entries()]) {
        if (!seen.has(k)) {
          for (const slot of n.hooks) {
            if (slot.kind === "effect" && typeof slot.cleanup === "function") {
              slot.cleanup();
            }
          }
          nodes.delete(k);
        }
      }
      while (pendingEffects.length > 0) {
        const eff = pendingEffects.shift()!;
        if (typeof eff.slot.cleanup === "function") {
          eff.slot.cleanup();
          eff.slot.cleanup = undefined;
        }
        const fn = eff.slot.pendingEffect;
        eff.slot.pendingEffect = undefined;
        if (fn) {
          eff.slot.cleanup = fn();
        }
      }
      if (!dirty) break;
    }
  }

  function findElement(
    node: React.ReactNode,
    pred: (el: React.ReactElement) => boolean,
  ): React.ReactElement | null {
    if (!node || typeof node !== "object") return null;
    if (Array.isArray(node)) {
      for (const c of node) {
        const hit = findElement(c, pred);
        if (hit) return hit;
      }
      return null;
    }
    if (React.isValidElement(node)) {
      if (pred(node)) return node;
      return findElement(
        (node.props as { children?: React.ReactNode }).children,
        pred,
      );
    }
    return null;
  }

  function textOf(node: React.ReactNode): string {
    if (typeof node === "string" || typeof node === "number") return String(node);
    if (Array.isArray(node)) return node.map(textOf).join("");
    if (React.isValidElement(node)) {
      return textOf((node.props as { children?: React.ReactNode }).children);
    }
    return "";
  }

  flushRender();

  return {
    async settle(ticks = 5) {
      for (let i = 0; i < ticks; i++) {
        await new Promise((r) => setTimeout(r, 10));
        flushRender();
      }
    },
    getHtml(): string {
      return renderToStaticMarkup(<>{lastTree}</>).replaceAll("&#x27;", "'");
    },
    clickRowWithTitle(title: string) {
      const hit = findElement(lastTree, (el) => {
        const p = el.props as { onClick?: () => void; className?: string };
        return (
          el.type === "div" &&
          typeof p.onClick === "function" &&
          textOf(el).includes(title)
        );
      });
      if (!hit) {
        throw new Error(`Could not find clickable meeting row with title "${title}"`);
      }
      (hit.props as { onClick: () => void }).onClick();
      flushRender();
    },
    clickBack() {
      const hit = findElement(lastTree, (el) => {
        const p = el.props as { onClick?: () => void };
        return (
          el.type === "button" &&
          typeof p.onClick === "function" &&
          textOf(el).includes("All meetings")
        );
      });
      if (!hit) {
        throw new Error("Could not find back button to All meetings");
      }
      (hit.props as { onClick: () => void }).onClick();
      flushRender();
    },
    unmount() {
      mounted = false;
      for (const n of nodes.values()) {
        for (const slot of n.hooks) {
          if (slot.kind === "effect" && typeof slot.cleanup === "function") {
            slot.cleanup();
          }
        }
      }
      nodes.clear();
    },
  };
}

afterEach(() => {
  listenHandlers.clear();
});

describe("X3k saved system_audio_silent warning (AC-B19p)", () => {
  test("AC-B19p: reopening a meeting from history through openMeetingDetail shows the amber no-call-audio line iff system_audio_silent is true", async () => {
    const prevWindow = g.window;
    const prevDocument = g.document;
    g.window = {
      setTimeout: globalThis.setTimeout.bind(globalThis),
      clearTimeout: globalThis.clearTimeout.bind(globalThis),
    };
    g.document = {
      addEventListener: () => {},
      removeEventListener: () => {},
    };

    const items = [
      {
        id: 71,
        title: "Silent call sync",
        started_at: 1_760_100_000,
        ended_at: 1_760_100_600,
        notes_ready: false,
        attendees: [],
        system_audio_silent: true,
      },
      {
        id: 72,
        title: "Heard call sync",
        started_at: 1_760_101_000,
        ended_at: 1_760_101_600,
        notes_ready: false,
        attendees: [],
        system_audio_silent: false,
      },
      {
        id: 73,
        title: "Legacy row without key",
        started_at: 1_760_102_000,
        ended_at: 1_760_102_600,
        notes_ready: false,
        attendees: [],
      },
    ];

    const detailSegmentsById: Record<number, unknown[]> = {
      71: [
        {
          id: 1,
          meeting_id: 71,
          channel: "me",
          start_ms: 1000,
          end_ms: 4000,
          text: "I spoke while the Mac was silent.",
        },
      ],
      72: [
        {
          id: 2,
          meeting_id: 72,
          channel: "others",
          start_ms: 1000,
          end_ms: 4000,
          text: "Call audio came through fine.",
        },
      ],
      73: [
        {
          id: 3,
          meeting_id: 73,
          channel: "me",
          start_ms: 1000,
          end_ms: 4000,
          text: "Older saved meeting line.",
        },
      ],
    };

    invokeHandler = async (cmd, args = {}) => {
      switch (cmd) {
        case "list_meetings":
          return items;
        case "meeting_status":
          return {
            state: "idle",
            elapsed_secs: 0,
            meeting_id: null,
            mic_name: null,
            warnings: [],
          };
        case "meeting_notes_snapshot":
          return {
            meeting_id: args.meetingId ?? args.id ?? 0,
            state: "idle",
            progress_label: null,
            elapsed_secs: 0,
            notepad_text: "",
            lines: [],
            oversized_count: 0,
          };
        case "meeting_segments": {
          const mid = Number(args.meetingId ?? args.id ?? 0);
          return detailSegmentsById[mid] ?? [];
        }
        case "list_meeting_speakers":
          return [];
        case "list_meeting_folders":
          return [];
        case "list_meeting_templates":
          return [];
        default:
          return null;
      }
    };

    const i18n = makeTestI18n();
    const mounted = mountInteractiveTree(
      <I18nextProvider i18n={i18n}>
        <MeetingsList />
      </I18nextProvider>,
    );

    try {
      await mounted.settle(6);
      const listHtml = mounted.getHtml();
      expect(listHtml).toContain("Silent call sync");
      expect(listHtml).toContain("Heard call sync");
      expect(listHtml).toContain("Legacy row without key");

      // 1. Click meeting with system_audio_silent: true -> real openMeetingDetail path.
      mounted.clickRowWithTitle("Silent call sync");
      await mounted.settle(6);
      const silentDetailHtml = mounted.getHtml();
      expect(silentDetailHtml).toContain("I spoke while the Mac was silent.");
      expect(silentDetailHtml).toContain(SILENT_WARNING_TEXT);
      expect(silentDetailHtml).toContain("border-amber-500/30 bg-amber-500/10");

      // 2. Back to list -> click meeting with system_audio_silent: false -> NO amber line.
      mounted.clickBack();
      await mounted.settle(4);
      mounted.clickRowWithTitle("Heard call sync");
      await mounted.settle(6);
      const heardDetailHtml = mounted.getHtml();
      expect(heardDetailHtml).toContain("Call audio came through fine.");
      expect(heardDetailHtml).not.toContain(SILENT_WARNING_TEXT);

      // 3. Back to list -> click legacy meeting with no system_audio_silent key -> NO amber line.
      mounted.clickBack();
      await mounted.settle(4);
      mounted.clickRowWithTitle("Legacy row without key");
      await mounted.settle(6);
      const legacyDetailHtml = mounted.getHtml();
      expect(legacyDetailHtml).toContain("Older saved meeting line.");
      expect(legacyDetailHtml).not.toContain(SILENT_WARNING_TEXT);
    } finally {
      mounted.unmount();
      if (prevWindow === undefined) {
        delete g.window;
      } else {
        g.window = prevWindow;
      }
      if (prevDocument === undefined) {
        delete g.document;
      } else {
        g.document = prevDocument;
      }
    }
  });
});
