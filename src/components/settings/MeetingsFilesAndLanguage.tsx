import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { SUPPORTED_LANGUAGES } from "../../i18n";
import { SettingsGroup } from "../ui/SettingsGroup";
import { ToggleSwitch } from "../ui/ToggleSwitch";

const WARN_TRIANGLE = "▲";
const DEFAULT_LOCAL_FOLDER = "~/Documents/Coco Voice Meetings";

const SYNC_MATCHERS: Array<[RegExp, string]> = [
  [/mobile documents|icloud/i, "iCloud Drive"],
  [/dropbox/i, "Dropbox"],
  [/onedrive/i, "OneDrive"],
  [/google drive|cloudstorage\/googledrive/i, "Google Drive"],
];

export function detectSyncedFolderService(folderPath: string): string | null {
  for (const [pattern, service] of SYNC_MATCHERS) {
    if (pattern.test(folderPath)) return service;
  }
  return null;
}

export interface MeetingsFilesAndLanguageSectionProps {
  saveMarkdownEnabled: boolean;
  folderPath: string;
  meetingLanguage: string;
  perMeetingOverride: boolean;
  onToggleSaveMarkdown: (enabled: boolean) => void;
  onFolderChange: (path: string) => void;
  onChooseFolder?: () => void;
  onLanguageChange: (lang: string) => void;
  onTogglePerMeetingOverride: (enabled: boolean) => void;
}

export const MeetingsFilesAndLanguageSection: React.FC<
  MeetingsFilesAndLanguageSectionProps
> = ({
  saveMarkdownEnabled,
  folderPath,
  meetingLanguage,
  perMeetingOverride,
  onToggleSaveMarkdown,
  onFolderChange,
  onChooseFolder,
  onLanguageChange,
  onTogglePerMeetingOverride,
}) => {
  const { t } = useTranslation();
  const syncedService = detectSyncedFolderService(folderPath);
  const langOptions = SUPPORTED_LANGUAGES.map((l) => (
    <option key={l.code} value={l.code}>
      {l.nativeName}
    </option>
  ));

  return (
    <div data-testid="frame-12-files-and-language" className="space-y-4">
      <SettingsGroup title={t("meetings.storage.filesTitle")}>
        <ToggleSwitch
          checked={saveMarkdownEnabled}
          onChange={onToggleSaveMarkdown}
          label={t("meetings.storage.saveMarkdownLabel")}
          description={t("meetings.storage.saveMarkdownDescription")}
          descriptionMode="inline"
          grouped={true}
        />

        <div className="space-y-2 px-4 py-3 border-t border-mid-gray/15">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <label
              htmlFor="meetings-markdown-folder"
              className="text-sm font-medium text-text"
            >
              {t("meetings.storage.folderLabel")}
            </label>
            <div className="flex items-center gap-2">
              <input
                id="meetings-markdown-folder"
                type="text"
                value={folderPath}
                onChange={(e) => onFolderChange(e.target.value)}
                className="w-64 rounded-lg border border-mid-gray/35 bg-background px-2.5 py-1 font-mono text-xs text-text"
              />
              <button
                type="button"
                onClick={onChooseFolder}
                className="rounded-lg border border-mid-gray/45 px-2.5 py-1 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
              >
                {t("meetings.storage.chooseFolder")}
              </button>
            </div>
          </div>

          <p className="text-xs text-mid-gray">
            {t("meetings.storage.syncRule")}
          </p>

          {syncedService ? (
            <p
              role="status"
              data-testid="synced-folder-warning"
              className="rounded-lg border border-amber-500/35 bg-amber-500/10 px-3 py-1.5 text-xs font-medium text-text"
            >
              <span aria-hidden="true">{WARN_TRIANGLE}</span>{" "}
              {t("meetings.storage.syncWarning", { service: syncedService })}
            </p>
          ) : null}
        </div>
      </SettingsGroup>

      <SettingsGroup title={t("meetings.storage.languageTitle")}>
        <div className="flex items-center justify-between gap-4 px-4 py-3">
          <label
            htmlFor="meeting-language-select"
            className="text-sm font-medium text-text"
          >
            {t("meetings.storage.languageTitle")}
          </label>
          <select
            id="meeting-language-select"
            value={meetingLanguage}
            onChange={(e) => onLanguageChange(e.target.value)}
            className="rounded-lg border border-mid-gray/35 bg-background px-2.5 py-1 text-xs font-medium text-text"
          >
            <option value="auto">{t("meetings.storage.autoDetect")}</option>
            {langOptions}
          </select>
        </div>
        <ToggleSwitch
          checked={perMeetingOverride}
          onChange={onTogglePerMeetingOverride}
          label={t("meetings.storage.thisMeetingOverride")}
          description={t("meetings.storage.autoDetect")}
          descriptionMode="inline"
          grouped={true}
        />
      </SettingsGroup>
    </div>
  );
};

export const MeetingsFilesAndLanguage: React.FC = () => {
  const [saveMd, setSaveMd] = useState(false);
  const [folder, setFolder] = useState(DEFAULT_LOCAL_FOLDER);
  const [lang, setLang] = useState("auto");
  const [override, setOverride] = useState(false);

  return (
    <MeetingsFilesAndLanguageSection
      saveMarkdownEnabled={saveMd}
      folderPath={folder}
      meetingLanguage={lang}
      perMeetingOverride={override}
      onToggleSaveMarkdown={setSaveMd}
      onFolderChange={setFolder}
      onLanguageChange={setLang}
      onTogglePerMeetingOverride={setOverride}
    />
  );
};
