import React from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { useSettings } from "../../hooks/useSettings";
import { SettingsGroup } from "../ui/SettingsGroup";
import { ToggleSwitch } from "../ui/ToggleSwitch";

export interface ReadFocusedTextSectionProps {
  enabled: boolean;
  windowContextEnabled: boolean;
  onToggleEnabled: (next: boolean) => void;
  onToggleWindowContext: (next: boolean) => void;
}

export const ReadFocusedTextSection: React.FC<ReadFocusedTextSectionProps> = ({
  enabled,
  windowContextEnabled,
  onToggleEnabled,
  onToggleWindowContext,
}) => {
  const { t } = useTranslation();

  return (
    <div data-testid="read-focused-text-card" className="mt-5">
      <SettingsGroup title={t("settings.focusedText.title")}>
        <ToggleSwitch
          checked={enabled}
          onChange={onToggleEnabled}
          label={t("settings.focusedText.readBoxLabel")}
          description={t("settings.focusedText.readBoxDescription")}
          descriptionMode="inline"
          grouped={true}
        />
        <div
          aria-disabled={!enabled}
          className={enabled ? "" : "opacity-50 pointer-events-none"}
        >
          <ToggleSwitch
            checked={enabled && windowContextEnabled}
            disabled={!enabled}
            onChange={onToggleWindowContext}
            label={t("settings.focusedText.windowContextLabel")}
            description={t("settings.focusedText.windowContextDescription")}
            descriptionMode="inline"
            grouped={true}
          />
        </div>
        <div
          data-testid={
            enabled ? "read-focused-text-on" : "read-focused-text-off"
          }
          className="flex flex-wrap items-center justify-between gap-2 px-4 py-2.5 border-t border-mid-gray/15"
        >
          <span className="text-xs text-mid-gray">
            {t("settings.focusedText.privacyNote")}
          </span>
          <span className="rounded-full border border-mid-gray/35 px-2.5 py-0.5 font-mono text-[11px] font-medium text-mid-gray">
            {t("settings.focusedText.offByDefaultBadge")}
          </span>
        </div>
      </SettingsGroup>
    </div>
  );
};

export const ReadFocusedTextCard: React.FC = () => {
  const { settings, updateSetting, refreshSettings } = useSettings();
  const enabled = Boolean(settings?.read_focused_text_enabled);
  const windowContextEnabled = Boolean(
    settings?.read_focused_window_context_enabled,
  );

  return (
    <ReadFocusedTextSection
      enabled={enabled}
      windowContextEnabled={windowContextEnabled}
      onToggleEnabled={(next) => {
        void updateSetting("read_focused_text_enabled" as any, next as any);
        void commands
          .setReadFocusedTextEnabled(next)
          .then(() => refreshSettings())
          .catch(() => {});
      }}
      onToggleWindowContext={(next) => {
        void updateSetting(
          "read_focused_window_context_enabled" as any,
          next as any,
        );
        void commands
          .setReadFocusedWindowContextEnabled(next)
          .then(() => refreshSettings())
          .catch(() => {});
      }}
    />
  );
};
