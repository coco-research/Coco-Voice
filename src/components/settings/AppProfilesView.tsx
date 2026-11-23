import React, { useEffect, useState } from "react";
import type { TFunction } from "i18next";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import {
  commands,
  type AppProfile,
  type AppTone,
  type CapitalizationStyle,
  type PunctuationStyle,
} from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { useSettingsStore } from "../../stores/settingsStore";
import { OUTLINED_BTN_SM } from "../meetings/MeetingsList";
import type { ShellSectionId } from "../layout/shellRoutes";
import {
  NeverDictateCard,
  type BlockedSiteEntry,
} from "./NeverDictateCard";

export type CleanupLevelChoice = "default" | "on" | "light" | "off";

export type AppProfileWithCleanup = AppProfile & {
  cleanup?: CleanupLevelChoice;
  repo_words?: boolean | null;
};

const MID_SEP = " · ";
const INPUT_CLS =
  "rounded-[10px] border border-mid-gray/40 bg-background px-3 py-1.5 text-sm text-text";
const TONE_KEYS: Partial<Record<AppTone, string>> = {
  formal: "profiles.toneFormal",
  casual: "profiles.toneCasual",
  direct: "profiles.toneDirect",
  code: "profiles.toneCode",
  auto: "profiles.toneAuto",
};
const TONE_PRESETS: Record<
  AppTone,
  {
    capitalization: CapitalizationStyle;
    punctuation: PunctuationStyle;
    post_process_enabled: boolean | null;
    cleanup: CleanupLevelChoice;
    repo_words: boolean | null;
  }
> = {
  formal: {
    capitalization: "sentence",
    punctuation: "full",
    post_process_enabled: true,
    cleanup: "on",
    repo_words: null,
  },
  direct: {
    capitalization: "sentence",
    punctuation: "full",
    post_process_enabled: true,
    cleanup: "on",
    repo_words: null,
  },
  casual: {
    capitalization: "lowercase",
    punctuation: "drop_trailing_period",
    post_process_enabled: true,
    cleanup: "on",
    repo_words: null,
  },
  code: {
    capitalization: "default",
    punctuation: "drop_trailing_period",
    post_process_enabled: false,
    cleanup: "light",
    repo_words: true,
  },
  default: {
    capitalization: "default",
    punctuation: "default",
    post_process_enabled: false,
    cleanup: "off",
    repo_words: null,
  },
  auto: {
    capitalization: "default",
    punctuation: "default",
    post_process_enabled: true,
    cleanup: "default",
    repo_words: null,
  },
};

export function effectiveCleanupLevel(
  profile: AppProfileWithCleanup,
): CleanupLevelChoice {
  if (profile.cleanup && profile.cleanup !== "default") {
    return profile.cleanup;
  }
  if (profile.post_process_enabled === true) return "on";
  if (profile.post_process_enabled === false) return "off";
  return "default";
}

export function summarizeAppProfile(
  profile: AppProfileWithCleanup,
  t: TFunction | ((key: string, defaultValue?: string) => string),
): string {
  const tr = (key: string, defaultValue?: string) =>
    (t as (k: string, d?: string) => string)(key, defaultValue);
  const level = effectiveCleanupLevel(profile);
  const cleanup =
    level === "off"
      ? tr("profiles.cleanupOff", "clean-up off")
      : level === "light"
        ? tr("profiles.cleanupLight", "clean-up light")
        : level === "on"
          ? tr("profiles.cleanupOn", "clean-up on")
          : tr("profiles.cleanupDefault", "clean-up default");
  const tone = profile.tone ?? "auto";
  if (level === "off" && tone === "default") {
    return `${tr("profiles.exactWords", "Exactly what you said")}${MID_SEP}${cleanup}`;
  }
  const parts: string[] = [];
  if (TONE_KEYS[tone]) parts.push(tr(TONE_KEYS[tone]!));
  const caps = profile.capitalization ?? "default";
  const punct = profile.punctuation ?? "default";
  if (
    tone !== "code" &&
    (punct === "drop_trailing_period" ||
      (punct === "default" && tone === "casual"))
  ) {
    parts.push(tr("profiles.noTrailingStop", "no full stop at the end"));
  } else if (
    (caps === "sentence" && punct === "full") ||
    (caps === "default" &&
      punct === "default" &&
      (tone === "formal" || tone === "direct"))
  ) {
    parts.push(tr("profiles.capsAndPunct", "capitals and punctuation"));
  }
  if (profile.repo_words === true) {
    parts.push(tr("profiles.repoWordsOn", "repo words on"));
  } else if (profile.repo_words === false) {
    parts.push(tr("profiles.repoWordsOff", "repo words off"));
  }
  parts.push(cleanup);
  return parts.join(MID_SEP);
}

export interface AppProfilesSectionProps {
  profiles: AppProfileWithCleanup[];
  blockedApps: string[];
  blockedSites?: BlockedSiteEntry[];
  displayNames?: Record<string, string>;
  onSaveProfiles: (next: AppProfileWithCleanup[]) => Promise<void>;
  onSaveBlockedApps: (next: string[]) => Promise<void>;
  onSaveBlockedSites?: (next: BlockedSiteEntry[]) => Promise<void>;
  onReadFrontApp?: () => Promise<{
    app_name: string;
    bundle_id: string | null;
  } | null>;
  onNavigate?: (s: ShellSectionId) => void;
}

export const AppProfilesSection: React.FC<AppProfilesSectionProps> = ({
  profiles,
  blockedApps,
  blockedSites = [],
  displayNames = {},
  onSaveProfiles,
  onSaveBlockedApps,
  onSaveBlockedSites,
  onReadFrontApp,
  onNavigate,
}) => {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<AppProfileWithCleanup | null>(null);
  const visibleProfiles = profiles.filter((p) => !p.blocked);

  const openNewProfile = () =>
    setDraft({
      id: `profile-${Date.now()}`,
      name: "",
      app_identifier: "",
      tone: "auto",
      ...TONE_PRESETS.auto,
    });

  const fillFromFrontApp = async () => {
    const info = await onReadFrontApp?.();
    if (!info || !draft) return;
    setDraft({
      ...draft,
      name: draft.name || info.app_name,
      app_identifier: info.bundle_id || info.app_name,
    });
  };

  const saveDraft = async () => {
    if (!draft?.app_identifier.trim()) return;
    const id = draft.app_identifier.trim();
    const level = effectiveCleanupLevel(draft);
    const clean: AppProfileWithCleanup = {
      ...draft,
      name: draft.name.trim() || id,
      app_identifier: id,
      cleanup: level,
      post_process_enabled:
        level === "on" ? true : level === "off" || level === "light" ? false : null,
    };
    const next = profiles.some((p) => p.id === clean.id)
      ? profiles.map((p) => (p.id === clean.id ? clean : p))
      : [...profiles, clean];
    await onSaveProfiles(next);
    setDraft(null);
  };

  return (
    <div
      data-testid="per-app-profiles-section"
      className="max-w-3xl w-full mx-auto"
    >
      <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
        {t("sidebar.voiceSection")}
      </p>
      <header className="mt-1 flex items-center justify-between gap-3">
        <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
          {t("sidebar.perAppProfiles")}
        </h2>
        <button
          type="button"
          onClick={openNewProfile}
          className={OUTLINED_BTN_SM}
        >
          {t("profiles.addApp")}
        </button>
      </header>
      <p className="mt-1.5 text-[13px] leading-[18px] text-mid-gray">
        {t("profiles.subtitle")}
      </p>

      {draft ? (
        <div
          data-testid="profile-edit-form"
          className="mt-3 grid gap-2.5 rounded-xl border border-mid-gray/30 bg-background p-3.5"
        >
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
            <input
              type="text"
              value={draft.name}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              placeholder={t("profiles.namePlaceholder")}
              aria-label={t("profiles.namePlaceholder")}
              className={INPUT_CLS}
            />
            <input
              type="text"
              value={draft.app_identifier}
              onChange={(e) =>
                setDraft({ ...draft, app_identifier: e.target.value })
              }
              placeholder={t("profiles.appIdPlaceholder")}
              aria-label={t("profiles.appIdPlaceholder")}
              className={INPUT_CLS}
            />
          </div>
          <div className="flex flex-wrap items-center gap-2.5">
            <label className="flex items-center gap-2 text-xs font-medium text-mid-gray">
              <span>{t("profiles.toneLabel")}</span>
              <select
                aria-label={t("profiles.toneLabel")}
                value={draft.tone ?? "auto"}
                onChange={(e) => {
                  const tone = e.target.value as AppTone;
                  setDraft({ ...draft, tone, ...TONE_PRESETS[tone] });
                }}
                className="rounded-[10px] border border-mid-gray/40 bg-background px-2.5 py-1 text-xs text-text"
              >
                <option value="auto">{t("profiles.toneAuto")}</option>
                <option value="formal">{t("profiles.toneFormal")}</option>
                <option value="direct">{t("profiles.toneDirect")}</option>
                <option value="casual">{t("profiles.toneCasual")}</option>
                <option value="code">{t("profiles.toneCode")}</option>
                <option value="default">{t("profiles.exactWords")}</option>
              </select>
            </label>

            <label className="flex items-center gap-2 text-xs font-medium text-mid-gray">
              <span>{t("profiles.cleanupLabel", "Clean-up")}</span>
              <select
                aria-label={t("profiles.cleanupLabel", "Clean-up")}
                value={effectiveCleanupLevel(draft)}
                onChange={(e) => {
                  const cleanup = e.target.value as CleanupLevelChoice;
                  setDraft({
                    ...draft,
                    cleanup,
                    post_process_enabled:
                      cleanup === "on"
                        ? true
                        : cleanup === "off" || cleanup === "light"
                          ? false
                          : null,
                  });
                }}
                className="rounded-[10px] border border-mid-gray/40 bg-background px-2.5 py-1 text-xs text-text"
              >
                <option value="default">
                  {t("profiles.sameAsDefault", "Same as default")}
                </option>
                <option value="on">{t("profiles.cleanupOptionOn", "On")}</option>
                <option value="light">
                  {t("profiles.cleanupOptionLight", "Light")}
                </option>
                <option value="off">
                  {t("profiles.cleanupOptionOff", "Off")}
                </option>
              </select>
            </label>

            <label className="flex items-center gap-2 text-xs font-medium text-mid-gray">
              <span>
                {t("profiles.repoWordsLabel", "Words from the open repo")}
              </span>
              <select
                aria-label={t(
                  "profiles.repoWordsLabel",
                  "Words from the open repo",
                )}
                value={
                  draft.repo_words === true
                    ? "on"
                    : draft.repo_words === false
                      ? "off"
                      : "default"
                }
                onChange={(e) => {
                  const v = e.target.value;
                  setDraft({
                    ...draft,
                    repo_words: v === "on" ? true : v === "off" ? false : null,
                  });
                }}
                className="rounded-[10px] border border-mid-gray/40 bg-background px-2.5 py-1 text-xs text-text"
              >
                <option value="default">
                  {t("profiles.sameAsDefault", "Same as default")}
                </option>
                <option value="on">{t("profiles.cleanupOptionOn", "On")}</option>
                <option value="off">
                  {t("profiles.cleanupOptionOff", "Off")}
                </option>
              </select>
            </label>

            {onReadFrontApp ? (
              <button
                type="button"
                onClick={() => void fillFromFrontApp()}
                className={OUTLINED_BTN_SM}
              >
                {t("profiles.useFrontApp")}
              </button>
            ) : null}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => void saveDraft()}
              disabled={!draft.app_identifier.trim()}
              className={OUTLINED_BTN_SM}
            >
              {t("settings.history.saveEdit")}
            </button>
            <button
              type="button"
              onClick={() => setDraft(null)}
              className={OUTLINED_BTN_SM}
            >
              {t("tray.cancel")}
            </button>
            {profiles.some((p) => p.id === draft.id) ? (
              <button
                type="button"
                onClick={() => {
                  void onSaveProfiles(
                    profiles.filter((p) => p.id !== draft.id),
                  );
                  setDraft(null);
                }}
                className={OUTLINED_BTN_SM}
              >
                {t("profiles.removeProfile", "Remove profile")}
              </button>
            ) : null}
          </div>
        </div>
      ) : null}

      <div className="mt-3 divide-y divide-mid-gray/20 border-b-[1.5px] border-text/80">
        {visibleProfiles.map((p) => (
          <div
            key={p.id}
            className="flex items-center justify-between gap-4 py-3"
          >
            <div className="min-w-0">
              <p className="text-[14px] font-semibold text-text">
                {p.name || p.app_identifier}
              </p>
              <p className="mt-0.5 text-[13px] text-mid-gray">
                {summarizeAppProfile(p, t)}
              </p>
            </div>
            <button
              type="button"
              onClick={() => setDraft({ ...p })}
              className={OUTLINED_BTN_SM}
            >
              {t("profiles.edit")}
            </button>
          </div>
        ))}
        <div className="flex items-center justify-between gap-4 py-3">
          <div className="min-w-0">
            <p className="text-[14px] font-semibold text-text">
              {t("profiles.defaultName")}
            </p>
            <p className="mt-0.5 text-[13px] text-mid-gray">
              {t("profiles.defaultDesc")}
            </p>
          </div>
          <button
            type="button"
            onClick={() => onNavigate?.("voice-words")}
            className={OUTLINED_BTN_SM}
          >
            {t("profiles.edit")}
          </button>
        </div>
      </div>

      <NeverDictateCard
        blockedApps={blockedApps}
        blockedSites={blockedSites}
        displayNames={displayNames}
        onSaveBlockedApps={onSaveBlockedApps}
        onSaveBlockedSites={onSaveBlockedSites}
        onReadFrontApp={onReadFrontApp}
      />
    </div>
  );
};

export const AppProfilesView: React.FC<{
  onNavigate?: (s: ShellSectionId) => void;
}> = ({ onNavigate }) => {
  const { settings } = useSettings();
  const refreshSettings = useSettingsStore((s) => s.refreshSettings);
  const [displayNames, setDisplayNames] = useState<Record<string, string>>({});

  const blockedApps = settings?.blocked_apps ?? [];
  const blockedSites =
    ((settings as Record<string, unknown> | null)?.blocked_sites as
      | BlockedSiteEntry[]
      | undefined) ?? [];

  useEffect(() => {
    if (blockedApps.length === 0) {
      setDisplayNames({});
      return;
    }
    let active = true;
    void invoke<Array<{ id: string; name: string }>>("app_display_names", {
      ids: blockedApps,
    })
      .then((rows) => {
        if (!active || !Array.isArray(rows)) return;
        const map: Record<string, string> = {};
        for (const row of rows) {
          map[row.id] = row.name;
        }
        setDisplayNames(map);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [blockedApps.join("|")]);

  return (
    <AppProfilesSection
      profiles={(settings?.app_profiles ?? []) as AppProfileWithCleanup[]}
      blockedApps={blockedApps}
      blockedSites={blockedSites}
      displayNames={displayNames}
      onSaveProfiles={async (next) => {
        if ((await commands.updateAppProfiles(next)).status === "ok")
          await refreshSettings();
      }}
      onSaveBlockedApps={async (next) => {
        if ((await commands.updateBlockedApps(next)).status === "ok")
          await refreshSettings();
      }}
      onSaveBlockedSites={async (next) => {
        try {
          await invoke("update_blocked_sites", { sites: next });
          await refreshSettings();
        } catch {
          // Ignore when running outside Tauri runtime.
        }
      }}
      onReadFrontApp={async () => {
        const res = await commands.getActiveAppInfo();
        return res.status === "ok" ? res.data : null;
      }}
      onNavigate={onNavigate}
    />
  );
};
