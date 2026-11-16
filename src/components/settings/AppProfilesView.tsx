import React, { useState } from "react";
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
import { NeverDictateCard } from "./NeverDictateCard";

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
  }
> = {
  formal: {
    capitalization: "sentence",
    punctuation: "full",
    post_process_enabled: true,
  },
  direct: {
    capitalization: "sentence",
    punctuation: "full",
    post_process_enabled: true,
  },
  casual: {
    capitalization: "lowercase",
    punctuation: "drop_trailing_period",
    post_process_enabled: true,
  },
  code: {
    capitalization: "default",
    punctuation: "drop_trailing_period",
    post_process_enabled: true,
  },
  default: {
    capitalization: "default",
    punctuation: "default",
    post_process_enabled: false,
  },
  auto: {
    capitalization: "default",
    punctuation: "default",
    post_process_enabled: true,
  },
};

export function summarizeAppProfile(
  profile: AppProfile,
  t: (key: string) => string,
): string {
  const cleanup =
    profile.post_process_enabled === false
      ? t("profiles.cleanupOff")
      : profile.post_process_enabled === true
        ? t("profiles.cleanupOn")
        : t("profiles.cleanupDefault");
  const tone = profile.tone ?? "auto";
  if (profile.post_process_enabled === false && tone === "default") {
    return `${t("profiles.exactWords")}${MID_SEP}${cleanup}`;
  }
  const parts: string[] = [];
  if (TONE_KEYS[tone]) parts.push(t(TONE_KEYS[tone]!));
  const caps = profile.capitalization ?? "default";
  const punct = profile.punctuation ?? "default";
  if (
    tone !== "code" &&
    (punct === "drop_trailing_period" ||
      (punct === "default" && tone === "casual"))
  ) {
    parts.push(t("profiles.noTrailingStop"));
  } else if (
    (caps === "sentence" && punct === "full") ||
    (caps === "default" &&
      punct === "default" &&
      (tone === "formal" || tone === "direct"))
  ) {
    parts.push(t("profiles.capsAndPunct"));
  }
  parts.push(cleanup);
  return parts.join(MID_SEP);
}

export interface AppProfilesSectionProps {
  profiles: AppProfile[];
  blockedApps: string[];
  onSaveProfiles: (next: AppProfile[]) => Promise<void>;
  onSaveBlockedApps: (next: string[]) => Promise<void>;
  onReadFrontApp?: () => Promise<{
    app_name: string;
    bundle_id: string | null;
  } | null>;
  onNavigate?: (s: ShellSectionId) => void;
}

export const AppProfilesSection: React.FC<AppProfilesSectionProps> = ({
  profiles,
  blockedApps,
  onSaveProfiles,
  onSaveBlockedApps,
  onReadFrontApp,
  onNavigate,
}) => {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<AppProfile | null>(null);
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
    const clean: AppProfile = {
      ...draft,
      name: draft.name.trim() || id,
      app_identifier: id,
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
        <div className="mt-3 grid gap-2.5 rounded-xl border border-mid-gray/30 bg-background p-3.5">
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
          <div className="flex flex-wrap items-center gap-2">
            <label className="flex items-center gap-2 text-xs font-medium text-mid-gray">
              <span>{t("profiles.toneLabel")}</span>
              <select
                value={draft.tone ?? "auto"}
                onChange={(e) => {
                  const tone = e.target.value as AppTone;
                  setDraft({ ...draft, tone, ...TONE_PRESETS[tone] });
                }}
                className="rounded-[10px] border border-mid-gray/40 bg-background px-2.5 py-1 text-xs text-text"
              >
                <option value="auto">{t("profiles.toneAuto")}</option>
                <option value="formal">{t("profiles.toneFormal")}</option>
                <option value="casual">{t("profiles.toneCasual")}</option>
                <option value="code">{t("profiles.toneCode")}</option>
                <option value="default">{t("profiles.exactWords")}</option>
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
                {t("profiles.remove")}
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
        onSaveBlockedApps={onSaveBlockedApps}
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
  return (
    <AppProfilesSection
      profiles={settings?.app_profiles ?? []}
      blockedApps={settings?.blocked_apps ?? []}
      onSaveProfiles={async (next) => {
        if ((await commands.updateAppProfiles(next)).status === "ok")
          await refreshSettings();
      }}
      onSaveBlockedApps={async (next) => {
        if ((await commands.updateBlockedApps(next)).status === "ok")
          await refreshSettings();
      }}
      onReadFrontApp={async () => {
        const res = await commands.getActiveAppInfo();
        return res.status === "ok" ? res.data : null;
      }}
      onNavigate={onNavigate}
    />
  );
};
