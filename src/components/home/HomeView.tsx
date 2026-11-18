import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, events, type HistoryEntry } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import type { ShellSectionId } from "../layout/shellNavigation";
import { observeRecentHistory } from "./recentHistory";

export const HOME_RECENT_LIMIT = 2;
const buttonClass =
  "rounded-lg border border-mid-gray/40 px-3 py-2 text-sm font-medium text-start cursor-pointer hover:bg-mid-gray/15 focus-visible:ring-2 focus-visible:ring-logo-primary disabled:opacity-50 disabled:cursor-not-allowed";
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
            <button type="button" disabled className={buttonClass}>
              {t("sidebar.startRecording")}
            </button>
            <button
              type="button"
              onClick={() => onNavigate("meetings-history")}
              className={buttonClass}
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
              className={buttonClass}
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
            className={buttonClass}
          >
            {t("sidebar.seeAllDictations")}
          </button>
        </section>
      </div>
    </section>
  );
}

export function HomeView({
  showMeetings,
  onNavigate,
}: Pick<HomeSectionProps, "showMeetings" | "onNavigate">) {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  useEffect(
    () =>
      observeRecentHistory(
        async () => {
          const result = await commands.getHistoryEntries(
            null,
            HOME_RECENT_LIMIT,
          );
          if (result.status !== "ok") throw new Error("history unavailable");
          return result.data.entries;
        },
        (refresh) => events.historyUpdatePayload.listen(() => refresh()),
        (value) => {
          setEntries(value);
          setLoading(false);
          setError(false);
        },
        () => {
          setLoading(false);
          setError(true);
        },
        () => {
          setEntries([]);
          setLoading(true);
          setError(false);
        },
      ),
    [],
  );
  const shortcut =
    settings?.bindings?.transcribe?.current_binding?.trim() ||
    t("sidebar.shortcutNotSet");
  return (
    <HomeSection
      shortcut={shortcut}
      entries={entries}
      loading={loading}
      error={error}
      showMeetings={showMeetings}
      onNavigate={onNavigate}
    />
  );
}
