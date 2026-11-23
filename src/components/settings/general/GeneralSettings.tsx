import React from "react";
import { useTranslation } from "react-i18next";
import { type } from "@tauri-apps/plugin-os";
import { MicrophoneSelector } from "../MicrophoneSelector";
import { ClamshellMicrophoneSelector } from "../ClamshellMicrophoneSelector";
import { AgentsCanAskCard } from "../AgentsCanAskCard";
import { ReadFocusedTextCard } from "../ReadFocusedTextCard";
import { ShortcutInput } from "../ShortcutInput";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { OutputDeviceSelector } from "../OutputDeviceSelector";
import { PushToTalk } from "../PushToTalk";
import { AudioFeedback } from "../AudioFeedback";
import { useSettings } from "../../../hooks/useSettings";
import { VolumeSlider } from "../VolumeSlider";
import { MuteWhileRecording } from "../MuteWhileRecording";
import { ModelSettingsCard } from "./ModelSettingsCard";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { TrayMenuSurface } from "../../whats-new/UpdateAndTraySurfaces";

export const GeneralSettings: React.FC = () => {
  const { t } = useTranslation();
  const { audioFeedbackEnabled, getSetting, updateSetting, settings } =
    useSettings();
  const handsFree = Boolean(
    settings?.double_tap_hands_free ?? settings?.hands_free_enabled,
  );
  const pillPos = settings?.overlay_position === "top" ? "top" : "bottom";
  const pushToTalk = getSetting("push_to_talk");
  const isLinux = type() === "linux";
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.general.title")}>
        <ShortcutInput shortcutId="transcribe" grouped={true} />
        <PushToTalk descriptionMode="tooltip" grouped={true} />
        {/* Cancel shortcut is hidden with push-to-talk (release key cancels) and on Linux (dynamic shortcut instability) */}
        {!isLinux && !pushToTalk && (
          <ShortcutInput shortcutId="cancel" grouped={true} />
        )}
        <ToggleSwitch
          checked={handsFree}
          onChange={(next) => void updateSetting("double_tap_hands_free", next)}
          label={t("v022.settings.handsFreeTitle", "Hands-free")}
          description={t(
            "v022.settings.handsFreeGeneralDesc",
            "Double-tap your shortcut to keep listening without holding the key.",
          )}
          descriptionMode="inline"
          grouped={true}
        />
        <div className="flex items-center justify-between gap-4 px-4 py-3 border-t border-mid-gray/15">
          <div>
            <p className="text-sm font-medium text-text">
              {t("v022.settings.wherePillShows", "Where the pill shows")}
            </p>
            <p className="text-xs text-mid-gray">
              {t(
                "v022.settings.wherePillShowsGeneralDesc",
                "Choose top or bottom of your active screen.",
              )}
            </p>
          </div>
          <select
            aria-label={t(
              "v022.settings.wherePillShows",
              "Where the pill shows",
            )}
            value={pillPos}
            onChange={(e) =>
              void updateSetting(
                "overlay_position",
                e.target.value as "top" | "bottom",
              )
            }
            className="rounded-lg border border-mid-gray/35 bg-background px-2.5 py-1 text-xs font-medium text-text"
          >
            <option value="bottom">
              {t("v022.settings.pillBottomTitle", "Bottom of the screen")}
            </option>
            <option value="top">
              {t("v022.settings.pillTopTitle", "Top of the screen")}
            </option>
          </select>
        </div>
      </SettingsGroup>
      <ModelSettingsCard />
      <SettingsGroup title={t("settings.sound.title")}>
        <MicrophoneSelector descriptionMode="tooltip" grouped={true} />
        <ClamshellMicrophoneSelector descriptionMode="tooltip" grouped={true} />
        <MuteWhileRecording descriptionMode="tooltip" grouped={true} />
        <AudioFeedback descriptionMode="tooltip" grouped={true} />
        <OutputDeviceSelector
          descriptionMode="tooltip"
          grouped={true}
          disabled={!audioFeedbackEnabled}
        />
        <VolumeSlider disabled={!audioFeedbackEnabled} />
      </SettingsGroup>
      <SettingsGroup
        title={t(
          "v022.settings.trayPasteLastTitle",
          "Menu bar · Paste last take",
        )}
      >
        <div className="p-4 flex flex-wrap items-center justify-between gap-4">
          <div className="space-y-1 max-w-sm">
            <p className="text-sm font-medium text-text">
              {t(
                "v022.settings.trayPasteLastHeading",
                "Paste last take from the menu bar",
              )}
            </p>
            <p className="text-xs text-mid-gray">
              {t(
                "v022.settings.trayPasteLastDesc",
                "If focus moved while you spoke, click the menu bar orb and choose Paste last take.",
              )}
            </p>
          </div>
          <TrayMenuSurface hasLastTake={true} />
        </div>
      </SettingsGroup>
      <AgentsCanAskCard />
      <ReadFocusedTextCard />
    </div>
  );
};
