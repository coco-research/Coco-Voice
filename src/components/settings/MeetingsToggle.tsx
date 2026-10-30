import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";
import { useSettingsStore } from "@/stores/settingsStore";
import type { AppSettings as Settings } from "@/bindings";
import { commands } from "../../bindings.meetings";

interface MeetingsToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

function meetingsEnabled(settings: unknown): boolean {
  return (
    typeof settings === "object" &&
    settings !== null &&
    "meetings_enabled" in settings &&
    settings.meetings_enabled === true
  );
}

async function setMeetingsEnabled(enabled: boolean): Promise<void> {
  const { settings, setUpdating, setSettings } = useSettingsStore.getState();
  const previous = meetingsEnabled(settings);
  setUpdating("meetings_enabled", true);
  if (settings) {
    setSettings({ ...settings, meetings_enabled: enabled } as Settings);
  }
  try {
    const result = await commands.changeMeetingsEnabledSetting(enabled);
    if (result.status === "error") {
      throw new Error(String(result.error));
    }
  } catch (error) {
    console.error("Failed to update meetings_enabled", error);
    const current = useSettingsStore.getState().settings;
    if (current) {
      useSettingsStore
        .getState()
        .setSettings({ ...current, meetings_enabled: previous } as Settings);
    }
  } finally {
    setUpdating("meetings_enabled", false);
  }
}

export const MeetingsToggle: React.FC<MeetingsToggleProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
}) => {
  const { t } = useTranslation();
  const { settings, isUpdating } = useSettings();

  return (
    <ToggleSwitch
      checked={meetingsEnabled(settings)}
      onChange={(enabled) => {
        void setMeetingsEnabled(enabled);
      }}
      isUpdating={isUpdating("meetings_enabled")}
      label={t("meetings.toggle.label")}
      description={t("meetings.toggle.description")}
      descriptionMode={descriptionMode}
      grouped={grouped}
    />
  );
};
