import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type CalendarChoice } from "../../bindings.meetings";
import { useSettings } from "../../hooks/useSettings";

interface CalendarPickerViewProps {
  choices: CalendarChoice[];
  selected: string[];
  onToggle: (id: string, on: boolean) => void;
  onMarkAsWork?: (id: string) => void;
}

export const CALENDAR_PERMISSION_USAGE_COPY =
  "Coco reads your calendar on this Mac to show upcoming meetings. It never changes your calendar or sends it anywhere.";

/** Rows for Settings > Meetings. Nothing is checked unless the person checked it. */
export const CalendarPickerView: React.FC<CalendarPickerViewProps> = ({
  choices,
  selected,
  onToggle,
  onMarkAsWork,
}) => {
  const { t } = useTranslation();
  const [markingWork, setMarkingWork] = useState(false);
  const unblockedChoices = choices.filter((c) => c.blocked === null);

  return (
    <div
      data-testid="frame-calendars-picker"
      className="px-4 py-3 border-t border-mid-gray/15 space-y-3"
    >
      <fieldset className="space-y-2">
        <legend className="text-sm font-medium text-text">
          {t("meetings.calendar.title", "Calendars for Coming up")}
        </legend>
        <p className="text-xs text-mid-gray">
          {selected.length === 0
            ? t(
                "meetings.calendar.noneChosen",
                "None chosen yet. Pick the calendars Coming up may read.",
              )
            : t("meetings.calendar.chosenCount", {
                defaultValue: "{{count}} chosen.",
                count: selected.length,
              })}
        </p>
        {choices.length === 0 ? (
          <p className="text-xs text-mid-gray">
            {t(
              "meetings.calendar.empty",
              "No test calendars available yet on this Mac.",
            )}
          </p>
        ) : (
          <ul className="space-y-1.5">
            {choices.map((c) => {
              const blocked = c.blocked !== null;
              const checked = !blocked && selected.includes(c.id);
              const reasonId = `calendar-reason-${c.id}`;
              return (
                <li
                  key={c.id}
                  className={`flex items-center justify-between gap-3 py-1 ${blocked ? "opacity-55" : ""}`}
                >
                  <div className="min-w-0">
                    <div className="text-sm text-text">{c.title}</div>
                    <div id={reasonId} className="text-xs text-mid-gray">
                      {blocked
                        ? t(
                            "meetings.calendar.blockedWorkMarked",
                            "You marked this as work: never read",
                          )
                        : c.account}
                    </div>
                  </div>
                  <label
                    className={`inline-flex shrink-0 items-center ${blocked ? "cursor-not-allowed" : "cursor-pointer"}`}
                  >
                    <input
                      type="checkbox"
                      role="switch"
                      className="sr-only peer"
                      checked={checked}
                      disabled={blocked}
                      aria-label={`${t("meetings.calendar.readAria", "Read calendar")} ${c.title}`}
                      aria-describedby={blocked ? reasonId : undefined}
                      onChange={(e) => onToggle(c.id, e.target.checked)}
                    />
                    <div className="relative h-6 w-11 rounded-full bg-mid-gray/25 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-logo-primary peer-checked:bg-background-ui peer-disabled:opacity-50 after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:h-5 after:w-5 after:rounded-full after:border after:border-gray-300 after:bg-white after:transition-all peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white" />
                  </label>
                </li>
              );
            })}
          </ul>
        )}
      </fieldset>

      {onMarkAsWork && unblockedChoices.length > 0 ? (
        <div className="space-y-1.5">
          <button
            type="button"
            onClick={() => setMarkingWork((v) => !v)}
            className="rounded-lg border border-mid-gray/30 px-2.5 py-1 text-xs font-medium text-text hover:bg-mid-gray/10 cursor-pointer"
          >
            {t("meetings.calendar.markWorkButton", "Mark a calendar as work")}
          </button>
          {markingWork ? (
            <div className="flex flex-wrap gap-1.5">
              {unblockedChoices.map((c) => (
                <button
                  key={c.id}
                  type="button"
                  onClick={() => {
                    onMarkAsWork(c.id);
                    setMarkingWork(false);
                  }}
                  className="rounded border border-mid-gray/30 px-2 py-0.5 text-xs text-mid-gray hover:text-text cursor-pointer"
                >
                  {c.title}
                </button>
              ))}
            </div>
          ) : null}
        </div>
      ) : null}

      <p className="text-xs text-mid-gray">
        {t(
          "meetings.calendar.retentionNote",
          "Only the title, start, end, calendar name and attendee names are kept, on this Mac, and only while Meetings is open unless you start notes. Coco never changes your calendar.",
        )}
      </p>
      <div className="rounded-lg border border-mid-gray/20 p-2.5 space-y-1">
        <p className="text-xs font-medium text-text">
          {t("meetings.calendar.promptHeading", "The macOS permission prompt")}
        </p>
        <p className="text-xs text-mid-gray">
          {`"${CALENDAR_PERMISSION_USAGE_COPY}"`}
        </p>
      </div>
      <p className="text-xs text-mid-gray">
        {t(
          "meetings.calendar.consentNote",
          "Start notes from Coming up always shows the consent reminder first.",
        )}
      </p>
    </div>
  );
};

function selectedFrom(settings: unknown): string[] {
  const v = (settings as { selected_calendars?: unknown } | null)
    ?.selected_calendars;
  return Array.isArray(v)
    ? v.filter((x): x is string => typeof x === "string")
    : [];
}

export const CalendarPicker: React.FC = () => {
  const { settings, refreshSettings } = useSettings();
  const [choices, setChoices] = useState<CalendarChoice[]>([]);
  const selected = selectedFrom(settings);

  const loadChoices = () => {
    void commands.listCalendarChoices().then((r) => {
      setChoices(r.status === "ok" ? r.data : []);
    });
  };

  useEffect(() => {
    loadChoices();
  }, []);

  return (
    <CalendarPickerView
      choices={choices}
      selected={selected}
      onToggle={(id, on) => {
        const next = on
          ? [...selected.filter((x) => x !== id), id]
          : selected.filter((x) => x !== id);
        void commands
          .changeSelectedCalendarsSetting(next)
          .then(() => refreshSettings());
      }}
      onMarkAsWork={(id) => {
        void commands.markCalendarAsWorkSetting(id).then(() => {
          loadChoices();
          void refreshSettings();
        });
      }}
    />
  );
};
