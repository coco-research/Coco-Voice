import { beforeEach, describe, expect, mock, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { useSettingsStore } from "../src/stores/settingsStore";
import type { MeetingStatus } from "../src/bindings.meetings";
import {
  MEETING_STATE_EVENT,
  MEETINGS_OPEN_CONSENT_EVENT,
} from "../src/components/meetings/meetingEvents";

const ROOT = join(import.meta.dir, "..");

const invokeCalls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
let nextInvokeHandler:
  | ((cmd: string, args?: Record<string, unknown>) => Promise<unknown>)
  | null = null;
const applyThemeCalls: string[] = [];
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

mock.module("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    invokeCalls.push({ cmd, args });
    if (nextInvokeHandler) {
      return nextInvokeHandler(cmd, args);
    }
    return null;
  },
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
}));

mock.module("../src/components/whats-new/releaseNotes", () => ({
  findReleaseNoteToShow: () => null,
  findLatestReleaseNote: () => null,
}));

mock.module("../src/i18n", () => ({
  default: i18next,
  SUPPORTED_LANGUAGES: [{ code: "en", name: "English", nativeName: "English" }],
  syncLanguageFromSettings: async () => {},
  getLanguageDirection: () => "ltr",
  isRTLLanguage: () => false,
}));

mock.module("@/lib/utils/theme", () => ({
  THEME_OPTIONS: ["system", "light", "dark"] as const,
  applyTheme: (theme: string) => {
    applyThemeCalls.push(theme);
  },
  getEffectiveTheme: (theme: string) => (theme === "dark" ? "dark" : "light"),
  setupSystemThemeListener: () => () => {},
}));

const { SettingsV022Page } =
  await import("../src/components/settings/SettingsV022Page");
const { AboutSettings } =
  await import("../src/components/settings/about/AboutSettings");
const { MeetingsRetentionSection } =
  await import("../src/components/settings/MeetingsRetention");
const { MeetingsFilesAndLanguageSection } =
  await import("../src/components/settings/MeetingsFilesAndLanguage");
const { RecordMeetingPage, RecordMeetingSection } =
  await import("../src/components/meetings/RecordMeetingPage");
const { MeetingFloatingPill } =
  await import("../src/components/meetings/MeetingEntryPanels");
const { MeetingPillView } =
  await import("../src/overlay/RecordingPillStates");

async function makeI18n() {
  const inst = i18next.createInstance();
  await inst.use(initReactI18next).init({
    lng: "en",
    resources: { en: { translation: en } },
    interpolation: { escapeValue: false },
  });
  return inst;
}

function withSettings<T>(
  partial: Record<string, unknown>,
  fn: () => T,
  onUpdateSetting?: (key: string, value: unknown) => Promise<void>,
): T {
  const g = globalThis as { window?: unknown };
  const prevWin = Object.getOwnPropertyDescriptor(globalThis, "window");
  g.window = { __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" } };
  const prevState = useSettingsStore.getState();
  const prevInitial = { ...useSettingsStore.getInitialState() };
  const nextState = {
    isLoading: false,
    settings: {
      ...(prevState.settings ?? {}),
      ...partial,
    } as never,
    ...(onUpdateSetting
      ? { updateSetting: onUpdateSetting as never }
      : {}),
  };
  Object.assign(useSettingsStore.getInitialState(), nextState);
  useSettingsStore.setState(nextState);
  try {
    return fn();
  } finally {
    Object.assign(useSettingsStore.getInitialState(), prevInitial);
    useSettingsStore.setState({
      isLoading: prevState.isLoading,
      settings: prevState.settings,
      updateSetting: prevState.updateSetting,
    });
    if (prevWin) Object.defineProperty(globalThis, "window", prevWin);
    else delete g.window;
  }
}

type Elem = React.ReactElement<Record<string, unknown>>;

/** Walk a rendered React element tree (invoking function components via a minimal hook shim for stateless/stateful inspection). */
function renderTree(
  node: React.ReactNode,
  stateSlots: Array<{ value: unknown; set: (v: unknown) => void }>,
  slotIdxRef: { current: number },
): React.ReactNode {
  if (node == null || typeof node !== "object" || Array.isArray(node)) {
    if (Array.isArray(node)) {
      return node.map((child) => renderTree(child, stateSlots, slotIdxRef));
    }
    return node;
  }
  const el = node as Elem;
  if (typeof el.type === "function") {
    const fn = el.type as (props: Record<string, unknown>) => React.ReactNode;
    const out = fn(el.props);
    return renderTree(out, stateSlots, slotIdxRef);
  }
  if (el.props && "children" in el.props) {
    return React.cloneElement(
      el,
      undefined,
      renderTree(el.props.children as React.ReactNode, stateSlots, slotIdxRef),
    );
  }
  return el;
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

/** Minimal synchronous hook harness for driving a single React component's useState/useEffect/onClick handlers without a browser. */
function mountInteractive<P extends Record<string, unknown>>(
  Component: React.FC<P>,
  props: P,
  options?: { runEffects?: boolean },
) {
  const runEffects = options?.runEffects ?? false;
  const states: unknown[] = [];
  const refs: Array<{ current: unknown }> = [];
  const effects: Array<{
    deps?: readonly unknown[];
    cleanup?: void | (() => void);
  }> = [];
  const callbacks: Array<{ fn: unknown; deps?: readonly unknown[] }> = [];
  let stateCursor = 0;
  let refCursor = 0;
  let effectCursor = 0;
  let cbCursor = 0;

  // React 18 / 19 hook dispatcher shim (only for test-local single-component interaction)
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const reactAny = React as any;
  const r18 = reactAny.__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED?.ReactCurrentDispatcher;
  const r19 = reactAny.__CLIENT_INTERNALS_DO_NOT_USE_OR_WARN_USERS_THEY_CANNOT_UPGRADE;
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

  let currentTree: React.ReactNode = null;
  const rerender = (nextProps: P = props) => {
    stateCursor = 0;
    refCursor = 0;
    effectCursor = 0;
    cbCursor = 0;
    const pendingEffects: Array<{ idx: number; fn: () => void | (() => void); deps?: readonly unknown[] }> = [];
    setDisp({
      useState<T>(init: T | (() => T)): [T, (next: T | ((p: T) => T)) => void] {
        const idx = stateCursor++;
        if (idx >= states.length) {
          states[idx] = typeof init === "function" ? (init as () => T)() : init;
        }
        const set = (next: T | ((p: T) => T)) => {
          states[idx] =
            typeof next === "function"
              ? (next as (p: T) => T)(states[idx] as T)
              : next;
        };
        return [states[idx] as T, set];
      },
      useEffect(fn: () => void | (() => void), deps?: readonly unknown[]) {
        const idx = effectCursor++;
        if (!runEffects) return;
        const prev = effects[idx];
        if (!prev || depsChanged(prev.deps, deps)) {
          pendingEffects.push({ idx, fn, deps });
        }
      },
      useLayoutEffect() {},
      useRef<T>(init: T) {
        const idx = refCursor++;
        if (idx >= refs.length) refs[idx] = { current: init };
        return refs[idx];
      },
      useCallback<T>(fn: T, deps?: readonly unknown[]) {
        const idx = cbCursor++;
        const prev = callbacks[idx];
        if (!prev || depsChanged(prev.deps, deps)) {
          callbacks[idx] = { fn, deps };
        }
        return callbacks[idx].fn as T;
      },
      useMemo<T>(fn: () => T) {
        return fn();
      },
      useContext(ctx: { _currentValue?: unknown }) {
        return ctx._currentValue;
      },
      useSyncExternalStore<T>(
        _sub: unknown,
        getSnap: () => T,
      ): T {
        return getSnap();
      },
      useId: () => "test-id",
      useTransition: () => [false, (fn: () => void) => fn()],
      useDebugValue() {},
      useDeferredValue<T>(v: T) {
        return v;
      },
      useInsertionEffect() {},
      useImperativeHandle() {},
    });
    try {
      currentTree = Component(nextProps);
    } finally {
      setDisp(origDispatcher);
    }
    for (const eff of pendingEffects) {
      const prev = effects[eff.idx];
      if (prev?.cleanup) prev.cleanup();
      const cleanup = eff.fn();
      effects[eff.idx] = { deps: eff.deps, cleanup };
    }
    return currentTree;
  };

  const unmount = () => {
    for (const eff of effects) {
      if (eff?.cleanup) eff.cleanup();
    }
    effects.length = 0;
  };

  rerender();
  return {
    getTree: () => currentTree,
    rerender,
    unmount,
  };
}

function listFilesRecursive(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) out.push(...listFilesRecursive(full));
    else out.push(full);
  }
  return out;
}

describe("X3c UI (AC-D-R2, AC-D-R4, AC-B19d, AC-B19e)", () => {
  beforeEach(() => {
    invokeCalls.length = 0;
    applyThemeCalls.length = 0;
    eventListeners.clear();
    nextInvokeHandler = null;
  });

  test("AC-D-R2: Settings > App has Appearance segmented control after Sounds (no accent blue), calls updateSetting+applyTheme, and About has no theme control", async () => {
    const i18n = await makeI18n();
    const updated: Array<{ key: string; value: unknown }> = [];

    const settingsHtml = withSettings(
      { theme: "system", audio_feedback: true, update_checks_enabled: true },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <SettingsV022Page />
          </I18nextProvider>,
        ).replaceAll("&#x27;", "'"),
    );

    // Extract App card HTML.
    const appIdx = settingsHtml.indexOf('data-testid="settings-card-app"');
    expect(appIdx).toBeGreaterThan(-1);
    const appEnd = settingsHtml.indexOf("</section>", appIdx);
    const appHtml = settingsHtml.slice(appIdx, appEnd);

    // Row order: Open at login -> Show in the menu bar -> Sounds -> Appearance -> Check for updates.
    const idxLogin = appHtml.indexOf("Open at login");
    const idxMenu = appHtml.indexOf("Show in the menu bar");
    const idxSounds = appHtml.indexOf("Sounds");
    const idxAppearance = appHtml.indexOf("Appearance");
    const idxUpdates = appHtml.indexOf("Check for updates");
    expect(idxLogin).toBeGreaterThan(-1);
    expect(idxMenu).toBeGreaterThan(idxLogin);
    expect(idxSounds).toBeGreaterThan(idxMenu);
    expect(idxAppearance).toBeGreaterThan(idxSounds);
    expect(idxUpdates).toBeGreaterThan(idxAppearance);

    // Segmented control exists with System (pressed), Light, Dark and zero accent classes.
    const segIdx = appHtml.indexOf('data-testid="appearance-segmented"');
    expect(segIdx).toBeGreaterThan(-1);
    const segTagStart = appHtml.lastIndexOf("<div", segIdx);
    const segEnd = appHtml.indexOf("</div>", segIdx);
    const segHtml = appHtml.slice(segTagStart, segEnd);
    expect(segHtml).toContain('role="group"');
    expect(segHtml).toContain("System");
    expect(segHtml).toContain("Light");
    expect(segHtml).toContain("Dark");
    expect(segHtml).toContain('aria-pressed="true"');
    expect(segHtml).not.toContain("bg-logo-primary");
    expect(segHtml).not.toContain("text-logo-primary");
    expect(segHtml).not.toContain("border-logo-primary");
    expect(segHtml).not.toContain("bg-accent");

    // Clicking "Light" invokes applyTheme("light") and updateSetting("theme", "light").
    withSettings(
      { theme: "system" },
      () => {
        const mounted = mountInteractive(SettingsV022Page, {});
        const buttons = collectElements(
          mounted.getTree(),
          (el) => el.type === "button" && el.props["data-testid"] === "appearance-option-light",
        );
        expect(buttons.length).toBe(1);
        const onClick = buttons[0].props.onClick as (() => void) | undefined;
        expect(typeof onClick).toBe("function");
        onClick?.();
      },
      async (key, value) => {
        updated.push({ key, value });
      },
    );
    expect(applyThemeCalls).toEqual(["light"]);
    expect(updated).toEqual([{ key: "theme", value: "light" }]);

    // About has NO theme control.
    const aboutHtml = withSettings({ theme: "system" }, () =>
      renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <AboutSettings />
        </I18nextProvider>,
      ).replaceAll("&#x27;", "'"),
    );
    expect(aboutHtml).not.toContain("Application Theme");
    expect(aboutHtml).not.toContain("App theme");
    const aboutSrc = readFileSync(
      join(ROOT, "src/components/settings/about/AboutSettings.tsx"),
      "utf8",
    );
    expect(aboutSrc).not.toContain("ThemeSelector");
  });

  test("AC-D-R4: all 4 Settings sub-pages place ← Settings first above the eyebrow, click returns to Settings, top-level has no back link, and src has zero All settings / backToSettings", async () => {
    const i18n = await makeI18n();

    // 1. Top-level Settings has NO back link.
    const topHtml = withSettings({}, () =>
      renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <SettingsV022Page />
        </I18nextProvider>,
      ),
    );
    expect(topHtml).not.toContain('data-testid="settings-back-link"');
    expect(topHtml).not.toContain("←");

    // 2. Retention sub-page: ← Settings appears BEFORE eyebrow and invokes onBack.
    const retentionHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <MeetingsRetentionSection
          deleteAudioAfterNotes={true}
          transcriptDeleteAfterDays={null}
          deleteAllStep={0}
          onToggleDeleteAudio={() => {}}
          onChangeTranscriptTimer={() => {}}
          onRequestDeleteAllStep1={() => {}}
          onAdvanceDeleteAllStep2={() => {}}
          onCancelDeleteAll={() => {}}
          onConfirmDeleteAll={() => {}}
          onBack={() => {}}
          showBreadcrumb={true}
        />
      </I18nextProvider>,
    );
    const retBackIdx = retentionHtml.indexOf('data-testid="settings-back-link"');
    const retEyebrowIdx = retentionHtml.indexOf("Settings › Meetings");
    expect(retBackIdx).toBeGreaterThan(-1);
    expect(retEyebrowIdx).toBeGreaterThan(retBackIdx);
    expect(retentionHtml).toContain('<span aria-hidden="true">←</span>');
    expect(retentionHtml).not.toContain("All settings");

    let retentionBacked = false;
    const retMounted = mountInteractive(MeetingsRetentionSection, {
      deleteAudioAfterNotes: true,
      transcriptDeleteAfterDays: null,
      deleteAllStep: 0 as const,
      onToggleDeleteAudio: () => {},
      onChangeTranscriptTimer: () => {},
      onRequestDeleteAllStep1: () => {},
      onAdvanceDeleteAllStep2: () => {},
      onCancelDeleteAll: () => {},
      onConfirmDeleteAll: () => {},
      onBack: () => {
        retentionBacked = true;
      },
      showBreadcrumb: true,
    });
    const retBackBtns = collectElements(
      retMounted.getTree(),
      (el) => el.type === "button" && el.props["data-testid"] === "settings-back-link",
    );
    expect(retBackBtns.length).toBe(1);
    (retBackBtns[0].props.onClick as () => void)();
    expect(retentionBacked).toBe(true);

    // 3. Files and Language sub-pages: ← Settings appears BEFORE eyebrow and invokes onBack.
    for (const mode of ["files", "language"] as const) {
      const flHtml = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <MeetingsFilesAndLanguageSection
            mode={mode}
            saveMarkdownEnabled={false}
            folderPath=""
            meetingLanguage="auto"
            perMeetingOverride={false}
            onToggleSaveMarkdown={() => {}}
            onFolderChange={() => {}}
            onLanguageChange={() => {}}
            onTogglePerMeetingOverride={() => {}}
            onBack={() => {}}
          />
        </I18nextProvider>,
      );
      const backIdx = flHtml.indexOf('data-testid="settings-back-link"');
      const eyebrowIdx = flHtml.indexOf("Settings › Meetings");
      expect(backIdx).toBeGreaterThan(-1);
      expect(eyebrowIdx).toBeGreaterThan(backIdx);
      expect(flHtml).toContain('<span aria-hidden="true">←</span>');
      expect(flHtml).not.toContain("All settings");

      let backed = false;
      const mounted = mountInteractive(MeetingsFilesAndLanguageSection, {
        mode,
        saveMarkdownEnabled: false,
        folderPath: "",
        meetingLanguage: "auto",
        perMeetingOverride: false,
        onToggleSaveMarkdown: () => {},
        onFolderChange: () => {},
        onLanguageChange: () => {},
        onTogglePerMeetingOverride: () => {},
        onBack: () => {
          backed = true;
        },
      });
      const btns = collectElements(
        mounted.getTree(),
        (el) => el.type === "button" && el.props["data-testid"] === "settings-back-link",
      );
      expect(btns.length).toBe(1);
      (btns[0].props.onClick as () => void)();
      expect(backed).toBe(true);
    }

    // 4. SettingsV022Page Hands-free and Calendars sub-pages: ← Settings first and click returns to Settings.
    for (const target of ["hands_free", "calendars"] as const) {
      const subHtml = withSettings({}, () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <SettingsV022Page target={target} />
          </I18nextProvider>,
        ),
      );
      const backIdx = subHtml.indexOf('data-testid="settings-back-link"');
      const eyebrowIdx = subHtml.indexOf("Settings ›");
      expect(backIdx).toBeGreaterThan(-1);
      expect(eyebrowIdx).toBeGreaterThan(backIdx);
      expect(subHtml).toContain('<span aria-hidden="true">←</span>');
      expect(subHtml).not.toContain("All settings");

      withSettings({}, () => {
        const mounted = mountInteractive(SettingsV022Page, { target });
        const btns = collectElements(
          mounted.getTree(),
          (el) => el.type === "button" && el.props["data-testid"] === "settings-back-link",
        );
        expect(btns.length).toBe(1);
        (btns[0].props.onClick as () => void)();
        const afterTree = mounted.rerender({ target });
        const overview = collectElements(
          afterTree,
          (el) => el.props["data-testid"] === "settings-v022-page",
        );
        expect(overview.length).toBe(1);
      });
    }

    // 5. No remaining "All settings" or "backToSettings" anywhere in src/.
    const srcFiles = listFilesRecursive(join(ROOT, "src"));
    const hits: string[] = [];
    for (const f of srcFiles) {
      const txt = readFileSync(f, "utf8");
      if (txt.includes("All settings") || txt.includes("backToSettings")) {
        hits.push(f);
      }
    }
    expect(hits).toEqual([]);
  });

  test("AC-B19d: confirm_meeting_start refusals show permission (3 buttons), device (2 buttons with error code), or fallback message, and Record my mic only calls confirm_meeting_start_mic_only once", async () => {
    const i18n = await makeI18n();

    // Helper: drive real RecordMeetingPage through onConfirmStart with a mocked refusal error.
    async function driveStartRefusal(refusalError: unknown) {
      invokeCalls.length = 0;
      let micOnlyCalls = 0;
      nextInvokeHandler = async (cmd) => {
        if (cmd === "confirm_meeting_start") {
          throw refusalError;
        }
        if (cmd === "confirm_meeting_start_mic_only") {
          micOnlyCalls += 1;
          return {
            state: "recording",
            elapsed_secs: 0,
            meeting_id: 99,
            mic_name: "Built-in Mic",
            mic_only: true,
            warnings: [],
          } satisfies MeetingStatus;
        }
        return {
          state: "idle",
          elapsed_secs: 0,
          meeting_id: null,
          mic_name: null,
          warnings: [],
        } satisfies MeetingStatus;
      };

      const mounted = withSettings({}, () =>
        mountInteractive(RecordMeetingPage, {}),
      );
      const section = mounted.getTree() as Elem;
      const onConfirmStart = section.props.onConfirmStart as () => void;
      onConfirmStart();
      // Flush async invoke + .then microtasks cleanly.
      await new Promise((r) => setTimeout(r, 0));
      const afterTree = withSettings({}, () => mounted.rerender()) as Elem;
      const renderedHtml = renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>{afterTree}</I18nextProvider>,
      ).replaceAll("&#x27;", "'");
      return {
        afterSection: afterTree,
        renderedHtml,
        getMicOnlyCalls: () => micOnlyCalls,
        rerender: () => withSettings({}, () => mounted.rerender()) as Elem,
      };
    }

    // 1. Permission refusal (`system_audio_permission_denied`) -> panel right after Start now, 3 buttons in order.
    const perm1 = await driveStartRefusal("system_audio_permission_denied");
    expect(perm1.afterSection.props.consentOpen).toBe(false);
    expect(perm1.renderedHtml).toContain('data-testid="meeting-start-problem-panel"');
    expect(perm1.renderedHtml).toContain("Your Mac's sound isn't being recorded");
    expect(perm1.renderedHtml).toContain(
      "Coco Voice needs permission to record your Mac's audio.",
    );
    const startNowIdx = perm1.renderedHtml.indexOf("Start now");
    const panelIdx = perm1.renderedHtml.indexOf('data-testid="meeting-start-problem-panel"');
    expect(panelIdx).toBeGreaterThan(startNowIdx);

    const permPanelSlice = perm1.renderedHtml.slice(
      panelIdx,
      perm1.renderedHtml.indexOf("</section>", panelIdx),
    );
    const permButtons = [...permPanelSlice.matchAll(/<button[^>]*>(.*?)<\/button>/g)].map(
      (m) => m[1].replace(/<[^>]+>/g, "").trim(),
    );
    expect(permButtons).toEqual([
      "Record my mic only",
      "Open System Settings",
      "Cancel",
    ]);

    // Permission via system_audio_tap_failed { cause: "permission" } also shows 3 buttons.
    const perm2 = await driveStartRefusal({
      system_audio_tap_failed: {
        cause: "permission",
        call: "create_process_tap",
        code: "!obj",
      },
    });
    expect(perm2.renderedHtml).toContain(
      "Coco Voice needs permission to record your Mac's audio.",
    );
    expect(perm2.renderedHtml).toContain("Open System Settings");

    // Clicking "Open System Settings" calls zero-arg open_system_audio_privacy_settings (no URL argument).
    const onOpenSysSettings = perm2.afterSection.props.onOpenSystemSettings as
      | (() => void)
      | undefined;
    expect(typeof onOpenSysSettings).toBe("function");
    onOpenSysSettings?.();
    const sysCall = invokeCalls.find(
      (c) => c.cmd === "open_system_audio_privacy_settings",
    );
    expect(sysCall).toBeDefined();
    expect(sysCall?.args).toBeUndefined();

    // 2. Device refusal with !dev -> shows "(error !dev)" and 2 buttons (no Open System Settings).
    const dev = await driveStartRefusal({
      system_audio_tap_failed: {
        cause: "device",
        call: "create_process_tap",
        code: "!dev",
      },
    });
    expect(dev.afterSection.props.consentOpen).toBe(false);
    expect(dev.renderedHtml).toContain("Your Mac's sound isn't being recorded");
    expect(dev.renderedHtml).toContain("The audio device didn't start (error !dev).");
    const devPanelIdx = dev.renderedHtml.indexOf('data-testid="meeting-start-problem-panel"');
    const devPanelSlice = dev.renderedHtml.slice(
      devPanelIdx,
      dev.renderedHtml.indexOf("</section>", devPanelIdx),
    );
    const devButtons = [...devPanelSlice.matchAll(/<button[^>]*>(.*?)<\/button>/g)].map(
      (m) => m[1].replace(/<[^>]+>/g, "").trim(),
    );
    expect(devButtons).toEqual(["Record my mic only", "Cancel"]);
    expect(devPanelSlice).not.toContain("Open System Settings");

    // 3. "Record my mic only" calls confirm_meeting_start_mic_only exactly once.
    const onMicOnly = dev.afterSection.props.onConfirmStartMicOnly as
      | (() => void)
      | undefined;
    expect(typeof onMicOnly).toBe("function");
    onMicOnly?.();
    await new Promise((r) => setTimeout(r, 0));
    expect(dev.getMicOnlyCalls()).toBe(1);
    const afterMicOnly = dev.rerender();
    expect((afterMicOnly.props.status as MeetingStatus).state).toBe("recording");
    expect((afterMicOnly.props.status as MeetingStatus).mic_only).toBe(true);

    // Step 3b AC-B19d: "!dev (560227702)" -> "(error !dev)" and "-10875" -> "(error -10875)".
    const devFourCharWithDecimal = await driveStartRefusal({
      system_audio_tap_failed: {
        cause: "device",
        call: "create_process_tap",
        code: "!dev (560227702)",
      },
    });
    expect(devFourCharWithDecimal.renderedHtml).toContain("(error !dev)");

    const devNumeric = await driveStartRefusal({
      system_audio_tap_failed: {
        cause: "device",
        call: "create_process_tap",
        code: "-10875",
      },
    });
    expect(devNumeric.renderedHtml).toContain("(error -10875)");

    // 4. Any other refusal (e.g. already_recording / system_audio_unavailable) shows a visible message.
    for (const otherErr of [
      "already_recording",
      "system_audio_unavailable",
      { no_long_audio_model: { model_id: "parakeet" } },
    ]) {
      const other = await driveStartRefusal(otherErr);
      expect(other.renderedHtml).toContain("Could not start the meeting.");
      expect(other.renderedHtml).toContain('role="alert"');
    }
  });

  test("AC-B19e: Mic only chip shows in pill and meeting page header when mic_only is true, and never when false", async () => {
    const i18n = await makeI18n();

    // Pill: micOnly=true shows chip next to timer (neutral style, no accent blue).
    const pillMicOnlyHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <MeetingPillView elapsedSecs={768} micOnly={true} onStop={() => {}} />
      </I18nextProvider>,
    );
    expect(pillMicOnlyHtml).toContain('data-testid="mic-only-chip"');
    expect(pillMicOnlyHtml).toContain("Mic only");
    const chipStart = pillMicOnlyHtml.indexOf('data-testid="mic-only-chip"');
    const chipEnd = pillMicOnlyHtml.indexOf("</span>", chipStart);
    const pillChipTag = pillMicOnlyHtml.slice(chipStart, chipEnd);
    expect(pillChipTag).not.toContain("bg-logo-primary");
    expect(pillChipTag).not.toMatch(/\bbg-/);

    // Pill: micOnly=false (and omitted) hides chip.
    const pillNormalHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <MeetingPillView elapsedSecs={768} micOnly={false} onStop={() => {}} />
      </I18nextProvider>,
    );
    expect(pillNormalHtml).not.toContain('data-testid="mic-only-chip"');
    expect(pillNormalHtml).not.toContain("Mic only");

    // Meeting page header (RecordMeetingSection + MeetingFloatingPill): mic_only=true shows chip with no bg-.
    const pageMicOnlyHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <RecordMeetingSection
          offerEnabled={true}
          offer={null}
          consentOpen={false}
          status={{
            state: "recording",
            elapsed_secs: 768,
            meeting_id: 7,
            mic_name: "Built-in Mic",
            mic_only: true,
            warnings: [],
          }}
          segments={[]}
          meetingWords={[]}
          meetingTitle="Product sync"
          onToggleOffer={() => {}}
          onOpenConsent={() => {}}
          onConfirmStart={() => {}}
          onCancelConsent={() => {}}
          onDismissOffer={() => {}}
          onStopMeeting={() => {}}
          onAddMeetingWord={() => {}}
          onRemoveMeetingWord={() => {}}
        />
      </I18nextProvider>,
    );
    expect(pageMicOnlyHtml).toContain('data-testid="mic-only-chip"');
    expect(pageMicOnlyHtml).toContain("Mic only");
    const pageChipStart = pageMicOnlyHtml.indexOf('data-testid="mic-only-chip"');
    const pageChipEnd = pageMicOnlyHtml.indexOf("</span>", pageChipStart);
    expect(pageMicOnlyHtml.slice(pageChipStart, pageChipEnd)).not.toMatch(/\bbg-/);

    // Paused meeting header chip also carries no bg- class.
    const pagePausedMicOnlyHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <RecordMeetingSection
          offerEnabled={true}
          offer={null}
          consentOpen={false}
          status={{
            state: "paused",
            elapsed_secs: 768,
            meeting_id: 7,
            mic_name: "Built-in Mic",
            mic_only: true,
            warnings: [],
          }}
          segments={[]}
          meetingWords={[]}
          meetingTitle="Product sync"
          onToggleOffer={() => {}}
          onOpenConsent={() => {}}
          onConfirmStart={() => {}}
          onCancelConsent={() => {}}
          onDismissOffer={() => {}}
          onStopMeeting={() => {}}
          onAddMeetingWord={() => {}}
          onRemoveMeetingWord={() => {}}
        />
      </I18nextProvider>,
    );
    const pausedChipStart = pagePausedMicOnlyHtml.indexOf('data-testid="mic-only-chip"');
    expect(pausedChipStart).toBeGreaterThan(-1);
    const pausedChipEnd = pagePausedMicOnlyHtml.indexOf("</span>", pausedChipStart);
    expect(pagePausedMicOnlyHtml.slice(pausedChipStart, pausedChipEnd)).not.toMatch(/\bbg-/);

    // Meeting page header: mic_only=false hides chip.
    const pageNormalHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <RecordMeetingSection
          offerEnabled={true}
          offer={null}
          consentOpen={false}
          status={{
            state: "recording",
            elapsed_secs: 768,
            meeting_id: 7,
            mic_name: "Built-in Mic",
            mic_only: false,
            warnings: [],
          }}
          segments={[]}
          meetingWords={[]}
          meetingTitle="Product sync"
          onToggleOffer={() => {}}
          onOpenConsent={() => {}}
          onCancelConsent={() => {}}
          onDismissOffer={() => {}}
          onStopMeeting={() => {}}
          onAddMeetingWord={() => {}}
          onRemoveMeetingWord={() => {}}
        />
      </I18nextProvider>,
    );
    expect(pageNormalHtml).not.toContain('data-testid="mic-only-chip"');
    expect(pageNormalHtml).not.toContain("Mic only");

    // Direct MeetingFloatingPill check too.
    const floatOn = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <MeetingFloatingPill elapsedSecs={768} micOnly={true} onStop={() => {}} />
      </I18nextProvider>,
    );
    expect(floatOn).toContain('data-testid="mic-only-chip"');
    const floatChipStart = floatOn.indexOf('data-testid="mic-only-chip"');
    const floatChipEnd = floatOn.indexOf("</span>", floatChipStart);
    expect(floatOn.slice(floatChipStart, floatChipEnd)).not.toMatch(/\bbg-/);
    const floatOff = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <MeetingFloatingPill elapsedSecs={768} micOnly={false} onStop={() => {}} />
      </I18nextProvider>,
    );
    expect(floatOff).not.toContain('data-testid="mic-only-chip"');
  });

  test("AC-B19f: mic_only without attendees reads Me on both lines (no Me and Others); mic_only false reads Me and Others; known attendees still show", async () => {
    const i18n = await makeI18n();
    const renderRecordPage = (status: MeetingStatus) =>
      renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>
          <RecordMeetingSection
            offerEnabled={true}
            offer={null}
            consentOpen={false}
            status={status}
            segments={[]}
            meetingWords={[]}
            meetingTitle="Product sync"
            onToggleOffer={() => {}}
            onOpenConsent={() => {}}
            onConfirmStart={() => {}}
            onCancelConsent={() => {}}
            onDismissOffer={() => {}}
            onStopMeeting={() => {}}
            onAddMeetingWord={() => {}}
            onRemoveMeetingWord={() => {}}
          />
        </I18nextProvider>,
      );

    // 1. mic_only=true, no attendees -> both lines read "Me" and "Me and Others" appears nowhere.
    const micOnlyHtml = renderRecordPage({
      state: "recording",
      elapsed_secs: 768,
      meeting_id: 7,
      mic_name: "Built-in Mic",
      mic_only: true,
      warnings: [],
    });
    expect(micOnlyHtml).not.toContain("Me and Others");
    expect(
      micOnlyHtml.match(/data-testid="live-meeting-participants"[^>]*>(.*?)<\/p>/)?.[1]?.trim(),
    ).toBe("Me");
    expect(
      micOnlyHtml.match(
        /data-testid="transcript-channels-outside"[\s\S]*?<h3[^>]*>(.*?)<\/h3>/,
      )?.[1]?.trim(),
    ).toBe("Me");

    // 2. mic_only=false, no attendees -> both lines read "Me and Others".
    const normalHtml = renderRecordPage({
      state: "recording",
      elapsed_secs: 768,
      meeting_id: 7,
      mic_name: "Built-in Mic",
      mic_only: false,
      warnings: [],
    });
    expect(
      normalHtml.match(/data-testid="live-meeting-participants"[^>]*>(.*?)<\/p>/)?.[1]?.trim(),
    ).toBe("Me and Others");
    expect(
      normalHtml.match(
        /data-testid="transcript-channels-outside"[\s\S]*?<h3[^>]*>(.*?)<\/h3>/,
      )?.[1]?.trim(),
    ).toBe("Me and Others");

    // 3. mic_only=true with known calendar attendees -> names still show on both lines.
    const attendeesHtml = renderRecordPage({
      state: "recording",
      elapsed_secs: 768,
      meeting_id: 7,
      mic_name: "Built-in Mic",
      mic_only: true,
      attendees: ["Sam", "Dana", "Raj"],
      warnings: [],
    });
    expect(
      attendeesHtml.match(/data-testid="live-meeting-participants"[^>]*>(.*?)<\/p>/)?.[1]?.trim(),
    ).toBe("Sam with Dana, Raj");
    expect(
      attendeesHtml.match(
        /data-testid="transcript-channels-outside"[\s\S]*?<h3[^>]*>(.*?)<\/h3>/,
      )?.[1]?.trim(),
    ).toBe("Sam with Dana, Raj");
    expect(attendeesHtml).not.toContain("Me and Others");
  });

  test("AC-B19h: real click sequence (Start recording -> consent_open event -> refused start) shows panel without consent dialog, handles mic-only, Cancel, never-silent, and shortcut retry", async () => {
    const i18n = await makeI18n();
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
    const micOnlyRecordingStatus: MeetingStatus = {
      state: "recording",
      elapsed_secs: 0,
      meeting_id: 101,
      title: "Product sync",
      mic_name: "Built-in Mic",
      mic_only: true,
      warnings: [],
    };

    const renderSection = (tree: React.ReactNode) =>
      renderToStaticMarkup(
        <I18nextProvider i18n={i18n}>{tree}</I18nextProvider>,
      ).replaceAll("&#x27;", "'");

    // Helper: run steps 1-3 cleanly with real event listeners and explicit consent_open delivery.
    async function runRealClickRefusal(refusalError: unknown) {
      invokeCalls.length = 0;
      eventListeners.clear();
      let openConsentCalls = 0;
      let micOnlyCalls = 0;
      let cancelCalls = 0;

      nextInvokeHandler = async (cmd) => {
        if (cmd === "meeting_status") return idleStatus;
        if (cmd === "current_meeting_offer") return null;
        if (cmd === "get_meeting_custom_words") return [];
        if (cmd === "open_meeting_consent") {
          openConsentCalls += 1;
          return consentOpenStatus;
        }
        if (cmd === "confirm_meeting_start") {
          throw refusalError;
        }
        if (cmd === "confirm_meeting_start_mic_only") {
          micOnlyCalls += 1;
          return micOnlyRecordingStatus;
        }
        if (cmd === "cancel_meeting_consent") {
          cancelCalls += 1;
          return idleStatus;
        }
        return null;
      };

      const mounted = withSettings({}, () =>
        mountInteractive(RecordMeetingPage, {}, { runEffects: true }),
      );
      await new Promise((r) => setTimeout(r, 0));
      let section = withSettings({}, () => mounted.rerender()) as Elem;

      // Step 2: Click Start recording (section's onOpenConsent), open_meeting_consent called once,
      // deliver consent_open via MEETING_STATE_EVENT, and assert status.state === "consent_open".
      (section.props.onOpenConsent as () => void)();
      await new Promise((r) => setTimeout(r, 0));
      expect(openConsentCalls).toBe(1);
      emitMockEvent(MEETING_STATE_EVENT, consentOpenStatus);
      section = withSettings({}, () => mounted.rerender()) as Elem;
      expect((section.props.status as MeetingStatus).state).toBe("consent_open");
      expect(section.props.consentOpen).toBe(true);

      // Step 3: confirm_meeting_start returns refusal; controller stays consent_open (no cancel).
      (section.props.onConfirmStart as () => void)();
      await new Promise((r) => setTimeout(r, 0));
      section = withSettings({}, () => mounted.rerender()) as Elem;
      const htmlAfterRefusal = renderSection(section);

      return {
        mounted,
        section,
        htmlAfterRefusal,
        getMicOnlyCalls: () => micOnlyCalls,
        getCancelCalls: () => cancelCalls,
        rerender: () => withSettings({}, () => mounted.rerender()) as Elem,
      };
    }

    // Steps 1-4: permission refusal (`!obj (560947818)`) -> panel shows, dialog gone, consentOpen false,
    // status stays consent_open, then "Record my mic only" starts once with Mic only chip.
    const permRun = await runRealClickRefusal({
      system_audio_tap_failed: {
        cause: "permission",
        call: "create_process_tap",
        code: "!obj (560947818)",
      },
    });
    try {
      expect(permRun.htmlAfterRefusal).toContain('data-testid="meeting-start-problem-panel"');
      expect(permRun.htmlAfterRefusal).not.toContain('data-testid="frame-2-consent"');
      expect(permRun.section.props.consentOpen).toBe(false);
      expect((permRun.section.props.status as MeetingStatus).state).toBe("consent_open");
      expect(permRun.getCancelCalls()).toBe(0);

      // Step 4: Click "Record my mic only".
      (permRun.section.props.onConfirmStartMicOnly as () => void)();
      await new Promise((r) => setTimeout(r, 0));
      expect(permRun.getMicOnlyCalls()).toBe(1);
      const afterMicOnly = permRun.rerender();
      const htmlMicOnly = renderSection(afterMicOnly);
      expect(htmlMicOnly).toContain('data-testid="mic-only-chip"');
      expect(htmlMicOnly).toContain("Mic only");
      expect((afterMicOnly.props.status as MeetingStatus).state).toBe("recording");
      expect((afterMicOnly.props.status as MeetingStatus).mic_only).toBe(true);
    } finally {
      permRun.mounted.unmount();
    }

    // Step 5: Fresh run of steps 1-3 -> click panel's Cancel -> cancel_meeting_consent called once,
    // deliver idle event -> panel goes and "Start now" shows.
    const cancelRun = await runRealClickRefusal({
      system_audio_tap_failed: {
        cause: "permission",
        call: "create_process_tap",
        code: "!obj (560947818)",
      },
    });
    try {
      expect(cancelRun.htmlAfterRefusal).toContain('data-testid="meeting-start-problem-panel"');
      (cancelRun.section.props.onDismissStartRefusal as () => void)();
      await new Promise((r) => setTimeout(r, 0));
      expect(cancelRun.getCancelCalls()).toBe(1);
      emitMockEvent(MEETING_STATE_EVENT, idleStatus);
      const afterCancel = cancelRun.rerender();
      const htmlCancel = renderSection(afterCancel);
      expect(htmlCancel).not.toContain('data-testid="meeting-start-problem-panel"');
      expect(htmlCancel).not.toContain('data-testid="frame-2-consent"');
      expect(htmlCancel).toContain("Start now");
    } finally {
      cancelRun.mounted.unmount();
    }

    // Step 6: Fresh run of steps 1-3 with `system_audio_unavailable` -> never-silent message in page,
    // consent dialog NOT in page.
    const unavailRun = await runRealClickRefusal("system_audio_unavailable");
    try {
      expect(unavailRun.htmlAfterRefusal).toContain("Could not start the meeting.");
      expect(unavailRun.htmlAfterRefusal).toContain('role="alert"');
      expect(unavailRun.htmlAfterRefusal).not.toContain('data-testid="frame-2-consent"');
      expect(unavailRun.section.props.consentOpen).toBe(false);
    } finally {
      unavailRun.mounted.unmount();
    }

    // Step 7: Fresh run of steps 1-3 -> deliver `meetings-open-consent` (shortcut) -> consent dialog
    // shows again and panel goes.
    const shortcutRun = await runRealClickRefusal({
      system_audio_tap_failed: {
        cause: "permission",
        call: "create_process_tap",
        code: "!obj (560947818)",
      },
    });
    try {
      expect(shortcutRun.htmlAfterRefusal).toContain('data-testid="meeting-start-problem-panel"');
      emitMockEvent(MEETINGS_OPEN_CONSENT_EVENT, null);
      const afterShortcut = shortcutRun.rerender();
      const htmlShortcut = renderSection(afterShortcut);
      expect(htmlShortcut).toContain('data-testid="frame-2-consent"');
      expect(htmlShortcut).not.toContain('data-testid="meeting-start-problem-panel"');
      expect(afterShortcut.props.consentOpen).toBe(true);
    } finally {
      shortcutRun.mounted.unmount();
    }
  });
});
