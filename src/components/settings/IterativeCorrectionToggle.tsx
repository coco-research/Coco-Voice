import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";

interface IterativeCorrectionToggleProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/**
 * Settings toggle for the explicit-hotkey iterative-correction feature. When
 * enabled, a dedicated "Speak Correction" hotkey (assigned separately) lets the
 * user replace their last dictation with a spoken correction. It never triggers
 * automatically from keywords — the hotkey is the sole trigger.
 */
export const IterativeCorrectionToggle: React.FC<IterativeCorrectionToggleProps> =
  React.memo(({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const enabled = getSetting("iterative_correction_enabled") || false;

    return (
      <ToggleSwitch
        checked={enabled}
        onChange={(next) => updateSetting("iterative_correction_enabled", next)}
        isUpdating={isUpdating("iterative_correction_enabled")}
        label={t("settings.postProcessing.correction.toggleLabel")}
        description={t("settings.postProcessing.correction.toggleDescription")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  });

IterativeCorrectionToggle.displayName = "IterativeCorrectionToggle";
