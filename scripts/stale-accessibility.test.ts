import { expect, mock, test } from "bun:test";
import React, { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

const eventListeners = new Map<
  string,
  Array<(event: { payload: unknown }) => void>
>();

mock.module("@tauri-apps/api/event", () => ({
  listen: async (
    event: string,
    handler: (ev: { payload: unknown }) => void,
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

mock.module("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string) => {
    if (cmd === "evaluate_try_it_take") {
      return { dropped: "Monday, scratch that" };
    }
    return null;
  },
  transformCallback: () => 0,
  Channel: class {},
  Resource: class {},
  SERIALIZE_TO_IPC_FN: "__TAURI_TO_IPC_KEY__",
  convertFileSrc: (s: string) => s,
}));

// Translation keys stand in for the strings, so the markup names what it shows.
mock.module("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const { accessibilityStatus } =
  await import("../src/lib/utils/accessibilityStatus");
const { default: AccessibilityStaleNotice, TryItStepCard } =
  await import("../src/components/onboarding/AccessibilityStaleNotice");

const { default: PermissionAction } =
  await import("../src/components/onboarding/PermissionAction");

const STALE = "onboarding.permissions.accessibility.stale";

test("trusted with a failed input probe is a stale entry", () => {
  expect(accessibilityStatus("granted", "failed")).toBe("stale");
});

test("trusted with a working input probe is granted", () => {
  expect(accessibilityStatus("granted", "ok")).toBe("granted");
});

test("an untrusted app is not granted, whatever the probe says", () => {
  expect(accessibilityStatus("needed", "not-run")).toBe("needed");
  expect(accessibilityStatus("needed", "failed")).toBe("needed");
  expect(accessibilityStatus("needed", "ok")).toBe("needed");
});

test("a check that couldn't complete stays an error, whatever the probe says", () => {
  expect(accessibilityStatus("error", "ok")).toBe("error");
  expect(accessibilityStatus("error", "failed")).toBe("error");
});

test("a trusted app whose probe didn't run is not granted", () => {
  expect(accessibilityStatus("granted", "not-run")).toBe("needed");
});

test("the stale notice shows the remove-and-re-add steps and Check again", () => {
  const html = renderToStaticMarkup(
    createElement(AccessibilityStaleNotice, { onCheckAgain: () => {} }),
  );
  for (const key of ["description", "step1", "step2", "step3", "checkAgain"]) {
    expect(html).toContain(`${STALE}.${key}`);
  }
  expect(html).toContain("<button");
  expect(html).toContain("<ol");
});

test("the permission card shows the stale steps for a stale entry", () => {
  const html = renderToStaticMarkup(
    createElement(PermissionAction, {
      status: "stale",
      grantLabel: "grant-label",
      onGrant: () => {},
      onRetry: () => {},
    }),
  );
  expect(html).toContain(`${STALE}.step1`);
  expect(html).toContain(`${STALE}.checkAgain`);
  expect(html).not.toContain("onboarding.permissions.granted");
});

type Elem = React.ReactElement<Record<string, unknown>>;

function mountTryIt() {
  const states: unknown[] = [];
  const refs: Array<{ current: unknown }> = [];
  const effects: Array<{
    deps?: readonly unknown[];
    cleanup?: void | (() => void);
  }> = [];
  const r18 = (
    React as unknown as {
      __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED?: {
        ReactCurrentDispatcher: { current: unknown };
      };
    }
  ).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const orig = r18?.ReactCurrentDispatcher.current;
  let tree: React.ReactNode = null;

  const rerender = () => {
    let sc = 0;
    let rc = 0;
    let ec = 0;
    const pending: Array<{
      idx: number;
      fn: () => void | (() => void);
      deps?: readonly unknown[];
    }> = [];
    if (r18) {
      r18.ReactCurrentDispatcher.current = {
        useState: <T>(init: T | (() => T)) => {
          const i = sc++;
          if (i >= states.length) {
            states[i] = typeof init === "function" ? (init as () => T)() : init;
          }
          return [
            states[i] as T,
            (next: T | ((p: T) => T)) => {
              states[i] =
                typeof next === "function"
                  ? (next as (p: T) => T)(states[i] as T)
                  : next;
            },
          ];
        },
        useRef: <T>(init: T) => {
          const i = rc++;
          if (i >= refs.length) refs[i] = { current: init };
          return refs[i];
        },
        useEffect: (
          fn: () => void | (() => void),
          deps?: readonly unknown[],
        ) => {
          const i = ec++;
          const prev = effects[i];
          const changed =
            !prev ||
            !prev.deps ||
            !deps ||
            prev.deps.length !== deps.length ||
            deps.some((v, idx) => !Object.is(prev.deps![idx], v));
          if (changed) pending.push({ idx: i, fn, deps });
        },
        useCallback: <T>(fn: T) => fn,
        useMemo: <T>(fn: () => T) => fn(),
        useContext: (ctx: { _currentValue?: unknown }) => ctx._currentValue,
      };
    }
    try {
      tree = TryItStepCard({ shortcut: "fn", onDone: () => {} });
    } finally {
      if (r18) r18.ReactCurrentDispatcher.current = orig;
    }
    for (const eff of pending) {
      effects[eff.idx]?.cleanup?.();
      effects[eff.idx] = { deps: eff.deps, cleanup: eff.fn() };
    }
    return tree;
  };

  const findInput = (): Elem => {
    const stack: React.ReactNode[] = [tree];
    while (stack.length > 0) {
      const node = stack.pop();
      if (node == null || typeof node !== "object") continue;
      if (Array.isArray(node)) {
        stack.push(...node);
        continue;
      }
      if ("type" in node) {
        const el = node as Elem;
        if (el.type === "input") return el;
        if (el.props && "children" in el.props) {
          stack.push(el.props.children as React.ReactNode);
        }
      }
    }
    throw new Error("TryItStepCard input not found");
  };

  const changeInput = (nextValue: string) => {
    const input = findInput();
    (input.props.onChange as (e: { target: { value: string } }) => void)({
      target: { value: nextValue },
    });
    rerender();
  };

  const value = () => String(findInput().props.value ?? "");
  const html = () => renderToStaticMarkup(tree as React.ReactElement);
  const unmount = () => {
    for (const eff of effects) eff?.cleanup?.();
    eventListeners.clear();
  };

  rerender();
  return { rerender, value, changeInput, html, unmount };
}

async function emitTake(raw: string, postProcessed: string) {
  for (const cb of [...(eventListeners.get("history-update-payload") ?? [])]) {
    cb({
      payload: {
        action: "added",
        entry: {
          transcription_text: raw,
          post_processed_text: postProcessed,
        },
      },
    });
  }
  await new Promise((r) => setTimeout(r, 10));
}

test("TryItStepCard shows one take once when history event fires before paste or direct typing", async () => {
  const pasted = mountTryIt();
  try {
    await emitTake("Monday, scratch that, Tuesday.", "Tuesday.");
    pasted.rerender();
    pasted.changeInput(`${pasted.value()}Tuesday.`);
    expect(pasted.value()).toBe("Tuesday.");
    expect(pasted.html()).toContain('data-testid="try-it-cleanup-explanation"');
  } finally {
    pasted.unmount();
  }

  const typed = mountTryIt();
  try {
    await emitTake("Monday, scratch that, Tuesday.", "Tuesday.");
    typed.rerender();
    for (const ch of "Tuesday.") {
      typed.changeInput(`${typed.value()}${ch}`);
    }
    expect(typed.value()).toBe("Tuesday.");
    expect(typed.html()).toContain('data-testid="try-it-cleanup-explanation"');
  } finally {
    typed.unmount();
  }
});

test("TryItStepCard shows one take once when typed or pasted input arrives before or across the history event", async () => {
  const typedFirst = mountTryIt();
  try {
    for (const ch of "Tuesday.") {
      typedFirst.changeInput(`${typedFirst.value()}${ch}`);
    }
    expect(typedFirst.value()).toBe("Tuesday.");
    await emitTake("Monday, scratch that, Tuesday.", "Tuesday.");
    typedFirst.rerender();
    expect(typedFirst.value()).toBe("Tuesday.");
    expect(typedFirst.html()).toContain(
      'data-testid="try-it-cleanup-explanation"',
    );
  } finally {
    typedFirst.unmount();
  }

  const interleaved = mountTryIt();
  try {
    for (const ch of "Tues") {
      interleaved.changeInput(`${interleaved.value()}${ch}`);
    }
    await emitTake("Monday, scratch that, Tuesday.", "Tuesday.");
    interleaved.rerender();
    for (const ch of "day.") {
      interleaved.changeInput(`${interleaved.value()}${ch}`);
    }
    expect(interleaved.value()).toBe("Tuesday.");
  } finally {
    interleaved.unmount();
  }
});

test("TryItStepCard keeps hand-typed self-correction working", () => {
  const card = mountTryIt();
  try {
    card.changeInput("Monday, scratch that, Friday");
    expect(card.value()).toBe("Friday");
    expect(card.html()).toContain('data-testid="try-it-cleanup-explanation"');
  } finally {
    card.unmount();
  }
});
