import React, { useEffect, useState } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { LANGUAGES } from "../../lib/constants/languages";
import { applyTheme, THEME_OPTIONS } from "@/lib/utils/theme";
import type { Theme } from "@/bindings";
import { commands as meetingsCommands } from "../../bindings.meetings";
import { useSettings } from "../../hooks/useSettings";
import type { ShellSectionId } from "../layout/shellNavigation";
import { CalendarPicker } from "./CalendarPicker";
import { MeetingsFilesAndLanguage } from "./MeetingsFilesAndLanguage";
import { MeetingsRetention } from "./MeetingsRetention";
import { ShortcutInput } from "./ShortcutInput";
import { MicrophoneSelector } from "./MicrophoneSelector";
import { LanguageSelector } from "./LanguageSelector";
import { PasteMethodSetting } from "./PasteMethod";
import { PasteDelay } from "./debug/PasteDelay";
import { ClamshellMicrophoneSelector } from "./ClamshellMicrophoneSelector";
import { MuteWhileRecording } from "./MuteWhileRecording";
import { OutputDeviceSelector } from "./OutputDeviceSelector";
import { VolumeSlider } from "./VolumeSlider";
import { AgentsCanAskCard } from "./AgentsCanAskCard";
import { ReadFocusedTextCard } from "./ReadFocusedTextCard";
import { AdvancedSettings } from "./advanced/AdvancedSettings";
import { TrayMenuSurface } from "../whats-new/UpdateAndTraySurfaces";

export type SettingsSubPage =
  | null
  | "retention"
  | "files"
  | "language"
  | "calendars"
  | "hands_free";

export type SettingsTarget =
  | null
  | "shortcut"
  | "hands_free"
  | "retention"
  | "files"
  | "language"
  | "calendars"
  | "updates";

const CompactSwitch: React.FC<{
  checked: boolean;
  label: string;
  disabled?: boolean;
  onChange: (next: boolean) => void;
}> = ({ checked, label, disabled = false, onChange }) => (
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={label}
    disabled={disabled}
    onClick={() => onChange(!checked)}
    className={`relative inline-flex h-[22px] w-[38px] shrink-0 items-center rounded-full border transition-colors cursor-pointer disabled:opacity-50 ${
      checked
        ? "border-logo-primary bg-logo-primary"
        : "border-mid-gray/50 bg-transparent"
    }`}
  >
    <span
      className={`inline-block h-4 w-4 rounded-full transition-transform ${
        checked ? "translate-x-[18px] bg-white" : "translate-x-[2px] bg-text"
      }`}
    />
  </button>
);

const PillWave: React.FC = () => (
  <span className="inline-flex items-center gap-[3px]" aria-hidden="true">
    {[6, 12, 18, 10, 20, 14, 8, 16, 6].map((h, idx) => (
      <i
        key={idx}
        style={{ height: `${h}px` }}
        className="block w-[3px] rounded-sm bg-logo-primary"
      />
    ))}
  </span>
);

export function formatShortcutKey(raw: string | undefined): string {
  if (!raw || raw.trim().length === 0) return "";
  return raw
    .split("+")
    .map((part) => {
      const p = part.trim();
      const lower = p.toLowerCase();
      if (lower === "fn") return "fn";
      if (lower === "esc" || lower === "escape") return "Esc";
      if (p.includes("_")) {
        return p
          .split("_")
          .filter(Boolean)
          .map((w) =>
            w.toLowerCase() === "fn"
              ? "fn"
              : w.charAt(0).toUpperCase() + w.slice(1).toLowerCase(),
          )
          .join(" ");
      }
      return p.charAt(0).toUpperCase() + p.slice(1);
    })
    .join("+");
}

export function formatShortcutDisplay(
  raw: string | undefined,
  hold: boolean,
): string {
  const pretty = formatShortcutKey(raw);
  if (!pretty) return "Not set";
  return hold ? `Hold ${pretty}` : `Press ${pretty}`;
}

export function formatPasteMethodLabel(
  pasteMethod: string | undefined,
  t: TFunction,
): string {
  switch (pasteMethod || "ctrl_v") {
    case "ctrl_v":
      return t("v022.settings.pasteLabel", "Paste");
    case "direct":
      return t("v022.settings.pasteDirectLabel", "Type it out");
    case "shift_insert":
      return t("v022.settings.pasteShiftInsertLabel", "Shift+Insert");
    case "ctrl_shift_v":
      return t("v022.settings.pasteCtrlShiftVLabel", "Ctrl+Shift+V");
    case "external_script":
      return t("v022.settings.pasteExternalScriptLabel", "Your script");
    case "none":
      return t("v022.settings.pasteNoneLabel", "Don't type");
    default:
      return t("v022.settings.pasteLabel", "Paste");
  }
}

export function formatMeetingAudioSummary(
  deleteAudioAfterNotes: boolean,
  transcriptDeleteAfterDays: number | null,
  t: TFunction,
): string {
  if (deleteAudioAfterNotes) {
    return t(
      "v022.settings.deletedAfterNotes",
      "Deleted after notes are made",
    );
  }
  if (transcriptDeleteAfterDays !== null && transcriptDeleteAfterDays > 0) {
    return t("v022.settings.keptForDaysThenDeleted", {
      days: transcriptDeleteAfterDays,
      defaultValue: `${transcriptDeleteAfterDays} days, then deleted`,
    });
  }
  return t("v022.settings.keptOnMac", "Kept on this Mac");
}

function resolveInitialSubPage(
  defaultSubPage: SettingsSubPage,
  target: SettingsTarget,
): SettingsSubPage {
  if (
    target === "hands_free" ||
    target === "retention" ||
    target === "files" ||
    target === "language" ||
    target === "calendars"
  ) {
    return target;
  }
  return defaultSubPage;
}

export const SettingsV022Page: React.FC<{
  defaultSubPage?: SettingsSubPage;
  target?: SettingsTarget;
  onNavigate?: (section: ShellSectionId) => void;
  meetingRecording?: boolean;
  onPickMarkdownFolder?: () => Promise<string | null>;
}> = ({
  defaultSubPage = null,
  target = null,
  onNavigate,
  meetingRecording,
  onPickMarkdownFolder,
}) => {
  const { t } = useTranslation();
  const { settings, updateSetting, audioFeedbackEnabled } = useSettings();

  const [activeSub, setActiveSub] = useState<SettingsSubPage>(() =>
    resolveInitialSubPage(defaultSubPage, target),
  );
  const [openEditor, setOpenEditor] = useState<
    | null
    | "shortcut"
    | "mic"
    | "lang"
    | "pasteMethod"
    | "pasteDelay"
    | "secondKey"
  >(() => (target === "shortcut" ? "shortcut" : null));
  const [deleteAudioSummary, setDeleteAudioSummary] = useState(true);
  const [transcriptDeleteDays, setTranscriptDeleteDays] = useState<
    number | null
  >(null);

  useEffect(() => {
    if (
      typeof performance !== "undefined" &&
      typeof performance.mark === "function"
    ) {
      performance.mark("plan-d-settings-first-paint");
    }
  }, []);

  useEffect(() => {
    if (target === "shortcut") {
      setActiveSub(null);
      setOpenEditor("shortcut");
    } else if (
      target === "hands_free" ||
      target === "retention" ||
      target === "files" ||
      target === "language" ||
      target === "calendars"
    ) {
      setActiveSub(target);
      setOpenEditor(null);
    } else if (target === "updates") {
      setActiveSub(null);
    }
  }, [target]);

  useEffect(() => {
    let cancelled = false;
    void meetingsCommands
      .meetingRetentionState()
      .then((res) => {
        if (!cancelled && res) {
          if (typeof res.delete_audio_after_notes === "boolean") {
            setDeleteAudioSummary(res.delete_audio_after_notes);
          }
          const maybeDays = (
            res as unknown as { transcript_delete_after_days?: number | null }
          ).transcript_delete_after_days;
          if (typeof maybeDays === "number" || maybeDays === null) {
            setTranscriptDeleteDays(maybeDays);
          }
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const pushToTalk = settings?.push_to_talk ?? true;
  const transcribeBinding = settings?.bindings?.transcribe?.current_binding;
  const prettyShortcutKey =
    formatShortcutKey(transcribeBinding) || t("v022.settings.yourShortcut", "your shortcut");
  const shortcutSummary = formatShortcutDisplay(transcribeBinding, pushToTalk);
  const micSummary =
    settings?.selected_microphone ||
    t("v022.settings.defaultMicrophone", "System default microphone");
  const selectedLang = settings?.selected_language || "auto";
  const langSummary =
    selectedLang === "auto"
      ? t("meetings.storage.autoDetect")
      : LANGUAGES.find((l) => l.value === selectedLang)?.label || selectedLang;

  const meetingLang = settings?.meeting_language || "auto";
  const meetingLangSummary =
    meetingLang === "auto"
      ? t("meetings.storage.autoDetect")
      : LANGUAGES.find((l) => l.value === meetingLang)?.label || meetingLang;

  const pasteMethodSummary = formatPasteMethodLabel(settings?.paste_method, t);
  const pasteDelayMs = settings?.paste_delay_ms ?? 60;

  // D-D1: Switch ON = copy_to_clipboard ("Also after pasting");
  //       Switch OFF = dont_modify ("Only if pasting fails").
  const keepClipboardAfterPasting =
    (settings?.clipboard_handling || "dont_modify") === "copy_to_clipboard";

  const saveNotesAsMd = Boolean(settings?.save_notes_as_markdown);
  const notesFolder = settings?.notes_markdown_folder || "";
  const selectedCalendars =
    ((settings as Record<string, unknown> | null)?.selected_calendars as
      | string[]
      | undefined) ?? [];

  const autostartEnabled = Boolean(settings?.autostart_enabled);
  const showTrayIcon = settings?.show_tray_icon ?? true;
  const soundsEnabled = Boolean(settings?.audio_feedback ?? true);
  const currentTheme: Theme = settings?.theme ?? "system";
  const updateChecksEnabled = settings?.update_checks_enabled ?? true;

  const handsFreeEnabled = Boolean(
    settings?.double_tap_hands_free ??
      (settings as Record<string, unknown> | null)?.hands_free_enabled,
  );
  const handsFreeSecondKey =
    settings?.bindings?.hands_free?.current_binding || "";
  const handsFreeSummary = handsFreeEnabled
    ? t("v022.settings.handsFreeDoubleTapSummary", {
        key: prettyShortcutKey,
        defaultValue: `Double-tap ${prettyShortcutKey}`,
      })
    : t("v022.settings.off", "Off");
  const pillPosition = settings?.overlay_position === "top" ? "top" : "bottom";

  const toggleInlineEditor = (
    key:
      | "shortcut"
      | "mic"
      | "lang"
      | "pasteMethod"
      | "pasteDelay"
      | "secondKey",
  ) => {
    setOpenEditor((prev) => (prev === key ? null : key));
  };

  if (activeSub === "hands_free") {
    return (
      <div className="max-w-3xl w-full mx-auto space-y-4">
        <section
          data-testid="settings-hands-free-section"
          className="space-y-4"
        >
          <div className="space-y-1">
            <button
              type="button"
              data-testid="settings-back-link"
              onClick={() => {
                setOpenEditor(null);
                setActiveSub(null);
              }}
              className="inline-flex items-center gap-1 text-[13px] font-semibold text-logo-primary hover:underline cursor-pointer"
            >
              <span aria-hidden="true">←</span>
              <span>{t("sidebar.settings")}</span>
            </button>
            <div>
              <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
                {t("sidebar.settings")} ›{" "}
                {t(
                  "v022.settings.shortcutAndMicHeading",
                  "Shortcut and microphone",
                )}{" "}
                › {t("v022.settings.handsFreeTitle", "Hands-free")}
              </p>
              <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
                {t("v022.settings.handsFreeTitle", "Hands-free")}
              </h2>
            </div>
          </div>

          <div className="rounded-xl border border-mid-gray/25 bg-surface px-4 py-2 divide-y divide-mid-gray/15">
            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t("v022.settings.handsFreeTitle", "Hands-free")}
                </div>
                <p className="text-xs text-mid-gray">
                  {t("v022.settings.handsFreeDescWithKey", {
                    key: prettyShortcutKey,
                    defaultValue: `Double-tap ${prettyShortcutKey} to start, tap once to stop, Escape cancels`,
                  })}
                </p>
              </div>
              <CompactSwitch
                checked={handsFreeEnabled}
                label={t("v022.settings.handsFreeTitle", "Hands-free")}
                onChange={(next) =>
                  void updateSetting("double_tap_hands_free", next)
                }
              />
            </div>

            <div className="py-2.5 space-y-2">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-[13.5px] font-medium text-text">
                    {t(
                      "v022.settings.secondKeyHandsFree",
                      "Second key for hands-free",
                    )}
                  </div>
                  <p className="text-xs text-mid-gray">
                    {formatShortcutKey(handsFreeSecondKey) ||
                      t("v022.settings.noneSelected", "None")}
                  </p>
                </div>
                <button
                  type="button"
                  aria-label={t(
                    "v022.settings.chooseSecondKeyAria",
                    "Choose Second key for hands-free",
                  )}
                  onClick={() => toggleInlineEditor("secondKey")}
                  className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                >
                  {t("v022.settings.chooseButton", "Choose")}
                </button>
              </div>
              {openEditor === "secondKey" ? (
                <div className="pt-1" data-testid="second-key-shortcut-editor">
                  <ShortcutInput shortcutId="hands_free" grouped={true} />
                </div>
              ) : null}
            </div>

            <div className="py-3 space-y-1.5">
              <div className="text-[13.5px] font-medium text-text">
                {t("v022.settings.wherePillShows", "Where the pill shows")}
              </div>
              <div
                data-testid="pill-placement-rows"
                className="divide-y divide-mid-gray/15"
              >
                <label className="flex items-center justify-between gap-3 py-2.5 cursor-pointer">
                  <span>
                    <span className="block text-[13px] font-medium text-text">
                      {t("v022.settings.pillTopTitle", "Top of the screen")}
                    </span>
                    <span className="block text-xs text-mid-gray">
                      {t("v022.settings.pillTopDesc", "Under the camera")}
                    </span>
                  </span>
                  <input
                    type="radio"
                    name="pill-placement-mode"
                    value="top"
                    checked={pillPosition === "top"}
                    onChange={() =>
                      void updateSetting("overlay_position", "top")
                    }
                    className="shrink-0 accent-logo-primary"
                  />
                </label>
                <label className="flex items-center justify-between gap-3 py-2.5 cursor-pointer">
                  <span>
                    <span className="block text-[13px] font-medium text-text">
                      {t(
                        "v022.settings.pillBottomTitle",
                        "Bottom of the screen",
                      )}
                    </span>
                    <span className="block text-xs text-mid-gray">
                      {t("v022.settings.pillBottomDesc", "As today")}
                    </span>
                  </span>
                  <input
                    type="radio"
                    name="pill-placement-mode"
                    value="bottom"
                    checked={pillPosition === "bottom"}
                    onChange={() =>
                      void updateSetting("overlay_position", "bottom")
                    }
                    className="shrink-0 accent-logo-primary"
                  />
                </label>
              </div>
            </div>
          </div>

          <div
            data-testid="hands-free-pill-preview"
            className="flex flex-wrap items-center gap-3 pt-1"
          >
            {handsFreeSecondKey.trim().length > 0 ? (
              <span className="inline-flex min-h-[44px] items-center gap-2.5 rounded-full border border-mid-gray/30 bg-surface px-4 py-1.5 text-[13px] font-semibold text-text shadow-sm">
                <PillWave />
                <span>
                  {t("v022.settings.handsFreePillPreviewWithKey", {
                    key: prettyShortcutKey,
                    defaultValue: `Hands-free · tap ${prettyShortcutKey} to stop · Esc cancels`,
                  })}
                </span>
              </span>
            ) : (
              <span className="inline-flex min-h-[44px] items-center gap-2 rounded-full border border-mid-gray/30 bg-surface px-3.5 py-1.5 text-xs font-semibold text-text shadow-sm">
                <PillWave />
                <span>
                  {t("v022.settings.listeningPreview", "Listening")}
                </span>
              </span>
            )}
          </div>
        </section>
      </div>
    );
  }

  if (activeSub === "retention") {
    return (
      <div className="max-w-3xl w-full mx-auto space-y-4">
        <MeetingsRetention
          onBack={() => setActiveSub(null)}
          onRetentionChange={(next) => {
            setDeleteAudioSummary(next.delete_audio_after_notes);
            setTranscriptDeleteDays(next.transcript_delete_after_days);
          }}
        />
      </div>
    );
  }

  if (activeSub === "files") {
    return (
      <div className="max-w-3xl w-full mx-auto space-y-4">
        <MeetingsFilesAndLanguage
          mode="files"
          onBack={() => setActiveSub(null)}
          onPickFolder={onPickMarkdownFolder}
        />
      </div>
    );
  }

  if (activeSub === "language") {
    return (
      <div className="max-w-3xl w-full mx-auto space-y-4">
        <MeetingsFilesAndLanguage
          mode="language"
          onBack={() => setActiveSub(null)}
          meetingRecording={meetingRecording}
        />
      </div>
    );
  }

  if (activeSub === "calendars") {
    return (
      <div className="max-w-3xl w-full mx-auto space-y-4">
        <div className="space-y-1">
          <button
            type="button"
            data-testid="settings-back-link"
            onClick={() => setActiveSub(null)}
            className="inline-flex items-center gap-1 text-[13px] font-semibold text-logo-primary hover:underline cursor-pointer"
          >
            <span aria-hidden="true">←</span>
            <span>{t("sidebar.settings")}</span>
          </button>
          <div>
            <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
              {t("sidebar.settings")} › {t("meetings.sidebar")} ›{" "}
              {t("v022.settings.calendars", "Calendars")}
            </p>
            <h2 className="text-[20px] leading-[26px] font-semibold tracking-[-0.02em] text-text">
              {t("v022.settings.calendars", "Calendars")}
            </h2>
          </div>
        </div>
        <CalendarPicker />
      </div>
    );
  }

  return (
    <div
      data-testid="settings-v022-page"
      className="max-w-5xl w-full mx-auto space-y-6"
    >
      <header className="space-y-1">
        <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
          {t("sidebar.settings")}
        </p>
        <h1 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
          {t("sidebar.settings")}
        </h1>
      </header>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {/* Card 1: Shortcut and microphone */}
        <section
          data-testid="settings-card-shortcut"
          className="rounded-xl border border-mid-gray/25 bg-surface px-4 py-3 flex flex-col"
        >
          <h2 className="text-[15px] font-semibold text-text mb-1">
            {t(
              "v022.settings.shortcutAndMicHeading",
              "Shortcut and microphone",
            )}
          </h2>
          <div className="divide-y divide-mid-gray/15">
            <div className="py-2.5 space-y-2">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-[13.5px] font-medium text-text">
                    {t("v022.settings.dictationShortcut", "Dictation shortcut")}
                  </div>
                  <p
                    data-testid="dictation-shortcut-summary"
                    className="text-xs text-mid-gray"
                  >
                    {shortcutSummary}
                  </p>
                </div>
                <button
                  type="button"
                  aria-label={t(
                    "v022.settings.changeDictationShortcut",
                    "Change Dictation shortcut",
                  )}
                  onClick={() => toggleInlineEditor("shortcut")}
                  className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                >
                  {t("v022.settings.changeButton", "Change")}
                </button>
              </div>
              {openEditor === "shortcut" ? (
                <div className="pt-1" data-testid="dictation-shortcut-editor">
                  <ShortcutInput shortcutId="transcribe" grouped={true} />
                </div>
              ) : null}
            </div>

            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t("v022.settings.holdToTalk", "Hold to talk")}
                </div>
                <p className="text-xs text-mid-gray">
                  {t(
                    "v022.settings.holdToTalkHint",
                    "Off: press once to start, again to stop",
                  )}
                </p>
              </div>
              <CompactSwitch
                checked={pushToTalk}
                label={t("v022.settings.holdToTalk", "Hold to talk")}
                onChange={(next) => void updateSetting("push_to_talk", next)}
              />
            </div>

            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t("v022.settings.handsFreeTitle", "Hands-free")}
                </div>
                <p
                  data-testid="hands-free-summary"
                  className="text-xs text-mid-gray"
                >
                  {handsFreeSummary}
                </p>
              </div>
              <button
                type="button"
                aria-label={t(
                  "v022.settings.openHandsFreeAria",
                  "Open Hands-free",
                )}
                onClick={() => setActiveSub("hands_free")}
                className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("v022.settings.openButton", "Open")}
              </button>
            </div>

            <div className="py-2.5 space-y-2">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-[13.5px] font-medium text-text">
                    {t("v022.settings.microphone", "Microphone")}
                  </div>
                  <p className="text-xs text-mid-gray truncate">{micSummary}</p>
                </div>
                <button
                  type="button"
                  aria-label={t(
                    "v022.settings.changeMicrophone",
                    "Change Microphone",
                  )}
                  onClick={() => toggleInlineEditor("mic")}
                  className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                >
                  {t("v022.settings.changeButton", "Change")}
                </button>
              </div>
              {openEditor === "mic" ? (
                <div className="pt-1">
                  <MicrophoneSelector descriptionMode="tooltip" grouped={true} />
                </div>
              ) : null}
            </div>

            <div className="py-2.5 space-y-2">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-[13.5px] font-medium text-text">
                    {t("v022.settings.language", "Language")}
                  </div>
                  <p className="text-xs text-mid-gray">{langSummary}</p>
                </div>
                <button
                  type="button"
                  aria-label={t(
                    "v022.settings.changeLanguage",
                    "Change Language",
                  )}
                  onClick={() => toggleInlineEditor("lang")}
                  className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                >
                  {t("v022.settings.changeButton", "Change")}
                </button>
              </div>
              {openEditor === "lang" ? (
                <div className="pt-1">
                  <LanguageSelector descriptionMode="tooltip" grouped={true} />
                </div>
              ) : null}
            </div>
          </div>
        </section>

        {/* Card 2: Typing */}
        <section
          data-testid="settings-card-typing"
          className="rounded-xl border border-mid-gray/25 bg-surface px-4 py-3 flex flex-col"
        >
          <h2 className="text-[15px] font-semibold text-text mb-1">
            {t("v022.settings.typingHeading", "Typing")}
          </h2>
          <div className="divide-y divide-mid-gray/15">
            <div className="py-2.5 space-y-2">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-[13.5px] font-medium text-text">
                    {t("v022.settings.howWordsTyped", "How words are typed")}
                  </div>
                  <p
                    data-testid="paste-method-summary"
                    className="text-xs text-mid-gray"
                  >
                    {pasteMethodSummary}
                  </p>
                </div>
                <button
                  type="button"
                  aria-label={t(
                    "v022.settings.changePasteMethod",
                    "Change How words are typed",
                  )}
                  onClick={() => toggleInlineEditor("pasteMethod")}
                  className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                >
                  {t("v022.settings.changeButton", "Change")}
                </button>
              </div>
              {openEditor === "pasteMethod" ? (
                <div className="pt-1">
                  <PasteMethodSetting descriptionMode="tooltip" grouped={true} />
                </div>
              ) : null}
            </div>

            <div className="py-2.5 space-y-2">
              <div className="flex items-center justify-between gap-3">
                <div className="min-w-0">
                  <div className="text-[13.5px] font-medium text-text">
                    {t("v022.settings.pasteDelayLabel", "Paste delay")}
                  </div>
                  <p className="text-xs text-mid-gray">{`${pasteDelayMs} ms`}</p>
                </div>
                <button
                  type="button"
                  aria-label={t(
                    "v022.settings.changePasteDelay",
                    "Change Paste delay",
                  )}
                  onClick={() => toggleInlineEditor("pasteDelay")}
                  className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                >
                  {t("v022.settings.changeButton", "Change")}
                </button>
              </div>
              {openEditor === "pasteDelay" ? (
                <div className="pt-1">
                  <PasteDelay descriptionMode="tooltip" grouped={true} />
                </div>
              ) : null}
            </div>

            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t(
                    "v022.settings.keepClipboard",
                    "Keep words on the clipboard",
                  )}
                </div>
                <p
                  data-testid="clipboard-handling-summary"
                  className="text-xs text-mid-gray"
                >
                  {keepClipboardAfterPasting
                    ? t(
                        "v022.settings.clipboardAlways",
                        "Also after pasting",
                      )
                    : t(
                        "v022.settings.clipboardOnFailOnly",
                        "Only if pasting fails",
                      )}
                </p>
              </div>
              <CompactSwitch
                checked={keepClipboardAfterPasting}
                label={t(
                  "v022.settings.keepClipboard",
                  "Keep words on the clipboard",
                )}
                onChange={(next) =>
                  void updateSetting(
                    "clipboard_handling",
                    next ? "copy_to_clipboard" : "dont_modify",
                  )
                }
              />
            </div>
          </div>
        </section>

        {/* Card 3: Meetings */}
        <section
          data-testid="settings-card-meetings"
          className="rounded-xl border border-mid-gray/25 bg-surface px-4 py-3 flex flex-col"
        >
          <h2 className="text-[15px] font-semibold text-text mb-1">
            {t("meetings.sidebar")}
          </h2>
          <div className="divide-y divide-mid-gray/15">
            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t("v022.settings.keepMeetingAudio", "Keep meeting audio")}
                </div>
                <p
                  data-testid="meeting-audio-summary"
                  className="text-xs text-mid-gray"
                >
                  {formatMeetingAudioSummary(
                    deleteAudioSummary,
                    transcriptDeleteDays,
                    t,
                  )}
                </p>
              </div>
              <button
                type="button"
                aria-label={t(
                  "v022.settings.openKeepMeetingAudio",
                  "Open Keep meeting audio",
                )}
                onClick={() => setActiveSub("retention")}
                className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("v022.settings.openButton", "Open")}
              </button>
            </div>

            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t("v022.settings.notesAsFiles", "Notes as files")}
                </div>
                <p
                  data-testid="notes-as-files-summary"
                  className={`text-xs text-mid-gray truncate ${
                    saveNotesAsMd && notesFolder ? "font-mono" : ""
                  }`}
                >
                  {saveNotesAsMd && notesFolder
                    ? notesFolder
                    : t("v022.settings.off", "Off")}
                </p>
              </div>
              <button
                type="button"
                aria-label={t(
                  "v022.settings.openNotesAsFiles",
                  "Open Notes as files",
                )}
                onClick={() => setActiveSub("files")}
                className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("v022.settings.openButton", "Open")}
              </button>
            </div>

            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t("meetings.language.heading", "Meeting language")}
                </div>
                <p
                  data-testid="meeting-language-summary"
                  className="text-xs text-mid-gray"
                >
                  {meetingLangSummary}
                </p>
              </div>
              <button
                type="button"
                aria-label={t(
                  "v022.settings.openMeetingLanguage",
                  "Open Meeting language",
                )}
                onClick={() => setActiveSub("language")}
                className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("v022.settings.openButton", "Open")}
              </button>
            </div>

            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t("v022.settings.calendars", "Calendars")}
                </div>
                <p className="text-xs text-mid-gray">
                  {selectedCalendars.length > 0
                    ? t("v022.settings.calendarsSelected", {
                        count: selectedCalendars.length,
                        defaultValue: `${selectedCalendars.length} selected`,
                      })
                    : t("v022.settings.noneSelected", "None")}
                </p>
              </div>
              <button
                type="button"
                aria-label={t(
                  "v022.settings.openCalendars",
                  "Open Calendars",
                )}
                onClick={() => setActiveSub("calendars")}
                className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("v022.settings.openButton", "Open")}
              </button>
            </div>

            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t(
                    "v022.settings.meetingWordsAndNames",
                    "Meeting words and names",
                  )}
                </div>
                <p className="text-xs text-mid-gray">
                  {t(
                    "v022.settings.meetingWordsAndNamesDesc",
                    "Names and terms saved for meetings",
                  )}
                </p>
              </div>
              <button
                type="button"
                aria-label={t(
                  "v022.settings.openMeetingWords",
                  "Open Meeting words and names",
                )}
                onClick={() => onNavigate?.("meetings-record")}
                className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("v022.settings.openButton", "Open")}
              </button>
            </div>
          </div>
        </section>

        {/* Card 4: App */}
        <section
          data-testid="settings-card-app"
          className="rounded-xl border border-mid-gray/25 bg-surface px-4 py-3 flex flex-col"
        >
          <h2 className="text-[15px] font-semibold text-text mb-1">
            {t("v022.settings.appHeading", "App")}
          </h2>
          <div className="divide-y divide-mid-gray/15">
            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t("v022.settings.openAtLogin", "Open at login")}
                </div>
              </div>
              <CompactSwitch
                checked={autostartEnabled}
                label={t("v022.settings.openAtLogin", "Open at login")}
                onChange={(next) =>
                  void updateSetting("autostart_enabled", next)
                }
              />
            </div>

            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t("v022.settings.showInMenuBar", "Show in the menu bar")}
                </div>
              </div>
              <CompactSwitch
                checked={showTrayIcon}
                label={t("v022.settings.showInMenuBar", "Show in the menu bar")}
                onChange={(next) => void updateSetting("show_tray_icon", next)}
              />
            </div>

            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t("v022.settings.sounds", "Sounds")}
                </div>
              </div>
              <CompactSwitch
                checked={soundsEnabled}
                label={t("v022.settings.sounds", "Sounds")}
                onChange={(next) => void updateSetting("audio_feedback", next)}
              />
            </div>

            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t("v022.settings.appearance", "Appearance")}
                </div>
              </div>
              <div
                role="group"
                aria-label={t("v022.settings.appearance", "Appearance")}
                data-testid="appearance-segmented"
                className="inline-flex shrink-0 items-center rounded-[8px] border border-mid-gray/25 bg-mid-gray/10 p-0.5 dark:bg-mid-gray/20"
              >
                {THEME_OPTIONS.map((option) => {
                  const selected = currentTheme === option;
                  return (
                    <button
                      key={option}
                      type="button"
                      data-testid={`appearance-option-${option}`}
                      aria-pressed={selected}
                      onClick={() => {
                        applyTheme(option);
                        void updateSetting("theme", option);
                      }}
                      className={`rounded-[6px] px-2.5 py-0.5 text-[12px] font-medium transition-colors cursor-pointer ${
                        selected
                          ? "bg-background text-text shadow-xs dark:bg-surface-elevated dark:text-text"
                          : "text-mid-gray hover:text-text"
                      }`}
                    >
                      {t(`theme.options.${option}`)}
                    </button>
                  );
                })}
              </div>
            </div>

            <div className="flex items-center justify-between gap-3 py-2.5">
              <div className="min-w-0">
                <div className="text-[13.5px] font-medium text-text">
                  {t("v022.settings.checkForUpdates", "Check for updates")}
                </div>
                <p className="text-xs text-mid-gray">
                  {t(
                    "v022.settings.contactsGithubAtLaunch",
                    "Contacts github.com at launch",
                  )}
                </p>
              </div>
              <CompactSwitch
                checked={updateChecksEnabled}
                label={t("v022.settings.checkForUpdates", "Check for updates")}
                onChange={(next) =>
                  void updateSetting("update_checks_enabled", next)
                }
              />
            </div>
          </div>
        </section>
      </div>

      {/* D-D7: Single "More settings" section below the 4 cards, plain heading, NO fold, rendered ONCE */}
      <section
        data-testid="settings-more-section"
        className="space-y-4 pt-2"
      >
        <h2 className="text-[15px] font-semibold text-text">
          {t("v022.settings.moreSettingsHeading", "More settings")}
        </h2>
        <TrayMenuSurface />
        <ReadFocusedTextCard />
        <AgentsCanAskCard />
        <div className="rounded-xl border border-mid-gray/25 bg-surface px-4 py-3 space-y-2">
          <ClamshellMicrophoneSelector
            descriptionMode="tooltip"
            grouped={true}
          />
          <MuteWhileRecording descriptionMode="tooltip" grouped={true} />
          <OutputDeviceSelector
            descriptionMode="tooltip"
            grouped={true}
            disabled={!audioFeedbackEnabled}
          />
          <VolumeSlider disabled={!audioFeedbackEnabled} />
        </div>
        <AdvancedSettings />
      </section>
    </div>
  );
};
