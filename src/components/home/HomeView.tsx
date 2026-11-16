import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { commands, type HistoryEntry } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { useSettingsStore } from "../../stores/settingsStore";
import {
  formatDurationMinutes,
  NOTES_READY_TONE,
  OUTLINED_BTN_SM,
  type MeetingListItem,
} from "../meetings/MeetingsList";
import type { ShellSectionId } from "../layout/shellRoutes";

const DOT_FILLED = "●";
const DOT_OPEN = "○";
const QUOTE = '"';
const MID_SEP = " · ";
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
const ROUTE_MODELS: ShellSectionId = "models";
const ROUTE_MEETINGS_RECORD: ShellSectionId = "meetings-record";

export const HOME_RECENT_LIMIT = 2;

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
  missingLocalModelNotice?: boolean;
  onDismissCleanupNotice?: () => void;
  onStartRecording: () => void;
  onNavigate: (section: ShellSectionId) => void;
}

function snippet(text: string): string {
  const clean = text.trim().replace(/\s+/g, " ");
  return clean.length > 42 ? `${clean.slice(0, 41)}…` : clean;
}

export const HomeViewSection: React.FC<HomeViewSectionProps> = ({
  shortcut,
  meetings,
  dictations,
  missingLocalModelNotice = false,
  onDismissCleanupNotice,
  onStartRecording,
  onNavigate,
}) => {
  const { t, i18n } = useTranslation();
  const recentM = meetings.slice(0, HOME_RECENT_LIMIT);
  const recentD = dictations.slice(0, HOME_RECENT_LIMIT);

  return (
    <div
      data-route-domain="home"
      className="max-w-4xl w-full mx-auto space-y-2.5"
    >
      <p className={EYEBROW}>{t("sidebar.home")}</p>
      <h2 className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
        {t("sidebar.homeHeading")}
      </h2>
      {missingLocalModelNotice ? (
        <div
          data-testid="home-cleanup-notice"
          className="grid gap-2 rounded-xl border border-logo-primary/50 bg-background p-3.5"
        >
          <h3 className="text-[14px] font-bold text-text">
            {t("cleanupStatus.chip.noModel")}
          </h3>
          <p className="text-[13px] leading-[18px] text-mid-gray">
            {t("cleanupStatus.sentence.noModel")}
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => onNavigate(ROUTE_MODELS)}
              className="inline-flex min-h-[30px] items-center rounded-[10px] bg-logo-primary px-3 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
            >
              {t("cleanupStatus.action.download")}
            </button>
            {onDismissCleanupNotice ? (
              <button
                type="button"
                onClick={onDismissCleanupNotice}
                className={OUTLINED_BTN_SM}
              >
                {t("common.close")}
              </button>
            ) : null}
          </div>
        </div>
      ) : null}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5 pt-1">
        <section
          aria-label={t("meetings.sidebar")}
          data-home-card="meetings"
          className="flex flex-col gap-2 rounded-xl border border-mid-gray/25 bg-background p-3.5"
        >
          <p className={EYEBROW}>{t("meetings.sidebar")}</p>
          <h3 className="text-[18px] leading-[24px] font-semibold tracking-[-0.02em] text-text">
            {t("sidebar.recordMeeting")}
          </h3>
          <p className="text-[13px] leading-[18px] text-mid-gray">
            {t("sidebar.homeMeetingsBody")}
          </p>
          <div className="flex flex-wrap items-center gap-2 pt-0.5">
            <button
              type="button"
              onClick={onStartRecording}
              className="inline-flex min-h-[32px] items-center gap-1.5 rounded-[10px] bg-logo-primary px-3 text-[13px] font-semibold text-white cursor-pointer hover:opacity-90"
            >
              <span aria-hidden="true">{DOT_FILLED}</span>
              <span>{t("sidebar.startRecording")}</span>
            </button>
          </div>
          <p className="mt-1.5 text-[12px] leading-[16px] font-medium text-mid-gray">
            {t("sidebar.recentMeetings")}
          </p>
          {recentM.length === 0 ? (
            <p className="py-2 text-[13px] text-mid-gray">
              {t("meetings.list.emptyList")}
            </p>
          ) : (
            <div className="divide-y divide-mid-gray/20 border-b-[1.5px] border-text/75">
              {recentM.map((m) => {
                const title = m.title.trim() || t("meetings.list.untitled");
                const dStr = new Date(m.started_at * 1000).toLocaleDateString(
                  i18n.language,
                  DATE_OPTS,
                );
                const mins = formatDurationMinutes(m.started_at, m.ended_at);
                const fullMeta =
                  mins !== null
                    ? t("meetings.list.dateAndDuration", {
                        date: dStr,
                        minutes: mins,
                      })
                    : dStr;
                const suffix = fullMeta.startsWith(dStr)
                  ? fullMeta.slice(dStr.length)
                  : "";
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
                      className={`inline-flex shrink-0 items-center gap-1 text-[11.5px] font-semibold ${m.notes_ready ? NOTES_READY_TONE : "text-mid-gray"}`}
                    >
                      <span aria-hidden="true">
                        {m.notes_ready ? DOT_FILLED : DOT_OPEN}
                      </span>
                      <span>
                        {m.notes_ready
                          ? t("meetings.list.notesReady")
                          : t("meetings.list.transcriptOnly")}
                      </span>
                    </span>
                  </div>
                );
              })}
            </div>
          )}
          <button
            type="button"
            onClick={() => onNavigate(ROUTE_MEETINGS_HISTORY)}
            className="mt-auto pt-1 self-start text-[13px] font-semibold text-logo-primary cursor-pointer hover:underline"
          >
            {t("sidebar.seeAllMeetings")}
          </button>
        </section>

        <section
          aria-label={t("sidebar.voiceSection")}
          data-home-card="voice"
          className="flex flex-col gap-2 rounded-xl border border-mid-gray/25 bg-background p-3.5"
        >
          <p className={EYEBROW}>{t("sidebar.voiceSection")}</p>
          <h3 className="text-[18px] leading-[24px] font-semibold tracking-[-0.02em] text-text">
            {t("sidebar.dictateHeading")}
          </h3>
          <p className="text-[13px] leading-[18px] text-mid-gray">
            {t("sidebar.dictateBody")}
          </p>
          <div className="flex flex-wrap items-center gap-2 pt-0.5">
            <kbd className="inline-grid min-w-[32px] h-[26px] place-items-center rounded-[6px] border border-mid-gray/60 px-2 font-mono text-[12px] font-bold text-text">
              {shortcut}
            </kbd>
            <span className="text-[13px] text-mid-gray">
              {t("sidebar.holdToTalk")}
            </span>
            <button
              type="button"
              onClick={() => onNavigate(ROUTE_SETTINGS)}
              className={OUTLINED_BTN_SM}
            >
              {t("sidebar.changeShortcut")}
            </button>
          </div>
          <p className="mt-1.5 text-[12px] leading-[16px] font-medium text-mid-gray">
            {t("sidebar.recentDictations")}
          </p>
          {recentD.length === 0 ? (
            <p className="py-2 text-[13px] text-mid-gray">
              {t("settings.history.empty")}
            </p>
          ) : (
            <div className="divide-y divide-mid-gray/20 border-b-[1.5px] border-text/75">
              {recentD.map((d) => {
                const app = d.appName?.trim() || "";
                return (
                  <div key={d.id} className="py-2.5">
                    <p className="truncate text-[13.5px] font-semibold text-text">
                      {`${QUOTE}${snippet(d.text)}${QUOTE}`}
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
              })}
            </div>
          )}
          <button
            type="button"
            onClick={() => onNavigate(ROUTE_VOICE_HISTORY)}
            className="mt-auto pt-1 self-start text-[13px] font-semibold text-logo-primary cursor-pointer hover:underline"
          >
            {t("sidebar.seeAllDictations")}
          </button>
        </section>
      </div>
    </div>
  );
};

export function toHomeDictation(
  e: HistoryEntry,
  lang: string,
): HomeDictationItem {
  const txt =
    e.post_processed_text && e.post_processed_text.trim().length > 0
      ? e.post_processed_text
      : e.transcription_text;
  const rawClock = new Date(e.timestamp * 1000).toLocaleTimeString(
    lang,
    TIME_OPTS,
  );
  const time = rawClock.replace(/^0(?=\d:)/, "");
  return { id: e.id, text: txt, appName: e.app_name, time };
}

export const HomeView: React.FC<{
  onNavigate?: (s: ShellSectionId) => void;
}> = ({ onNavigate = () => {} }) => {
  const { i18n } = useTranslation();
  const { settings } = useSettings();
  const lastCleanupStatus = useSettingsStore((s) => s.lastCleanupStatus);
  const setLastCleanupStatus = useSettingsStore((s) => s.setLastCleanupStatus);
  const [meetings, setMeetings] = useState<MeetingListItem[]>([]);
  const [dictations, setDictations] = useState<HomeDictationItem[]>([]);
  const shortcut =
    settings?.bindings?.transcribe?.current_binding?.trim() || "fn";

  useEffect(() => {
    let cancelled = false;
    void (async () => {
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
    };
  }, [i18n.language]);

  const startRecording = () => {
    void emit("meetings-open-consent").catch(() => {});
    onNavigate(ROUTE_MEETINGS_RECORD);
  };

  return (
    <HomeViewSection
      shortcut={shortcut}
      meetings={meetings}
      dictations={dictations}
      missingLocalModelNotice={
        lastCleanupStatus?.reason === "local_model_not_downloaded"
      }
      onDismissCleanupNotice={() => setLastCleanupStatus(null)}
      onStartRecording={startRecording}
      onNavigate={onNavigate}
    />
  );
};
