import { beforeEach, describe, expect, mock, test } from "bun:test";
import React from "react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { ModelInfo } from "../src/bindings";
import { FIRST_RUN_DEFAULT_MODEL_ID } from "../src/components/onboarding/FirstRunModelDownloadView";

type EventHandler = (event: { payload: unknown }) => void;

const listeners = new Map<string, Set<EventHandler>>();
const downloadModelCalls: string[] = [];
const cancelDownloadCalls: string[] = [];
const deleteModelCalls: string[] = [];
let downloadBehavior: "resolve-ok" | "pending" = "resolve-ok";

function emitTauriEvent(name: string, payload: unknown) {
  const set = listeners.get(name);
  if (!set) return;
  for (const fn of [...set]) {
    fn({ payload });
  }
}

mock.module("@tauri-apps/api/event", () => ({
  listen: async (name: string, handler: EventHandler) => {
    let set = listeners.get(name);
    if (!set) {
      set = new Set();
      listeners.set(name, set);
    }
    set.add(handler);
    return () => {
      listeners.get(name)?.delete(handler);
    };
  },
  emit: async () => {},
}));

const defaultModel: ModelInfo = {
  id: FIRST_RUN_DEFAULT_MODEL_ID,
  name: "Parakeet V3",
  description: "Fast multilingual speech model",
  filename: "parakeet-tdt-0.6b-v3-int8",
  source: {
    HuggingFace: {
      repo_id: "org/parakeet",
      revision: "main",
      sha256: "abc",
    },
  },
  size_mb: 478,
  is_downloaded: false,
  is_downloading: false,
  partial_size: 0,
  is_directory: true,
  engine_type: "Parakeet",
  accuracy_score: 0.9,
  speed_score: 0.9,
  supports_translation: false,
  is_recommended: true,
  measured_best: null,
  supported_languages: ["en"],
  supports_language_selection: false,
  is_custom: false,
  supports_streaming: false,
  supports_language_detection: false,
};

mock.module("@/bindings", () => ({
  commands: {
    getAvailableModels: async () => ({ status: "ok", data: [defaultModel] }),
    getCurrentModel: async () => ({ status: "ok", data: "" }),
    hasAnyModelsAvailable: async () => ({ status: "ok", data: false }),
    downloadModel: (modelId: string) => {
      downloadModelCalls.push(modelId);
      if (downloadBehavior === "pending") {
        return new Promise<{ status: "ok"; data: null }>(() => {});
      }
      return Promise.resolve({ status: "ok" as const, data: null });
    },
    cancelDownload: async (modelId: string) => {
      cancelDownloadCalls.push(modelId);
      return { status: "ok", data: null };
    },
    deleteModel: async (modelId: string) => {
      deleteModelCalls.push(modelId);
      return { status: "ok", data: null };
    },
    setActiveModel: async () => ({ status: "ok", data: null }),
    isRecording: async () => false,
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

mock.module("sonner", () => ({
  toast: {
    error: () => {},
    success: () => {},
    info: () => {},
  },
}));

const { useModelStore } = await import("../src/stores/modelStore");
const { default: Onboarding } =
  await import("../src/components/onboarding/Onboarding");

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

  const flushAndRerender = async (rounds = 4) => {
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

function collectElements(
  node: React.ReactNode,
  pred: (el: Elem) => boolean,
  acc: Elem[] = [],
): Elem[] {
  if (node == null || typeof node !== "object") return acc;
  if (Array.isArray(node)) {
    for (const child of node) collectElements(child, pred, acc);
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
    const props = (node as Elem).props;
    return props && "children" in props
      ? textContent(props.children as React.ReactNode)
      : "";
  }
  return "";
}

describe("AC-A5 & AC-A6 real Onboarding component flows", () => {
  beforeEach(async () => {
    listeners.clear();
    downloadModelCalls.length = 0;
    cancelDownloadCalls.length = 0;
    deleteModelCalls.length = 0;
    downloadBehavior = "pending";
    useModelStore.setState({
      models: [defaultModel],
      currentModel: "",
      downloadingModels: {},
      verifyingModels: {},
      extractingModels: {},
      downloadProgress: {},
      downloadStats: {},
      downloadFailures: {},
      loading: false,
      error: null,
      initialized: false,
      isRescanning: false,
      isRecording: false,
      suppressFailureToast: false,
    });
    await useModelStore.getState().initialize();
  });

  test("AC-A5: model-download-failed (network) after 312,000,000 bytes renders exact message and Try again calls downloadModel once", async () => {
    const mounted = mountComponent(() =>
      React.createElement(Onboarding, {
        onModelSelected: () => {},
      }),
    );

    try {
      await mounted.flushAndRerender(4);
      // Initial auto-start invoked downloadModel once.
      expect(downloadModelCalls).toEqual([FIRST_RUN_DEFAULT_MODEL_ID]);

      // Emit progress at 312,000,000 bytes of 478,000,000 bytes.
      emitTauriEvent("model-download-progress", {
        model_id: FIRST_RUN_DEFAULT_MODEL_ID,
        downloaded: 312_000_000,
        total: 478_000_000,
        percentage: 65.27,
      });
      await mounted.flushAndRerender(4);

      // Emit network failure event.
      emitTauriEvent("model-download-failed", {
        model_id: FIRST_RUN_DEFAULT_MODEL_ID,
        error: "connection reset",
        cause: "network",
      });
      await mounted.flushAndRerender(6);

      // Partial cleanup ran before showing the failed alert.
      expect(deleteModelCalls).toEqual([FIRST_RUN_DEFAULT_MODEL_ID]);

      const failedAlerts = collectElements(
        mounted.getTree(),
        (el) => el.props?.["data-testid"] === "first-run-download-failed",
      );
      expect(failedAlerts).toHaveLength(1);
      expect(textContent(failedAlerts[0])).toContain(
        "The download stopped: the connection dropped at 312 MB. Nothing was saved.",
      );

      // Click Try again once -> calls downloadModel again, exactly once.
      const tryAgainButtons = collectElements(
        mounted.getTree(),
        (el) =>
          el.type === "button" && textContent(el).trim() === "Try again",
      );
      expect(tryAgainButtons).toHaveLength(1);

      const beforeRetryCount = downloadModelCalls.length;
      const onClick = tryAgainButtons[0]!.props.onClick as () => void;
      onClick();
      await mounted.flushAndRerender(4);

      expect(downloadModelCalls.length - beforeRetryCount).toBe(1);
      expect(downloadModelCalls).toEqual([
        FIRST_RUN_DEFAULT_MODEL_ID,
        FIRST_RUN_DEFAULT_MODEL_ID,
      ]);
    } finally {
      mounted.unmount();
    }
  });

  test("AC-A6: clicking Cancel while downloading calls cancelDownload exactly once and renders the cancelled layout", async () => {
    const mounted = mountComponent(() =>
      React.createElement(Onboarding, {
        onModelSelected: () => {},
      }),
    );

    try {
      await mounted.flushAndRerender(4);
      emitTauriEvent("model-download-progress", {
        model_id: FIRST_RUN_DEFAULT_MODEL_ID,
        downloaded: 100_000_000,
        total: 478_000_000,
        percentage: 20.9,
      });
      await mounted.flushAndRerender(4);

      const cancelButtons = collectElements(
        mounted.getTree(),
        (el) => el.type === "button" && textContent(el).trim() === "Cancel",
      );
      expect(cancelButtons).toHaveLength(1);

      // Click Cancel once.
      const onCancel = cancelButtons[0]!.props.onClick as () => void;
      onCancel();
      await mounted.flushAndRerender(4);

      expect(cancelDownloadCalls).toEqual([FIRST_RUN_DEFAULT_MODEL_ID]);

      const cancelledCards = collectElements(
        mounted.getTree(),
        (el) => el.props?.["data-testid"] === "first-run-download-cancelled",
      );
      expect(cancelledCards).toHaveLength(1);
      expect(textContent(cancelledCards[0])).toContain("Try again");
      expect(textContent(cancelledCards[0])).toContain("Skip for now");

      // Cancel button is gone in the cancelled layout.
      const cancelButtonsAfter = collectElements(
        mounted.getTree(),
        (el) => el.type === "button" && textContent(el).trim() === "Cancel",
      );
      expect(cancelButtonsAfter).toHaveLength(0);
    } finally {
      mounted.unmount();
    }
  });
});
