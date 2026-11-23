import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { useSettings } from "../../../hooks/useSettings";
import { useUpdateStore } from "../../../stores/updateStore";
import type { ShellSectionId } from "../../layout/shellNavigation";
import type { SettingsTarget } from "../SettingsV022Page";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Button } from "../../ui/Button";
import { AppDataDirectory } from "../AppDataDirectory";
import { AppLanguageSelector } from "../AppLanguageSelector";
import { ShowWhatsNewOnUpdate } from "../ShowWhatsNewOnUpdate";
import { UpdateChecksToggle } from "../UpdateChecksToggle";
import { LogDirectory } from "../debug";
import { WhatsNewModal } from "../../whats-new/WhatsNewModal";
import {
  findLatestReleaseNote,
  type ReleaseNote,
} from "../../whats-new/releaseNotes";
import { WhatGoesOnline, WhatGoesOnlineSummaryCard } from "./WhatGoesOnline";
import { SPEAKER_SPLIT_ENABLED } from "../../meetings/gate";

export type ManualUpdateCheckResult =
  | { kind: "up_to_date" }
  | { kind: "update_ready"; version: string; update?: Update | null }
  | { kind: "error"; message?: string };

export async function runManualUpdateCheck(): Promise<ManualUpdateCheckResult> {
  try {
    const update = await check();
    if (update) {
      return {
        kind: "update_ready",
        version: update.version,
        update,
      };
    }
    return { kind: "up_to_date" };
  } catch (error) {
    return {
      kind: "error",
      message: error instanceof Error ? error.message : String(error),
    };
  }
}

export const AboutSettings: React.FC<{
  onNavigate?: (section: ShellSectionId, target?: SettingsTarget) => void;
  checkForUpdate?: () => Promise<ManualUpdateCheckResult>;
}> = ({ onNavigate, checkForUpdate = runManualUpdateCheck }) => {
  const { t } = useTranslation();
  const { getSetting } = useSettings();
  const [version, setVersion] = useState("");
  const [whatsNewOpen, setWhatsNewOpen] = useState(false);
  const [updateCheckBusy, setUpdateCheckBusy] = useState(false);
  const [updateStatusNote, setUpdateStatusNote] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    const fetchVersion = async () => {
      try {
        const appVersion = await getVersion();
        if (active && appVersion) setVersion(appVersion);
      } catch {
        if (active) setVersion("");
      }
    };

    void fetchVersion();
    return () => {
      active = false;
    };
  }, []);

  const displayVersion = version;
  const updateChecksEnabled = getSetting("update_checks_enabled") ?? true;

  const whatsNewNote: ReleaseNote = useMemo(() => {
    const latest = findLatestReleaseNote();
    if (latest) return latest;
    return {
      version: displayVersion || "",
      markdown: "",
    };
  }, [displayVersion]);

  const handleCheckForUpdates = async () => {
    if (updateCheckBusy) return;
    setUpdateCheckBusy(true);
    setUpdateStatusNote(t("v022.about.checkingUpdates", "Checking…"));
    try {
      const result = await checkForUpdate();
      if (result.kind === "up_to_date") {
        setUpdateStatusNote(
          t("v022.about.upToDateNote", "Coco Voice is up to date"),
        );
      } else if (result.kind === "update_ready") {
        if (result.update) {
          useUpdateStore.getState().setReady(result.version, result.update);
        }
        setUpdateStatusNote(
          t("v022.about.updateReadyNote", {
            version: result.version,
            defaultValue: `Update ready: ${result.version}`,
          }),
        );
      } else {
        setUpdateStatusNote(
          t(
            "v022.about.checkUpdatesFailed",
            "Couldn't check. Try again.",
          ),
        );
      }
    } catch {
      setUpdateStatusNote(
        t("v022.about.checkUpdatesFailed", "Couldn't check. Try again."),
      );
    } finally {
      setUpdateCheckBusy(false);
    }
  };

  return (
    <div data-route-domain="shared" className="max-w-3xl w-full mx-auto">
      <div className="space-y-3">
        <div>
          <p className="text-[11px] font-bold uppercase tracking-[0.06em] text-logo-primary">
            {t("sidebar.about")}
          </p>
          <h1 className="mt-0.5 text-[22px] leading-[28px] font-semibold tracking-[-0.02em] text-text">
            {t("v022.about.heading", {
              version: displayVersion,
              defaultValue: "Coco Voice {{version}}",
            }).trim()}
          </h1>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={() => setWhatsNewOpen(true)}
            className="rounded-lg border border-mid-gray/45 bg-surface px-3 py-1.5 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
          >
            {t("v022.about.whatsNewButton", "What's new")}
          </button>
          <button
            type="button"
            disabled={updateCheckBusy}
            onClick={() => void handleCheckForUpdates()}
            className="rounded-lg border border-mid-gray/45 bg-surface px-3 py-1.5 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10 disabled:opacity-50"
          >
            {t("v022.about.checkForUpdatesButton", "Check for updates")}
          </button>
          <button
            type="button"
            onClick={() =>
              void openUrl("https://github.com/coco-research/Coco-Voice")
            }
            className="rounded-lg border border-mid-gray/45 bg-surface px-3 py-1.5 text-xs font-semibold text-text cursor-pointer hover:bg-mid-gray/10"
          >
            {t("v022.about.sourceCodeButton", "Source code")}
          </button>
          {updateStatusNote ? (
            <span
              role="status"
              data-testid="about-update-status"
              className="text-xs text-mid-gray"
            >
              {updateStatusNote}
            </span>
          ) : null}
        </div>

        <WhatGoesOnlineSummaryCard
          updateChecksEnabled={updateChecksEnabled}
          onOpenSettings={
            onNavigate ? () => onNavigate("settings", "updates") : undefined
          }
        />

        <WhatGoesOnline />
      </div>

      <WhatsNewModal
        note={whatsNewNote}
        open={whatsNewOpen}
        onDismiss={() => setWhatsNewOpen(false)}
      />

      <div className="mt-16 space-y-6 pb-8">
        <SettingsGroup title={t("settings.about.acknowledgments.title")}>
          <SettingContainer
            title={t("settings.about.acknowledgments.ggml.title")}
            description={t("settings.about.acknowledgments.ggml.description")}
            grouped={true}
            layout="stacked"
          >
            <div className="text-sm text-mid-gray">
              {t("settings.about.acknowledgments.ggml.details")}
            </div>
          </SettingContainer>
          {(SPEAKER_SPLIT_ENABLED
            ? [
                {
                  id: "wespeaker",
                  title: "WeSpeaker (wespeaker_en_voxceleb_resnet34.onnx)",
                  license: "CC BY 4.0",
                  authors:
                    "Hongji Wang, Chengdong Liang, Binbin Zhang, Zhengyang Chen, Yanmin Qian, Kong Aik Lee, and WeSpeaker contributors",
                  url: "https://github.com/wenet-e2e/wespeaker",
                  details: t(
                    "settings.about.acknowledgments.wespeakerDetails",
                    "Pretrained speaker embedding checkpoint trained on VoxCeleb under CC BY 4.0 (https://creativecommons.org/licenses/by/4.0/) from WeSpeaker (https://github.com/wenet-e2e/wespeaker), converted to ONNX by k2-fsa/sherpa-onnx.",
                  ),
                },
                {
                  id: "pyannote",
                  title: "pyannote-segmentation-3.0",
                  license: "MIT, Copyright (c) 2022 CNRS",
                  authors: "Hervé Bredin and pyannote.audio contributors",
                  url: "https://huggingface.co/pyannote/segmentation-3.0",
                  details: t(
                    "settings.about.acknowledgments.pyannoteDetails",
                    "On-device speaker turn segmentation model licensed under MIT, Copyright (c) 2022 CNRS (https://huggingface.co/pyannote/segmentation-3.0), packaged as ONNX by k2-fsa/sherpa-onnx.",
                  ),
                },
              ]
            : []
          ).map((credit) => (
            <SettingContainer
              key={credit.id}
              title={`${credit.title} (${credit.license})`}
              description={credit.authors}
              grouped={true}
              layout="stacked"
            >
              <div className="text-sm text-mid-gray flex items-center justify-between gap-3">
                <span>{credit.details}</span>
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => void openUrl(credit.url)}
                >
                  {t("settings.about.sourceCode.button")}
                </Button>
              </div>
            </SettingContainer>
          ))}
        </SettingsGroup>

        <SettingsGroup title={t("settings.about.title")}>
          <AppLanguageSelector descriptionMode="tooltip" grouped={true} />
          <UpdateChecksToggle descriptionMode="tooltip" grouped={true} />
          <ShowWhatsNewOnUpdate descriptionMode="tooltip" grouped={true} />
          <AppDataDirectory descriptionMode="tooltip" grouped={true} />
          <LogDirectory grouped={true} />
        </SettingsGroup>
      </div>
    </div>
  );
};
