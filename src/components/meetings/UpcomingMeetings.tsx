import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "../../bindings.meetings";
import {
  currentStartNotesDeps,
  startNotesFlow,
  type UpcomingMeeting,
} from "./upcoming";

interface UpcomingMeetingsListProps {
  meetings: UpcomingMeeting[];
  /** False when nothing is picked in Settings > Meetings. */
  hasCalendars: boolean;
  startNotesAvailable: boolean;
  onStartNotes: (meeting: UpcomingMeeting) => void;
}

function timeRange(m: UpcomingMeeting, locale: string): string {
  const fmt = new Intl.DateTimeFormat(locale, {
    weekday: "short",
    hour: "numeric",
    minute: "2-digit",
  });
  const end = new Intl.DateTimeFormat(locale, {
    hour: "numeric",
    minute: "2-digit",
  });
  return `${fmt.format(new Date(m.start * 1000))} - ${end.format(new Date(m.end * 1000))}`;
}

export const UpcomingMeetingsList: React.FC<UpcomingMeetingsListProps> = ({
  meetings,
  hasCalendars,
  startNotesAvailable,
  onStartNotes,
}) => {
  const { t, i18n } = useTranslation();
  return (
    <section aria-labelledby="coming-up-title" className="space-y-2">
      <h2 id="coming-up-title" className="text-sm font-medium">
        {t("meetings.upcoming.title")}
      </h2>
      {!hasCalendars ? (
        <p className="text-sm text-text/60">
          {t("meetings.upcoming.noneSelected")}
        </p>
      ) : meetings.length === 0 ? (
        <p className="text-sm text-text/60">{t("meetings.upcoming.empty")}</p>
      ) : (
        <ul className="space-y-2">
          {meetings.map((m) => (
            <li
              key={`${m.start}|${m.title}|${m.calendar_name}`}
              className="flex items-center justify-between gap-3 rounded border border-mid-gray/20 p-3"
            >
              <div className="min-w-0">
                <p className="truncate text-sm font-medium">{m.title}</p>
                <p className="text-xs text-text/60">
                  {timeRange(m, i18n.language)} · {m.calendar_name}
                </p>
                {m.attendee_display_names.length > 0 ? (
                  <p className="truncate text-xs text-text/60">
                    {t("meetings.upcoming.with", {
                      names: m.attendee_display_names.join(", "),
                    })}
                  </p>
                ) : null}
              </div>
              <button
                type="button"
                className="shrink-0 rounded border border-mid-gray/40 px-3 py-1 text-sm disabled:opacity-50"
                disabled={!startNotesAvailable}
                title={
                  startNotesAvailable
                    ? undefined
                    : t("meetings.upcoming.startUnavailable")
                }
                onClick={() => onStartNotes(m)}
              >
                {t("meetings.upcoming.startNotes")}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
};

function selectedCount(settings: unknown): number {
  const v = (settings as { selected_calendars?: unknown } | null)
    ?.selected_calendars;
  return Array.isArray(v) ? v.length : 0;
}

/** Reads the picked calendars on open; drops what it read when Meetings closes. */
export const UpcomingMeetings: React.FC<{ settings: unknown }> = ({
  settings,
}) => {
  const [meetings, setMeetings] = useState<UpcomingMeeting[]>([]);
  const picked = selectedCount(settings);

  useEffect(() => {
    let cancelled = false;
    if (picked === 0) {
      setMeetings([]);
    } else {
      void commands.listUpcomingMeetings().then((r) => {
        if (!cancelled) setMeetings(r.status === "ok" ? r.data : []);
      });
    }
    return () => {
      cancelled = true;
      setMeetings([]);
      void commands.closeUpcomingMeetings();
    };
  }, [picked]);

  const deps = currentStartNotesDeps();
  const available = Boolean(deps.showConsentReminder && deps.beginNotes);
  return (
    <UpcomingMeetingsList
      meetings={meetings}
      hasCalendars={picked > 0}
      startNotesAvailable={available}
      onStartNotes={(m) => {
        void startNotesFlow(m, {
          ...deps,
          keepForNotes: (meeting) =>
            commands.keepUpcomingForNotes(
              meeting.start,
              meeting.title,
              meeting.calendar_name,
            ),
        });
      }}
    />
  );
};
