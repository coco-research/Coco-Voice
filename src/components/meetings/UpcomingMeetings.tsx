import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "../../bindings.meetings";
import { useSettings } from "../../hooks/useSettings";
import type { UpcomingMeeting } from "./upcoming";

interface UpcomingMeetingsListProps {
  meetings: UpcomingMeeting[];
  hasCalendars: boolean;
  permissionDenied?: boolean;
  totalCount?: number;
  startedKey?: string | null;
  mode?: "card" | "page";
  nowSecs?: number;
  onStartNotes: (meeting: UpcomingMeeting) => void;
  onOpenView?: () => void;
  onBack?: () => void;
  onOpenSettings?: () => void;
}

const NOT_STARTED_GLYPH = "\u25CB";
const STARTED_GLYPH = "\u25CF";

export function navigateToSettingsMeetings(): void {
  if (typeof document !== "undefined") {
    const settingsBtn = document.querySelector<HTMLButtonElement>(
      'button[data-nav-id="settings"]',
    );
    settingsBtn?.click();
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent("coco-voice:open-settings-meetings"));
  }
}

export function upcomingEventKey(m: UpcomingMeeting): string {
  return `${m.start}|${m.title}|${m.calendar_name}`;
}

export function formatShortDateDddDDMon(
  epochSecs: number,
  locale: string,
): string {
  const parts = new Intl.DateTimeFormat(locale, {
    weekday: "short",
    day: "numeric",
    month: "short",
  }).formatToParts(new Date(epochSecs * 1000));
  const weekday = parts.find((p) => p.type === "weekday")?.value ?? "";
  const day = parts.find((p) => p.type === "day")?.value ?? "";
  const month = parts.find((p) => p.type === "month")?.value ?? "";
  return [weekday, day, month].filter(Boolean).join(" ");
}

function formatEventMeta(m: UpcomingMeeting, locale: string): string {
  const day = formatShortDateDddDDMon(m.start, locale);
  const time = new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(new Date(m.start * 1000));
  const mins = Math.max(1, Math.round((m.end - m.start) / 60));
  return `${day} · ${time} · ${mins} min`;
}

function formatCardEventMeta(
  m: UpcomingMeeting,
  nowSecs: number,
  locale: string,
): string {
  const startDate = new Date(m.start * 1000);
  const nowDate = new Date(nowSecs * 1000);
  const time = new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(startDate);
  const sameDay =
    startDate.getFullYear() === nowDate.getFullYear() &&
    startDate.getMonth() === nowDate.getMonth() &&
    startDate.getDate() === nowDate.getDate();
  const deltaMins = Math.round((m.start - nowSecs) / 60);
  if (sameDay && deltaMins > 0 && deltaMins <= 180) {
    return `Today ${time} · in ${deltaMins} min`;
  }
  return formatEventMeta(m, locale);
}

export const UpcomingMeetingsList: React.FC<UpcomingMeetingsListProps> = ({
  meetings,
  hasCalendars,
  permissionDenied = false,
  totalCount,
  startedKey = null,
  mode = "page",
  nowSecs = Math.floor(Date.now() / 1000),
  onStartNotes,
  onOpenView,
  onOpenSettings,
}) => {
  const { t, i18n } = useTranslation();
  const visibleCount = meetings.length;
  const effectiveTotal = Math.max(visibleCount, totalCount ?? visibleCount);
  const showCountRatio = hasCalendars && !permissionDenied && visibleCount > 0;
  const handleOpenSettings = onOpenSettings ?? navigateToSettingsMeetings;

  const emptyOrDeniedBody = permissionDenied ? (
    <p className="text-sm text-mid-gray">
      {t(
        "meetings.upcoming.permissionDenied",
        "Calendar access is turned off in System Settings.",
      )}
    </p>
  ) : !hasCalendars ? (
    <div className="flex flex-wrap items-center justify-between gap-2">
      <p className="text-sm text-mid-gray">
        {t(
          "meetings.upcoming.pickInSettings",
          "Pick calendars in Settings › Meetings to see what's coming up.",
        )}
      </p>
      <button
        type="button"
        data-testid="coming-up-open-settings"
        onClick={handleOpenSettings}
        className="rounded-lg border border-mid-gray/40 px-2.5 py-1 text-xs font-semibold text-text hover:bg-mid-gray/10 cursor-pointer"
      >
        {t("meetings.upcoming.openSettings", "Open")}
      </button>
    </div>
  ) : meetings.length === 0 ? (
    <p className="text-sm text-mid-gray">
      {t(
        "meetings.upcoming.emptyTodayTomorrow",
        "No meetings today or tomorrow.",
      )}
    </p>
  ) : null;

  if (mode === "card") {
    return (
      <section
        data-testid="frame-10-coming-up-card"
        aria-labelledby="coming-up-card-title"
        className="rounded-[14px] border border-mid-gray/25 bg-surface p-4 space-y-2.5"
      >
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <div className="space-y-0.5">
            {onOpenView ? (
              <button
                type="button"
                data-testid="coming-up-open-view"
                onClick={onOpenView}
                className="text-left cursor-pointer hover:underline focus:outline-none focus:underline"
              >
                <h3
                  id="coming-up-card-title"
                  className="text-[15px] font-semibold text-text"
                >
                  {t("meetings.upcoming.title", "Coming up")}
                </h3>
              </button>
            ) : (
              <h3
                id="coming-up-card-title"
                className="text-[15px] font-semibold text-text"
              >
                {t("meetings.upcoming.title", "Coming up")}
              </h3>
            )}
            <p className="text-[12.5px] text-mid-gray">
              {t(
                "meetings.upcoming.cardSubtitle",
                "From the calendar on this Mac, once you turn it on.",
              )}
            </p>
          </div>
        </div>
        {emptyOrDeniedBody ?? (
          <ul className="divide-y divide-mid-gray/20">
            {meetings.slice(0, 3).map((m) => {
              const key = upcomingEventKey(m);
              return (
                <li
                  key={key}
                  data-calendar={m.calendar_name}
                  data-attendees={m.attendee_display_names.join(", ")}
                  className="flex items-center justify-between gap-3 py-2.5"
                >
                  <div className="min-w-0 space-y-0.5">
                    <p className="truncate text-sm font-semibold text-text">
                      {m.title}
                    </p>
                    <p className="text-xs text-mid-gray">
                      {formatCardEventMeta(m, nowSecs, i18n.language)}
                    </p>
                  </div>
                  <button
                    type="button"
                    onClick={() => onStartNotes(m)}
                    className="shrink-0 rounded-lg border border-mid-gray/35 bg-transparent px-3 py-1.5 text-xs font-semibold text-text hover:bg-mid-gray/10 cursor-pointer"
                  >
                    {t("meetings.upcoming.startNotes", "Start notes")}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </section>
    );
  }

  // Standalone Coming up view (Frame 10, B2 Amendment 5 / CX-5): unscrolled, flat rows with dividers, NO per-row card
  return (
    <section
      data-testid="frame-10-coming-up"
      aria-labelledby="coming-up-title"
      className="max-w-3xl w-full mx-auto space-y-3"
    >
      <header className="space-y-1">
        <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
          {t("meetings.sidebar", "Meetings")}
        </p>
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <h2
            id="coming-up-title"
            className="text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text"
          >
            {t("meetings.upcoming.title", "Coming up")}
          </h2>
          {showCountRatio ? (
            <span
              data-testid="coming-up-count"
              className="text-[12.5px] font-medium text-mid-gray"
            >
              {t("meetings.upcoming.countOfTotal", {
                defaultValue: "{{shown}} of {{total}}",
                shown: visibleCount,
                total: effectiveTotal,
              })}
            </span>
          ) : null}
        </div>
        <p className="text-[13px] text-mid-gray">
          {t(
            "meetings.upcoming.nothingStartsByItself",
            "Nothing starts by itself.",
          )}
        </p>
      </header>
      {emptyOrDeniedBody ?? (
        <ul
          data-testid="coming-up-flat-list"
          className="divide-y divide-mid-gray/20 border-t border-b border-mid-gray/20"
        >
          {meetings.map((m) => {
            const key = upcomingEventKey(m);
            const isStarted = startedKey === key;
            return (
              <li
                key={key}
                data-calendar={m.calendar_name}
                data-attendees={m.attendee_display_names.join(", ")}
                className="flex items-center justify-between gap-3 py-3"
              >
                <div className="min-w-0 space-y-0.5">
                  <p className="truncate text-sm font-medium text-text">
                    {m.title}
                  </p>
                  <p className="text-xs text-mid-gray">
                    {formatEventMeta(m, i18n.language)}
                  </p>
                </div>
                <div className="flex shrink-0 items-center gap-2.5">
                  <span
                    className={`inline-flex items-center gap-1 text-[12px] font-medium ${
                      isStarted ? "text-logo-primary" : "text-mid-gray"
                    }`}
                  >
                    <span aria-hidden="true">
                      {isStarted ? STARTED_GLYPH : NOT_STARTED_GLYPH}
                    </span>
                    <span>
                      {isStarted
                        ? t("meetings.upcoming.notesStarted", "Notes started")
                        : t("meetings.upcoming.notStarted", "Not started")}
                    </span>
                  </span>
                  <button
                    type="button"
                    onClick={() => onStartNotes(m)}
                    className="rounded-lg bg-logo-primary px-3 py-1.5 text-xs font-medium text-background hover:opacity-90 cursor-pointer"
                  >
                    {t("meetings.upcoming.startNotes", "Start notes")}
                  </button>
                </div>
              </li>
            );
          })}
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

/**
 * Reads the picked calendars on open; drops what it read when Meetings closes.
 * Clicking Start notes always opens the Frame 2 consent reminder first.
 */
export const UpcomingMeetings: React.FC<{
  mode?: "card" | "page";
  startedKey?: string | null;
  onOpenConsent: () => void;
  onStartNotes?: (meeting: UpcomingMeeting) => void;
  onOpenView?: () => void;
  onBack?: () => void;
  onOpenSettings?: () => void;
}> = ({
  mode = "card",
  startedKey = null,
  onOpenConsent,
  onStartNotes,
  onOpenView,
  onBack,
  onOpenSettings,
}) => {
  const { settings } = useSettings();
  const [meetings, setMeetings] = useState<UpcomingMeeting[]>([]);
  const [permissionDenied, setPermissionDenied] = useState(false);
  const picked = selectedCount(settings);

  useEffect(() => {
    let cancelled = false;
    if (picked === 0) {
      setMeetings([]);
      setPermissionDenied(false);
    } else {
      void commands.listUpcomingMeetings().then((r) => {
        if (cancelled) return;
        if (r.status === "ok") {
          setPermissionDenied(false);
          setMeetings(r.data);
        } else {
          const msg = String(r.error ?? "").toLowerCase();
          setPermissionDenied(
            msg.includes("denied") ||
              msg.includes("permission") ||
              msg.includes("access"),
          );
          setMeetings([]);
        }
      });
    }
    return () => {
      cancelled = true;
      setMeetings([]);
      void commands.closeUpcomingMeetings();
    };
  }, [picked]);

  return (
    <UpcomingMeetingsList
      meetings={meetings}
      hasCalendars={picked > 0}
      permissionDenied={permissionDenied}
      startedKey={startedKey}
      mode={mode}
      onOpenView={onOpenView}
      onBack={onBack}
      onOpenSettings={onOpenSettings}
      onStartNotes={(m) => {
        if (onStartNotes) {
          onStartNotes(m);
        } else {
          onOpenConsent();
        }
      }}
    />
  );
};
