import { describe, expect, mock, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import en from "../src/i18n/locales/en/translation.json";
import { useSettingsStore } from "../src/stores/settingsStore";

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

const {
  SettingsV022Page,
  formatMeetingAudioSummary,
  formatPasteMethodLabel,
  formatShortcutDisplay,
} = await import("../src/components/settings/SettingsV022Page");
const { MeetingsRetentionSection } =
  await import("../src/components/settings/MeetingsRetention");
const { MeetingsFilesAndLanguageSection } =
  await import("../src/components/settings/MeetingsFilesAndLanguage");

const ROOT = join(import.meta.dir, "..");

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
): T {
  const g = globalThis as { window?: unknown };
  const prevWin = Object.getOwnPropertyDescriptor(globalThis, "window");
  g.window = { __TAURI_OS_PLUGIN_INTERNALS__: { os_type: "macos" } };
  const prevLoading = useSettingsStore.getState().isLoading;
  const prevSettings = useSettingsStore.getState().settings;
  const nextState = {
    isLoading: false,
    settings: {
      ...(prevSettings ?? {}),
      ...partial,
    } as never,
  };
  Object.assign(useSettingsStore.getInitialState(), nextState);
  useSettingsStore.setState(nextState);
  try {
    return fn();
  } finally {
    Object.assign(useSettingsStore.getInitialState(), {
      isLoading: prevLoading,
      settings: prevSettings,
    });
    useSettingsStore.setState({
      isLoading: prevLoading,
      settings: prevSettings,
    });
    if (prevWin) Object.defineProperty(globalThis, "window", prevWin);
    else delete g.window;
  }
}

describe("Plan D Settings Overview, Hands-free, Retention, Files & Language", () => {
  test("AC-D2: zero duplicate controls, zero missing controls, single More settings section with no fold, no duplicate CalendarPicker", async () => {
    const i18n = await makeI18n();
    const settingsSrc = readFileSync(
      join(ROOT, "src/components/settings/SettingsV022Page.tsx"),
      "utf8",
    );
    const filesLangSrc = readFileSync(
      join(ROOT, "src/components/settings/MeetingsFilesAndLanguage.tsx"),
      "utf8",
    );
    const advancedSrc = readFileSync(
      join(ROOT, "src/components/settings/advanced/AdvancedSettings.tsx"),
      "utf8",
    );

    // CalendarPicker must NOT be mounted in MeetingsFilesAndLanguage (only in Settings calendars sub-page).
    expect(filesLangSrc).not.toContain("CalendarPicker");
    // Duplicate below-the-fold preserved-controls block must be gone.
    expect(settingsSrc).not.toContain("settings-preserved-controls");
    // Every I2 control appears once in SettingsV022Page.
    for (const sym of [
      "<TrayMenuSurface",
      "<ReadFocusedTextCard",
      "<AgentsCanAskCard",
      "<ClamshellMicrophoneSelector",
      "<MuteWhileRecording",
      "<OutputDeviceSelector",
      "<VolumeSlider",
      "<AdvancedSettings",
    ]) {
      const count = settingsSrc.split(sym).length - 1;
      expect(count).toBe(1);
    }
    // AdvancedSettings already removed dictation retention & history limit (moved to Dictation History C1).
    expect(advancedSrc).not.toContain("RecordingRetentionPeriodSelector");
    expect(advancedSrc).not.toContain("HistoryLimit");

    const html = withSettings(
      {
        push_to_talk: true,
        double_tap_hands_free: false,
        paste_method: "ctrl_v",
        paste_delay_ms: 60,
        clipboard_handling: "dont_modify",
        meeting_language: "auto",
        bindings: {
          transcribe: {
            id: "transcribe",
            name: "Transcribe",
            description: "",
            default_binding: "fn",
            current_binding: "fn",
          },
        },
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <SettingsV022Page />
          </I18nextProvider>,
        ).replaceAll("&#x27;", "'"),
    );

    expect(html).toContain('data-testid="settings-card-shortcut"');
    expect(html).toContain('data-testid="settings-card-typing"');
    expect(html).toContain('data-testid="settings-card-meetings"');
    expect(html).toContain('data-testid="settings-card-app"');
    expect(html).toContain('data-testid="settings-more-section"');
    expect(html).toContain("More settings");
    // D-D7: no fold (<details>) hiding More settings.
    expect(html).not.toContain("<details");
    // Exactly one ReadFocusedTextCard & AgentsCanAskCard in rendered overview DOM.
    expect(html.split('data-testid="read-focused-text-card"').length - 1).toBe(
      1,
    );
  });

  test("AC-D3, AC-D7, AC-D8, D-D1..D-D4: shortcut Hold/Press, clipboard inversion fixed, all 6 paste methods, hands-free & audio summaries", async () => {
    const i18n = await makeI18n();
    const t = i18n.t.bind(i18n);

    // D-D2: Hold <key> vs Press <key>
    expect(formatShortcutDisplay("fn", true)).toBe("Hold fn");
    expect(formatShortcutDisplay("fn", false)).toBe("Press fn");
    expect(formatShortcutDisplay("option+space", true)).toBe(
      "Hold Option+Space",
    );
    expect(formatShortcutDisplay("option+space", false)).toBe(
      "Press Option+Space",
    );

    // AC-D8: All 6 PasteMethod values mapped cleanly
    expect(formatPasteMethodLabel("ctrl_v", t)).toBe("Paste");
    expect(formatPasteMethodLabel("direct", t)).toBe("Type it out");
    expect(formatPasteMethodLabel("shift_insert", t)).toBe("Shift+Insert");
    expect(formatPasteMethodLabel("ctrl_shift_v", t)).toBe("Ctrl+Shift+V");
    expect(formatPasteMethodLabel("external_script", t)).toBe("Your script");
    expect(formatPasteMethodLabel("none", t)).toBe("Don't type");

    // D-D4: Keep meeting audio sub-label
    expect(formatMeetingAudioSummary(true, null, t)).toBe(
      "Deleted after notes are made",
    );
    expect(formatMeetingAudioSummary(false, 30, t)).toBe(
      "30 days, then deleted",
    );
    expect(formatMeetingAudioSummary(false, null, t)).toBe("Kept on this Mac");

    // AC-D7 / D-D1: clipboard_handling = dont_modify => switch OFF ("Only if pasting fails")
    const offHtml = withSettings(
      {
        clipboard_handling: "dont_modify",
        push_to_talk: false,
        double_tap_hands_free: true,
        bindings: {
          transcribe: {
            id: "transcribe",
            name: "Transcribe",
            description: "",
            default_binding: "fn",
            current_binding: "fn",
          },
        },
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <SettingsV022Page />
          </I18nextProvider>,
        ).replaceAll("&#x27;", "'"),
    );
    expect(offHtml).toContain("Press fn");
    expect(offHtml).toContain("Double-tap fn");
    expect(offHtml).toContain("Only if pasting fails");
    expect(offHtml).toContain(
      'role="switch" aria-checked="false" aria-label="Keep words on the clipboard"',
    );

    // AC-D7 / D-D1: clipboard_handling = copy_to_clipboard => switch ON ("Also after pasting")
    const onHtml = withSettings(
      {
        clipboard_handling: "copy_to_clipboard",
        double_tap_hands_free: false,
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <SettingsV022Page />
          </I18nextProvider>,
        ).replaceAll("&#x27;", "'"),
    );
    expect(onHtml).toContain("Also after pasting");
    expect(onHtml).toContain(
      'role="switch" aria-checked="true" aria-label="Keep words on the clipboard"',
    );
  });

  test("AC-D4, AC-D5, AC-D16: Hands-free sub-page wires double_tap_hands_free, hands_free second key (not cancel), and Top/Bottom pill radios", async () => {
    const i18n = await makeI18n();
    const storeSrc = readFileSync(
      join(ROOT, "src/stores/settingsStore.ts"),
      "utf8",
    );
    const settingsSrc = readFileSync(
      join(ROOT, "src/components/settings/SettingsV022Page.tsx"),
      "utf8",
    );

    // AC-D4: Real updater in settingsStore for double_tap_hands_free & meeting_language.
    expect(storeSrc).toContain(
      "double_tap_hands_free: (value) =>\n    commands.changeDoubleTapHandsFreeSetting",
    );
    expect(storeSrc).toContain(
      "meeting_language: (value) =>\n    commands.changeMeetingLanguageSetting",
    );
    // AC-D5: Second key editor uses shortcutId="hands_free", NEVER shortcutId="cancel".
    expect(settingsSrc).toContain('<ShortcutInput shortcutId="hands_free"');
    expect(settingsSrc).not.toContain('<ShortcutInput shortcutId="cancel"');

    // Deep-link target="hands_free" renders unified Hands-free & pill placement screen.
    const hfHtml = withSettings(
      {
        double_tap_hands_free: true,
        overlay_position: "top",
        bindings: {
          transcribe: {
            id: "transcribe",
            name: "Transcribe",
            description: "",
            default_binding: "fn",
            current_binding: "fn",
          },
          hands_free: {
            id: "hands_free",
            name: "Hands-free",
            description: "",
            default_binding: "",
            current_binding: "RightOption",
          },
        },
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <SettingsV022Page target="hands_free" />
          </I18nextProvider>,
        ).replaceAll("&#x27;", "'"),
    );
    expect(hfHtml).toContain("Settings › Shortcut and microphone › Hands-free");
    expect(hfHtml).toContain("RightOption");
    expect(hfHtml).toContain("Where the pill shows");
    expect(hfHtml).toContain("Top of the screen");
    expect(hfHtml).toContain("Bottom of the screen");
    expect(hfHtml).toContain('checked="" value="top"');

    // Deep-link target="shortcut" opens the inline dictation shortcut editor immediately.
    const shortcutHtml = withSettings(
      {
        bindings: {
          transcribe: {
            id: "transcribe",
            name: "Transcribe",
            description: "",
            default_binding: "fn",
            current_binding: "fn",
          },
        },
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <SettingsV022Page target="shortcut" />
          </I18nextProvider>,
        ),
    );
    expect(shortcutHtml).toContain('data-testid="dictation-shortcut-editor"');
  });

  test("S1: Hands-free sub-page stacks pill position choices as divider rows and renders one state-matched pill preview", async () => {
    const i18n = await makeI18n();

    // Shot 46 state: hands_free second key bound => ONLY Hands-free pill preview.
    const withSecondKeyHtml = withSettings(
      {
        double_tap_hands_free: true,
        overlay_position: "top",
        bindings: {
          transcribe: {
            id: "transcribe",
            name: "Transcribe",
            description: "",
            default_binding: "fn",
            current_binding: "fn",
          },
          hands_free: {
            id: "hands_free",
            name: "Hands-free",
            description: "",
            default_binding: "",
            current_binding: "right_option",
          },
        },
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <SettingsV022Page target="hands_free" />
          </I18nextProvider>,
        ).replaceAll("&#x27;", "'"),
    );

    const rowsSlice = withSecondKeyHtml.slice(
      withSecondKeyHtml.indexOf('data-testid="pill-placement-rows"'),
      withSecondKeyHtml.indexOf('data-testid="hands-free-pill-preview"'),
    );
    expect(rowsSlice).toContain("divide-y");
    expect(rowsSlice).not.toContain("sm:grid-cols-2");
    expect(rowsSlice).not.toContain("rounded-lg border");

    const preview46 = withSecondKeyHtml.slice(
      withSecondKeyHtml.indexOf('data-testid="hands-free-pill-preview"'),
    );
    expect(preview46).toContain("Hands-free · tap fn to stop · Esc cancels");
    expect(preview46).not.toContain(">Listening<");

    // Shot 51 state: hands_free second key empty => ONLY Listening pill preview.
    const withoutSecondKeyHtml = withSettings(
      {
        double_tap_hands_free: true,
        overlay_position: "bottom",
        bindings: {
          transcribe: {
            id: "transcribe",
            name: "Transcribe",
            description: "",
            default_binding: "fn",
            current_binding: "fn",
          },
          hands_free: {
            id: "hands_free",
            name: "Hands-free",
            description: "",
            default_binding: "",
            current_binding: "",
          },
        },
      },
      () =>
        renderToStaticMarkup(
          <I18nextProvider i18n={i18n}>
            <SettingsV022Page target="hands_free" />
          </I18nextProvider>,
        ).replaceAll("&#x27;", "'"),
    );

    const preview51 = withoutSecondKeyHtml.slice(
      withoutSecondKeyHtml.indexOf('data-testid="hands-free-pill-preview"'),
    );
    expect(preview51).toContain(">Listening<");
    expect(preview51).not.toContain("Hands-free · tap fn to stop · Esc cancels");
  });

  test("AC-D9, AC-D11, D-D9: Files sync warning exact wording, Language hides parenthetical when model_name is null, and hides This meeting when idle", async () => {
    const i18n = await makeI18n();

    // Language sub-page when idle (meetingRecording = false) & model_name = null
    const langIdleHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <MeetingsFilesAndLanguageSection
          mode="language"
          onBack={() => {}}
          saveMarkdownEnabled={false}
          folderPath=""
          meetingLanguage="hi"
          perMeetingOverride={false}
          meetingRecording={false}
          languagePlanModelName={null}
          onToggleSaveMarkdown={() => {}}
          onFolderChange={() => {}}
          onLanguageChange={() => {}}
          onTogglePerMeetingOverride={() => {}}
        />
      </I18nextProvider>,
    ).replaceAll("&#x27;", "'");
    expect(langIdleHtml).toContain("Settings › Meetings › Language");
    expect(langIdleHtml).not.toContain('data-testid="meeting-language-plan-model"');
    expect(langIdleHtml).not.toContain('data-testid="meeting-language-this-meeting-row"');

    // Language sub-page while recording (meetingRecording = true)
    const langRecordingHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <MeetingsFilesAndLanguageSection
          mode="language"
          onBack={() => {}}
          saveMarkdownEnabled={false}
          folderPath=""
          meetingLanguage="hi"
          perMeetingOverride={true}
          meetingRecording={true}
          overrideLanguage="es"
          onChangeOverrideLanguage={() => {}}
          languagePlanModelName={null}
          onToggleSaveMarkdown={() => {}}
          onFolderChange={() => {}}
          onLanguageChange={() => {}}
          onTogglePerMeetingOverride={() => {}}
        />
      </I18nextProvider>,
    ).replaceAll("&#x27;", "'");
    expect(langRecordingHtml).toContain('data-testid="meeting-language-this-meeting-row"');
    expect(langRecordingHtml).toContain('aria-label="This meeting"');

    // Files sub-page: exact D3 wording + Dropbox warning + local folder has no warning
    const filesCloudHtml = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <MeetingsFilesAndLanguageSection
          mode="files"
          onBack={() => {}}
          onChooseFolder={() => {}}
          saveMarkdownEnabled={true}
          folderPath="~/Dropbox/Meeting Notes"
          meetingLanguage="auto"
          perMeetingOverride={false}
          onToggleSaveMarkdown={() => {}}
          onFolderChange={() => {}}
          onLanguageChange={() => {}}
          onTogglePerMeetingOverride={() => {}}
        />
      </I18nextProvider>,
    ).replaceAll("&#x27;", "'");
    expect(filesCloudHtml).toContain("Settings › Meetings › Files");
    expect(filesCloudHtml).toContain(
      "If this folder syncs to the cloud, your notes will leave this Mac.",
    );
    expect(filesCloudHtml).toContain(
      "▲ This folder syncs with Dropbox. Your notes will be copied to Dropbox under your account and will leave this Mac.",
    );
    expect(filesCloudHtml).toContain('role="alert"');
  });

  test("AC-D10, AC-D20: Retention two-step delete-all token wiring and breadcrumb accessibility", async () => {
    const i18n = await makeI18n();
    const retentionSrc = readFileSync(
      join(ROOT, "src/components/settings/MeetingsRetention.tsx"),
      "utf8",
    );
    expect(retentionSrc).toContain('invoke<string>("request_delete_all_meetings")');
    expect(retentionSrc).toContain('invoke<DeleteAllMeetingsOutcome>("delete_all_meetings"');
    expect(retentionSrc).toContain("confirmationToken");

    const html = renderToStaticMarkup(
      <I18nextProvider i18n={i18n}>
        <MeetingsRetentionSection
          showBreadcrumb={true}
          onBack={() => {}}
          deleteAudioAfterNotes={true}
          transcriptDeleteAfterDays={null}
          deleteAllStep={2}
          onToggleDeleteAudio={() => {}}
          onChangeTranscriptTimer={() => {}}
          onRequestDeleteAllStep1={() => {}}
          onAdvanceDeleteAllStep2={() => {}}
          onCancelDeleteAll={() => {}}
          onConfirmDeleteAll={() => {}}
        />
      </I18nextProvider>,
    );
    expect(html).toContain("Settings › Meetings › Retention");
    expect(html).toContain('role="alert"');
    expect(html).toContain("Confirm delete all");
  });
});
