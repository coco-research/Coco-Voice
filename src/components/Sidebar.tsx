import React from "react";
import { useTranslation } from "react-i18next";
import { Cog, FlaskConical, History, Info, Sparkles, Cpu } from "lucide-react";
import CocoVoiceTextLogo from "./icons/CocoVoiceTextLogo";
import CocoVoiceIcon from "./icons/CocoVoiceIcon";
import { useSettings } from "../hooks/useSettings";
import { useMeetingsUi } from "./meetings/gate";
import {
  GeneralSettings,
  AdvancedSettings,
  HistorySettings,
  DebugSettings,
  AboutSettings,
  PostProcessingSettings,
  ModelsSettings,
} from "./settings";

export type SidebarSection = keyof typeof SECTIONS_CONFIG | "meetings";

interface IconProps {
  width?: number | string;
  height?: number | string;
  size?: number | string;
  className?: string;
  [key: string]: any;
}

interface SectionConfig {
  labelKey: string;
  icon: React.ComponentType<IconProps>;
  component: React.ComponentType;
  enabled: (settings: any) => boolean;
}

export const SECTIONS_CONFIG = {
  general: {
    labelKey: "sidebar.general",
    icon: CocoVoiceIcon,
    component: GeneralSettings,
    enabled: () => true,
  },
  models: {
    labelKey: "sidebar.models",
    icon: Cpu,
    component: ModelsSettings,
    enabled: () => true,
  },
  advanced: {
    labelKey: "sidebar.advanced",
    icon: Cog,
    component: AdvancedSettings,
    enabled: () => true,
  },
  history: {
    labelKey: "sidebar.history",
    icon: History,
    component: HistorySettings,
    enabled: () => true,
  },
  postprocessing: {
    labelKey: "sidebar.postProcessing",
    icon: Sparkles,
    component: PostProcessingSettings,
    enabled: (settings) => settings?.post_process_enabled ?? false,
  },
  debug: {
    labelKey: "sidebar.debug",
    icon: FlaskConical,
    component: DebugSettings,
    enabled: (settings) => settings?.debug_mode ?? false,
  },
  about: {
    labelKey: "sidebar.about",
    icon: Info,
    component: AboutSettings,
    enabled: () => true,
  },
} as const satisfies Record<string, SectionConfig>;

interface SidebarProps {
  activeSection: SidebarSection;
  onSectionChange: (section: SidebarSection) => void;
}

export const Sidebar: React.FC<SidebarProps> = ({
  activeSection,
  onSectionChange,
}) => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const meetingsUi = useMeetingsUi();
  const showMeetings =
    meetingsUi !== null && meetingsUi.sidebarVisible(settings);

  const availableSections = Object.entries(SECTIONS_CONFIG)
    .filter(([_, config]) => config.enabled(settings))
    .map(([id, config]) => ({ id: id as SidebarSection, ...config }));
  const aboutIndex = availableSections.findIndex(
    (section) => section.id === "about",
  );
  const sectionsBeforeAbout =
    aboutIndex === -1
      ? availableSections
      : availableSections.slice(0, aboutIndex);
  const sectionsAfterAbout =
    aboutIndex === -1 ? [] : availableSections.slice(aboutIndex);

  const renderSection = (section: {
    id: SidebarSection;
    labelKey: string;
    icon: React.ComponentType<{
      width?: number | string;
      height?: number | string;
      className?: string;
    }>;
  }) => {
    const Icon = section.icon;
    const isActive = activeSection === section.id;
    return (
      <div
        key={section.id}
        className={`flex gap-2 items-center p-2 w-full rounded-lg cursor-pointer transition-colors ${
          isActive
            ? "bg-logo-primary/80"
            : "hover:bg-mid-gray/20 hover:opacity-100 opacity-85"
        }`}
        onClick={() => onSectionChange(section.id)}
      >
        <Icon width={24} height={24} className="shrink-0" />
        <p className="text-sm font-medium truncate" title={t(section.labelKey)}>
          {t(section.labelKey)}
        </p>
      </div>
    );
  };

  return (
    <div className="flex flex-col w-40 h-full border-e border-mid-gray/20 items-center px-2">
      <CocoVoiceTextLogo width={120} className="m-4" />
      <div className="flex flex-col w-full items-center gap-1 pt-2 border-t border-mid-gray/20">
        {sectionsBeforeAbout.map(renderSection)}
        {showMeetings && meetingsUi
          ? renderSection({
              id: "meetings",
              labelKey: "meetings.sidebar",
              icon: meetingsUi.icon,
            })
          : null}
        {sectionsAfterAbout.map(renderSection)}
      </div>
    </div>
  );
};
