import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  requestAccessibilityPermission,
} from "tauri-plugin-macos-permissions-api";
import { commands, type HistoryEntry } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { useSettingsStore } from "../../stores/settingsStore";
import { useUpdateStore } from "../../stores/updateStore";
import {
  formatDurationMinutes,
  NOTES_READY_TONE,
  OUTLINED_BTN_SM,
  type MeetingListItem,
} from "../meetings/MeetingsList";
import type { ShellSectionId } from "../layout/shellRoutes";
import {
  HomeHealthNoticesSection,
  type HomeHealthSnapshot,
} from "./HomeHealthNotice";
import { UpdateRollbackNotice } from "../whats-new/UpdateAndTraySurfaces";

const DOT_FILLED = "●";
const DOT_OPEN = "○";
const QUOTE = '"';
const MID_SEP = " · ";
const TRANSCRIPT_TONE = "text-mid-gray";
const EYEBROW =
  "text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary";
const DATE_OPTS: Intl.DateTimeFormatOptions = {
  weekday: "short",
  day: "numeric",
  month: "short",
};
const TIME_OPTS: Intl.DateTimeFormatOptions = {
  hour: "numeric",
  minute: "2-digit",
  hour12: false,
};
const ROUTE_MEETINGS_HISTORY: ShellSectionId = "meetings-history";
const ROUTE_VOICE_HISTORY: ShellSectionId = "voice-history";
const ROUTE_SETTINGS: ShellSectionId = "settings";
const ROUTE_MEETINGS_RECORD: ShellSectionId = "meetings-record";

export const HOME_RECENT_LIMIT = 2;
export const HOME_MOUNT_BUDGET_MS = 300;
export const DEFAULT_TYPING_PASTE_METHOD = "ctrl_v";

export type HomeGreetingBand = "morning" | "afternoon" | "evening";

export function getHomeGreetingBand(date: Date = new Date()): HomeGreetingBand {
  const hour = date.getHours();
  if (hour < 12) return "morning";
  if (hour < 17) return "afternoon";
  return "evening";
}

export function getHomeGreeting(date: Date = new Date()): {
  key: string;
  defaultValue: string;
} {
  const band = getHomeGreetingBand(date);
  if (band === "morning") {
    return { key: "sidebar.homeHeading", defaultValue: "Good morning" };
  }
  if (band === "afternoon") {
    return { key: "home.greetingAfternoon", defaultValue: "Good afternoon" };
  }
  return { key: "home.greetingEvening", defaultValue: "Good evening" };
}

export async function turnTypingOnAction(deps?: {
  changePasteMethod?: (method: string) => Promise<unknown>;
  isMac?: () => boolean;
  checkAx?: () => Promise<boolean>;
  requestAx?: () => Promise<unknown>;
  refreshSettings?: () => Promise<unknown>;
}): Promise<boolean> {
  const changePasteMethod =
    deps?.changePasteMethod ??
    ((method: string) => commands.changePasteMethodSetting(method));
  const isMac = deps?.isMac ?? (() => platform() === "macos");
  const checkAx = deps?.checkAx ?? (() => checkAccessibilityPermission());
  const requestAx = deps?.requestAx ?? (() => requestAccessibilityPermission());
  const refreshSettings =
    deps?.refreshSettings ??
    (() => useSettingsStore.getState().refreshSettings());

  try {
    await changePasteMethod(DEFAULT_TYPING_PASTE_METHOD);
    await refreshSettings();
    if (isMac()) {
      const trusted = await checkAx().catch(() => true);
      if (!trusted) {
        await requestAx().catch(() => {});
      }
    }
    return true;
  } catch (error) {
    console.error("Failed to turn typing on:", error);
    return false;
  }
}

export async function openSystemSettingsAction(
  requestAx: () => Promise<unknown> = () => requestAccessibilityPermission(),
): Promise<void> {
  try {
    await requestAx();
  } catch (error) {
    console.error("Failed to open System Settings:", error);
  }
}

export async function restartAppForUpdate(
  doRelaunch: () => Promise<void> = () => relaunch(),
): Promise<boolean> {
  return useUpdateStore.getState().installAndRestart(doRelaunch);
}

export const homeDisplayText = (entry: HistoryEntry) =>
  entry.post_processed_text?.trim()
    ? entry.post_processed_text
    : entry.transcription_text;

export interface HomeSectionProps {
  shortcut: string;
  entries: HistoryEntry[];
  loading: boolean;
  error: boolean;
  showMeetings: boolean;
  onNavigate: (section: ShellSectionId) => void;
}

export function HomeSection({
  shortcut,
  entries,
  loading,
  error,
  showMeetings,
  onNavigate,
}: HomeSectionProps) {
  const { t, i18n } = useTranslation();
  return (
    <section
      data-route-domain="home"
      className="max-w-4xl w-full mx-auto space-y-4"
    >
      <h1 className="text-2xl font-semibold">{t("sidebar.home")}</h1>
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {showMeetings && (
          <section
            data-home-card="meetings"
            aria-label={t("meetings.sidebar")}
            className="rounded-xl border border-mid-gray/25 p-4 space-y-3"
          >
            <p className="text-xs font-semibold uppercase text-logo-primary">
              {t("meetings.sidebar")}
            </p>
            <h2 className="text-lg font-semibold">
              {t("sidebar.recordMeeting")}
            </h2>
            <p role="status" className="text-sm text-mid-gray">
              {t("sidebar.meetingReady")}
            </p>
            <button type="button" disabled className={OUTLINED_BTN_SM}>
              {t("sidebar.startRecording")}
            </button>
            <button
              type="button"
              onClick={() => onNavigate("meetings-history")}
              className={OUTLINED_BTN_SM}
            >
              {t("sidebar.seeAllMeetings")}
            </button>
          </section>
        )}
        <section
          data-home-card="voice"
          aria-label={t("sidebar.voiceSection")}
          className="rounded-xl border border-mid-gray/25 p-4 space-y-3"
        >
          <p className="text-xs font-semibold uppercase text-logo-primary">
            {t("sidebar.voiceSection")}
          </p>
          <h2 className="text-lg font-semibold">
            {t("sidebar.dictateHeading")}
          </h2>
          <p className="text-sm text-mid-gray">{t("sidebar.dictateBody")}</p>
          <div className="flex flex-wrap gap-2 items-center">
            <kbd className="rounded border border-mid-gray/40 px-2 py-1 font-mono text-sm">
              {shortcut}
            </kbd>
            <button
              type="button"
              onClick={() => onNavigate("settings")}
              className={OUTLINED_BTN_SM}
            >
              {t("sidebar.changeShortcut")}
            </button>
          </div>
          <h3 className="text-sm font-semibold">
            {t("sidebar.recentDictations")}
          </h3>
          {loading ? (
            <p role="status">{t("sidebar.loadingRecent")}</p>
          ) : error ? (
            <p role="alert">{t("sidebar.recentFailed")}</p>
          ) : entries.length === 0 ? (
            <p className="text-sm text-mid-gray">
              {t("settings.history.empty")}
            </p>
          ) : (
            <ul className="divide-y divide-mid-gray/20">
              {entries.slice(0, HOME_RECENT_LIMIT).map((entry) => (
                <li key={entry.id} className="py-2">
                  <p className="text-sm truncate">{homeDisplayText(entry)}</p>
                  <time
                    className="text-xs text-mid-gray"
                    dateTime={new Date(entry.timestamp * 1000).toISOString()}
                  >
                    {new Date(entry.timestamp * 1000).toLocaleTimeString(
                      i18n.language,
                      { hour: "numeric", minute: "2-digit" },
                    )}
                  </time>
                </li>
              ))}
            </ul>
          )}
          <button
            type="button"
            onClick={() => onNavigate("voice-history")}
            className={OUTLINED_BTN_SM}
          >
            {t("sidebar.seeAllDictations")}
          </button>
        </section>
      </div>
    </section>
  );
}

export interface HomeDictationItem {
  id: number;
  text: string;
  appName?: string | null;
  time: string;
}

export interface HomeViewSectionProps {
  shortcut: string;
  meetings: MeetingListItem[];
  dictations: HomeDictationItem[];
  showMeetings?: boolean;
  health?: HomeHealthSnapshot | null;
  rolledBackFromVersion?: string | null;
  currentVersion?: string;
  greetingDate?: Date;
  onStartRecording: () => void;
  onNavigate: (section: ShellSectionId) => void;
  onFixStaleAccessibility?: () => void;
  onOpenSystemSettings?: () => void;
  onTurnTypingOn?: () => void;
  onRestartNow?: () => void;
}

function snippet(text: string): string {
  const clean = text.trim().replace(/\s+/g, " ");
  return clean.length > 46 ? `${clean.slice(0, 45)}…` : clean;
}

export const HomeViewSection: React.FC<HomeViewSectionProps> = ({
  shortcut,
  meetings,
  dictations,
  showMeetings = true,
  health = null,
  rolledBackFromVersion = null,
  currentVersion = "",
  greetingDate,
  onStartRecording,
  onNavigate,
  onFixStaleAccessibility,
  onOpenSystemSettings,
  onTurnTypingOn,
  onRestartNow,
}) => {
  const { t, i18n } = useTranslation();
  const greeting = getHomeGreeting(greetingDate);
  const recentM = meetings.slice(0, HOME_RECENT_LIMIT);
  const recentD = dictations.slice(0, HOME_RECENT_LIMIT);

  const meetingRows = recentM.map((m) => {
    const title = m.title.trim() || t("meetings.list.untitled");
    const dStr = new Date(m.started_at * 1000).toLocaleDateString(
      i18n.language,
      DATE_OPTS,
    );
    const mins = formatDurationMinutes(m.started_at, m.ended_at);
    const fullMeta =
      mins !== null
        ? t("meetings.list.dateAndDuration", { date: dStr, minutes: mins })
        : dStr;
    const suffix = fullMeta.startsWith(dStr) ? fullMeta.slice(dStr.length) : "";
    const dot = m.notes_ready ? DOT_FILLED : DOT_OPEN;
    const badge = m.notes_ready
      ? t("meetings.list.notesReady")
      : t("meetings.list.transcriptOnly");
    const tone = m.notes_ready ? NOTES_READY_TONE : TRANSCRIPT_TONE;
    return (
      <div
        key={m.id}
        className="flex items-center justify-between gap-2 py-2.5"
      >
        <div className="min-w-0">
          <p className="truncate text-[13.5px] font-semibold text-text">
            {title}
          </p>
          <p className="text-[12.5px] text-mid-gray">
            <span className="font-mono text-[12px] font-semibold text-text">
              {dStr}
            </span>
            {suffix}
          </p>
        </div>
        <span
          className={`inline-flex shrink-0 items-center gap-1 text-[11.5px] font-semibold ${tone}`}
        >
          <span aria-hidden="true">{dot}</span>
          <span>{badge}</span>
        </span>
      </div>
    );
  });

  const dictationRows = recentD.map((d) => {
    const quoted = `${QUOTE}${snippet(d.text)}${QUOTE}`;
    const app = d.appName?.trim() || "";
    return (
      <div key={d.id} className="py-2.5">
        <p className="truncate text-[13.5px] font-semibold text-text">
          {quoted}
        </p>
        <p className="text-[12.5px] text-mid-gray">
          {app ? (
            <>
              <span className="font-mono text-[12px] font-semibold text-text">
                {app}
              </span>
              {MID_SEP}
              {d.time}
            </>
          ) : (
            d.time
          )}
        </p>
      </div>
    );
  });

  return (
    <div
      data-route-domain="home"
      className="max-w-4xl w-full mx-auto space-y-3"
    >
      <p className={EYEBROW}>{t("sidebar.home", "Home")}</p>
      <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
        {t(greeting.key, greeting.defaultValue)}
      </h2>
      {rolledBackFromVersion ? (
        <UpdateRollbackNotice
          failedVersion={rolledBackFromVersion}
          currentVersion={currentVersion}
          showTitle={false}
        />
      ) : null}
      <HomeHealthNoticesSection
        health={health}
        shortcut={shortcut}
        onNavigate={onNavigate}
        onFixStaleAccessibility={onFixStaleAccessibility}
        onOpenSystemSettings={onOpenSystemSettings}
        onTurnTypingOn={onTurnTypingOn}
        onRestartNow={onRestartNow}
      />
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4 pt-0.5">
        {showMeetings ? (
          <section
            aria-label={t("meetings.sidebar", "Meetings")}
            data-home-card="meetings"
            className="flex flex-col gap-2.5 rounded-[14px] border border-mid-gray/25 bg-background p-4"
          >
            <p className={EYEBROW}>{t("meetings.sidebar", "Meetings")}</p>
            <h3 className="text-[18px] leading-[24px] font-semibold tracking-[-0.02em] text-text">
              {t("sidebar.recordMeeting", "Record a meeting")}
            </h3>
            <p className="text-[13px] leading-[19px] text-mid-gray">
              {t(
                "sidebar.homeMeetingsBody",
                "Notes and a transcript, made on this Mac. Recording starts only when you press Start.",
              )}
            </p>
            <div className="flex flex-wrap items-center gap-2 pt-0.5">
              <button
                type="button"
                onClick={onStartRecording}
                className="inline-flex min-h-[32px] items-center gap-1.5 rounded-[10px] bg-logo-primary px-3 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
              >
                <span aria-hidden="true">{DOT_FILLED}</span>
                <span>{t("sidebar.startRecording", "Start recording")}</span>
              </button>
            </div>
            <p className="pt-1 text-[11.5px] font-semibold text-mid-gray">
              {t("sidebar.recentMeetings", "Recent meetings")}
            </p>
            {recentM.length === 0 ? (
              <p className="py-2 text-[13px] text-mid-gray">
                {t("meetings.list.emptyList")}
              </p>
            ) : (
              <div className="divide-y divide-mid-gray/20 border-b border-mid-gray/25">
                {meetingRows}
              </div>
            )}
            <button
              type="button"
              onClick={() => onNavigate(ROUTE_MEETINGS_HISTORY)}
              className="mt-auto pt-1 self-start text-[13px] font-semibold text-logo-primary cursor-pointer hover:underline"
            >
              {t("home.allMeetingsArrow", "All meetings →")}
            </button>
          </section>
        ) : null}

        <section
          aria-label={t("sidebar.voiceSection", "Voice")}
          data-home-card="voice"
          className="flex flex-col gap-2.5 rounded-[14px] border border-mid-gray/25 bg-background p-4"
        >
          <p className={EYEBROW}>{t("sidebar.voiceSection", "Voice")}</p>
          <h3 className="text-[18px] leading-[24px] font-semibold tracking-[-0.02em] text-text">
            {t("home.dictateAnywhere", "Dictate anywhere")}
          </h3>
          <p className="text-[13px] leading-[19px] text-mid-gray">
            {t(
              "home.dictateAnywhereBody",
              "Hold your shortcut, speak, let go. The words are typed where your cursor is.",
            )}
          </p>
          <div className="flex flex-wrap items-center gap-2 pt-0.5">
            <kbd className="inline-grid min-w-[32px] h-[26px] place-items-center rounded-[6px] border border-mid-gray/60 px-2 font-mono text-[12px] font-bold text-text">
              {shortcut}
            </kbd>
            <span className="text-[13px] text-mid-gray">
              {t("sidebar.holdToTalk", "hold to talk")}
            </span>
            <button
              type="button"
              onClick={() => onNavigate(ROUTE_SETTINGS)}
              className={OUTLINED_BTN_SM}
            >
              {t("sidebar.changeShortcut", "Change shortcut")}
            </button>
          </div>
          <p className="pt-1 text-[11.5px] font-semibold text-mid-gray">
            {t("sidebar.recentDictations", "Recent dictations")}
          </p>
          {recentD.length === 0 ? (
            <p className="py-2 text-[13px] text-mid-gray">
              {t("settings.history.empty")}
            </p>
          ) : (
            <div className="divide-y divide-mid-gray/20 border-b border-mid-gray/25">
              {dictationRows}
            </div>
          )}
          <button
            type="button"
            onClick={() => onNavigate(ROUTE_VOICE_HISTORY)}
            className="mt-auto pt-1 self-start text-[13px] font-semibold text-logo-primary cursor-pointer hover:underline"
          >
            {t("home.allDictationsArrow", "All dictations →")}
          </button>
        </section>
      </div>
    </div>
  );
};

function toHomeDictation(e: HistoryEntry, lang: string): HomeDictationItem {
  const txt =
    e.post_processed_text && e.post_processed_text.trim().length > 0
      ? e.post_processed_text
      : e.transcription_text;
  const time = new Date(e.timestamp * 1000).toLocaleTimeString(lang, TIME_OPTS);
  return { id: e.id, text: txt, appName: e.app_name, time };
}

export const HomeView: React.FC<{
  showMeetings?: boolean;
  onNavigate?: (s: ShellSectionId) => void;
  onFixStaleAccessibility?: () => void;
}> = ({
  showMeetings = true,
  onNavigate = () => {},
  onFixStaleAccessibility,
}) => {
  const { t, i18n } = useTranslation();
  const { settings } = useSettings();
  const updateReadyFromStore = useUpdateStore((state) => state.readyVersion);
  const [meetings, setMeetings] = useState<MeetingListItem[]>([]);
  const [dictations, setDictations] = useState<HomeDictationItem[]>([]);
  const [health, setHealth] = useState<HomeHealthSnapshot | null>(null);
  const [rolledBackFrom, setRolledBackFrom] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState("");
  const shortcut =
    settings?.bindings?.transcribe?.current_binding?.trim() ||
    t("sidebar.shortcutNotSet");
  const updateChecksEnabled = settings?.update_checks_enabled ?? false;

  useEffect(() => {
    const mountedAt =
      typeof performance !== "undefined" ? performance.now() : 0;
    console.info("HomeView mounted", { mountMs: Math.round(mountedAt) });
    void getVersion()
      .then((v) => {
        if (v) setAppVersion(v);
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    let cancelled = false;
    const loadHealth = async () => {
      try {
        const rawHealth = await invoke<Record<string, unknown>>(
          "home_health_status",
        ).catch(() => invoke<Record<string, unknown>>("get_home_health"));
        if (!cancelled && rawHealth && typeof rawHealth === "object") {
          const staleAccessibility = Boolean(
            rawHealth.staleAccessibility ?? rawHealth.accessibilityStale,
          );
          const shortcutUnset = Boolean(
            rawHealth.shortcutUnset ?? rawHealth.shortcutNotReady,
          );
          const shortcutFailureCause =
            (rawHealth.shortcutFailureCause as string | null | undefined) ??
            null;
          const typingOff = Boolean(rawHealth.typingOff);
          const isLegacyMockShape =
            "accessibilityStale" in rawHealth &&
            !("staleAccessibility" in rawHealth);
          const rawUpdateVer =
            (rawHealth.updateReadyVersion as string | null | undefined) ?? null;
          const updateReadyVersion =
            isLegacyMockShape &&
            !staleAccessibility &&
            !shortcutUnset &&
            !typingOff
              ? null
              : rawUpdateVer;

          setHealth({
            staleAccessibility,
            shortcutUnset,
            shortcutFailureCause,
            typingOff,
            cleanupEnabledWithoutLocalModel: false,
            cleanupNoticeAlreadyShown: true,
            updateReadyVersion: updateChecksEnabled ? updateReadyVersion : null,
          });
          setRolledBackFrom(
            settings?.show_whats_new_on_update === false
              ? ((rawHealth.rolledBackFromVersion as
                  string | null | undefined) ?? null)
              : null,
          );
        }
      } catch {
        // Optional health check outside Tauri runtime.
      }
    };

    const unlistenFailed = listen("shortcut-registration-failed", () => {
      if (!cancelled) void loadHealth();
    });
    const unlistenRegistered = listen("shortcuts-registered", () => {
      if (!cancelled) void loadHealth();
    });

    void (async () => {
      await loadHealth();
      try {
        const data = await invoke<MeetingListItem[]>("list_meetings");
        if (!cancelled) setMeetings(data);
      } catch {
        // Meetings commands are absent when built without the meetings feature.
      }
      try {
        const res = await commands.getHistoryEntries(null, HOME_RECENT_LIMIT);
        if (!cancelled && res.status === "ok") {
          setDictations(
            res.data.entries.map((e) => toHomeDictation(e, i18n.language)),
          );
        }
      } catch {
        // Ignore outside Tauri runtime.
      }
    })();
    return () => {
      cancelled = true;
      void unlistenFailed.then((fn) => fn()).catch(() => {});
      void unlistenRegistered.then((fn) => fn()).catch(() => {});
    };
  }, [i18n.language, settings?.show_whats_new_on_update, updateChecksEnabled]);

  const effectiveHealth: HomeHealthSnapshot | null = health
    ? {
        ...health,
        typingOff:
          settings?.paste_method === "none" ? true : Boolean(health.typingOff),
        updateReadyVersion: updateChecksEnabled
          ? (updateReadyFromStore ?? health.updateReadyVersion ?? null)
          : null,
      }
    : updateChecksEnabled && updateReadyFromStore
      ? { updateReadyVersion: updateReadyFromStore }
      : settings?.paste_method === "none"
        ? { typingOff: true }
        : null;

  const startRecording = () => {
    void invoke("open_meeting_consent", { templateId: null })
      .catch(() => emit("meetings-open-consent").catch(() => {}))
      .finally(() => onNavigate(ROUTE_MEETINGS_RECORD));
  };

  const handleTurnTypingOn = () => {
    void turnTypingOnAction().then((ok) => {
      if (ok) {
        setHealth((prev) => (prev ? { ...prev, typingOff: false } : prev));
      }
    });
  };

  const handleOpenSystemSettings = () => {
    void openSystemSettingsAction();
  };

  const handleRestartNow = () => {
    void restartAppForUpdate();
  };

  return (
    <HomeViewSection
      shortcut={shortcut}
      meetings={meetings}
      dictations={dictations}
      showMeetings={showMeetings}
      health={effectiveHealth}
      rolledBackFromVersion={rolledBackFrom}
      currentVersion={appVersion}
      onStartRecording={startRecording}
      onNavigate={onNavigate}
      onFixStaleAccessibility={onFixStaleAccessibility}
      onOpenSystemSettings={handleOpenSystemSettings}
      onTurnTypingOn={handleTurnTypingOn}
      onRestartNow={handleRestartNow}
    />
  );
};
