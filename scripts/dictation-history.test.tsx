import { describe, expect, test } from "bun:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import type { CorrectionPair, HistoryEntry } from "../src/bindings";
import { SHELL_ROUTES, Sidebar } from "../src/components/layout/shellRoutes";
import { buildHistoryRowMenuItems } from "../src/components/settings/history/HistoryRow";
import {
  createHistoryCommandHandlers,
  displayTextFor,
  HistorySettings,
  HistorySettingsSection,
  selectDictationEntries,
  type HistoryViewActions,
  type HistoryViewState,
} from "../src/components/settings/history/HistorySettings";

const i18n = i18next.createInstance();
void i18n.use(initReactI18next).init({
  lng: "en",
  resources: { en: { translation: en } },
  interpolation: { escapeValue: false },
});

const REF_NOW = Math.floor(new Date(2026, 9, 27, 11, 0, 0).getTime() / 1000);
const TODAY_941 = Math.floor(new Date(2026, 9, 27, 9, 41, 0).getTime() / 1000);
const YESTERDAY_1730 = Math.floor(
  new Date(2026, 9, 26, 17, 30, 0).getTime() / 1000,
);

const take = (
  id: number,
  timestamp: number,
  raw: string,
  cleaned: string | null,
  appName: string | null,
  saved = false,
): HistoryEntry => ({
  id,
  file_name: `take-${id}.wav`,
  timestamp,
  saved,
  title: `Recording ${id}`,
  transcription_text: raw,
  post_processed_text: cleaned,
  post_process_prompt: cleaned ? "default" : null,
  post_process_requested: Boolean(cleaned),
  app_name: appName,
});

const DICTATION_TAKES = [
  take(
    11,
    TODAY_941,
    "move the cocoa review",
    "Move the cocoa review to Thursday.",
    "Mail",
  ),
  take(10, YESTERDAY_1730, "Thanks, see you at three.", null, null, true),
];

const SAVED_MEETING = {
  id: 99,
  title: "Saved executive sync meeting",
  transcription_text: "Saved executive sync meeting transcript",
  started_at: TODAY_941,
  ended_at: TODAY_941 + 1800,
  notes_ready: true,
  saved: true,
};

function withTauriWindow<T>(
  invokeImpl: (cmd: string, args?: Record<string, unknown>) => unknown,
  fn: () => Promise<T> | T,
): Promise<T> | T {
  const g = globalThis as { window?: unknown };
  const prev = Object.getOwnPropertyDescriptor(globalThis, "window");
  g.window = {
    __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" },
    __TAURI_INTERNALS__: {
      invoke: (cmd: string, args?: Record<string, unknown>) =>
        Promise.resolve(invokeImpl(cmd, args)),
    },
  };
  const restore = () => {
    if (prev) Object.defineProperty(globalThis, "window", prev);
    else delete g.window;
  };
  try {
    const out = fn();
    if (out instanceof Promise) return out.finally(restore);
    restore();
    return out;
  } catch (err) {
    restore();
    throw err;
  }
}

function renderHtml(node: React.ReactElement): string {
  return renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>{node}</I18nextProvider>,
  )
    .replaceAll("&#x27;", "'")
    .replaceAll("&quot;", '"');
}

describe("Dictation history under Voice (V-L4 Page 6 + G2 More actions)", () => {
  test("renders compact Page 6 rows, quiet Open recordings folder left of Kept 30 days, Kept suffix on line 2, Copy/Play/More buttons, and excludes meetings", () => {
    const spec = SHELL_ROUTES.find((r) => r.id === "voice-history");
    expect(spec?.group).toBe("voice");
    expect(spec?.domain).toBe("voice");
    const nav = renderHtml(
      <Sidebar activeSection="voice-history" onSectionChange={() => {}} />,
    );
    expect(nav).toContain('data-nav-id="voice-history" aria-current="page"');

    const mixed = [...DICTATION_TAKES, SAVED_MEETING];
    expect(selectDictationEntries(mixed, "").map((e) => e.id)).toEqual([
      11, 10,
    ]);

    const html = withTauriWindow(
      () => null,
      () =>
        renderHtml(
          <HistorySettings
            initialEntries={mixed}
            initialOpenMenuId={11}
            nowTimestamp={REF_NOW}
          />,
        ),
    ) as string;

    expect(html).toContain('data-testid="dictation-history-section"');
    expect(html).toContain(">Voice<");
    expect(html).toContain("Dictation history");

    const openIdx = html.indexOf(">Open recordings folder</button>");
    const keptDaysIdx = html.indexOf(">Kept 30 days</span>");
    expect(openIdx).toBeGreaterThan(-1);
    expect(keptDaysIdx).toBeGreaterThan(openIdx);

    expect(html).toContain('>"Move the cocoa review to Thursday."<');
    expect(html).toContain(">Mail</span> · Today 9:41 · Cleaned up<");
    expect(html).toContain('>"Thanks, see you at three."<');
    expect(html).toContain(">Yesterday 17:30 · Kept</p>");

    expect(html).toContain(">Copy</button>");
    expect(html).toContain(">Play</button>");
    expect(html).toContain(">More</button>");

    const menuStart = html.indexOf('role="menu"');
    expect(menuStart).toBeGreaterThan(-1);
    const menuSlice = html.slice(menuStart, html.indexOf("</div>", menuStart));
    const labels = Array.from(
      menuSlice.matchAll(/role="menuitem"[^>]*>([^<]+)<\/button>/g),
    ).map((m) => m[1]);
    expect(labels).toEqual(["Keep", "Edit text", "Try again", "Delete"]);
    expect(html).not.toContain("Saved executive sync meeting");
  });

  test("selecting Keep, Edit text, Try again, Delete from More menu and Open recordings folder calls each command, removes deleted row, and updates corrections", async () => {
    const calls: Array<{ cmd: string; args?: Record<string, unknown> }> = [];
    let storedCorrections: CorrectionPair[] = [];

    await withTauriWindow(
      (cmd, args) => {
        calls.push({ cmd, args });
        if (cmd === "get_app_settings") {
          return { corrections: storedCorrections };
        }
        if (cmd === "update_corrections") {
          storedCorrections = (args?.corrections as CorrectionPair[]) ?? [];
        }
        return null;
      },
      async () => {
        let entries = DICTATION_TAKES.map((e) => ({ ...e }));
        const state: HistoryViewState = {
          entries,
          loading: false,
          query: "",
          openMenuId: null,
          editingId: null,
          editDraft: "",
          savingEdit: false,
        };
        const handlers = createHistoryCommandHandlers({
          getEntries: () => entries,
          setEntries: (fn) => {
            entries = fn(entries);
            state.entries = entries;
          },
          t: (k) => i18n.t(k),
        });
        const actions: HistoryViewActions = {
          setQuery: (q) => {
            state.query = q;
          },
          setOpenMenuId: (id) => {
            state.openMenuId = id;
          },
          startEdit: (entry) => {
            state.editingId = entry.id;
            state.editDraft = displayTextFor(entry);
          },
          setEditDraft: (d) => {
            state.editDraft = d;
          },
          cancelEdit: () => {
            state.editingId = null;
          },
          commitEdit: async (id, draft) => {
            await handlers.saveEditAndLearn(id, draft);
            state.editingId = null;
          },
          toggleSaved: handlers.toggleSaved,
          copyText: async () => {},
          deleteEntry: handlers.deleteAudioEntry,
          retryEntry: handlers.retryHistoryEntry,
          openFolder: handlers.openRecordingsFolder,
          getAudioUrl: async () => null,
        };

        await actions.openFolder();
        expect(calls.some((c) => c.cmd === "open_recordings_folder")).toBe(
          true,
        );

        const menuForRow11 = () => {
          const row = state.entries.find((e) => e.id === 11)!;
          return buildHistoryRowMenuItems(
            {
              entry: row,
              onMenuClose: () => actions.setOpenMenuId(null),
              onToggleSaved: () => void actions.toggleSaved(11),
              onStartEdit: () => actions.startEdit(row),
              retryTranscription: actions.retryEntry,
              deleteAudio: actions.deleteEntry,
            },
            (k) => i18n.t(k),
            true,
            false,
            () => {},
          );
        };

        expect(menuForRow11().map((i) => i.label)).toEqual([
          "Keep",
          "Edit text",
          "Try again",
          "Delete",
        ]);

        // 1) Click More -> Keep
        actions.setOpenMenuId(11);
        menuForRow11()[0]!.onSelect();
        await Promise.resolve();
        expect(state.openMenuId).toBeNull();
        expect(calls).toContainEqual({
          cmd: "toggle_history_entry_saved",
          args: { id: 11 },
        });
        expect(
          renderHtml(
            <HistorySettingsSection
              state={state}
              actions={actions}
              nowTimestamp={REF_NOW}
            />,
          ),
        ).toContain(">Mail</span> · Today 9:41 · Cleaned up · Kept<");

        // 2) Click More -> Edit text -> Save updates corrections
        actions.setOpenMenuId(11);
        menuForRow11()[1]!.onSelect();
        expect(state.editingId).toBe(11);
        actions.setEditDraft("Move the Coco review to Thursday.");
        await actions.commitEdit(11, state.editDraft);
        expect(calls).toContainEqual({
          cmd: "update_history_entry_text",
          args: { id: 11, text: "Move the Coco review to Thursday." },
        });
        expect(storedCorrections).toEqual([{ from: "cocoa", to: "Coco" }]);

        // 3) Click More -> Try again
        actions.setOpenMenuId(11);
        menuForRow11()[2]!.onSelect();
        await Promise.resolve();
        expect(calls).toContainEqual({
          cmd: "retry_history_entry_transcription",
          args: { id: 11 },
        });

        // 4) Click More -> Delete removes row
        actions.setOpenMenuId(11);
        menuForRow11()[3]!.onSelect();
        await Promise.resolve();
        expect(calls).toContainEqual({
          cmd: "delete_history_entry",
          args: { id: 11 },
        });
        const afterDelete = renderHtml(
          <HistorySettingsSection
            state={state}
            actions={actions}
            nowTimestamp={REF_NOW}
          />,
        );
        expect(afterDelete).not.toContain("Move the Coco review");
        expect(afterDelete).toContain("Thanks, see you at three.");
      },
    );
  });
});
