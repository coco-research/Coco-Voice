import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type CalendarChoice } from "../../bindings.meetings";
import { useSettings } from "../../hooks/useSettings";

interface CalendarPickerViewProps {
  choices: CalendarChoice[];
  selected: string[];
  onToggle: (id: string, on: boolean) => void;
}

/** Rows for Settings > Meetings. Nothing is checked unless the person checked it. */
export const CalendarPickerView: React.FC<CalendarPickerViewProps> = ({
  choices,
  selected,
  onToggle,
}) => {
  const { t } = useTranslation();
  return (
    <fieldset className="space-y-2">
      <legend className="text-sm font-medium">
        {t("meetings.calendar.title")}
      </legend>
      <p className="text-xs text-text/60">
        {t("meetings.calendar.description")}
      </p>
      {choices.length === 0 ? (
        <p className="text-sm text-text/60">{t("meetings.calendar.empty")}</p>
      ) : (
        <ul className="space-y-1">
          {choices.map((c) => {
            const blocked = c.blocked !== null;
            const reasonId = `calendar-reason-${c.id}`;
            return (
              <li key={c.id} className="flex flex-col">
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={!blocked && selected.includes(c.id)}
                    disabled={blocked}
                    aria-describedby={blocked ? reasonId : undefined}
                    onChange={(e) => onToggle(c.id, e.target.checked)}
                  />
                  <span>{c.title}</span>
                  <span className="text-xs text-text/60">{c.account}</span>
                </label>
                {blocked ? (
                  <span id={reasonId} className="ml-6 text-xs text-text/60">
                    {t("meetings.calendar.blockedWork")}
                  </span>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
    </fieldset>
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

  useEffect(() => {
    void commands.listCalendarChoices().then((r) => {
      setChoices(r.status === "ok" ? r.data : []);
    });
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
    />
  );
};
