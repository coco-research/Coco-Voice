import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { SettingContainer } from "../../ui/SettingContainer";
import { Button } from "../../ui/Button";
import { AppDataDirectory } from "../AppDataDirectory";
import { AppLanguageSelector } from "../AppLanguageSelector";
import { ShowWhatsNewOnUpdate } from "../ShowWhatsNewOnUpdate";
import { ThemeSelector } from "../ThemeSelector";
import { LogDirectory } from "../debug";

export const AboutSettings: React.FC = () => {
  const { t } = useTranslation();
  const [version, setVersion] = useState("");

  useEffect(() => {
    const fetchVersion = async () => {
      try {
        const appVersion = await getVersion();
        setVersion(appVersion);
      } catch (error) {
        console.error("Failed to get app version:", error);
        setVersion("0.1.2");
      }
    };

    fetchVersion();
  }, []);

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.about.title")}>
        <AppLanguageSelector descriptionMode="tooltip" grouped={true} />
        <ThemeSelector descriptionMode="tooltip" grouped={true} />
        <SettingContainer
          title={t("settings.about.version.title")}
          description={t("settings.about.version.description")}
          grouped={true}
        >
          {/* eslint-disable-next-line i18next/no-literal-string */}
          <span className="text-sm font-mono">v{version}</span>
        </SettingContainer>

        <ShowWhatsNewOnUpdate descriptionMode="tooltip" grouped={true} />

        <SettingContainer
          title={t("settings.about.sourceCode.title")}
          description={t("settings.about.sourceCode.description")}
          grouped={true}
        >
          <Button
            variant="secondary"
            size="md"
            onClick={() =>
              openUrl("https://github.com/coco-research/Coco-Voice")
            }
          >
            {t("settings.about.sourceCode.button")}
          </Button>
        </SettingContainer>

        <AppDataDirectory descriptionMode="tooltip" grouped={true} />
        <LogDirectory grouped={true} />
      </SettingsGroup>

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
        {[
          {
            id: "wespeaker",
            title: "WeSpeaker (wespeaker_en_voxceleb_resnet34.onnx)",
            license: "CC-BY-4.0",
            authors:
              "Hongji Wang, Chengdong Liang, Binbin Zhang, Zhengyang Chen, Yanmin Qian, Kong Aik Lee, and WeSpeaker contributors",
            url: "https://github.com/wenet-e2e/wespeaker",
            details:
              "Speaker embedding model for per-meeting grouping (https://github.com/wenet-e2e/wespeaker).",
          },
          {
            id: "pyannote",
            title: "pyannote-segmentation-3.0",
            license: "MIT",
            authors: "Hervé Bredin and pyannote.audio contributors",
            url: "https://huggingface.co/pyannote/segmentation-3.0",
            details:
              "On-device speaker turn segmentation model (https://huggingface.co/pyannote/segmentation-3.0).",
          },
        ].map((credit) => (
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
                onClick={() => openUrl(credit.url)}
              >
                {credit.license}
              </Button>
            </div>
          </SettingContainer>
        ))}
      </SettingsGroup>
    </div>
  );
};
