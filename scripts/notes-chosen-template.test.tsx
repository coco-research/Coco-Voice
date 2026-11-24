import { beforeEach, describe, expect, mock, test } from "bun:test";
import React from "react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type {
  MeetingFolderView,
  MeetingStatus,
  MeetingTemplateView,
} from "../src/bindings.meetings";

interface InvokeCall {
  cmd: string;
  args: Record<string, unknown> | undefined;
}

const invokeCalls: InvokeCall[] = [];
let mockFolders: MeetingFolderView[] = [];
let mockTemplates: MeetingTemplateView[] = [];

const consentOpenStatus: MeetingStatus = {
  state: "consent_open",
  elapsed_secs: 0,
  meeting_id: null,
  mic_name: null,
  mic_only: false,
  warnings: [],
};

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
    if (cmd === "list_meeting_folders") return mockFolders;
    if (cmd === "list_meeting_templates") return mockTemplates;
    if (cmd === "open_meeting_consent") return consentOpenStatus;
    return null;
  },
  transformCallback: () => 0,
  Channel: class {},
  Resource: class {},
  SERIALIZE_TO_IPC_FN: "__TAURI_TO_IPC_KEY__",
  convertFileSrc: (s: string) => s,
}));

const { FoldersAndTemplatesView } =
  await import("../src/components/meetings/FoldersAndTemplatesView");
const { commands } = await import("../src/bindings.meetings");

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
            `${path}/${String(el.type)}`,
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
      const cleanup = eff.fn();
      eff.inst.effects[eff.idx] = { deps: eff.deps, cleanup };
    }
    return renderedTree;
  };

  const unmount = () => {
    for (const inst of instances.values()) {
      inst.effects.forEach((e) => e?.cleanup?.());
    }
    instances.clear();
  };

  return { rerender, getTree: () => renderedTree, unmount };
}

function textOf(node: React.ReactNode): string {
  if (node == null || typeof node === "boolean") return "";
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textOf).join("");
  if (typeof node === "object" && "props" in node) {
    return textOf((node as Elem).props.children as React.ReactNode);
  }
  return "";
}

function findElements(
  node: React.ReactNode,
  predicate: (el: Elem) => boolean,
): Elem[] {
  const out: Elem[] = [];
  const visit = (cur: React.ReactNode) => {
    if (cur == null || typeof cur !== "object") return;
    if (Array.isArray(cur)) {
      cur.forEach(visit);
      return;
    }
    if ("type" in cur) {
      const el = cur as Elem;
      if (predicate(el)) out.push(el);
      if (el.props && "children" in el.props) {
        visit(el.props.children as React.ReactNode);
      }
    }
  };
  visit(node);
  return out;
}

async function flushMicrotasks() {
  for (let i = 0; i < 6; i++) {
    await Promise.resolve();
  }
}

describe("FoldersAndTemplatesView start with chosen template", () => {
  beforeEach(() => {
    invokeCalls.length = 0;
    mockFolders = [
      {
        id: 3,
        name: "Customers",
        last_template_id: null,
        meetings: [],
      },
    ];
    mockTemplates = [
      {
        id: 11,
        name: "Weekly sync",
        body: "## Updates\n## Blockers\n",
      },
      {
        id: 17,
        name: "Customer call",
        body: "## Their needs\n## Decisions\n## Follow-ups\n",
      },
    ];
  });

  test("clicking Start notes passes the selected template id to open_meeting_consent", async () => {
    const app = mountAppTree(() => <FoldersAndTemplatesView />);
    try {
      app.rerender();
      await flushMicrotasks();
      app.rerender();

      const openDialogBtn = findElements(
        app.getTree(),
        (el) =>
          el.type === "button" &&
          el.props["data-testid"] === "open-note-template-dialog",
      )[0];
      expect(openDialogBtn).toBeDefined();
      (openDialogBtn.props.onClick as () => void)();
      app.rerender();

      const customerTemplateBtn = findElements(
        app.getTree(),
        (el) => el.type === "button" && textOf(el).includes("Customer call"),
      )[0];
      expect(customerTemplateBtn).toBeDefined();
      (customerTemplateBtn.props.onClick as () => void)();
      app.rerender();

      const startBtn = findElements(
        app.getTree(),
        (el) =>
          el.type === "button" &&
          textOf(el).trim() === i18nInst.t("meetings.offer.start"),
      )[0];
      expect(startBtn).toBeDefined();
      (startBtn.props.onClick as () => void)();
      await flushMicrotasks();

      const consentCalls = invokeCalls.filter(
        (c) => c.cmd === "open_meeting_consent",
      );
      expect(consentCalls).toHaveLength(1);
      expect(consentCalls[0].args).toEqual({ templateId: 17 });
    } finally {
      app.unmount();
    }
  });

  test("default openMeetingConsent() without arguments omits templateId", async () => {
    await commands.openMeetingConsent();
    const consentCalls = invokeCalls.filter(
      (c) => c.cmd === "open_meeting_consent",
    );
    expect(consentCalls).toHaveLength(1);
    expect(consentCalls[0].args).toBeUndefined();
  });

  test("clicking Start notes with no templates passes templateId: null rather than 0", async () => {
    mockTemplates = [];
    const app = mountAppTree(() => <FoldersAndTemplatesView />);
    try {
      app.rerender();
      await flushMicrotasks();
      app.rerender();

      const openDialogBtn = findElements(
        app.getTree(),
        (el) =>
          el.type === "button" &&
          el.props["data-testid"] === "open-note-template-dialog",
      )[0];
      expect(openDialogBtn).toBeDefined();
      (openDialogBtn.props.onClick as () => void)();
      app.rerender();

      const startBtn = findElements(
        app.getTree(),
        (el) =>
          el.type === "button" &&
          textOf(el).trim() === i18nInst.t("meetings.offer.start"),
      )[0];
      expect(startBtn).toBeDefined();
      (startBtn.props.onClick as () => void)();
      await flushMicrotasks();

      const consentCalls = invokeCalls.filter(
        (c) => c.cmd === "open_meeting_consent",
      );
      expect(consentCalls).toHaveLength(1);
      expect(consentCalls[0].args).toEqual({ templateId: null });
    } finally {
      app.unmount();
    }
  });
});
