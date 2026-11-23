import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "../../bindings";
import { commands as meetingsCommands } from "../../bindings.meetings";
import { useSettings } from "../../hooks/useSettings";
import { LANGUAGES } from "../../lib/constants/languages";

const DOT_FILLED = "●";
const DOT_EMPTY = "○";

const SYNC_MARKERS: ReadonlyArray<{ service: string; needle: string }> = [
  { service: "iCloud Drive", needle: "library/mobile documents" },
  { service: "iCloud Drive", needle: "icloud" },
  { service: "Dropbox", needle: "/dropbox" },
  { service: "Google Drive", needle: "google drive" },
  { service: "OneDrive", needle: "onedrive" },
];

export function detectSyncedFolderService(folderPath: string): string | null {
  const lower = folderPath.replace(/\\/g, "/").toLowerCase();
  for (const marker of SYNC_MARKERS) {
    if (lower.includes(marker.needle)) return marker.service;
  }
  return null;
}

export interface MeetingsFilesAndLanguageProps {
  saveMarkdownEnabled: boolean;
  folderPath: string;
  meetingLanguage: string;
  perMeetingOverride: boolean;
  onToggleSaveMarkdown: (next: boolean) => void;
  onFolderChange: (folder: string) => void;
  onLanguageChange: (language: string) => void;
  onTogglePerMeetingOverride: (next: boolean) => void;
  mode?: "all" | "files" | "language";
  onBack?: () => void;
  onChooseFolder?: () => void;
  folderError?: string | null;
  meetingRecording?: boolean;
  overrideLanguage?: string | null;
  onChangeOverrideLanguage?: (language: string | null) => void;
  languagePlanModelName?: string | null;
}

export const MeetingsFilesAndLanguageSection: React.FC<
  MeetingsFilesAndLanguageProps
> = ({
  saveMarkdownEnabled,
  folderPath,
  meetingLanguage,
  perMeetingOverride,
  onToggleSaveMarkdown,
  onFolderChange,
  onLanguageChange,
  onTogglePerMeetingOverride,
  mode = "all",
  onBack,
  onChooseFolder,
  folderError = null,
  meetingRecording,
  overrideLanguage = null,
  onChangeOverrideLanguage,
  languagePlanModelName = null,
}) => {
  const { t } = useTranslation();
  const [langPickerOpen, setLangPickerOpen] = useState(false);
  const [overridePickerOpen, setOverridePickerOpen] = useState(false);
  const syncedService =
    saveMarkdownEnabled && folderPath.trim().length > 0
      ? detectSyncedFolderService(folderPath)
      : null;

  const showFiles = mode === "all" || mode === "files";
  const showLanguage = mode === "all" || mode === "language";
  const showOverrideRow =
    meetingRecording !== undefined ? meetingRecording : mode === "all";
  const meetingLangLabel =
    meetingLanguage === "auto"
      ? t("meetings.storage.autoDetect")
      : LANGUAGES.find((l) => l.value === meetingLanguage)?.label ||
        meetingLanguage;
  const overrideLangLabel = !overrideLanguage
    ? t("meetings.language.sameAsDefault", "Same as default")
    : overrideLanguage === "auto"
      ? t("meetings.storage.autoDetect")
      : LANGUAGES.find((l) => l.value === overrideLanguage)?.label ||
        overrideLanguage;

  return (
    <section
      data-testid={
        mode === "files"
          ? "meetings-files-section"
          : mode === "language"
            ? "meetings-language-section"
            : "meetings-files-language-section"
      }
      className="space-y-5 rounded-xl border border-mid-gray/20 bg-surface p-4"
    >
      {onBack ? (
        <div className="space-y-1">
          <button
            type="button"
            data-testid="settings-back-link"
            onClick={onBack}
            className="inline-flex items-center gap-1 text-[13px] font-semibold text-logo-primary hover:underline cursor-pointer"
          >
            <span aria-hidden="true">←</span>
            <span>{t("sidebar.settings")}</span>
          </button>
          <div>
            <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
              {t("sidebar.settings")} › {t("meetings.sidebar")} ›{" "}
              {mode === "files"
                ? t("v022.settings.filesBreadcrumb", "Files")
                : t("v022.settings.languageBreadcrumb", "Language")}
            </p>
            <h2 className="text-[20px] leading-[26px] font-semibold tracking-[-0.02em] text-text">
              {mode === "files"
                ? t("v022.settings.filesBreadcrumb", "Files")
                : t("v022.settings.languageBreadcrumb", "Language")}
            </h2>
          </div>
        </div>
      ) : null}

      {showFiles ? (
        <div className="space-y-3">
          <div className="flex items-center justify-between gap-4">
            <div>
              <h3 className="text-sm font-semibold text-text">
                {t("meetings.storage.saveMarkdownLabel")}
              </h3>
              <p className="text-xs text-mid-gray">
                {t(
                  "meetings.files.switchDescV020",
                  "Off by default. When on, each finished meeting is also saved as a Markdown file in the folder you choose.",
                )}
              </p>
            </div>
            <div className="flex shrink-0 items-center gap-2.5">
              <span className="inline-flex items-center gap-1 text-xs font-medium text-text/85">
                <span aria-hidden="true">
                  {saveMarkdownEnabled ? DOT_FILLED : DOT_EMPTY}
                </span>
                <span>
                  {saveMarkdownEnabled
                    ? t("v022.settings.stateOn", "On")
                    : t("v022.settings.stateOff", "Off")}
                </span>
              </span>
              <button
                type="button"
                role="switch"
                aria-checked={saveMarkdownEnabled}
                aria-label={t("meetings.storage.saveMarkdownLabel")}
                onClick={() => onToggleSaveMarkdown(!saveMarkdownEnabled)}
                className={`relative inline-flex h-[22px] w-[38px] shrink-0 items-center rounded-full border transition-colors cursor-pointer ${
                  saveMarkdownEnabled
                    ? "border-logo-primary bg-logo-primary"
                    : "border-mid-gray/50 bg-transparent"
                }`}
              >
                <span
                  className={`inline-block h-4 w-4 rounded-full transition-transform ${
                    saveMarkdownEnabled
                      ? "translate-x-[18px] bg-white"
                      : "translate-x-[2px] bg-text"
                  }`}
                />
              </button>
            </div>
          </div>

          <div className="space-y-1.5">
            <label className="block text-xs font-medium text-text/80">
              {t("meetings.storage.folderLabel")}
            </label>
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={folderPath}
                onChange={(e) => onFolderChange(e.target.value)}
                placeholder={t(
                  "meetings.storage.folderPlaceholder",
                  "~/Documents/Coco Voice Notes",
                )}
                className="w-full rounded-lg border border-mid-gray/30 bg-background px-2.5 py-1.5 text-xs text-text font-mono"
              />
              {onChooseFolder ? (
                <button
                  type="button"
                  onClick={onChooseFolder}
                  className="shrink-0 rounded-lg border border-mid-gray/40 px-2.5 py-1.5 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                >
                  {folderPath.trim().length > 0
                    ? t("v022.settings.changeButton", "Change")
                    : t("meetings.storage.chooseFolder")}
                </button>
              ) : null}
            </div>
            <p className="text-xs text-mid-gray">
              {t(
                "meetings.files.subtitleV020",
                "If this folder syncs to the cloud, your notes will leave this Mac. Deleting a meeting in Coco Voice doesn't delete files already saved here.",
              )}
            </p>
          </div>

          {folderError ? (
            <p role="alert" className="text-xs text-red-600 dark:text-red-400">
              {folderError}
            </p>
          ) : null}

          {syncedService ? (
            <div
              role="alert"
              data-testid="synced-folder-warning"
              className="rounded-lg border border-amber-600/45 bg-amber-100/90 p-3 text-xs font-medium text-amber-950 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-200"
            >
              {t("meetings.files.syncWarningV020", {
                service: syncedService,
                defaultValue: `▲ This folder syncs with ${syncedService}. Your notes will be copied to ${syncedService} under your account and will leave this Mac.`,
              })}
            </div>
          ) : null}
        </div>
      ) : null}

      {showLanguage ? (
        <div
          className={`${showFiles ? "border-t border-mid-gray/15 pt-4" : ""} space-y-3`}
        >
          <div className="space-y-2">
            <div className="flex items-center justify-between gap-4">
              <div className="min-w-0">
                <div className="text-sm font-medium text-text">
                  {t("meetings.storage.languageTitle")}
                </div>
                <p className="text-xs text-mid-gray">
                  <span>{meetingLangLabel}</span>
                  {languagePlanModelName ? (
                    <span data-testid="meeting-language-plan-model">
                      {` (${languagePlanModelName})`}
                    </span>
                  ) : null}
                </p>
              </div>
              <button
                type="button"
                aria-expanded={langPickerOpen}
                aria-label={t(
                  "meetings.language.changeAria",
                  "Change Meeting language",
                )}
                onClick={() => setLangPickerOpen((prev) => !prev)}
                className="shrink-0 rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("v022.settings.changeButton", "Change")}
              </button>
            </div>
            {langPickerOpen ? (
              <div
                role="listbox"
                aria-label={t("meetings.storage.languageTitle")}
                data-testid="meeting-language-list"
                className="max-h-52 overflow-y-auto rounded-lg border border-mid-gray/30 bg-background py-1"
              >
                <button
                  type="button"
                  role="option"
                  aria-selected={meetingLanguage === "auto"}
                  onClick={() => {
                    onLanguageChange("auto");
                    setLangPickerOpen(false);
                  }}
                  className={`w-full px-3 py-1.5 text-start text-xs cursor-pointer ${
                    meetingLanguage === "auto"
                      ? "bg-logo-primary/15 font-semibold text-logo-primary"
                      : "text-text hover:bg-mid-gray/10"
                  }`}
                >
                  {t("meetings.storage.autoDetect")}
                </button>
                {LANGUAGES.filter((l) => l.value !== "auto").map((l) => (
                  <button
                    key={l.value}
                    type="button"
                    role="option"
                    aria-selected={meetingLanguage === l.value}
                    onClick={() => {
                      onLanguageChange(l.value);
                      setLangPickerOpen(false);
                    }}
                    className={`w-full px-3 py-1.5 text-start text-xs cursor-pointer ${
                      meetingLanguage === l.value
                        ? "bg-logo-primary/15 font-semibold text-logo-primary"
                        : "text-text hover:bg-mid-gray/10"
                    }`}
                  >
                    {l.label}
                  </button>
                ))}
              </div>
            ) : null}
          </div>

          {showOverrideRow ? (
            <div
              data-testid="meeting-language-this-meeting-row"
              className="space-y-2 border-t border-mid-gray/15 pt-3"
            >
              <div className="flex items-center justify-between gap-4">
                <div className="min-w-0">
                  <p className="text-sm font-medium text-text">
                    {t("meetings.storage.thisMeetingOverride")}
                  </p>
                  <p className="text-xs text-mid-gray">{overrideLangLabel}</p>
                </div>
                {onChangeOverrideLanguage ? (
                  <button
                    type="button"
                    aria-expanded={overridePickerOpen}
                    aria-label={t(
                      "meetings.language.thisMeetingSelectLabel",
                      "This meeting",
                    )}
                    onClick={() => setOverridePickerOpen((prev) => !prev)}
                    className="shrink-0 rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
                  >
                    {t("v022.settings.changeButton", "Change")}
                  </button>
                ) : (
                  <label className="flex items-center gap-2 text-xs text-mid-gray cursor-pointer">
                    <input
                      type="checkbox"
                      checked={perMeetingOverride}
                      onChange={(e) =>
                        onTogglePerMeetingOverride(e.target.checked)
                      }
                    />
                    <span>{t("meetings.storage.thisMeetingOverride")}</span>
                  </label>
                )}
              </div>
              {onChangeOverrideLanguage && overridePickerOpen ? (
                <div
                  role="listbox"
                  aria-label={t(
                    "meetings.language.thisMeetingSelectLabel",
                    "This meeting",
                  )}
                  className="max-h-52 overflow-y-auto rounded-lg border border-mid-gray/30 bg-background py-1"
                >
                  <button
                    type="button"
                    role="option"
                    aria-selected={!overrideLanguage}
                    onClick={() => {
                      onChangeOverrideLanguage(null);
                      setOverridePickerOpen(false);
                    }}
                    className={`w-full px-3 py-1.5 text-start text-xs cursor-pointer ${
                      !overrideLanguage
                        ? "bg-logo-primary/15 font-semibold text-logo-primary"
                        : "text-text hover:bg-mid-gray/10"
                    }`}
                  >
                    {t("meetings.language.sameAsDefault", "Same as default")}
                  </button>
                  <button
                    type="button"
                    role="option"
                    aria-selected={overrideLanguage === "auto"}
                    onClick={() => {
                      onChangeOverrideLanguage("auto");
                      setOverridePickerOpen(false);
                    }}
                    className={`w-full px-3 py-1.5 text-start text-xs cursor-pointer ${
                      overrideLanguage === "auto"
                        ? "bg-logo-primary/15 font-semibold text-logo-primary"
                        : "text-text hover:bg-mid-gray/10"
                    }`}
                  >
                    {t("meetings.storage.autoDetect")}
                  </button>
                  {LANGUAGES.filter((l) => l.value !== "auto").map((l) => (
                    <button
                      key={l.value}
                      type="button"
                      role="option"
                      aria-selected={overrideLanguage === l.value}
                      onClick={() => {
                        onChangeOverrideLanguage(l.value);
                        setOverridePickerOpen(false);
                      }}
                      className={`w-full px-3 py-1.5 text-start text-xs cursor-pointer ${
                        overrideLanguage === l.value
                          ? "bg-logo-primary/15 font-semibold text-logo-primary"
                          : "text-text hover:bg-mid-gray/10"
                      }`}
                    >
                      {l.label}
                    </button>
                  ))}
                </div>
              ) : null}
            </div>
          ) : null}
        </div>
      ) : null}
    </section>
  );
};

async function pickFolderViaDialog(): Promise<string | null> {
  try {
    const dialog = await import("@tauri-apps/plugin-dialog");
    const chosen = await dialog.open({
      directory: true,
      multiple: false,
    });
    if (typeof chosen === "string" && chosen.trim().length > 0) {
      return chosen.trim();
    }
    return null;
  } catch {
    return null;
  }
}

export const MeetingsFilesAndLanguage: React.FC<{
  mode?: "all" | "files" | "language";
  onBack?: () => void;
  meetingRecording?: boolean;
  onPickFolder?: () => Promise<string | null>;
}> = ({
  mode = "all",
  onBack,
  meetingRecording: meetingRecordingProp,
  onPickFolder = pickFolderViaDialog,
}) => {
  const { settings, updateSetting } = useSettings();
  const [saveMarkdown, setSaveMarkdown] = useState(
    Boolean(settings?.save_notes_as_markdown),
  );
  const [folder, setFolder] = useState(settings?.notes_markdown_folder ?? "");
  const [folderError, setFolderError] = useState<string | null>(null);
  const [language, setLanguage] = useState(
    settings?.meeting_language || "auto",
  );
  const [perMeetingOverride, setPerMeetingOverride] = useState(false);
  const [overrideLanguage, setOverrideLanguage] = useState<string | null>(null);
  const [languagePlanModelName, setLanguagePlanModelName] = useState<
    string | null
  >(null);
  const [liveRecording, setLiveRecording] = useState(false);

  useEffect(() => {
    setSaveMarkdown(Boolean(settings?.save_notes_as_markdown));
    if (typeof settings?.notes_markdown_folder === "string") {
      setFolder(settings.notes_markdown_folder);
    }
    if (typeof settings?.meeting_language === "string") {
      setLanguage(settings.meeting_language || "auto");
    }
  }, [
    settings?.save_notes_as_markdown,
    settings?.notes_markdown_folder,
    settings?.meeting_language,
  ]);

  useEffect(() => {
    let mounted = true;
    void meetingsCommands
      .meetingRetentionState()
      .then((res) => {
        if (!mounted || !res) return;
        const extended = res as unknown as {
          save_notes_as_markdown?: boolean;
          notes_markdown_folder?: string | null;
        };
        if (typeof extended.save_notes_as_markdown === "boolean") {
          setSaveMarkdown(extended.save_notes_as_markdown);
        }
        if (typeof extended.notes_markdown_folder === "string") {
          setFolder(extended.notes_markdown_folder);
        }
      })
      .catch(() => {});
    return () => {
      mounted = false;
    };
  }, []);

  useEffect(() => {
    if (mode === "files") return;
    let mounted = true;
    if (meetingRecordingProp === undefined) {
      void meetingsCommands
        .meetingStatus()
        .then((status) => {
          if (!mounted || !status) return;
          setLiveRecording(
            status.state === "recording" || status.state === "paused",
          );
        })
        .catch(() => {
          if (mounted) setLiveRecording(false);
        });
    }
    void commands
      .meetingLanguagePlan()
      .then((res) => {
        if (!mounted || res.status !== "ok") return;
        setLanguagePlanModelName(res.data.model_name ?? null);
      })
      .catch(() => {
        if (mounted) setLanguagePlanModelName(null);
      });
    return () => {
      mounted = false;
    };
  }, [mode, language, overrideLanguage, meetingRecordingProp]);

  const persistFolderSetting = async (enabled: boolean, nextFolder: string) => {
    setFolderError(null);
    const trimmed = nextFolder.trim();
    try {
      await meetingsCommands.changeNotesMarkdownFolderSetting(
        enabled,
        trimmed.length > 0 ? trimmed : null,
      );
      setSaveMarkdown(enabled && trimmed.length > 0);
      setFolder(trimmed);
    } catch (error) {
      setFolderError(
        error instanceof Error ? error.message : String(error),
      );
      if (enabled) {
        setSaveMarkdown(false);
      }
    }
  };

  const handleToggleSaveMarkdown = async (next: boolean) => {
    if (next && folder.trim().length === 0) {
      const picked = await onPickFolder();
      if (!picked || picked.trim().length === 0) {
        // D-D8: cancelling folder chooser leaves the switch OFF.
        setSaveMarkdown(false);
        return;
      }
      await persistFolderSetting(true, picked);
      return;
    }
    setSaveMarkdown(next);
    await persistFolderSetting(next, folder);
  };

  const handleChooseFolder = async () => {
    const picked = await onPickFolder();
    if (!picked || picked.trim().length === 0) return;
    await persistFolderSetting(true, picked);
  };

  const handleFolderChange = (nextFolder: string) => {
    setFolder(nextFolder);
    setFolderError(null);
    void persistFolderSetting(saveMarkdown, nextFolder);
  };

  const handleLanguageChange = (nextLanguage: string) => {
    setLanguage(nextLanguage);
    void updateSetting("meeting_language", nextLanguage);
  };

  const handleOverrideLanguageChange = (nextOverride: string | null) => {
    setOverrideLanguage(nextOverride);
    setPerMeetingOverride(nextOverride !== null);
    void commands.setMeetingLanguageOverrideCmd(nextOverride).catch(() => {});
  };

  const effectiveRecording =
    meetingRecordingProp !== undefined ? meetingRecordingProp : liveRecording;

  return (
    <MeetingsFilesAndLanguageSection
      mode={mode}
      onBack={onBack}
      saveMarkdownEnabled={saveMarkdown}
      folderPath={folder}
      meetingLanguage={language}
      perMeetingOverride={perMeetingOverride}
      onToggleSaveMarkdown={(next) => void handleToggleSaveMarkdown(next)}
      onFolderChange={handleFolderChange}
      onChooseFolder={() => void handleChooseFolder()}
      folderError={folderError}
      onLanguageChange={handleLanguageChange}
      onTogglePerMeetingOverride={(next) => {
        setPerMeetingOverride(next);
        if (!next) {
          handleOverrideLanguageChange(null);
        }
      }}
      meetingRecording={effectiveRecording}
      overrideLanguage={overrideLanguage}
      onChangeOverrideLanguage={handleOverrideLanguageChange}
      languagePlanModelName={languagePlanModelName}
    />
  );
};
